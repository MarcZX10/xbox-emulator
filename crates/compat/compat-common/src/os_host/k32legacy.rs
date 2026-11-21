use super::patch::{dll_matches, Hook};
use super::{affinity, dlldir};
use core::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(n: *const u16) -> *mut c_void;
    fn LoadLibraryW(n: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

pub const API_SET_PREFIX: &str = "api-ms-win-core-kernel32-legacy-";
pub const CONSOLE_DLLS: &[&str] = &["api-ms-win-core-kernel32-legacy-", "kernel32legacy.dll"];
pub const SHARED_VAR: &str = "TOMBOX_OSHOST_SHARED";
pub const BUILD_VAR: &str = "TOMBOX_CONSOLE_OSBUILD";
pub const DLL_VAR: &str = "TOMBOX_KERNEL32LEGACY";

pub fn console_os_build() -> u32 {
    std::env::var(BUILD_VAR)
        .ok()
        .or_else(|| crate::cfg("console_os_build"))
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or_else(crate::gameinput_ext::default_os_build)
}

pub fn dll_name(os_build: u32) -> &'static str {
    if os_build < 22000 {
        "kernel32legacy_19041.dll"
    } else {
        "kernel32legacy.dll"
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn candidates(os_build: u32, exe_dir: Option<PathBuf>) -> Vec<String> {
    let name = dll_name(os_build);
    let mut v = Vec::new();
    if let Ok(p) = std::env::var(DLL_VAR) {
        if !p.trim().is_empty() && p.trim() != "0" {
            v.push(p.trim().to_string());
        }
    }
    if let Some(d) = exe_dir {
        v.push(d.join(name).display().to_string());
    }
    v
}

pub fn module() -> usize {
    static M: OnceLock<usize> = OnceLock::new();
    *M.get_or_init(|| unsafe {
        if std::env::var(DLL_VAR).ok().as_deref() == Some("0") {
            return 0;
        }
        let build = console_os_build();
        let h = GetModuleHandleW(wide(dll_name(build)).as_ptr());
        if !h.is_null() {
            return h as usize;
        }
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf()));
        for c in candidates(build, exe_dir) {
            if !std::path::Path::new(&c).is_file() {
                continue;
            }
            let h = LoadLibraryW(wide(&c).as_ptr());
            if !h.is_null() {
                crate::record_raw("os_host", "kernel32legacy", &format!("loaded {c} for console OS build {build}"));
                return h as usize;
            }
        }
        0
    })
}

unsafe fn rd32(p: *const u8) -> u32 {
    core::ptr::read_unaligned(p as *const u32)
}

pub unsafe fn export_names(base: usize) -> Vec<String> {
    let b = base as *const u8;
    if b.is_null() || core::ptr::read_unaligned(b as *const u16) != 0x5a4d {
        return Vec::new();
    }
    let pe = b.add(rd32(b.add(0x3c)) as usize);
    let erva = rd32(pe.add(24 + 112)) as usize;
    if erva == 0 {
        return Vec::new();
    }
    let ed = b.add(erva);
    let n = rd32(ed.add(24)) as usize;
    let names = b.add(rd32(ed.add(32)) as usize);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = b.add(rd32(names.add(i * 4)) as usize);
        let mut len = 0;
        while *p.add(len) != 0 && len < 256 {
            len += 1;
        }
        out.push(String::from_utf8_lossy(core::slice::from_raw_parts(p, len)).into_owned());
    }
    out
}

pub fn covered_by_host_hooks(name: &str, hooks: &[Hook]) -> bool {
    hooks.iter().any(|h| h.name == name && dll_matches("api-ms-win-core-kernel32-legacy-l1-1-0.dll", h.dlls))
}

pub unsafe fn redirects_from(base: usize, hooks: &[Hook]) -> Vec<Hook> {
    let mut v = Vec::new();
    for n in export_names(base) {
        if covered_by_host_hooks(&n, hooks) || super::routes_rt::LOADER_FUNCS.contains(&n.as_str()) {
            continue;
        }
        let p = GetProcAddress(base as *mut c_void, format!("{n}\0").as_ptr());
        if p.is_null() {
            continue;
        }
        let name: &'static str = Box::leak(n.into_boxed_str());
        v.push(Hook { name, dlls: CONSOLE_DLLS, hook: p as usize });
    }
    v
}

pub fn redirects(hooks: &[Hook]) -> Vec<Hook> {
    let m = module();
    if m == 0 {
        return Vec::new();
    }
    let v = unsafe { redirects_from(m, hooks) };
    crate::record_raw("os_host", "kernel32legacy", &format!("{} api-ms-win-core-kernel32-legacy imports redirected to the compat DLL at {m:#x}", v.len()));
    v
}

