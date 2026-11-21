use super::*;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(r: *mut c_void, model: u32) -> i32;
}

fn load_xmem() -> isize {
    crate::diagveh::install();
    let exe = std::env::current_exe().unwrap();
    let dll = exe.ancestors().map(|d| d.join("xmem.dll")).find(|p| p.is_file());
    let dll = dll.unwrap_or_else(|| panic!("build the xmem crate first (no xmem.dll above {})", exe.display()));
    let path = wide(dll.to_str().unwrap());
    let m = unsafe { LoadLibraryW(path.as_ptr()) };
    assert_ne!(m, 0);
    m
}

static SEEN_SIZE: AtomicUsize = AtomicUsize::new(0);
static SEEN_ATTRS: AtomicU64 = AtomicU64::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);
static mut ARENA: [u8; 256] = [0; 256];

unsafe extern "system" fn hook_alloc(size: usize, attrs: u64) -> *mut c_void {
    SEEN_SIZE.store(size, Ordering::SeqCst);
    SEEN_ATTRS.store(attrs, Ordering::SeqCst);
    core::ptr::addr_of_mut!(ARENA) as *mut c_void
}

unsafe extern "system" fn hook_free(p: *mut c_void, attrs: u64) {
    FREED.store(p as usize, Ordering::SeqCst);
    SEEN_ATTRS.store(attrs, Ordering::SeqCst);
}

#[test]
fn dll_main_returns_true_for_every_reason() {
    let me = unsafe { GetModuleHandleW(core::ptr::null()) };
    for r in 0..4 {
        assert_eq!(unsafe { DllMain(me, r, core::ptr::null_mut()) }, 1);
    }
}

#[test]
fn default_processor_is_processor6() {
    assert_eq!(GetXAudioDefaultProcessorForCurrentPlatform(), 0x20);
}

#[test]
fn affinity_mask_change_returns_s_ok() {
    for m in [0u64, 1, 0x30, u64::MAX] {
        assert_eq!(OnXAudioThreadAffinityMaskChanged(m), 0);
    }
}

#[test]
fn get_xaudio_heap_returns_null_heap() {
    let mut h = 0x1234usize as *mut c_void;
    assert_eq!(unsafe { GetXAudioHeap(&mut h) }, 0);
    assert!(h.is_null());
}

#[test]
fn heap_attributes_match_console_builder() {
    assert_eq!(XAUDIO_HEAP_ATTRIBUTES, 0x0481_0000);
    assert_eq!(xalloc_attributes(0x81, 0, 0, 0, 0, 0), 0x0081_0000);
    assert_eq!(xalloc_attributes(0x1ff, 0xffff, 0x1f, 7, 0x3f, 3), 0x3_ffff_ffff);
    assert_eq!(xalloc_attributes(0, 0, 1, 0, 0, 0), 0x2000_0000);
    assert_eq!(xalloc_attributes(0, 0, 0, 1, 0, 0), 0x4000);
    assert_eq!(xalloc_attributes(0, 0, 0, 0, 0, 1), 0x2_0000_0000);
}

#[test]
fn xaudio_heap_alloc_and_free_go_through_xmem() {
    let _g = crate::acp_tests::serial();
    let m = load_xmem();
    unsafe {
        let real = XAudioHeapAlloc(core::ptr::null_mut(), 100);
        assert!(!real.is_null());
        (real as *mut u8).write_bytes(0x5a, 100);
        assert_eq!(XAudioHeapFree(core::ptr::null_mut(), real), 1);
        let set = GetProcAddress(m, b"XMemSetAllocationHooks\0".as_ptr());
        assert!(!set.is_null());
        let set: unsafe extern "system" fn(usize, usize) = core::mem::transmute(set);
        set(hook_alloc as usize, hook_free as usize);
        let p = XAudioHeapAlloc(0x77 as *mut c_void, 48);
        let seen = (SEEN_SIZE.load(Ordering::SeqCst), SEEN_ATTRS.load(Ordering::SeqCst));
        SEEN_ATTRS.store(0, Ordering::SeqCst);
        let r = XAudioHeapFree(0x77 as *mut c_void, p);
        let freed = (FREED.load(Ordering::SeqCst), SEEN_ATTRS.load(Ordering::SeqCst));
        set(0, 0);
        assert_eq!(p, core::ptr::addr_of_mut!(ARENA) as *mut c_void);
        assert_eq!(seen, (48, 0x0481_0000));
        assert_eq!(r, 1);
        assert_eq!(freed, (p as usize, 0x0481_0000));
    }
}

