use core::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, name: *const u16, out: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(h: *mut c_void, buf: *mut u16, n: u32) -> u32;
}

const FROM_ADDRESS: u32 = 4;
const UNCHANGED_REFCOUNT: u32 = 2;
const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 8;
pub const GAMES: &str = r"D:\xbox-pc-port\games";
pub const SYSTEM32: &str = r"EmbeddedXvd\Windows\System32";

pub struct ConsoleDll {
    pub file: &'static str,
    pub env: &'static str,
    pub side: &'static str,
    pub probe: &'static str,
    pub deps: &'static [&'static str],
    pub handle: OnceLock<usize>,
}

fn module_of(addr: usize) -> usize {
    let mut h: *mut c_void = core::ptr::null_mut();
    unsafe { GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, addr as *const u16, &mut h) };
    h as usize
}

fn module_path(h: usize) -> Option<PathBuf> {
    if h == 0 {
        return None;
    }
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(h as *mut c_void, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 {
        None
    } else {
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
    }
}

pub fn file_version(path: &Path) -> u64 {
    let Ok(b) = std::fs::read(path) else { return 0 };
    let sig = 0xfeef_04bdu32.to_le_bytes();
    let Some(i) = b.windows(4).position(|w| w == sig) else { return 0 };
    if i + 16 > b.len() {
        return 0;
    }
    let ms = u32::from_le_bytes(b[i + 8..i + 12].try_into().unwrap()) as u64;
    let ls = u32::from_le_bytes(b[i + 12..i + 16].try_into().unwrap()) as u64;
    (ms << 32) | ls
}

pub fn dump_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(titles) = std::fs::read_dir(root) else { return out };
    for t in titles.flatten().map(|e| e.path()) {
        let ext = t.join("extracted");
        let direct = ext.join(SYSTEM32);
        if direct.is_dir() {
            out.push(direct);
            continue;
        }
        if let Ok(inner) = std::fs::read_dir(&ext) {
            out.extend(inner.flatten().map(|e| e.path().join(SYSTEM32)).filter(|p| p.is_dir()));
        }
    }
    out
}

pub fn load_path(path: &Path) -> usize {
    if !path.is_absolute() || !path.is_file() {
        return 0;
    }
    let w: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe { LoadLibraryExW(w.as_ptr(), core::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) as usize }
}

pub fn symbol(h: usize, name: &str) -> Option<*mut c_void> {
    if h == 0 {
        return None;
    }
    let z: Vec<u8> = name.bytes().chain(Some(0)).collect();
    let f = unsafe { GetProcAddress(h as *mut c_void, z.as_ptr()) };
    if f.is_null() {
        None
    } else {
        Some(f)
    }
}

impl ConsoleDll {
    pub const fn new(file: &'static str, env: &'static str, side: &'static str, probe: &'static str, deps: &'static [&'static str]) -> ConsoleDll {
        ConsoleDll { file, env, side, probe, deps, handle: OnceLock::new() }
    }

    pub fn candidates(&self, own: usize) -> Vec<PathBuf> {
        let mut v = Vec::new();
        if let Some(p) = std::env::var_os(self.env).filter(|p| !p.is_empty()) {
            v.push(PathBuf::from(p));
        }
        if let Some(d) = module_path(module_of(own)).and_then(|p| p.parent().map(Path::to_path_buf)) {
            v.push(d.join(self.side));
        }
        let mut dumps: Vec<PathBuf> = dump_dirs(Path::new(GAMES)).into_iter().map(|d| d.join(self.file)).filter(|p| p.is_file()).collect();
        dumps.sort_by_key(|p| std::cmp::Reverse(file_version(p)));
        v.extend(dumps);
        v
    }

    fn open(&self, own: usize) -> usize {
        let me = module_of(own);
        for c in self.candidates(own) {
            if !c.is_file() {
                continue;
            }
            if let Some(dir) = c.parent() {
                for d in self.deps {
                    load_path(&dir.join(d));
                }
            }
            let h = load_path(&c);
            if h == 0 || h == me {
                continue;
            }
            if symbol(h, self.probe).is_none() {
                continue;
            }
            compat_common::record_raw(self.file, "console", &format!("forwarding to {} = {h:#x}", c.display()));
            return h;
        }
        compat_common::record_raw(self.file, "console", &format!("no console {} found ({}, {} beside this DLL, or a title EmbeddedXvd dump)", self.file, self.env, self.side));
        0
    }

    pub fn module(&self, own: usize) -> usize {
        *self.handle.get_or_init(|| self.open(own))
    }

    pub fn proc(&self, own: usize, name: &str) -> Option<*mut c_void> {
        symbol(self.module(own), name)
    }

    pub fn missing(&self, name: &str, fail: usize) {
        compat_common::record_raw(self.file, name, &format!("console {} unavailable; returning the console failure value {fail:#x}", self.file));
    }
}

pub type Fwd = unsafe extern "system" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize) -> usize;

#[macro_export]
macro_rules! forward {
    ($dll:expr, $own:expr; $($name:ident = $fail:expr;)*) => {
        $(
            #[no_mangle]
            pub unsafe extern "system" fn $name(a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize, a8: usize, a9: usize, a10: usize, a11: usize, a12: usize) -> usize {
                match $dll.proc($own, stringify!($name)) {
                    Some(p) => {
                        let f: $crate::Fwd = core::mem::transmute(p);
                        f(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12)
                    }
                    None => {
                        $dll.missing(stringify!($name), $fail as usize);
                        $fail as usize
                    }
                }
            }
        )*
        pub const FORWARDED: &[(&str, $crate::Fwd, usize)] = &[$((stringify!($name), $name as $crate::Fwd, $fail as usize)),*];
    };
}
