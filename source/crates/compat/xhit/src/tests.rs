use super::*;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

#[test]
fn xhit_initialize_without_kmd_returns_zero() {
    let _g = LOCK.lock().unwrap();
    assert_eq!(unsafe { XhitInitialize(0) }, 0);
    assert!(xhit_s::active());
    assert!(xhit_s::regions().is_none());
}

#[test]
fn xhit_terminate_clears_active() {
    let _g = LOCK.lock().unwrap();
    unsafe { XhitInitialize(0) };
    assert_eq!(unsafe { XhitTerminate() }, 0);
    assert!(!xhit_s::active());
}

#[test]
fn xhit_summary_null_context_is_unexpected() {
    let _g = LOCK.lock().unwrap();
    let mut out = vec![u64::MAX; 0x320 / 8];
    let hr = unsafe { XhitPerformHangSummary(core::ptr::null(), out.as_mut_ptr()) };
    assert_eq!(hr, xhit_s::E_UNEXPECTED);
    assert_eq!(out[0] & xhit_s::DEADLOCK_BIT, 0);
    assert_eq!(out[0x318 / 8], 0);
}

#[test]
fn xhit_analysis_fails_without_hang_drive() {
    let _g = LOCK.lock().unwrap();
    let mut name = [0u16; 260];
    let hr = unsafe { XhitPerformHangAnalysis(0, name.as_mut_ptr(), 260, core::ptr::null()) };
    assert_eq!(hr, xhit_s::E_FAIL);
}
