//! Native utilities used only on demand by the GUI, never by the sampling timer.
use std::{path::PathBuf, ptr::{null, null_mut}, mem::{size_of, zeroed}};
use base64::Engine;
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, System::{Registry::*, Threading::*, Diagnostics::ToolHelp::*}, UI::{WindowsAndMessaging::*, Controls::Dialogs::*, Shell::*}};

fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }
fn error(context: &str) -> String { format!("{context}: {}", std::io::Error::last_os_error()) }

pub struct GuiGuard(HANDLE);
impl Drop for GuiGuard { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
pub fn gui_single_instance(activate_existing:bool) -> Result<Option<GuiGuard>, String> {
    let suffix = std::env::var("TIMEWRAPPED_DATA_DIR").unwrap_or_default().replace(['\\', '/', ':'], "_");
    unsafe {
        let handle = CreateMutexW(null(), 0, wide(&format!("Local\\TimeWrapped.GUI.v1.{suffix}")).as_ptr());
        if handle.is_null() { return Err(error("Не удалось создать mutex интерфейса")); }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            let window = FindWindowW(null(), wide("TimeWrapped").as_ptr());
            if activate_existing&&!window.is_null() { ShowWindow(window, SW_RESTORE); SetForegroundWindow(window); }
            return Ok(None);
        }
        Ok(Some(GuiGuard(handle)))
    }
}
pub fn show_error(message: &str) {
    unsafe { MessageBoxW(null_mut(), wide(message).as_ptr(), wide("TimeWrapped").as_ptr(), MB_OK | MB_ICONERROR); }
}
pub fn close_gui() {
    unsafe { let window=FindWindowW(null(), wide("TimeWrapped").as_ptr()); if !window.is_null() { PostMessageW(window, WM_CLOSE, 0, 0); } }
}

