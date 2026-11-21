use crate::host;
use crate::{Family, Guid, E_INVALIDARG, E_NOINTERFACE};
use compat_common::record;
use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};

pub const IID_ID3D12DEVICE: Guid = Guid::new(0x1898_19f1, 0x1db6, 0x4b57, [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7]);
pub const IID_ID3D12DEVICE1: Guid = Guid::new(0x77ac_ce80, 0x638e, 0x4e65, [0x88, 0x95, 0xc1, 0xf2, 0x33, 0x86, 0x86, 0x3e]);
pub const IID_ID3D12DEVICE2: Guid = Guid::new(0x30ba_a41e, 0xb15b, 0x475c, [0xa0, 0xbb, 0x1a, 0xf5, 0xc5, 0xb6, 0x43, 0x28]);

pub const ONE_DEVICE_IIDS: [Guid; 5] = [
    IID_ID3D12DEVICE,
    IID_ID3D12DEVICE1,
    IID_ID3D12DEVICE2,
    Guid::new(0x4609_acf9, 0xab9d, 0x4392, [0x97, 0x1d, 0xa5, 0xe2, 0x59, 0xca, 0x4d, 0x98]),
    Guid::new(0x547e_33c7, 0xff86, 0x4cd9, [0xbe, 0xa3, 0x5d, 0x4a, 0x28, 0x37, 0x53, 0x96]),
];

pub const SCARLETT_DEVICE_IIDS: [Guid; 9] = [
    IID_ID3D12DEVICE,
    IID_ID3D12DEVICE1,
    IID_ID3D12DEVICE2,
    Guid::new(0x81da_dc15, 0x2bad, 0x4392, [0x93, 0xc5, 0x10, 0x13, 0x45, 0xc4, 0xaa, 0x98]),
    Guid::new(0xe865_df17, 0xa9ee, 0x46f9, [0xa4, 0x63, 0x30, 0x98, 0x31, 0x5a, 0xa2, 0xe5]),
    Guid::new(0x8b4f_173b, 0x2fea, 0x4b80, [0x8f, 0x58, 0x43, 0x07, 0x19, 0x1a, 0xb9, 0x5d]),
    Guid::new(0xc70b_221b, 0x40e4, 0x4a17, [0x89, 0xaf, 0x02, 0x5a, 0x07, 0x27, 0xa6, 0xdc]),
    Guid::new(0x5c01_4b53, 0x68a1, 0x4b9b, [0x8b, 0xd1, 0xdd, 0x60, 0x46, 0xb9, 0x35, 0x8b]),
    Guid::new(0x728f_1fa2, 0xea11, 0x4a9f, [0x9b, 0x52, 0x9b, 0x42, 0xa4, 0x84, 0xd5, 0xc9]),
];

pub const ONE_CURRENT_VERSION: u32 = 0x1000a;
pub const SCARLETT_MIN_VERSION: u32 = 0x20001;
pub const SDK_MISMATCH: &[u8] = b"D3D12_SDK_VERSION is incompatible and the title needs to be rebuilt\n\0";
const SUPPRESS_FILE: &[u8] = b"D3D_suppressSdkVersionCheck\0";
const D3D_FEATURE_LEVEL_12_0: u32 = 0xc000;

extern "system" {
    fn OutputDebugStringA(s: *const u8);
    fn GetFileAttributesA(p: *const u8) -> u32;
}

pub type CreateFn = unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut *mut c_void) -> i32;

static TARGET_OVERRIDE: AtomicUsize = AtomicUsize::new(0);

pub fn override_target(f: Option<CreateFn>) {
    TARGET_OVERRIDE.store(f.map_or(0, |f| f as usize), Ordering::Release);
}

pub fn accepts(family: Family, riid: &Guid) -> bool {
    match family {
        Family::One => ONE_DEVICE_IIDS.contains(riid),
        Family::Scarlett => SCARLETT_DEVICE_IIDS.contains(riid),
    }
}

pub unsafe fn validate(family: Family, params: *const u32, riid: *const Guid, pp: *mut *mut c_void) -> Result<(), i32> {
    if params.is_null() || pp.is_null() || riid.is_null() {
        return Err(E_INVALIDARG);
    }
    let version = *params;
    match family {
        Family::One => {
            if version != ONE_CURRENT_VERSION && version > 0x10009 {
                return Err(E_INVALIDARG);
            }
        }
        Family::Scarlett => {
            if version < SCARLETT_MIN_VERSION {
                OutputDebugStringA(SDK_MISMATCH.as_ptr());
                if GetFileAttributesA(SUPPRESS_FILE.as_ptr()) == u32::MAX {
                    return Err(E_INVALIDARG);
                }
            }
        }
    }
    if !accepts(family, &*riid) {
        return Err(E_NOINTERFACE);
    }
    Ok(())
}

