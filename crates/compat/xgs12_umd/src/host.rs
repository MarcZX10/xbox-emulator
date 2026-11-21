use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};

pub type Hmodule = *mut c_void;

extern "system" {
    fn LoadLibraryW(p: *const u16) -> Hmodule;
    fn GetModuleHandleW(p: *const u16) -> Hmodule;
    fn GetProcAddress(h: Hmodule, n: *const u8) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, addr: *const c_void, out: *mut Hmodule) -> i32;
    fn GetModuleFileNameW(h: Hmodule, buf: *mut u16, n: u32) -> u32;
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(core::iter::once(0)).collect()
}

fn cached(cell: &AtomicUsize, open: impl FnOnce() -> usize) -> Hmodule {
    let v = cell.load(Ordering::Acquire);
    if v != 0 {
        return if v == 1 { core::ptr::null_mut() } else { v as Hmodule };
    }
    let h = open();
    cell.store(if h == 0 { 1 } else { h }, Ordering::Release);
    h as Hmodule
}

fn own_dir() -> Option<String> {
    let mut h: Hmodule = core::ptr::null_mut();
    let ok = unsafe { GetModuleHandleExW(0x4 | 0x2, own_dir as *const c_void, &mut h) };
    if ok == 0 {
        return None;
    }
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(h, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let path = String::from_utf16_lossy(&buf[..n]);
    path.rfind('\\').map(|i| path[..i].to_string())
}

pub fn emulator_module(name: &str) -> usize {
    let loaded = unsafe { GetModuleHandleW(wide(name).as_ptr()) } as usize;
    if loaded != 0 {
        return loaded;
    }
    let sibling = own_dir().map(|d| format!("{d}\\{name}"));
    if let Some(p) = sibling {
        if std::path::Path::new(&p).is_file() {
            let h = unsafe { LoadLibraryW(wide(&p).as_ptr()) } as usize;
            if h != 0 {
                return h;
            }
        }
    }
    0
}

pub fn d3d12_xs() -> Hmodule {
    static CELL: AtomicUsize = AtomicUsize::new(0);
    cached(&CELL, || emulator_module("d3d12_xs.dll"))
}

pub fn xmem() -> Hmodule {
    static CELL: AtomicUsize = AtomicUsize::new(0);
    cached(&CELL, || emulator_module("xmem.dll"))
}

pub fn host_d3d12() -> Hmodule {
    static CELL: AtomicUsize = AtomicUsize::new(0);
    cached(&CELL, || unsafe { LoadLibraryW(wide(r"C:\Windows\System32\d3d12.dll").as_ptr()) as usize })
}

pub fn proc_in(h: Hmodule, name: &[u8]) -> usize {
    if h.is_null() {
        return 0;
    }
    unsafe { GetProcAddress(h, name.as_ptr()) as usize }
}

pub fn runtime_proc(name: &[u8]) -> (usize, &'static str) {
    let p = proc_in(d3d12_xs(), name);
    if p != 0 {
        return (p, "d3d12_xs");
    }
    (proc_in(host_d3d12(), name), "host d3d12")
}
