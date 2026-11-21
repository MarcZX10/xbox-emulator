use core::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, name: *const u16, out: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(h: *mut c_void, buf: *mut u16, n: u32) -> u32;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

const M: &str = "sc_dll";
const FROM_ADDRESS: u32 = 4;
const UNCHANGED_REFCOUNT: u32 = 2;
const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 8;
const GAMES: &str = r"D:\xbox-pc-port\games";
const SUBPATH: &str = r"EmbeddedXvd\Windows\System32\sc_dll.dll";
pub const SIBLINGS: [&str; 3] = ["scdxil.dll", "newbe.dll", "dxbc2dxil.dll"];

fn own_module() -> usize {
    let mut h: *mut c_void = core::ptr::null_mut();
    unsafe { GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, own_module as *const u16, &mut h) };
    h as usize
}

fn own_dir() -> Option<PathBuf> {
    let h = own_module();
    if h == 0 {
        return None;
    }
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(h as *mut c_void, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    PathBuf::from(String::from_utf16_lossy(&buf[..n])).parent().map(Path::to_path_buf)
}

fn dumps(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(titles) = std::fs::read_dir(root) else { return out };
    let mut titles: Vec<PathBuf> = titles.flatten().map(|e| e.path()).collect();
    titles.sort();
    for t in titles {
        let ext = t.join("extracted");
        let direct = ext.join(SUBPATH);
        if direct.is_file() {
            out.push(direct);
            continue;
        }
        if let Ok(inner) = std::fs::read_dir(&ext) {
            let mut inner: Vec<PathBuf> = inner.flatten().map(|e| e.path().join(SUBPATH)).filter(|p| p.is_file()).collect();
            inner.sort();
            out.extend(inner);
        }
    }
    out
}

pub fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("TOMBOX_SC_CONSOLE").filter(|p| !p.is_empty()) {
        v.push(PathBuf::from(p));
    }
    if let Some(d) = own_dir() {
        v.push(d.join("sc_dll_console.dll"));
    }
    v.extend(dumps(Path::new(GAMES)));
    v
}

pub fn load_from(path: &Path) -> usize {
    if !path.is_absolute() || !path.is_file() {
        return 0;
    }
    let w = wide(path);
    let h = unsafe { LoadLibraryExW(w.as_ptr(), core::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) } as usize;
    if h == own_module() {
        return 0;
    }
    if h != 0 && unsafe { GetProcAddress(h as *mut c_void, b"SCGetExportFunctions\0".as_ptr()) }.is_null() {
        return 0;
    }
    h
}

fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub fn preload_siblings(dir: &Path) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for n in SIBLINGS {
        let loaded = unsafe { GetModuleHandleW(wide(Path::new(n)).as_ptr()) } as usize;
        if loaded != 0 {
            out.push((n.to_string(), loaded));
            continue;
        }
        let p = dir.join(n);
        if p.is_file() {
            let h = unsafe { LoadLibraryExW(wide(&p).as_ptr(), core::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) } as usize;
            if h != 0 {
                out.push((n.to_string(), h));
            }
        }
    }
    out
}

static DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn dir() -> Option<&'static Path> {
    module();
    DIR.get().map(PathBuf::as_path)
}

fn load() -> usize {
    for c in candidates() {
        if !c.is_absolute() || !c.is_file() {
            continue;
        }
        let parent = c.parent().map(Path::to_path_buf).unwrap_or_default();
        let siblings = preload_siblings(&parent);
        let h = load_from(&c);
        if h != 0 {
            let _ = DIR.set(parent);
            compat_common::record_raw(M, "console", &format!("forwarding to {} = {h:#x}; siblings {siblings:x?}", c.display()));
            return h;
        }
    }
    compat_common::record_raw(M, "console", "no console sc_dll.dll found (TOMBOX_SC_CONSOLE, sc_dll_console.dll next to this DLL, or a title EmbeddedXvd dump)");
    0
}

pub fn module() -> usize {
    static H: OnceLock<usize> = OnceLock::new();
    *H.get_or_init(load)
}

pub fn proc(name: &str) -> Option<*mut c_void> {
    let h = module();
    if h == 0 {
        return None;
    }
    let f = unsafe { GetProcAddress(h as *mut c_void, name.as_ptr()) };
    if f.is_null() { None } else { Some(f) }
}

pub fn missing(name: &str, fail: usize) {
    compat_common::record_raw(M, name, &format!("console sc_dll.dll unavailable; returning the console's own failure value {fail:#x}"));
}
