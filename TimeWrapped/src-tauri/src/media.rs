//! Read-only Spotify playback state. Never requests media titles or artwork.
use std::{sync::{Arc,Mutex,mpsc},time::Duration};
use windows::{core::Interface,Foundation::TypedEventHandler,Media::Control::*,Win32::{System::Com::*,Media::Audio::{*,Endpoints::*}}};
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct State {pub playing:bool,pub status:String,pub available:bool}
impl Default for State{fn default()->Self{Self{playing:false,status:"Проверяем состояние Spotify…".into(),available:false}}}
enum Command{Refresh,Stop}
pub struct Watcher{state:Arc<Mutex<State>>,tx:mpsc::Sender<Command>}
impl Watcher {
    pub fn start(hwnd:windows_sys::Win32::Foundation::HWND,message:u32)->Self {
        let state=Arc::new(Mutex::new(State::default()));let shared=state.clone();let(tx,rx)=mpsc::channel();let wake=tx.clone();let window=hwnd as usize;
        let _=std::thread::Builder::new().name("spotify-state".into()).spawn(move||{
            unsafe{if CoInitializeEx(None,COINIT_MULTITHREADED).is_err(){return;}}
            let manager=GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op|op.join());
            let manager=match manager{Ok(m)=>m,Err(e)=>{if let Ok(mut state)=shared.lock(){*state=State{playing:false,status:format!("Windows не предоставила медиастатус: {}",e.code()),available:false};}unsafe{windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(window as _,message,0,0);CoUninitialize();}return;}};
            let events=wake.clone();
            let manager_token=manager.SessionsChanged(&TypedEventHandler::<GlobalSystemMediaTransportControlsSessionManager,SessionsChangedEventArgs>::new(move|_,_|{let _=events.send(Command::Refresh);Ok(())})).ok();
            let mut hooks:Vec<(GlobalSystemMediaTransportControlsSession,i64)>=Vec::new();
            loop {
                let current=read_state(&manager,&mut hooks,&wake);
                if let Ok(mut old)=shared.lock(){if *old!=current{*old=current;unsafe{windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(window as _,message,0,0);}}}
                match rx.recv_timeout(Duration::from_secs(5)){Ok(Command::Stop)|Err(mpsc::RecvTimeoutError::Disconnected)=>break,_=>{}}
            }
            for(s,token)in hooks{let _=s.RemovePlaybackInfoChanged(token);}if let Some(token)=manager_token{let _=manager.RemoveSessionsChanged(token);}unsafe{CoUninitialize();}
        });
        Self{state,tx}
    }
    pub fn state(&self)->State{self.state.lock().map(|s|s.clone()).unwrap_or_default()}
}
impl Drop for Watcher{fn drop(&mut self){let _=self.tx.send(Command::Stop);}}

fn read_state(manager:&GlobalSystemMediaTransportControlsSessionManager,hooks:&mut Vec<(GlobalSystemMediaTransportControlsSession,i64)>,wake:&mpsc::Sender<Command>)->State {
    let sessions=match manager.GetSessions(){Ok(s)=>s,Err(_)=>return State{playing:false,status:"Медиастатус Windows временно недоступен".into(),available:false}};
    let mut found=false;let mut playing=false;let mut present=std::collections::HashSet::new();
    if let Ok(count)=sessions.Size(){for i in 0..count{if let Ok(s)=sessions.GetAt(i){
        if !s.SourceAppUserModelId().map(|v|v.to_string().to_lowercase().contains("spotify")).unwrap_or(false){continue;}
        found=true;present.insert(s.as_raw() as usize);
        if !hooks.iter().any(|(old,_)|old.as_raw()==s.as_raw()){
            let wake=wake.clone();if let Ok(token)=s.PlaybackInfoChanged(&TypedEventHandler::<GlobalSystemMediaTransportControlsSession,PlaybackInfoChangedEventArgs>::new(move|_,_|{let _=wake.send(Command::Refresh);Ok(())})){hooks.push((s.clone(),token));}
        }
        playing|=s.GetPlaybackInfo().and_then(|p|p.PlaybackStatus()).is_ok_and(|v|v==GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
    }}}
    hooks.retain(|(s,t)|{if present.contains(&(s.as_raw() as usize)){true}else{let _=s.RemovePlaybackInfoChanged(*t);false}});
    if !found{return State{playing:false,status:"Spotify не воспроизводит через медиасессию Windows".into(),available:true};}
    if !playing{return State{playing:false,status:"Spotify: пауза".into(),available:true};}
    match local_audio(){Ok(true)=>State{playing:true,status:"Spotify играет на этом ПК".into(),available:true},Ok(false)=>State{playing:false,status:"Нет локального звука Spotify: пауза, mute или другое устройство".into(),available:true},Err(_)=>State{playing:false,status:"Не удалось подтвердить локальный звук Spotify".into(),available:false}}
}
fn local_audio()->windows::core::Result<bool>{unsafe{
    let enumerator:IMMDeviceEnumerator=CoCreateInstance(&MMDeviceEnumerator,None,CLSCTX_ALL)?;
    let devices=enumerator.EnumAudioEndpoints(eRender,DEVICE_STATE_ACTIVE)?;
    for i in 0..devices.GetCount()?{
        let device=devices.Item(i)?;
        if let Ok(volume)=device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL,None){if volume.GetMute()?.as_bool() || volume.GetMasterVolumeLevelScalar()?<=0.00001{continue;}}
        let Ok(manager)=device.Activate::<IAudioSessionManager2>(CLSCTX_ALL,None)else{continue;};
        let sessions=manager.GetSessionEnumerator()?;
        for j in 0..sessions.GetCount()?{
            let control=sessions.GetSession(j)?;
            if control.GetState()?!=AudioSessionStateActive{continue;}
            let control2:IAudioSessionControl2=control.cast()?;
            if !crate::inventory::is_spotify_pid(control2.GetProcessId()?){continue;}
            let volume:ISimpleAudioVolume=control.cast()?;
            if !volume.GetMute()?.as_bool() && volume.GetMasterVolume()?>0.00001{return Ok(true);}
        }
    }Ok(false)
}}

#[cfg(test)]mod tests{use super::*;#[test]fn unavailable_state_never_invents_playback(){let s=State::default();assert!(!s.playing);assert!(!s.available);}}
