#![allow(non_snake_case)]

use core::ffi::c_void;

pub const S_OK: i32 = 0;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const CLASS_E_CLASSNOTAVAILABLE: i32 = 0x8004_0111u32 as i32;

pub type HSTRING = *mut c_void;

#[link(name = "runtimeobject")]
extern "system" {
    fn WindowsIsStringEmpty(s: HSTRING) -> i32;
    fn WindowsStringHasEmbeddedNull(s: HSTRING, has: *mut i32) -> i32;
    fn RoOriginateError(error: i32, message: HSTRING) -> i32;
    fn RoOriginateErrorW(error: i32, length: u32, message: *const u16) -> i32;
}

pub const ACTIVATABLE_CLASS_ID_NAME: &str = "activatibleClassId";

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn DllGetActivationFactory(id: HSTRING, out: *mut *mut c_void) -> i32 {
    *out = core::ptr::null_mut();
    let mut has_null = 0;
    if WindowsIsStringEmpty(id) != 0 || WindowsStringHasEmbeddedNull(id, &mut has_null) < 0 || has_null == 1 {
        let name: Vec<u16> = ACTIVATABLE_CLASS_ID_NAME.encode_utf16().chain(Some(0)).collect();
        RoOriginateErrorW(E_INVALIDARG, 0x12, name.as_ptr());
        return E_INVALIDARG;
    }
    RoOriginateError(CLASS_E_CLASSNOTAVAILABLE, id);
    CLASS_E_CLASSNOTAVAILABLE
}

#[cfg(test)]
mod tests;
