use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
#[path = "../src/reset.rs"]
mod reset;
use reset::*;

extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

type D3D12CreateDeviceFn = unsafe extern "system" fn(*mut c_void, u32, *const Guid, *mut *mut c_void) -> i32;
type CreateManagerFn = unsafe extern "system" fn(*mut u32, *mut *mut c_void) -> i32;
type OpenHandleFn = unsafe extern "system" fn(*mut c_void, *mut usize) -> i32;
type CloseHandleFn = unsafe extern "system" fn(*mut c_void, usize) -> i32;
type GetServiceFn = unsafe extern "system" fn(*mut c_void, usize, *const Guid, *mut *mut c_void) -> i32;

#[repr(C)]
struct FakeWrapper {
    vtable: *const usize,
    real: *mut c_void,
    refs: AtomicU32,
}

unsafe extern "system" fn w_qi(this: *mut FakeWrapper, riid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    let g = unsafe { *riid };
    if g == IID_ID3D12DEVICE || g == IID_IUNKNOWN {
        unsafe {
            (*this).refs.fetch_add(1, Ordering::SeqCst);
            *ppv = this as *mut c_void;
        }
        return 0;
    }
    match unsafe { query((*this).real, &g) } {
        Ok(p) => {
            unsafe { *ppv = p };
            0
        }
        Err(hr) => {
            unsafe { *ppv = core::ptr::null_mut() };
            hr
        }
    }
}

unsafe extern "system" fn w_addref(this: *mut FakeWrapper) -> u32 {
    unsafe { (*this).refs.fetch_add(1, Ordering::SeqCst) + 1 }
}

unsafe extern "system" fn w_release(this: *mut FakeWrapper) -> u32 {
    unsafe { (*this).refs.fetch_sub(1, Ordering::SeqCst) - 1 }
}

fn sys(dll: &[u8], f: &[u8]) -> *mut c_void {
    let h = unsafe { LoadLibraryA(dll.as_ptr()) };
    assert!(!h.is_null(), "load {}", String::from_utf8_lossy(dll));
    let p = unsafe { GetProcAddress(h, f.as_ptr()) };
    assert!(!p.is_null(), "resolve {}", String::from_utf8_lossy(f));
    p
}

unsafe fn slot(obj: *mut c_void, i: usize) -> usize {
    unsafe { *(*(obj as *const *const usize)).add(i) }
}

#[test]
fn reset_binds_real_d3d12_device_behind_wrapper() {
    let create: D3D12CreateDeviceFn = unsafe { core::mem::transmute(sys(b"d3d12.dll\0", b"D3D12CreateDevice\0")) };
    let mut real: *mut c_void = core::ptr::null_mut();
    let hr = unsafe { create(core::ptr::null_mut(), 0xb000, &IID_ID3D12DEVICE, &mut real) };
    if hr < 0 || real.is_null() {
        eprintln!("no D3D12 device on this machine (hr={hr:#x}); skipping");
        return;
    }
    let vt: [usize; 3] = [w_qi as usize, w_addref as usize, w_release as usize];
    let mut wrapper = FakeWrapper { vtable: vt.as_ptr(), real, refs: AtomicU32::new(1) };
    let wp = &mut wrapper as *mut FakeWrapper as *mut c_void;

    let un = unsafe { unwrap_device(wp) }.expect("unwrap");
    assert_eq!(un.device, real);
    assert_ne!(un.device, wp);
    unsafe { release(un.device) };

    let mk: CreateManagerFn = unsafe { core::mem::transmute(sys(b"mfplat.dll\0", b"MFCreateDXGIDeviceManager\0")) };
    let mut token = 0u32;
    let mut mgr: *mut c_void = core::ptr::null_mut();
    assert!(unsafe { mk(&mut token, &mut mgr) } >= 0);

    let bad = unsafe { reset_device_manager(mgr, wp, token.wrapping_add(1)) };
    assert!(bad.hr < 0);

    let out = unsafe { reset_device_manager(mgr, wp, token) };
    assert_eq!(out.hr, 0, "ResetDevice hr={:#x}", out.hr);
    assert_eq!(out.real_device, real as usize);
    assert_eq!(wrapper.refs.load(Ordering::SeqCst), 1);

    let open: OpenHandleFn = unsafe { core::mem::transmute(slot(mgr, 6)) };
    let get: GetServiceFn = unsafe { core::mem::transmute(slot(mgr, 4)) };
    let close: CloseHandleFn = unsafe { core::mem::transmute(slot(mgr, 3)) };
    let mut h = 0usize;
    assert_eq!(unsafe { open(mgr, &mut h) }, 0);
    let mut dev: *mut c_void = core::ptr::null_mut();
    assert_eq!(unsafe { get(mgr, h, &IID_ID3D12DEVICE, &mut dev) }, 0);
    assert_eq!(dev, real);
    unsafe { release(dev) };
    unsafe { close(mgr, h) };

    let null = unsafe { reset_device_manager(mgr, core::ptr::null_mut(), token) };
    assert_eq!(null.hr, E_POINTER);
    unsafe { release(mgr) };
    unsafe { release(real) };
}

#[test]
fn guids_match_console_mfplat_and_d3d12() {
    assert_eq!(IID_ID3D12DEVICE, Guid { d1: 0x189819f1, d2: 0x1db6, d3: 0x4b57, d4: [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7] });
    assert_ne!(IID_ID3D12DEVICE9, IID_ID3D12DEVICE10);
}
