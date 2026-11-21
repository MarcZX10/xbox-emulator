use core::ffi::c_void;

pub const PC_ENCODER_DLL: &str = "mfh264enc.dll";
pub const CLSID_CMSH264_ENCODER_MFT: [u8; 16] = [0x44, 0x03, 0xa5, 0x6c, 0x1a, 0x05, 0xed, 0x4d, 0x97, 0x79, 0xa4, 0x33, 0x05, 0x16, 0x5e, 0x35];
pub const E_OUTOFMEMORY: i32 = 0x8007_000eu32 as i32;
pub const S_FALSE: i32 = 1;
const DLL_PROCESS_ATTACH: u32 = 1;

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemDirectoryW(buf: *mut u16, n: u32) -> u32;
    fn LoadLibraryW(name: *const u16) -> isize;
    fn GetProcAddress(m: isize, name: *const u8) -> *const c_void;
    fn DisableThreadLibraryCalls(m: isize) -> i32;
}

fn system_dll(name: &str) -> Vec<u16> {
    let mut buf = [0u16; 520];
    let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let mut v: Vec<u16> = buf[..n.min(buf.len())].to_vec();
    v.extend("\\".encode_utf16().chain(name.encode_utf16()).chain(Some(0)));
    v
}

pub fn pc_proc(name: &[u8]) -> *const c_void {
    let p = system_dll(PC_ENCODER_DLL);
    let m = unsafe { LoadLibraryW(p.as_ptr()) };
    if m == 0 {
        return core::ptr::null();
    }
    unsafe { GetProcAddress(m, name.as_ptr()) }
}

#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(clsid: *const u8, riid: *const u8, ppv: *mut *mut c_void) -> i32 {
    *ppv = core::ptr::null_mut();
    let f = pc_proc(b"DllGetClassObject\0");
    if f.is_null() {
        return E_OUTOFMEMORY;
    }
    let f: unsafe extern "system" fn(*const u8, *const u8, *mut *mut c_void) -> i32 = core::mem::transmute(f);
    f(clsid, riid, ppv)
}

#[no_mangle]
pub unsafe extern "system" fn MFDllGetClassObject(clsid: *const u8, riid: *const u8, ppv: *mut *mut c_void) -> i32 {
    DllGetClassObject(clsid, riid, ppv)
}

#[no_mangle]
pub unsafe extern "system" fn DllCanUnloadNow() -> i32 {
    let f = pc_proc(b"DllCanUnloadNow\0");
    if f.is_null() {
        return 0;
    }
    let f: unsafe extern "system" fn() -> i32 = core::mem::transmute(f);
    f()
}

#[no_mangle]
pub extern "system" fn DllRegisterServer() -> i32 {
    0
}

#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "system" fn DllMain(module: isize, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        DisableThreadLibraryCalls(module);
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    const IID_ICLASSFACTORY: [u8; 16] = [1, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];
    const IID_IMFTRANSFORM: [u8; 16] = [0x21, 0xc1, 0x94, 0xbf, 0x05, 0x5b, 0x6f, 0x4e, 0x80, 0x00, 0xba, 0x59, 0x89, 0x61, 0x41, 0x4d];

    unsafe fn slot(o: *mut c_void, i: usize) -> usize {
        *(*(o as *const *const usize)).add(i)
    }

    type Get = unsafe extern "system" fn(*const u8, *const u8, *mut *mut c_void) -> i32;

    unsafe fn make_encoder(get: Get) {
        let mut f: *mut c_void = 0x55 as *mut c_void;
        assert_eq!(get(CLSID_CMSH264_ENCODER_MFT.as_ptr(), IID_ICLASSFACTORY.as_ptr(), &mut f), 0);
        let create: unsafe extern "system" fn(*mut c_void, *mut c_void, *const u8, *mut *mut c_void) -> i32 = core::mem::transmute(slot(f, 3));
        let mut mft: *mut c_void = core::ptr::null_mut();
        assert_eq!(create(f, core::ptr::null_mut(), IID_IMFTRANSFORM.as_ptr(), &mut mft), 0);
        assert!(!mft.is_null());
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(mft, 2));
        rel(mft);
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(f, 2));
        rel(f);
    }

    #[test]
    fn dll_get_class_object_creates_the_encoder() {
        unsafe { make_encoder(DllGetClassObject) };
    }

    #[test]
    fn mf_dll_get_class_object_is_the_same_entry() {
        unsafe { make_encoder(MFDllGetClassObject) };
    }

    #[test]
    fn dll_can_unload_now_tracks_live_objects() {
        unsafe {
            let mut f: *mut c_void = core::ptr::null_mut();
            assert_eq!(DllGetClassObject(CLSID_CMSH264_ENCODER_MFT.as_ptr(), IID_ICLASSFACTORY.as_ptr(), &mut f), 0);
            assert_eq!(DllCanUnloadNow(), S_FALSE);
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(f, 2));
            rel(f);
        }
    }

    #[test]
    fn dll_register_server_returns_s_ok() {
        assert_eq!(DllRegisterServer(), 0);
    }

    #[test]
    fn dll_unregister_server_returns_s_ok() {
        assert_eq!(DllUnregisterServer(), 0);
    }

    #[test]
    fn dll_main_accepts_every_reason() {
        extern "system" {
            fn GetModuleHandleW(n: *const u16) -> isize;
        }
        let me = unsafe { GetModuleHandleW(core::ptr::null()) };
        for r in 0..4 {
            assert_eq!(unsafe { DllMain(me, r, core::ptr::null_mut()) }, 1);
        }
    }
}
