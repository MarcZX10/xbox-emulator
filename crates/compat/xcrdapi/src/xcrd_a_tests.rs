use crate::xcrd_a::*;
use crate::xcrd_core::*;
use crate::xcrd_storage::*;
use crate::xcrd_testkit::*;
use core::ffi::c_void;
use core::ptr::{null, null_mut};

const EINVAL: i32 = 0x8007_0057u32 as i32;

fn run(f: impl FnOnce(*mut c_void) -> i32) -> (i32, Vec<(u32, Vec<u8>)>) {
    let m = Mock::new();
    let _g = m.install();
    let a = adapter(3);
    let hr = f(a);
    (hr, m.xvd_calls())
}

fn run_with(m: &Mock, f: impl FnOnce(*mut c_void) -> i32) -> (i32, Vec<(u32, Vec<u8>)>) {
    let _g = m.install();
    let a = adapter(3);
    let hr = f(a);
    (hr, m.xvd_calls())
}

fn rejects_null_adapter(f: impl FnOnce(*mut c_void) -> i32) {
    let (hr, calls) = run(|_| f(null_mut()));
    assert_eq!(hr, EINVAL);
    assert!(calls.is_empty());
}

#[test]
fn open_adapter_probes_scsi_ports_until_signature() {
    let m = Mock::new();
    m.file("\\\\.\\Scsi2:", Ok(0x77));
    m.on_raw(|_, buf| {
        assert_eq!(u32::from_le_bytes(buf[0x10..0x14].try_into().unwrap()), 0x7800_000c);
        buf[4..12].copy_from_slice(&XVD_SIGNATURE);
        buf[0x14..0x18].copy_from_slice(&0i32.to_le_bytes());
        0
    });
    let _g = m.install();
    let mut a: *mut c_void = null_mut();
    let hr = unsafe { XCrdOpenAdapter(&mut a) };
    assert_eq!(hr, 0);
    assert_eq!(unsafe { adapter_handle(a) }, 0x77);
    assert_eq!(unsafe { adapter_index(a) }, 2);
    let creates = m.calls().iter().filter(|c| matches!(c, Call::Create { .. })).count();
    assert_eq!(creates, 3);
    assert_eq!(unsafe { XCrdCloseAdapter(a) }, 0);
    assert!(m.calls().contains(&Call::Close(0x77)));
}

#[test]
fn open_adapter_without_miniport_returns_last_open_error() {
    let m = Mock::new();
    let _g = m.install();
    let mut a: *mut c_void = null_mut();
    assert_eq!(unsafe { XCrdOpenAdapter(&mut a) }, 0x8007_0002u32 as i32);
    assert!(a.is_null());
}

#[test]
fn create_xvd_sends_create_code() {
    rejects_null_adapter(|a| unsafe { XCrdCreateXVD(a, w("x").as_ptr(), [0u8; 16].as_ptr(), 0, 0x1000, 0, 0, null_mut(), null(), null(), 0, null()) });
    let id = [7u8; 16];
    let (_, calls) = run(|a| unsafe { XCrdCreateXVD(a, w("[XTE:]\\a.xvd").as_ptr(), id.as_ptr(), 0, 0x10_0000, 0, 0, null_mut(), null(), null(), 0, null()) });
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, 0x7800_0020);
}

#[test]
fn mount_and_device_path_flow() {
    let (hr, c) = run(|a| unsafe { XCrdMount(&mut null_mut(), a, w("").as_ptr(), 0) });
    assert_eq!((hr, c.len()), (EINVAL, 0));
    let (_, calls) = run(|a| unsafe { XCrdMount(&mut null_mut(), a, w("[XTE:]\\g.xvd").as_ptr(), 0) });
    assert_eq!(calls[0].0, 0x7800_0004);
    assert_eq!(wide_at(&calls[0].1, 0x24), "[XTE:]\\g.xvd");
    assert_eq!(unsafe { XCrdQueryDevicePath(&mut null_mut(), null()) }, EINVAL);
    let (_, c) = run(|a| unsafe { XCrdQueryDevicePathByPath(a, w("[XTE:]\\q").as_ptr(), &mut null_mut()) });
    assert_eq!(c[0].0, 0x7800_0078);
    let (_, c) = run(|a| unsafe { XCrdMountContentType(&mut null_mut(), a, w("q").as_ptr(), 5, 0) });
    assert_eq!(le32(&c[0].1, 0x1c), 5);
    assert_ne!(le32(&c[0].1, 0x18) & 0x80, 0);
}

#[test]
fn free_device_path_buffer_releases_heap() {
    let p = unsafe { heap_alloc(0x40) } as *mut c_void;
    assert_eq!(unsafe { XCrdFreeDevicePathBuffer(p) }, 0);
}

#[test]
fn corruption_error_classifier() {
    assert_eq!(XCrdIsCorruptionError(0), 0);
    assert_eq!(XCrdIsCorruptionError(EINVAL), 0);
    for hr in [0x8007_01e7u32, 0x8007_0570, 0x8007_0571, 0x8007_045d] {
        assert_eq!(XCrdIsCorruptionError(hr as i32), 1);
    }
    assert_eq!(XCrdIsCorruptionError(0x8007_0572u32 as i32), 0);
}

#[test]
fn build_and_parse_path_round_trip() {
    let mut out: *mut u16 = null_mut();
    let hr = unsafe { XCrdBuildPath(&mut out, 10, w("game.xvd").as_ptr(), 0) };
    assert!(hr >= 0);
    let s = String::from_utf16_lossy(unsafe { wslice(out) });
    assert!(s.starts_with("[XVE:]"));
    let mut idx = -1;
    assert!(unsafe { XCrdParsePath(out, &mut idx) } >= 0);
    assert_eq!(idx, 10);
    unsafe { heap_free(out as *mut c_void) };
}

#[test]
fn delete_and_move_sidecar_codes() {
    rejects_null_adapter(|a| unsafe { XCrdDeleteXVD(a, w("p").as_ptr(), 0) });
    let (_, c) = run(|a| unsafe { XCrdDeleteXVD(a, w("[XTE:]\\d").as_ptr(), 0) });
    assert_eq!(c[0].0, 0x7800_0024);
    rejects_null_adapter(|a| unsafe { XCrdMoveSidecarFiles(a, w("s").as_ptr(), w("d").as_ptr(), 0) });
    let (_, c) = run(|a| unsafe { XCrdMoveSidecarFiles(a, w("s").as_ptr(), w("d").as_ptr(), 0) });
    assert_eq!(c[0].0, 0x7800_01b0);
}

