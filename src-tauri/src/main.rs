#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod native;
mod store;
mod tracker;
mod visibility;
mod inventory;
mod app_policy;
mod lang;
mod media;
mod extensions;
mod bridge;
mod net;
mod web_history;
mod faceit_public;
mod genres;
mod integration_check;

use base64::Engine;
use store::{App, Dashboard, Settings, Store};

fn open_store() -> Result<Store, String> { Store::open(&Store::default_path()) }

#[tauri::command]
async fn get_dashboard(from: String, to: String) -> Result<Dashboard, String> {
    agent::flush()?;
    open_store()?.dashboard(&from, &to)
}
#[tauri::command]
async fn get_wrapped(week_start: String) -> Result<serde_json::Value, String> {
    agent::flush()?;
    let wrapped=open_store()?.wrapped(&week_start)?;
    let mut value=serde_json::to_value(&wrapped).map_err(|e|e.to_string())?;
    value["extensions"]=extensions::Db::open()?.report(&wrapped.week_start,&wrapped.week_end)?;
    Ok(value)
}
#[tauri::command]
async fn list_weeks() -> Result<Vec<String>, String> { agent::flush()?; open_store()?.list_weeks() }
#[tauri::command]
async fn get_settings() -> Result<Settings, String> { open_store()?.settings() }
#[tauri::command]
async fn save_settings(settings: Settings) -> Result<(), String> {
    let store = open_store()?;
    let previous=store.settings()?;
    store.save_settings(&settings)?;
    if previous.autostart!=settings.autostart || previous.autostart_open_window!=settings.autostart_open_window {
        if let Err(error)=native::set_autostart(settings.autostart,settings.autostart_open_window) {
            store.save_settings(&previous)?;
            return Err(error);
        }
    }
    agent::notify_settings_changed()?;
    Ok(())
}
#[tauri::command]
async fn get_apps() -> Result<Vec<App>, String> { agent::flush()?; open_store()?.apps() }
#[tauri::command]
async fn update_app(id: String, category: String, excluded: bool) -> Result<(), String> {
    agent::flush()?;
    open_store()?.update_app(&id, &category, excluded)?;
    agent::notify_settings_changed()?;
    Ok(())
}
#[tauri::command]
async fn clear_data() -> Result<(), String> {
    // Pause and flush across the process boundary before deleting. No buffered data can reappear.
    let mut store = open_store()?;
    let mut settings = store.settings()?;
    settings.tracking_enabled = false;
    store.save_settings(&settings)?;
    agent::notify_settings_changed()?;
    agent::flush()?;
    store.clear_data()?;
    extensions::Db::open()?.clear()?;
    let backup=Store::default_path().with_extension("pre-v0.2.sqlite3");
    match std::fs::remove_file(backup){Ok(())=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string())}
    native::clear_icon_cache()?;
    for name in ["last-notified-week", "agent-error.log"] {
        let path=Store::default_path().with_file_name(name);
        match std::fs::remove_file(path) { Ok(())=>{}, Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{}, Err(e)=>return Err(e.to_string()) }
    }
    agent::notify_settings_changed()?;
    Ok(())
}
#[tauri::command]
async fn export_data(format: String) -> Result<Option<String>, String> {
    agent::flush()?;
    let store = open_store()?;
    let (name, filter, content) = match format.as_str() {
        "json" => ("TimeWrapped.json", "JSON files\0*.json\0\0", store.export_json()?),
        "csv" => ("TimeWrapped.csv", "CSV files\0*.csv\0\0", store.export_csv()?),
        _ => return Err("Неизвестный формат экспорта".into()),
    };
    native::save_file(name, filter, content.as_bytes())
}
#[tauri::command]
async fn save_card(png_base64: String) -> Result<Option<String>, String> {
    if png_base64.len() > 24_000_000 { return Err("Изображение слишком большое".into()); }
    let content = base64::engine::general_purpose::STANDARD.decode(png_base64.split(',').next_back().unwrap_or("")).map_err(|e| e.to_string())?;
    if !content.starts_with(b"\x89PNG\r\n\x1a\n") { return Err("Ожидалось изображение PNG".into()); }
    native::save_file("TimeWrapped-week.png", "PNG images\0*.png\0\0", &content)
}
#[tauri::command]
async fn get_diagnostics() -> Result<serde_json::Value, String> {
    agent::flush()?;
    let path = Store::default_path().with_file_name("agent-diagnostics.json");
    match std::fs::read_to_string(path) {
        Ok(value) => serde_json::from_str(&value).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({"status":"Сборщик запускается; измерения появятся после первой записи"})),
        Err(e) => Err(e.to_string()),
    }
}
#[tauri::command]
async fn get_app_icon(id: String) -> Result<Option<String>, String> { native::app_icon(&id) }

