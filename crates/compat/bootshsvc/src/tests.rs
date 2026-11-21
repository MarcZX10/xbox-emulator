use super::*;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn push_stores_and_replaces_the_globals_pointer() {
    let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut a = [0u8; 0xe0];
    let mut b = [0u8; 0xe0];
    SvchostPushServiceGlobals(a.as_mut_ptr() as *mut c_void);
    assert_eq!(svchost_globals(), a.as_mut_ptr() as *mut c_void);
    SvchostPushServiceGlobals(b.as_mut_ptr() as *mut c_void);
    assert_eq!(svchost_globals(), b.as_mut_ptr() as *mut c_void);
    SvchostPushServiceGlobals(core::ptr::null_mut());
    assert!(svchost_globals().is_null());
    assert_eq!(a[0], 0);
}

#[test]
fn service_main_runs_console_19041_bootsh_against_sandbox() {
    let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    crate::sandbox_test::run_console_bootsh(r"D:\xbox-pc-port\games\Fortnite\extracted\EmbeddedXvd\Windows\System32\bootshsvc.dll");
}