use core::ffi::c_void;

pub const E_OUTOFMEMORY: i32 = 0x8007_000eu32 as i32;
pub const CLSID_CMSH264_DECODER_MFT: [u8; 16] = [0x72, 0x7e, 0xce, 0x62, 0x71, 0x4c, 0x20, 0x4d, 0xb1, 0x5d, 0x45, 0x28, 0x31, 0xa8, 0x7d, 0x9d];
pub const PC_DECODER_DLL: &str = "msmpeg2vdec.dll";

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemDirectoryW(buf: *mut u16, n: u32) -> u32;
    fn LoadLibraryW(name: *const u16) -> isize;
    fn GetProcAddress(m: isize, name: *const u8) -> *const c_void;
}

type GetClassObject = unsafe extern "system" fn(*const u8, *const u8, *mut *mut c_void) -> i32;

pub fn system_dll(name: &str) -> Vec<u16> {
    let mut buf = [0u16; 520];
    let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let mut v: Vec<u16> = buf[..n.min(buf.len())].to_vec();
    v.extend("\\".encode_utf16().chain(name.encode_utf16()).chain(Some(0)));
    v
}

pub fn pc_get_class_object() -> Option<GetClassObject> {
    let p = system_dll(PC_DECODER_DLL);
    let m = unsafe { LoadLibraryW(p.as_ptr()) };
    if m == 0 {
        return None;
    }
    let f = unsafe { GetProcAddress(m, b"DllGetClassObject\0".as_ptr()) };
    (!f.is_null()).then(|| unsafe { core::mem::transmute::<*const c_void, GetClassObject>(f) })
}

#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(clsid: *const u8, riid: *const u8, ppv: *mut *mut c_void) -> i32 {
    if core::slice::from_raw_parts(clsid, 16) != CLSID_CMSH264_DECODER_MFT {
        return E_OUTOFMEMORY;
    }
    match pc_get_class_object() {
        Some(f) => f(clsid, riid, ppv),
        None => E_OUTOFMEMORY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IID_ICLASSFACTORY: [u8; 16] = [1, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];
    const IID_IMFTRANSFORM: [u8; 16] = [0x21, 0xc1, 0x94, 0xbf, 0x05, 0x5b, 0x6f, 0x4e, 0x80, 0x00, 0xba, 0x59, 0x89, 0x61, 0x41, 0x4d];

    unsafe fn slot(o: *mut c_void, i: usize) -> usize {
        *(*(o as *const *const usize)).add(i)
    }

    #[test]
    fn dll_get_class_object_matches_console() {
        let mut p: *mut c_void = 0x1234 as *mut c_void;
        let other = [0u8; 16];
        assert_eq!(unsafe { DllGetClassObject(other.as_ptr(), IID_ICLASSFACTORY.as_ptr(), &mut p) }, E_OUTOFMEMORY);
        assert_eq!(p as usize, 0x1234);
        unsafe {
            assert_eq!(DllGetClassObject(CLSID_CMSH264_DECODER_MFT.as_ptr(), IID_ICLASSFACTORY.as_ptr(), &mut p), 0);
            let create: unsafe extern "system" fn(*mut c_void, *mut c_void, *const u8, *mut *mut c_void) -> i32 = core::mem::transmute(slot(p, 3));
            let mut mft: *mut c_void = core::ptr::null_mut();
            assert_eq!(create(p, core::ptr::null_mut(), IID_IMFTRANSFORM.as_ptr(), &mut mft), 0);
            assert!(!mft.is_null());
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(mft, 2));
            rel(mft);
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(p, 2));
            rel(p);
        }
    }
}