#[test]
fn read_user_data_and_bat_info() {
    let mut buf = [0u8; 64];
    let mut len = 64u32;
    rejects_null_adapter(|a| unsafe { XCrdReadUserDataXVD(a, w("p").as_ptr(), 0, buf.as_mut_ptr(), &mut len) });
    let (_, c) = run(|a| unsafe { XCrdReadUserDataXVD(a, w("p").as_ptr(), 0, buf.as_mut_ptr(), &mut len) });
    assert_eq!(c[0].0, 0x7800_0028);
    let mut info = [0u8; 0x40];
    let mut len = 64u32;
    rejects_null_adapter(|a| unsafe { XCrdReadBatInfoXVD(a, w("p").as_ptr(), buf.as_mut_ptr(), &mut len, info.as_mut_ptr()) });
}

#[test]
fn mapped_control_page_matches_driver_page() {
    assert_eq!(crate::xcrd_driver::dstorage_init(), 0);
    let mut p = 1usize as *mut c_void;
    assert_eq!(unsafe { DStorageGetMappedControlPage(&mut p) }, 0);
    assert_eq!(p as usize, crate::xcrd_driver::control_page().unwrap().page);
    assert!(!p.is_null());
}

#[test]
fn console_not_supported_exports() {
    assert_eq!(XCrdQueryEncryptedInfo(), E_NOTSUPPORTED);
    assert_eq!(XCrdQueryUnlockInfo(), E_NOTSUPPORTED);
    assert_eq!(XCrdRepairFiles(), E_NOTSUPPORTED);
}

#[test]
fn grow_thin_and_dynamic_share_payload() {
    for f in [XCrdGrowThinProvisionedXvd as unsafe extern "system" fn(*mut c_void, *const u16, u64) -> i32, XCrdGrowDynamicXvd] {
        rejects_null_adapter(|a| unsafe { f(a, w("p").as_ptr(), 1) });
        let (hr, _) = run(|a| unsafe { f(a, w("p").as_ptr(), 0) });
        assert_eq!(hr, EINVAL);
        let (_, c) = run(|a| unsafe { f(a, w("ab").as_ptr(), 0x5000) });
        assert_eq!(c[0].0, 0x7800_0170);
        assert_eq!(le32(&c[0].1, 0), 0x1e);
        assert_eq!(le64(&c[0].1, 0x10), 0x5000);
        assert_eq!(wide_at(&c[0].1, 0x18), "ab");
    }
}

#[test]
fn reset_streamed_state_payload() {
    rejects_null_adapter(|a| unsafe { XCrdResetStreamedState(a, w("p").as_ptr(), 1) });
    let (hr, _) = run(|a| unsafe { XCrdResetStreamedState(a, w("p").as_ptr(), 0) });
    assert_eq!(hr, EINVAL);
    let (_, c) = run(|a| unsafe { XCrdResetStreamedState(a, w("p").as_ptr(), 9) });
    assert_eq!(c[0].0, 0x7800_004c);
    assert_eq!(le32(&c[0].1, 0), 9);
    assert_eq!(wide_at(&c[0].1, 0xc), "p");
}

#[test]
fn odd_pm_reference() {
    let mut out = 0u32;
    rejects_null_adapter(|a| unsafe { XCrdOddPmAcquireReleaseReference(&mut out, a, 1) });
    let (_, c) = run(|a| unsafe { XCrdOddPmAcquireReleaseReference(&mut out, a, 1) });
    assert_eq!(c[0].0, 0x7800_0098);
}

#[test]
fn crd_information_classes() {
    let mut out = [0u8; 0x670];
    rejects_null_adapter(|a| unsafe { XCrdQueryCrdInformation(a, 0, 0, out.as_mut_ptr(), 0x10) });
    let (hr, _) = run(|a| unsafe { XCrdQueryCrdInformation(a, 0x25, 0, out.as_mut_ptr(), 0x10) });
    assert_eq!(hr, EINVAL);
    let (hr, _) = run(|a| unsafe { XCrdQueryCrdInformation(a, 0, 1, out.as_mut_ptr(), 0x10) });
    assert_eq!(hr, EINVAL);
    let m = Mock::new();
    m.on_xvd(|_, _, p| {
        p[0x10..0x18].copy_from_slice(&0x1234u64.to_le_bytes());
        0
    });
    let (hr, c) = run_with(&m, |a| unsafe { XCrdQueryCrdInformation(a, 4, 0, out.as_mut_ptr(), 0x10) });
    assert_eq!(hr, 0);
    assert_eq!(le32(&c[0].1, 8), 4);
    assert_eq!(le32(&c[0].1, 0xc), 2);
    assert_eq!(le64(&out, 0), 0x1234);
}

#[test]
fn crd_log_and_vendor_information() {
    let mut buf = [0u8; 0x20];
    rejects_null_adapter(|a| unsafe { XCrdQueryCrdLogInformation(a, 0, 1, 0, 0, buf.as_mut_ptr(), 0x20) });
    rejects_null_adapter(|a| unsafe { XCrdQueryCrdVendorInformation(a, 0, 1, 2, 3, 4, buf.as_mut_ptr(), 0x20) });
    let (_, c) = run(|a| unsafe { XCrdQueryCrdLogInformation(a, 0, 1, 0, 0, buf.as_mut_ptr(), 0x20) });
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, 0x7800_0050);
}

#[test]
fn xvd_information_and_feature_bits() {
    let mut out = [0u8; 0x400];
    rejects_null_adapter(|a| unsafe { XCrdQueryXvdInformation(a, w("p").as_ptr(), 0, out.as_mut_ptr(), 0x400) });
    let mut bits = 0u64;
    let m = Mock::new();
    let (_, c) = run_with(&m, |a| unsafe { XCloudQueryFeatureBits(a, &mut bits) });
    assert_eq!(c[0].0, 0x7800_0050);
    assert_eq!(wide_at(&c[0].1, 0x418), "[XUC:]\\ANYXVC");
}

