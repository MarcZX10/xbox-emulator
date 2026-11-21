use std::ffi::c_void;
use std::path::{Path, PathBuf};

pub type HModule = *mut c_void;

pub const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;
pub const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0000_0800;
const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;
const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x2;
const SEM_FAILCRITICALERRORS: u32 = 0x1;
const SEM_NOGPFAULTERRORBOX: u32 = 0x2;
const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> HModule;
    fn GetProcAddress(module: HModule, name: *const u8) -> *mut c_void;
    fn GetModuleFileNameW(module: HModule, buf: *mut u16, size: u32) -> u32;
    fn GetModuleHandleExW(flags: u32, name: *const u16, out: *mut HModule) -> i32;
    fn GetLastError() -> u32;
    fn SetLastError(code: u32);
    fn SetErrorMode(mode: u32) -> u32;
    fn GetModuleHandleW(name: *const u16) -> HModule;
    fn SetDefaultDllDirectories(flags: u32) -> i32;
    fn AddDllDirectory(dir: *const u16) -> *mut c_void;
}

pub const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x0000_0100;
pub const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x0000_1000;

pub fn prefer_dir(dir: &Path) -> bool {
    let w = wide(&dir.to_string_lossy());
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) != 0 && !AddDllDirectory(w.as_ptr()).is_null() }
}

type ResolveDelayLoadedApi = unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, *const c_void, *mut c_void, u32) -> *mut c_void;

unsafe extern "system" fn delay_failure(_dll: *const u8, _proc: *const u8) -> *mut c_void {
    std::ptr::null_mut()
}

pub fn resolve_delay(base: usize, desc_rva: u32, iat_rva: u32) -> Result<usize, String> {
    let f = kernel_proc("ResolveDelayLoadedAPI").ok_or("ResolveDelayLoadedAPI not exported by kernelbase/kernel32")?;
    let f: ResolveDelayLoadedApi = unsafe { std::mem::transmute::<usize, ResolveDelayLoadedApi>(f) };
    unsafe {
        SetLastError(0);
        let failure: unsafe extern "system" fn(*const u8, *const u8) -> *mut c_void = delay_failure;
        let p = f(base as *mut c_void, (base + desc_rva as usize) as *const c_void, std::ptr::null(), failure as *const c_void, (base + iat_rva as usize) as *mut c_void, 0);
        if p.is_null() {
            Err(describe_error(GetLastError()))
        } else {
            Ok(p as usize)
        }
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn quiet_errors() {
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    }
}

pub fn load(name: &str, flags: u32) -> Result<HModule, u32> {
    let w = wide(name);
    unsafe {
        SetLastError(0);
        let h = LoadLibraryExW(w.as_ptr(), std::ptr::null_mut(), flags);
        if h.is_null() {
            Err(GetLastError())
        } else {
            Ok(h)
        }
    }
}

pub fn load_path(path: &Path, flags: u32) -> Result<HModule, u32> {
    load(&path.to_string_lossy(), flags)
}

pub fn loaded(name: &str) -> Option<HModule> {
    let w = wide(name);
    let h = unsafe { GetModuleHandleW(w.as_ptr()) };
    if h.is_null() {
        None
    } else {
        Some(h)
    }
}

pub fn proc_by_name(h: HModule, name: &str) -> Result<usize, u32> {
    let mut c = name.as_bytes().to_vec();
    c.push(0);
    unsafe {
        SetLastError(0);
        let p = GetProcAddress(h, c.as_ptr());
        if p.is_null() {
            Err(GetLastError())
        } else {
            Ok(p as usize)
        }
    }
}

pub fn proc_by_ordinal(h: HModule, ordinal: u16) -> Result<usize, u32> {
    unsafe {
        SetLastError(0);
        let p = GetProcAddress(h, ordinal as usize as *const u8);
        if p.is_null() {
            Err(GetLastError())
        } else {
            Ok(p as usize)
        }
    }
}

pub fn module_path(h: HModule) -> Option<PathBuf> {
    let mut buf = vec![0u16; 32768];
    let n = unsafe { GetModuleFileNameW(h, buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 {
        None
    } else {
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])))
    }
}

pub fn module_of(addr: usize) -> Option<HModule> {
    let mut h: HModule = std::ptr::null_mut();
    let ok = unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, addr as *const u16, &mut h) };
    if ok == 0 || h.is_null() {
        None
    } else {
        Some(h)
    }
}

