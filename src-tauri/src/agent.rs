//! Native collector. This branch runs before Tauri initialization and owns no WebView.
use std::{collections::{HashMap, HashSet}, ffi::c_void, mem::{size_of, zeroed},
    path::{Path, PathBuf}, ptr::{null, null_mut}, sync::{Mutex,atomic::{AtomicBool, AtomicIsize, Ordering}},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use chrono::Local;
use crate::{store::{Segment, Settings, Store}, tracker::{Identity, InputClock, Sample, Tracker}};
use windows_sys::Win32::{
    Foundation::*,
    System::{LibraryLoader::GetModuleHandleW, ProcessStatus::*, RemoteDesktop::*, SystemInformation::*, Threading::*, WindowsProgramming::QueryUnbiasedInterruptTime},
    UI::{Accessibility::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const CLASS: &str = "TimeWrapped.Agent.v1";
const MSG_FOREGROUND: u32 = WM_APP + 10;
const MSG_SETTINGS: u32 = WM_APP + 11;
const MSG_FLUSH: u32 = WM_APP + 12;
const MSG_QUIT: u32 = WM_APP + 13;
const MSG_TRAY: u32 = WM_APP + 14;
const TIMER: usize = 1;
const SCENE_TIMER:usize=2;
const MSG_SCENE:u32=WM_APP+16;
const MSG_PROTOCOL:u32=WM_APP+15;
const MSG_MEDIA:u32=WM_APP+17;
static SCENE_QUEUED:AtomicBool=AtomicBool::new(false);
const CMD_OPEN: usize = 100;
const CMD_PAUSE: usize = 101;
const CMD_QUIT: usize = 102;
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static INPUT_CLOCK: Mutex<InputClock> = Mutex::new(InputClock::new());
struct OwnedIcon(HICON);
impl Drop for OwnedIcon {
    fn drop(&mut self) {if !self.0.is_null() {unsafe {DestroyIcon(self.0);}}}
}

fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }
fn window_name() -> Vec<u16> {
    // The explicit data directory also isolates IPC and mutexes in automated tests.
    let suffix = std::env::var("TIMEWRAPPED_DATA_DIR").unwrap_or_default();
    wide(&format!("TimeWrapped collector {suffix}"))
}
fn find_window() -> HWND { unsafe { FindWindowW(wide(CLASS).as_ptr(), window_name().as_ptr()) } }
fn win_error(context: &str) -> String { format!("{context}: {}", std::io::Error::last_os_error()) }

pub fn ensure_running() -> Result<(), String> {
    let existing=find_window();
    if !existing.is_null() {
        let mut protocol=0;
        unsafe{SendMessageTimeoutW(existing,MSG_PROTOCOL,0,0,SMTO_ABORTIFHUNG|SMTO_BLOCK,2000,&mut protocol);}
        if protocol==5{return Ok(());}
        // An earlier collector must commit/exit before the new schema is opened.
        shutdown()?;
    }
    use std::os::windows::process::CommandExt;
    std::process::Command::new(std::env::current_exe().map_err(|e|e.to_string())?)
        .arg("--background").creation_flags(0x08000000).spawn().map_err(|e|e.to_string())?;
    let deadline=Instant::now()+Duration::from_secs(30);
    while Instant::now()<deadline {
        if !find_window().is_null() { return Ok(()); }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err("Фоновый сборщик не запустился. Проверьте agent-error.log в папке данных.".into())
}

pub fn notify_settings_changed()->Result<(),String> {
    let window=find_window();if window.is_null(){return Err("Фоновый сборщик не запущен".into());}
    let mut result=0;
    let sent=unsafe{SendMessageTimeoutW(window,MSG_SETTINGS,0,0,SMTO_ABORTIFHUNG|SMTO_BLOCK,5000,&mut result)};
    if sent==0 || result!=1 {return Err("Не удалось применить настройки сборщика".into());}Ok(())
}
pub fn shutdown()->Result<(),String> {
    let window=find_window();if window.is_null(){return Ok(());}
    let mut result=0;
    let sent=unsafe{SendMessageTimeoutW(window,MSG_QUIT,0,0,SMTO_ABORTIFHUNG|SMTO_BLOCK,10_000,&mut result)};
    if sent==0{return Err(win_error("Сборщик не подтвердил выход"));}
    let deadline=Instant::now()+Duration::from_secs(5);
    while !find_window().is_null() && Instant::now()<deadline {std::thread::sleep(Duration::from_millis(20));}
    if !find_window().is_null(){return Err("Сборщик не завершился: статистика не сохранена".into());}Ok(())
}
pub fn flush() -> Result<(), String> {
    let window=find_window();
    if window.is_null() { return Err("Фоновый сборщик не запущен".into()); }
    let mut result=0usize;
    let sent=unsafe { SendMessageTimeoutW(window,MSG_FLUSH,0,0,SMTO_ABORTIFHUNG|SMTO_BLOCK,5_000,&mut result) };
    if sent==0 { return Err(win_error("Сборщик не ответил на запрос сохранения")); }
    if result!=1 { return Err("Не удалось сохранить текущую статистику. Подробности в agent-error.log.".into()); }
    Ok(())
}

fn sample() -> Sample {
    unsafe {
        let uptime=GetTickCount64();
        let mut awake=0;
        let awake=if QueryUnbiasedInterruptTime(&mut awake)!=0 {awake/10_000}else{uptime};
        let mut info=LASTINPUTINFO { cbSize:size_of::<LASTINPUTINFO>() as u32, dwTime:0 };
        // Failure is conservative: consider input idle, never assume activity.
        let idle=if GetLastInputInfo(&mut info)!=0 {
            INPUT_CLOCK.lock().map(|mut clock|clock.idle_ms(uptime,info.dwTime)).unwrap_or(u64::MAX)
        } else { u64::MAX };
        Sample { wall_ms:SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64,
            uptime_ms:uptime,awake_ms:awake,idle_ms:idle }
    }
}

fn app_pid(hwnd:HWND)->u32 {
    unsafe{
        let mut class=[0u16;128];let n=GetClassNameW(hwnd,class.as_mut_ptr(),class.len() as i32);
        let class=String::from_utf16_lossy(&class[..n.max(0) as usize]);
        if ["Progman","WorkerW","Shell_TrayWnd","Shell_SecondaryTrayWnd"].contains(&class.as_str()){return 0;}
        let mut target=hwnd;
        if class=="ApplicationFrameWindow" {
            let child=FindWindowExW(hwnd,null_mut(),wide("Windows.UI.Core.CoreWindow").as_ptr(),null());
            if !child.is_null(){target=child;}
        }
        let mut pid=0;GetWindowThreadProcessId(target,&mut pid);pid
    }
}
struct ProcessCache {
    values:HashMap<u32,(String,Identity)>,
}
impl ProcessCache {
    fn new()->Self { Self { values:HashMap::new() } }
    fn identify(&mut self, hwnd:HWND)->Option<Identity> {
        if hwnd.is_null() { return None; }
        unsafe {
            let pid=app_pid(hwnd);
            if pid==0{return None;}
            // Query just the foreground process. Recheck its executable to avoid
            // PID reuse; cached friendly metadata avoids repeated version reads.
            let process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid);
            if process.is_null() { return None; }
            let mut buffer=vec![0u16;32_768]; let mut len=buffer.len() as u32;
            let ok=QueryFullProcessImageNameW(process,0,buffer.as_mut_ptr(),&mut len);
            CloseHandle(process); if ok==0 { return None; }
            let path=String::from_utf16_lossy(&buffer[..len as usize]);
            if let Some((old,app))=self.values.get(&pid) { if old==&path { return Some(app.clone()); } }
            let exe=Path::new(&path).file_name()?.to_string_lossy().to_string();
            let id=exe.to_lowercase();
            // Lock and Windows shell secure surfaces are not application use.
            if ["lockapp.exe","logonui.exe","dwm.exe"].contains(&id.as_str()) { return None; }
            let app=Identity { id,name:friendly_name(&path,&exe),exe };
            if self.values.len()>=256 { self.values.clear(); }
            self.values.insert(pid,(path,app.clone())); Some(app)
        }
    }
}

pub(crate) fn friendly_name(path:&str, exe:&str)->String {
    if exe.eq_ignore_ascii_case("spotify.exe"){return "Spotify".into();}
    if exe.eq_ignore_ascii_case("steamwebhelper.exe"){return "Steam".into();}
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW,GetFileVersionInfoW,VerQueryValueW};
    unsafe {
        let path=wide(path); let mut unused=0; let size=GetFileVersionInfoSizeW(path.as_ptr(),&mut unused);
        if size>0 && size<1_048_576 {
            let mut data=vec![0u8;size as usize];
            if GetFileVersionInfoW(path.as_ptr(),0,size,data.as_mut_ptr() as *mut c_void)!=0 {
                let mut pointer=null_mut(); let mut length=0u32;
                if VerQueryValueW(data.as_ptr() as *const c_void,wide("\\VarFileInfo\\Translation").as_ptr(),&mut pointer,&mut length)!=0 && length>=4 {
                    let translation=std::slice::from_raw_parts(pointer as *const u16,2);
                    let key=wide(&format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription",translation[0],translation[1]));
                    if VerQueryValueW(data.as_ptr() as *const c_void,key.as_ptr(),&mut pointer,&mut length)!=0 && length>1 {
                        let offset=(pointer as usize).checked_sub(data.as_ptr() as usize);
                        if let Some(offset)=offset.filter(|offset|offset.saturating_add(length as usize*2)<=data.len()) {
                            let _=offset;
                            let units=std::slice::from_raw_parts(pointer as *const u16,length as usize);
                            let end=units.iter().position(|v|*v==0).unwrap_or(units.len());
                            if let Some(name)=clean_description(&String::from_utf16_lossy(&units[..end])){return name;}
                        }
                    }
                }
            }
        }
    }
    match exe.to_ascii_lowercase().as_str() {
        "chrome.exe"=>"Google Chrome".into(),"msedge.exe"=>"Microsoft Edge".into(),
        "firefox.exe"=>"Mozilla Firefox".into(),"code.exe"=>"Visual Studio Code".into(),
        "discord.exe"=>"Discord".into(),"steam.exe"=>"Steam".into(),
        _=>exe.strip_suffix(".exe").unwrap_or(exe).to_string()
    }
}

fn clean_description(value:&str)->Option<String>{
    let value=value.split('\0').next().unwrap_or("").trim();
    if value.is_empty() || value.chars().any(char::is_control){None}else{Some(value.chars().take(120).collect())}
}
#[cfg(test)]mod metadata_tests{use super::*;#[test]fn malformed_version_resources_cannot_pollute_application_names(){assert_eq!(clean_description("Spotify\0FileVersion"),Some("Spotify".into()));assert_eq!(clean_description("Spotify8\u{16}FileVersion"),None);assert_eq!(clean_description("  Visual Studio Code  "),Some("Visual Studio Code".into()));}}

struct Agent {
    store:Store, path:PathBuf, tracker:Tracker, settings:Settings,
    excluded:HashSet<String>, cache:ProcessCache, pending:Vec<Segment>,
    since_flush:Instant, since_diagnostics:Instant, started:Instant, db_transactions:u64, segments_written:u64,
    foreground_events:u64, flush_failures:u64, last_error:Option<String>,
    tray:NOTIFYICONDATAW, taskbar_message:u32, checked_date:String, stopping:bool,
    session_blocked:bool, suspended:bool,
    monitor_count:usize, visible_app_count:usize, scene_updates:u64,
    inventory:crate::inventory::Inventory,since_inventory:Instant,running_app_count:usize,
    media:crate::media::Watcher,spotify_state:crate::media::State,
    tray_available:bool, last_tray_error:Option<String>, last_tray_attempt:Instant,
}

impl Agent {
    fn tick(&mut self) {
        let rows=self.tracker.advance(sample());
        for row in rows {
            if let Some(previous)=self.pending.iter_mut().rev().find(|p|p.app_id==row.app_id && p.kind==row.kind) {
                if previous.app_id==row.app_id && previous.kind==row.kind && previous.ended_at==row.started_at && row.switches==0 {
                    previous.ended_at=row.ended_at; continue;
                }
            }
            self.pending.push(row);
        }
    }
    fn refresh_scene(&mut self) {
        self.tick();
        let(windows,monitors)=crate::visibility::snapshot();
        self.monitor_count=monitors;self.scene_updates+=1;
        let mut apps=std::collections::BTreeMap::new();
        let mut pids=HashSet::new();
        for hwnd in windows {
            let pid=app_pid(hwnd);
            if !pids.insert(pid){continue;}
            if let Some(app)=self.cache.identify(hwnd).filter(|a|!self.excluded.contains(&a.id)){apps.entry(app.id.clone()).or_insert(app);}
        }
        let visible:Vec<_>=apps.into_values().collect();
        self.visible_app_count=visible.len();self.inventory.remember(&visible);self.tracker.set_visible(visible);
        self.refresh_inventory(false);
    }
    fn refresh_inventory(&mut self,force:bool){
        self.tick();
        let apps=if force || self.since_inventory.elapsed()>=Duration::from_secs(15){self.since_inventory=Instant::now();self.inventory.refresh()}else{self.inventory.current()};
        let apps:Vec<_>=apps.into_iter().filter(|a|!self.excluded.contains(&a.id)).collect();
        self.running_app_count=apps.len();self.tracker.set_running(apps);
    }
    fn refresh_media(&mut self){
        self.tick();self.spotify_state=self.media.state();
        self.tracker.set_spotify(self.spotify_state.playing && !self.excluded.contains("spotify.exe"));
    }
    fn foreground(&mut self, hwnd:HWND) {
        self.tick(); self.foreground_events+=1;
        let app=self.cache.identify(hwnd).filter(|app|!self.excluded.contains(&app.id));
        self.tracker.set_app(app);
    }
    fn refresh_settings(&mut self)->Result<(),String> {
        self.tick();
        self.settings=self.store.settings()?;
        self.excluded=self.store.apps()?.into_iter().filter(|app|app.excluded).map(|app|app.id).collect();
        self.tracker.configure(self.settings.tracking_enabled,self.settings.idle_seconds);
        self.foreground(unsafe {GetForegroundWindow()});
        self.refresh_scene();self.refresh_inventory(true);self.refresh_media();self.update_tooltip(); Ok(())
    }
    fn error(&mut self,error:String) {
        self.flush_failures+=1;
        if self.last_error.as_ref()!=Some(&error) {
            let _=std::fs::write(self.path.with_file_name("agent-error.log"),format!("{} {error}\n",Local::now().to_rfc3339()));
            self.balloon("TimeWrapped: ошибка сохранения","Статистика остаётся в памяти. Проверьте доступ к папке данных.",NIIF_WARNING);
        }
        self.last_error=Some(error);
    }
    fn commit(&mut self)->Result<(),String> {
        self.tick();
        if !self.pending.is_empty() {
            match self.store.record_batch(&self.pending) {
                Ok(())=>{self.db_transactions+=1;self.segments_written+=self.pending.len() as u64;self.pending.clear();self.last_error=None;},
                Err(error)=>{self.error(error.clone());return Err(error);}
            }
        }
        self.since_flush=Instant::now();
        if self.since_diagnostics.elapsed()>=Duration::from_secs(300) {self.write_diagnostics();}
        self.weekly_notification();
        Ok(())
    }
    fn update_tooltip(&mut self) {
        let text=if self.settings.tracking_enabled {"TimeWrapped · отслеживание включено"} else {"TimeWrapped · пауза"};
        fill(&mut self.tray.szTip,text); self.tray.uFlags=NIF_TIP;
        if self.tray_available {unsafe {Shell_NotifyIconW(NIM_MODIFY,&self.tray);}}
    }
    fn try_tray(&mut self) {
        self.last_tray_attempt=Instant::now();
        self.tray.uFlags=NIF_MESSAGE|NIF_ICON|NIF_TIP;
        self.tray_available=unsafe {Shell_NotifyIconW(NIM_ADD,&self.tray)!=0};
        self.last_tray_error=if self.tray_available {None}else{
            Some("Shell_NotifyIconW(NIM_ADD) returned false; Explorer/tray may not be ready or available".into())
        };
        if self.tray_available {self.weekly_notification();}
    }
    fn balloon(&mut self,title:&str,body:&str,flags:u32) {
        if !self.tray_available {return;}
        self.tray.uFlags=NIF_INFO; fill(&mut self.tray.szInfoTitle,title); fill(&mut self.tray.szInfo,body);
        self.tray.dwInfoFlags=flags;
        unsafe {Shell_NotifyIconW(NIM_MODIFY,&self.tray);}
    }
    fn weekly_notification(&mut self) {
        // A missing shell must not consume the once-per-week notification.
        if !self.tray_available {return;}
        let date=Local::now().date_naive().to_string();
        if self.checked_date==date || !self.settings.notifications { return; }
        self.checked_date=date;
        let weeks=match self.store.list_weeks() {Ok(v)=>v,Err(error)=>{self.error(error);return;}};
        let Some(latest)=weeks.iter().max() else {return;};
        let marker=self.path.with_file_name("last-notified-week");
        let previous=std::fs::read_to_string(&marker).unwrap_or_default();
        if previous.trim()!=latest {
            self.balloon("TimeWrapped","Твои итоги недели готовы! Открой TimeWrapped из трея.",NIIF_INFO);
            if let Err(error)=std::fs::write(marker,latest) {self.error(error.to_string());}
        }
    }
    fn write_diagnostics(&mut self) {
        self.since_diagnostics=Instant::now();
        unsafe {
            let process=GetCurrentProcess(); let mut memory:PROCESS_MEMORY_COUNTERS_EX=zeroed();
            memory.cb=size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
            let memory_ok=GetProcessMemoryInfo(process,&mut memory as *mut _ as *mut PROCESS_MEMORY_COUNTERS,memory.cb)!=0;
            let mut created:FILETIME=zeroed(); let mut exited:FILETIME=zeroed();
            let mut kernel:FILETIME=zeroed(); let mut user:FILETIME=zeroed();
            let cpu_ok=GetProcessTimes(process,&mut created,&mut exited,&mut kernel,&mut user)!=0;
            let cpu_seconds=(filetime(kernel)+filetime(user)) as f64/10_000_000.;
            let lifetime=self.started.elapsed().as_secs_f64().max(0.001);
            let mut io:IO_COUNTERS=zeroed(); let io_ok=GetProcessIoCounters(process,&mut io)!=0;
            let processors=std::thread::available_parallelism().map(|n|n.get()).unwrap_or(1);
            let metrics=serde_json::json!({
                "pid":GetCurrentProcessId(),"sampled_at":Local::now().to_rfc3339(),
                "uptime_seconds":lifetime,"logical_processors":processors,
                "cpu_lifetime_percent":if cpu_ok && lifetime>=1.0 {Some(cpu_seconds/lifetime*100./processors as f64)}else{None},
                "cpu_single_core_percent":if cpu_ok && lifetime>=1.0 {Some(cpu_seconds/lifetime*100.)}else{None},
                "working_set_bytes":if memory_ok {Some(memory.WorkingSetSize)}else{None},
                "private_bytes":if memory_ok {Some(memory.PrivateUsage)}else{None},
                "write_operations":if io_ok {Some(io.WriteOperationCount)}else{None},
                "write_bytes":if io_ok {Some(io.WriteTransferCount)}else{None},
                "db_transactions":self.db_transactions,"segments_written":self.segments_written,
                "pending_segments":self.pending.len(),"foreground_events":self.foreground_events,
                "flush_failures":self.flush_failures,"last_error":self.last_error,
                "tracking_enabled":self.settings.tracking_enabled,"blocked":self.tracker.blocked,
                "tray_available":self.tray_available,"last_tray_error":self.last_tray_error,
                "boot_seconds":GetTickCount64() as f64/1000.0,"awake_since_boot_seconds":sample().awake_ms as f64/1000.0,
                "spotify_playing":self.spotify_state.playing,"spotify_status":self.spotify_state.status,"spotify_media_available":self.spotify_state.available,
                "running_app_count":self.running_app_count,"process_inventory_scans":self.inventory.scans,"running_sample_interval_seconds":15,
                "monitor_count":self.monitor_count,"visible_app_count":self.visible_app_count,"scene_updates":self.scene_updates,
                "protocol_version":5,"foreground_cache_entries":self.cache.values.len(),"webview_created":false,
                "note":"I/O counters include all process writes, not only SQLite; lifetime CPU includes startup. GPU requires external measurement."
            });
            if let Ok(json)=serde_json::to_vec_pretty(&metrics) {
                let _=std::fs::write(self.path.with_file_name("agent-diagnostics.json"),json);
            }
        }
    }
    fn command(&mut self,hwnd:HWND,command:usize) {
        match command {
            CMD_OPEN=>{if let Err(error)=open_ui(){self.error(error);}},
            CMD_PAUSE=>{
                self.tick(); self.settings.tracking_enabled=!self.settings.tracking_enabled;
                if let Err(error)=self.store.save_settings(&self.settings){self.error(error);return;}
                if let Err(error)=self.refresh_settings(){self.error(error);}
                let _=self.commit();
            },
            CMD_QUIT=>{ if self.commit().is_ok() { self.stopping=true;crate::native::close_gui();unsafe {DestroyWindow(hwnd);} } },
            _=>{}
        }
    }
}

fn fill<const N:usize>(target:&mut [u16;N],value:&str) {
    target.fill(0); for (slot,ch) in target.iter_mut().take(N-1).zip(value.encode_utf16()) {*slot=ch;}
}
fn filetime(value:FILETIME)->u64 {((value.dwHighDateTime as u64)<<32)|value.dwLowDateTime as u64}
fn process_start_instant()->Instant {
    // CPU counters include loader/startup work before run_inner. Anchor the
    // monotonic denominator to the actual process creation time once; later
    // wall-clock adjustments cannot distort lifetime CPU measurements.
    let current=Instant::now();
    unsafe {
        let mut created:FILETIME=zeroed();let mut exited:FILETIME=zeroed();
        let mut kernel:FILETIME=zeroed();let mut user:FILETIME=zeroed();
        if GetProcessTimes(GetCurrentProcess(),&mut created,&mut exited,&mut kernel,&mut user)==0 {return current;}
        let mut now:FILETIME=zeroed();GetSystemTimeAsFileTime(&mut now);
        let ticks=filetime(now).saturating_sub(filetime(created));
        let age=Duration::new(ticks/10_000_000,((ticks%10_000_000)*100) as u32);
        current.checked_sub(age).unwrap_or(current)
    }
}
fn session_is_blocked()->Option<bool> {
    unsafe {
        let mut buffer=null_mut();let mut bytes=0;
        if WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE,WTS_CURRENT_SESSION,WTSSessionInfoEx,&mut buffer,&mut bytes)==0 {return None;}
        let result=if bytes as usize>=size_of::<WTSINFOEXW>() {
            let info=&*(buffer as *const WTSINFOEXW);
            if info.Level==1 {
                // Read only session/lock flags; never inspect user/domain names.
                let state=&info.Data.WTSInfoExLevel1;
                Some(state.SessionState!=WTSActive || state.SessionFlags==WTS_SESSIONSTATE_LOCK as i32)
            }else{None}
        }else{None};
        WTSFreeMemory(buffer as *mut c_void);result
    }
}
unsafe fn tray_menu(hwnd:HWND,tracking_enabled:bool)->usize {
    let menu=CreatePopupMenu(); if menu.is_null() {return 0;}
    AppendMenuW(menu,MF_STRING,CMD_OPEN,wide("Открыть TimeWrapped").as_ptr());
    AppendMenuW(menu,MF_STRING,CMD_PAUSE,wide(if tracking_enabled {"Приостановить отслеживание"} else {"Продолжить отслеживание"}).as_ptr());
    AppendMenuW(menu,MF_SEPARATOR,0,null());
    AppendMenuW(menu,MF_STRING,CMD_QUIT,wide("Полностью выйти").as_ptr());
    let mut cursor:POINT=zeroed();GetCursorPos(&mut cursor);SetForegroundWindow(hwnd);
    // TrackPopupMenu runs a nested message loop, so no Agent reference may be
    // borrowed across this call: WM_TIMER and foreground events can re-enter.
    let command=TrackPopupMenu(menu,TPM_RETURNCMD|TPM_NONOTIFY|TPM_RIGHTBUTTON,cursor.x,cursor.y,0,hwnd,null());
    DestroyMenu(menu); PostMessageW(hwnd,WM_NULL,0,0);command as usize
}
fn open_ui()->Result<(),String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(std::env::current_exe().map_err(|e|e.to_string())?).creation_flags(0x08000000)
        .spawn().map_err(|e|e.to_string())?;Ok(())
}