#[test]
fn cancel_update_code() {
    rejects_null_adapter(|a| unsafe { XCrdCancelUpdate(a, w("p").as_ptr()) });
    let (_, c) = run(|a| unsafe { XCrdCancelUpdate(a, w("p").as_ptr()) });
    assert_eq!(c[0].0, 0x7800_0114);
}

struct FakeWnf(std::rc::Rc<std::cell::RefCell<Vec<(u64, &'static str)>>>);

impl Wnf for FakeWnf {
    fn query(&mut self, name: u64, _cb: WnfCallback, _ctx: *mut c_void, stamp: &mut u32) -> i32 {
        self.0.borrow_mut().push((name, "query"));
        *stamp = 5;
        0
    }
    fn subscribe(&mut self, name: u64, stamp: u32, _cb: WnfCallback, _ctx: *mut c_void, sub: &mut *mut c_void) -> i32 {
        assert_eq!(stamp, 5);
        self.0.borrow_mut().push((name, "sub"));
        *sub = 0x99usize as *mut c_void;
        0
    }
    fn unsubscribe(&mut self, sub: *mut c_void) -> i32 {
        assert_eq!(sub as usize, 0x99);
        self.0.borrow_mut().push((0, "unsub"));
        0
    }
}

static SEEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

unsafe extern "system" fn user_cb(_ctx: *mut c_void, v: u32) {
    SEEN.store(v, std::sync::atomic::Ordering::SeqCst);
}

#[test]
fn wnf_register_and_unregister_all_three() {
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    WNF_OVERRIDE.with(|o| *o.borrow_mut() = Some(Box::new(FakeWnf(log.clone()))));
    let mut r: *mut c_void = null_mut();
    assert_eq!(unsafe { XCrdRegisterExternalStorageChangeCallback(Some(user_cb), null_mut(), &mut r) }, 0);
    let buf = [0x34u8, 0x12, 0x02, 0x00];
    unsafe { external_storage_thunk(0, 0, null(), r, buf.as_ptr(), 4) };
    assert_eq!(SEEN.load(std::sync::atomic::Ordering::SeqCst), 0x2_1234);
    assert_eq!(unsafe { XCrdUnregisterExternalStorageChangeCallback(r) }, 0);
    assert_eq!(unsafe { XCrdRegisterWriteBudgetCallback(Some(user_cb), null_mut(), &mut r) }, 0);
    assert_eq!(unsafe { XCrdUnregisterWriteBudgetCallback(r) }, 0);
    assert_eq!(unsafe { XCrdRegisterCorruptionCallback(Some(user_cb), &mut r) }, 0);
    assert_eq!(unsafe { XCrdUnregisterCorruptionCallback(r) }, 0);
    WNF_OVERRIDE.with(|o| *o.borrow_mut() = None);
    let l = log.borrow();
    assert_eq!(l[0].0, 0x1989_0c35_a3bc_f075);
    assert_eq!(l.iter().filter(|e| e.1 == "unsub").count(), 3);
}

#[test]
fn defrag_xvc_payload_and_outputs() {
    let (mut a1, mut a2, mut a3) = (9u32, 9u32, 9u32);
    rejects_null_adapter(|a| unsafe { XCrdDefragXvc(a, w("p").as_ptr(), 0, &mut a1, &mut a2, &mut a3) });
    let (hr, _) = run(|a| unsafe { XCrdDefragXvc(a, w("p").as_ptr(), 2, &mut a1, &mut a2, &mut a3) });
    assert_eq!(hr, EINVAL);
    let m = Mock::new();
    m.on_xvd(|_, _, p| {
        p[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
        p[0x14..0x18].copy_from_slice(&2u32.to_le_bytes());
        p[0x18..0x1c].copy_from_slice(&3u32.to_le_bytes());
        0
    });
    let (hr, c) = run_with(&m, |a| unsafe { XCrdDefragXvc(a, w("xy").as_ptr(), 1, &mut a1, &mut a2, &mut a3) });
    assert_eq!(hr, 0);
    assert_eq!(c[0].0, 0x7800_0150);
    assert_eq!(le32(&c[0].1, 0), 0x40);
    assert_eq!(le32(&c[0].1, 4), 4);
    assert_eq!(le32(&c[0].1, 8), 1);
    assert_eq!(wide_at(&c[0].1, 0x40), "xy");
    assert_eq!((a1, a2, a3), (1, 2, 3));
}

#[test]
fn metadata_handle_open_close() {
    let mut h: *mut u32 = null_mut();
    rejects_null_adapter(|a| unsafe { XCrdOpenMetadataHandle(&mut h, a, w("p").as_ptr()) });
    let m = Mock::new();
    m.on_xvd(|_, code, p| {
        if code == 0x7800_00d0 {
            p[8..12].copy_from_slice(&0xabcu32.to_le_bytes());
        }
        0
    });
    let (hr, c) = run_with(&m, |a| unsafe {
        let r = XCrdOpenMetadataHandle(&mut h, a, w("md").as_ptr());
        assert_eq!(*h, 0xabc);
        assert_eq!(XCrdCloseMetadataHandle(&mut h, a), 0);
        r
    });
    assert_eq!(hr, 0);
    assert!(h.is_null());
    assert_eq!(c[0].0, 0x7800_00d0);
    assert_eq!(le32(&c[0].1, 0), 0x30);
    assert_eq!(le32(&c[0].1, 4), 6);
    assert_eq!(wide_at(&c[0].1, 0x30), "md");
    assert_eq!(c[1].0, 0x7800_00d4);
    assert_eq!(le32(&c[1].1, 0), 0xabc);
    let m = Mock::new();
    m.on_xvd(|_, _, _| 0xc000_0001u32 as i32);
    let mut h2: *mut u32 = null_mut();
    let (hr, _) = run_with(&m, |a| unsafe { XCrdOpenMetadataHandle(&mut h2, a, w("md").as_ptr()) });
    assert!(hr < 0);
    assert!(h2.is_null());
}

#[test]
fn remap_reload_and_test() {
    rejects_null_adapter(|a| unsafe { XCrdRemapReload(a) });
    let (_, c) = run(|a| unsafe { XCrdRemapReload(a) });
    assert_eq!(c[0].0, 0x7800_00f0);
    let mut out = [0u16; 32];
    rejects_null_adapter(|a| unsafe { XCrdRemapTest(a, w("p").as_ptr(), out.as_mut_ptr(), 64) });
    let (hr, _) = run(|a| unsafe { XCrdRemapTest(a, w("p").as_ptr(), out.as_mut_ptr(), 0) });
    assert_eq!(hr, EINVAL);
    let m = Mock::new();
    m.on_xvd(|_, _, p| {
        for (i, c) in "res".encode_utf16().enumerate() {
            p[0x10 + i * 2..0x12 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        0
    });
    let (hr, c) = run_with(&m, |a| unsafe { XCrdRemapTest(a, w("in").as_ptr(), out.as_mut_ptr(), 64) });
    assert_eq!(hr, 0);
    assert_eq!(c[0].0, 0x7800_00f4);
    assert_eq!(le32(&c[0].1, 4), 0x428);
    assert_eq!(wide_at(&c[0].1, 0x428), "in");
    assert_eq!(String::from_utf16_lossy(zstr(&out)), "res");
}

#[test]
fn query_corrupt_xvc_allocates_copy() {
    let mut out: *mut u16 = null_mut();
    rejects_null_adapter(|a| unsafe { XCrdQueryCorruptXvc(a, &mut out) });
    let m = Mock::new();
    m.on_xvd(|_, _, p| {
        p[0..2].copy_from_slice(&(b'Q' as u16).to_le_bytes());
        0
    });
    let (hr, c) = run_with(&m, |a| unsafe { XCrdQueryCorruptXvc(a, &mut out) });
    assert_eq!(hr, 0);
    assert_eq!(c[0].0, 0x7800_0100);
    assert_eq!(String::from_utf16_lossy(unsafe { wslice(out) }), "Q");
    unsafe { heap_free(out as *mut c_void) };
}

#[test]
fn is_corrupt_maps_not_found_to_clean() {
    let mut b = 7u8;
    rejects_null_adapter(|a| unsafe { XCrdIsCorrupt(a, w("p").as_ptr(), &mut b) });
    let (hr, c) = run(|a| unsafe { XCrdIsCorrupt(a, w("p").as_ptr(), &mut b) });
    assert_eq!((hr, b), (0, 1));
    assert_eq!(c[0].0, 0x7800_0108);
    assert_eq!(le32(&c[0].1, 0), 8);
    let m = Mock::new();
    m.on_xvd(|_, _, _| 0xc000_0225u32 as i32);
    let (hr, _) = run_with(&m, |a| unsafe { XCrdIsCorrupt(a, w("p").as_ptr(), &mut b) });
    assert_eq!(nt_to_hr(0xc000_0225u32 as i32), E_NOT_FOUND);
    assert_eq!((hr, b), (0, 0));
}

#[test]
fn repair_xvc_validation_and_layout() {
    rejects_null_adapter(|a| unsafe { XCrdRepairXvc(a, null()) });
    let x = w("xv");
    let s = w("src");
    let bad = RepairInfo { kind: 0, xvc: x.as_ptr(), src: null(), value: 0, flags: 0 };
    let (hr, _) = run(|a| unsafe { XCrdRepairXvc(a, &bad) });
    assert_eq!(hr, EINVAL);
    let bad = RepairInfo { kind: 3, xvc: null(), src: null(), value: 0, flags: 4 };
    let (hr, _) = run(|a| unsafe { XCrdRepairXvc(a, &bad) });
    assert_eq!(hr, EINVAL);
    let ok = RepairInfo { kind: 0, xvc: x.as_ptr(), src: s.as_ptr(), value: 0x55, flags: 1 };
    let (hr, c) = run(|a| unsafe { XCrdRepairXvc(a, &ok) });
    assert_eq!(hr, 0);
    let p = &c[0].1;
    assert_eq!(c[0].0, 0x7800_0104);
    assert_eq!(p.len(), 0x1e + 4 + 8);
    assert_eq!(le32(p, 4), 1);
    assert_eq!(le32(p, 8), 0x55);
    assert_eq!(le32(p, 0xc), 0x1c);
    assert_eq!(wide_at(p, 0x1c), "xv");
    assert_eq!(le32(p, 0x14), 0x22);
    assert_eq!(wide_at(p, 0x22), "src");
}

#[test]
fn write_stats() {
    let mut out = [0u8; 0x20];
    rejects_null_adapter(|a| unsafe { XCrdQueryWriteStats(a, 0, out.as_mut_ptr()) });
    let (hr, _) = run(|a| unsafe { XCrdQueryWriteStats(a, 1, out.as_mut_ptr()) });
    assert_eq!(hr, EINVAL);
    let m = Mock::new();
    m.on_xvd(|_, _, p| {
        p[8] = 0x11;
        p[0x27] = 0x22;
        0
    });
    let (_, c) = run_with(&m, |a| unsafe { XCrdQueryWriteStats(a, 0, out.as_mut_ptr()) });
    assert_eq!(c[0].0, 0x7800_0168);
    assert_eq!((out[0], out[0x1f]), (0x11, 0x22));
}

#[test]
fn xrsc_information() {
    let mut out = [0u8; 0xb8];
    rejects_null_adapter(|a| unsafe { XCrdQueryXRscInformation(a, w("p").as_ptr(), 0, out.as_mut_ptr(), 0xb8) });
    let (hr, _) = run(|a| unsafe { XCrdQueryXRscInformation(a, w("p").as_ptr(), 0, out.as_mut_ptr(), 0xb7) });
    assert_eq!(hr, EINVAL);
    let (hr, _) = run(|a| unsafe { XCrdQueryXRscInformation(a, w("p").as_ptr(), 2, out.as_mut_ptr(), 0xb8) });
    assert_eq!(hr, EINVAL);
    let m = Mock::new();
    m.on_xvd(|_, _, p| {
        p[0x10] = 1;
        p[0x40] = 2;
        p[0x44] = 3;
        0
    });
    let (_, c) = run_with(&m, |a| unsafe { XCrdQueryXRscInformation(a, w("p").as_ptr(), 1, out.as_mut_ptr(), 0x90) });
    assert_eq!(le32(&c[0].1, 0xc), 0x19);
    assert_eq!(wide_at(&c[0].1, 0x418), "p");
    assert_eq!((out[0], out[0x2c], out[0x30]), (1, 2, 3));
}

#[test]
fn create_xvd_pointer_rules_and_layout() {
    let p = w("ab");
    let ptr = w("c");
    let hdr = [1u8; 0xe00];
    let xi = [2u8; 0xda8];
    rejects_null_adapter(|a| unsafe { XCrdCreateXvdPointer(a, p.as_ptr(), ptr.as_ptr(), 0, 0, null(), null(), null()) });
    let (hr, _) = run(|a| unsafe { XCrdCreateXvdPointer(a, p.as_ptr(), ptr.as_ptr(), 0, 4, null(), null(), null()) });
    assert_eq!(hr, EINVAL);
    let (hr, _) = run(|a| unsafe { XCrdCreateXvdPointer(a, p.as_ptr(), null(), 0, 0, null(), null(), null()) });
    assert_eq!(hr, EINVAL);
    let (hr, _) = run(|a| unsafe { XCrdCreateXvdPointer(a, p.as_ptr(), ptr.as_ptr(), 0, 1, hdr.as_ptr(), null(), null()) });
    assert_eq!(hr, EINVAL);
    let (hr, c) = run(|a| unsafe { XCrdCreateXvdPointer(a, p.as_ptr(), ptr.as_ptr(), 9, 1, hdr.as_ptr(), xi.as_ptr(), null()) });
    assert_eq!(hr, 0);
    let b = &c[0].1;
    assert_eq!(c[0].0, 0x7800_00e0);
    assert_eq!(le32(b, 0) as usize, b.len());
    assert_eq!(le32(b, 0), 6 + 0x3c + 0xe00 + 0xda8);
    assert_eq!(wide_at(b, 0x38), "c");
    assert_eq!(le32(b, 4), 0x3c);
    assert_eq!(wide_at(b, 0x3c), "ab");
    assert_eq!(le32(b, 0x14), 0x42);
    assert_eq!(b[0x42], 1);
    assert_eq!(le32(b, 0x1c), 0xe42);
    assert_eq!(b[0xe42], 2);
    assert_eq!(le32(b, 0x24), 1);
    assert_eq!(le64(b, 0x28), 9);
}

fn xvd_header() -> Vec<u8> {
    let mut h = vec![0u8; 0x1000];
    h[0..8].copy_from_slice(b"msft-xvd");
    h
}

#[test]
fn xvc_offset_from_header() {
    let mut off = 0u64;
    assert_eq!(unsafe { XCrdGetXvcOffsetFromHeader(null(), &mut off) }, EINVAL);
    let mut h = xvd_header();
    h[0x84] = 2;
    assert_eq!(unsafe { XCrdGetXvcOffsetFromHeader(h.as_ptr(), &mut off) }, EINVAL);
    h[0x84] = 1;
    h[0x8c..0x90].copy_from_slice(&0x1000u32.to_le_bytes());
    h[0x18..0x20].copy_from_slice(&0x20_0000u64.to_le_bytes());
    h[0x88..0x8c].copy_from_slice(&0x2000u32.to_le_bytes());
    h[0x270] = 1;
    assert_eq!(unsafe { XCrdGetXvcOffsetFromHeader(h.as_ptr(), &mut off) }, 0);
    let data = 0x200 + 1;
    let tree = (data + 0xa9) / 0xaa;
    let top = 1;
    assert_eq!(off, ((tree + top + 1 + 2 + 1 + 3) as u64) << 12);
    h[8] = 4;
    assert_eq!(unsafe { XCrdGetXvcOffsetFromHeader(h.as_ptr(), &mut off) }, 0);
    assert_eq!(off, 7u64 << 12);
    assert_eq!(hash_level_blocks(28900, 1), 1);
    assert_eq!(hash_level_blocks(28901, 1), 2);
    assert_eq!(hash_level_blocks(5, 4), 0);
}

#[test]
fn streaming_roamable_console_mode() {
    let mut v = 0u16;
    let junk = vec![0u8; 0x400];
    assert_eq!(unsafe { XCrdGetStreamingRoamableXvdConsoleMode(junk.as_ptr(), &mut v) }, 0x8007_0570u32 as i32);
    assert_eq!(unsafe { XCrdGetStreamingRoamableXvdConsoleMode(junk.as_ptr(), null_mut()) }, EINVAL);
    let mut h = xvd_header();
    assert_eq!(unsafe { XCrdGetStreamingRoamableXvdConsoleMode(h.as_ptr(), &mut v) }, E_NOTSUPPORTED);
    h[9] = 2;
    h[0x276] = 2;
    assert_eq!(unsafe { XCrdGetStreamingRoamableXvdConsoleMode(h.as_ptr(), &mut v) }, 0);
    assert_eq!(v, 0x64);
    h[0x276] = 9;
    assert_eq!(unsafe { XCrdGetStreamingRoamableXvdConsoleMode(h.as_ptr(), &mut v) }, 0x8007_0570u32 as i32);
}

#[test]
fn sideloaded_key_present() {
    let key = [5u8; 16];
    let mut out = 0u8;
    rejects_null_adapter(|a| unsafe { XCrdIsSideloadedKeyPresent(a, key.as_ptr(), &mut out) });
    let m = Mock::new();
    m.on_xvd(|_, _, p| {
        p[0x10] = 1;
        0
    });
    let (_, c) = run_with(&m, |a| unsafe { XCrdIsSideloadedKeyPresent(a, key.as_ptr(), &mut out) });
    assert_eq!(c[0].0, 0x7800_0180);
    assert_eq!(&c[0].1[0..16], &key);
    assert_eq!(out, 1);
}

#[test]
fn populate_sparse_cache_blocks_layout() {
    let md = [0xaau8; 3];
    let data = [0xbbu8; 5];
    let id = [1u8; 16];
    let mut done = 0u32;
    rejects_null_adapter(|a| unsafe { XCrdPopulateSparseCacheBlocks(a, w("p").as_ptr(), md.as_ptr(), 3, data.as_ptr(), 5, id.as_ptr(), &mut done) });
    let (hr, _) = run(|a| unsafe { XCrdPopulateSparseCacheBlocks(a, w("").as_ptr(), md.as_ptr(), 3, data.as_ptr(), 5, id.as_ptr(), &mut done) });
    assert_eq!(hr, EINVAL);
    let m = Mock::new();
    m.on_xvd(|_, _, p| {
        p[0x10..0x14].copy_from_slice(&42u32.to_le_bytes());
        0
    });
    let (hr, c) = run_with(&m, |a| unsafe { XCrdPopulateSparseCacheBlocks(a, w("p").as_ptr(), md.as_ptr(), 3, data.as_ptr(), 5, id.as_ptr(), &mut done) });
    assert_eq!(hr, 0);
    let b = &c[0].1;
    assert_eq!(c[0].0, 0x7800_0184);
    assert_eq!(b.len(), 3 + 0x32 + 2 + 5);
    assert_eq!(le32(b, 0x18), 0x34);
    assert_eq!(b[0x34], 0xaa);
    assert_eq!(le32(b, 0x20), 0x37);
    assert_eq!(b[0x37], 0xbb);
    assert_eq!(wide_at(b, 0x30), "p");
    assert_eq!(done, 42);
}

#[test]
fn uab_bitmap_offsets() {
    let mut u = vec![0u8; 0x1000];
    u[0x2c0..0x2c8].copy_from_slice(&(0xaa000u64 * 9).to_le_bytes());
    u[0x2a0..0x2a8].copy_from_slice(&(0xaa000u64 * 26).to_le_bytes());
    u[0x208..0x210].copy_from_slice(&0x300u64.to_le_bytes());
    u[0x210..0x218].copy_from_slice(&0x500u64.to_le_bytes());
    let (mut mp, mut dp) = (null_mut(), null_mut());
    let (mut ml, mut dl) = (0u32, 0u32);
    assert_eq!(unsafe { XCrdGetUabBitmapOffset(u.as_mut_ptr(), 0x100, 0, &mut mp, &mut ml, &mut dp, &mut dl) }, 0x8007_0570u32 as i32);
    assert_eq!(unsafe { XCrdGetUabBitmapOffset(u.as_mut_ptr(), 0x1000, 1, &mut mp, &mut ml, &mut dp, &mut dl) }, 0);
    assert_eq!((ml, dl), (2, 3));
    assert_eq!(mp as usize - u.as_ptr() as usize, 0x500);
    assert_eq!(dp as usize - u.as_ptr() as usize, 0x502);
}

#[test]
fn flat_content_path_and_extended_attributes() {
    rejects_null_adapter(|a| unsafe { XCrdSetFlatContentPath(a, w("a").as_ptr(), w("b").as_ptr()) });
    let (_, c) = run(|a| unsafe { XCrdSetFlatContentPath(a, w("ab").as_ptr(), w("c").as_ptr()) });
    assert_eq!(c[0].0, 0x7800_0194);
    assert_eq!(le32(&c[0].1, 8), 0x16);
    assert_eq!(wide_at(&c[0].1, 0x10), "ab");
    assert_eq!(wide_at(&c[0].1, 0x16), "c");
    let (hr, _) = run(|a| unsafe { XCrdSetExtendedAttributes(a, w("a").as_ptr(), 2, 0) });
    assert_eq!(hr, EINVAL);
    let (_, c) = run(|a| unsafe { XCrdSetExtendedAttributes(a, w("a").as_ptr(), 0x8000_0009, 77) });
    assert_eq!(c[0].0, 0x7800_01a4);
    assert_eq!(le32(&c[0].1, 8), 0x8000_0009);
    assert_eq!(le64(&c[0].1, 0x10), 77);
    assert_eq!(wide_at(&c[0].1, 0x18), "a");
    let (hr, _) = run(|a| unsafe { XCrdCopyExtendedAttributes(a, w("a").as_ptr(), w("b").as_ptr(), 0x10) });
    assert_eq!(hr, EINVAL);
    let (_, c) = run(|a| unsafe { XCrdCopyExtendedAttributes(a, w("a").as_ptr(), w("bc").as_ptr(), 0xf) });
    assert_eq!(c[0].0, 0x7800_01a8);
    assert_eq!(le32(&c[0].1, 0x10), 7);
    assert_eq!(wide_at(&c[0].1, 0x14), "a");
    assert_eq!(le32(&c[0].1, 8), 0x18);
    assert_eq!(wide_at(&c[0].1, 0x18), "bc");
}

#[test]
fn title_quirks() {
    rejects_null_adapter(|a| unsafe { XCrdSetTitleQuirks(a, 1) });
    let (hr, _) = run(|a| unsafe { XCrdSetTitleQuirks(a, 2) });
    assert_eq!(hr, EINVAL);
    let (_, c) = run(|a| unsafe { XCrdSetTitleQuirks(a, 1) });
    assert_eq!((c[0].0, le32(&c[0].1, 0)), (0x7800_01b4, 1));
}

#[test]
fn storage_device_enumeration() {
    let mut d = 0u32;
    assert_eq!(unsafe { XCrdFindFirstStorageDevice(null_mut(), &mut d) }, E_POINTER_HR);
    assert_eq!(unsafe { XCrdFindNextStorageDevice(null_mut(), &mut d) }, EINVAL);
    assert_eq!(unsafe { XCrdFindCloseStorageDevice(null_mut()) }, EINVAL);
    let m = Mock::new();
    m.file("\\\\.\\Scsi0:", Ok(0x44));
    m.on_raw(|_, buf| {
        buf[4..12].copy_from_slice(&XVD_SIGNATURE);
        buf[0x14..0x18].copy_from_slice(&0i32.to_le_bytes());
        0
    });
    let next = std::rc::Rc::new(std::cell::Cell::new(10u32));
    let n2 = next.clone();
    m.on_xvd(move |_, code, p| {
        assert_eq!(code, 0x7800_008c);
        let dev = n2.get();
        p[4..8].copy_from_slice(&dev.to_le_bytes());
        p[8..12].copy_from_slice(&(dev + 100).to_le_bytes());
        n2.set(dev + 1);
        0
    });
    let _g = m.install();
    let mut f = null_mut();
    assert_eq!(unsafe { XCrdFindFirstStorageDevice(&mut f, &mut d) }, 0);
    assert_eq!(d, 10);
    assert_eq!(unsafe { XCrdFindNextStorageDevice(f, &mut d) }, 0);
    assert_eq!(d, 11);
    assert_eq!(unsafe { XCrdFindCloseStorageDevice(f) }, 0);
    let ops: Vec<(u32, u32)> = m.xvd_calls().iter().map(|(_, p)| (le32(p, 0), le32(p, 8))).collect();
    assert_eq!(ops, vec![(0, 0), (1, 110), (2, 111)]);
    assert!(m.calls().contains(&Call::Close(0x44)));
}

#[test]
fn internal_device_table_and_storage_requests() {
    let g = [0x52u8, 0xce, 0x9e, 0xa8, 0x8e, 0x7e, 0x4f, 0x44, 0xbb, 0xd0, 0xc6, 0x8b, 0x76, 0xc2, 0xec, 0xa4];
    let mut id = 0u32;
    assert_eq!(unsafe { XCrdIsInternalStorageDevice(g.as_ptr(), &mut id) }, 1);
    assert_eq!(id, 1);
    let mut out = [0u8; 16];
    assert_eq!(unsafe { XCrdHasInternalDeviceId(1, out.as_mut_ptr()) }, 1);
    assert_eq!(out, g);
    rejects_null_adapter(|a| unsafe { XCrdSetEraLaunchMapping(a, w("p").as_ptr(), 1) });
    let (_, c) = run(|a| unsafe { XCrdSetEraLaunchMapping(a, w("p").as_ptr(), 1) });
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, 0x7800_0088);
    let (hr, c) = run(|a| unsafe { XCrdSetExternalPlsMapping(a, w("p").as_ptr(), 1) });
    assert_eq!((hr, c.len()), (EINVAL, 0));
    let (_, c) = run(|a| unsafe { XCrdSetExternalPlsMapping(a, w("p").as_ptr(), 0x13) });
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, 0x7800_0088);
    let mut o = 0u32;
    let (_, c) = run(|a| unsafe { XCrdFindStorageDevice(a, g.as_ptr(), &mut o) });
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, 0x7800_0088);
    let (_, c) = run(|a| unsafe { XCrdSetDeviceIdMapping(a, g.as_ptr(), g.as_ptr()) });
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, 0x7800_0088);
    let (_, c) = run(|a| unsafe { XCrdGetDeviceIdMapping(a, g.as_ptr(), out.as_mut_ptr()) });
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, 0x7800_0088);
    let mut blob = [0u8; 0x80];
    let (_, c) = run(|a| unsafe { XCrdQueryAttestationBlob(a, g.as_ptr(), blob.as_mut_ptr()) });
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, 0x7800_009c);
    rejects_null_adapter(|a| unsafe { XCrdResizeXVD(a, w("p").as_ptr(), 0x1000) });
}

