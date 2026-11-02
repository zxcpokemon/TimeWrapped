//! Window geometry only. Never reads window captions, pixels, documents or input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect { pub left:i32, pub top:i32, pub right:i32, pub bottom:i32 }
impl Rect {
    fn intersection(self,b:Self)->Option<Self> {
        let r=Self{left:self.left.max(b.left),top:self.top.max(b.top),right:self.right.min(b.right),bottom:self.bottom.min(b.bottom)};
        (r.right>r.left && r.bottom>r.top).then_some(r)
    }
    fn subtract(self,b:Self,out:&mut Vec<Self>) {
        let Some(i)=self.intersection(b) else {out.push(self);return;};
        for r in [Self{bottom:i.top,..self},Self{top:i.bottom,..self},
            Self{top:i.top,bottom:i.bottom,right:i.left,..self},Self{top:i.top,bottom:i.bottom,left:i.right,..self}] {
            if r.right>r.left && r.bottom>r.top {out.push(r);}
        }
    }
}
pub struct Coverage { remaining:Vec<Rect> }
impl Coverage {
    pub fn new(monitors:Vec<Rect>)->Self {Self{remaining:monitors}}
    pub fn visit(&mut self,rect:Rect,opaque:bool)->bool {
        let visible=self.remaining.iter().any(|r|r.intersection(rect).is_some());
        if visible && opaque {
            let mut next=Vec::new();
            for r in &self.remaining {r.subtract(rect,&mut next);}
            // A complex desktop must never monopolize the collector thread.
            // Keeping the old mask is conservative (possible exposure overcount).
            if next.len()<=2048 {self.remaining=next;}
        }
        visible
    }
}

#[cfg(windows)]
pub fn snapshot()->(Vec<windows_sys::Win32::Foundation::HWND>,usize) {
    use std::{mem::{size_of,zeroed},ptr::{null,null_mut}};
    use windows_sys::Win32::{Foundation::*,Graphics::{Dwm::*,Gdi::*},UI::WindowsAndMessaging::*};
    unsafe extern "system" fn monitor(_:HMONITOR,_:HDC,r:*mut RECT,data:LPARAM)->i32 {
        if !r.is_null(){let r=*r;(&mut *(data as *mut Vec<Rect>)).push(Rect{left:r.left,top:r.top,right:r.right,bottom:r.bottom});}1
    }
    unsafe {
        let mut monitors=Vec::new();EnumDisplayMonitors(null_mut(),null(),Some(monitor),&mut monitors as *mut _ as isize);
        let count=monitors.len();let mut coverage=Coverage::new(monitors);let mut result=Vec::new();
        let mut window=GetTopWindow(null_mut());let mut seen=std::collections::HashSet::new();
        for _ in 0..4096 {
            if window.is_null() || !seen.insert(window as usize){break;}
            let current=window;window=GetWindow(window,GW_HWNDNEXT);
            if IsWindowVisible(current)==0 || IsIconic(current)!=0 {continue;}
            let mut cloaked=0u32;
            if DwmGetWindowAttribute(current,DWMWA_CLOAKED as u32,&mut cloaked as *mut _ as *mut _,size_of::<u32>() as u32)>=0 && cloaked!=0 {continue;}
            let ex=GetWindowLongPtrW(current,GWL_EXSTYLE) as u32;
            let mut name=[0u16;128];let len=GetClassNameW(current,name.as_mut_ptr(),name.len() as i32);
            let class=String::from_utf16_lossy(&name[..len.max(0) as usize]);
            // Invisible input/overlay/tool windows may have a full-monitor
            // bounding box with a transparent DWM surface. They must not erase
            // ordinary restored windows behind them from the coverage mask.
            if ["Progman","WorkerW","Shell_TrayWnd","Shell_SecondaryTrayWnd"].contains(&class.as_str()) || ex&WS_EX_TOOLWINDOW!=0 {continue;}
            let mut rect:RECT=zeroed();
            let bounds=DwmGetWindowAttribute(current,DWMWA_EXTENDED_FRAME_BOUNDS as u32,&mut rect as *mut _ as *mut _,size_of::<RECT>() as u32);
            if (bounds<0 || rect.right<=rect.left || rect.bottom<=rect.top) && GetWindowRect(current,&mut rect)==0 {continue;}
            let mut opaque=ex&WS_EX_TRANSPARENT==0;
            if ex&WS_EX_LAYERED!=0 {
                let mut color=0;let mut alpha=0;let mut flags=0;
                opaque=opaque && GetLayeredWindowAttributes(current,&mut color,&mut alpha,&mut flags)!=0 && flags&LWA_ALPHA!=0 && alpha==255;
            }
            if !coverage.visit(Rect{left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom},opaque){continue;}
            result.push(current);
        }
        (result,count)
    }
}

#[cfg(test)] mod tests {
    use super::*;
    fn r(l:i32,t:i32,r:i32,b:i32)->Rect {Rect{left:l,top:t,right:r,bottom:b}}
    #[test] fn second_monitor_and_negative_coordinates_are_included() {
        let mut c=Coverage::new(vec![r(-1920,0,0,1080),r(0,0,1920,1080)]);
        assert!(c.visit(r(0,0,1920,1080),true));assert!(c.visit(r(-1920,0,0,1080),true));
        assert!(!c.visit(r(-1920,0,1920,1080),true));
    }
    #[test] fn covered_offscreen_minuscule_gaps_and_transparency() {
        let mut c=Coverage::new(vec![r(0,0,100,100)]);
        assert!(!c.visit(r(101,0,200,100),true));assert!(c.visit(r(0,0,100,100),false));
        assert!(c.visit(r(0,0,50,100),true));assert!(!c.visit(r(0,0,49,100),true));
        assert!(c.visit(r(0,0,51,100),true));assert!(c.visit(r(50,0,100,100),true));
    }
    #[test] fn restored_partial_windows_receive_exposure() {
        let mut c=Coverage::new(vec![r(0,0,1920,1080)]);
        assert!(c.visit(r(700,100,1500,800),true));
        assert!(c.visit(r(100,200,900,900),true));
        assert!(!c.visit(r(800,300,850,400),true));
    }
    #[test] fn monitor_gaps_are_not_visible_area() {
        let mut c=Coverage::new(vec![r(0,0,100,100),r(200,0,300,100)]);
        assert!(!c.visit(r(110,0,190,100),true));assert!(c.visit(r(50,0,250,100),true));
    }
}
