//! Local, opt-in browsing sessions. No requests to visited sites are made here.
use crate::extensions::{Db,now,awake_ms};
use rusqlite::{params,OptionalExtension};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use chrono::{Local,NaiveDate,TimeZone,Duration};
type Result<T>=std::result::Result<T,String>;
const ADULT:&[&str]=&["pornhub.com","pornhub.org","pornhubpremium.com","xvideos.com","xnxx.com","xhamster.com","youporn.com","redtube.com","spankbang.com","tube8.com","chaturbate.com","stripchat.com","bongacams.com","rule34.xxx","e621.net","nhentai.net"];
pub fn init(conn:&rusqlite::Connection)->Result<()>{
 conn.execute_batch("CREATE TABLE IF NOT EXISTS site_sessions(id TEXT PRIMARY KEY,domain TEXT NOT NULL,site TEXT NOT NULL,adult INTEGER NOT NULL,path TEXT NOT NULL,title TEXT NOT NULL,started INTEGER NOT NULL,ended INTEGER NOT NULL,milliseconds INTEGER NOT NULL);CREATE INDEX IF NOT EXISTS sites_time ON site_sessions(ended);CREATE INDEX IF NOT EXISTS sites_domain ON site_sessions(domain,started);CREATE TABLE IF NOT EXISTS artist_genres(artist TEXT PRIMARY KEY,genre TEXT NOT NULL,source TEXT NOT NULL,updated INTEGER NOT NULL);
 CREATE TABLE IF NOT EXISTS site_visits(id TEXT PRIMARY KEY,domain TEXT NOT NULL,site TEXT NOT NULL,adult INTEGER NOT NULL,path TEXT NOT NULL,title TEXT NOT NULL,url TEXT NOT NULL,visited INTEGER NOT NULL,last_seen INTEGER NOT NULL,page_key TEXT NOT NULL);
 CREATE INDEX IF NOT EXISTS visits_time ON site_visits(visited DESC);CREATE INDEX IF NOT EXISTS visits_page ON site_visits(page_key,last_seen);").map_err(|e|e.to_string())?;
 let migrated:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM options WHERE key='browser_visits_v1')",[],|r|r.get(0)).map_err(|e|e.to_string())?;
 if !migrated {
  if let Some(path)=conn.path().filter(|p|*p!=":memory:") {
   let backup=std::path::Path::new(path).with_extension("pre-v1.1.sqlite3");
   let count:i64=conn.query_row("SELECT COUNT(*) FROM site_sessions",[],|r|r.get(0)).map_err(|e|e.to_string())?;
   if count>0&&!backup.exists(){conn.execute("VACUUM INTO ?1",[backup.to_string_lossy().as_ref()]).map_err(|e|e.to_string())?;}
  }
  conn.execute_batch("BEGIN IMMEDIATE").map_err(|e|e.to_string())?;
  let result=(||->Result<()>{
   let claimed=conn.execute("INSERT OR IGNORE INTO options(key,value) VALUES('browser_visits_v1','1')",[]).map_err(|e|e.to_string())?;
   if claimed==0{return Ok(());}
   let mut stmt=conn.prepare("SELECT rowid,domain,site,adult,path,title,started,ended FROM site_sessions ORDER BY started,rowid").map_err(|e|e.to_string())?;
   let rows=stmt.query_map([],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,bool>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,i64>(6)?,r.get::<_,i64>(7)?))).map_err(|e|e.to_string())?.collect::<std::result::Result<Vec<_>,_>>().map_err(|e|e.to_string())?;drop(stmt);
   let mut pages=std::collections::HashMap::<String,(String,i64)>::new();
   for (row,domain,site,adult,path,title,start,end) in rows {
    let key=page_key(&domain,&path,&title);let url=format!("https://{domain}{path}");
    let prior=pages.get(&key).filter(|(_,last)|start-*last<=30*60*1000).map(|v|v.0.clone());
    let id=prior.unwrap_or_else(||format!("legacy-{row}"));
    conn.execute("INSERT INTO site_visits VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(id) DO UPDATE SET last_seen=MAX(last_seen,excluded.last_seen),path=excluded.path,url=excluded.url",params![id,domain,site,adult,path,title,url,start,end,key]).map_err(|e|e.to_string())?;
    let last=pages.get(&key).map(|v|v.1).unwrap_or(end).max(end);pages.insert(key,(id,last));
   }Ok(())
  })();
  match result {Ok(())=>conn.execute_batch("COMMIT").map_err(|e|e.to_string())?,Err(e)=>{let _=conn.execute_batch("ROLLBACK");return Err(e);}}
 }
 Ok(())
}
fn page_key(domain:&str,path:&str,title:&str)->String{format!("{domain}|{}",if title.is_empty(){path}else{title})}