pub unsafe fn import_slot(base: usize, dll_prefix: &str, name: &str) -> Option<usize> {
    let b = base as *const u8;
    if b.is_null() || core::ptr::read_unaligned(b as *const u16) != 0x5a4d {
        return None;
    }
    let pe = b.add(rd32(b.add(0x3c)) as usize);
    let imp = rd32(pe.add(24 + 112 + 8)) as usize;
    if imp == 0 {
        return None;
    }
    let mut d = b.add(imp);
    loop {
        let int_rva = rd32(d) as usize;
        let name_rva = rd32(d.add(12)) as usize;
        let iat_rva = rd32(d.add(16)) as usize;
        if name_rva == 0 && iat_rva == 0 {
            return None;
        }
        let dn = b.add(name_rva);
        let mut len = 0;
        while *dn.add(len) != 0 && len < 256 {
            len += 1;
        }
        let dll = String::from_utf8_lossy(core::slice::from_raw_parts(dn, len)).to_ascii_lowercase();
        if dll.starts_with(dll_prefix) {
            let lookup = if int_rva != 0 { int_rva } else { iat_rva };
            let mut i = 0usize;
            loop {
                let t = core::ptr::read_unaligned(b.add(lookup + i * 8) as *const u64);
                if t == 0 {
                    break;
                }
                if t & (1 << 63) == 0 {
                    let f = b.add(t as usize + 2);
                    let mut fl = 0;
                    while *f.add(fl) != 0 && fl < 256 {
                        fl += 1;
                    }
                    if core::slice::from_raw_parts(f, fl) == name.as_bytes() {
                        return Some(core::ptr::read_volatile(b.add(iat_rva + i * 8) as *const usize));
                    }
                }
                i += 1;
            }
        }
        d = d.add(20);
    }
}

type SetThread = unsafe extern "system" fn(*mut c_void, usize) -> usize;
type GetProcess = unsafe extern "system" fn(*mut c_void, *mut usize, *mut usize) -> i32;
type SetProcess = unsafe extern "system" fn(*mut c_void, usize) -> i32;
type SetDir = unsafe extern "system" fn(*const u16) -> i32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shared {
    pub set_thread: usize,
    pub get_process: usize,
    pub set_process: usize,
    pub set_dll_dir: usize,
}

pub fn local() -> Shared {
    Shared {
        set_thread: affinity::set_thread_affinity_mask as *const () as usize,
        get_process: affinity::get_process_affinity_mask as *const () as usize,
        set_process: affinity::set_process_affinity_mask as *const () as usize,
        set_dll_dir: dlldir::set_dll_directory_w as *const () as usize,
    }
}

pub fn encode(s: Shared) -> String {
    format!("{:x},{:x},{:x},{:x}", s.set_thread, s.get_process, s.set_process, s.set_dll_dir)
}

pub fn decode(v: &str) -> Option<Shared> {
    let p: Vec<usize> = v.split(',').map(|x| usize::from_str_radix(x.trim(), 16)).collect::<Result<_, _>>().ok()?;
    if p.len() != 4 || p.iter().any(|x| *x == 0) {
        return None;
    }
    Some(Shared { set_thread: p[0], get_process: p[1], set_process: p[2], set_dll_dir: p[3] })
}

pub fn publish() {
    std::env::set_var(SHARED_VAR, encode(local()));
}

pub fn owner() -> Shared {
    super::ensure();
    if super::is_owner() {
        return local();
    }
    std::env::var(SHARED_VAR).ok().and_then(|v| decode(&v)).unwrap_or_else(local)
}

pub unsafe extern "system" fn set_thread_affinity_mask(h: *mut c_void, mask: usize) -> usize {
    let f: SetThread = core::mem::transmute(owner().set_thread);
    f(h, mask)
}

pub unsafe extern "system" fn get_process_affinity_mask(h: *mut c_void, process: *mut usize, system: *mut usize) -> i32 {
    let f: GetProcess = core::mem::transmute(owner().get_process);
    f(h, process, system)
}

pub unsafe extern "system" fn set_process_affinity_mask(h: *mut c_void, mask: usize) -> i32 {
    let f: SetProcess = core::mem::transmute(owner().set_process);
    f(h, mask)
}

pub unsafe extern "system" fn set_dll_directory_w(path: *const u16) -> i32 {
    let f: SetDir = core::mem::transmute(owner().set_dll_dir);
    f(path)
}
