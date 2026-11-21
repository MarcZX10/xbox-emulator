use crate::guids::{Guid, IID_IDXC_BLOB_ENCODING};
use core::ffi::c_void;

pub type Hr = i32;

pub const S_OK: Hr = 0;
pub const S_FALSE: Hr = 1;
pub const E_NOTIMPL: Hr = 0x8000_4001u32 as i32;
pub const E_NOINTERFACE: Hr = 0x8000_4002u32 as i32;
pub const E_POINTER: Hr = 0x8000_4003u32 as i32;
pub const E_FAIL: Hr = 0x8000_4005u32 as i32;
pub const E_INVALIDARG: Hr = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY: Hr = 0x8007_000Eu32 as i32;
pub const E_BOUNDS: Hr = 0x8000_000Bu32 as i32;
pub const REGDB_E_CLASSNOTREG: Hr = 0x8004_0154u32 as i32;
pub const HRESULT_FILE_NOT_FOUND: Hr = 0x8007_0002u32 as i32;
pub const DXC_E_CONTAINER_INVALID: Hr = 0x80AA_000Du32 as i32;
pub const DXC_E_MISSING_PART: Hr = 0x80AA_0012u32 as i32;

pub const DXC_CP_ACP: u32 = 0;
pub const DXC_CP_UTF16: u32 = 1200;
pub const DXC_CP_UTF8: u32 = 65001;

#[repr(C)]
pub struct UnknownVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hr,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
pub struct BlobVtblPrefix {
    pub unk: UnknownVtbl,
    pub get_buffer_pointer: unsafe extern "system" fn(*mut c_void) -> *mut c_void,
    pub get_buffer_size: unsafe extern "system" fn(*mut c_void) -> usize,
}

#[repr(C)]
pub struct BlobEncodingVtblPrefix {
    pub blob: BlobVtblPrefix,
    pub get_encoding: unsafe extern "system" fn(*mut c_void, *mut i32, *mut u32) -> Hr,
}

#[repr(C)]
pub struct MallocVtbl {
    pub unk: UnknownVtbl,
    pub alloc: unsafe extern "system" fn(*mut c_void, usize) -> *mut c_void,
    pub realloc: unsafe extern "system" fn(*mut c_void, *mut c_void, usize) -> *mut c_void,
    pub free: unsafe extern "system" fn(*mut c_void, *mut c_void),
}

#[repr(C)]
pub struct DxcBuffer {
    pub ptr: *const c_void,
    pub size: usize,
    pub encoding: u32,
}

pub unsafe fn vtbl<T>(obj: *mut c_void) -> &'static T {
    &**(obj as *mut *const T)
}

pub unsafe fn add_ref(obj: *mut c_void) -> u32 {
    (vtbl::<UnknownVtbl>(obj).add_ref)(obj)
}

pub unsafe fn release(obj: *mut c_void) -> u32 {
    (vtbl::<UnknownVtbl>(obj).release)(obj)
}

pub unsafe fn query(obj: *mut c_void, iid: &Guid) -> Option<*mut c_void> {
    let mut out = core::ptr::null_mut();
    let hr = (vtbl::<UnknownVtbl>(obj).query_interface)(obj, iid, &mut out);
    (hr >= 0 && !out.is_null()).then_some(out)
}

pub unsafe fn blob_bytes<'a>(blob: *mut c_void) -> &'a [u8] {
    let v = vtbl::<BlobVtblPrefix>(blob);
    let p = (v.get_buffer_pointer)(blob) as *const u8;
    let n = (v.get_buffer_size)(blob);
    if p.is_null() || n == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(p, n)
    }
}

pub unsafe fn blob_encoding(blob: *mut c_void) -> Option<u32> {
    let enc = query(blob, &IID_IDXC_BLOB_ENCODING)?;
    let (mut known, mut cp) = (0i32, 0u32);
    let hr = (vtbl::<BlobEncodingVtblPrefix>(enc).get_encoding)(enc, &mut known, &mut cp);
    release(enc);
    (hr >= 0 && known != 0).then_some(cp)
}

pub unsafe fn wide_to_string(p: *const u16) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let mut n = 0usize;
    while *p.add(n) != 0 {
        n += 1;
    }
    Some(String::from_utf16_lossy(core::slice::from_raw_parts(p, n)))
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(core::iter::once(0)).collect()
}

pub unsafe fn put<T>(out: *mut *mut T, v: *mut T) {
    if !out.is_null() {
        *out = v;
    }
}
