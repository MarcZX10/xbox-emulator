use core::ffi::c_void;
use std::sync::Once;
use core::sync::atomic::{AtomicUsize, Ordering};
use compat_common::record;

use crate::device_wrap::Guid;

static CLSID_D3D12_SDK_CONFIGURATION: Guid = Guid {
    d1: 0x7cda_6aca, d2: 0xa03e, d3: 0x49c8, d4: [0x94, 0x58, 0x03, 0x34, 0xd2, 0x0e, 0x07, 0xce],
};
static IID_ID3D12_SDK_CONFIGURATION: Guid = Guid {
    d1: 0xe9eb_5314, d2: 0x33aa, d3: 0x42b2, d4: [0xa7, 0x18, 0xd7, 0x7f, 0x58, 0xb1, 0xf1, 0xc7],
};

static IID_ID3D12_SDK_CONFIGURATION1: Guid = Guid {
    d1: 0x8aaf_9303, d2: 0xad25, d3: 0x48b9, d4: [0x9a, 0x57, 0xd9, 0xc3, 0x7e, 0x00, 0x9d, 0x9f],
};
static IID_ID3D12_DEVICE_FACTORY: Guid = Guid {
    d1: 0x61f3_07d3, d2: 0xd34e, d3: 0x4e7c, d4: [0x83, 0x74, 0x3b, 0xa4, 0xde, 0x23, 0xcc, 0xcb],
};
static CLSID_D3D12_DEBUG: Guid = Guid {
    d1: 0xf235_2aeb, d2: 0xdd84, d3: 0x49fe, d4: [0xb9, 0x7b, 0xa9, 0xdc, 0xfd, 0xcc, 0x1b, 0x4f],
};
static IID_ID3D12_DEBUG: Guid = Guid {
    d1: 0x3444_88b7, d2: 0x6846, d3: 0x474b, d4: [0xb9, 0x89, 0xf0, 0x27, 0x44, 0x82, 0x45, 0xe0],
};

static FACTORY: AtomicUsize = AtomicUsize::new(0);

unsafe fn slot(obj: *mut c_void, i: usize) -> usize {
    *(*(obj as *const *const usize)).add(i)
}

unsafe fn release(obj: *mut c_void) {
    let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(obj, 2));
    rel(obj);
}

unsafe fn make_factory(cfg: *mut c_void, version: u32, cdir: &[u8]) -> i32 {
    let qi: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(slot(cfg, 0));
    let mut cfg1: *mut c_void = core::ptr::null_mut();
    let hr = qi(cfg, &IID_ID3D12_SDK_CONFIGURATION1, &mut cfg1);
    if hr < 0 || cfg1.is_null() {
        return hr;
    }
    let create: unsafe extern "system" fn(*mut c_void, u32, *const u8, *const Guid, *mut *mut c_void) -> i32 =
        core::mem::transmute(slot(cfg1, 4));
    let mut factory: *mut c_void = core::ptr::null_mut();
    let hr = create(cfg1, version, cdir.as_ptr(), &IID_ID3D12_DEVICE_FACTORY, &mut factory);
    release(cfg1);
    if hr >= 0 && !factory.is_null() {
        FACTORY.store(factory as usize, Ordering::Release);
    }
    hr
}

pub fn create_device(adapter: *mut c_void, level: u32, riid: *const c_void, pp: *mut *mut c_void) -> Option<i32> {
    let factory = FACTORY.load(Ordering::Acquire) as *mut c_void;
    if factory.is_null() {
        return None;
    }
    unsafe {
        if std::env::var("TOMBOX_D3D12_DEBUG").is_ok() {
            let get: unsafe extern "system" fn(*mut c_void, *const Guid, *const Guid, *mut *mut c_void) -> i32 =
                core::mem::transmute(slot(factory, 7));
            let mut dbg: *mut c_void = core::ptr::null_mut();
            let hr = get(factory, &CLSID_D3D12_DEBUG, &IID_ID3D12_DEBUG, &mut dbg);
            if hr >= 0 && !dbg.is_null() {
                let enable: unsafe extern "system" fn(*mut c_void) = core::mem::transmute(slot(dbg, 3));
                enable(dbg);
                release(dbg);
                record("d3d12_xs", "agility", "debug layer ENABLED on the Agility device factory");
            } else {
                record("d3d12_xs", "agility", &format!("factory GetConfigurationInterface(D3D12Debug) hr={hr:#010x}"));
            }
        }
        let create: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *const c_void, *mut *mut c_void) -> i32 =
            core::mem::transmute(slot(factory, 9));
        let hr = create(factory, adapter, level, riid, pp);
        record("d3d12_xs", "agility", &format!("factory CreateDevice hr={hr:#010x}"));
        Some(hr)
    }
}

pub fn select() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Ok(path) = std::env::var("TOMBOX_AGILITY_PATH") else { return };
        let version: u32 = std::env::var("TOMBOX_AGILITY_VERSION").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        if version == 0 {
            record("d3d12_xs", "agility", &format!("TOMBOX_AGILITY_PATH={path} but TOMBOX_AGILITY_VERSION is unset"));
            return;
        }
        let p = crate::host_proc(b"D3D12GetInterface\0");
        if p == 0 {
            record("d3d12_xs", "agility", "host d3d12.dll has no D3D12GetInterface");
            return;
        }
        let get: unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32 =
            unsafe { core::mem::transmute(p) };
        let mut cfg: *mut c_void = core::ptr::null_mut();
        let hr = unsafe { get(&CLSID_D3D12_SDK_CONFIGURATION, &IID_ID3D12_SDK_CONFIGURATION, &mut cfg) };
        if hr < 0 || cfg.is_null() {
            record("d3d12_xs", "agility", &format!("D3D12GetInterface(SDKConfiguration) hr={hr:#010x}"));
            return;
        }
        let mut dir = path.clone();
        if !dir.ends_with('\\') {
            dir.push('\\');
        }
        let cdir: Vec<u8> = dir.bytes().chain(std::iter::once(0)).collect();
        let hr = unsafe {
            let set: unsafe extern "system" fn(*mut c_void, u32, *const u8) -> i32 = core::mem::transmute(slot(cfg, 3));
            set(cfg, version, cdir.as_ptr())
        };
        record("d3d12_xs", "agility", &format!("SetSDKVersion({version}, {dir}) hr={hr:#010x}"));
        if hr < 0 {
            let hr = unsafe { make_factory(cfg, version, &cdir) };
            record("d3d12_xs", "agility", &format!("CreateDeviceFactory({version}, {dir}) hr={hr:#010x}"));
        }
        unsafe { release(cfg) };
    });
}