#[test]
fn set_xvd_information_classes() {
    let v = 0x1122_3344_5566_7788u64.to_le_bytes();
    rejects_null_adapter(|a| unsafe { XCrdSetXvdInformation(a, w("p").as_ptr(), 0, v.as_ptr(), 8) });
    let (hr, c) = run(|a| unsafe { XCrdSetXvdInformation(a, w("p").as_ptr(), 0, null(), 8) });
    assert_eq!((hr, c.len()), (EINVAL, 0));
    let (hr, _) = run(|a| unsafe { XCrdSetXvdInformation(a, w("p").as_ptr(), 0, v.as_ptr(), 7) });
    assert_eq!(hr, EINVAL);
    let (hr, _) = run(|a| unsafe { XCrdSetXvdInformation(a, w("p").as_ptr(), 12, v.as_ptr(), 8) });
    assert_eq!(hr, EINVAL);
    let (hr, c) = run(|a| unsafe { XCrdSetXvdInformation(a, w("p").as_ptr(), 3, null(), 0) });
    assert_eq!((hr, c.len()), (0, 1));
    assert_eq!(c[0].0, 0x7800_0054);
    assert_eq!(le32(&c[0].1, 8), 3);
    assert_eq!(wide_at(&c[0].1, 0x110), "p");
    let (_, c) = run(|a| unsafe { XCrdSetXvdInformation(a, w("p").as_ptr(), 11, v.as_ptr(), 8) });
    assert_eq!((le32(&c[0].1, 8), le32(&c[0].1, 0x10), le32(&c[0].1, 0x14)), (0x11, 0x5566_7788, 0x1122_3344));
    let mut e = vec![0u8; 0x54];
    e[0] = 0x34;
    e[1] = 0x12;
    e[2] = 0x56;
    e[4] = b'n';
    let (_, c) = run(|a| unsafe { XCrdSetXvdInformation(a, w("p").as_ptr(), 8, e.as_ptr(), 0x54) });
    let b = &c[0].1;
    assert_eq!(b.len(), 0x112 + 2 + 0x50);
    assert_eq!(le32(b, 0), 0x160);
    assert_eq!((le32(b, 8), b[0x10], b[0x11], b[0x12], b[0x13]), (8, 0x34, 0x12, 0x56, 0x50));
    assert_eq!(wide_at(b, 0x14), "n");
    assert_eq!(wide_at(b, 0x160), "p");
    let bits = 0xabcdu64;
    let (hr, c) = run(|a| unsafe { XCloudSetFeatureBits(a, &bits) });
    assert_eq!(hr, 0);
    assert_eq!((le32(&c[0].1, 8), le64(&c[0].1, 0x10)), (0xa, 0xabcd));
    assert_eq!(wide_at(&c[0].1, 0x110), "[XUC:]\\ANYXVC");
}

