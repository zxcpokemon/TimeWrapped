//! Bounded periodic discovery of user-session application processes. No captions.
use std::{collections::{BTreeMap,HashMap,HashSet},mem::{zeroed,size_of},path::Path};
use windows_sys::Win32::{Foundation::*,System::{Diagnostics::ToolHelp::*,Threading::*,RemoteDesktop::ProcessIdToSessionId}};
use crate::tracker::Identity;

struct Entry { handle:HANDLE, app:Identity }
impl Drop for Entry {fn drop(&mut self){unsafe{CloseHandle(self.handle);}}}
pub struct Inventory { entries:HashMap<u32,Entry>, known:HashSet<String>, pub scans:u64 }
impl Inventory {
    pub fn new(known:HashSet<String>)->Self {Self{entries:HashMap::new(),known,scans:0}}
    pub fn remember(&mut self,apps:&[Identity]) {for a in apps{self.known.insert(a.id.clone());}}
    pub fn refresh(&mut self)->Vec<Identity> {
        self.scans+=1;
        self.entries.retain(|_,e|alive(e.handle));
        let windows=crate::visibility::application_pids();
        unsafe {
            let mut session=0;ProcessIdToSessionId(GetCurrentProcessId(),&mut session);
            let snap=CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS,0);
            if snap!=INVALID_HANDLE_VALUE {
                let mut e:PROCESSENTRY32W=zeroed();e.dwSize=size_of::<PROCESSENTRY32W>() as u32;
                let mut more=Process32FirstW(snap,&mut e);
                let mut processes=Vec::new();
                while more!=0 {processes.push((e.th32ProcessID,e.th32ParentProcessID,String::from_utf16_lossy(&e.szExeFile).trim_end_matches('\0').to_ascii_lowercase()));more=Process32NextW(snap,&mut e);}
                let names:HashMap<_,_>=processes.iter().map(|(pid,_,exe)|(*pid,exe.as_str())).collect();
                for &(pid,parent,ref exe_name) in &processes {
                    let same_exe_child=names.get(&parent).is_some_and(|v|*v==exe_name);
                    if !crate::app_policy::eligible(exe_name,windows.contains(&pid),self.known.contains(exe_name),same_exe_child){continue;}
                    if !self.entries.contains_key(&pid) && self.entries.len()<768 {
                        let mut owner=0;
                        if ProcessIdToSessionId(pid,&mut owner)!=0 && owner==session {
                            let handle=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid);
                            if !handle.is_null() {
                                if let Some(path)=path_for(handle) {
                                    let exe=Path::new(&path).file_name().map(|v|v.to_string_lossy().to_string()).unwrap_or_default();
                                    let id=exe.to_lowercase();
                                    if !exe.is_empty() && !crate::app_policy::supporting_process(&id) {
                                        self.known.insert(id.clone());
                                        let name=crate::agent::friendly_name(&path,&exe);
                                        self.entries.insert(pid,Entry{handle,app:Identity{id,name,exe}});
                                    }else{CloseHandle(handle);}
                                }else{CloseHandle(handle);}
                            }
                        }
                    }
                }
                CloseHandle(snap);
            }
        }
        self.current()
    }
    pub fn current(&mut self)->Vec<Identity> {
        self.entries.retain(|_,e|alive(e.handle));
        let mut apps=BTreeMap::new();for e in self.entries.values(){apps.entry(e.app.id.clone()).or_insert_with(||e.app.clone());}apps.into_values().collect()
    }
}
fn alive(handle:HANDLE)->bool {unsafe{let mut code=0;GetExitCodeProcess(handle,&mut code)!=0 && code==STILL_ACTIVE as u32}}
fn path_for(handle:HANDLE)->Option<String> {unsafe{let mut buf=vec![0u16;32768];let mut len=buf.len() as u32;if QueryFullProcessImageNameW(handle,0,buf.as_mut_ptr(),&mut len)==0{None}else{Some(String::from_utf16_lossy(&buf[..len as usize]))}}}
pub fn is_spotify_pid(pid:u32)->bool {unsafe{let handle=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid);if handle.is_null(){return false;}let result=path_for(handle).is_some_and(|p|Path::new(&p).file_name().is_some_and(|n|n.to_string_lossy().eq_ignore_ascii_case("spotify.exe")));CloseHandle(handle);result}}

