//! Pure interval attribution. Input contains only timings and executable identity.
use crate::store::Segment;

/// Extend GetLastInputInfo's 32-bit tick count while the process remains alive.
/// Reusing an unchanged input tick prevents a phantom input every 49.7 days.
#[derive(Default)]
pub struct InputClock {
    previous: Option<(u32, u64)>,
}
impl InputClock {
    pub const fn new() -> Self {Self {previous:None}}
    pub fn idle_ms(&mut self, uptime:u64, raw_input:u32)->u64 {
        let input=match self.previous {
            Some((old_raw,extended)) if old_raw==raw_input=>extended,
            _=>uptime.saturating_sub((uptime as u32).wrapping_sub(raw_input) as u64),
        };
        self.previous=Some((raw_input,input));
        uptime.saturating_sub(input)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub exe: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub wall_ms: i64,
    pub uptime_ms: u64,
    /// Windows unbiased interrupt time excludes suspend/hibernate duration.
    pub awake_ms: u64,
    pub idle_ms: u64,
}

pub struct Tracker {
    previous: Sample,
    timeline_end:i64,
    last_input: u64,
    app: Option<Identity>,
    visible: Vec<Identity>,
    running: Vec<Identity>,
    spotify_playing: bool,
    pub enabled: bool,
    pub blocked: bool,
    threshold_ms: u64,
    pending_switch: bool,
}

impl Tracker {
    pub fn new(sample: Sample, enabled: bool, idle_seconds: u32) -> Self {
        Self {
            previous: sample,timeline_end:sample.wall_ms,
            last_input: sample.uptime_ms.saturating_sub(sample.idle_ms),
            app: None,
            visible: Vec::new(),
            running: Vec::new(),spotify_playing:false,
            enabled,
            blocked: false,
            threshold_ms: u64::from(idle_seconds).max(1) * 1000,
            pending_switch: false,
        }
    }

    pub fn set_app(&mut self, app: Option<Identity>) {
        if self.app.as_ref().map(|a| &a.id) != app.as_ref().map(|a| &a.id) {
            self.pending_switch = app.is_some();
        }
        self.app = app;
    }

    pub fn set_visible(&mut self, apps:Vec<Identity>) {
        let mut unique=std::collections::BTreeMap::new();
        for app in apps {unique.entry(app.id.clone()).or_insert(app);}
        self.visible=unique.into_values().collect();
    }

    pub fn set_running(&mut self,apps:Vec<Identity>){
        let mut unique=std::collections::BTreeMap::new();for a in apps{unique.entry(a.id.clone()).or_insert(a);}self.running=unique.into_values().collect();
    }
    pub fn set_spotify(&mut self,playing:bool){self.spotify_playing=playing;}

    pub fn configure(&mut self, enabled: bool, idle_seconds: u32) {
        if !self.enabled && enabled {
            self.pending_switch = self.app.is_some();
        }
        self.enabled = enabled;
        self.threshold_ms = u64::from(idle_seconds).max(1) * 1000;
    }

