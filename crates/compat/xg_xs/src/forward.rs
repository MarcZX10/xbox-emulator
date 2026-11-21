use core::ffi::c_void;
use std::sync::OnceLock;

extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, name: *const u16, out: *mut *mut c_void) -> i32;
}

const FROM_ADDRESS: u32 = 4;
const UNCHANGED_REFCOUNT: u32 = 2;

fn own_module() -> usize {
    let mut h: *mut c_void = core::ptr::null_mut();
    unsafe { GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, own_module as *const u16, &mut h) };
    h as usize
}

fn load() -> usize {
    let path = match std::env::var_os("TOMBOX_XG_CONSOLE") {
        Some(p) if !p.is_empty() => p,
        _ => return 0,
    };
    let p = std::path::Path::new(&path);
    if !p.is_absolute() {
        compat_common::record("xg_xs", "forward", "TOMBOX_XG_CONSOLE must be an absolute path; using native layouts");
        return 0;
    }
    use std::os::windows::ffi::OsStrExt;
    let w: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let h = unsafe { LoadLibraryW(w.as_ptr()) } as usize;
    if h == 0 || h == own_module() {
        compat_common::record("xg_xs", "forward", "console xg_xs.dll not loadable or is this module; using native layouts");
        return 0;
    }
    compat_common::record("xg_xs", "forward", &format!("forwarding to {}", p.display()));
    h
}

pub fn console(name: &str) -> Option<*mut c_void> {
    static H: OnceLock<usize> = OnceLock::new();
    let h = *H.get_or_init(load);
    if h == 0 {
        return None;
    }
    let f = unsafe { GetProcAddress(h as *mut c_void, name.as_ptr()) };
    if f.is_null() { None } else { Some(f) }
}
