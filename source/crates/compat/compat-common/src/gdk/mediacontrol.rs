use super::log;
use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

#[link(name = "ole32")]
extern "system" {
    fn CoIncrementMTAUsage(cookie: *mut *mut c_void) -> i32;
    fn CoGetApartmentType(kind: *mut i32, qualifier: *mut i32) -> i32;
}

extern "system" {
    fn GetModuleFileNameW(m: *mut c_void, buf: *mut u16, n: u32) -> u32;
}

pub const EXPORTS: [&str; 8] = [
    "CreateRenderAudioStateMonitor",
    "CreateRenderAudioStateMonitorForCategory",
    "CreateRenderAudioStateMonitorForCategoryAndDeviceId",
    "CreateRenderAudioStateMonitorForCategoryAndDeviceRole",
    "CreateCaptureAudioStateMonitor",
    "CreateCaptureAudioStateMonitorForCategory",
    "CreateCaptureAudioStateMonitorForCategoryAndDeviceId",
    "CreateCaptureAudioStateMonitorForCategoryAndDeviceRole",
];

static REAL: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];

pub fn category_name(c: u32) -> &'static str {
    match c {
        0 => "Other",
        1 => "ForegroundOnlyMedia",
        2 => "BackgroundCapableMedia",
        3 => "Communications",
        4 => "Alerts",
        5 => "SoundEffects",
        6 => "GameEffects",
        7 => "GameMedia",
        8 => "GameChat",
        9 => "Speech",
        10 => "Movie",
        11 => "Media",
        12 => "FarFieldSpeech",
        13 => "UniformSpeech",
        14 => "VoiceTyping",
        _ => "Unknown",
    }
}

pub fn sound_level_name(l: i32) -> &'static str {
    match l {
        0 => "Muted",
        1 => "Low",
        2 => "Full",
        _ => "Unknown",
    }
}

fn apartment() -> (bool, String) {
    let (mut k, mut q) = (0i32, 0i32);
    let hr = unsafe { CoGetApartmentType(&mut k, &mut q) };
    if hr < 0 { (false, format!("none (hr={hr:#010x})")) } else { (true, format!("type {k} qualifier {q}")) }
}

fn ensure_mta() -> i32 {
    static MTA: OnceLock<i32> = OnceLock::new();
    *MTA.get_or_init(|| {
        let mut cookie = core::ptr::null_mut();
        let hr = unsafe { CoIncrementMTAUsage(&mut cookie) };
        log("AudioStateMonitor", &format!("process multithreaded apartment held for the title (CoIncrementMTAUsage hr={hr:#010x})"));
        hr
    })
}

unsafe fn sound_level(monitor: *mut c_void) -> i32 {
    let vt = *(monitor as *const *const usize);
    let f: unsafe extern "system" fn(*mut c_void) -> i32 = core::mem::transmute(*vt.add(5));
    f(monitor)
}

unsafe fn finish(i: usize, desc: String, hr: i32, out: *mut *mut c_void) -> i32 {
    let lvl = if hr >= 0 && !out.is_null() && !(*out).is_null() { format!(", sound level {}", sound_level_name(sound_level(*out))) } else { String::new() };
    log("AudioStateMonitor", &format!("{}({desc}) -> hr={hr:#010x}{lvl}", EXPORTS[i]));
    hr
}

unsafe fn real<T: Copy>(i: usize) -> Option<T> {
    let p = REAL[i].load(Ordering::Acquire);
    if p == 0 { None } else { Some(core::mem::transmute_copy::<usize, T>(&p)) }
}

const E_POINTER: i32 = 0x8000_4003u32 as i32;
const E_PROC_NOT_FOUND: i32 = 0x8007_007fu32 as i32;

macro_rules! plain {
    ($name:ident, $i:expr) => {
        unsafe extern "system" fn $name(out: *mut *mut c_void) -> i32 {
            if out.is_null() {
                return E_POINTER;
            }
            let (has_apartment, apt) = apartment();
            if !has_apartment {
                ensure_mta();
            }
            let Some(f) = real::<unsafe extern "system" fn(*mut *mut c_void) -> i32>($i) else { return E_PROC_NOT_FOUND };
            let hr = f(out);
            finish($i, format!("apartment before call {apt}"), hr, out)
        }
    };
}

