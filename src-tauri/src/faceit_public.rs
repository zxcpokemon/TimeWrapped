//! Unauthenticated data used by FACEIT's public web client. May be unavailable independently of the Data API.
use serde_json::{Value,json};
type Result<T>=std::result::Result<T,String>;
pub fn fetch(steam:&str)->Result<Value>{
    crate::extensions::steam_number(steam)?;
    let get=|path:&str|crate::net::json("www.faceit.com",path,None);
    let response=get(&format!("/api/users/v1/users?game=cs2&game_id={steam}"))?;
    let raw=response["payload"].as_array().and_then(|a|a.iter().find(|p|p["games"]["cs2"]["game_id"].as_str()==Some(steam))).ok_or("FACEIT не нашёл публичный профиль CS2 с этим SteamID")?;
    let id=raw["id"].as_str().filter(|v|v.len()<80&&v.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')).ok_or("FACEIT не вернул идентификатор профиля")?;
    let stats=get(&format!("/api/stats/v1/stats/users/{id}/games/cs2")).unwrap_or(Value::Null);
    let matches=get(&format!("/api/stats/v1/stats/time/users/{id}/games/cs2?size=20"));
    let (items,history_error)=match matches{Ok(v)=>(normalize_matches(&v,id),None),Err(e)=>(Vec::new(),Some(e))};
    let lifetime=&stats["lifetime"];
    let count=number(&lifetime["m1"]);let wins=number(&lifetime["m2"]);
    Ok(json!({"source":"public","profile":{"player_id":id,"nickname":raw["nickname"],"country":raw["country"],"games":{"cs2":{"faceit_elo":raw["games"]["cs2"]["faceit_elo"],"skill_level":raw["games"]["cs2"]["skill_level"]}}},"history":{"items":items},"history_error":history_error,"stats":{"lifetime":{"Matches":count,"Win Rate %":count.zip(wins).filter(|(n,_)|*n>0.).map(|(n,w)|format!("{:.1}",w/n*100.)),"Average K/D Ratio":lifetime["k5"]}}}))
}
fn number(v:&Value)->Option<f64>{v.as_f64().or_else(||v.as_str()?.parse().ok())}
fn normalize_matches(value:&Value,player:&str)->Vec<Value>{
    value.as_array().map(|a|a.iter().take(20).filter_map(|m|{
        let id=m["matchId"].as_str().filter(|s|s.len()<100&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'))?;
        let won=match m["i10"].as_str(){Some("1")=>Some(true),Some("0")=>Some(false),_=>None};
        Some(json!({"match_id":id,"competition_name":m["i1"].as_str().unwrap_or("CS2"),"finished_at":m["date"].as_i64().map(|v|v/1000),"results":{"winner":won.map(|w|if w{"self"}else{"opponent"})},"teams":{"self":{"players":[{"player_id":player}]}},"public_score":m["i18"],"kills":number(&m["i6"]),"deaths":number(&m["i8"]),"assists":number(&m["i9"]),"elo":number(&m["elo"]),"elo_delta":number(&m["elo_delta"])}))
    }).collect()).unwrap_or_default()
}
#[cfg(test)]mod tests{use super::*;#[test]fn only_explicit_public_result_flags_become_wins(){let rows=normalize_matches(&json!([{"matchId":"1-abcd","date":1700000000000i64,"i10":"0","i6":"21","i8":"23","i9":"2","i18":"13 / 16"},{"matchId":"2-abcd"}]),"p");assert_eq!(rows[0]["results"]["winner"],"opponent");assert_eq!(rows[0]["kills"],21.);assert!(rows[1]["results"]["winner"].is_null());}}
