use crate::blob;
use crate::host;
use crate::mem::Source;
use crate::{E_FAIL, E_INVALIDARG, S_OK};
use core::ffi::c_void;

type SerializeFn = unsafe extern "system" fn(*const c_void, u32, *mut *mut c_void, *mut *mut c_void) -> i32;
type SerializeVersionedFn = unsafe extern "system" fn(*const c_void, *mut *mut c_void, *mut *mut c_void) -> i32;
type DeserializerFn = unsafe extern "system" fn(*const c_void, usize, *const c_void, *mut *mut c_void) -> i32;

pub unsafe fn serialize(desc: *const c_void, version: u32, pp_blob: *mut *mut c_void, pp_error: *mut *mut c_void) -> i32 {
    let (p, _) = host::runtime_proc(b"D3D12SerializeRootSignature\0");
    if p == 0 {
        return E_FAIL;
    }
    let f: SerializeFn = core::mem::transmute(p);
    f(desc, version, pp_blob, pp_error)
}

pub fn unsupported_version_message(version: u32) -> Vec<u8> {
    format!("D3DSerializeRootSignature: unsupported root signature version ({version})\n\0").into_bytes()
}

pub unsafe fn serialize_versioned(src: Source, desc: *const c_void, pp_blob: *mut *mut c_void, pp_error: *mut *mut c_void) -> i32 {
    if desc.is_null() || pp_blob.is_null() {
        return E_INVALIDARG;
    }
    *pp_blob = core::ptr::null_mut();
    if !pp_error.is_null() {
        *pp_error = core::ptr::null_mut();
    }
    let version = *(desc as *const u32);
    if version != 1 && version != 2 {
        if !pp_error.is_null() {
            let mut b: *mut c_void = core::ptr::null_mut();
            let hr = blob::with_bytes(src, &unsupported_version_message(version), &mut b);
            if hr != S_OK {
                return hr;
            }
            *pp_error = b;
        }
        return E_FAIL;
    }
    let (p, _) = host::runtime_proc(b"D3D12SerializeVersionedRootSignature\0");
    if p == 0 {
        return E_FAIL;
    }
    let f: SerializeVersionedFn = core::mem::transmute(p);
    f(desc, pp_blob, pp_error)
}

