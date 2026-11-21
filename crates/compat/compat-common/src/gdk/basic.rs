use super::win::wide;
use super::{cstr, log, setting, Guid, Iface, This};
use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

pub const XERROR: Guid = Guid::parse("8ca467f7-22e8-4096-8456-bb8aa13f79d8");
pub const FEATURE: Guid = Guid::parse("8836fe87-edb9-4fe3-8dad-05f0d2cd5b40");
pub const DISPLAY: Guid = Guid::parse("03f0fe74-fdd9-4e5c-b630-f9339c47acc5");
pub const DISPLAY_IID: Guid = Guid::parse("35f07670-706e-4bfb-9476-090798c5ebf3");

#[link(name = "kernel32")]
extern "system" {
    fn IsDebuggerPresent() -> i32;
    fn OutputDebugStringA(s: *const u8);
    fn DebugBreak();
    fn GetCurrentThreadId() -> u32;
    fn RaiseFailFastException(r: *const c_void, c: *const c_void, flags: u32);
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
}

#[link(name = "user32")]
extern "system" {
    fn MonitorFromWindow(h: usize, flags: u32) -> usize;
}

pub const OPT_OUTPUT_DEBUG_STRING: u32 = 1;
pub const OPT_DEBUG_BREAK: u32 = 2;
pub const OPT_FAIL_FAST: u32 = 4;

static DEBUGGER_OPTIONS: AtomicU32 = AtomicU32::new(OPT_OUTPUT_DEBUG_STRING);
static NO_DEBUGGER_OPTIONS: AtomicU32 = AtomicU32::new(0);
static CALLBACK: AtomicUsize = AtomicUsize::new(0);
static CONTEXT: AtomicUsize = AtomicUsize::new(0);
static BREAK_WARNED: AtomicBool = AtomicBool::new(false);
static REPORTS: AtomicU32 = AtomicU32::new(0);

pub fn error_options() -> (u32, u32) {
    (DEBUGGER_OPTIONS.load(Ordering::Acquire), NO_DEBUGGER_OPTIONS.load(Ordering::Acquire))
}

pub fn error_reports() -> u32 {
    REPORTS.load(Ordering::Acquire)
}

type ErrorCallback = unsafe extern "system" fn(i32, *const u8, *mut c_void) -> bool;

pub unsafe fn report(hr: i32, msg: *const u8) {
    REPORTS.fetch_add(1, Ordering::AcqRel);
    let text = cstr(msg).unwrap_or_default();
    let debugger = IsDebuggerPresent() != 0;
    let opts = if debugger { DEBUGGER_OPTIONS.load(Ordering::Acquire) } else { NO_DEBUGGER_OPTIONS.load(Ordering::Acquire) };
    log("XErrorReport", &format!("hr={:#010x} {text}", hr as u32));
    let cb = CALLBACK.load(Ordering::Acquire);
    if cb != 0 {
        let f: ErrorCallback = core::mem::transmute(cb);
        if f(hr, msg, CONTEXT.load(Ordering::Acquire) as *mut c_void) {
            return;
        }
    }
    let tid = GetCurrentThreadId();
    if opts & (OPT_OUTPUT_DEBUG_STRING | OPT_DEBUG_BREAK) != 0 {
        let line = if text.len() >= 0xc0 {
            format!("XERROR: HR:{:08X} threadID:{tid} \0", hr as u32)
        } else {
            format!("XERROR: HR:{:08X} threadID:{tid} {text}\n\0", hr as u32)
        };
        OutputDebugStringA(line.as_ptr());
        if text.len() >= 0xc0 {
            let t = format!("{text}\0");
            OutputDebugStringA(t.as_ptr());
            OutputDebugStringA(b"\n\0".as_ptr());
        }
    }
    if opts & OPT_DEBUG_BREAK != 0 && setting("xerror_disable_break").as_deref() != Some("1") {
        if !BREAK_WARNED.swap(true, Ordering::AcqRel) {
            OutputDebugStringA(b"XERROR: This break was triggered because XErrorSetOptions was called with DebugBreakOnError.\n\0".as_ptr());
        }
        DebugBreak();
    }
    if opts & OPT_FAIL_FAST != 0 {
        log("XErrorReport", "FailFastOnError set: terminating");
        RaiseFailFastException(core::ptr::null(), core::ptr::null(), 0);
    }
}

unsafe extern "system" fn error_report(_t: This, hr: i32, msg: *const u8) {
    report(hr, msg)
}

unsafe extern "system" fn error_set_callback(_t: This, cb: usize, ctx: usize) {
    CONTEXT.store(ctx, Ordering::Release);
    CALLBACK.store(cb, Ordering::Release);
}

unsafe extern "system" fn error_set_options(_t: This, with_debugger: u32, without_debugger: u32) {
    DEBUGGER_OPTIONS.store(with_debugger, Ordering::Release);
    NO_DEBUGGER_OPTIONS.store(without_debugger, Ordering::Release);
    log("XErrorSetOptions", &format!("debugger={with_debugger:#x} noDebugger={without_debugger:#x}"));
}

pub const FEATURE_COUNT: u32 = 22;
pub const FEATURE_GAME_STREAMING: u32 = 21;

pub fn feature_available(f: u32) -> bool {
    f < FEATURE_COUNT && f != FEATURE_GAME_STREAMING
}

unsafe extern "system" fn is_feature_available(_t: This, f: u32) -> bool {
    feature_available(f)
}

