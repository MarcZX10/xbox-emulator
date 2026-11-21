use core::ffi::c_void;

pub const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
pub const DRIVERS: [&str; 6] = ["xgs12_d.dll", "xgs12_i.dll", "xgs12.dll", "xgs12_10d.dll", "xgs12_10i.dll", "xgs12_10.dll"];

type DriverCall = unsafe extern "system" fn(usize, usize, usize, usize) -> i32;

extern "system" {
    fn GetModuleHandleW(n: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(core::iter::once(0)).collect()
}

fn loaded_driver_call() -> Option<DriverCall> {
    for d in DRIVERS {
        let h = unsafe { GetModuleHandleW(wide(d).as_ptr()) };
        if !h.is_null() {
            let f = unsafe { GetProcAddress(h, b"UMDInternalDriverCallX\0".as_ptr()) };
            return if f.is_null() { None } else { Some(unsafe { core::mem::transmute::<*mut c_void, DriverCall>(f) }) };
        }
    }
    None
}

#[no_mangle]
pub unsafe extern "system" fn InternalDriverCallX(a: usize, b: usize, c: usize, d: usize) -> i32 {
    match loaded_driver_call() {
        Some(f) => f(a, b, c, d),
        None => E_NOINTERFACE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_loaded_driver_is_e_nointerface() {
        assert!(DRIVERS.iter().all(|d| unsafe { GetModuleHandleW(wide(d).as_ptr()) }.is_null()));
        assert_eq!(unsafe { InternalDriverCallX(0x10, 0, 0, 0) }, E_NOINTERFACE);
    }
}