macro_rules! category {
    ($name:ident, $i:expr) => {
        unsafe extern "system" fn $name(cat: u32, out: *mut *mut c_void) -> i32 {
            if out.is_null() {
                return E_POINTER;
            }
            let (has_apartment, apt) = apartment();
            if !has_apartment {
                ensure_mta();
            }
            let Some(f) = real::<unsafe extern "system" fn(u32, *mut *mut c_void) -> i32>($i) else { return E_PROC_NOT_FOUND };
            let hr = f(cat, out);
            finish($i, format!("category {cat} {}, apartment before call {apt}", category_name(cat)), hr, out)
        }
    };
}

macro_rules! device_id {
    ($name:ident, $i:expr) => {
        unsafe extern "system" fn $name(cat: u32, id: *const u16, out: *mut *mut c_void) -> i32 {
            if out.is_null() {
                return E_POINTER;
            }
            let (has_apartment, apt) = apartment();
            if !has_apartment {
                ensure_mta();
            }
            let Some(f) = real::<unsafe extern "system" fn(u32, *const u16, *mut *mut c_void) -> i32>($i) else { return E_PROC_NOT_FOUND };
            let hr = f(cat, id, out);
            finish($i, format!("category {cat} {}, device {:?}, apartment before call {apt}", category_name(cat), super::wstr(id).unwrap_or_default()), hr, out)
        }
    };
}

macro_rules! device_role {
    ($name:ident, $i:expr) => {
        unsafe extern "system" fn $name(cat: u32, role: u32, out: *mut *mut c_void) -> i32 {
            if out.is_null() {
                return E_POINTER;
            }
            let (has_apartment, apt) = apartment();
            if !has_apartment {
                ensure_mta();
            }
            let Some(f) = real::<unsafe extern "system" fn(u32, u32, *mut *mut c_void) -> i32>($i) else { return E_PROC_NOT_FOUND };
            let hr = f(cat, role, out);
            finish($i, format!("category {cat} {}, role {role}, apartment before call {apt}", category_name(cat)), hr, out)
        }
    };
}

plain!(render, 0);
category!(render_cat, 1);
device_id!(render_cat_id, 2);
device_role!(render_cat_role, 3);
plain!(capture, 4);
category!(capture_cat, 5);
device_id!(capture_cat_id, 6);
device_role!(capture_cat_role, 7);

fn wrappers() -> [usize; 8] {
    [
        render as usize,
        render_cat as usize,
        render_cat_id as usize,
        render_cat_role as usize,
        capture as usize,
        capture_cat as usize,
        capture_cat_id as usize,
        capture_cat_role as usize,
    ]
}

pub unsafe fn is_media_control(h: *mut c_void) -> bool {
    if h.is_null() {
        return false;
    }
    let mut buf = [0u16; 1024];
    let n = GetModuleFileNameW(h, buf.as_mut_ptr(), buf.len() as u32) as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())]).to_ascii_lowercase().ends_with("\\windows.media.mediacontrol.dll")
}

pub unsafe fn route(h: *mut c_void, name: *const u8, resolved: *mut c_void) -> *mut c_void {
    if resolved.is_null() || (name as usize) < 0x10000 || !is_media_control(h) {
        return resolved;
    }
    let Some(n) = super::cstr(name) else { return resolved };
    match EXPORTS.iter().position(|e| *e == n) {
        Some(i) => {
            REAL[i].store(resolved as usize, Ordering::Release);
            wrappers()[i] as *mut c_void
        }
        None => resolved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_sdk_enums() {
        assert_eq!(category_name(8), "GameChat");
        assert_eq!(category_name(6), "GameEffects");
        assert_eq!(sound_level_name(2), "Full");
        assert_eq!(EXPORTS.len(), wrappers().len());
    }
}
