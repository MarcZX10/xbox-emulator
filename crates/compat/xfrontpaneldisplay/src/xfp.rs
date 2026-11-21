use core::ffi::c_void;

extern "system" {
    fn GetModuleHandleW(n: *const u16) -> *mut c_void;
    fn LoadLibraryW(n: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
}

pub const E_MOD_NOT_FOUND: i32 = 0x8007_007eu32 as i32;
pub const E_PROC_NOT_FOUND: i32 = 0x8007_007fu32 as i32;

pub fn try_connect() -> i32 {
    let name: Vec<u16> = "xfp_gc.dll".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut h = GetModuleHandleW(name.as_ptr());
        if h.is_null() {
            h = LoadLibraryW(name.as_ptr());
        }
        if h.is_null() {
            return E_MOD_NOT_FOUND;
        }
        let p = GetProcAddress(h, b"XfpTryConnect\0".as_ptr());
        if p.is_null() {
            return E_PROC_NOT_FOUND;
        }
        let f: extern "system" fn() -> i32 = core::mem::transmute(p);
        f()
    }
}
