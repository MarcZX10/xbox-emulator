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

const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 8;
const GAMES: &str = r"D:\xbox-pc-port\games";
const SUBPATH: &str = r"EmbeddedXvd\Windows\System32\scdxil.dll";
pub const SIBLINGS: [&str; 1] = ["newbe.dll"];

pub type Fwd = unsafe extern "system" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize) -> usize;

fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn own_module() -> usize {
    let mut h: *mut c_void = core::ptr::null_mut();
    unsafe { GetModuleHandleExW(4 | 2, own_module as *const u16, &mut h) };
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
    if let Some(p) = std::env::var_os("TOMBOX_SCDXIL_CONSOLE").filter(|p| !p.is_empty()) {
        v.push(PathBuf::from(p));
    }
    if let Some(d) = own_dir() {
        v.push(d.join("scdxil_console.dll"));
    }
    v.extend(dumps(Path::new(GAMES)));
    v
}

fn preload(dir: &Path) {
    for n in SIBLINGS {
        if !unsafe { GetModuleHandleW(wide(Path::new(n)).as_ptr()) }.is_null() {
            continue;
        }
        let p = dir.join(n);
        if p.is_file() {
            unsafe { LoadLibraryExW(wide(&p).as_ptr(), core::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
        }
    }
}

pub fn load_from(path: &Path) -> usize {
    if !path.is_absolute() || !path.is_file() {
        return 0;
    }
    if let Some(d) = path.parent() {
        preload(d);
    }
    let h = unsafe { LoadLibraryExW(wide(path).as_ptr(), core::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) } as usize;
    if h == 0 || h == own_module() || unsafe { GetProcAddress(h as *mut c_void, b"SCDxil_SCSimpleCompileShader\0".as_ptr()) }.is_null() {
        return 0;
    }
    h
}

pub fn module() -> usize {
    static H: OnceLock<usize> = OnceLock::new();
    *H.get_or_init(|| candidates().iter().map(|c| load_from(c)).find(|h| *h != 0).unwrap_or(0))
}

pub fn proc(name: &str) -> Option<Fwd> {
    let h = module();
    if h == 0 {
        return None;
    }
    let f = unsafe { GetProcAddress(h as *mut c_void, name.as_ptr()) };
    if f.is_null() { None } else { Some(unsafe { core::mem::transmute::<*mut c_void, Fwd>(f) }) }
}
