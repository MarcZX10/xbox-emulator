use crate::mem::{self, Source, ATTR_BUFFER, ATTR_OBJECT};
use crate::{Guid, E_NOINTERFACE, E_OUTOFMEMORY, S_OK};
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};

pub const IID_IUNKNOWN: Guid = Guid::new(0x0000_0000, 0x0000, 0x0000, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
pub const IID_ID3D10BLOB: Guid = Guid::new(0x8ba5_fb08, 0x5195, 0x40e2, [0xac, 0x58, 0x0d, 0x98, 0x9c, 0x3a, 0x01, 0x02]);

#[repr(C)]
struct Vtbl {
    query_interface: unsafe extern "system" fn(*mut Blob, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut Blob) -> u32,
    release: unsafe extern "system" fn(*mut Blob) -> u32,
    get_buffer_pointer: unsafe extern "system" fn(*mut Blob) -> *mut c_void,
    get_buffer_size: unsafe extern "system" fn(*mut Blob) -> usize,
}

#[repr(C)]
pub struct Blob {
    vtbl: *const Vtbl,
    pad: u32,
    refs: AtomicI32,
    data: *mut c_void,
    size: usize,
    source: Source,
}

static VTBL: Vtbl = Vtbl { query_interface, add_ref, release, get_buffer_pointer, get_buffer_size };

unsafe extern "system" fn query_interface(this: *mut Blob, riid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    if ppv.is_null() {
        return crate::E_POINTER;
    }
    if !riid.is_null() && (*riid == IID_IUNKNOWN || *riid == IID_ID3D10BLOB) {
        add_ref(this);
        *ppv = this as *mut c_void;
        return S_OK;
    }
    *ppv = core::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn add_ref(this: *mut Blob) -> u32 {
    ((*this).refs.fetch_add(1, Ordering::AcqRel) + 1) as u32
}

unsafe extern "system" fn release(this: *mut Blob) -> u32 {
    let n = (*this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n < 0 {
        (*this).refs.fetch_add(1, Ordering::AcqRel);
        return 0;
    }
    if n == 0 {
        let src = (*this).source;
        if !(*this).data.is_null() {
            mem::free(src, (*this).data, ATTR_BUFFER);
        }
        mem::free(src, this as *mut c_void, ATTR_OBJECT);
    }
    n as u32
}

unsafe extern "system" fn get_buffer_pointer(this: *mut Blob) -> *mut c_void {
    if this.is_null() { core::ptr::null_mut() } else { (*this).data }
}

unsafe extern "system" fn get_buffer_size(this: *mut Blob) -> usize {
    if this.is_null() { 0 } else { (*this).size }
}

pub unsafe fn create(src: Source, size: usize, pp: *mut *mut c_void) -> i32 {
    let obj = mem::alloc(src, core::mem::size_of::<Blob>(), ATTR_OBJECT) as *mut Blob;
    if obj.is_null() {
        *pp = core::ptr::null_mut();
        return E_OUTOFMEMORY;
    }
    let data = mem::alloc(src, size, ATTR_BUFFER);
    if data.is_null() {
        mem::free(src, obj as *mut c_void, ATTR_OBJECT);
        *pp = core::ptr::null_mut();
        return E_OUTOFMEMORY;
    }
    core::ptr::write_bytes(data as *mut u8, 0, size);
    obj.write(Blob { vtbl: &VTBL, pad: 0, refs: AtomicI32::new(1), data, size, source: src });
    *pp = obj as *mut c_void;
    S_OK
}

pub unsafe fn with_bytes(src: Source, bytes: &[u8], pp: *mut *mut c_void) -> i32 {
    let hr = create(src, bytes.len(), pp);
    if hr == S_OK {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), (**(pp as *mut *mut Blob)).data as *mut u8, bytes.len());
    }
    hr
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub unsafe fn call_vt<T>(obj: *mut c_void, slot: usize) -> T
    where
        T: Copy,
    {
        let vt = *(obj as *const *const usize);
        core::mem::transmute_copy(&*vt.add(slot))
    }

    pub unsafe fn bytes_of(blob: *mut c_void) -> Vec<u8> {
        let ptr: unsafe extern "system" fn(*mut c_void) -> *const u8 = call_vt(blob, 3);
        let size: unsafe extern "system" fn(*mut c_void) -> usize = call_vt(blob, 4);
        core::slice::from_raw_parts(ptr(blob), size(blob)).to_vec()
    }

    pub unsafe fn release_obj(obj: *mut c_void) -> u32 {
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = call_vt(obj, 2);
        rel(obj)
    }

    #[test]
    fn blob_lifecycle_and_interfaces() {
        unsafe {
            for src in [Source::XMem, Source::Hooks] {
                let mut b: *mut c_void = core::ptr::null_mut();
                assert_eq!(create(src, 48, &mut b), S_OK);
                assert_eq!(bytes_of(b), vec![0u8; 48]);
                let qi: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 = call_vt(b, 0);
                let mut q: *mut c_void = core::ptr::null_mut();
                assert_eq!(qi(b, &IID_ID3D10BLOB, &mut q), S_OK);
                assert_eq!(q, b);
                assert_eq!(qi(b, &IID_IUNKNOWN, &mut q), S_OK);
                let other = Guid::new(1, 2, 3, [4; 8]);
                assert_eq!(qi(b, &other, &mut q), E_NOINTERFACE);
                assert!(q.is_null());
                assert_eq!(release_obj(b), 2);
                assert_eq!(release_obj(b), 1);
                assert_eq!(release_obj(b), 0);
            }
            assert!(get_buffer_pointer(core::ptr::null_mut()).is_null());
            assert_eq!(get_buffer_size(core::ptr::null_mut()), 0);
            let mut b: *mut c_void = core::ptr::null_mut();
            assert_eq!(with_bytes(Source::XMem, b"abc\0", &mut b), S_OK);
            assert_eq!(bytes_of(b), b"abc\0");
            assert_eq!(release_obj(b), 0);
        }
    }
}