#[test]
fn query_xvd_information_payload() {
    let mut out = [0u8; 0x400];
    let (hr, c) = run(|a| unsafe { XCrdQueryXvdInformation(a, w("p").as_ptr(), 0, out.as_mut_ptr(), 0x400) });
    assert_eq!((hr, c.len()), (0, 1));
    assert_eq!(c[0].0, 0x7800_0050);
    assert_eq!(le32(&c[0].1, 0), 0x418);
    assert_eq!(le32(&c[0].1, 0xc), XVD_INFO_CODE[0]);
    assert_eq!(wide_at(&c[0].1, 0x418), "p");
}

fn xvd_probe(m: &Mock) {
    m.file("\\\\.\\Scsi0:", Ok(0x44));
    m.on_raw(|_, buf| {
        buf[4..12].copy_from_slice(&XVD_SIGNATURE);
        buf[0x14..0x18].copy_from_slice(&0i32.to_le_bytes());
        0
    });
}

unsafe fn info_text(ret: *mut u8, field: usize) -> String {
    unsafe {
        let base = ret;
        let off = (base.add(field) as *const u32).read_unaligned() as usize;
        String::from_utf16_lossy(wslice(base.add(off) as *const u16))
    }
}

#[test]
fn find_blob_on_crd_device() {
    let (mut h, mut info) = (null_mut(), null_mut());
    assert_eq!(unsafe { XCrdFindFirstBlob(&mut h, &mut info, 0, null(), 2) }, EINVAL);
    let m = Mock::new();
    xvd_probe(&m);
    let step = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let s2 = step.clone();
    m.on_xvd(move |_, code, p| {
        assert_eq!(code, 0x7800_0084);
        let k = s2.get();
        s2.set(k + 1);
        if le32(p, 0x18) == 0 {
            p[0..16].copy_from_slice(&[9u8; 16]);
        }
        let name = if k == 0 { "a.xvd" } else { "b.xvd" };
        for (i, c) in name.encode_utf16().chain([0]).enumerate() {
            p[0x20 + i * 2..0x22 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        p[0x228..0x230].copy_from_slice(&0x100u64.to_le_bytes());
        p[0x230..0x238].copy_from_slice(&0x77u64.to_le_bytes());
        p[0x348..0x34c].copy_from_slice(&0x20u32.to_le_bytes());
        0
    });
    let _g = m.install();
    assert_eq!(unsafe { XCrdFindFirstBlob(&mut h, &mut info, 0, w("dir").as_ptr(), 0) }, 0);
    unsafe {
        assert_eq!(info_text(info, 8), "\\dir\\a.xvd");
        assert_eq!(info_text(info, 0), "a.xvd");
        assert_eq!(info_text(info, 0x10), "[XTE:]\\dir\\a.xvd");
        assert_eq!(le64(core::slice::from_raw_parts(info, 0x30), 0x20), 0x100);
        assert_eq!(le64(core::slice::from_raw_parts(info, 0x30), 0x28), 0x77);
        assert_eq!(XCrdFindNextBlob(h, &mut info), 0);
        assert_eq!(info_text(info, 0x10), "[XTE:]\\dir\\b.xvd");
        assert_eq!(XCrdFindCloseBlob(h), 0);
    }
    let x = m.xvd_calls();
    let ops: Vec<u32> = x.iter().map(|(_, p)| le32(p, 0x18)).collect();
    assert_eq!(ops, vec![0, 1, 2]);
    assert_eq!(wide_at(&x[0].1, 0x20), "dir");
    assert_eq!(&x[1].1[0..16], &[9u8; 16]);
    assert!(m.calls().contains(&Call::Close(0x44)));
}

#[test]
fn find_blob_on_disc_reads_headers() {
    let (mut h, mut info) = (null_mut(), null_mut());
    let m = Mock::new();
    let _g = m.install();
    assert_eq!(unsafe { XCrdFindFirstBlob(&mut h, &mut info, 9, null(), 0) }, E_NOT_FOUND);
    drop(_g);
    let m = Mock::new();
    xvd_probe(&m);
    m.0.borrow_mut().drive = 5;
    m.find_entry("x.xvd", 0x20, 0x2000, 0x5);
    m.find_entry("sub", 0x10, 0, 0x6);
    let mut disc = vec![0u8; 0x1000];
    let hdr = &mut disc[0x200..];
    hdr[0..8].copy_from_slice(b"msft-xvd");
    hdr[0x18..0x20].copy_from_slice(&0x10_0000u64.to_le_bytes());
    hdr[0xc] = 3;
    hdr[0x84] = 0x77;
    hdr[0x271] = 5;
    m.file_content("O:\\x.xvd", 0x66, disc);
    let _g = m.install();
    unsafe {
        assert_eq!(XCrdFindFirstBlob(&mut h, &mut info, 9, null(), 1), 0);
        assert_eq!(info_text(info, 0x10), "[XOD:]\\O:\\x.xvd");
        let raw = info.sub(0x10);
        assert_eq!(rd(raw, 0x40) & 5, 5);
        let hh = (raw.add(0x28) as *const *const u8).read_unaligned();
        assert_eq!((rd(hh as *mut u8, 4), rd(hh as *mut u8, 0x10), *hh.add(0xa)), (1, 0x77, 5));
        assert_eq!(XCrdFindNextBlob(h, &mut info), 0);
        let raw = info.sub(0x10);
        assert_eq!(rd(raw, 0x40), 6);
        assert_eq!(XCrdFindNextBlob(h, &mut info), 0x8007_0012u32 as i32);
        assert_eq!(XCrdFindCloseBlob(h), 0);
    }
    let c = m.calls();
    assert!(c.contains(&Call::Create { path: "O:\\*.*".into(), access: 0, share: 0, disp: 0, flags: 0xf1 }));
    assert!(c.contains(&Call::Close(0xf1f1)));
    assert!(c.contains(&Call::Close(0x66)));
}

unsafe fn rd(p: *mut u8, off: usize) -> u32 {
    unsafe { (p.add(off) as *const u32).read_unaligned() }
}

#[test]
fn xvd_header_validation_rules() {
    let mut h = vec![0u8; 0x400];
    h[0..8].copy_from_slice(b"msft-xvd");
    h[0x18..0x20].copy_from_slice(&0x10_0000u64.to_le_bytes());
    h[0xc] = 3;
    assert_eq!(unsafe { validate_xvd_header(h.as_ptr()) }, 0);
    h[0x90] = 1;
    h[0x84] = 2;
    assert_eq!(unsafe { validate_xvd_header(h.as_ptr()) }, 0x8007_0001u32 as i32);
    h[0x84] = 1;
    assert_eq!(unsafe { validate_xvd_header(h.as_ptr()) }, 0);
    h[0x18] = 1;
    assert_eq!(unsafe { validate_xvd_header(h.as_ptr()) }, 0x8007_0001u32 as i32);
}
