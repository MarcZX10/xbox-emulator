use compat_common::gdk::{self, xasync};
use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(C)]
struct Block {
    queue: usize,
    context: usize,
    callback: usize,
    internal: [u8; 32],
}

#[repr(C)]
struct Data {
    block: *mut Block,
    size: usize,
    buffer: *mut u8,
    context: usize,
}

unsafe fn slot(slot: usize) -> (*mut c_void, usize) {
    let mut obj: *mut c_void = core::ptr::null_mut();
    assert_eq!(gdk::query_api_impl(&xasync::XASYNC, &xasync::XASYNC, &mut obj), 0);
    let vt = *(obj as *const *const usize);
    (obj, *vt.add(slot))
}

static HELD: AtomicUsize = AtomicUsize::new(0);
static ORDER: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

unsafe extern "system" fn provider(op: u32, d: *const Data) -> i32 {
    match op {
        0 => {
            let (_, sched) = slot(9);
            let sched: unsafe extern "system" fn(*mut c_void, *mut Block, u32) -> i32 = core::mem::transmute(sched);
            sched(core::ptr::null_mut(), (*d).block, 0)
        }
        1 => {
            HELD.store(d as usize, Ordering::SeqCst);
            0x8000_000Au32 as i32
        }
        _ => 0,
    }
}

unsafe extern "system" fn canceled_task(_ctx: *mut c_void, canceled: bool) {
    ORDER.lock().unwrap().push(if canceled { "task canceled" } else { "task ran" });
}

unsafe extern "system" fn terminated(ctx: *mut c_void) {
    assert_eq!(ctx as usize, 0x77);
    ORDER.lock().unwrap().push("terminated");
}

#[test]
fn provider_data_outlives_the_provider_call_and_termination_is_dispatched() {
    std::env::set_var("TOMBOX_GDK_XASYNC", "emulated");
    std::env::set_var("TOMBOX_GDK_ROOT", std::env::temp_dir().join(format!("tombox_gdk_rtt_{}", std::process::id())));
    assert!(gdk::rt::active());
    unsafe {
        let (o, create) = slot(12);
        let create: unsafe extern "system" fn(*mut c_void, u32, u32, *mut usize) -> i32 = core::mem::transmute(create);
        let (_, begin) = slot(7);
        let begin: unsafe extern "system" fn(*mut c_void, *mut Block, usize, usize, *const u8, unsafe extern "system" fn(u32, *const Data) -> i32) -> i32 = core::mem::transmute(begin);
        let (_, complete) = slot(10);
        let complete: unsafe extern "system" fn(*mut c_void, *mut Block, i32, usize) = core::mem::transmute(complete);
        let (_, status) = slot(3);
        let status: unsafe extern "system" fn(*mut c_void, *mut Block, bool) -> i32 = core::mem::transmute(status);
        let (_, dispatch) = slot(16);
        let dispatch: unsafe extern "system" fn(*mut c_void, usize, u32, u32) -> bool = core::mem::transmute(dispatch);
        let (_, submit) = slot(18);
        let submit: unsafe extern "system" fn(*mut c_void, usize, u32, *mut c_void, unsafe extern "system" fn(*mut c_void, bool)) -> i32 = core::mem::transmute(submit);
        let (_, terminate) = slot(22);
        let terminate: unsafe extern "system" fn(*mut c_void, usize, bool, *mut c_void, unsafe extern "system" fn(*mut c_void)) -> i32 = core::mem::transmute(terminate);
        let (_, close) = slot(17);
        let close: unsafe extern "system" fn(*mut c_void, usize) = core::mem::transmute(close);

        let mut q = 0usize;
        assert_eq!(create(o, 1, 1, &mut q), 0);
        let mut b = Box::new(Block { queue: q, context: 0, callback: 0, internal: [0; 32] });
        assert_eq!(begin(o, &mut *b, 0x1234, 0, core::ptr::null(), provider), 0);
        let start = std::time::Instant::now();
        while HELD.load(Ordering::SeqCst) == 0 && start.elapsed().as_secs() < 5 {
            std::thread::yield_now();
        }
        let d = HELD.load(Ordering::SeqCst) as *const Data;
        assert!(!d.is_null());
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!((*d).context, 0x1234, "XAsyncProviderData must stay valid after the provider returns E_PENDING");
        assert_eq!((*d).block, &mut *b as *mut Block);
        complete(o, (*d).block, 0, 0);
        assert_eq!(status(o, &mut *b, true), 0);

        let mut m = 0usize;
        assert_eq!(create(o, 0, 0, &mut m), 0);
        assert_eq!(submit(o, m, 1, core::ptr::null_mut(), canceled_task), 0);
        assert_eq!(terminate(o, m, false, 0x77 as *mut c_void, terminated), 0);
        assert!(!ORDER.lock().unwrap().contains(&"terminated"), "termination callback ran before the queue was dispatched");
        for _ in 0..8 {
            dispatch(o, m, 0, 0);
            dispatch(o, m, 1, 0);
        }
        let order = ORDER.lock().unwrap().clone();
        assert_eq!(order.last(), Some(&"terminated"), "{order:?}");
        assert_eq!(order.iter().filter(|s| **s == "terminated").count(), 1, "{order:?}");
        assert!(order.contains(&"task canceled"), "{order:?}");
        close(o, m);
        close(o, q);
    }
}
