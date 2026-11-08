//! Local-only storage and deterministic analytics. No window titles or paths are stored.
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Timelike, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Path, PathBuf}, time::Duration as StdDuration};

type Result<T> = std::result::Result<T, String>;
const CATEGORIES: [&str; 7] = ["games", "social", "work", "study", "entertainment", "creativity", "other"];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub tracking_enabled: bool,
    pub idle_seconds: u32,
    pub autostart: bool,
    #[serde(default)]
    pub autostart_open_window: bool,
    pub notifications: bool,
    #[serde(default="default_language")] pub language:String,
}
fn default_language()->String{"ru".into()}
impl Default for Settings {
    fn default() -> Self { Self { tracking_enabled: true, idle_seconds: 180, autostart: false, autostart_open_window: false, notifications: true,language:default_language() } }
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Segment {
    pub app_id: String, pub name: String, pub exe: String,
    pub started_at: i64, pub ended_at: i64, pub switches: u32, pub kind: String,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct App { pub id: String, pub name: String, pub exe: String, pub category: String, pub excluded: bool }
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppUsage {
    pub id: String, pub name: String, pub exe: String, pub category: String,
    pub seconds: f64, pub switches: u64, pub focused_seconds:f64, pub running_seconds:f64, pub background_seconds:f64, pub music_seconds:f64,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CategoryUsage { pub category: String, pub seconds: f64 }
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DayUsage { pub date: String, pub seconds: f64 }
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HourUsage { pub hour: u32, pub seconds: f64 }
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Dashboard {
    pub from: String, pub to: String, pub total_seconds: f64, pub idle_seconds: f64,
    pub previous_seconds: f64, pub change_percent: Option<f64>,
    pub awake_seconds:f64, pub sleep_seconds:f64, pub active_seconds:f64, pub locked_seconds:f64,
    pub spotify_seconds:f64, pub app_total_seconds:f64, pub unattributed_seconds:f64, pub focus_hours:Vec<HourUsage>,
    pub apps: Vec<AppUsage>, pub categories: Vec<CategoryUsage>,
    pub days: Vec<DayUsage>, pub hours: Vec<HourUsage>, pub switches: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Wrapped {
    pub week_start: String, pub week_end: String, pub available: bool,
    pub summary: Dashboard, pub previous: Dashboard, pub peak_day: Option<DayUsage>,
    pub rhythm: String, pub rhythm_description: String,
    pub persona: String, pub persona_description: String,
}

pub struct Store { conn: Connection }
impl Store {
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("TIMEWRAPPED_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir).join("TimeWrapped"));
        base.join("activity.sqlite3")
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        let conn = Connection::open(path).map_err(err)?;
        conn.busy_timeout(StdDuration::from_secs(4)).map_err(err)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
            PRAGMA foreign_keys=ON; PRAGMA wal_autocheckpoint=128;
            PRAGMA journal_size_limit=4194304; PRAGMA cache_size=-1024;").map_err(err)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(err)?;
        if version > 5 { return Err("База создана более новой версией TimeWrapped. Обновите приложение.".into()); }
        if version == 0 {
            conn.execute_batch("BEGIN IMMEDIATE;
                CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS apps (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, exe TEXT NOT NULL,
                    category TEXT NOT NULL DEFAULT 'other', excluded INTEGER NOT NULL DEFAULT 0 CHECK(excluded IN (0,1)));
                CREATE TABLE IF NOT EXISTS segments (
                    id INTEGER PRIMARY KEY, app_id TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('active','idle')),
                    started_at INTEGER NOT NULL, ended_at INTEGER NOT NULL CHECK(ended_at>started_at), switches INTEGER NOT NULL);
                CREATE INDEX IF NOT EXISTS idx_segments_end ON segments(ended_at);
                CREATE INDEX IF NOT EXISTS idx_segments_app_start ON segments(app_id,started_at);
                CREATE TABLE IF NOT EXISTS buckets (
                    date TEXT NOT NULL, hour INTEGER NOT NULL CHECK(hour BETWEEN 0 AND 23),
                    app_id TEXT NOT NULL, kind TEXT NOT NULL, duration_ms INTEGER NOT NULL CHECK(duration_ms>=0),
                    switches INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY(date,hour,app_id,kind)) WITHOUT ROWID;
                CREATE INDEX IF NOT EXISTS idx_buckets_app_date ON buckets(app_id,date);
                PRAGMA user_version=1; COMMIT;").map_err(err)?;
        }
        if version<2 {
            // Preserve the original database before the first schema upgrade.
            if path!=Path::new(":memory:") && version==1 {
                let backup=path.with_extension("pre-v0.2.sqlite3");
                if !backup.exists(){conn.execute("VACUUM INTO ?1",[backup.to_string_lossy().as_ref()]).map_err(err)?;}
            }
            conn.execute_batch("BEGIN IMMEDIATE;
                ALTER TABLE segments RENAME TO segments_v1;
                CREATE TABLE segments(id INTEGER PRIMARY KEY,app_id TEXT NOT NULL,kind TEXT NOT NULL
                    CHECK(kind IN ('active','idle','visible','computer','sleep','locked','unattributed','legacy','legacy_idle')),
                    started_at INTEGER NOT NULL,ended_at INTEGER NOT NULL CHECK(ended_at>started_at),switches INTEGER NOT NULL);
                INSERT INTO segments SELECT id,app_id,CASE kind WHEN 'active' THEN 'legacy' WHEN 'idle' THEN 'legacy_idle' ELSE kind END,started_at,ended_at,switches FROM segments_v1;
                DROP TABLE segments_v1;
                CREATE INDEX idx_segments_end ON segments(ended_at);
                CREATE INDEX idx_segments_app_start ON segments(app_id,kind,started_at);
                UPDATE buckets SET kind=CASE kind WHEN 'active' THEN 'legacy' WHEN 'idle' THEN 'legacy_idle' ELSE kind END;
                PRAGMA user_version=2; COMMIT;").map_err(err)?;
        }
        if version<3 {
            if path!=Path::new(":memory:") && version>0 {
                let backup=path.with_extension("pre-v0.3.sqlite3");if !backup.exists(){conn.execute("VACUUM INTO ?1",[backup.to_string_lossy().as_ref()]).map_err(err)?;}
            }
            conn.execute_batch("BEGIN IMMEDIATE;
                ALTER TABLE segments RENAME TO segments_v2;
                CREATE TABLE segments(id INTEGER PRIMARY KEY,app_id TEXT NOT NULL,kind TEXT NOT NULL CHECK(kind IN ('active','idle','visible','computer','sleep','locked','unattributed','legacy','legacy_idle','running','background','music')),started_at INTEGER NOT NULL,ended_at INTEGER NOT NULL CHECK(ended_at>started_at),switches INTEGER NOT NULL);
                INSERT INTO segments SELECT * FROM segments_v2;DROP TABLE segments_v2;
                CREATE INDEX idx_segments_end ON segments(ended_at);
                CREATE INDEX idx_segments_app_start ON segments(app_id,kind,started_at);
                INSERT INTO segments(app_id,kind,started_at,ended_at,switches) SELECT app_id,'running',started_at,ended_at,0 FROM segments WHERE kind IN ('visible','legacy');
                INSERT INTO buckets(date,hour,app_id,kind,duration_ms,switches) SELECT date,hour,app_id,'running',SUM(duration_ms),0 FROM buckets WHERE kind IN ('visible','legacy') GROUP BY date,hour,app_id;
                PRAGMA user_version=3;COMMIT;").map_err(err)?;
        }
        if version<4 {
            if path!=Path::new(":memory:") && version>0 {
                let backup=path.with_extension("pre-v1.0.sqlite3");
                if !backup.exists(){conn.execute("VACUUM INTO ?1",[backup.to_string_lossy().as_ref()]).map_err(err)?;}
            }
            conn.execute_batch("BEGIN IMMEDIATE;
                CREATE TABLE IF NOT EXISTS app_catalog(app_id TEXT PRIMARY KEY);
                INSERT OR IGNORE INTO app_catalog SELECT DISTINCT app_id FROM buckets WHERE kind IN ('visible','active','legacy','music');
                PRAGMA user_version=4;").map_err(err)?;
            conn.execute_batch("COMMIT;").map_err(err)?;
        }
        if version<5 {
            if path!=Path::new(":memory:")&&version>0{let backup=path.with_extension("pre-v1.1.sqlite3");if !backup.exists(){conn.execute("VACUUM INTO ?1",[backup.to_string_lossy().as_ref()]).map_err(err)?;}}
            conn.pragma_update(None,"user_version",5).map_err(err)?;
        }
        // Reapply policy to existing v4 databases when a newly recognized helper
        // is added. Original app/segment/bucket rows remain untouched.
        let mut query=conn.prepare("SELECT app_id FROM app_catalog").map_err(err)?;
        let ids=query.query_map([],|r|r.get::<_,String>(0)).map_err(err)?.collect::<std::result::Result<Vec<_>,_>>().map_err(err)?;
        drop(query);
        for id in ids {if crate::app_policy::supporting_process(&id){conn.execute("DELETE FROM app_catalog WHERE app_id=?1",[id]).map_err(err)?;}}
        Ok(Self { conn })
    }

    pub fn settings(&self) -> Result<Settings> {
        let value: Option<String> = self.conn.query_row("SELECT value FROM settings WHERE id=1", [], |r| r.get(0))
            .optional().map_err(err)?;
        value.map(|v| serde_json::from_str(&v).map_err(err)).unwrap_or_else(|| Ok(Settings::default()))
    }
    pub fn save_settings(&self, value: &Settings) -> Result<()> {
        if !["ru","en","uk"].contains(&value.language.as_str()){return Err("Неподдерживаемый язык".into());}
        if !(15..=3600).contains(&value.idle_seconds) { return Err("Порог простоя должен быть от 15 до 3600 секунд.".into()); }
        self.conn.execute("INSERT INTO settings(id,value) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET value=excluded.value",
            [serde_json::to_string(value).map_err(err)?]).map_err(err)?;
        Ok(())
    }

    /// A batch and all its hourly buckets commit atomically. A retry cannot count time twice.
    pub fn record_batch(&mut self, segments: &[Segment]) -> Result<()> {
        if segments.is_empty() { return Ok(()); }
        for s in segments {
            if !["active","idle","visible","computer","sleep","locked","unattributed","legacy","legacy_idle","running","background","music"].contains(&s.kind.as_str()) { return Err("Неизвестный тип сессии.".into()); }
            if s.ended_at <= s.started_at { return Err("Окончание сессии должно быть позже начала.".into()); }
            if Utc.timestamp_millis_opt(s.started_at).single().is_none() || Utc.timestamp_millis_opt(s.ended_at).single().is_none() {
                return Err("Недопустимая дата сессии.".into());
            }
            if ["active","visible","legacy","running","background","music"].contains(&s.kind.as_str()) && (s.app_id.is_empty() || s.exe.is_empty() || s.app_id.contains(['\\', '/', ':'])
                || s.exe.contains(['\\', '/', ':']) || s.app_id.len()>1024 || s.exe.len()>1024 || s.name.len()>4096) {
                return Err("Допускается только имя исполняемого файла, без пути.".into());
            }
        }
        let tx = self.conn.transaction().map_err(err)?;
        // Only clip overlaps inside this batch. A global UTC high-water mark would
        // discard valid elapsed activity after the user moves the wall clock back.
        let mut lanes: BTreeMap<String,i64> = BTreeMap::new();
        let mut ordered: Vec<&Segment> = segments.iter().collect();
        ordered.sort_by_key(|s| s.started_at);
        for s in ordered {
            if crate::app_policy::supporting_process(&s.exe) {continue;}
            let lane=match s.kind.as_str(){"active"|"idle"|"unattributed"=>"focus".into(),"visible"|"running"|"background"|"music"=>format!("{}:{}",s.kind,s.app_id),"legacy"|"legacy_idle"=>"legacy".into(),other=>other.into()};
            let last_end=*lanes.get(&lane).unwrap_or(&i64::MIN);
            let start = s.started_at.max(last_end);
            if start >= s.ended_at { continue; }
            let app_id = &s.app_id;
            if ["active","visible","legacy","running","background","music"].contains(&s.kind.as_str()) {
                if ["active","visible","legacy","music"].contains(&s.kind.as_str()) {
                    tx.execute("INSERT OR IGNORE INTO app_catalog(app_id) VALUES(?1)",[app_id]).map_err(err)?;
                }
                tx.execute("INSERT INTO apps(id,name,exe,category) VALUES(?1,?2,?3,?4)
                    ON CONFLICT(id) DO UPDATE SET name=excluded.name,exe=excluded.exe WHERE apps.name<>excluded.name OR apps.exe<>excluded.exe",
                    params![app_id, s.name, s.exe, guess_category(&s.exe)]).map_err(err)?;
                let excluded: bool = tx.query_row("SELECT excluded FROM apps WHERE id=?1", [app_id], |r| r.get(0)).map_err(err)?;
                if excluded { lanes.insert(lane,s.ended_at); continue; }
            }
            let switches = if start == s.started_at && ["active","legacy"].contains(&s.kind.as_str()) { s.switches } else { 0 };
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM segments WHERE app_id=?1 AND kind=?2 AND started_at=?3 AND ended_at=?4)",
                params![app_id,s.kind,start,s.ended_at],|r|r.get(0)).map_err(err)?;
            if exists {lanes.insert(lane,s.ended_at);continue;}
            tx.execute("INSERT INTO segments(app_id,kind,started_at,ended_at,switches) VALUES(?1,?2,?3,?4,?5)",
                params![app_id, s.kind, start, s.ended_at, switches]).map_err(err)?;
            for (i, bucket) in split_interval(&Local, start, s.ended_at)?.iter().enumerate() {
                tx.execute("INSERT INTO buckets(date,hour,app_id,kind,duration_ms,switches) VALUES(?1,?2,?3,?4,?5,?6)
                    ON CONFLICT(date,hour,app_id,kind) DO UPDATE SET
                    duration_ms=duration_ms+excluded.duration_ms,switches=switches+excluded.switches",
                    params![bucket.date,bucket.hour,app_id,s.kind,bucket.duration_ms,if i==0 {switches} else {0}]).map_err(err)?;
            }
            lanes.insert(lane,s.ended_at);
        }
        tx.commit().map_err(err)
    }

    pub fn apps(&self) -> Result<Vec<App>> {
        let mut q = self.conn.prepare("SELECT id,name,exe,category,excluded FROM apps WHERE id IN (SELECT app_id FROM app_catalog) ORDER BY name COLLATE NOCASE,id").map_err(err)?;
        let rows=q.query_map([], |r| Ok(App { id:r.get(0)?, name:r.get(1)?, exe:r.get(2)?, category:r.get(3)?, excluded:r.get(4)? }))
            .map_err(err)?.collect::<std::result::Result<Vec<_>,_>>().map_err(err);
        rows
    }
    /// Called only with identities proven by Inventory's ordinary-window policy.
    pub fn register_applications(&self, apps:&[crate::tracker::Identity])->Result<()> {
        for a in apps {
            if crate::app_policy::supporting_process(&a.exe){continue;}
            self.conn.execute("INSERT INTO apps(id,name,exe,category) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET name=excluded.name,exe=excluded.exe WHERE apps.name<>excluded.name OR apps.exe<>excluded.exe",params![a.id,a.name,a.exe,guess_category(&a.exe)]).map_err(err)?;
            self.conn.execute("INSERT OR IGNORE INTO app_catalog(app_id) VALUES(?1)",[&a.id]).map_err(err)?;
        }
        Ok(())
    }
    pub fn update_app(&self, id: &str, category: &str, excluded: bool) -> Result<()> {
        if !CATEGORIES.contains(&category) { return Err("Неизвестная категория.".into()); }
        if self.conn.execute("UPDATE apps SET category=?2,excluded=?3 WHERE id=?1", params![id,category,excluded]).map_err(err)? == 0 {
            return Err("Приложение не найдено.".into());
        }
        Ok(())
    }
    pub fn clear_data(&mut self) -> Result<()> {
        self.conn.execute_batch("PRAGMA secure_delete=ON;").map_err(err)?;
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute_batch("DELETE FROM buckets; DELETE FROM segments; DELETE FROM apps; DELETE FROM app_catalog; DELETE FROM meta;").map_err(err)?;
        tx.commit().map_err(err)?;
        // Secure-delete actively clears free pages. Checkpoint truncates the previous WAL history.
        self.conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);").map_err(err)?;
        if let Some(path)=self.conn.path().filter(|p|*p!=":memory:") {
            for suffix in ["pre-v0.2.sqlite3","pre-v0.3.sqlite3","pre-v1.0.sqlite3","pre-v1.1.sqlite3"]{
                match std::fs::remove_file(Path::new(path).with_extension(suffix)){Ok(())=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(err(e))}
            }
        }
        Ok(())
    }

    pub fn dashboard(&self, from: &str, to: &str) -> Result<Dashboard> {
        let (first,last) = dates(from,to)?;
        let days = (last-first).num_days()+1;
        let prev_first = first.checked_sub_signed(Duration::days(days)).ok_or("Дата вне допустимого диапазона.")?;
        let prev_last = first.pred_opt().ok_or("Дата вне допустимого диапазона.")?;
        let previous_seconds = self.total_between(&prev_first.to_string(), &prev_last.to_string())?;
        let mut result = self.summary(first,last)?;
        result.previous_seconds = previous_seconds;
        result.change_percent = change(result.total_seconds,previous_seconds);
        Ok(result)
    }
    fn total_between(&self,from:&str,to:&str)->Result<f64> {
        self.conn.query_row("SELECT COALESCE(SUM(duration_ms),0)/1000.0 FROM buckets WHERE date>=?1 AND date<=?2 AND kind IN ('computer','legacy','legacy_idle')",params![from,to],|r|r.get(0)).map_err(err)
    }
    fn summary(&self,first:NaiveDate,last:NaiveDate)->Result<Dashboard> {
        let from=first.to_string();let to=last.to_string();
        let mut q=self.conn.prepare("SELECT a.id,a.name,a.exe,a.category,
            SUM(CASE WHEN b.kind IN ('visible','legacy') THEN b.duration_ms ELSE 0 END)/1000.0,
            SUM(CASE WHEN b.kind IN ('active','legacy') THEN b.switches ELSE 0 END),
            SUM(CASE WHEN b.kind IN ('active','legacy') THEN b.duration_ms ELSE 0 END)/1000.0,
            SUM(CASE WHEN b.kind='running' THEN b.duration_ms ELSE 0 END)/1000.0,
            SUM(CASE WHEN b.kind='background' THEN b.duration_ms ELSE 0 END)/1000.0,
            SUM(CASE WHEN b.kind='music' THEN b.duration_ms ELSE 0 END)/1000.0
            FROM buckets b JOIN apps a ON a.id=b.app_id WHERE b.date>=?1 AND b.date<=?2 AND b.kind IN ('visible','legacy','active','running','background','music') AND a.excluded=0 AND a.id IN (SELECT app_id FROM app_catalog)
            GROUP BY a.id ORDER BY 5 DESC,8 DESC,a.name COLLATE NOCASE").map_err(err)?;
        let apps=q.query_map(params![from,to],|r|Ok(AppUsage{id:r.get(0)?,name:r.get(1)?,exe:r.get(2)?,category:r.get(3)?,seconds:r.get(4)?,switches:r.get(5)?,focused_seconds:r.get(6)?,running_seconds:r.get(7)?,background_seconds:r.get(8)?,music_seconds:r.get(9)?}))
            .map_err(err)?.collect::<std::result::Result<Vec<_>,_>>().map_err(err)?;
        let spotify_seconds=apps.iter().filter(|a|a.id.eq_ignore_ascii_case("spotify.exe")).map(|a|a.music_seconds).sum();
        let sum=|kinds:&str|->Result<f64>{self.conn.query_row(&format!("SELECT COALESCE(SUM(duration_ms),0)/1000.0 FROM buckets WHERE date>=?1 AND date<=?2 AND kind IN ({kinds})"),params![from,to],|r|r.get(0)).map_err(err)};
        let total_seconds=self.total_between(&from,&to)?;
        let idle_seconds=sum("'idle','legacy_idle'")?;let sleep_seconds=sum("'sleep'")?;
        let awake_seconds=(total_seconds-sleep_seconds).max(0.0);
        let active_seconds=sum("'active','legacy'")?;let locked_seconds=sum("'locked'")?;
        let unattributed_seconds=sum("'unattributed'")?;
        let app_total_seconds=apps.iter().map(|a|a.seconds).sum();
        let switches=self.conn.query_row("SELECT COALESCE(SUM(switches),0) FROM buckets WHERE date>=?1 AND date<=?2 AND kind IN ('active','legacy')",params![from,to],|r|r.get(0)).map_err(err)?;
        let mut grouped:BTreeMap<String,f64>=BTreeMap::new();for a in &apps{*grouped.entry(a.category.clone()).or_default()+=a.seconds;}
        let mut categories:Vec<_>=grouped.into_iter().map(|(category,seconds)|CategoryUsage{category,seconds}).collect();
        categories.sort_by(|a,b|b.seconds.total_cmp(&a.seconds));
        let mut day_q=self.conn.prepare("SELECT date,SUM(duration_ms)/1000.0 FROM buckets WHERE date>=?1 AND date<=?2 AND kind IN ('computer','legacy','legacy_idle') GROUP BY date").map_err(err)?;
        let day_map:BTreeMap<String,f64>=day_q.query_map(params![from,to],|r|Ok((r.get(0)?,r.get(1)?))).map_err(err)?.collect::<std::result::Result<_,_>>().map_err(err)?;
        let mut days=Vec::new();let mut date=first;loop{let key=date.to_string();days.push(DayUsage{seconds:*day_map.get(&key).unwrap_or(&0.0),date:key});if date==last{break;}date=date.succ_opt().ok_or("Дата вне диапазона")?;}
        let hours_for=|kinds:&str|->Result<Vec<HourUsage>>{
            let mut hours:Vec<_>=(0..24).map(|hour|HourUsage{hour,seconds:0.0}).collect();
            let mut q=self.conn.prepare(&format!("SELECT hour,SUM(duration_ms)/1000.0 FROM buckets WHERE date>=?1 AND date<=?2 AND kind IN ({kinds}) GROUP BY hour")).map_err(err)?;
            for row in q.query_map(params![from,to],|r|Ok((r.get::<_,usize>(0)?,r.get::<_,f64>(1)?))).map_err(err)?{let(h,s)=row.map_err(err)?;if let Some(v)=hours.get_mut(h){v.seconds=s;}}
            Ok(hours)
        };
        let hours=hours_for("'computer','legacy','legacy_idle'")?;let focus_hours=hours_for("'active','legacy'")?;
        Ok(Dashboard{spotify_seconds,from,to,total_seconds,idle_seconds,previous_seconds:0.0,change_percent:None,awake_seconds,sleep_seconds,active_seconds,locked_seconds,app_total_seconds,unattributed_seconds,focus_hours,apps,categories,days,hours,switches})
    }

    pub fn wrapped(&self, week_start:&str) -> Result<Wrapped> {
        let first=parse_date(week_start)?;
        if first.weekday()!=chrono::Weekday::Mon {return Err("Неделя должна начинаться в понедельник.".into());}
        let last=first.checked_add_signed(Duration::days(6)).ok_or("Дата вне допустимого диапазона.")?;
        let prev_first=first.checked_sub_signed(Duration::days(7)).ok_or("Дата вне допустимого диапазона.")?;
        let prev_last=first.pred_opt().ok_or("Дата вне допустимого диапазона.")?;
        let summary=self.dashboard(&first.to_string(),&last.to_string())?;
        let previous=self.dashboard(&prev_first.to_string(),&prev_last.to_string())?;
        let available=last<Local::now().date_naive() && summary.total_seconds>0.0;
        let peak_day=summary.days.iter().filter(|d|d.seconds>0.0)
            .max_by(|a,b|a.seconds.total_cmp(&b.seconds).then_with(||b.date.cmp(&a.date))).cloned();
        let (rhythm,rhythm_description,persona,persona_description)=describe(&summary);
        Ok(Wrapped{week_start:first.to_string(),week_end:last.to_string(),available,summary,previous,peak_day,
            rhythm,rhythm_description,persona,persona_description})
    }
    pub fn list_weeks(&self) -> Result<Vec<String>> {
        let current_monday=Local::now().date_naive()-Duration::days(Local::now().weekday().num_days_from_monday() as i64);
        let mut q=self.conn.prepare("SELECT DISTINCT date FROM buckets WHERE kind IN ('computer','legacy','legacy_idle') AND duration_ms>0 ORDER BY date DESC").map_err(err)?;
        let mut weeks=std::collections::BTreeSet::new();
        for date in q.query_map([],|r|r.get::<_,String>(0)).map_err(err)? {
            let date=parse_date(&date.map_err(err)?)?;
            let monday=date-Duration::days(date.weekday().num_days_from_monday() as i64);
            if monday<current_monday {weeks.insert(monday.to_string());}
        }
        Ok(weeks.into_iter().rev().collect())
    }

    pub fn export_json(&self) -> Result<String> {
        let sessions=self.export_sessions()?;
        serde_json::to_string_pretty(&serde_json::json!({"schema_version":5,"exported_at":Utc::now().to_rfc3339(),
            "timezone":"local-at-collection","units":"UTC milliseconds; duration in milliseconds",
            "settings":self.settings()?,"apps":self.apps()?,"sessions":sessions})).map_err(err)
    }
    pub fn export_csv(&self) -> Result<String> {
        let mut result=String::from("\u{feff}app_id,app_name,executable,category,kind,started_at_utc,ended_at_utc,duration_seconds,switches\r\n");
        for s in self.export_sessions()? {
            let start=Utc.timestamp_millis_opt(s.started_at).single().ok_or("Недопустимая дата.")?.to_rfc3339();
            let end=Utc.timestamp_millis_opt(s.ended_at).single().ok_or("Недопустимая дата.")?.to_rfc3339();
            result.push_str(&[csv(&s.app_id),csv(&s.name),csv(&s.exe),csv(&s.category),csv(&s.kind),csv(&start),csv(&end),
                format!("{:.3}",(s.ended_at-s.started_at) as f64/1000.0),s.switches.to_string()].join(","));
            result.push_str("\r\n");
        }
        Ok(result)
    }
    fn export_sessions(&self) -> Result<Vec<ExportSession>> {
        let mut q=self.conn.prepare("SELECT s.app_id,COALESCE(a.name,s.kind),COALESCE(a.exe,''),COALESCE(a.category,'other'),
            s.kind,s.started_at,s.ended_at,s.switches FROM segments s LEFT JOIN apps a ON a.id=s.app_id
            WHERE s.kind IN ('idle','legacy_idle','computer','sleep','locked','unattributed') OR (a.excluded=0 AND a.id IN (SELECT app_id FROM app_catalog)) ORDER BY s.started_at,s.id").map_err(err)?;
        let rows=q.query_map([],|r|Ok(ExportSession{app_id:r.get(0)?,name:r.get(1)?,exe:r.get(2)?,category:r.get(3)?,
            kind:r.get(4)?,started_at:r.get(5)?,ended_at:r.get(6)?,switches:r.get(7)?})).map_err(err)?
            .collect::<std::result::Result<Vec<_>,_>>().map_err(err);
        rows
    }
}

#[derive(Serialize)]
struct ExportSession {app_id:String,name:String,exe:String,category:String,kind:String,started_at:i64,ended_at:i64,switches:u32}
fn err(e:impl std::fmt::Display)->String {e.to_string()}
fn parse_date(value:&str)->Result<NaiveDate> {
    let date=NaiveDate::parse_from_str(value,"%Y-%m-%d").map_err(|_|"Дата должна быть в формате ГГГГ-ММ-ДД.".to_string())?;
    if date.to_string()!=value || !(1970..=9998).contains(&date.year()) {return Err("Недопустимая дата.".into());}
    Ok(date)
}
fn dates(from:&str,to:&str)->Result<(NaiveDate,NaiveDate)> {
    let first=parse_date(from)?; let last=parse_date(to)?;
    if last<first || (last-first).num_days()>3660 {return Err("Выберите период от 1 дня до 10 лет.".into());}
    Ok((first,last))
}
fn change(current:f64,previous:f64)->Option<f64> {if previous>0.0 {Some((current-previous)/previous*100.0)}else{None}}
fn csv(value:&str)->String {
    // Prevent spreadsheet formula execution when an executable has an unusual name.
    let safe=if value.starts_with(['=','+','-','@','\t','\r']) {format!("'{value}")}else{value.to_string()};
    format!("\"{}\"",safe.replace('"',"\"\""))
}
fn guess_category(exe:&str)->&'static str {
    let lower=exe.to_ascii_lowercase();
    if ["discord.exe","telegram.exe","slack.exe","whatsapp.exe","signal.exe","teams.exe","ms-teams.exe","skype.exe"].contains(&lower.as_str()) {"social"}
    else if ["code.exe","devenv.exe","idea64.exe","rider64.exe","pycharm64.exe","excel.exe","winword.exe","powerpnt.exe","outlook.exe","notion.exe","obsidian.exe","windowsterminal.exe","cmd.exe","powershell.exe","pwsh.exe"].contains(&lower.as_str()) {"work"}
    else if ["steam.exe","minecraft.exe","minecraftlauncher.exe","epicgameslauncher.exe","robloxplayerbeta.exe","dota2.exe","cs2.exe","valorant-win64-shipping.exe","fortniteclient-win64-shipping.exe"].contains(&lower.as_str()) {"games"}
    else if ["blender.exe","photoshop.exe","illustrator.exe","figma.exe","krita.exe","gimp.exe","fl64.exe","ableton live.exe","audacity.exe","resolve.exe","premiere pro.exe"].contains(&lower.as_str()) {"creativity"}
    else if ["spotify.exe","vlc.exe","mpv.exe","netflix.exe"].contains(&lower.as_str()) {"entertainment"}
    else if ["anki.exe","ankiapp.exe","duolingo.exe"].contains(&lower.as_str()) {"study"}
    else {"other"}
}

#[derive(Debug)]
struct Bucket {date:String,hour:u32,duration_ms:i64}
/// Walk UTC boundaries and observe local offsets. This preserves elapsed duration through DST,
/// including repeated/missing hours and zones whose offsets use half/quarter hours.
fn split_interval<Tz:TimeZone>(tz:&Tz,start:i64,end:i64)->Result<Vec<Bucket>> {
    let mut parts:Vec<Bucket>=Vec::new(); let mut cursor=start;
    while cursor<end {
        let local:DateTime<Tz>=tz.timestamp_millis_opt(cursor).single().ok_or("Недопустимая дата сессии.")?;
        let key=local.date_naive().to_string(); let hour=local.hour();
        let remaining_hour=3_600_000-(i64::from(local.minute())*60_000+i64::from(local.second())*1000+i64::from(local.timestamp_subsec_millis()));
        // A timezone transition can happen within an hour. Check minute boundaries as well.
        let remaining_minute=60_000-(i64::from(local.second())*1000+i64::from(local.timestamp_subsec_millis()));
        let next=end.min(cursor.saturating_add(remaining_hour.min(remaining_minute).max(1)));
        if let Some(last)=parts.last_mut().filter(|b|b.date==key && b.hour==hour) {last.duration_ms+=next-cursor;}
        else {parts.push(Bucket{date:key,hour,duration_ms:next-cursor});}
        cursor=next;
    }
    Ok(parts)
}

fn describe(d:&Dashboard)->(String,String,String,String) {
    if d.active_seconds<=0.0 {
        return ("Пока мало данных".into(),"Режим появится после первых активных сессий.".into(),
            "Твоя история начинается".into(),"Для цифрового типа нужна завершённая неделя с активностью.".into());
    }
    let night=d.focus_hours.iter().filter(|h|h.hour>=22 || h.hour<6).map(|h|h.seconds).sum::<f64>()/d.active_seconds.max(1.0);
    let morning=d.focus_hours.iter().filter(|h|(6..12).contains(&h.hour)).map(|h|h.seconds).sum::<f64>()/d.active_seconds.max(1.0);
    let (rhythm,rhythm_description)=if night>=0.35 {("Сова",format!("{:.0}% активности приходится на 22:00–06:00. Порог для совы — 35%.",night*100.0))}
        else if morning>=0.40 {("Жаворонок",format!("{:.0}% активности приходится на 06:00–12:00. Порог для жаворонка — 40%.",morning*100.0))}
        else {("Свой ритм","Активность распределена по дню: менее 35% ночью и менее 40% утром.".into())};
    let share=|category:&str|d.categories.iter().filter(|c|c.category==category).map(|c|c.seconds).sum::<f64>()/d.app_total_seconds.max(1.0);
    let (persona,persona_description)=if d.active_seconds<7.0*3600.0 {("Цифровой минималист","За записанную неделю набралось менее 7 часов активного времени. Тип описывает только собранные данные.".into())}
        else if share("creativity")>=0.25 {("Творческий экспериментатор",format!("Творческие приложения занимают {:.0}% времени — не менее 25%.",share("creativity")*100.0))}
        else if share("games")>=0.40 {("Игровой стратег",format!("Игры занимают {:.0}% времени — не менее 40%.",share("games")*100.0))}
        else if night>=0.35 {("Ночной исследователь",format!("Ночью, с 22:00 до 06:00, прошло {:.0}% активности — не менее 35%.",night*100.0))}
        else if d.switches>=500 {("Мастер многозадачности",format!("За неделю — {} переключений на приложения. Порог этого типа — 500.",d.switches))}
        else {("В своём темпе","За неделю — не менее 7 часов; ни одно из специальных правил не преобладает.".into())};
    (rhythm.into(),rhythm_description,persona.into(),persona_description)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    fn memory()->Store {Store::open(Path::new(":memory:")).unwrap()}
    fn stamp(s:&str)->i64 {DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()}
    fn segment(start:i64,end:i64)->Segment {Segment{app_id:"code.exe".into(),name:"Visual Studio Code".into(),exe:"Code.exe".into(),started_at:start,ended_at:end,switches:1,kind:"legacy".into()}}
    fn local_stamp(date:&str,hour:u32,min:u32)->i64 {
        let d=parse_date(date).unwrap().and_hms_opt(hour,min,0).unwrap();
        Local.from_local_datetime(&d).earliest().unwrap().timestamp_millis()
    }
    /// A deterministic European-style DST zone keeps this test independent of the host timezone.
    #[derive(Clone)] struct DstZone;
    impl TimeZone for DstZone {
        type Offset=FixedOffset;
        fn from_offset(_: &Self::Offset)->Self { Self }
        fn offset_from_local_date(&self,date:&NaiveDate)->chrono::LocalResult<Self::Offset> {
            self.offset_from_local_datetime(&date.and_hms_opt(12,0,0).unwrap())
        }
        fn offset_from_local_datetime(&self,date:&chrono::NaiveDateTime)->chrono::LocalResult<Self::Offset> {
            let choices:Vec<_>=[3600,7200].into_iter().filter_map(|seconds| {
                let offset=FixedOffset::east_opt(seconds).unwrap();
                let utc=*date-Duration::seconds(seconds as i64);
                (self.offset_from_utc_datetime(&utc)==offset).then_some(offset)
            }).collect();
            match choices.as_slice() {[]=>chrono::LocalResult::None,[one]=>chrono::LocalResult::Single(*one),[one,two]=>chrono::LocalResult::Ambiguous(*one,*two),_=>unreachable!()}
        }
        fn offset_from_utc_date(&self,date:&NaiveDate)->Self::Offset {self.offset_from_utc_datetime(&date.and_hms_opt(12,0,0).unwrap())}
        fn offset_from_utc_datetime(&self,date:&chrono::NaiveDateTime)->Self::Offset {
            let summer=*date>=NaiveDate::from_ymd_opt(2025,3,30).unwrap().and_hms_opt(1,0,0).unwrap()
                && *date<NaiveDate::from_ymd_opt(2025,10,26).unwrap().and_hms_opt(1,0,0).unwrap();
            FixedOffset::east_opt(if summer {7200}else{3600}).unwrap()
        }
    }
    #[test] fn dst_spring_skips_missing_hour_and_autumn_preserves_repeated_hour() {
        let spring=split_interval(&DstZone,stamp("2025-03-30T00:30:00Z"),stamp("2025-03-30T02:30:00Z")).unwrap();
        assert_eq!(spring.iter().map(|b|(b.hour,b.duration_ms)).collect::<Vec<_>>(),vec![(1,1_800_000),(3,3_600_000),(4,1_800_000)]);
        let autumn=split_interval(&DstZone,stamp("2025-10-26T00:30:00Z"),stamp("2025-10-26T02:30:00Z")).unwrap();
        assert_eq!(autumn.iter().map(|b|(b.hour,b.duration_ms)).collect::<Vec<_>>(),vec![(2,5_400_000),(3,1_800_000)]);
        assert_eq!(autumn.iter().map(|b|b.duration_ms).sum::<i64>(),7_200_000);
    }
    #[test] fn buckets_split_midnight_without_losing_milliseconds() {
        let zone=FixedOffset::east_opt(5*3600+45*60).unwrap();
        let start=stamp("2025-01-01T23:59:59.123+05:45"); let end=start+182_001;
        let buckets=split_interval(&zone,start,end).unwrap();
        assert_eq!(buckets.iter().map(|b|b.duration_ms).sum::<i64>(),182_001);
        assert_eq!((buckets[0].date.as_str(),buckets[0].hour,buckets[0].duration_ms),("2025-01-01",23,877));
        assert_eq!((buckets[1].date.as_str(),buckets[1].hour),("2025-01-02",0));
    }
    #[test] fn buckets_handle_leap_day_and_fractional_seconds() {
        let start=stamp("2024-02-29T23:59:30.500Z");
        let b=split_interval(&Utc,start,start+60_000).unwrap();
        assert_eq!(b.len(),2); assert_eq!(b[0].duration_ms,29_500); assert_eq!(b[1].duration_ms,30_500);
        assert_eq!(b[1].date,"2024-03-01");
    }
    #[test] fn active_idle_and_duplicate_batches_are_separate() {
        let mut db=memory(); let start=local_stamp("2025-01-06",12,0);
        let active=segment(start,start+60_000);
        let mut idle=segment(start+60_000,start+90_000); idle.kind="legacy_idle".into(); idle.switches=0;
        db.record_batch(&[active.clone(),idle.clone()]).unwrap();
        db.record_batch(&[active,idle]).unwrap();
        let d=db.dashboard("2025-01-06","2025-01-06").unwrap();
        assert_eq!(d.total_seconds,90.0);assert_eq!(d.idle_seconds,30.0);assert_eq!(d.switches,1);
        assert_eq!(d.hours.iter().map(|h|h.seconds).sum::<f64>(),d.total_seconds);
        assert_eq!(d.days.iter().map(|h|h.seconds).sum::<f64>(),d.total_seconds);
    }
    #[test] fn overlapping_intervals_cannot_inflate_total() {
        let mut db=memory(); let start=local_stamp("2025-01-06",12,0);
        db.record_batch(&[segment(start,start+60_000),segment(start+30_000,start+90_000)]).unwrap();
        let d=db.dashboard("2025-01-06","2025-01-06").unwrap();
        assert_eq!(d.total_seconds,90.0);assert_eq!(d.switches,1);
    }
    #[test] fn wall_clock_rollback_does_not_discard_future_elapsed_activity() {
        let mut db=memory();let start=local_stamp("2025-01-06",12,0);
        db.record_batch(&[segment(start,start+60_000)]).unwrap();
        // The tracker drops the instant of the clock jump, then resumes with an
        // earlier UTC label. These are two distinct real elapsed minutes.
        db.record_batch(&[segment(start-3_600_000,start-3_540_000)]).unwrap();
        assert_eq!(db.dashboard("2025-01-06","2025-01-06").unwrap().total_seconds,120.0);
    }
    #[test] fn categories_and_exclusions_update_history_and_skip_future_collection() {
        let mut db=memory();let start=local_stamp("2025-01-06",12,0);
        db.record_batch(&[segment(start,start+60_000)]).unwrap();
        db.update_app("code.exe","study",false).unwrap();
        assert_eq!(db.dashboard("2025-01-06","2025-01-06").unwrap().categories[0].category,"study");
        db.update_app("code.exe","study",true).unwrap();
        db.record_batch(&[segment(start+60_000,start+120_000)]).unwrap();
        assert_eq!(db.dashboard("2025-01-06","2025-01-06").unwrap().apps.len(),0);
        db.update_app("code.exe","study",false).unwrap();
        assert_eq!(db.dashboard("2025-01-06","2025-01-06").unwrap().total_seconds,60.0);
    }
    #[test] fn completed_wrapped_and_zero_baseline() {
        let mut db=memory(); let start=local_stamp("2025-01-06",12,0);
        db.record_batch(&[segment(start,start+3600_000)]).unwrap();
        let w=db.wrapped("2025-01-06").unwrap();
        assert!(w.available);assert_eq!(w.summary.total_seconds,3600.0);assert_eq!(w.summary.change_percent,None);
        assert_eq!(w.persona,"Цифровой минималист"); assert_eq!(w.peak_day.unwrap().date,"2025-01-06");
        assert_eq!(db.list_weeks().unwrap(),vec!["2025-01-06"]);
        assert!(db.wrapped("2025-01-07").is_err());
        assert!(!db.wrapped("2025-01-13").unwrap().available);
        assert_eq!(change(0.0,100.0),Some(-100.0));
    }
    #[test] fn comparison_uses_an_equal_length_previous_period() {
        let mut db=memory();let previous=local_stamp("2025-01-06",12,0);let current=local_stamp("2025-01-13",12,0);
        db.record_batch(&[segment(previous,previous+60_000),segment(current,current+90_000)]).unwrap();
        let d=db.dashboard("2025-01-13","2025-01-19").unwrap();
        assert_eq!(d.previous_seconds,60.0);assert_eq!(d.total_seconds,90.0);assert_eq!(d.change_percent,Some(50.0));
        assert_eq!(d.days.len(),7);assert_eq!(d.hours.len(),24);
    }
    #[test] fn persists_across_restart_and_deletes_statistics_preserving_settings() {
        let path=std::env::temp_dir().join(format!("timewrapped-store-{}-{}.sqlite3",std::process::id(),Utc::now().timestamp_nanos_opt().unwrap()));
        let start=local_stamp("2025-01-06",12,0);
        {let mut db=Store::open(&path).unwrap(); db.save_settings(&Settings{idle_seconds:300,..Settings::default()}).unwrap();
            db.record_batch(&[segment(start,start+60_000)]).unwrap();}
        {let mut db=Store::open(&path).unwrap();assert_eq!(db.dashboard("2025-01-06","2025-01-06").unwrap().total_seconds,60.0);
            assert_eq!(db.settings().unwrap().idle_seconds,300);db.clear_data().unwrap();
            assert!(db.apps().unwrap().is_empty());assert!(db.list_weeks().unwrap().is_empty());assert_eq!(db.settings().unwrap().idle_seconds,300);}
        std::fs::remove_file(&path).unwrap();
        for suffix in ["-wal","-shm"] {let _=std::fs::remove_file(format!("{}{suffix}",path.display()));}
    }
    #[test] fn rejects_paths_bad_categories_and_bad_ranges() {
        let mut db=memory();let start=local_stamp("2025-01-06",12,0);let mut bad=segment(start,start+60_000);bad.exe="C:\\Users\\private\\Code.exe".into();
        assert!(db.record_batch(&[bad]).is_err());assert!(db.dashboard("2025-01-07","2025-01-06").is_err());
        assert!(db.dashboard("2025-1-6","2025-01-06").is_err());assert!(db.save_settings(&Settings{idle_seconds:0,..Settings::default()}).is_err());
    }
    #[test] fn parallel_visibility_clock_sleep_and_exclusion_are_independent() {
        let mut db=memory();let start=local_stamp("2025-01-06",12,0);
        let mut clock=segment(start,start+3600_000);clock.kind="computer".into();clock.app_id="__computer__".into();clock.switches=0;
        let mut one=segment(start,start+3600_000);one.kind="visible".into();one.switches=0;
        let mut two=one.clone();two.app_id="discord.exe".into();two.exe="Discord.exe".into();two.name="Discord".into();
        let mut focus=one.clone();focus.kind="active".into();focus.switches=1;
        db.record_batch(&[clock.clone(),one.clone(),two.clone(),focus.clone()]).unwrap();
        db.record_batch(&[clock,one,two,focus]).unwrap();
        let d=db.dashboard("2025-01-06","2025-01-06").unwrap();
        assert_eq!(d.total_seconds,3600.0);assert_eq!(d.app_total_seconds,7200.0);assert_eq!(d.active_seconds,3600.0);assert_eq!(d.switches,1);
        assert_eq!(d.apps.len(),2);assert!(d.apps.iter().all(|a|a.seconds==3600.0));
        db.update_app("discord.exe","social",true).unwrap();let d=db.dashboard("2025-01-06","2025-01-06").unwrap();
        assert_eq!(d.total_seconds,3600.0);assert_eq!(d.app_total_seconds,3600.0);
        let mut clock=segment(start+3600_000,start+7200_000);clock.kind="computer".into();clock.app_id="__computer__".into();
        let mut sleep=clock.clone();sleep.kind="sleep".into();sleep.app_id="__sleep__".into();
        db.record_batch(&[clock,sleep]).unwrap();let d=db.dashboard("2025-01-06","2025-01-06").unwrap();
        assert_eq!(d.total_seconds,7200.0);assert_eq!(d.sleep_seconds,3600.0);assert_eq!(d.awake_seconds,3600.0);
    }
    #[test] fn upgrades_v1_without_discarding_settings_or_old_idle() {
        let path=std::env::temp_dir().join(format!("timewrapped-migrate-{}.sqlite3",Utc::now().timestamp_nanos_opt().unwrap()));
        let start=local_stamp("2025-01-06",12,0);
        {let db=Store::open(&path).unwrap();db.save_settings(&Settings{idle_seconds:300,..Settings::default()}).unwrap();
            db.conn.execute_batch("DROP TABLE segments;CREATE TABLE segments(id INTEGER PRIMARY KEY,app_id TEXT,kind TEXT,started_at INTEGER,ended_at INTEGER,switches INTEGER);PRAGMA user_version=1;").unwrap();
            db.conn.execute("INSERT INTO apps(id,name,exe) VALUES('code.exe','Code','code.exe')",[]).unwrap();
            for (kind,id,a,b) in [("active","code.exe",start,start+60_000),("idle","__idle__",start+60_000,start+90_000)] {
                db.conn.execute("INSERT INTO segments(app_id,kind,started_at,ended_at,switches) VALUES(?1,?2,?3,?4,0)",params![id,kind,a,b]).unwrap();
                db.conn.execute("INSERT INTO buckets(date,hour,app_id,kind,duration_ms,switches) VALUES('2025-01-06',12,?1,?2,?3,0)",params![id,kind,b-a]).unwrap();
            }
        }
        {let db=Store::open(&path).unwrap();let d=db.dashboard("2025-01-06","2025-01-06").unwrap();
            assert_eq!(d.total_seconds,90.0);assert_eq!(d.apps[0].seconds,60.0);assert_eq!(d.idle_seconds,30.0);
            assert_eq!(db.settings().unwrap().idle_seconds,300);assert!(path.with_extension("pre-v0.2.sqlite3").exists());
            assert_eq!(db.conn.query_row("PRAGMA integrity_check",[],|r|r.get::<_,String>(0)).unwrap(),"ok");}
        let _=std::fs::remove_file(path.with_extension("pre-v0.2.sqlite3"));let _=std::fs::remove_file(&path);
    }
    #[test] fn spotify_music_background_and_visible_streams_never_get_mixed() {
        let mut db=memory();let start=local_stamp("2025-01-06",12,0);
        let row=|kind:&str,a:i64,b:i64|Segment{app_id:"spotify.exe".into(),name:"Spotify".into(),exe:"Spotify.exe".into(),started_at:start+a,ended_at:start+b,switches:0,kind:kind.into()};
        db.record_batch(&[row("running",0,120_000),row("visible",0,30_000),row("background",30_000,120_000),row("music",20_000,80_000)]).unwrap();
        let d=db.dashboard("2025-01-06","2025-01-06").unwrap();assert_eq!(d.spotify_seconds,60.0);assert_eq!(d.total_seconds,0.0);
        let a=&d.apps[0];assert_eq!(a.seconds,30.0);assert_eq!(a.background_seconds,90.0);assert_eq!(a.running_seconds,120.0);
        assert!(db.export_json().unwrap().contains("music"));
        db.update_app("spotify.exe","entertainment",true).unwrap();let d=db.dashboard("2025-01-06","2025-01-06").unwrap();assert_eq!(d.spotify_seconds,0.0);assert!(d.apps.is_empty());
    }
    #[test] fn version_two_backfill_only_uses_known_visible_intervals() {
        let path=std::env::temp_dir().join(format!("timewrapped-v3-{}.sqlite3",Utc::now().timestamp_nanos_opt().unwrap()));
        let start=local_stamp("2025-01-06",12,0);
        {let mut db=Store::open(&path).unwrap();let mut row=segment(start,start+60_000);row.kind="visible".into();db.record_batch(&[row]).unwrap();db.conn.execute_batch("PRAGMA user_version=2;").unwrap();}
        {let mut db=Store::open(&path).unwrap();let d=db.dashboard("2025-01-06","2025-01-06").unwrap();assert_eq!(d.apps[0].running_seconds,60.0);assert_eq!(d.apps[0].background_seconds,0.0);assert_eq!(d.spotify_seconds,0.0);assert!(path.with_extension("pre-v0.3.sqlite3").exists());db.clear_data().unwrap();assert!(!path.with_extension("pre-v0.3.sqlite3").exists());}
        let _=std::fs::remove_file(path);
    }
    #[test] fn csv_escapes_quotes_commas_and_formula_prefix() {
        assert_eq!(csv("a,\"b"),"\"a,\"\"b\"");assert_eq!(csv("=1+1"),"\"'=1+1\"");
    }
    #[test] fn language_defaults_and_persistence_are_backward_compatible(){
        let old:Settings=serde_json::from_str(r#"{"tracking_enabled":true,"idle_seconds":180,"autostart":false,"notifications":true}"#).unwrap();assert_eq!(old.language,"ru");
        let db=memory();for language in ["en","uk","ru"]{let value=Settings{language:language.into(),..Settings::default()};db.save_settings(&value).unwrap();assert_eq!(db.settings().unwrap().language,language);}
        assert!(db.save_settings(&Settings{language:"invalid".into(),..Settings::default()}).is_err());assert_eq!(db.settings().unwrap().language,"ru");
    }
    #[test] fn v1_catalog_hides_old_helpers_without_deleting_sessions_or_pc_time() {
        let path=std::env::temp_dir().join(format!("timewrapped-catalog-{}.sqlite3",Utc::now().timestamp_nanos_opt().unwrap()));
        let start=local_stamp("2025-01-06",12,0);
        {
            let mut db=Store::open(&path).unwrap();
            db.save_settings(&Settings{idle_seconds:300,..Settings::default()}).unwrap();
            let mut clock=segment(start,start+60_000);clock.app_id="__computer__".into();clock.kind="computer".into();db.record_batch(&[clock]).unwrap();
            for (exe,kind) in [("discord.exe","visible"),("cargo.exe","active"),("unknown-worker.exe","running")] {
                db.conn.execute("INSERT INTO apps(id,name,exe) VALUES(?1,?1,?1)",[exe]).unwrap();
                db.conn.execute("INSERT INTO segments(app_id,kind,started_at,ended_at,switches) VALUES(?1,?2,?3,?4,0)",params![exe,kind,start,start+60_000]).unwrap();
                db.conn.execute("INSERT INTO buckets VALUES('2025-01-06',12,?1,?2,60000,0)",params![exe,kind]).unwrap();
            }
            db.conn.execute_batch("DROP TABLE app_catalog;PRAGMA user_version=3;").unwrap();
        }
        {
            let mut db=Store::open(&path).unwrap();
            assert_eq!(db.apps().unwrap().iter().map(|a|a.id.as_str()).collect::<Vec<_>>(),vec!["discord.exe"]);
            let report=db.dashboard("2025-01-06","2025-01-06").unwrap();assert_eq!(report.total_seconds,60.);assert_eq!(report.apps.len(),1);
            assert_eq!(db.conn.query_row("SELECT COUNT(*) FROM segments",[],|r|r.get::<_,i64>(0)).unwrap(),4);
            assert_eq!(db.settings().unwrap().idle_seconds,300);
            assert!(path.with_extension("pre-v1.0.sqlite3").exists());
            assert!(!db.export_json().unwrap().contains("cargo.exe"));
            let mut helper=segment(start+60_000,start+90_000);helper.app_id="codex.exe".into();helper.exe="codex.exe".into();helper.kind="visible".into();db.record_batch(&[helper]).unwrap();
            assert_eq!(db.apps().unwrap().len(),1);db.clear_data().unwrap();
            assert!(!path.with_extension("pre-v1.0.sqlite3").exists());
        }
        let _=std::fs::remove_file(path);
    }
    #[test] fn proven_minimized_apps_can_have_running_time_before_first_visible_interval() {
        let mut db=memory();let start=local_stamp("2025-01-06",12,0);
        db.register_applications(&[crate::tracker::Identity{id:"spotify.exe".into(),name:"Spotify".into(),exe:"Spotify.exe".into()}]).unwrap();
        let mut row=segment(start,start+60_000);row.app_id="spotify.exe".into();row.exe="Spotify.exe".into();row.kind="running".into();db.record_batch(&[row]).unwrap();
        let apps=db.dashboard("2025-01-06","2025-01-06").unwrap().apps;assert_eq!(apps.len(),1);assert_eq!(apps[0].running_seconds,60.);assert_eq!(apps[0].seconds,0.);
    }
}
