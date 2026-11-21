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

fn block(queue: usize) -> Box<Block> {
    Box::new(Block { queue, context: 0, callback: 0, internal: [0; 32] })
}

unsafe fn slot(slot: usize) -> (*mut c_void, usize) {
    let mut obj: *mut c_void = core::ptr::null_mut();
    assert_eq!(gdk::query_api_impl(&xasync::XASYNC, &xasync::XASYNC, &mut obj), 0);
    let vt = *(obj as *const *const usize);
    (obj, *vt.add(slot))
}

static HITS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn task(ctx: *mut c_void, canceled: bool) {
    if !canceled {
        HITS.fetch_add(ctx as usize, Ordering::SeqCst);
    }
}

static COMPLETIONS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn done(b: *mut Block) {
    COMPLETIONS.fetch_add((*b).context, Ordering::SeqCst);
}

#[test]
fn emulated_task_queue_and_async_provider_semantics() {
    std::env::set_var("TOMBOX_GDK_XASYNC", "emulated");
    std::env::set_var("TOMBOX_GDK_ROOT", std::env::temp_dir().join(format!("tombox_gdk_rt_{}", std::process::id())));
    assert!(gdk::rt::active());
    unsafe {
        let (o, create) = slot(12);
        let create: unsafe extern "system" fn(*mut c_void, u32, u32, *mut usize) -> i32 = core::mem::transmute(create);
        let (_, submit) = slot(18);
        let submit: unsafe extern "system" fn(*mut c_void, usize, u32, *mut c_void, unsafe extern "system" fn(*mut c_void, bool)) -> i32 = core::mem::transmute(submit);
        let (_, dispatch) = slot(16);
        let dispatch: unsafe extern "system" fn(*mut c_void, usize, u32, u32) -> bool = core::mem::transmute(dispatch);
        let (_, close) = slot(17);
        let close: unsafe extern "system" fn(*mut c_void, usize) = core::mem::transmute(close);
        let (_, process) = slot(25);
        let process: unsafe extern "system" fn(*mut c_void, *mut usize) -> bool = core::mem::transmute(process);

        let mut q = 0usize;
        assert_eq!(create(o, 0, 0, &mut q), 0);
        assert_ne!(q, 0);
        assert_eq!(submit(o, q, 0, 5 as *mut c_void, task), 0);
        assert_eq!(HITS.load(Ordering::SeqCst), 0, "manual port ran a callback without dispatch");
        assert!(dispatch(o, q, 0, 0));
        assert_eq!(HITS.load(Ordering::SeqCst), 5);
        assert!(!dispatch(o, q, 0, 0));

        let mut pq = 0usize;
        assert!(process(o, &mut pq));
        assert_ne!(pq, 0);
        assert_eq!(submit(o, 0, 0, 7 as *mut c_void, task), 0);
        let start = std::time::Instant::now();
        while HITS.load(Ordering::SeqCst) != 12 && start.elapsed().as_secs() < 5 {
            std::thread::yield_now();
        }
        assert_eq!(HITS.load(Ordering::SeqCst), 12, "null queue did not run on the process queue");

        let (so, lic) = {
            let mut obj: *mut c_void = core::ptr::null_mut();
            assert_eq!(gdk::query_api_impl(&gdk::store::CLSID, &gdk::store::IID, &mut obj), 0);
            let vt = *(obj as *const *const usize);
            (obj, (*vt.add(28), *vt.add(29)))
        };
        let begin: unsafe extern "system" fn(*mut c_void, usize, *mut Block) -> i32 = core::mem::transmute(lic.0);
        let result: unsafe extern "system" fn(*mut c_void, *mut Block, *mut u8) -> i32 = core::mem::transmute(lic.1);
        let mut b = block(q);
        b.context = 3;
        b.callback = done as usize;
        assert_eq!(begin(so, 0, &mut *b), 0);
        let mut spins = 0;
        while xasync::get_status(&mut *b as *mut Block as *mut c_void, false) == 0x8000_000Au32 as i32 && spins < 200 {
            dispatch(o, q, 0, 10);
            dispatch(o, q, 1, 10);
            spins += 1;
        }
        assert_eq!(xasync::get_status(&mut *b as *mut Block as *mut c_void, true), 0);
        while COMPLETIONS.load(Ordering::SeqCst) == 0 && spins < 400 {
            dispatch(o, q, 1, 10);
            spins += 1;
        }
        assert_eq!(COMPLETIONS.load(Ordering::SeqCst), 3, "completion callback not delivered on the completion port");
        let mut lic_bytes = [0u8; 104];
        assert_eq!(result(so, &mut *b, lic_bytes.as_mut_ptr()), 0);
        assert_eq!(lic_bytes[18], 1);
        assert_eq!(&lic_bytes[..], &gdk::store::game_license_bytes()[..]);
        assert_ne!(result(so, &mut *b, lic_bytes.as_mut_ptr()), 0, "result was readable twice");

        close(o, q);
    }
}
