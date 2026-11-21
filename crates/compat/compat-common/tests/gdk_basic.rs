use compat_common::gdk::{self, basic, Guid};
use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

unsafe fn slot(c: Guid, i: Guid, n: usize) -> (*mut c_void, usize) {
    let mut obj: *mut c_void = core::ptr::null_mut();
    assert_eq!(gdk::query_api_impl(&c, &i, &mut obj), 0);
    let vt = *(obj as *const *const usize);
    (obj, *vt.add(n))
}

static SEEN_HR: AtomicU32 = AtomicU32::new(0);
static SEEN_CTX: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn on_error(hr: i32, msg: *const u8, ctx: *mut c_void) -> bool {
    assert_eq!(gdk::cstr(msg).as_deref(), Some("boom"));
    SEEN_HR.store(hr as u32, Ordering::SeqCst);
    SEEN_CTX.store(ctx as usize, Ordering::SeqCst);
    true
}

#[test]
fn xerror_feature_and_display_are_emulated() {
    assert!(gdk::FORWARDED.is_empty());
    unsafe {
        let (o, report) = slot(basic::XERROR, basic::XERROR, 3);
        let report: unsafe extern "system" fn(*mut c_void, i32, *const u8) = core::mem::transmute(report);
        let (_, set_cb) = slot(basic::XERROR, basic::XERROR, 4);
        let set_cb: unsafe extern "system" fn(*mut c_void, usize, usize) = core::mem::transmute(set_cb);
        let (_, set_opts) = slot(basic::XERROR, basic::XERROR, 5);
        let set_opts: unsafe extern "system" fn(*mut c_void, u32, u32) = core::mem::transmute(set_opts);
        assert_eq!(basic::error_options(), (1, 0));
        set_opts(o, 1, 0);
        let before = basic::error_reports();
        report(o, 0x8000_4005u32 as i32, b"no callback\0".as_ptr());
        set_cb(o, on_error as usize, 0x1234);
        report(o, 0x8007_0057u32 as i32, b"boom\0".as_ptr());
        assert_eq!(SEEN_HR.load(Ordering::SeqCst), 0x8007_0057);
        assert_eq!(SEEN_CTX.load(Ordering::SeqCst), 0x1234);
        assert_eq!(basic::error_reports(), before + 2);
        set_cb(o, 0, 0);

        let (o, feat) = slot(basic::FEATURE, basic::FEATURE, 3);
        let feat: unsafe extern "system" fn(*mut c_void, u32) -> bool = core::mem::transmute(feat);
        for f in 0..21 {
            assert!(feat(o, f), "feature {f}");
        }
        assert!(!feat(o, 21));
        assert!(!feat(o, 22));
        assert!(!feat(o, 1000));

        let (o, hdr) = slot(basic::DISPLAY, basic::DISPLAY_IID, 4);
        let hdr: unsafe extern "system" fn(*mut c_void, u32, *mut f32) -> u32 = core::mem::transmute(hdr);
        let mut info = [9.0f32; 4];
        let r = hdr(o, 0, info.as_mut_ptr());
        let st = basic::hdr_state().unwrap();
        assert_eq!(r, if st.enabled { 1 } else { 2 });
        if st.enabled {
            assert_eq!(info[1], st.max);
        } else {
            assert_eq!(&info[..3], &[0.0, 0.0, 0.0]);
        }
        assert_eq!(info[3], 9.0);
        assert_eq!(hdr(o, 7, info.as_mut_ptr()), 0);
        let (_, hdr1) = slot(basic::DISPLAY, basic::DISPLAY_IID, 3);
        let hdr1: unsafe extern "system" fn(*mut c_void, *mut f32) -> u32 = core::mem::transmute(hdr1);
        assert_eq!(hdr1(o, core::ptr::null_mut()), r);
    }
}

#[test]
fn xasync_is_emulated_by_default() {
    assert!(gdk::rt::active());
}