#[tauri::command]
async fn get_extensions() -> Result<serde_json::Value,String>{extensions::state()}
#[tauri::command]
async fn save_extensions(config:extensions::Config,profile:Option<String>,key:Option<String>)->Result<serde_json::Value,String>{
    tauri::async_runtime::spawn_blocking(move||extensions::save(config,profile,key)).await.map_err(|e|e.to_string())?
}
#[tauri::command]
async fn refresh_extensions()->Result<serde_json::Value,String>{tauri::async_runtime::spawn_blocking(||extensions::refresh(true)).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn clear_extension_history()->Result<serde_json::Value,String>{extensions::Db::open()?.clear()?;extensions::state()}
#[tauri::command]
async fn export_browser_addon()->Result<Option<String>,String>{native::save_file("TimeWrapped-Browser.zip","ZIP archive\0*.zip\0\0",&bridge::browser_zip())}
#[tauri::command]
async fn open_extension_link(url:String)->Result<(),String>{native::open_link(&url)}
#[tauri::command]
async fn get_autostart_status()->Result<serde_json::Value,String>{native::autostart_status()}
#[tauri::command]
async fn repair_autostart()->Result<serde_json::Value,String>{let s=open_store()?.settings()?;native::set_autostart(s.autostart,s.autostart_open_window)?;native::autostart_status()}

#[tauri::command]
async fn get_extension_report(from:String,to:String,site_filter:Option<String>,adult_only:Option<bool>,history_search:Option<String>)->Result<serde_json::Value,String>{extensions::Db::open()?.report_history(&from,&to,site_filter.as_deref(),adult_only.unwrap_or(false),history_search.as_deref())}
#[tauri::command]
async fn open_visited_page(id:String)->Result<(),String>{let url=extensions::Db::open()?.visit_url(&id)?;native::open_history_url(&url)}
#[tauri::command]
async fn set_artist_genre(artist:String,genre:String)->Result<serde_json::Value,String>{extensions::Db::open()?.set_genre(&artist,&genre,"manual")?;extensions::state()}
#[tauri::command]
async fn lookup_music_genres()->Result<serde_json::Value,String>{tauri::async_runtime::spawn_blocking(||genres::resolve_pending(10)).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn delete_site_history(adult_only:bool)->Result<(),String>{extensions::Db::open()?.delete_sites(adult_only)}

fn app_main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|s|s=="--migration-check"){
        if std::env::var_os("TIMEWRAPPED_DATA_DIR").is_none(){return Err("Migration check requires an isolated data directory".into());}
        let _=open_store()?;let db=extensions::Db::open()?;
        let report=serde_json::json!({"site_intervals":db.conn.query_row("SELECT COUNT(*) FROM site_sessions",[],|r|r.get::<_,i64>(0)).map_err(|e|e.to_string())?,"visits":db.conn.query_row("SELECT COUNT(*) FROM site_visits",[],|r|r.get::<_,i64>(0)).map_err(|e|e.to_string())?,"milliseconds":db.conn.query_row("SELECT COALESCE(SUM(milliseconds),0) FROM site_sessions",[],|r|r.get::<_,i64>(0)).map_err(|e|e.to_string())?,"integrity":db.conn.query_row("PRAGMA integrity_check",[],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?});
        std::fs::write(Store::default_path().with_file_name("migration-check.json"),report.to_string()).map_err(|e|e.to_string())?;return Ok(());
    }
    if args.iter().any(|s|s=="--browser-self-check"){
        if std::env::var_os("TIMEWRAPPED_DATA_DIR").is_none(){return Err("Browser check requires an isolated TIMEWRAPPED_DATA_DIR".into());}
        let db=extensions::Db::open()?;let report=integration_check::browser_counts(&db)?;
        std::fs::write(Store::default_path().with_file_name("browser-check.json"),report.to_string()).map_err(|e|e.to_string())?;
        std::fs::write(Store::default_path().with_file_name("browser-addon.zip"),bridge::browser_zip()).map_err(|e|e.to_string())?;return Ok(());
    }
    if args.iter().any(|s|s=="--self-check"){
        if std::env::var_os("TIMEWRAPPED_DATA_DIR").is_none(){return Err("Self-check requires an isolated TIMEWRAPPED_DATA_DIR".into());}
        let db=extensions::Db::open()?;let crypto=db.set_key("timewrapped-self-check").and_then(|_|db.key()).map(|v|v.as_deref()==Some("timewrapped-self-check"));db.set_key("")?;
        db.save(&extensions::Config{dota:true,faceit:true,faceit_mode:"public".into(),steam_id:"76561198047011640".into(),..Default::default()})?;
        let browser=integration_check::browser_and_genres(&db)?;
        let state=extensions::refresh(true)?;let data=&state["dota"]["data"];
        let report=serde_json::json!({"browser":browser,"crypto_roundtrip":crypto.as_ref().copied().unwrap_or(false),"crypto_error":crypto.err(),"dota_profile_loaded":data["profile"]["profile"].is_object(),"dota_recent_matches":data["recent"].as_array().map(Vec::len),"dota_heroes":data["heroes"].as_array().map(Vec::len),"error":state["dota"]["error"],"faceit_public_loaded":state["faceit"]["data"]["profile"]["player_id"].is_string(),"faceit_public_matches":state["faceit"]["data"]["history"]["items"].as_array().map(Vec::len),"faceit_error":state["faceit"]["error"]});
        let path=Store::default_path().with_file_name("self-check.json");std::fs::write(path,report.to_string()).map_err(|e|e.to_string())?;
        std::fs::write(Store::default_path().with_file_name("browser-addon.zip"),bridge::browser_zip()).map_err(|e|e.to_string())?;return Ok(());
    }
    if args.iter().any(|s| s == "--quit") { agent::shutdown()?; return Ok(()); }
    if args.iter().any(|s| s == "--background") { return agent::run(Store::default_path()); }
    // Headless CLI utilities also allow reproducible isolation/performance checks.
    if args.iter().any(|s| s == "--diagnostics") {
        agent::flush()?;
        println!("{}", std::fs::read_to_string(Store::default_path().with_file_name("agent-diagnostics.json")).map_err(|e| e.to_string())?);
        return Ok(());
    }
    if args.iter().any(|s| s == "--export-json") {
        agent::flush()?;
        println!("{}", open_store()?.export_json()?);
        return Ok(());
    }
    let upgraded=agent::ensure_running()?;
    let settings=open_store()?.settings()?;if settings.autostart{native::set_autostart(true,settings.autostart_open_window)?;}
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(8);
    let _gui_guard=loop{if let Some(guard)=native::gui_single_instance(!upgraded)?{break guard;}if !upgraded||std::time::Instant::now()>=deadline{return Ok(());}std::thread::sleep(std::time::Duration::from_millis(80));};
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_dashboard, get_wrapped, list_weeks, get_settings, save_settings, get_apps, update_app, clear_data, export_data, save_card, get_diagnostics, get_app_icon,get_extensions,save_extensions,refresh_extensions,clear_extension_history,export_browser_addon,open_extension_link,get_autostart_status,repair_autostart,get_extension_report,set_artist_genre,lookup_music_genres,delete_site_history,open_visited_page])
        .run(tauri::generate_context!())
        .map_err(|e| e.to_string())
}

fn main() {
    if let Err(error) = app_main() {
        eprintln!("TimeWrapped: {error}");
        if std::env::args().any(|a|a.starts_with("--")) {
            let path=Store::default_path();
            if let Some(parent)=path.parent(){let _=std::fs::create_dir_all(parent);}
            let _=std::fs::write(path.with_file_name("agent-error.log"),&error);
        } else {native::show_error(&error);}
        std::process::exit(1);
    }
}
