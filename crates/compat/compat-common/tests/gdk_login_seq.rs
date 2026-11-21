use compat_common::gdk::{self, store, user, xasync};
use core::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

#[repr(C)]
struct Block {
    queue: usize,
    context: usize,
    callback: usize,
    internal: [u8; 32],
}

unsafe fn m(c: gdk::Guid, i: gdk::Guid, n: usize) -> (*mut c_void, usize) {
    let mut o = core::ptr::null_mut();
    assert_eq!(gdk::query_api_impl(&c, &i, &mut o), 0);
    (o, *(*(o as *const *const usize)).add(n))
}

static COUNT_HR: AtomicI32 = AtomicI32::new(1);
static COUNT: AtomicU32 = AtomicU32::new(99);
static STATUS_IN_CB: AtomicI32 = AtomicI32::new(1);

unsafe extern "system" fn on_done(b: *mut Block) {
    let (a, status) = m(xasync::XASYNC, xasync::XASYNC, 3);
    let status: unsafe extern "system" fn(*mut c_void, *mut Block, bool) -> i32 = core::mem::transmute(status);
    STATUS_IN_CB.store(status(a, b, false), Ordering::SeqCst);
    let (s, count) = m(store::CLSID, store::IID, 56);
    let count: unsafe extern "system" fn(*mut c_void, *mut Block, *mut u32) -> i32 = core::mem::transmute(count);
    let mut n = 0u32;
    COUNT_HR.store(count(s, b, &mut n), Ordering::SeqCst);
    COUNT.store(n, Ordering::SeqCst);
}

unsafe fn sequence(queue: usize, dispatch: Option<(*mut c_void, usize)>) -> (i32, u32, i32, i32) {
    COUNT_HR.store(1, Ordering::SeqCst);
    STATUS_IN_CB.store(1, Ordering::SeqCst);
    let (u, add) = m(user::CLSID, user::IID_V2, 7);
    let add: unsafe extern "system" fn(*mut c_void, u32, *mut Block) -> i32 = core::mem::transmute(add);
    let (_, add_result) = m(user::CLSID, user::IID_V2, 8);
    let add_result: unsafe extern "system" fn(*mut c_void, *mut Block, *mut usize) -> i32 = core::mem::transmute(add_result);
    let (a, status) = m(xasync::XASYNC, xasync::XASYNC, 3);
    let status: unsafe extern "system" fn(*mut c_void, *mut Block, bool) -> i32 = core::mem::transmute(status);
    let mut ub = Box::new(Block { queue, context: 0, callback: 0, internal: [0; 32] });
    assert_eq!(add(u, 1, &mut *ub), 0);
    if let Some((q, f)) = dispatch {
        let f: unsafe extern "system" fn(*mut c_void, usize, u32, u32) -> bool = core::mem::transmute(f);
        for _ in 0..50 {
            f(q, queue, 0, 10);
        }
    }
    assert_eq!(status(a, &mut *ub, true), 0);
    let mut h = 0usize;
    assert_eq!(add_result(u, &mut *ub, &mut h), 0);
    let (_, check) = m(user::CLSID, user::IID_V2, 20);
    let check: unsafe extern "system" fn(*mut c_void, usize, u32, u32, *mut bool, *mut u32) -> i32 = core::mem::transmute(check);
    let (mut has, mut why) = (false, 0u32);
    assert_eq!(check(u, h, 0, 254, &mut has, &mut why), 0);
    assert!(has);
    let (s, ctx) = m(store::CLSID, store::IID, 3);
    let ctx: unsafe extern "system" fn(*mut c_void, usize, *mut usize) -> i32 = core::mem::transmute(ctx);
    let mut sc = 0usize;
    assert_eq!(ctx(s, h, &mut sc), 0);
    let (_, upd) = m(store::CLSID, store::IID, 55);
    let upd: unsafe extern "system" fn(*mut c_void, usize, *mut Block) -> i32 = core::mem::transmute(upd);
    let mut b = Box::new(Block { queue, context: 0, callback: on_done as usize, internal: [0; 32] });
    let start = upd(s, sc, &mut *b);
    if let Some((q, f)) = dispatch {
        let f: unsafe extern "system" fn(*mut c_void, usize, u32, u32) -> bool = core::mem::transmute(f);
        for _ in 0..50 {
            f(q, queue, 0, 10);
            f(q, queue, 1, 10);
        }
    }
    let st = status(a, &mut *b, true);
    for _ in 0..200 {
        if COUNT_HR.load(Ordering::SeqCst) != 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let _ = start;
    (st, COUNT.load(Ordering::SeqCst), COUNT_HR.load(Ordering::SeqCst), STATUS_IN_CB.load(Ordering::SeqCst))
}

#[test]
fn privilege_then_package_updates_complete_like_console() {
    std::env::set_var("TOMBOX_GDK_ROOT", std::env::temp_dir().join(format!("tombox_login_seq_{}", std::process::id())));
    unsafe {
        let r = sequence(0, None);
        eprintln!("null queue: {r:x?}");
        assert_eq!(r, (0, 0, 0, 0));
        let (a, create) = m(xasync::XASYNC, xasync::XASYNC, 12);
        let create: unsafe extern "system" fn(*mut c_void, u32, u32, *mut usize) -> i32 = core::mem::transmute(create);
        for (w, c) in [(1u32, 1u32), (1, 0), (2, 0), (0, 0), (3, 3)] {
            let mut q = 0usize;
            assert_eq!(create(a, w, c, &mut q), 0);
            let (_, disp) = m(xasync::XASYNC, xasync::XASYNC, 16);
            let r = sequence(q, Some((a, disp)));
            eprintln!("queue {w}/{c}: {r:x?}");
            assert_eq!(r, (0, 0, 0, 0), "queue modes {w}/{c}");
        }
    }
}