pub unsafe fn create(family: Family, adapter: *mut c_void, params: *const c_void, riid: *const c_void, pp: *mut *mut c_void) -> i32 {
    if let Err(hr) = validate(family, params as *const u32, riid as *const Guid, pp) {
        record(crate::M, "UMDCreateDevice12", &format!("{family:?} params {params:p} -> {hr:#010x}"));
        return hr;
    }
    *pp = core::ptr::null_mut();
    let o = TARGET_OVERRIDE.load(Ordering::Acquire);
    if o != 0 {
        let f: CreateFn = core::mem::transmute(o);
        return f(adapter, params, riid, pp);
    }
    let xs = host::proc_in(host::d3d12_xs(), b"D3D12XboxCreateDevice\0");
    if xs != 0 {
        let f: CreateFn = core::mem::transmute(xs);
        let hr = f(adapter, params, riid, pp);
        record(crate::M, "UMDCreateDevice12", &format!("{family:?} -> d3d12_xs D3D12XboxCreateDevice {:p} hr={hr:#010x}", *pp));
        return hr;
    }
    let p = host::proc_in(host::host_d3d12(), b"D3D12CreateDevice\0");
    if p == 0 {
        return crate::E_FAIL;
    }
    let f: unsafe extern "system" fn(*mut c_void, u32, *const c_void, *mut *mut c_void) -> i32 = core::mem::transmute(p);
    let hr = f(core::ptr::null_mut(), D3D_FEATURE_LEVEL_12_0, riid, pp);
    record(crate::M, "UMDCreateDevice12", &format!("{family:?} d3d12_xs.dll not loaded -> host D3D12CreateDevice hr={hr:#010x}"));
    hr
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::tests::release_obj;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());
    static SEEN: Mutex<Vec<(usize, usize, usize, usize)>> = Mutex::new(Vec::new());

    unsafe extern "system" fn fake(a: *mut c_void, p: *const c_void, r: *const c_void, pp: *mut *mut c_void) -> i32 {
        SEEN.lock().unwrap().push((a as usize, p as usize, r as usize, pp as usize));
        *pp = 0x1234 as *mut c_void;
        0
    }

    #[test]
    fn iid_tables_match_console_text() {
        assert_eq!(ONE_DEVICE_IIDS[0].to_string(), "189819f1-1db6-4b57-be54-1821339b85f7");
        assert_eq!(ONE_DEVICE_IIDS[4].to_string(), "547e33c7-ff86-4cd9-bea3-5d4a28375396");
        assert_eq!(SCARLETT_DEVICE_IIDS[8].to_string(), "728f1fa2-ea11-4a9f-9b52-9b42a484d5c9");
        assert_eq!(SCARLETT_DEVICE_IIDS[5].to_string(), "8b4f173b-2fea-4b80-8f58-4307191ab95d");
    }

    #[test]
    fn console_validation() {
        unsafe {
            let mut out: *mut c_void = core::ptr::null_mut();
            let one = [0x1000au32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let old = [0x10005u32; 20];
            let future = [0x1000bu32; 20];
            let xs = [0x20004u32; 20];
            let xs_old = [0x10000u32; 20];
            assert_eq!(validate(Family::One, core::ptr::null(), &IID_ID3D12DEVICE, &mut out), Err(E_INVALIDARG));
            assert_eq!(validate(Family::One, one.as_ptr(), &IID_ID3D12DEVICE, core::ptr::null_mut()), Err(E_INVALIDARG));
            assert_eq!(validate(Family::One, one.as_ptr(), &IID_ID3D12DEVICE, &mut out), Ok(()));
            assert_eq!(validate(Family::One, old.as_ptr(), &IID_ID3D12DEVICE2, &mut out), Ok(()));
            assert_eq!(validate(Family::One, future.as_ptr(), &IID_ID3D12DEVICE, &mut out), Err(E_INVALIDARG));
            assert_eq!(validate(Family::One, one.as_ptr(), &SCARLETT_DEVICE_IIDS[8], &mut out), Err(E_NOINTERFACE));
            assert_eq!(validate(Family::Scarlett, xs.as_ptr(), &SCARLETT_DEVICE_IIDS[8], &mut out), Ok(()));
            assert_eq!(validate(Family::Scarlett, xs.as_ptr(), &ONE_DEVICE_IIDS[4], &mut out), Err(E_NOINTERFACE));
            assert_eq!(validate(Family::Scarlett, xs_old.as_ptr(), &IID_ID3D12DEVICE, &mut out), Err(E_INVALIDARG));
        }
    }

    #[test]
    fn forwards_to_runtime_target_unchanged() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            override_target(Some(fake));
            let params = [0x20004u32; 20];
            let mut out: *mut c_void = 1 as *mut c_void;
            let hr = create(Family::Scarlett, 0x55 as *mut c_void, params.as_ptr() as *const c_void, &IID_ID3D12DEVICE as *const _ as *const c_void, &mut out);
            override_target(None);
            assert_eq!(hr, 0);
            assert_eq!(out as usize, 0x1234);
            let seen = SEEN.lock().unwrap().pop().unwrap();
            assert_eq!(seen, (0x55, params.as_ptr() as usize, &IID_ID3D12DEVICE as *const _ as usize, &mut out as *mut _ as usize));
            let mut out: *mut c_void = 1 as *mut c_void;
            let hr = create(Family::One, core::ptr::null_mut(), params.as_ptr() as *const c_void, &IID_ID3D12DEVICE as *const _ as *const c_void, &mut out);
            assert_eq!(hr, E_INVALIDARG);
            assert_eq!(out as usize, 1);
        }
    }

    #[test]
    fn creates_real_device_without_emulator_runtime() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let params = [0x1000au32; 20];
            let mut dev: *mut c_void = core::ptr::null_mut();
            let hr = create(Family::One, core::ptr::null_mut(), params.as_ptr() as *const c_void, &IID_ID3D12DEVICE as *const _ as *const c_void, &mut dev);
            assert_eq!(hr, 0, "{hr:#x}");
            assert!(!dev.is_null());
            release_obj(dev);
        }
    }
}
