//! Optional integrations, isolated from the screen-time database.
use std::{path::Path,time::Duration,sync::{Arc,atomic::{AtomicBool,Ordering}}};
use rusqlite::{Connection,OptionalExtension,params};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use windows_sys::Win32::{Security::Cryptography::*,Foundation::LocalFree};
type Result<T>=std::result::Result<T,String>;
pub const STEAM_BASE:u64=76561197960265728;
pub fn now()->i64{chrono::Utc::now().timestamp_millis()}
#[derive(Default,Clone,Serialize,Deserialize)]
#[serde(default)]
pub struct Config{pub spotify_titles:bool,pub soundcloud:bool,pub dota:bool,pub faceit:bool,pub steam_id:String,pub faceit_mode:String,pub sites_history:bool,pub site_details:bool,pub adult_stats:bool,pub adult_domains:Vec<String>,pub excluded_sites:Vec<String>,pub music_genres:bool,pub genres_online:bool}
pub struct Db{pub(crate) conn:Connection}
impl Db{
    pub fn open()->Result<Self>{Self::at(&crate::store::Store::default_path().with_file_name("extensions.sqlite3"))}
    pub(crate) fn at(path:&Path)->Result<Self>{
        if let Some(p)=path.parent().filter(|p|!p.as_os_str().is_empty()){std::fs::create_dir_all(p).map_err(|e|e.to_string())?;}
        let conn=Connection::open(path).map_err(|e|e.to_string())?;
        conn.busy_timeout(Duration::from_secs(4)).map_err(|e|e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
            CREATE TABLE IF NOT EXISTS options(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS secrets(key TEXT PRIMARY KEY,value BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS cache(service TEXT NOT NULL,identity TEXT NOT NULL,updated INTEGER NOT NULL DEFAULT 0,attempted INTEGER NOT NULL DEFAULT 0,value TEXT,error TEXT,PRIMARY KEY(service,identity));
            CREATE TABLE IF NOT EXISTS tracks(id INTEGER PRIMARY KEY,service TEXT NOT NULL,title TEXT NOT NULL,artist TEXT NOT NULL,started INTEGER NOT NULL,ended INTEGER NOT NULL,milliseconds INTEGER NOT NULL CHECK(milliseconds>0));
            CREATE INDEX IF NOT EXISTS tracks_time ON tracks(ended);").map_err(|e|e.to_string())?;
        crate::web_history::init(&conn)?;Ok(Self{conn})
    }
    pub fn option(&self,key:&str)->Result<Option<String>>{self.conn.query_row("SELECT value FROM options WHERE key=?1",[key],|r|r.get(0)).optional().map_err(|e|e.to_string())}
    pub fn set_option(&self,key:&str,value:&str)->Result<()>{self.conn.execute("INSERT INTO options VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value]).map(|_|()).map_err(|e|e.to_string())}
    pub fn config(&self)->Result<Config>{self.option("config")?.map(|s|serde_json::from_str(&s).map_err(|e|e.to_string())).unwrap_or(Ok(Config::default()))}
    pub fn save(&self,c:&Config)->Result<()>{if !c.steam_id.is_empty(){steam_number(&c.steam_id)?;}self.set_option("config",&serde_json::to_string(c).map_err(|e|e.to_string())?)}
    pub fn token(&self)->Result<String>{
        if let Some(v)=self.option("bridge_token")?{return Ok(v);}
        let mut bytes=[0u8;32];unsafe{if BCryptGenRandom(std::ptr::null_mut(),bytes.as_mut_ptr(),32,BCRYPT_USE_SYSTEM_PREFERRED_RNG)<0{return Err("Не удалось создать код подключения".into());}}
        let token=bytes.iter().map(|b|format!("{b:02x}")).collect::<String>();
        self.conn.execute("INSERT OR IGNORE INTO options VALUES('bridge_token',?1)",[&token]).map_err(|e|e.to_string())?;
        self.option("bridge_token")?.ok_or("Не создан код подключения".into())
    }
    pub fn set_key(&self,key:&str)->Result<()>{
        if key.is_empty(){self.conn.execute("DELETE FROM secrets WHERE key='faceit'",[]).map_err(|e|e.to_string())?;return Ok(());}
        if key.len()>512||key.chars().any(|c|c.is_whitespace()||c.is_control()){return Err("Некорректный API-ключ FACEIT".into());}
        let value=protect(key.as_bytes(),false)?;
        self.conn.execute("INSERT INTO secrets VALUES('faceit',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[value]).map(|_|()).map_err(|e|e.to_string())
    }
    pub fn key(&self)->Result<Option<String>>{let b:Option<Vec<u8>>=self.conn.query_row("SELECT value FROM secrets WHERE key='faceit'",[],|r|r.get(0)).optional().map_err(|e|e.to_string())?;b.map(|b|String::from_utf8(protect(&b,true)?).map_err(|e|e.to_string())).transpose()}
    pub fn cache(&self,service:&str,id:&str)->Value{self.conn.query_row("SELECT updated,attempted,value,error FROM cache WHERE service=?1 AND identity=?2",params![service,id],|r|{let raw:Option<String>=r.get(2)?;Ok(json!({"updated":r.get::<_,i64>(0)?,"attempted":r.get::<_,i64>(1)?,"data":raw.and_then(|s|serde_json::from_str::<Value>(&s).ok()),"error":r.get::<_,Option<String>>(3)?}))}).unwrap_or(json!({"updated":0,"data":null,"error":null}))}
    fn claim(&self,service:&str,id:&str,manual:bool)->Result<bool>{
        let stamp=now();let old=self.cache(service,id);let age=stamp-old["updated"].as_i64().unwrap_or(0);let since=stamp-old["attempted"].as_i64().unwrap_or(0);
        if since<(if manual{60_000}else{900_000})||(!manual&&age<900_000){return Ok(false);}
        let n=self.conn.execute("INSERT INTO cache(service,identity,attempted) VALUES(?1,?2,?3) ON CONFLICT(service,identity) DO UPDATE SET attempted=excluded.attempted WHERE cache.attempted<?4",params![service,id,stamp,stamp-60_000]).map_err(|e|e.to_string())?;Ok(n>0)
    }
    fn result(&self,service:&str,id:&str,result:Result<Value>)->Result<()>{match result{Ok(value)=>{self.conn.execute("UPDATE cache SET value=?3,updated=?4,error=NULL WHERE service=?1 AND identity=?2",params![service,id,value.to_string(),now()]).map_err(|e|e.to_string())?;},Err(error)=>{self.conn.execute("UPDATE cache SET error=?3 WHERE service=?1 AND identity=?2",params![service,id,error]).map_err(|e|e.to_string())?;}}Ok(())}
    pub fn record(&self,track:&Track,ms:u64,end:i64)->Result<()>{
        if ms==0||ms>if track.service=="soundcloud"{75_000}else{15_000}{return Ok(());}
        let start=end-ms as i64;
        let previous:Option<(i64,i64)>=self.conn.query_row("SELECT id,ended FROM tracks WHERE service=?1 AND title=?2 AND artist=?3 ORDER BY id DESC LIMIT 1",params![track.service,track.title,track.artist],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
        if let Some((id,ended))=previous.filter(|(_,ended)|(*ended-start).abs()<1500){let _=ended;self.conn.execute("UPDATE tracks SET ended=?2,milliseconds=milliseconds+?3 WHERE id=?1",params![id,end,ms]).map_err(|e|e.to_string())?;}
        else{self.conn.execute("INSERT INTO tracks(service,title,artist,started,ended,milliseconds) VALUES(?1,?2,?3,?4,?5,?6)",params![track.service,track.title,track.artist,start,end,ms]).map_err(|e|e.to_string())?;}
        Ok(())
    }
    pub fn tracks(&self)->Result<Value>{
        let rows=self.conn.prepare("SELECT service,title,artist,started,ended,milliseconds FROM tracks ORDER BY ended DESC LIMIT 100").map_err(|e|e.to_string())?.query_map([],|r|Ok(json!({"service":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"artist":r.get::<_,String>(2)?,"started":r.get::<_,i64>(3)?,"ended":r.get::<_,i64>(4)?,"seconds":r.get::<_,i64>(5)? as f64/1000.}))).map_err(|e|e.to_string())?.collect::<std::result::Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        let totals=self.conn.prepare("SELECT service,SUM(milliseconds)/1000.0 FROM tracks GROUP BY service").map_err(|e|e.to_string())?.query_map([],|r|Ok(json!({"service":r.get::<_,String>(0)?,"seconds":r.get::<_,f64>(1)?}))).map_err(|e|e.to_string())?.collect::<std::result::Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        Ok(json!({"recent":rows,"totals":totals}))
    }
    pub fn clear(&self)->Result<()>{self.conn.execute_batch("DELETE FROM tracks;DELETE FROM cache;DELETE FROM site_sessions;DELETE FROM site_visits;DELETE FROM artist_genres;PRAGMA wal_checkpoint(TRUNCATE);VACUUM;").map_err(|e|e.to_string())?;self.remove_history_backup()}
}
fn protect(bytes:&[u8],decode:bool)->Result<Vec<u8>>{unsafe{
    let input=CRYPT_INTEGER_BLOB{cbData:bytes.len()as u32,pbData:bytes.as_ptr()as _};let mut output=CRYPT_INTEGER_BLOB::default();
    let ok=if decode{CryptUnprotectData(&input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)}else{CryptProtectData(&input,std::ptr::null(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)};
    if ok==0{return Err(format!("Windows не смогла защитить/прочитать API-ключ текущего пользователя: {}",std::io::Error::last_os_error()));}
    let data=std::slice::from_raw_parts(output.pbData,output.cbData as usize).to_vec();LocalFree(output.pbData as _);Ok(data)
}}
pub fn steam_number(raw:&str)->Result<String>{let n=raw.parse::<u64>().map_err(|_|"Нужен SteamID64 или ссылка steamcommunity.com".to_string())?;if !(STEAM_BASE+1..=STEAM_BASE+u32::MAX as u64).contains(&n){return Err("Некорректный SteamID64".into());}Ok(n.to_string())}
pub fn resolve_profile(raw:&str)->Result<String>{
    let raw=raw.trim();if raw.bytes().all(|b|b.is_ascii_digit())&&!raw.is_empty(){return steam_number(raw);}
    let url=url::Url::parse(raw).map_err(|_|"Вставьте полную HTTPS-ссылку Steam-профиля или SteamID64".to_string())?;
    if url.scheme()!="https"||url.host_str()!=Some("steamcommunity.com")||url.port().is_some()||!url.username().is_empty()||url.password().is_some(){return Err("Поддерживается только https://steamcommunity.com/profiles/… или /id/…".into());}
    let parts:Vec<_>=url.path_segments().unwrap().filter(|s|!s.is_empty()).collect();
    if parts.len()!=2{return Err("Вставьте ссылку на профиль, без разделов games или inventory".into());}
    match parts[0]{"profiles"=>steam_number(parts[1]),"id" if parts[1].len()<=64&&parts[1].bytes().all(|c|c.is_ascii_alphanumeric()||c==b'_'||c==b'-')=>{
        let text=crate::net::get("steamcommunity.com",&format!("/id/{}/?xml=1",parts[1]),None)?;
        let value=text.split("<steamID64>").nth(1).and_then(|s|s.split("</steamID64>").next()).ok_or("Steam не вернул SteamID64. Вставьте числовую ссылку /profiles/…")?;steam_number(value.trim())
    },_=>Err("Неподдерживаемая ссылка Steam-профиля".into())}
}
pub fn state()->Result<Value>{let db=Db::open()?;let c=db.config()?;let token=db.token()?;Ok(json!({"config":c,"faceit_key_saved":db.key()?.is_some(),"bridge_token":token,"bridge_status":db.option("bridge_status")?,"soundcloud_last_seen":db.option("soundcloud_last_seen")?,"browser_last_seen":db.option("browser_last_seen")?,"music":db.tracks()?,"artists":db.artists()?,"dota":db.cache("dota",&c.steam_id),"faceit":db.cache("faceit",&faceit_identity(&c))}))}
pub fn save(mut c:Config,profile:Option<String>,key:Option<String>)->Result<Value>{
    if let Some(profile)=profile{c.steam_id=if profile.trim().is_empty(){String::new()}else{resolve_profile(&profile)?};}
    if (c.dota||c.faceit)&&c.steam_id.is_empty(){return Err("Сначала укажите Steam-профиль".into());}
    if !["","public","api"].contains(&c.faceit_mode.as_str()){return Err("Выберите публичный режим или API FACEIT".into());}
    c.adult_domains=normalize_domains(&c.adult_domains)?;c.excluded_sites=normalize_domains(&c.excluded_sites)?;
    let db=Db::open()?;if let Some(key)=key{db.set_key(key.trim())?;}db.save(&c)?;state()
}
pub fn refresh(manual:bool)->Result<Value>{
    let db=Db::open()?;let c=db.config()?;
    if !c.steam_id.is_empty(){
        if c.dota&&db.claim("dota",&c.steam_id,manual)?{db.result("dota",&c.steam_id,dota(&db,&c.steam_id))?;}
        if c.faceit{let identity=faceit_identity(&c);if db.claim("faceit",&identity,manual)?{let result=if c.faceit_mode=="api"{db.key()?.ok_or("Для режима FACEIT API нужен ключ. Выберите публичный режим для просмотра без ключа".into()).and_then(|key|faceit(&c.steam_id,&key))}else{crate::faceit_public::fetch(&c.steam_id)};db.result("faceit",&identity,result)?;}}
    }state()
}
fn faceit_identity(c:&Config)->String{format!("{}:{}",c.steam_id,if c.faceit_mode=="api"{"api"}else{"public"})}
fn normalize_domains(rows:&[String])->Result<Vec<String>>{if rows.len()>200{return Err("Максимум 200 доменов в списке".into());}let mut out=Vec::new();for raw in rows{if raw.trim().is_empty(){continue;}let host=crate::web_history::domain(raw).ok_or("Проверьте домены в списке")?;if !out.contains(&host){out.push(host);}}Ok(out)}
fn dota(db:&Db,id:&str)->Result<Value>{
    let account=steam_number(id)?.parse::<u64>().unwrap()-STEAM_BASE;
    let base=format!("/api/players/{account}");let get=|suffix:&str|crate::net::json("api.opendota.com",&format!("{base}{suffix}"),None);
    let profile=get("")?;let recent=get("/recentMatches")?;
    let wl=get("/wl").unwrap_or(Value::Null);let heroes=get("/heroes").unwrap_or(Value::Null);let peers=get("/peers").unwrap_or(Value::Null);
    let constants=if let Some(v)=db.option("heroes")?{serde_json::from_str::<Value>(&v).unwrap_or(Value::Null)}else{match crate::net::json("api.opendota.com","/api/heroes",None){Ok(v)=>{db.set_option("heroes",&v.to_string())?;v},Err(_)=>Value::Null}};
    Ok(json!({"account_id":account,"profile":profile,"recent":recent,"wl":wl,"heroes":heroes,"peers":peers,"hero_names":constants}))
}
fn faceit(id:&str,key:&str)->Result<Value>{
    steam_number(id)?;let get=|path:&str|crate::net::json("open.faceit.com",&format!("/data/v4{path}"),Some(key));
    let profile=get(&format!("/players?game=cs2&game_player_id={id}"))?;
    let player=profile["player_id"].as_str().filter(|s|!s.is_empty()&&s.len()<80&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')).ok_or("FACEIT не вернул player_id для CS2")?;
    let history=get(&format!("/players/{player}/history?game=cs2&offset=0&limit=20"))?;
    let stats=get(&format!("/players/{player}/stats/cs2")).unwrap_or(Value::Null);
    Ok(json!({"profile":profile,"history":history,"stats":stats}))
}
pub struct Worker(Arc<AtomicBool>);
impl Worker{pub fn start()->Self{let stop=Arc::new(AtomicBool::new(false));let flag=stop.clone();let _=std::thread::Builder::new().name("public-game-stats".into()).spawn(move||{while !flag.load(Ordering::Relaxed){let _=refresh(false);let _=crate::genres::resolve_pending(3);for _ in 0..60{if flag.load(Ordering::Relaxed){return;}std::thread::sleep(Duration::from_secs(1));}}});Self(stop)}}
impl Drop for Worker{fn drop(&mut self){self.0.store(true,Ordering::Relaxed);}}
#[derive(Clone,PartialEq,Eq)]pub struct Track{pub service:String,pub title:String,pub artist:String}
impl Track{pub fn new(service:&str,title:&str,artist:&str)->Option<Self>{let clean=|s:&str|s.chars().filter(|c|!c.is_control()).take(300).collect::<String>().trim().to_owned();let title=clean(title);if title.is_empty()||!["spotify","soundcloud"].contains(&service){None}else{Some(Self{service:service.into(),title,artist:clean(artist)})}}}
pub struct Journal{previous:Option<Track>,position:Option<f64>,awake:u64,wall:i64}
impl Journal{
    pub fn new()->Self{Self{previous:None,position:None,awake:awake_ms(),wall:now()}}
    pub fn sample(&mut self,db:&Db,current:Option<Track>,enabled:bool){let _=self.advance(db,current,enabled,None,false,now(),awake_ms());}
    pub fn sample_browser(&mut self,db:&Db,current:Option<Track>,enabled:bool,position:Option<f64>)->Result<u64>{self.advance(db,current,enabled,position,true,now(),awake_ms())}
    fn advance(&mut self,db:&Db,current:Option<Track>,enabled:bool,position:Option<f64>,browser:bool,wall:i64,awake:u64)->Result<u64>{
        let elapsed=awake.saturating_sub(self.awake);let wall_elapsed=wall-self.wall;
        let position=position.filter(|p|p.is_finite()&&*p>=0.0);
        let mut milliseconds=0;
        if enabled&&wall_elapsed>=0&&(wall_elapsed-elapsed as i64).abs()<1200&&self.previous.is_some(){
            if elapsed<=8_000 {milliseconds=elapsed;}
            else if browser&&elapsed<=75_000&&current==self.previous&&current.is_some(){
                // A delayed hidden-tab heartbeat is accepted only when the same
                // track's playhead actually advanced. Pauses/seeks/sleep do not
                // become invented listening time.
                if let (Some(before),Some(after))=(self.position,position){let progress=(after-before)*1000.;if progress>0.&&progress<=elapsed as f64+3000.{milliseconds=elapsed.min(progress.round() as u64);}}
            }
        }
        let old=self.previous.take();self.previous=if enabled{current}else{None};self.position=if enabled{position}else{None};self.awake=awake;self.wall=wall;
        if let Some(old)=old {if milliseconds>0 {db.record(&old,milliseconds,wall)?;}}
        Ok(milliseconds)
    }
}
pub fn awake_ms()->u64{let mut value=0;unsafe{windows_sys::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime(&mut value);}value/10_000}

#[cfg(test)]mod tests{
use super::*;
#[test]fn profile_input_cannot_be_used_as_an_arbitrary_url(){assert_eq!(resolve_profile("https://steamcommunity.com/profiles/76561198047011640/").unwrap(),"76561198047011640");for bad in ["https://evil.test/profiles/76561198047011640","https://steamcommunity.com@evil.test/id/name","http://steamcommunity.com/id/name","https://steamcommunity.com:8080/id/name","0","76561197960265728"]{assert!(resolve_profile(bad).is_err(),"{bad}");}}
#[test]fn disabled_extensions_do_not_collect_by_default(){let db=Db::at(Path::new(":memory:")).unwrap();let c=db.config().unwrap();assert!(!c.spotify_titles&&!c.soundcloud&&!c.dota&&!c.faceit);let token=db.token().unwrap();assert_eq!(token.len(),64);assert_eq!(db.token().unwrap(),token);}
#[test]fn music_sessions_merge_only_contiguous_matching_tracks(){let db=Db::at(Path::new(":memory:")).unwrap();let t=Track::new("spotify","Track","Artist").unwrap();db.record(&t,5000,10000).unwrap();db.record(&t,5000,15000).unwrap();db.record(&t,5000,30000).unwrap();let v=db.tracks().unwrap();assert_eq!(v["recent"].as_array().unwrap().len(),2);assert_eq!(v["totals"][0]["seconds"],15.);db.clear().unwrap();assert!(db.tracks().unwrap()["recent"].as_array().unwrap().is_empty());}
#[test]fn delayed_soundcloud_heartbeat_uses_real_playhead_progress_and_skips_pause_seek_and_sleep(){
 let db=Db::at(Path::new(":memory:")).unwrap();let track=Track::new("soundcloud","Song","Artist");
 let mut j=Journal::new();j.awake=0;j.wall=0;
 assert_eq!(j.advance(&db,track.clone(),true,Some(10.),true,0,0).unwrap(),0);
 assert_eq!(j.advance(&db,track.clone(),true,Some(40.),true,30000,30000).unwrap(),30000);
 // The player was paused for twenty seconds between delayed observations.
 assert_eq!(j.advance(&db,track.clone(),true,Some(50.),true,60000,60000).unwrap(),10000);
 assert_eq!(j.advance(&db,track.clone(),true,Some(50.),true,90000,90000).unwrap(),0);
 assert_eq!(j.advance(&db,track.clone(),true,Some(250.),true,120000,120000).unwrap(),0);
 assert_eq!(j.advance(&db,track.clone(),true,Some(270.),true,220000,140000).unwrap(),0);
 assert_eq!(db.tracks().unwrap()["totals"][0]["seconds"],40.);
}
#[test]fn legacy_heartbeat_without_playhead_stays_conservative_and_spotify_keeps_short_gap_limit(){
 let db=Db::at(Path::new(":memory:")).unwrap();let track=Track::new("soundcloud","Song","Artist");let mut j=Journal::new();j.wall=0;j.awake=0;
 j.advance(&db,track.clone(),true,None,true,0,0).unwrap();assert_eq!(j.advance(&db,track.clone(),true,None,true,30000,30000).unwrap(),0);
 assert_eq!(j.advance(&db,track.clone(),true,None,true,35000,35000).unwrap(),5000);
 assert_eq!(j.advance(&db,track.clone(),true,Some(70.),false,65000,65000).unwrap(),0);
 assert_eq!(j.advance(&db,track,false,Some(75.),true,70000,70000).unwrap(),0);
}
#[test]#[ignore="Requires an interactive Windows user profile for DPAPI; run explicitly outside a restricted test account"]fn api_key_is_protected_at_rest_and_not_in_configuration(){let db=Db::at(Path::new(":memory:")).unwrap();db.set_key("example-test-secret").unwrap();assert_eq!(db.key().unwrap().unwrap(),"example-test-secret");let raw:Vec<u8>=db.conn.query_row("SELECT value FROM secrets",[],|r|r.get(0)).unwrap();assert!(!raw.windows(19).any(|s|s==b"example-test-secret"));assert!(!serde_json::to_string(&db.config().unwrap()).unwrap().contains("secret"));db.set_key("").unwrap();assert!(db.key().unwrap().is_none());}
#[test]fn failed_sync_preserves_the_last_successful_snapshot(){let db=Db::at(Path::new(":memory:")).unwrap();assert!(db.claim("dota","id",true).unwrap());db.result("dota","id",Ok(json!({"real":true}))).unwrap();db.result("dota","id",Err("offline".into())).unwrap();let c=db.cache("dota","id");assert_eq!(c["data"]["real"],true);assert_eq!(c["error"],"offline");assert!(!db.claim("dota","id",true).unwrap());}
}