unsafe extern "system" fn scene_callback(_:HWINEVENTHOOK,_:u32,_:HWND,object:i32,child:i32,_:u32,_:u32) {
    if object!=0 || child!=0{return;}
    let window=WINDOW.load(Ordering::Relaxed) as HWND;
    if !window.is_null() && !SCENE_QUEUED.swap(true,Ordering::Relaxed){PostMessageW(window,MSG_SCENE,0,0);}
}
unsafe extern "system" fn foreground_callback(_:HWINEVENTHOOK,_:u32,hwnd:HWND,_:i32,_:i32,_:u32,_:u32) {
    let window=WINDOW.load(Ordering::Relaxed) as HWND;
    if !window.is_null(){PostMessageW(window,MSG_FOREGROUND,0,hwnd as isize);scene_callback(null_mut(),0,hwnd,0,0,0,0);}
}

unsafe extern "system" fn window_proc(hwnd:HWND,message:u32,wparam:WPARAM,lparam:LPARAM)->LRESULT {
    let pointer=GetWindowLongPtrW(hwnd,GWLP_USERDATA) as *mut Agent;
    if pointer.is_null() {return DefWindowProcW(hwnd,message,wparam,lparam);}
    let agent=&mut *pointer;
    if agent.taskbar_message!=0 && message==agent.taskbar_message {
        agent.tray_available=false;agent.try_tray(); return 0;
    }
    match message {
        MSG_FOREGROUND=>{agent.foreground(lparam as HWND);0},
        MSG_PROTOCOL=>5,
        MSG_MEDIA=>{agent.refresh_media();0},
        MSG_SCENE=>{SetTimer(hwnd,SCENE_TIMER,250,None);0},
        WM_DISPLAYCHANGE=>{agent.refresh_scene();0},
        MSG_SETTINGS=>{match agent.refresh_settings(){Ok(())=>1,Err(error)=>{agent.error(error);0}}},
        MSG_FLUSH=>{
            let result=agent.commit().is_ok();
            // Visible UI may request several models together. One diagnostic
            // write per second is sufficient; hidden background uses 5 minutes.
            if agent.since_diagnostics.elapsed()>=Duration::from_secs(1) {agent.write_diagnostics();}
            isize::from(result)
        },
        MSG_QUIT=>{agent.command(hwnd,CMD_QUIT);0},
        WM_TIMER=>{
            if wparam==SCENE_TIMER {KillTimer(hwnd,SCENE_TIMER);SCENE_QUEUED.store(false,Ordering::Relaxed);agent.refresh_scene();return 0;}
            agent.tick();agent.refresh_inventory(false);agent.refresh_media();
            if !agent.tray_available && agent.last_tray_attempt.elapsed()>=Duration::from_secs(30) {
                agent.try_tray();
            }
            if agent.since_flush.elapsed()>=Duration::from_secs(45) {
                if let Err(error)=agent.refresh_settings(){agent.error(error);}
                let _=agent.commit();
            }0
        },
        MSG_TRAY=>{
            match lparam as u32 {
                WM_LBUTTONUP|WM_LBUTTONDBLCLK|NIN_BALLOONUSERCLICK=>{if let Err(error)=open_ui(){agent.error(error);}},
                WM_RBUTTONUP|WM_CONTEXTMENU=>{
                    let enabled=agent.settings.tracking_enabled;
                    let command=tray_menu(hwnd,enabled);
                    (&mut *pointer).command(hwnd,command);
                },_=>{}
            }0
        },
        WM_COMMAND=>{agent.command(hwnd,wparam&0xffff);0},
        WM_WTSSESSION_CHANGE=>{
            match wparam as u32 {
                2|4|6|7=>{agent.tick();agent.session_blocked=true;agent.tracker.blocked=true;let _=agent.commit();},
                1|3|5|8=>{
                    agent.tick();
                    agent.session_blocked=if wparam as u32==8 {false}else{session_is_blocked().unwrap_or(agent.session_blocked)};
                    agent.tracker.blocked=agent.session_blocked||agent.suspended;
                    agent.foreground(GetForegroundWindow());agent.refresh_scene();
                },
                _=>{}
            }0
        },
        WM_POWERBROADCAST=>{
            match wparam as u32 {
                4=>{agent.tick();agent.suspended=true;agent.tracker.blocked=true;let _=agent.commit();},
                7|18=>{
                    agent.tick();agent.suspended=false;
                    agent.session_blocked=session_is_blocked().unwrap_or(agent.session_blocked);
                    agent.tracker.blocked=agent.session_blocked;
                    agent.foreground(GetForegroundWindow());agent.refresh_scene();
                },
                _=>{}
            }1
        },
        WM_QUERYENDSESSION=>{let _=agent.commit();1},
        WM_ENDSESSION=>{if wparam!=0 {let _=agent.commit();DestroyWindow(hwnd);}0},
        WM_DESTROY=>{
            KillTimer(hwnd,TIMER);KillTimer(hwnd,SCENE_TIMER);WTSUnRegisterSessionNotification(hwnd);
            if agent.tray_available {Shell_NotifyIconW(NIM_DELETE,&agent.tray);}PostQuitMessage(0);0
        },
        _=>DefWindowProcW(hwnd,message,wparam,lparam)
    }
}