pub fn set_autostart(enabled: bool,open_window:bool) -> Result<(), String> {
    let command=autostart_command(open_window)?;
    unsafe {
        let mut key = null_mut();
        let status = RegCreateKeyExW(HKEY_CURRENT_USER, wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run").as_ptr(), 0, null(), 0, KEY_SET_VALUE, null(), &mut key, null_mut());
        if status != ERROR_SUCCESS { return Err(format!("Автозагрузка: ошибка Windows {status}")); }
        let result = if enabled {
            let value=wide(&command);
            RegSetValueExW(key, wide("TimeWrapped").as_ptr(), 0, REG_SZ, value.as_ptr() as *const u8, (value.len()*2) as u32)
        } else { RegDeleteValueW(key, wide("TimeWrapped").as_ptr()) };
        RegCloseKey(key);
        if result != ERROR_SUCCESS && !(result == ERROR_FILE_NOT_FOUND && !enabled) { return Err(format!("Автозагрузка: ошибка Windows {result}")); }
        Ok(())
    }
}

fn autostart_command(open_window:bool)->Result<String,String>{let exe=std::env::current_exe().map_err(|e|e.to_string())?;Ok(format!("\"{}\"{}",exe.display(),if open_window{""}else{" --background"}))}
pub fn autostart_status()->Result<serde_json::Value,String>{unsafe{
    let mut key=null_mut();let result=RegOpenKeyExW(HKEY_CURRENT_USER,wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run").as_ptr(),0,KEY_QUERY_VALUE,&mut key);
    if result==ERROR_FILE_NOT_FOUND{return Ok(serde_json::json!({"registered":false,"matches_current_exe":false}));}
    if result!=ERROR_SUCCESS{return Err(format!("Не удалось прочитать автозапуск Windows: {result}"));}
    let mut buffer=vec![0u16;32768];let mut length=(buffer.len()*2)as u32;let mut kind=0;
    let result=RegQueryValueExW(key,wide("TimeWrapped").as_ptr(),null(),&mut kind,buffer.as_mut_ptr()as _,&mut length);RegCloseKey(key);
    if result==ERROR_FILE_NOT_FOUND{return Ok(serde_json::json!({"registered":false,"matches_current_exe":false}));}
    if result!=ERROR_SUCCESS||kind!=REG_SZ{return Err("Не удалось прочитать запись TimeWrapped в автозапуске".into());}
    let value=String::from_utf16_lossy(&buffer[..(length as usize/2).min(buffer.len())]).trim_end_matches('\0').to_string();
    let background=value==autostart_command(false)?;let window=value==autostart_command(true)?;
    Ok(serde_json::json!({"registered":true,"matches_current_exe":background||window,"open_window":window}))
}}
pub fn open_link(value:&str)->Result<(),String>{
    if value!="ms-settings:startupapps"{
        let url=url::Url::parse(value).map_err(|_|"Некорректная ссылка".to_string())?;
        if url.scheme()!="https"||!url.username().is_empty()||url.password().is_some()||url.port().is_some()||!url.host_str().is_some_and(|h|["steamcommunity.com","www.dotabuff.com","www.opendota.com","stratz.com","www.faceit.com","developers.faceit.com","docs.faceit.com","soundcloud.com"].contains(&h)){return Err("Неподдерживаемая ссылка расширения".into());}
    }
    unsafe{let result=ShellExecuteW(null_mut(),wide("open").as_ptr(),wide(value).as_ptr(),null(),null(),SW_SHOWNORMAL);if result as isize<=32{return Err("Не удалось открыть ссылку в браузере".into());}}Ok(())
}

pub fn save_file(name: &str, filter: &str, bytes: &[u8]) -> Result<Option<String>, String> {
    unsafe {
        let mut file = vec![0u16; 32_768];
        let suggested = wide(name);
        file[..suggested.len()].copy_from_slice(&suggested);
        let filter = wide(filter);
        let extension = wide(name.rsplit('.').next().unwrap_or(""));
        let mut dialog: OPENFILENAMEW = zeroed();
        dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        dialog.hwndOwner = FindWindowW(null(), wide("TimeWrapped").as_ptr());
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = file.as_mut_ptr();
        dialog.nMaxFile = file.len() as u32;
        dialog.lpstrDefExt = extension.as_ptr();
        dialog.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_EXPLORER;
        if GetSaveFileNameW(&mut dialog) == 0 {
            let code = CommDlgExtendedError();
            return if code == 0 { Ok(None) } else { Err(format!("Диалог сохранения: {code}")) };
        }
        let path = PathBuf::from(String::from_utf16_lossy(&file[..file.iter().position(|v|*v==0).unwrap_or(file.len())]));
        std::fs::write(&path, bytes).map_err(|e|e.to_string())?;
        Ok(Some(path.to_string_lossy().into_owned()))
    }
}

fn cache_dir() -> PathBuf { crate::store::Store::default_path().with_file_name("icons") }
pub fn clear_icon_cache() -> Result<(), String> {
    let path = cache_dir();
    if path.exists() {
        for item in std::fs::read_dir(&path).map_err(|e|e.to_string())? {
            let item=item.map_err(|e|e.to_string())?;
            if item.file_type().map_err(|e|e.to_string())?.is_file() && item.path().extension().is_some_and(|v|v=="png") {
                std::fs::remove_file(item.path()).map_err(|e|e.to_string())?;
            }
        }
        // Only remove an empty directory; never recursively remove an unexpected entry.
        let _=std::fs::remove_dir(path);
    }
    Ok(())
}
pub fn app_icon(id: &str) -> Result<Option<String>, String> {
    // IDs are executable basenames. Prevent paths or oversized filenames from IPC.
    if id.len()>200 || id.contains(['/', '\\', ':']) || !id.ends_with(".exe") { return Ok(None); }
    let cache = cache_dir().join(format!("{id}.png"));
    if let Ok(bytes) = std::fs::read(&cache) { return Ok(Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))); }
    let Some(path) = executable_path(id) else { return Ok(None); };
    let Some(bytes) = extract_png(&path)? else { return Ok(None); };
    std::fs::create_dir_all(cache_dir()).map_err(|e|e.to_string())?;
    std::fs::write(cache, &bytes).map_err(|e|e.to_string())?;
    Ok(Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}
fn executable_path(id: &str) -> Option<String> {
    // A process snapshot is taken only on an uncached GUI icon request.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE { return None; }
        let mut entry: PROCESSENTRY32W = zeroed(); entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry);
        let mut found = None;
        while more != 0 {
            let len = entry.szExeFile.iter().position(|v|*v==0).unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(id) {
                let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                if !process.is_null() {
                    let mut buf = vec![0u16; 32_768]; let mut size=buf.len() as u32;
                    if QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut size) != 0 { found=Some(String::from_utf16_lossy(&buf[..size as usize])); }
                    CloseHandle(process);
                }
                if found.is_some() { break; }
            }
            more = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot); found
    }
}
fn extract_png(path: &str) -> Result<Option<Vec<u8>>, String> {
    unsafe {
        let mut info: SHFILEINFOW = zeroed();
        if SHGetFileInfoW(wide(path).as_ptr(), 0, &mut info, size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON) == 0 || info.hIcon.is_null() { return Ok(None); }
        let dc = CreateCompatibleDC(null_mut());
        let mut bitmap_info: BITMAPINFO = zeroed();
        bitmap_info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bitmap_info.bmiHeader.biWidth = 32; bitmap_info.bmiHeader.biHeight = -32;
        bitmap_info.bmiHeader.biPlanes = 1; bitmap_info.bmiHeader.biBitCount = 32;
        bitmap_info.bmiHeader.biCompression = BI_RGB;
        let mut pixels = null_mut();
        let bitmap = CreateDIBSection(dc, &bitmap_info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
        if dc.is_null() || bitmap.is_null() || pixels.is_null() { if !bitmap.is_null(){DeleteObject(bitmap);} if !dc.is_null(){DeleteDC(dc);} DestroyIcon(info.hIcon); return Ok(None); }
        let old = SelectObject(dc, bitmap);
        std::ptr::write_bytes(pixels as *mut u8, 0, 32*32*4);
        DrawIconEx(dc, 0, 0, info.hIcon, 32, 32, 0, null_mut(), DI_NORMAL);
        let mut rgba=std::slice::from_raw_parts(pixels as *const u8, 32*32*4).to_vec();
        SelectObject(dc,old); DeleteObject(bitmap); DeleteDC(dc); DestroyIcon(info.hIcon);
        let has_alpha=rgba.chunks_exact(4).any(|p|p[3]!=0);
        for pixel in rgba.chunks_exact_mut(4) { pixel.swap(0,2); if !has_alpha {pixel[3]=255;} }
        let mut bytes=Vec::new();
        { let mut encoder=png::Encoder::new(&mut bytes,32,32); encoder.set_color(png::ColorType::Rgba); encoder.set_depth(png::BitDepth::Eight); let mut writer=encoder.write_header().map_err(|e|e.to_string())?; writer.write_image_data(&rgba).map_err(|e|e.to_string())?; }
        Ok(Some(bytes))
    }
}

pub fn open_history_url(value:&str)->Result<(),String>{
 let u=url::Url::parse(value).map_err(|_|"Некорректный адрес страницы")?;
 if crate::web_history::domain(value).is_none()||!u.username().is_empty()||u.password().is_some(){return Err("Некорректный адрес страницы".into());}
 unsafe{let result=ShellExecuteW(null_mut(),wide("open").as_ptr(),wide(value).as_ptr(),null(),null(),SW_SHOWNORMAL);if result as usize<=32{return Err(error("Не удалось открыть страницу"));}}Ok(())
}
