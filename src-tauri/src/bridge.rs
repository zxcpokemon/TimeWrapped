//! Authenticated loopback receiver for the optional SoundCloud browser add-on.
use std::{io::{Read,Write},net::{TcpListener,TcpStream},sync::{Arc,atomic::{AtomicBool,Ordering}},time::Duration};
use crate::extensions::{Db,Journal,Track,now};
pub struct Server(Arc<AtomicBool>);
impl Server{
    pub fn start()->Self{
        let stop=Arc::new(AtomicBool::new(false));let flag=stop.clone();
        let _=std::thread::Builder::new().name("soundcloud-loopback".into()).spawn(move||{
            let address=if std::env::var_os("TIMEWRAPPED_DATA_DIR").is_some(){"127.0.0.1:0"}else{"127.0.0.1:38671"};
            let listener=match TcpListener::bind(address){Ok(l)=>l,Err(_)=>{if let Ok(db)=Db::open(){let _=db.set_option("bridge_status","Порт 38671 занят. Закройте другой экземпляр TimeWrapped и перезапустите приложение");}return;}};
            if listener.set_nonblocking(true).is_err(){return;}
            if let Ok(db)=Db::open(){let _=db.set_option("bridge_status","Готов к подключению браузерного дополнения");if let Ok(address)=listener.local_addr(){let _=db.set_option("bridge_address",&address.to_string());}}
            let mut journal=Journal::new();let mut websites=crate::web_history::Journal::new();
            while !flag.load(Ordering::Relaxed){match listener.accept(){Ok((mut stream,peer))=>{if peer.ip().is_loopback(){let _=serve(&mut stream,&mut journal,&mut websites);}},Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(200)),Err(_)=>break}}
        });Self(stop)
    }
}
impl Drop for Server{fn drop(&mut self){self.0.store(true,Ordering::Relaxed);}}
fn allowed_origin(value:&str)->bool{value.strip_prefix("chrome-extension://").or_else(||value.strip_prefix("moz-extension://")).is_some_and(|v|!v.is_empty()&&v.len()<100&&v.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'))}
fn response(stream:&mut TcpStream,code:u16,origin:&str,body:&str)->std::io::Result<()>{
    let cors=if allowed_origin(origin){format!("Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\nVary: Origin\r\n")}else{String::new()};
    write!(stream,"HTTP/1.1 {code} Result\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nConnection: close\r\n{cors}Content-Length: {}\r\n\r\n{body}",body.len())
}
fn serve(stream:&mut TcpStream,journal:&mut Journal,websites:&mut crate::web_history::Journal)->std::io::Result<()>{
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes=Vec::new();let mut buffer=[0u8;1024];let header_end;
    loop{let n=stream.read(&mut buffer)?;if n==0{return Ok(());}bytes.extend_from_slice(&buffer[..n]);if bytes.len()>8192{return response(stream,413,"","{}");}if let Some(i)=bytes.windows(4).position(|p|p==b"\r\n\r\n"){header_end=i+4;break;}}
    let text=String::from_utf8_lossy(&bytes[..header_end]).to_string();let mut lines=text.split("\r\n");let first=lines.next().unwrap_or("");
    let headers:std::collections::HashMap<_,_>=lines.filter_map(|s|s.split_once(':')).map(|(k,v)|(k.trim().to_ascii_lowercase(),v.trim().to_owned())).collect();
    let origin=headers.get("origin").map(String::as_str).unwrap_or("");
    if !origin.is_empty()&&!allowed_origin(origin){return response(stream,403,"","{}");}
    let host=headers.get("host").map(String::as_str).unwrap_or("");
    if host!=stream.local_addr()?.to_string(){return response(stream,403,origin,"{}");}
    if ["OPTIONS /soundcloud HTTP/1.1","OPTIONS /visit HTTP/1.1","OPTIONS /browser-config HTTP/1.1","OPTIONS /browser-settings HTTP/1.1"].contains(&first){return response(stream,204,origin,"");}
    if !["POST /soundcloud HTTP/1.1","POST /visit HTTP/1.1","GET /browser-config HTTP/1.1","POST /browser-settings HTTP/1.1"].contains(&first){return response(stream,404,origin,"{}");}
    let Ok(db)=Db::open()else{return response(stream,503,origin,"{}");};
    let token=db.token().unwrap_or_default();let auth=headers.get("authorization").map(String::as_str).unwrap_or("");
    if token.is_empty()||auth!=format!("Bearer {token}"){return response(stream,401,origin,"{\"error\":\"Неверный код подключения\"}");}
    if first=="GET /browser-config HTTP/1.1"{return response(stream,200,origin,&browser_config(&db).to_string());}
    let length=headers.get("content-length").and_then(|v|v.parse::<usize>().ok()).filter(|n|*n<=4096);
    let Some(length)=length else{return response(stream,400,origin,"{}");};
    while bytes.len()<header_end+length{let n=stream.read(&mut buffer)?;if n==0{return Ok(());}bytes.extend_from_slice(&buffer[..n]);if bytes.len()>12288{return response(stream,413,origin,"{}");}}
    let Ok(value)=serde_json::from_slice::<serde_json::Value>(&bytes[header_end..header_end+length])else{return response(stream,400,origin,"{}");};
    if first=="POST /browser-settings HTTP/1.1" {
        let Ok(c)=browser_settings(db.config().unwrap_or_default(),&value)else{return response(stream,400,origin,"{}");};
        if db.save(&c).is_err(){return response(stream,503,origin,"{}");}
        return response(stream,200,origin,&browser_config(&db).to_string());
    }
    let config=db.config().unwrap_or_default();let tracking=crate::store::Store::open(&crate::store::Store::default_path()).and_then(|s|s.settings()).is_ok_and(|s|s.tracking_enabled);
    if first=="POST /visit HTTP/1.1"{let Ok(visit)=serde_json::from_value::<crate::web_history::Visit>(value)else{return response(stream,400,origin,"{}");};websites.sample(&db,visit,tracking&&!crate::agent::session_is_blocked().unwrap_or(true));let _=db.set_option("browser_last_seen",&now().to_string());return response(stream,200,origin,"{\"ok\":true}");}
    let enabled=config.soundcloud&&tracking;
    let current=if value["playing"].as_bool()==Some(true){Track::new("soundcloud",value["title"].as_str().unwrap_or(""),value["artist"].as_str().unwrap_or(""))}else{None};
    let recorded=journal.sample_browser(&db,current,enabled,value["position"].as_f64());
    if let Err(error)=recorded {let _=db.set_option("soundcloud_error",&error);return response(stream,503,origin,"{\"error\":\"Не удалось записать музыку\"}");}
    let _=db.set_option("soundcloud_error","");let _=db.set_option("soundcloud_last_seen",&now().to_string());
    response(stream,200,origin,if enabled{"{\"ok\":true}"}else{"{\"ok\":true,\"paused\":true}"})
}
fn browser_config(db:&Db)->serde_json::Value {
    let c=db.config().unwrap_or_default();
    let tracking=crate::store::Store::open(&crate::store::Store::default_path()).and_then(|s|s.settings()).is_ok_and(|s|s.tracking_enabled);
    serde_json::json!({"language":crate::store::Store::open(&crate::store::Store::default_path()).and_then(|s|s.settings()).map(|s|s.language).unwrap_or_else(|_|"ru".into()),"version":env!("CARGO_PKG_VERSION"),"tracking_enabled":tracking,"sites_history":c.sites_history,"site_details":c.site_details,"adult_stats":c.adult_stats,"soundcloud":c.soundcloud,"spotify_titles":c.spotify_titles})
}
fn browser_settings(mut c:crate::extensions::Config,value:&serde_json::Value)->Result<crate::extensions::Config,()> {
    let fields=value.as_object().ok_or(())?;
    if fields.is_empty() || fields.keys().any(|k|!["sites_history","soundcloud","site_details","adult_stats","spotify_titles"].contains(&k.as_str())) {return Err(());}
    for (key,value) in fields {let enabled=value.as_bool().ok_or(())?;match key.as_str(){"sites_history"=>c.sites_history=enabled,"soundcloud"=>c.soundcloud=enabled,"site_details"=>c.site_details=enabled,"adult_stats"=>c.adult_stats=enabled,"spotify_titles"=>c.spotify_titles=enabled,_=>return Err(())}}
    Ok(c)
}
pub fn browser_zip()->Vec<u8>{
    let files=[("i18n.mjs",include_bytes!("../../browser-extension/i18n.mjs").as_slice()),("translations.mjs",include_bytes!("../../browser-extension/translations.mjs").as_slice()),("popup.html",include_bytes!("../../browser-extension/popup.html").as_slice()),("icon.png",include_bytes!("../../browser-extension/icon.png").as_slice()),("history.js",include_bytes!("../../browser-extension/history.js").as_slice()),("browser-model.mjs",include_bytes!("../../browser-extension/browser-model.mjs").as_slice()),("manifest.json",include_bytes!("../../browser-extension/manifest.json").as_slice()),("content.js",include_bytes!("../../browser-extension/content.js").as_slice()),("background.js",include_bytes!("../../browser-extension/background.js").as_slice()),("options.html",include_bytes!("../../browser-extension/options.html").as_slice()),("options.js",include_bytes!("../../browser-extension/options.js").as_slice()),("options.css",include_bytes!("../../browser-extension/options.css").as_slice()),("README.md",include_bytes!("../../browser-extension/README.md").as_slice())];
    let mut out=Vec::new();let mut directory=Vec::new();
    fn u16v(v:&mut Vec<u8>,n:u16){v.extend_from_slice(&n.to_le_bytes());}fn u32v(v:&mut Vec<u8>,n:u32){v.extend_from_slice(&n.to_le_bytes());}
    for(name,data)in files{let offset=out.len()as u32;let mut crc=!0u32;for b in data{crc^=*b as u32;for _ in 0..8{crc=(crc>>1)^if crc&1!=0{0xedb88320}else{0};}}crc=!crc;
        u32v(&mut out,0x04034b50);for n in [20,0,0,0,33]{u16v(&mut out,n);}u32v(&mut out,crc);u32v(&mut out,data.len()as u32);u32v(&mut out,data.len()as u32);u16v(&mut out,name.len()as u16);u16v(&mut out,0);out.extend_from_slice(name.as_bytes());out.extend_from_slice(data);
        u32v(&mut directory,0x02014b50);for n in [20,20,0,0,0,33]{u16v(&mut directory,n);}u32v(&mut directory,crc);u32v(&mut directory,data.len()as u32);u32v(&mut directory,data.len()as u32);u16v(&mut directory,name.len()as u16);for _ in 0..4{u16v(&mut directory,0);}u32v(&mut directory,0);u32v(&mut directory,offset);directory.extend_from_slice(name.as_bytes());
    }
    let start=out.len()as u32;let length=directory.len()as u32;out.extend_from_slice(&directory);u32v(&mut out,0x06054b50);u16v(&mut out,0);u16v(&mut out,0);u16v(&mut out,files.len()as u16);u16v(&mut out,files.len()as u16);u32v(&mut out,length);u32v(&mut out,start);u16v(&mut out,0);out
}
#[cfg(test)]mod tests{use super::*;#[test]fn ordinary_web_pages_cannot_use_browser_cors(){assert!(allowed_origin("chrome-extension://abcdefghijklmnop"));for s in ["https://soundcloud.com","http://localhost","chrome-extension://id.evil.test","null"]{assert!(!allowed_origin(s));}}#[test]fn addon_archive_has_zip_headers(){let v=browser_zip();assert_eq!(&v[..4],b"PK\x03\x04");assert!(v.windows(13).any(|v|v==b"manifest.json"));}}