#[test]
fn set_debug_configuration_copies_into_state() {
    let mut state = vec![0u8; 0x3c0];
    let cfg: Vec<u8> = (1..=0x18).collect();
    unsafe {
        SetXAudio2DebugConfiguration(state.as_mut_ptr(), cfg.as_ptr(), core::ptr::null_mut());
        assert_eq!(&state[0x390..0x3a8], &cfg[..]);
        assert!(state[..0x390].iter().all(|&b| b == 0));
        assert!(state[0x3a8..].iter().all(|&b| b == 0));
        let mut other = vec![0u8; 0x3c0];
        SetXAudio2DebugConfiguration(other.as_mut_ptr(), core::ptr::null(), core::ptr::null_mut());
        assert!(other.iter().all(|&b| b == 0));
        SetXAudio2DebugConfiguration(core::ptr::null_mut(), cfg.as_ptr(), core::ptr::null_mut());
    }
}

unsafe fn first_render_endpoint() -> Option<Vec<u16>> {
    let mut e: *mut c_void = core::ptr::null_mut();
    if CoCreateInstance(&CLSID_MM_DEVICE_ENUMERATOR, core::ptr::null_mut(), CLSCTX_ALL, &IID_IMM_DEVICE_ENUMERATOR, &mut e) < 0 {
        return None;
    }
    let mut c: *mut c_void = core::ptr::null_mut();
    let f: unsafe extern "system" fn(*mut c_void, u32, u32, *mut *mut c_void) -> i32 = core::mem::transmute(slot(e, 3));
    assert_eq!(f(e, E_RENDER, 1, &mut c), 0);
    let mut n = 0u32;
    let f: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32 = core::mem::transmute(slot(c, 3));
    assert_eq!(f(c, &mut n), 0);
    let mut out = None;
    if n > 0 {
        let mut d: *mut c_void = core::ptr::null_mut();
        let f: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 = core::mem::transmute(slot(c, 4));
        assert_eq!(f(c, 0, &mut d), 0);
        let mut id: *mut u16 = core::ptr::null_mut();
        let f: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32 = core::mem::transmute(slot(d, 5));
        assert_eq!(f(d, &mut id), 0);
        out = Some(core::slice::from_raw_parts(id, wcslen(id) + 1).to_vec());
        CoTaskMemFree(id as *mut c_void);
        release(d);
    }
    release(c);
    release(e);
    out
}

#[test]
fn endpoint_lookup_maps_interface_path_to_endpoint_id() {
    crate::diagveh::install();
    unsafe {
        CoInitializeEx(core::ptr::null_mut(), 0);
        let mut out = 0x99usize as *mut u16;
        let unknown = wide("\\\\?\\SWD#MMDEVAPI#{0.0.0.00000000}.{00000000-0000-0000-0000-000000000000}#{e6327cad-dcec-4949-ae8a-991e976a79d2}");
        assert_eq!(GetAudioEndpointIdFromDeviceInterfaceId(unknown.as_ptr(), &mut out), 0);
        assert!(out.is_null());
        if let Some(id) = first_render_endpoint() {
            let s = String::from_utf16_lossy(&id[..id.len() - 1]);
            let path = wide(&format!("\\\\?\\SWD#MMDEVAPI#{}#{{E6327CAD-DCEC-4949-AE8A-991E976A79D2}}", s.to_uppercase()));
            assert_eq!(GetAudioEndpointIdFromDeviceInterfaceId(path.as_ptr(), &mut out), 0);
            assert!(!out.is_null());
            assert_eq!(core::slice::from_raw_parts(out, id.len()), &id[..]);
            CoTaskMemFree(out as *mut c_void);
            let own = endpoint_interface_id(id.as_ptr()).unwrap();
            assert_eq!(CompareStringOrdinal(own.as_ptr(), -1, path.as_ptr(), -1, 1), CSTR_EQUAL);
        }
    }
}