    pub fn advance(&mut self, sample: Sample) -> Vec<Segment> {
        let old=self.previous;let old_input=self.last_input;
        let new_input=sample.uptime_ms.saturating_sub(sample.idle_ms);
        self.previous=sample;self.last_input=new_input;
        let elapsed=sample.uptime_ms.saturating_sub(old.uptime_ms);
        if !self.enabled || elapsed>366*24*60*60*1000 {self.timeline_end=sample.wall_ms;return Vec::new();}
        if elapsed==0 {return Vec::new();}
        let awake=sample.awake_ms.saturating_sub(old.awake_ms).min(elapsed);
        let sleep=elapsed-awake;
        // GetTickCount64 has coarser resolution than the wall clock. Anchor to
        // the previous monotonic endpoint, rather than introducing a gap/overlap
        // on every event. Re-anchor only on a real clock adjustment (>2 seconds).
        let proposed=self.timeline_end.saturating_add(elapsed as i64);
        let clock_changed=(sample.wall_ms-proposed).abs()>2_000;
        let end=if clock_changed{sample.wall_ms}else{proposed};
        let start=end-elapsed as i64;self.timeline_end=end;
        let mut result=vec![system_segment("computer",start,end)];
        if sleep>100 {result.push(system_segment("sleep",start,start+sleep as i64));}
        // Do not assign uncertain post-suspend/stalled intervals to an app.
        if sleep>100 || elapsed>15_000 || clock_changed {
            if awake>0 {result.push(system_segment("unattributed",end-awake as i64,end));}
            return result;
        }
        let mut running=std::collections::BTreeMap::new();
        for app in self.running.iter().chain(self.visible.iter()){running.entry(app.id.clone()).or_insert(app);}
        for app in running.into_values(){
            result.push(app_segment(app,"running",start,end));
            if self.blocked || !self.visible.iter().any(|a|a.id==app.id){result.push(app_segment(app,"background",start,end));}
        }
        if self.spotify_playing{result.push(app_segment(&Identity{id:"spotify.exe".into(),name:"Spotify".into(),exe:"Spotify.exe".into()},"music",start,end));}
        if self.blocked {result.push(system_segment("locked",start,end));return result;}
        // Visible exposure is independent of keyboard idle (video, a game or a
        // second-monitor conversation remains on screen). One interval per exe.
        for app in &self.visible {
            result.push(Segment{app_id:app.id.clone(),name:app.name.clone(),exe:app.exe.clone(),started_at:start,
                ended_at:end,switches:0,kind:"visible".into()});
        }
        let mut points=vec![old.uptime_ms,sample.uptime_ms];
        for point in [old_input.saturating_add(self.threshold_ms),new_input,new_input.saturating_add(self.threshold_ms)] {
            if point>old.uptime_ms && point<sample.uptime_ms {points.push(point);}
        }
        points.sort_unstable();points.dedup();
        for interval in points.windows(2) {
            let a=interval[0];let b=interval[1];
            let input=if new_input>old.uptime_ms && a<new_input {old_input}else{new_input};
            let idle=a>=input.saturating_add(self.threshold_ms);
            let left=start+(a-old.uptime_ms) as i64;let right=start+(b-old.uptime_ms) as i64;
            if idle {result.push(system_segment("idle",left,right));}
            else if let Some(app)=&self.app {
                let switches=u32::from(self.pending_switch);self.pending_switch=false;
                result.push(Segment{app_id:app.id.clone(),name:app.name.clone(),exe:app.exe.clone(),
                    started_at:left,ended_at:right,switches,kind:"active".into()});
            }else{result.push(system_segment("unattributed",left,right));}
        }
        result
    }

}

fn app_segment(app:&Identity,kind:&str,start:i64,end:i64)->Segment {
    Segment{app_id:app.id.clone(),name:app.name.clone(),exe:app.exe.clone(),started_at:start,ended_at:end,switches:0,kind:kind.into()}
}

fn system_segment(kind:&str,start:i64,end:i64)->Segment {
    Segment{app_id:format!("__{kind}__"),name:kind.into(),exe:String::new(),started_at:start,ended_at:end,switches:0,kind:kind.into()}
}

#[cfg(test)] mod tests {
    use super::*;
    fn s(t:u64,idle:u64)->Sample {Sample{wall_ms:1_700_000_000_000+t as i64,uptime_ms:t,awake_ms:t,idle_ms:idle}}
    fn app(id:&str)->Identity {Identity{id:id.into(),name:id.into(),exe:id.into()}}
    fn total(rows:&[Segment],kind:&str)->i64 {rows.iter().filter(|r|r.kind==kind).map(|r|r.ended_at-r.started_at).sum()}
    #[test] fn idle_never_stops_computer_or_visible_app_time() {
        let mut t=Tracker::new(s(179_000,179_000),true,180);t.set_app(Some(app("a.exe")));
        t.set_visible(vec![app("a.exe"),app("b.exe"),app("a.exe")]);
        let rows=t.advance(s(184_000,184_000));
        assert_eq!(total(&rows,"computer"),5000);assert_eq!(total(&rows,"active"),1000);
        assert_eq!(total(&rows,"idle"),4000);assert_eq!(total(&rows,"visible"),10000);
        assert_eq!(rows.iter().filter(|r|r.kind=="visible"&&r.app_id=="a.exe").count(),1);
        let rows=t.advance(s(189_000,2000));assert_eq!(total(&rows,"active"),2000);assert_eq!(total(&rows,"idle"),3000);
    }
    #[test] fn locked_unknown_and_paused_are_distinct() {
        let mut t=Tracker::new(s(0,0),true,180);t.set_visible(vec![app("a.exe")]);t.blocked=true;
        let rows=t.advance(s(5000,0));assert_eq!(total(&rows,"computer"),5000);assert_eq!(total(&rows,"locked"),5000);assert_eq!(total(&rows,"visible"),0);
        t.blocked=false;let rows=t.advance(s(10000,0));assert_eq!(total(&rows,"unattributed"),5000);
        t.configure(false,180);assert!(t.advance(s(15000,0)).is_empty());
    }
    #[test] fn sleep_and_long_stalls_do_not_lose_powered_time_or_fabricate_apps() {
        let mut t=Tracker::new(s(0,0),true,180);t.set_app(Some(app("a.exe")));
        let mut wake=s(3_600_000,0);wake.awake_ms=1000;
        let rows=t.advance(wake);assert_eq!(total(&rows,"computer"),3_600_000);assert_eq!(total(&rows,"sleep"),3_599_000);assert_eq!(total(&rows,"active"),0);
        let rows=t.advance(s(3_660_000,0));assert_eq!(total(&rows,"computer"),60_000);assert_eq!(total(&rows,"active"),0);
    }
    #[test] fn clock_jump_preserves_elapsed_and_switches_do_not_inflate() {
        let mut t=Tracker::new(s(0,0),true,180);t.set_app(Some(app("a.exe")));
        let mut changed=s(5000,0);changed.wall_ms-=3_600_000;
        let rows=t.advance(changed);assert_eq!(total(&rows,"computer"),5000);assert_eq!(total(&rows,"active"),0);
        changed.uptime_ms+=5000;changed.awake_ms+=5000;changed.wall_ms+=5000;
        let rows=t.advance(changed);assert_eq!(rows.iter().map(|r|r.switches).sum::<u32>(),1);
        changed.uptime_ms+=5000;changed.awake_ms+=5000;changed.wall_ms+=5000;
        assert_eq!(t.advance(changed).iter().map(|r|r.switches).sum::<u32>(),0);
    }
    #[test] fn input_tick_does_not_reset_after_49_days() {
        let mut c=InputClock::new();assert_eq!(c.idle_ms(2000,1000),1000);
        assert_eq!(c.idle_ms((1u64<<32)+2000,1000),(1u64<<32)+1000);
    }
    #[test] fn wall_clock_jitter_and_zero_uptime_ticks_do_not_split_or_overlap_intervals() {
        let mut t=Tracker::new(s(0,0),true,180);t.set_visible(vec![app("a.exe")]);
        let mut previous=None;let mut observed=0;
        for n in 1..=200 {
            let mut tick=s(n*16,0);tick.wall_ms+=if n%2==0{7}else{-7};
            let rows=t.advance(tick);let computer=rows.iter().find(|r|r.kind=="computer").unwrap();
            if let Some(end)=previous {assert_eq!(computer.started_at,end);}
            previous=Some(computer.ended_at);observed+=total(&rows,"computer");
            tick.wall_ms+=2;assert!(t.advance(tick).is_empty());
        }
        assert_eq!(observed,3200);assert_eq!(previous,Some(s(3200,0).wall_ms));
    }
    #[test] fn visible_background_and_running_are_distinct_and_deduplicated() {
        let mut t=Tracker::new(s(0,0),true,180);t.set_visible(vec![app("a.exe")]);t.set_running(vec![app("a.exe"),app("a.exe"),app("spotify.exe")]);
        let rows=t.advance(s(5000,0));assert_eq!(total(&rows,"running"),10000);assert_eq!(total(&rows,"visible"),5000);assert_eq!(total(&rows,"background"),5000);assert_eq!(total(&rows,"music"),0);
        t.set_spotify(true);t.set_visible(vec![]);let rows=t.advance(s(10000,0));assert_eq!(total(&rows,"music"),5000);assert_eq!(total(&rows,"background"),10000);
        t.set_spotify(false);assert_eq!(total(&t.advance(s(15000,0)),"music"),0);
    }
    #[test] fn music_can_continue_on_lock_but_never_during_sleep_or_tracking_pause() {
        let mut t=Tracker::new(s(0,0),true,180);t.set_spotify(true);t.blocked=true;
        assert_eq!(total(&t.advance(s(5000,0)),"music"),5000);
        let mut wake=s(10000,0);wake.awake_ms=5000;assert_eq!(total(&t.advance(wake),"music"),0);
        t.configure(false,180);assert!(t.advance(s(15000,0)).is_empty());
    }
    #[test] fn simulated_24h_parallel_apps_do_not_multiply_pc_time() {
        let mut t=Tracker::new(s(0,0),true,180);t.set_app(Some(app("a.exe")));t.set_visible(vec![app("a.exe"),app("b.exe")]);
        let mut pc=0;let mut exposure=0;let mut locked=0;
        for step in 1..=17280 {t.blocked=step%720<60;let rows=t.advance(s(step*5000,0));pc+=total(&rows,"computer");exposure+=total(&rows,"visible");locked+=total(&rows,"locked");}
        assert_eq!(pc,86_400_000);assert_eq!(exposure,2*(pc-locked));
    }
}
