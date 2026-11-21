use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
}

type Resolver = unsafe extern "system" fn(*const u16, *mut u16, u32) -> u32;

static RESOLVER: AtomicUsize = AtomicUsize::new(0);

pub fn register(f: usize) {
    RESOLVER.store(f, Ordering::Release);
}

pub fn enabled() -> bool {
    super::setting("xcrd_paths").as_deref() != Some("off")
}

pub fn is_xcrd_path(p: &str) -> bool {
    let n = p.replace('/', "\\").to_ascii_lowercase();
    let Some(body) = ["\\\\?\\", "\\\\.\\", "\\??\\"].iter().find_map(|x| n.strip_prefix(x)) else {
        return false;
    };
    body.starts_with("globalroot\\device\\harddisk") || body.starts_with("volume{5843524b-0000-0000-0000-0000000000")
}

fn resolver() -> Option<Resolver> {
    let mut f = RESOLVER.load(Ordering::Acquire);
    if f == 0 {
        let m: Vec<u16> = "xcrdapi.dll".encode_utf16().chain([0]).collect();
        let h = unsafe { GetModuleHandleW(m.as_ptr()) };
        if h.is_null() {
            return None;
        }
        f = unsafe { GetProcAddress(h, b"TomBoxXcrdResolvePath\0".as_ptr()) } as usize;
        if f == 0 {
            return None;
        }
        RESOLVER.store(f, Ordering::Release);
    }
    Some(unsafe { core::mem::transmute::<usize, Resolver>(f) })
}

pub fn resolve(p: &str) -> Option<String> {
    if !is_xcrd_path(p) || !enabled() {
        return None;
    }
    let f = resolver()?;
    let w: Vec<u16> = p.encode_utf16().chain([0]).collect();
    let mut cap = 520u32;
    for _ in 0..4 {
        let mut out = vec![0u16; cap as usize];
        let r = unsafe { f(w.as_ptr(), out.as_mut_ptr(), cap) };
        if r == 0 {
            return None;
        }
        if r < cap {
            return Some(String::from_utf16_lossy(&out[..r as usize]));
        }
        cap = r + 1;
    }
    None
}