pub const HDR_UNKNOWN: u32 = 0;
pub const HDR_ENABLED: u32 = 1;
pub const HDR_DISABLED: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HdrState {
    pub enabled: bool,
    pub min: f32,
    pub max: f32,
    pub max_full_frame: f32,
}

unsafe fn vslot(obj: *mut c_void, n: usize) -> usize {
    *(*(obj as *const *const usize)).add(n)
}

unsafe fn release(obj: *mut c_void) {
    if !obj.is_null() {
        let f: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(vslot(obj, 2));
        f(obj);
    }
}

const IID_FACTORY1: Guid = Guid::parse("770aae78-f26f-4dba-a829-253c83d1b387");
const IID_OUTPUT6: Guid = Guid::parse("068346e8-aaec-4b84-add7-137f513f77a1");
const COLOR_SPACE_HDR10: u32 = 12;

pub fn hdr_state() -> Result<HdrState, i32> {
    unsafe {
        let m = LoadLibraryW(wide("dxgi.dll").as_ptr());
        if m.is_null() {
            return Err(super::win::last_error_hr());
        }
        let p = GetProcAddress(m, b"CreateDXGIFactory1\0".as_ptr());
        if p.is_null() {
            return Err(super::win::last_error_hr());
        }
        let create: unsafe extern "system" fn(*const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(p);
        let mut f = core::ptr::null_mut();
        let hr = create(&IID_FACTORY1, &mut f);
        if hr < 0 {
            return Err(hr);
        }
        let want = super::win::main_window().map(|h| MonitorFromWindow(h, 1)).unwrap_or(0);
        let enum_adapters: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 = core::mem::transmute(vslot(f, 12));
        let mut best: Option<HdrState> = None;
        let mut ai = 0;
        loop {
            let mut a = core::ptr::null_mut();
            if enum_adapters(f, ai, &mut a) < 0 {
                break;
            }
            ai += 1;
            let enum_outputs: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 = core::mem::transmute(vslot(a, 7));
            let mut oi = 0;
            loop {
                let mut o = core::ptr::null_mut();
                if enum_outputs(a, oi, &mut o) < 0 {
                    break;
                }
                oi += 1;
                let qi: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(vslot(o, 0));
                let mut o6 = core::ptr::null_mut();
                if qi(o, &IID_OUTPUT6, &mut o6) >= 0 {
                    let desc1: unsafe extern "system" fn(*mut c_void, *mut u8) -> i32 = core::mem::transmute(vslot(o6, 27));
                    let mut d = [0u8; 152];
                    if desc1(o6, d.as_mut_ptr()) >= 0 {
                        let rd = |off: usize| f32::from_le_bytes(d[off..off + 4].try_into().unwrap());
                        let monitor = usize::from_le_bytes(d[88..96].try_into().unwrap());
                        let cs = u32::from_le_bytes(d[100..104].try_into().unwrap());
                        let st = HdrState { enabled: cs == COLOR_SPACE_HDR10, min: rd(136), max: rd(140), max_full_frame: rd(144) };
                        if monitor == want || best.is_none() {
                            best = Some(st);
                        }
                    }
                    release(o6);
                }
                release(o);
            }
            release(a);
        }
        release(f);
        best.ok_or(super::E_NOT_FOUND)
    }
}

static HDR_ON: AtomicBool = AtomicBool::new(false);

pub fn hdr_active() -> bool {
    HDR_ON.load(Ordering::Acquire)
}

pub fn try_enable_hdr(preference: u32, info: *mut f32) -> u32 {
    unsafe {
        if !info.is_null() {
            core::ptr::write_bytes(info, 0, 3);
        }
    }
    if preference > 1 {
        return HDR_UNKNOWN;
    }
    let forced = setting("display_hdr").map(|v| v.to_ascii_lowercase());
    let st = match hdr_state() {
        Ok(s) => s,
        Err(hr) => {
            log("XDisplayTryEnableHdrMode", &format!("display query failed hr={:#010x}", hr as u32));
            return HDR_UNKNOWN;
        }
    };
    let enabled = match forced.as_deref() {
        Some("0") | Some("off") => false,
        _ => st.enabled,
    };
    log("XDisplayTryEnableHdrMode", &format!("preference={preference} display hdr={} -> {}", st.enabled, if enabled { "Enabled" } else { "Disabled" }));
    if !enabled {
        return HDR_DISABLED;
    }
    HDR_ON.store(true, Ordering::Release);
    if !info.is_null() {
        unsafe {
            *info = st.min;
            *info.add(1) = st.max;
            *info.add(2) = st.max_full_frame;
        }
    }
    HDR_ENABLED
}

unsafe extern "system" fn display_try_enable_v1(_t: This, info: *mut f32) -> u32 {
    try_enable_hdr(0, info)
}

unsafe extern "system" fn display_try_enable(_t: This, preference: u32, info: *mut f32) -> u32 {
    try_enable_hdr(preference, info)
}

pub fn interfaces() -> Vec<Iface> {
    vec![
        Iface { clsid: XERROR, iid: XERROR, name: "XError(8ca467f7)", slots: vec![(3, error_report as usize), (4, error_set_callback as usize), (5, error_set_options as usize)] },
        Iface { clsid: FEATURE, iid: FEATURE, name: "XGameRuntimeFeature(8836fe87)", slots: vec![(3, is_feature_available as usize)] },
        Iface { clsid: DISPLAY, iid: DISPLAY_IID, name: "XDisplay(35f07670)", slots: vec![(3, display_try_enable_v1 as usize), (4, display_try_enable as usize)] },
    ]
}
