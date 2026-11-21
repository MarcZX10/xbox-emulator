use super::*;
use std::sync::Mutex;

pub(crate) static LOCK: Mutex<()> = Mutex::new(());
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub(crate) unsafe extern "system" fn sink(s: *const u8) {
    let c = core::ffi::CStr::from_ptr(s as *const core::ffi::c_char);
    LOG.lock().unwrap().push(c.to_string_lossy().into_owned());
}

pub(crate) fn take_log() -> Vec<String> {
    std::mem::take(&mut *LOG.lock().unwrap())
}

fn ctx_with(fences: &[u64], engines: &[u8], tail: u64) -> (Vec<u8>, Vec<u64>) {
    let arr = fences.to_vec();
    let mut c = vec![0u8; 0x60];
    let packed = (arr.as_ptr() as u64 & 0xff_ffff_ffff) | ((fences.len() as u64) << 40);
    c[8..16].copy_from_slice(&packed.to_le_bytes());
    c[0x10..0x10 + engines.len()].copy_from_slice(engines);
    c[0x50..0x58].copy_from_slice(&tail.to_le_bytes());
    (c, arr)
}

fn low_heap() -> bool {
    let v = vec![0u64; 4];
    (v.as_ptr() as u64) >> 40 == 0
}

#[test]
fn initialize_without_kmd_succeeds_and_maps_nothing() {
    let _g = LOCK.lock().unwrap();
    assert_eq!(unsafe { XhitInitialize(sink as *const () as usize) }, 0);
    assert!(active());
    assert!(regions().is_none());
    assert_eq!(device_handle(), -1);
    assert!(map_region(0).is_err());
    assert!(open_device().is_err());
}

#[test]
fn terminate_clears_active_object() {
    let _g = LOCK.lock().unwrap();
    unsafe { XhitInitialize(0) };
    assert!(active());
    assert_eq!(unsafe { XhitTerminate() }, 0);
    assert!(!active());
}

#[test]
fn engine_names_follow_descriptor_bits() {
    let mut c = vec![0u8; 0x60];
    c[0x10] = 1;
    c[0x11] = 2 | (2 << 3);
    c[0x12] = 3 | (1 << 3) | (5 << 5);
    c[0x13] = 0;
    assert_eq!(engine_name(c.as_ptr(), 0), "Gpu");
    assert_eq!(engine_name(c.as_ptr(), 1), "SDMA 2");
    assert_eq!(engine_name(c.as_ptr(), 2), "Compute pipe 1 queue 5");
    assert_eq!(engine_name(c.as_ptr(), 3), "Unknown 3");
}

#[test]
fn summary_reports_fence_deadlocks_and_unexpected_without_snapshot() {
    let _g = LOCK.lock().unwrap();
    if !low_heap() {
        return;
    }
    unsafe { XhitInitialize(sink as *const () as usize) };
    take_log();
    let f = 0x0001_0000_0000_0042u64;
    let (c, _arr) = ctx_with(&[0, CB_FENCE, f], &[1, 2, 1], 0x1234);
    let mut out = vec![0u64; 0x400 / 8];
    out[0] = 0xff;
    let hr = unsafe { XhitPerformHangSummary(c.as_ptr(), out.as_mut_ptr()) };
    assert_eq!(hr, E_UNEXPECTED);
    assert_eq!(out[0], 0xff | DEADLOCK_BIT);
    assert_eq!(out[0x318 / 8], 0x1234);
    let log = take_log().concat();
    assert!(log.contains("| XHIT Runtime GPU Hang Analysis"));
    assert!(log.contains("One or more fence deadlocks detected:\n"));
    assert!(log.contains("Engine SDMA 0 waiting for command buffer execution\n"));
    assert!(log.contains("Engine Gpu waiting for fence 0x0001000000000042 to be signaled from engine SDMA 0\n"));
    unsafe { XhitTerminate() };
}

#[test]
fn summary_null_context_clears_deadlock_bit() {
    let _g = LOCK.lock().unwrap();
    let mut out = vec![0u64; 0x400 / 8];
    out[0] = DEADLOCK_BIT | 1;
    out[0x318 / 8] = 9;
    let hr = unsafe { XhitPerformHangSummary(core::ptr::null(), out.as_mut_ptr()) };
    assert_eq!(hr, E_UNEXPECTED);
    assert_eq!(out[0], 1);
    assert_eq!(out[0x318 / 8], 0);
}

#[test]
fn banner_pads_lines_to_frame_width() {
    let _g = LOCK.lock().unwrap();
    unsafe { XhitInitialize(sink as *const () as usize) };
    take_log();
    banner("abc");
    let l = take_log();
    assert_eq!(l.len(), 3);
    assert_eq!(l[0].len(), 80);
    assert_eq!(l[1].len(), 80);
    assert!(l[1].starts_with("| abc "));
    assert!(l[1].ends_with("|\n"));
    unsafe { XhitTerminate() };
}

#[test]
fn hang_file_name_uses_system_time_fields() {
    let t = [2026u16, 11, 6, 21, 8, 5, 9, 0];
    assert_eq!(hang_file_name("t:\\", &t), "t:\\gpuhang-2026-11-21-08-05-09.xhit");
}

#[test]
fn analysis_without_hang_drive_fails() {
    let _g = LOCK.lock().unwrap();
    let hr = unsafe { analysis_in("q:\\nonexistent\\", 0, core::ptr::null_mut(), 0, core::ptr::null()) };
    assert_eq!(hr, E_FAIL);
}

#[test]
fn analysis_writes_header_then_fails_without_snapshot() {
    let _g = LOCK.lock().unwrap();
    let d = std::env::temp_dir().join(format!("xhit_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let dir = format!("{}\\", d.display());
    let mut buf = [0u16; 260];
    let hr = unsafe { analysis_in(&dir, 1, buf.as_mut_ptr(), 260, core::ptr::null()) };
    assert_eq!(hr, E_FAIL);
    let files: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(files.len(), 1);
    let b = std::fs::read(&files[0]).unwrap();
    assert_eq!(b.len(), 8);
    assert_eq!(&b[0..4], b"Xhit");
    assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), FILE_VERSION);
    assert_eq!(buf[0], 0);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn exported_analysis_uses_t_drive() {
    let _g = LOCK.lock().unwrap();
    if std::path::Path::new("t:\\").exists() {
        return;
    }
    let hr = unsafe { XhitPerformHangAnalysis(0, core::ptr::null_mut(), 0, core::ptr::null()) };
    assert_eq!(hr, E_FAIL);
}