pub fn run(path:PathBuf)->Result<(),String> {
    unsafe {
        let suffix=std::env::var("TIMEWRAPPED_DATA_DIR").unwrap_or_default();
        // Named mutex is process/session local. Hash keeps the name legal and
        // fixed length even when an isolated directory contains backslashes.
        let hash=suffix.bytes().fold(14695981039346656037u64,|hash,b|(hash^b as u64).wrapping_mul(1099511628211));
        let mutex=CreateMutexW(null(),1,wide(&format!("Local\\TimeWrapped.Agent.v1.{hash:016x}")).as_ptr());
        if mutex.is_null(){return Err(win_error("CreateMutexW"));}
        if GetLastError()==ERROR_ALREADY_EXISTS {CloseHandle(mutex);return Ok(());}
        let result=run_inner(path);
        ReleaseMutex(mutex);CloseHandle(mutex);result
    }
}

unsafe fn run_inner(path:PathBuf)->Result<(),String> {
    // DWM bounds and display rectangles must share physical coordinates even
    // with different scale factors and negative monitor positions.
    windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let store=Store::open(&path)?; let settings=store.settings()?;
    let excluded=store.apps()?.into_iter().filter(|app|app.excluded).map(|app|app.id).collect();
    let instance=GetModuleHandleW(null());let class_name=wide(CLASS);
    let mut class:WNDCLASSW=zeroed(); class.lpfnWndProc=Some(window_proc);class.hInstance=instance;class.lpszClassName=class_name.as_ptr();
    if RegisterClassW(&class)==0 {return Err(win_error("RegisterClassW"));}
    // A hidden top-level window receives WM_POWERBROADCAST; HWND_MESSAGE does not.
    let hwnd=CreateWindowExW(WS_EX_TOOLWINDOW|WS_EX_NOACTIVATE,class_name.as_ptr(),window_name().as_ptr(),
        WS_OVERLAPPED,0,0,0,0,null_mut(),null_mut(),instance,null());
    if hwnd.is_null(){return Err(win_error("CreateWindowExW"));}
    let mut own_icon=null_mut();
    if let Ok(executable)=std::env::current_exe() {
        ExtractIconExW(wide(&executable.to_string_lossy()).as_ptr(),0,&mut own_icon,null_mut(),1);
    }
    let icon=if own_icon.is_null(){LoadIconW(null_mut(),IDI_APPLICATION)}else{own_icon};
    let _own_icon_guard=OwnedIcon(own_icon);
    let mut tray:NOTIFYICONDATAW=zeroed();tray.cbSize=size_of::<NOTIFYICONDATAW>() as u32;
    tray.hWnd=hwnd;tray.uID=1;tray.uFlags=NIF_MESSAGE|NIF_ICON|NIF_TIP;tray.uCallbackMessage=MSG_TRAY;tray.hIcon=icon;
    fill(&mut tray.szTip,"TimeWrapped · локальная статистика");
    // Explorer is often not ready during Windows autostart. The collector and
    // IPC remain usable while the tray registration is retried in the timer.
    let tray_available=Shell_NotifyIconW(NIM_ADD,&tray)!=0;
    let last_tray_error=if tray_available {None}else{
        Some("Shell_NotifyIconW(NIM_ADD) returned false; Explorer/tray may not be ready or available".into())
    };
    let taskbar_message=RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
    let mut tracker=Tracker::new(sample(),settings.tracking_enabled,settings.idle_seconds);
    let session_blocked=session_is_blocked().unwrap_or(false);tracker.blocked=session_blocked;
    let known=store.apps()?.into_iter().map(|a|a.id).collect();
    let mut state=Box::new(Agent {store,path,tracker,settings,excluded,cache:ProcessCache::new(),pending:Vec::new(),
        since_flush:Instant::now(),since_diagnostics:Instant::now(),started:process_start_instant(),db_transactions:0,segments_written:0,foreground_events:0,
        flush_failures:0,last_error:None,tray,taskbar_message,checked_date:String::new(),stopping:false,session_blocked,suspended:false,
        monitor_count:0,visible_app_count:0,scene_updates:0,
        inventory:crate::inventory::Inventory::new(known),since_inventory:Instant::now()-Duration::from_secs(30),running_app_count:0,
        media:crate::media::Watcher::start(hwnd,MSG_MEDIA),spotify_state:crate::media::State::default(),tray_available,last_tray_error,last_tray_attempt:Instant::now()});
    SetWindowLongPtrW(hwnd,GWLP_USERDATA,&mut *state as *mut Agent as isize);
    WINDOW.store(hwnd as isize,Ordering::Relaxed);
    if WTSRegisterSessionNotification(hwnd,NOTIFY_FOR_THIS_SESSION)==0 {state.error(win_error("WTSRegisterSessionNotification"));}
    let hook=SetWinEventHook(EVENT_SYSTEM_FOREGROUND,EVENT_SYSTEM_FOREGROUND,null_mut(),Some(foreground_callback),0,0,WINEVENT_OUTOFCONTEXT);
    if hook.is_null(){DestroyWindow(hwnd);return Err(win_error("SetWinEventHook"));}
    if SetTimer(hwnd,TIMER,5_000,None)==0 {UnhookWinEvent(hook);DestroyWindow(hwnd);return Err(win_error("SetTimer"));}
    let mut scene_hooks=Vec::new();
    for(first,last) in [(EVENT_OBJECT_CREATE,EVENT_OBJECT_HIDE),(EVENT_OBJECT_LOCATIONCHANGE,EVENT_OBJECT_LOCATIONCHANGE),
        (EVENT_SYSTEM_MINIMIZESTART,EVENT_SYSTEM_MINIMIZEEND),(0x8017,0x8018),(EVENT_OBJECT_REORDER,EVENT_OBJECT_REORDER)] {
        let h=SetWinEventHook(first,last,null_mut(),Some(scene_callback),0,0,WINEVENT_OUTOFCONTEXT);
        if !h.is_null(){scene_hooks.push(h);}
    }
    state.foreground(GetForegroundWindow());state.refresh_scene();state.update_tooltip();state.weekly_notification();state.write_diagnostics();
    let mut message:MSG=zeroed();
    loop {
        let status=GetMessageW(&mut message,null_mut(),0,0);
        if status<=0 {if status<0 {state.error(win_error("GetMessageW"));}break;}
        TranslateMessage(&message);DispatchMessageW(&message);
    }
    WINDOW.store(0,Ordering::Relaxed);UnhookWinEvent(hook);for hook in scene_hooks{UnhookWinEvent(hook);}
    SetWindowLongPtrW(hwnd,GWLP_USERDATA,0);
    // Explicit exit is only allowed after a successful commit. Windows session
    // termination gets a final best effort, with errors retained in the log.
    if !state.stopping {state.commit()?;}
    state.write_diagnostics();
    Ok(())
}
