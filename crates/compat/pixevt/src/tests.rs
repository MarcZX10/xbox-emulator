use super::*;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

type F0 = unsafe extern "system" fn() -> i32;
type F2 = unsafe extern "system" fn(usize, usize) -> i32;
type F5 = unsafe extern "system" fn(usize, usize, usize, usize, usize) -> i32;
type V2 = unsafe extern "system" fn(usize, usize);

unsafe extern "system" fn add2(a: usize, b: usize) -> i32 {
    (a * 10 + b) as i32
}

unsafe extern "system" fn add5(a: usize, b: usize, c: usize, d: usize, e: usize) -> i32 {
    (a + b * 2 + c * 3 + d * 4 + e * 5) as i32
}

static SEEN: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn rec2(a: usize, b: usize) {
    SEEN.store(a * 100 + b, Ordering::SeqCst);
}

fn clear() {
    for i in 0..SLOTS {
        set_slot(i, 0);
    }
}

fn hr_exports() -> [(usize, usize, i32); 12] {
    [
        (0, PIXBeginCapture2 as usize, E_UNEXPECTED),
        (1, PIXEndCapture as usize, E_UNEXPECTED),
        (2, PIXGetCaptureState as usize, 0),
        (6, GetCaptureControlState as usize, 0),
        (7, PIXEventsReplaceBlock as usize, 0),
        (8, ConfigurePMCs as usize, 0),
        (9, ConfigureNBPMCs as usize, 0),
        (10, ConfigureL3PMCs as usize, 0),
        (11, SetCaptureCallgraphState as usize, 0),
        (12, SetCaptureFunctionDetailsState as usize, 0),
        (13, SetCaptureFunctionSummaryState as usize, 0),
        (14, SetCaptureInstructionTraceState as usize, 0),
    ]
}

fn void_exports() -> [(usize, usize); 3] {
    [
        (3, PIXReportCounter as usize),
        (4, RegisterForCaptureControl as usize),
        (5, UnregisterForCaptureControl as usize),
    ]
}

#[test]
fn begin_capture1_is_not_implemented_on_console() {
    assert_eq!(PIXBeginCapture1(1, 2), E_NOTIMPL);
}

#[test]
fn unloaded_runtime_returns_console_defaults() {
    let _g = LOCK.lock().unwrap();
    clear();
    for (_, f, want) in hr_exports() {
        let f: F0 = unsafe { core::mem::transmute(f) };
        assert_eq!(unsafe { f() }, want);
    }
    for (_, f) in void_exports() {
        let f: V2 = unsafe { core::mem::transmute(f) };
        unsafe { f(1, 2) };
    }
}

#[test]
fn loaded_runtime_slots_receive_arguments() {
    let _g = LOCK.lock().unwrap();
    clear();
    for (i, f, _) in hr_exports() {
        set_slot(i, add2 as usize);
        let f: F2 = unsafe { core::mem::transmute(f) };
        assert_eq!(unsafe { f(3, 4) }, 34, "slot {i}");
        set_slot(i, 0);
    }
    for (i, f) in void_exports() {
        SEEN.store(0, Ordering::SeqCst);
        set_slot(i, rec2 as usize);
        let f: V2 = unsafe { core::mem::transmute(f) };
        unsafe { f(7, 9) };
        assert_eq!(SEEN.load(Ordering::SeqCst), 709, "slot {i}");
        set_slot(i, 0);
    }
}

#[test]
fn stack_arguments_pass_through_forwarders() {
    let _g = LOCK.lock().unwrap();
    clear();
    for (i, f) in [(8usize, ConfigurePMCs as usize), (9, ConfigureNBPMCs as usize), (10, ConfigureL3PMCs as usize)] {
        set_slot(i, add5 as usize);
        let f: F5 = unsafe { core::mem::transmute(f) };
        assert_eq!(unsafe { f(1, 1, 1, 1, 100) }, 510);
        set_slot(i, 0);
    }
}

#[test]
fn missing_runtime_fails_attach_and_clears_table() {
    let _g = LOCK.lock().unwrap();
    set_slot(0, add2 as usize);
    unsafe { (*core::ptr::addr_of_mut!(PIXEVT_TOOL_MEMORY))[3] = 7 };
    let hr = attach();
    let rt = wide(".\\pixrt.dll");
    let found = unsafe { LoadLibraryExW(rt.as_ptr(), core::ptr::null_mut(), 0) } != 0;
    assert_eq!(hr, if found { 0x8007_000Eu32 as i32 } else { E_UNEXPECTED });
    assert!((0..SLOTS).all(|i| slot(i) == 0));
    assert_eq!(tool_memory(), [0; 16]);
}

#[test]
fn runtime_name_follows_kmd_presence() {
    let n = select_runtime_name();
    assert_eq!(n, RT);
}

#[test]
fn search_rejects_dll_without_pix_entry_points() {
    assert_eq!(search_and_load("kernel32.dll"), 0);
}