pub unsafe fn deserializer(versioned: bool, data: *const c_void, size: usize, riid: *const c_void, pp: *mut *mut c_void) -> i32 {
    if pp.is_null() {
        return E_INVALIDARG;
    }
    *pp = core::ptr::null_mut();
    let name: &[u8] = if versioned { b"D3D12CreateVersionedRootSignatureDeserializer\0" } else { b"D3D12CreateRootSignatureDeserializer\0" };
    let p = host::proc_in(host::host_d3d12(), name);
    if p == 0 {
        return E_INVALIDARG;
    }
    let f: DeserializerFn = core::mem::transmute(p);
    let hr = f(data, size, riid, pp);
    if hr == E_FAIL { E_INVALIDARG } else { hr }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::tests::{bytes_of, release_obj};
    use crate::Guid;

    #[repr(C)]
    struct RootDesc10 {
        num_parameters: u32,
        parameters: *const c_void,
        num_static_samplers: u32,
        static_samplers: *const c_void,
        flags: u32,
    }

    #[repr(C)]
    struct Versioned {
        version: u32,
        desc: RootDesc10,
    }

    const IID_DESERIALIZER: Guid = Guid::new(0x34ab_647b, 0x3cc8, 0x46ac, [0x84, 0x1b, 0xc0, 0x96, 0x56, 0x45, 0xc0, 0x46]);
    const IID_VERSIONED_DESERIALIZER: Guid = Guid::new(0x7f91_ce67, 0x090c, 0x4bb7, [0xb7, 0x8e, 0xed, 0x8f, 0xf2, 0xe3, 0x1d, 0xa0]);

    fn empty() -> RootDesc10 {
        RootDesc10 { num_parameters: 0, parameters: core::ptr::null(), num_static_samplers: 0, static_samplers: core::ptr::null(), flags: 1 }
    }

    #[test]
    fn serialize_and_deserialize_round_trip() {
        unsafe {
            let d = empty();
            let mut b: *mut c_void = core::ptr::null_mut();
            let mut e: *mut c_void = core::ptr::null_mut();
            assert_eq!(serialize(&d as *const _ as *const c_void, 1, &mut b, &mut e), S_OK);
            let bytes = bytes_of(b);
            assert_eq!(&bytes[..4], b"DXBC");
            let mut des: *mut c_void = core::ptr::null_mut();
            assert_eq!(deserializer(false, bytes.as_ptr() as *const c_void, bytes.len(), &IID_DESERIALIZER as *const _ as *const c_void, &mut des), S_OK);
            assert!(!des.is_null());
            release_obj(des);
            let mut vdes: *mut c_void = core::ptr::null_mut();
            assert_eq!(deserializer(true, bytes.as_ptr() as *const c_void, bytes.len(), &IID_VERSIONED_DESERIALIZER as *const _ as *const c_void, &mut vdes), S_OK);
            assert!(!vdes.is_null());
            release_obj(vdes);
            release_obj(b);
        }
    }

    #[test]
    fn deserializer_console_error_mapping() {
        unsafe {
            assert_eq!(deserializer(false, core::ptr::null(), 0, &IID_DESERIALIZER as *const _ as *const c_void, core::ptr::null_mut()), E_INVALIDARG);
            let junk = [0u8; 64];
            let mut p: *mut c_void = 1 as *mut c_void;
            let hr = deserializer(false, junk.as_ptr() as *const c_void, junk.len(), &IID_DESERIALIZER as *const _ as *const c_void, &mut p);
            assert!(hr < 0 && hr != E_FAIL, "{hr:#x}");
            assert!(p.is_null());
            let mut p: *mut c_void = 1 as *mut c_void;
            let hr = deserializer(true, junk.as_ptr() as *const c_void, junk.len(), &IID_VERSIONED_DESERIALIZER as *const _ as *const c_void, &mut p);
            assert!(hr < 0 && hr != E_FAIL, "{hr:#x}");
        }
    }

    #[test]
    fn versioned_serialize_console_paths() {
        unsafe {
            for src in [Source::XMem, Source::Hooks] {
                assert_eq!(serialize_versioned(src, core::ptr::null(), &mut core::ptr::null_mut(), core::ptr::null_mut()), E_INVALIDARG);
                let v = Versioned { version: 1, desc: empty() };
                assert_eq!(serialize_versioned(src, &v as *const _ as *const c_void, core::ptr::null_mut(), core::ptr::null_mut()), E_INVALIDARG);
                let mut b: *mut c_void = 1 as *mut c_void;
                let mut e: *mut c_void = 1 as *mut c_void;
                assert_eq!(serialize_versioned(src, &v as *const _ as *const c_void, &mut b, &mut e), S_OK);
                assert_eq!(&bytes_of(b)[..4], b"DXBC");
                assert!(e.is_null());
                release_obj(b);
                let bad = Versioned { version: 3, desc: empty() };
                let mut b: *mut c_void = 1 as *mut c_void;
                let mut e: *mut c_void = core::ptr::null_mut();
                assert_eq!(serialize_versioned(src, &bad as *const _ as *const c_void, &mut b, &mut e), E_FAIL);
                assert!(b.is_null());
                assert_eq!(bytes_of(e), unsupported_version_message(3));
                assert_eq!(bytes_of(e), b"D3DSerializeRootSignature: unsupported root signature version (3)\n\0");
                release_obj(e);
                let mut b: *mut c_void = core::ptr::null_mut();
                assert_eq!(serialize_versioned(src, &bad as *const _ as *const c_void, &mut b, core::ptr::null_mut()), E_FAIL);
            }
        }
    }
}
