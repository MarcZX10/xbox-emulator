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

const FROM_ADDRESS: u32 = 4;
const UNCHANGED_REFCOUNT: u32 = 2;
const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 8;
pub const GAMES: &str = r"D:\xbox-pc-port\games";
const SYSTEM32: &str = r"EmbeddedXvd\Windows\System32";

pub struct Spec {
    pub name: &'static str,
    pub env: &'static str,
    pub alias: &'static str,
    pub probe: &'static str,
    pub emulated_imports: &'static [&'static str],
}

pub struct Console {
    pub spec: Spec,
    handle: OnceLock<usize>,
    path: OnceLock<PathBuf>,
}

pub fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub fn module_path(h: usize) -> Option<PathBuf> {
    if h == 0 {
        return None;
    }
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(h as *mut c_void, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    (n != 0 && n < buf.len()).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..n])))
}

pub fn own_module(addr: usize) -> usize {
    let mut h: *mut c_void = core::ptr::null_mut();
    unsafe { GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, addr as *const u16, &mut h) };
    h as usize
}

fn fixed_version(bytes: &[u8]) -> u64 {
    let sig = 0xFEEF_04BDu32.to_le_bytes();
    let Some(i) = bytes.windows(4).rposition(|w| w == sig) else { return 0 };
    let rd = |o: usize| bytes.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()) as u64);
    (rd(i + 8) << 32) | rd(i + 12)
}

pub fn file_version(path: &Path) -> u64 {
    std::fs::read(path).map_or(0, |b| fixed_version(&b))
}

pub fn dumps(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(titles) = std::fs::read_dir(root) else { return out };
    for t in titles.flatten().map(|e| e.path()) {
        let ext = t.join("extracted");
        let direct = ext.join(SYSTEM32).join(name);
        if direct.is_file() {
            out.push(direct);
            continue;
        }
        if let Ok(inner) = std::fs::read_dir(&ext) {
            out.extend(inner.flatten().map(|e| e.path().join(SYSTEM32).join(name)).filter(|p| p.is_file()));
        }
    }
    let mut keyed: Vec<((u64, u64), PathBuf)> = out.into_iter().map(|p| ((os_build(&p), file_version(&p)), p)).collect();
    keyed.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    keyed.into_iter().map(|(_, p)| p).collect()
}

pub fn os_build(dll: &Path) -> u64 {
    dll.parent().map_or(0, |d| file_version(&d.join("ntdll.dll")))
}

pub fn self_module() -> usize {
    own_module(self_module as usize)
}

impl Console {
    pub const fn new(spec: Spec) -> Self {
        Console { spec, handle: OnceLock::new(), path: OnceLock::new() }
    }

    fn own_dir(&self) -> Option<PathBuf> {
        module_path(self_module()).and_then(|p| p.parent().map(Path::to_path_buf))
    }

    pub fn candidates(&self) -> Vec<PathBuf> {
        let mut v = Vec::new();
        if let Some(p) = std::env::var_os(self.spec.env).filter(|p| !p.is_empty()) {
            v.push(PathBuf::from(p));
        }
        if let Some(d) = self.own_dir() {
            v.push(d.join(self.spec.alias));
        }
        v.extend(dumps(Path::new(GAMES), self.spec.name));
        v
    }

    fn preload(&self) -> Result<Vec<(String, usize)>, String> {
        let mut out = Vec::new();
        for n in self.spec.emulated_imports {
            let loaded = unsafe { GetModuleHandleW(wide(Path::new(n)).as_ptr()) } as usize;
            if loaded != 0 {
                out.push((n.to_string(), loaded));
                continue;
            }
            let side = self.own_dir().map(|d| d.join(n)).filter(|p| p.is_file());
            let Some(p) = side else { return Err(format!("{n} (emulator compat) is not loaded and not beside this DLL")) };
            let h = unsafe { LoadLibraryExW(wide(&p).as_ptr(), core::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) } as usize;
            if h == 0 {
                return Err(format!("{} failed to load", p.display()));
            }
            out.push((n.to_string(), h));
        }
        Ok(out)
    }

    pub fn load_from(&self, path: &Path) -> usize {
        if !path.is_absolute() || !path.is_file() {
            return 0;
        }
        let h = unsafe { LoadLibraryExW(wide(path).as_ptr(), core::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) } as usize;
        if h == 0 || h == self_module() {
            return 0;
        }
        let probe = format!("{}\0", self.spec.probe);
        if unsafe { GetProcAddress(h as *mut c_void, probe.as_ptr()) }.is_null() {
            return 0;
        }
        h
    }

    fn load(&self) -> usize {
        let m = self.spec.name.trim_end_matches(".dll");
        let pre = match self.preload() {
            Ok(p) => p,
            Err(e) => {
                compat_common::record_raw(m, "console", &format!("console {} not loaded: {e}", self.spec.name));
                return 0;
            }
        };
        for c in self.candidates() {
            let h = self.load_from(&c);
            if h != 0 {
                let _ = self.path.set(c.clone());
                compat_common::record_raw(m, "console", &format!("forwarding to {} = {h:#x}; emulator imports {pre:x?}", c.display()));
                return h;
            }
        }
        compat_common::record_raw(m, "console", &format!("no console {} found ({}, {} beside this DLL, or a title EmbeddedXvd dump)", self.spec.name, self.spec.env, self.spec.alias));
        0
    }

    pub fn module(&self) -> usize {
        *self.handle.get_or_init(|| self.load())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.get().map(PathBuf::as_path)
    }

    pub fn proc(&self, name: &str) -> Option<usize> {
        let h = self.module();
        if h == 0 {
            return None;
        }
        let n = format!("{name}\0");
        let f = unsafe { GetProcAddress(h as *mut c_void, n.as_ptr()) } as usize;
        (f != 0).then_some(f)
    }

    pub fn missing(&self, func: &str, fail: &str) {
        compat_common::record_raw(self.spec.name.trim_end_matches(".dll"), func, &format!("console {} unavailable; returning the console's own failure value {fail}", self.spec.name));
    }
}
