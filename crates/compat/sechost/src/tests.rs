use super::*;
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use std::sync::Mutex as TestMutex;

static SERIAL: TestMutex<()> = TestMutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0x100);
static CLOSED: AtomicU32 = AtomicU32::new(0);
static PROCESSED: AtomicU32 = AtomicU32::new(0);

fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    table().clear();
    CLOSED.store(0, Ordering::SeqCst);
    PROCESSED.store(0, Ordering::SeqCst);
    g
}

unsafe extern "system" fn fake_open(logfile: *mut c_void) -> u64 {
    if logfile.is_null() {
        INVALID_PROCESSTRACE_HANDLE
    } else {
        NEXT.fetch_add(1, Ordering::SeqCst)
    }
}

unsafe extern "system" fn fake_close(_h: u64) -> u32 {
    CLOSED.fetch_add(1, Ordering::SeqCst);
    0
}

unsafe extern "system" fn fake_process(h: *const u64, _n: u32, _s: *const c_void, _e: *const c_void) -> u32 {
    PROCESSED.fetch_add(1, Ordering::SeqCst);
    let mut t = table();
    let st = find(&mut t, *h).map(|c| c.state);
    drop(t);
    assert_eq!(st, Some(STATE_PROCESSING));
    if CLOSED.load(Ordering::SeqCst) == 99 {
        assert_eq!(close_step(*h), CloseStep::Pending);
    }
    0
}

fn logfile(mode: u32) -> Vec<u8> {
    let mut v = vec![0u8; 0x1c0];
    v[0x1c..0x20].copy_from_slice(&mode.to_le_bytes());
    v
}

fn open(mode: u32) -> u64 {
    let mut lf = logfile(mode);
    unsafe { open_with(fake_open, lf.as_mut_ptr() as *mut c_void) }
}

#[test]
fn query_rejects_zero_and_invalid_handles_before_pointers() {
    let _g = fresh();
    let (mut a, mut b) = (5u32, 5u32);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(0, &mut a, &mut b) }, 6);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(u64::MAX, core::ptr::null_mut(), core::ptr::null_mut()) }, 6);
    assert_eq!((a, b), (5, 5));
}

#[test]
fn query_null_outputs_give_invalid_parameter() {
    let _g = fresh();
    let h = open(PROCESS_TRACE_MODE_REAL_TIME);
    let mut a = 0u32;
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(h, &mut a, core::ptr::null_mut()) }, 87);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(h, core::ptr::null_mut(), &mut a) }, 87);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(12345, core::ptr::null_mut(), &mut a) }, 87);
}

#[test]
fn query_realtime_consumer_returns_counters() {
    let _g = fresh();
    let h = open(PROCESS_TRACE_MODE_REAL_TIME | 0x1000);
    {
        let mut t = table();
        let c = find(&mut t, h).unwrap();
        c.events_lost = 17;
        c.buffers_lost = 3;
    }
    let (mut a, mut b) = (0u32, 0u32);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(h, &mut a, &mut b) }, 0);
    assert_eq!((a, b), (17, 3));
    let h2 = open(PROCESS_TRACE_MODE_REAL_TIME);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(h2, &mut a, &mut b) }, 0);
    assert_eq!((a, b), (0, 0));
}

#[test]
fn query_file_mode_and_unknown_handles_give_invalid_handle() {
    let _g = fresh();
    let h = open(0x1000);
    let (mut a, mut b) = (9u32, 9u32);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(h, &mut a, &mut b) }, 6);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(h + 1000, &mut a, &mut b) }, 6);
    assert_eq!((a, b), (9, 9));
}

#[test]
fn open_failure_registers_nothing() {
    let _g = fresh();
    assert_eq!(unsafe { open_with(fake_open, core::ptr::null_mut()) }, INVALID_PROCESSTRACE_HANDLE);
    assert!(table().is_empty());
}

#[test]
fn close_removes_consumer_and_rejects_bad_handles() {
    let _g = fresh();
    let h = open(PROCESS_TRACE_MODE_REAL_TIME);
    assert_eq!(unsafe { close_with(fake_close, 0) }, 6);
    assert_eq!(unsafe { close_with(fake_close, u64::MAX) }, 6);
    assert_eq!(CLOSED.load(Ordering::SeqCst), 0);
    assert_eq!(unsafe { close_with(fake_close, h) }, 0);
    assert_eq!(CLOSED.load(Ordering::SeqCst), 1);
    let (mut a, mut b) = (0u32, 0u32);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(h, &mut a, &mut b) }, 6);
}

#[test]
fn close_during_processing_is_pending_then_freed_by_process() {
    let _g = fresh();
    let h = open(PROCESS_TRACE_MODE_REAL_TIME);
    assert!(matches!(begin_processing(&[h]), Ok(ref k) if k == &[h]));
    assert_eq!(unsafe { close_with(fake_close, h) }, 7007);
    assert_eq!(unsafe { close_with(fake_close, h) }, 6);
    let (mut a, mut b) = (1u32, 1u32);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(h, &mut a, &mut b) }, 0);
    assert_eq!(begin_processing(&[h]), Err(6));
    end_processing(&[h]);
    assert_eq!(unsafe { EtwQueryRealtimeConsumer(h, &mut a, &mut b) }, 6);
}

#[test]
fn process_marks_and_restores_state() {
    let _g = fresh();
    let h = open(PROCESS_TRACE_MODE_REAL_TIME);
    let hs = [h];
    assert_eq!(unsafe { process_with(fake_process, hs.as_ptr(), 1, core::ptr::null(), core::ptr::null()) }, 0);
    assert_eq!(PROCESSED.load(Ordering::SeqCst), 1);
    assert_eq!(find(&mut table(), h).map(|c| c.state), Some(STATE_IDLE));
    CLOSED.store(99, Ordering::SeqCst);
    assert_eq!(unsafe { process_with(fake_process, hs.as_ptr(), 1, core::ptr::null(), core::ptr::null()) }, 0);
    assert!(find(&mut table(), h).is_none());
}

#[test]
fn pc_open_of_missing_file_fails_without_registering() {
    let _g = fresh();
    let path: Vec<u16> = "Z:\\tombox-no-such-dir\\none.etl".encode_utf16().chain(Some(0)).collect();
    let mut lf = logfile(0x1000);
    lf[0..8].copy_from_slice(&(path.as_ptr() as u64).to_le_bytes());
    unsafe extern "system" fn cb(_e: *mut c_void) {}
    lf[0x1a8..0x1b0].copy_from_slice(&(cb as usize as u64).to_le_bytes());
    let h = unsafe { OpenTraceW(lf.as_mut_ptr() as *mut c_void) };
    assert_eq!(h, INVALID_PROCESSTRACE_HANDLE);
    assert!(table().is_empty());
}
