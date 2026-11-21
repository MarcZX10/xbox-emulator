use core::ffi::c_void;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;

static TABLE: Mutex<Option<HashMap<(usize, u32), Vec<u32>>>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut HashMap<(usize, u32), Vec<u32>>) -> R) -> R {
    let mut g = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

#[no_mangle]
pub extern "system" fn TomBoxDriverStateWrite(obj: usize, kind: u32, index: u32, mask: u32, value: u32) -> u32 {
    with(|t| {
        let words = t.entry((obj, kind)).or_default();
        let i = index as usize;
        if words.len() <= i {
            words.resize(i + 1, 0);
        }
        words[i] = (words[i] & !mask) | (value & mask);
        words[i]
    })
}

#[no_mangle]
pub extern "system" fn TomBoxDriverStateRead(obj: usize, kind: u32, index: u32) -> u32 {
    with(|t| t.get(&(obj, kind)).and_then(|w| w.get(index as usize).copied()).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn TomBoxDriverStateRemove(obj: usize, kind: u32) -> u32 {
    with(|t| t.remove(&(obj, kind)).is_some() as u32)
}

extern "system" {
    fn GetModuleHandleW(p: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
    fn LoadLibraryW(p: *const u16) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, addr: *const c_void, out: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(h: *mut c_void, buf: *mut u16, n: u32) -> u32;
    fn FreeLibrary(h: *mut c_void) -> i32;
}

static OWNED: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn adopt_driver_module(h: *mut c_void) -> *mut c_void {
    match OWNED.compare_exchange(0, h as usize, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => h,
        Err(cur) => {
            unsafe { FreeLibrary(h) };
            cur as *mut c_void
        }
    }
}

pub(crate) fn owned_driver_module() -> *mut c_void {
    OWNED.load(Ordering::Acquire) as *mut c_void
}

pub(crate) fn unload_driver_module() -> bool {
    let h = OWNED.swap(0, Ordering::AcqRel);
    if h == 0 {
        return false;
    }
    unsafe { FreeLibrary(h as *mut c_void) };
    true
}

const DRIVERS: [&str; 6] = ["xgs12_10.dll", "xgs12_10i.dll", "xgs12_10d.dll", "xgs12.dll", "xgs12_i.dll", "xgs12_d.dll"];

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(core::iter::once(0)).collect()
}

fn driver_module() -> *mut c_void {
    let owned = owned_driver_module();
    if !owned.is_null() {
        return owned;
    }
    for d in DRIVERS {
        let h = unsafe { GetModuleHandleW(wide(d).as_ptr()) };
        if !h.is_null() {
            return h;
        }
    }
    let mut me: *mut c_void = core::ptr::null_mut();
    if unsafe { GetModuleHandleExW(0x6, driver_module as *const c_void, &mut me) } == 0 {
        return core::ptr::null_mut();
    }
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(me, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let path = String::from_utf16_lossy(&buf[..n]);
    let Some(dir) = path.rfind('\\').map(|i| path[..i].to_string()) else {
        return core::ptr::null_mut();
    };
    let p = format!("{dir}\\{}", DRIVERS[0]);
    if !std::path::Path::new(&p).is_file() {
        return core::ptr::null_mut();
    }
    let h = unsafe { LoadLibraryW(wide(&p).as_ptr()) };
    if h.is_null() {
        return h;
    }
    adopt_driver_module(h)
}

#[no_mangle]
pub unsafe extern "system" fn InternalDriverCallX(code: u32, args: *mut c_void) -> i32 {
    let h = driver_module();
    if h.is_null() {
        return E_NOINTERFACE;
    }
    let p = GetProcAddress(h, b"UMDInternalDriverCallX\0".as_ptr());
    if p.is_null() {
        return E_NOINTERFACE;
    }
    let f: unsafe extern "system" fn(u32, *mut c_void) -> i32 = core::mem::transmute(p);
    f(code, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masked_words_per_object_and_kind() {
        let o = 0x1234_5000usize;
        assert_eq!(TomBoxDriverStateRead(o, 1, 3), 0);
        assert_eq!(TomBoxDriverStateWrite(o, 1, 3, 0xff, 0x1ab), 0xab);
        assert_eq!(TomBoxDriverStateWrite(o, 1, 3, 0xf00, 0x500), 0x5ab);
        assert_eq!(TomBoxDriverStateRead(o, 1, 3), 0x5ab);
        assert_eq!(TomBoxDriverStateRead(o, 2, 3), 0);
        assert_eq!(TomBoxDriverStateRead(o, 1, 0), 0);
        assert_eq!(TomBoxDriverStateRemove(o, 1), 1);
        assert_eq!(TomBoxDriverStateRemove(o, 1), 0);
        assert_eq!(TomBoxDriverStateRead(o, 1, 3), 0);
    }

    #[test]
    fn driver_call_without_driver_is_no_interface() {
        assert_eq!(unsafe { InternalDriverCallX(99, core::ptr::null_mut()) }, E_NOINTERFACE);
        if driver_module().is_null() {
            assert_eq!(unsafe { InternalDriverCallX(1, core::ptr::null_mut()) }, E_NOINTERFACE);
        }
    }
}
