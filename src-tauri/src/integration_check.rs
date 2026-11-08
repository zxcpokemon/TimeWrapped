//! Explicit isolated smoke check, never run by normal application startup.
use crate::extensions::{Db,now};
use serde_json::{Value,json};
use std::{io::{Read,Write},net::TcpStream,time::Duration};
type Result<T>=std::result::Result<T,String>;
fn request(address:&str,token:&str,path:&str,body:Option<&str>)->Result<String>{let mut socket=TcpStream::connect(address).map_err(|e|e.to_string())?;socket.set_read_timeout(Some(Duration::from_secs(4))).map_err(|e|e.to_string())?;let method=if body.is_some(){"POST"}else{"GET"};let body=body.unwrap_or("");let message=format!("{method} /{path} HTTP/1.1\r\nHost: {address}\r\nOrigin: chrome-extension://selfcheck\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());socket.write_all(message.as_bytes()).map_err(|e|e.to_string())?;let mut response=String::new();socket.read_to_string(&mut response).map_err(|e|e.to_string())?;Ok(response)}
pub fn browser_and_genres(db:&Db)->Result<Value>{browser_check(db,true)}
pub fn browser_counts(db:&Db)->Result<Value>{browser_check(db,false)}
fn browser_check(db:&Db,online:bool)->Result<Value>{
    if std::env::var_os("TIMEWRAPPED_DATA_DIR").is_none(){return Err("Isolated data directory required".into());}
    let mut c=db.config()?;c.sites_history=true;c.adult_stats=true;c.site_details=false;c.adult_domains=vec!["adult.example".into()];c.music_genres=true;c.genres_online=true;db.save(&c)?;
    db.set_option("bridge_address","")?;let _server=crate::bridge::Server::start();let mut address=String::new();for _ in 0..40{address=db.option("bridge_address")?.unwrap_or_default();if !address.is_empty(){break;}std::thread::sleep(Duration::from_millis(50));}if address.is_empty(){return Err("Browser bridge did not start".into());}
    let token=db.token()?;let unauthorized=request(&address,"wrong","browser-config",None)?.starts_with("HTTP/1.1 401");
    let configuration=request(&address,&token,"browser-config",None)?;let config_ok=configuration.starts_with("HTTP/1.1 200")&&configuration.contains("\"sites_history\":true");
    let changed=request(&address,&token,"browser-settings",Some("{\"soundcloud\":true}"))?;
    let settings_ok=changed.starts_with("HTTP/1.1 200")&&db.config()?.soundcloud;
    let bad=request(&address,&token,"browser-settings",Some("{\"steam_id\":\"bad\"}"))?.starts_with("HTTP/1.1 400");
    let unauthorized_settings=request(&address,"wrong","browser-settings",Some("{\"adult_stats\":false}"))?.starts_with("HTTP/1.1 401")&&db.config()?.adult_stats;
    let body=json!({"id":"selfcheck-browser-visit","url":"https://adult.example/watch?token=secret#fragment","title":"Private test title","active":true}).to_string();
    request(&address,&token,"visit",Some(&body))?;std::thread::sleep(Duration::from_millis(150));request(&address,&token,"visit",Some(&body))?;
    let today=chrono::Local::now().date_naive().to_string();let report=db.report(&today,&today)?;let private=report["recent"].as_array().and_then(|a|a.iter().find(|r|r["domain"]=="adult.example"));
    let redacted=private.is_some_and(|r|r["title"]==""&&r["path"]=="");
    let song=json!({"title":"Local regression check","artist":"TimeWrapped test","playing":true,"position":10.}).to_string();
    let music_ok=request(&address,&token,"soundcloud",Some(&song))?.starts_with("HTTP/1.1 200");
    std::thread::sleep(Duration::from_millis(200));
    request(&address,&token,"soundcloud",Some(&json!({"title":"Local regression check","artist":"TimeWrapped test","playing":true,"position":10.2}).to_string()))?;
    let tracks=db.tracks()?;let soundcloud=tracks["totals"].as_array().and_then(|a|a.iter().find(|r|r["service"]=="soundcloud")).and_then(|r|r["seconds"].as_f64()).unwrap_or(0.);
    request(&address,&token,"soundcloud",Some("{\"playing\":false}"))?;
    let (genres,classified)=if online {
        db.record(&crate::extensions::Track::new("spotify","Self-check track","Radiohead").unwrap(),1000,now())?;
        let genres=crate::genres::resolve_pending(1)?;let classified=db.artists()?.as_array().and_then(|a|a.iter().find(|r|r["artist"]=="Radiohead")).and_then(|a|a["genre"].as_str()).map(str::to_owned);(genres,classified)
    }else{(json!({}),None)};
    Ok(json!({"unauthorized_rejected":unauthorized,"browser_configuration":config_ok,"shared_settings":settings_ok,"invalid_setting_rejected":bad,"unauthorized_settings_rejected":unauthorized_settings,"site_details_redacted":redacted,"adult_seconds_recorded":report["adult_seconds"],"soundcloud_transport":music_ok,"soundcloud_seconds_recorded":soundcloud,"genre_lookup":genres,"identified_genre":classified}))
}
