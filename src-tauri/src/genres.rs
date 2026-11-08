//! Optional artist-genre lookup. Unknown or ambiguous artists remain unclassified.
use crate::extensions::{Db,now};
use rusqlite::params;
use serde_json::{Value,json};
use std::time::Duration;
type Result<T>=std::result::Result<T,String>;
fn get(db:&Db,path:&str)->Result<Value>{
    for _ in 0..12{let stamp=now();let acquired=db.conn.execute("INSERT INTO options VALUES('musicbrainz_next_request',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE CAST(options.value AS INTEGER)<=?2",params![(stamp+1100).to_string(),stamp]).map_err(|e|e.to_string())?;
        if acquired>0{return crate::net::json("musicbrainz.org",path,None);}std::thread::sleep(Duration::from_millis(1100));
    }Err("MusicBrainz: подождите перед следующим запросом".into())
}
fn artist_id<'a>(value:&'a Value,name:&str)->Option<&'a str>{let normalize=|v:&str|v.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();let name=normalize(name);let matches:Vec<_>=value["artists"].as_array()?.iter().filter(|a|a["name"].as_str().is_some_and(|s|normalize(s)==name)&&a["score"].as_u64().or_else(||a["score"].as_str()?.parse().ok()).unwrap_or(0)>=95).collect();if matches.len()!=1{return None;}matches[0]["id"].as_str().filter(|s|s.len()==36&&s.bytes().all(|b|b.is_ascii_hexdigit()||b==b'-'))}
fn genre(value:&Value)->Option<String>{value["genres"].as_array()?.iter().filter(|g|g["count"].as_i64().unwrap_or(0)>0).max_by_key(|g|g["count"].as_i64().unwrap_or(0))?["name"].as_str().map(|s|s.chars().filter(|c|!c.is_control()).take(60).collect::<String>()).filter(|s|!s.is_empty())}
pub fn resolve_pending(limit:usize)->Result<Value>{
    let db=Db::open()?;let c=db.config()?;if !c.music_genres||!c.genres_online{return Ok(json!({"resolved":0,"message":"Включите поиск жанров MusicBrainz, чтобы отправлять сервису имена исполнителей"}));}
    let artists=db.artists()?;let mut resolved=0;let mut checked=0;let mut errors=Vec::new();
    for row in artists.as_array().into_iter().flatten(){if checked>=limit.min(10){break;}if row["source"].as_str()==Some("manual"){continue;}let artist=row["artist"].as_str().unwrap_or("");
        let updated:Option<i64>=db.conn.query_row("SELECT updated FROM artist_genres WHERE artist=?1",[artist],|r|r.get(0)).ok();if updated.is_some_and(|v|now()-v<30*86400_000){continue;}
        if !db.config()?.genres_online{break;}checked+=1;
        let result=(||{let query=format!("artist:\"{}\"",artist.replace('\\',"\\\\").replace('"',"\\\""));let query=url::form_urlencoded::Serializer::new(String::new()).append_pair("query",&query).append_pair("fmt","json").append_pair("limit","5").finish();let search=get(&db,&format!("/ws/2/artist?{query}"))?;let Some(id)=artist_id(&search,artist)else{return Ok(None);};let details=get(&db,&format!("/ws/2/artist/{id}?inc=genres&fmt=json"))?;Ok::<_,String>(genre(&details))})();
        match result{Ok(value)=>{if value.is_some(){resolved+=1;}db.set_genre(artist,value.as_deref().unwrap_or(""),"MusicBrainz · исполнитель")?;},Err(e)=>{errors.push(e);break;}}
    }
    Ok(json!({"checked":checked,"resolved":resolved,"errors":errors}))
}
#[cfg(test)]mod tests{use super::*;#[test]fn ambiguous_artist_names_do_not_get_a_guessed_genre(){let a=json!({"artists":[{"name":"Same","score":100,"id":"11111111-1111-1111-1111-111111111111"},{"name":"Same","score":100,"id":"22222222-2222-2222-2222-222222222222"}]});assert!(artist_id(&a,"Same").is_none());assert!(genre(&json!({"genres":[]})).is_none());assert_eq!(genre(&json!({"genres":[{"name":"rock","count":5},{"name":"pop","count":2}]})).as_deref(),Some("rock"));}}