pub fn kernel_proc(name: &str) -> Option<usize> {
    for m in ["kernelbase.dll", "kernel32.dll"] {
        if let Some(h) = loaded(m) {
            if let Ok(p) = proc_by_name(h, name) {
                return Some(p);
            }
        }
    }
    None
}

pub unsafe fn read_u32(addr: usize) -> u32 {
    std::ptr::read_unaligned(addr as *const u32)
}

pub unsafe fn read_u16(addr: usize) -> u16 {
    std::ptr::read_unaligned(addr as *const u16)
}

pub unsafe fn read_u64(addr: usize) -> u64 {
    std::ptr::read_unaligned(addr as *const u64)
}

pub unsafe fn read_cstr(addr: usize) -> String {
    let mut v = Vec::new();
    let mut p = addr as *const u8;
    while v.len() < 1024 {
        let c = *p;
        if c == 0 {
            break;
        }
        v.push(c);
        p = p.add(1);
    }
    String::from_utf8_lossy(&v).into_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportInfo {
    pub ordinal: u32,
    pub name: Option<String>,
}

pub fn export_at(h: HModule, addr: usize) -> Option<ExportInfo> {
    let base = h as usize;
    unsafe {
        if read_u16(base) != 0x5a4d {
            return None;
        }
        let nt = base + read_u32(base + 0x3c) as usize;
        let opt = nt + 24;
        if read_u16(opt) != 0x20b {
            return None;
        }
        let dir_rva = read_u32(opt + 112) as usize;
        if dir_rva == 0 {
            return None;
        }
        let dir = base + dir_rva;
        let ord_base = read_u32(dir + 16);
        let nfun = read_u32(dir + 20) as usize;
        let nnames = read_u32(dir + 24) as usize;
        let funcs = base + read_u32(dir + 28) as usize;
        let names = base + read_u32(dir + 32) as usize;
        let ords = base + read_u32(dir + 36) as usize;
        let target = addr.checked_sub(base)? as u32;
        let idx = (0..nfun.min(1 << 20)).find(|&i| read_u32(funcs + i * 4) == target)?;
        let name = (0..nnames.min(1 << 20)).find(|&k| read_u16(ords + k * 2) as usize == idx).map(|k| read_cstr(base + read_u32(names + k * 4) as usize));
        Some(ExportInfo { ordinal: ord_base + idx as u32, name })
    }
}

pub fn describe_error(code: u32) -> String {
    match code {
        0 => "ok".into(),
        2 => "ERROR_FILE_NOT_FOUND".into(),
        3 => "ERROR_PATH_NOT_FOUND".into(),
        126 => "ERROR_MOD_NOT_FOUND".into(),
        127 => "ERROR_PROC_NOT_FOUND".into(),
        182 => "ERROR_INVALID_ORDINAL".into(),
        193 => "ERROR_BAD_EXE_FORMAT".into(),
        1114 => "ERROR_DLL_INIT_FAILED".into(),
        c => format!("win32 error {c}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_kernel32_get_tick_count64() {
        let h = load("kernel32.dll", LOAD_LIBRARY_SEARCH_SYSTEM32).unwrap();
        let p = proc_by_name(h, "GetTickCount64").unwrap();
        let owner = module_of(p).unwrap();
        let path = module_path(owner).unwrap();
        let file = path.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
        assert!(file == "kernel32.dll" || file == "kernelbase.dll");
        let e = export_at(owner, p).unwrap();
        assert_eq!(e.name.as_deref(), Some("GetTickCount64"));
        assert_eq!(proc_by_ordinal(owner, e.ordinal as u16).unwrap(), p);
        let img = crate::image::Image::open(&path).unwrap();
        assert_eq!(img.exports().ordinal_of("GetTickCount64"), Some(e.ordinal));
    }

    #[test]
    fn missing_things_report_errors() {
        assert_eq!(load("tombox-no-such-module-xyz.dll", 0).unwrap_err(), 126);
        let h = load("kernel32.dll", 0).unwrap();
        assert_eq!(proc_by_name(h, "TomboxNoSuchExport").unwrap_err(), 127);
        assert_eq!(describe_error(126), "ERROR_MOD_NOT_FOUND");
    }
}
