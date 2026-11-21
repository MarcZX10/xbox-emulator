use crate::scale::*;
use core::ffi::c_void;
use core::ptr::null_mut;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::time::{Duration, Instant};

#[link(name = "user32")]
extern "system" {
    fn PostMessageW(h: isize, m: u32, w: usize, l: isize) -> i32;
}

const IID_IUNKNOWN: Guid = Guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;

#[repr(C)]
struct Handler {
    vtbl: *const HVtbl,
    refs: AtomicU32,
    agile: bool,
    with_iface: bool,
    calls: AtomicU32,
    last: AtomicI32,
}

#[repr(C)]
struct HVtbl {
    qi: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    invoke: unsafe extern "system" fn(*mut c_void, i32) -> i32,
}

unsafe extern "system" fn qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    let h = &*(this as *const Handler);
    let ok = *iid == IID_IUNKNOWN || (*iid == IID_IAGILEOBJECT && h.agile) || (*iid == IID_SCALE_HANDLER && h.with_iface);
    if !ok {
        *out = null_mut();
        return E_NOINTERFACE;
    }
    h.refs.fetch_add(1, Ordering::SeqCst);
    *out = this;
    0
}

unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    (*(this as *const Handler)).refs.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    (*(this as *const Handler)).refs.fetch_sub(1, Ordering::SeqCst) - 1
}

unsafe extern "system" fn invoke(this: *mut c_void, scale: i32) -> i32 {
    let h = &*(this as *const Handler);
    h.last.store(scale, Ordering::SeqCst);
    h.calls.fetch_add(1, Ordering::SeqCst);
    0
}

static VT: HVtbl = HVtbl { qi, add_ref, release, invoke };

fn handler(agile: bool, with_iface: bool) -> Box<Handler> {
    Box::new(Handler { vtbl: &VT, refs: AtomicU32::new(1), agile, with_iface, calls: AtomicU32::new(0), last: AtomicI32::new(-1) })
}

fn ptr(h: &Handler) -> *mut c_void {
    h as *const Handler as *mut c_void
}

fn serial() -> std::sync::MutexGuard<'static, ()> {
    static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

fn wait_calls(h: &Handler, n: u32) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if h.calls.load(Ordering::SeqCst) >= n {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn agile_handler_registers_is_notified_on_scale_change_and_revokes() {
    let _s = serial();
    let h = handler(true, true);
    let mut cookie = 0xdead;
    assert_eq!(unsafe { register(0, ptr(&h), &mut cookie) }, 0);
    assert_ne!(cookie, 0);
    assert_eq!(h.refs.load(Ordering::SeqCst), 2);
    let hw = sink_hwnd();
    let msg = message_for(0, cookie).unwrap();
    let want = unsafe { (host().unwrap().scale)(0) };
    unsafe { PostMessageW(hw, msg, 2, 0) };
    unsafe { PostMessageW(hw, msg, 1, 0) };
    assert!(wait_calls(&h, 1));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.last.load(Ordering::SeqCst), want);
    assert_eq!(unsafe { revoke(0, cookie) }, 0);
    assert_eq!(h.refs.load(Ordering::SeqCst), 1);
    assert!(message_for(0, cookie).is_none());
    assert_eq!(unsafe { revoke(0, cookie) }, E_INVALIDARG);
}

#[test]
fn cookies_share_the_pc_window_registration_counter() {
    let _s = serial();
    let h = handler(true, true);
    let host = host().unwrap();
    let hw = sink_hwnd();
    let mut a = 0;
    let mut w = 0;
    let mut b = 0;
    unsafe {
        assert_eq!(register(1, ptr(&h), &mut a), 0);
        assert_eq!((host.register)(1, hw, WM_APP + MSG_SPAN, &mut w), 0);
        assert_eq!(register(1, ptr(&h), &mut b), 0);
        assert!(a != w && w != b && a != b);
        assert_eq!((host.revoke)(1, w), 0);
        assert_eq!(revoke(1, a), 0);
        assert_eq!(revoke(1, b), 0);
    }
    assert_eq!(h.refs.load(Ordering::SeqCst), 1);
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(r: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
}

#[test]
fn non_agile_handler_without_com_on_the_caller_returns_co_e_notinitialized() {
    let _s = serial();
    let h = handler(false, true);
    let p = ptr(&h) as usize;
    let (hr, cookie) = std::thread::spawn(move || {
        let mut c = 9;
        (unsafe { register(0, p as *mut c_void, &mut c) }, c)
    })
    .join()
    .unwrap();
    assert_eq!(hr, 0x8004_01f0u32 as i32);
    assert_eq!(cookie, 0);
    assert_eq!(h.refs.load(Ordering::SeqCst), 1);
}

#[test]
fn non_agile_handler_goes_through_an_agile_reference() {
    let _s = serial();
    let h = handler(false, true);
    let mut cookie = 0;
    assert!(unsafe { CoInitializeEx(null_mut(), 0) } >= 0);
    assert_eq!(unsafe { register(2, ptr(&h), &mut cookie) }, 0);
    let msg = message_for(2, cookie).unwrap();
    let want = unsafe { (host().unwrap().scale)(2) };
    unsafe { PostMessageW(sink_hwnd(), msg, 1, 0) };
    assert!(wait_calls(&h, 1));
    assert_eq!(h.last.load(Ordering::SeqCst), want);
    assert_eq!(unsafe { revoke(2, cookie) }, 0);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.refs.load(Ordering::SeqCst), 1);
    unsafe { CoUninitialize() };
}

#[test]
fn non_agile_handler_without_the_handler_interface_is_refused() {
    let _s = serial();
    let h = handler(false, false);
    let mut cookie = 7;
    assert_eq!(unsafe { register(0, ptr(&h), &mut cookie) }, E_NOINTERFACE);
    assert_eq!(cookie, 0);
    assert_eq!(h.refs.load(Ordering::SeqCst), 1);
}

#[test]
fn bad_kind_and_null_handler_are_invalid() {
    let _s = serial();
    let h = handler(true, true);
    let mut cookie = 7;
    assert_eq!(unsafe { register(3, ptr(&h), &mut cookie) }, E_INVALIDARG);
    assert_eq!(cookie, 0);
    assert_eq!(unsafe { register(0, null_mut(), &mut cookie) }, E_INVALIDARG);
    assert_eq!(h.refs.load(Ordering::SeqCst), 1);
}
