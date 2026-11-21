use super::*;
use std::sync::Mutex;

static SEEN: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static TEST_LOCK: Mutex<()> = Mutex::new(());

unsafe extern "system" fn fake_register(wait: *mut *mut c_void, service: *const u16, object: *mut c_void, callback: *mut c_void, context: *mut c_void, flags: u32) -> u32 {
    *SEEN.lock().unwrap() = vec![wait as usize, service as usize, object as usize, callback as usize, context as usize, flags as usize];
    0x1234
}

fn legacy_block() -> Vec<u8> {
    let mut b = vec![0u8; LEGACY_SIZE];
    for i in 0..LEGACY_SIZE / 8 {
        b[i * 8..i * 8 + 8].copy_from_slice(&(0x1000_0000_0000usize + i).to_le_bytes());
    }
    b[LEGACY_REGISTER_STOP..LEGACY_REGISTER_STOP + 8].copy_from_slice(&(fake_register as RegisterStopCallback as usize).to_le_bytes());
    b
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateEventW(attr: *mut c_void, manual: i32, initial: i32, name: *const u16) -> *mut c_void;
    fn RegisterWaitForSingleObject(wait: *mut *mut c_void, object: *mut c_void, callback: *mut c_void, context: *mut c_void, ms: u32, flags: u32) -> i32;
    fn CloseHandle(h: *mut c_void) -> i32;
}

unsafe extern "system" fn never(_: *mut c_void, _: u8) {}

#[test]
fn svchost_push_service_globals_builds_pc_ex_block_and_adapts_stop_callbacks() {
    let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let legacy = legacy_block();
    let ex = unsafe { build_ex(legacy.as_ptr()) };
    let bytes = unsafe { std::slice::from_raw_parts(ex, EX_SIZE) };
    assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 1);
    assert_eq!(&bytes[8..8 + LEGACY_SIZE], &legacy[..]);
    assert_eq!(&bytes[8 + LEGACY_REGISTER_STOP..8 + LEGACY_REGISTER_STOP + 8], &legacy[LEGACY_REGISTER_STOP..LEGACY_REGISTER_STOP + 8]);
    assert_eq!(last_ex_block(), ex);
    let reg: RegisterStopCallback = unsafe { std::mem::transmute(usize::from_le_bytes(bytes[EX_REGISTER_STOP..EX_REGISTER_STOP + 8].try_into().unwrap())) };
    let unreg: UnregisterStopCallback = unsafe { std::mem::transmute(usize::from_le_bytes(bytes[EX_UNREGISTER_STOP..EX_UNREGISTER_STOP + 8].try_into().unwrap())) };
    let r = unsafe { reg(8 as _, 16 as _, 24 as _, 32 as _, 40 as _, 7) };
    assert_eq!(r, 0x1234);
    assert_eq!(*SEEN.lock().unwrap(), vec![8, 16, 24, 32, 40, 7]);
    assert_eq!(unsafe { unreg(std::ptr::null_mut()) }, ERROR_INVALID_PARAMETER);
    unsafe {
        let ev = CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null());
        let mut w: *mut c_void = std::ptr::null_mut();
        assert_ne!(RegisterWaitForSingleObject(&mut w, ev, never as *const () as usize as *mut c_void, std::ptr::null_mut(), u32::MAX, 8), 0);
        assert_eq!(unreg(w), 0);
        CloseHandle(ev);
    }
}

#[test]
fn null_legacy_block_clears_adapter_and_missing_pc_dll_is_recorded() {
    let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { SvchostPushServiceGlobals(std::ptr::null()) };
    assert!(last_ex_block().is_null());
    assert_eq!(unsafe { register_stop_callback(std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0) }, ERROR_INVALID_PARAMETER);
    assert_ne!(last_push_error(), 0, "the test exe has no timebrokerserver_sys.dll beside it, so the push failure is recorded");
}