pub fn domain(raw:&str)->Option<String>{let value=if raw.contains("://"){raw.to_owned()}else{format!("https://{}",raw.trim())};let u=url::Url::parse(&value).ok()?;if !["http","https"].contains(&u.scheme())||!u.username().is_empty()||u.password().is_some(){return None;}let h=u.host_str()?.trim_end_matches('.').to_ascii_lowercase();let h=h.strip_prefix("www.").unwrap_or(&h).to_string();if !h.contains('.')||h.parse::<std::net::IpAddr>().is_ok()||h.ends_with(".localhost")||h.ends_with(".local"){None}else{Some(h)}}
fn matches(host:&str,base:&str)->bool{host==base||host.strip_suffix(base).is_some_and(|prefix|prefix.ends_with('.'))}
pub fn is_adult(host:&str,extra:&[String])->bool{ADULT.iter().any(|d|matches(host,d))||extra.iter().any(|d|matches(host,d))}
pub fn site_name(host:&str,adult:bool)->String{
    for(base,name)in [("youtube.com","YouTube"),("youtu.be","YouTube"),("twitch.tv","Twitch"),("tiktok.com","TikTok"),("instagram.com","Instagram"),("reddit.com","Reddit"),("vk.com","VK"),("telegram.org","Telegram"),("web.telegram.org","Telegram"),("discord.com","Discord"),("github.com","GitHub"),("steamcommunity.com","Steam"),("faceit.com","FACEIT"),("pornhub.com","Pornhub")]{if matches(host,base){return name.into();}}
    if adult{host.to_owned()}else{host.to_owned()}
}
#[derive(Clone,Debug,Deserialize,Serialize)]pub struct Visit{pub id:String,pub url:String,#[serde(default)]pub title:String,#[serde(default)]pub active:bool,#[serde(default)]pub navigation:bool}
#[derive(Clone)]struct CleanVisit{id:String,domain:String,site:String,adult:bool,path:String,title:String}
fn clean(v:&Visit,c:&crate::extensions::Config)->Option<CleanVisit>{
    if !c.sites_history||v.id.is_empty()||v.id.len()>100||!v.id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'){return None;}
    let host=domain(&v.url)?;
    if c.excluded_sites.iter().any(|d|matches(&host,d)){return None;}
    let adult=is_adult(&host,&c.adult_domains);if adult&&!c.adult_stats{return None;}
    let (path,title)=if c.site_details{let u=url::Url::parse(&v.url).ok()?;(page_path(&u),v.title.chars().filter(|c|!c.is_control()).take(250).collect())}else{(String::new(),String::new())};
    Some(CleanVisit{id:v.id.clone(),site:site_name(&host,adult),domain:host,adult,path,title})
}
fn page_path(u:&url::Url)->String{
 let mut path=u.path().chars().take(500).collect::<String>();
 if u.host_str().is_some_and(|h|matches(h,"youtube.com"))&&path=="/watch"{
  if let Some((_,video))=u.query_pairs().find(|(key,value)|key=="v"&&value.len()==11&&value.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')){path.push_str("?v=");path.push_str(&video);}
 }path
}
pub struct Journal{previous:Option<CleanVisit>,wall:i64,awake:u64}
impl Journal{
    pub fn new()->Self{Self{previous:None,wall:now(),awake:awake_ms()}}
    pub fn sample(&mut self,db:&Db,v:Visit,enabled:bool){
        let config=db.config().unwrap_or_default();let wall=now();let awake=awake_ms();let elapsed=awake.saturating_sub(self.awake);let same_paused=self.previous.as_ref().is_some_and(|p|p.id==v.id);
        if enabled {let _=db.record_browser_visit(&v,&config,wall);}
        if !v.active&&!same_paused{return;}
        let current=if v.active{clean(&v,&config)}else{None};
        if enabled&&config.sites_history&&elapsed<=8000&&(wall-self.wall-elapsed as i64).abs()<1200{
            if let Some(previous)=&self.previous{if (!previous.adult||config.adult_stats)&&!config.excluded_sites.iter().any(|h|matches(&previous.domain,h)){let _=db.record_visit(previous,elapsed,wall);}}
        }
        self.previous=if enabled{current}else{None};self.wall=wall;self.awake=awake;
    }
}
impl Db{
    fn record_visit(&self,v:&CleanVisit,ms:u64,end:i64)->Result<()>{
        if ms==0||ms>8000{return Ok(());}let start=end-ms as i64;
        let latest:Option<(String,i64)>=self.conn.query_row("SELECT id,ended FROM site_sessions WHERE id=?1 OR id GLOB ?2 ORDER BY rowid DESC LIMIT 1",params![v.id,format!("{}:*",v.id)],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
        let id=match latest{Some((id,ended))if (ended-start).abs()<250&&end>=ended=>id,Some(_)=>format!("{}:{start}",v.id),None=>v.id.clone()};
        self.conn.execute("INSERT INTO site_sessions(id,domain,site,adult,path,title,started,ended,milliseconds) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET ended=excluded.ended,milliseconds=site_sessions.milliseconds+excluded.milliseconds,path=excluded.path,title=excluded.title",params![id,v.domain,v.site,v.adult,v.path,v.title,start,end,ms]).map(|_|()).map_err(|e|e.to_string())
    }
    fn record_browser_visit(&self,v:&Visit,c:&crate::extensions::Config,at:i64)->Result<()>{
     let Some(clean)=clean(v,c)else{return Ok(());};
     let exists:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM site_visits WHERE id=?1)",[&v.id],|r|r.get(0)).map_err(|e|e.to_string())?;
     if !exists&&!v.active&&!v.navigation{return Ok(());}
     let key=page_key(&clean.domain,&clean.path,&clean.title);
     let prior:Option<String>=if !v.navigation&&!exists {self.conn.query_row("SELECT id FROM site_visits WHERE page_key=?1 AND last_seen>=?2 ORDER BY last_seen DESC LIMIT 1",params![key,at-30*60*1000],|r|r.get(0)).optional().map_err(|e|e.to_string())?}else{None};
     let id=prior.as_deref().unwrap_or(&v.id);
     let parsed=url::Url::parse(&v.url).map_err(|e|e.to_string())?;let url=format!("{}{}",parsed.origin().ascii_serialization(),clean.path);
     self.conn.execute("INSERT INTO site_visits VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8,?9) ON CONFLICT(id) DO UPDATE SET last_seen=MAX(last_seen,excluded.last_seen),title=CASE WHEN excluded.title='' THEN title ELSE excluded.title END,path=excluded.path,url=excluded.url,page_key=excluded.page_key",params![id,clean.domain,clean.site,clean.adult,clean.path,clean.title,url,at,key]).map(|_|()).map_err(|e|e.to_string())
    }
    pub fn visit_url(&self,id:&str)->Result<String>{self.conn.query_row("SELECT url FROM site_visits WHERE id=?1",[id],|r|r.get(0)).map_err(|_|"Посещение не найдено".into())}
    pub fn delete_sites(&self,adult_only:bool)->Result<()>{
     for table in ["site_sessions","site_visits"] {self.conn.execute(&format!("DELETE FROM {table}{}",if adult_only{" WHERE adult=1"}else{""}),[]).map_err(|e|e.to_string())?;}
     self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);VACUUM;").map_err(|e|e.to_string())?;
     self.remove_history_backup()
    }
    pub(crate) fn remove_history_backup(&self)->Result<()>{if let Some(path)=self.conn.path().filter(|p|*p!=":memory:"){let backup=std::path::Path::new(path).with_extension("pre-v1.1.sqlite3");match std::fs::remove_file(backup){Ok(())=>{},Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string())}}Ok(())}
    pub fn set_genre(&self,artist:&str,genre:&str,source:&str)->Result<()>{let artist=artist.trim();let genre=genre.trim();if artist.is_empty()||artist.chars().count()>300||genre.chars().count()>60||genre.chars().any(char::is_control){return Err("Проверьте исполнителя и жанр".into());}self.conn.execute("INSERT INTO artist_genres VALUES(?1,?2,?3,?4) ON CONFLICT(artist) DO UPDATE SET genre=excluded.genre,source=excluded.source,updated=excluded.updated",params![artist,genre,source,now()]).map(|_|()).map_err(|e|e.to_string())}
    pub fn artists(&self)->Result<Value>{self.conn.prepare("SELECT t.artist,SUM(t.milliseconds)/1000.0,g.genre,g.source FROM tracks t LEFT JOIN artist_genres g ON t.artist=g.artist WHERE t.artist!='' GROUP BY t.artist ORDER BY SUM(t.milliseconds) DESC LIMIT 100").map_err(|e|e.to_string())?.query_map([],|r|Ok(json!({"artist":r.get::<_,String>(0)?,"seconds":r.get::<_,f64>(1)?,"genre":r.get::<_,Option<String>>(2)?,"source":r.get::<_,Option<String>>(3)?}))).map_err(|e|e.to_string())?.collect::<std::result::Result<Vec<_>,_>>().map(Value::Array).map_err(|e|e.to_string())}
    pub fn report(&self,from:&str,to:&str)->Result<Value>{
        self.report_filtered(from,to,None,false)
    }
    pub fn report_filtered(&self,from:&str,to:&str,site:Option<&str>,adult_only:bool)->Result<Value>{
        self.report_history(from,to,site,adult_only,None)
    }
    pub fn report_history(&self,from:&str,to:&str,site:Option<&str>,adult_only:bool,search:Option<&str>)->Result<Value>{
        let needle=search.unwrap_or("").trim().to_lowercase();let mut history_total=0usize;
        let (start,end)=range(from,to)?;let mut sites=std::collections::BTreeMap::<String,(String,bool,f64,u64)>::new();let mut days=std::collections::BTreeMap::<String,(f64,f64)>::new();let mut recent=Vec::new();let mut total=0.;let mut adult=0.;let mut adult_visits=0u64;
        let mut stmt=self.conn.prepare("SELECT domain,site,adult,path,title,started,ended,milliseconds FROM site_sessions WHERE ended>?1 AND started<?2 ORDER BY ended DESC").map_err(|e|e.to_string())?;
        let mut rows=stmt.query(params![start,end]).map_err(|e|e.to_string())?;
        while let Some(r)=rows.next().map_err(|e|e.to_string())?{
            let host:String=r.get(0).map_err(|e|e.to_string())?;let name:String=r.get(1).map_err(|e|e.to_string())?;let is_adult:bool=r.get(2).map_err(|e|e.to_string())?;let a:i64=r.get(5).map_err(|e|e.to_string())?;let b:i64=r.get(6).map_err(|e|e.to_string())?;let duration:f64=r.get::<_,i64>(7).map_err(|e|e.to_string())? as f64;
            let left=a.max(start);let right=b.min(end);let seconds=duration*(right-left)as f64/(b-a).max(1)as f64/1000.;total+=seconds;if is_adult{adult+=seconds;}
            let entry=sites.entry(host.clone()).or_insert((name.clone(),is_adult,0.,0));entry.2+=seconds;
            split_days(left,right,duration/(b-a).max(1)as f64,|date,s|{let v=days.entry(date).or_default();v.0+=s;if is_adult{v.1+=s;}})?;

        }
        let mut stmt=self.conn.prepare("SELECT id,domain,site,adult,path,title,visited FROM site_visits WHERE visited>=?1 AND visited<?2 ORDER BY visited DESC,rowid DESC").map_err(|e|e.to_string())?;
        let mut rows=stmt.query(params![start,end]).map_err(|e|e.to_string())?;
        while let Some(r)=rows.next().map_err(|e|e.to_string())?{
         let id:String=r.get(0).map_err(|e|e.to_string())?;let host:String=r.get(1).map_err(|e|e.to_string())?;let name:String=r.get(2).map_err(|e|e.to_string())?;let is_adult:bool=r.get(3).map_err(|e|e.to_string())?;
         let entry=sites.entry(host.clone()).or_insert((name.clone(),is_adult,0.,0));entry.3+=1;if is_adult{adult_visits+=1;}
         let path:String=r.get(4).map_err(|e|e.to_string())?;let title:String=r.get(5).map_err(|e|e.to_string())?;
         if (!adult_only||is_adult)&&site.is_none_or(|s|s==name)&&format!("{host} {title} {path}").to_lowercase().contains(&needle){history_total+=1;if recent.len()<300{recent.push(json!({"id":id,"domain":host,"site":name,"adult":is_adult,"path":path,"title":title,"started":r.get::<_,i64>(6).map_err(|e|e.to_string())?}));}}

        }
        let mut genres=std::collections::BTreeMap::<String,f64>::new();let mut music=0.;let mut identified=0.;
        let mut stmt=self.conn.prepare("SELECT t.started,t.ended,t.milliseconds,g.genre FROM tracks t LEFT JOIN artist_genres g ON t.artist=g.artist WHERE t.ended>?1 AND t.started<?2").map_err(|e|e.to_string())?;let mut rows=stmt.query(params![start,end]).map_err(|e|e.to_string())?;
        while let Some(r)=rows.next().map_err(|e|e.to_string())?{let a:i64=r.get(0).map_err(|e|e.to_string())?;let b:i64=r.get(1).map_err(|e|e.to_string())?;let ms:i64=r.get(2).map_err(|e|e.to_string())?;let genre:Option<String>=r.get(3).map_err(|e|e.to_string())?;let seconds=ms as f64*(b.min(end)-a.max(start))as f64/(b-a).max(1)as f64/1000.;music+=seconds;let genre=genre.filter(|s|!s.is_empty());if genre.is_some(){identified+=seconds;}*genres.entry(genre.unwrap_or("Не определён".into())).or_default()+=seconds;}
        let mut site_rows:Vec<_>=sites.into_iter().map(|(domain,(site,adult,seconds,visits))|json!({"domain":domain,"site":site,"adult":adult,"seconds":seconds,"visits":visits})).collect();site_rows.sort_by(|a,b|b["seconds"].as_f64().unwrap_or(0.).total_cmp(&a["seconds"].as_f64().unwrap_or(0.)));
        let mut genre_rows:Vec<_>=genres.into_iter().map(|(genre,seconds)|json!({"genre":genre,"seconds":seconds})).collect();genre_rows.sort_by(|a,b|b["seconds"].as_f64().unwrap_or(0.).total_cmp(&a["seconds"].as_f64().unwrap_or(0.)));
        Ok(json!({"from":from,"to":to,"total_seconds":total,"adult_seconds":adult,"adult_visits":adult_visits,"sites":site_rows,"recent":recent,"history_total":history_total,"days":days.into_iter().map(|(date,(seconds,adult))|json!({"date":date,"seconds":seconds,"adult_seconds":adult})).collect::<Vec<_>>(),"genres":genre_rows,"music_seconds":music,"identified_music_seconds":identified,"config":self.config()?}))
    }
}
fn range(from:&str,to:&str)->Result<(i64,i64)>{let parse=|s:&str|NaiveDate::parse_from_str(s,"%Y-%m-%d").map_err(|_|"Некорректный период".to_string());let a=parse(from)?;let b=parse(to)?;if b<a||b-a>Duration::days(366){return Err("Выберите период до 366 дней".into());}let stamp=|d:NaiveDate|Local.from_local_datetime(&d.and_hms_opt(0,0,0).unwrap()).earliest().map(|v|v.timestamp_millis()).ok_or("Не удалось определить локальную дату".to_string());Ok((stamp(a)?,stamp(b+Duration::days(1))?))}
fn split_days(mut a:i64,b:i64,ratio:f64,mut add:impl FnMut(String,f64))->Result<()>{while a<b{let date=chrono::DateTime::from_timestamp_millis(a).ok_or("Дата вне диапазона")?.with_timezone(&Local).date_naive();let next=Local.from_local_datetime(&(date+Duration::days(1)).and_hms_opt(0,0,0).unwrap()).earliest().ok_or("Не определена полночь")?.timestamp_millis().min(b);if next<=a{return Err("Некорректная граница суток".into());}add(date.to_string(),(next-a)as f64*ratio/1000.);a=next;}Ok(())}
#[cfg(test)]mod tests{
use super::*;
fn sample_visit(id:&str,title:&str,navigation:bool)->Visit{Visit{id:id.into(),url:"https://docs.google.com/forms/test?token=secret#private".into(),title:title.into(),active:true,navigation}}
#[test]fn youtube_reopen_url_keeps_only_the_video_identifier(){
 let mut c=crate::extensions::Config{sites_history:true,site_details:true,..Default::default()};let mut v=sample_visit("video","Video",true);v.url="https://www.youtube.com/watch?v=abcDEF_1234&token=secret&si=private#fragment".into();
 assert_eq!(clean(&v,&c).unwrap().path,"/watch?v=abcDEF_1234");v.url="https://youtube.com.evil.test/watch?v=abcDEF_1234".into();assert_eq!(clean(&v,&c).unwrap().path,"/watch");c.site_details=false;assert_eq!(clean(&v,&c).unwrap().path,"");
}
#[test]fn focus_returns_keep_one_history_entry_while_site_time_stays_exact(){
 let db=Db::at(std::path::Path::new(":memory:")).unwrap();let c=crate::extensions::Config{sites_history:true,site_details:true,..Default::default()};db.save(&c).unwrap();let(a,_)=range("2026-01-05","2026-01-05").unwrap();
 let v=sample_visit("page-1","Тест 3",true);db.record_browser_visit(&v,&c,a).unwrap();let clean=clean(&v,&c).unwrap();
 db.record_visit(&clean,5000,a+5000).unwrap();let mut heartbeat=v.clone();heartbeat.navigation=false;
 db.record_browser_visit(&heartbeat,&c,a+60000).unwrap();db.record_visit(&clean,5000,a+65000).unwrap();
 let r=db.report("2026-01-05","2026-01-05").unwrap();assert_eq!(r["total_seconds"],10.);assert_eq!(r["history_total"],1);assert_eq!(r["recent"][0]["started"],a);assert!(r["recent"][0].get("seconds").is_none());
 assert_eq!(db.visit_url("page-1").unwrap(),"https://docs.google.com/forms/test");
 assert_eq!(db.report_history("2026-01-05","2026-01-05",None,false,Some("ТЕСТ")).unwrap()["history_total"],1);
}
#[test]fn new_navigation_is_a_visit_even_in_background_and_title_updates_do_not_duplicate_it(){
 let db=Db::at(std::path::Path::new(":memory:")).unwrap();let c=crate::extensions::Config{sites_history:true,site_details:true,..Default::default()};db.save(&c).unwrap();let(a,_)=range("2026-01-05","2026-01-05").unwrap();
 let mut v=sample_visit("page-1","Loading",true);v.active=false;db.record_browser_visit(&v,&c,a).unwrap();v.title="Final title".into();v.navigation=false;db.record_browser_visit(&v,&c,a+1000).unwrap();
 let next=sample_visit("page-2","Final title",true);db.record_browser_visit(&next,&c,a+2000).unwrap();
 let r=db.report("2026-01-05","2026-01-05").unwrap();assert_eq!(r["history_total"],2);assert_eq!(r["total_seconds"],0.);assert_eq!(r["recent"][1]["title"],"Final title");
}
#[test]fn legacy_fragments_migrate_once_without_changing_raw_time(){
 let path=std::env::temp_dir().join(format!("timewrapped-visits-{}.sqlite3",now()));let(a,_)=range("2026-01-05","2026-01-05").unwrap();
 {let db=Db::at(&path).unwrap();for i in 0..3{let v=CleanVisit{id:format!("old-{i}"),domain:"docs.google.com".into(),site:"docs.google.com".into(),adult:false,path:if i%2==0{"/forms/viewform".into()}else{"/forms/formResponse".into()},title:"Test 3".into()};db.record_visit(&v,5000,a+i*10000+5000).unwrap();}db.conn.execute("DELETE FROM options WHERE key='browser_visits_v1'",[]).unwrap();}
 {let db=Db::at(&path).unwrap();let r=db.report("2026-01-05","2026-01-05").unwrap();assert_eq!(r["history_total"],1);assert_eq!(r["total_seconds"],15.);assert_eq!(db.conn.query_row("SELECT COUNT(*) FROM site_sessions",[],|r|r.get::<_,i64>(0)).unwrap(),3);assert!(path.with_extension("pre-v1.1.sqlite3").exists());}
 {let db=Db::at(&path).unwrap();assert_eq!(db.report("2026-01-05","2026-01-05").unwrap()["history_total"],1);db.clear().unwrap();assert!(!path.with_extension("pre-v1.1.sqlite3").exists());}
 let _=std::fs::remove_file(path);
}
#[test]fn suspended_or_rolled_back_sessions_do_not_spread_time_across_gaps(){let db=Db::at(std::path::Path::new(":memory:")).unwrap();let (a,_)=range("2026-01-05","2026-01-05").unwrap();let v=CleanVisit{id:"stable-tab".into(),domain:"youtube.com".into(),site:"YouTube".into(),adult:false,path:String::new(),title:String::new()};db.record_visit(&v,5000,a+5000).unwrap();db.record_visit(&v,5000,a+86400_000+5000).unwrap();db.record_visit(&v,5000,a+3600_000+5000).unwrap();assert_eq!(db.report("2026-01-05","2026-01-05").unwrap()["total_seconds"],10.);assert_eq!(db.report("2026-01-06","2026-01-06").unwrap()["total_seconds"],5.);}
#[test]fn deleting_adult_history_preserves_ordinary_sites(){let db=Db::at(std::path::Path::new(":memory:")).unwrap();let (a,_)=range("2026-01-05","2026-01-05").unwrap();for(adult,id)in [(false,"normal"),(true,"adult")]{let v=CleanVisit{id:id.into(),domain:format!("{id}.example"),site:id.into(),adult,path:String::new(),title:String::new()};db.record_visit(&v,5000,a+5000).unwrap();}db.delete_sites(true).unwrap();let r=db.report("2026-01-05","2026-01-05").unwrap();assert_eq!(r["total_seconds"],5.);assert_eq!(r["adult_seconds"],0.);}
#[test]fn site_matching_does_not_confuse_suffixes_or_private_urls(){assert_eq!(domain("https://www.youtube.com/watch?v=secret#x").as_deref(),Some("youtube.com"));assert!(is_adult("de.pornhub.com",&[]));assert!(!is_adult("pornhub.com.evil.test",&[]));assert!(!is_adult("notpornhub.com",&[]));for u in ["file:///private.txt","https://user:password@example.com/x","http://127.0.0.1/x","https://localhost/secret"]{assert!(domain(u).is_none());}}
#[test]fn browsing_is_disabled_by_default_and_details_are_opt_in(){let mut c=crate::extensions::Config::default();let v=Visit{id:"abc".into(),url:"https://youtube.com/watch?token=secret".into(),title:"Private title".into(),active:true,navigation:true};assert!(clean(&v,&c).is_none());c.sites_history=true;let visit=clean(&v,&c).unwrap();assert!(visit.path.is_empty()&&visit.title.is_empty());c.site_details=true;assert_eq!(clean(&v,&c).unwrap().path,"/watch");c.excluded_sites=vec!["youtube.com".into()];assert!(clean(&v,&c).is_none());}
#[test]fn adult_capture_requires_its_own_setting(){let mut c=crate::extensions::Config{sites_history:true,..Default::default()};let v=Visit{id:"abc".into(),url:"https://pornhub.com/".into(),title:String::new(),active:true,navigation:true};assert!(clean(&v,&c).is_none());c.adult_stats=true;assert!(clean(&v,&c).unwrap().adult);}
#[test]fn partial_periods_preserve_durations_and_unknown_genres_stay_unknown(){let db=Db::at(std::path::Path::new(":memory:")).unwrap();let (a,_)=range("2026-01-05","2026-01-05").unwrap();let v=CleanVisit{id:"a".into(),domain:"youtube.com".into(),site:"YouTube".into(),adult:false,path:String::new(),title:String::new()};db.record_visit(&v,5000,a+5000).unwrap();db.record(&crate::extensions::Track::new("spotify","Song","Artist").unwrap(),5000,a+5000).unwrap();let r=db.report("2026-01-05","2026-01-05").unwrap();assert_eq!(r["total_seconds"],5.);assert_eq!(r["genres"][0]["genre"],"Не определён");db.set_genre("Artist","rock","manual").unwrap();assert_eq!(db.report("2026-01-05","2026-01-05").unwrap()["genres"][0]["genre"],"rock");}
}
