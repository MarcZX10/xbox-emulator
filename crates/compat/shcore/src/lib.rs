use core::ffi::c_void;

pub mod scale;
#[cfg(test)]
mod tests;

#[no_mangle]
pub unsafe extern "system" fn Ordinal227(kind: i32, handler: *mut c_void, cookie: *mut u32) -> i32 {
    scale::register(kind, handler, cookie)
}

#[no_mangle]
pub unsafe extern "system" fn RevokeScaleChangeNotifications(kind: i32, cookie: u32) -> i32 {
    scale::revoke(kind, cookie)
}

#[link(name = "user32")]
extern "system" {
    fn SystemParametersInfoW(action: u32, ui: u32, pv: *mut c_void, flags: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> u32;
}

pub const SPI_CONSOLE_A7: u32 = 0xa7;
pub const SPI_GETLOGICALDPIOVERRIDE: u32 = 0x9e;
const E_FAIL: i32 = 0x8000_4005u32 as i32;

fn last_error_hresult() -> i32 {
    let e = unsafe { GetLastError() } as i32;
    let hr = if e > 0 { ((e as u32 & 0xffff) | 0x8007_0000) as i32 } else { e };
    if hr >= 0 { E_FAIL } else { hr }
}

#[no_mangle]
pub extern "system" fn Ordinal225() -> u8 {
    let mut v = [0u32; 3];
    unsafe { SystemParametersInfoW(SPI_CONSOLE_A7, 0xc, v.as_mut_ptr() as *mut c_void, 0) };
    (v[2] as i32 > 0) as u8
}

#[no_mangle]
pub unsafe extern "system" fn Ordinal226(out: *mut u8) -> i32 {
    *out = 0;
    let mut v: i32 = 0;
    if SystemParametersInfoW(SPI_GETLOGICALDPIOVERRIDE, 0, &mut v as *mut i32 as *mut c_void, 0) == 0 {
        return last_error_hresult();
    }
    *out = (v > 0) as u8;
    0
}
