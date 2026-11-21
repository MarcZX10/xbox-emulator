use crate::xcrd_a::*;
use crate::xcrd_core::*;
use crate::xcrd_driver::*;
use crate::xcrd_driver::info::{DEVICE_NAME, ST_NO_SUCH_DEVICE};
use crate::xcrd_storage::XCrdResizeXVD;
use crate::xcrd_testkit::{le32, le64, w, wide_at};
use core::ffi::c_void;
use core::ptr::{null, null_mut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

extern "system" {
    fn CreateEventW(sa: *mut c_void, manual: i32, initial: i32, name: *const u16) -> isize;
    fn CloseHandle(h: isize) -> i32;
}

struct Emu {
    drv: &'static Mutex<Driver>,
    root: PathBuf,
}

impl Drop for Emu {
    fn drop(&mut self) {
        HOST_OVERRIDE.with(|o| *o.borrow_mut() = None);
        WNF_OVERRIDE.with(|o| *o.borrow_mut() = None);
        TEST_DRIVER.with(|t| t.set(None));
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Emu {
    fn d(&self) -> std::sync::MutexGuard<'static, Driver> {
        self.drv.lock().unwrap()
    }
    fn last(&self, code: u32) -> Vec<u8> {
        self.d().log.iter().rev().find(|(c, _)| *c == code).map(|(_, p)| p.clone()).expect("request not sent")
    }
}

fn emu(tag: &str) -> Emu {
    let root = std::env::temp_dir().join(format!("xcrd_drv_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Mount")).unwrap();
    let drv: &'static Mutex<Driver> = Box::leak(Box::new(Mutex::new(Driver::new(root.join("Mount"), root.join("XcrdStore")))));
    TEST_DRIVER.with(|t| t.set(Some(drv)));
    HOST_OVERRIDE.with(|o| *o.borrow_mut() = Some(Box::new(EmuHost { drv })));
    WNF_OVERRIDE.with(|o| *o.borrow_mut() = Some(Box::new(EmuWnf { drv })));
    Emu { drv, root }
}

fn open() -> *mut c_void {
    let mut a = null_mut();
    assert_eq!(unsafe { XCrdOpenAdapter(&mut a) }, 0);
    a
}

fn create(a: *mut c_void, path: &str, id: [u8; 16], size: u64) -> i32 {
    let mut out = [0u8; 16];
    unsafe { XCrdCreateXVD(a, w(path).as_ptr(), id.as_ptr(), 0, size, 0, 0, out.as_mut_ptr(), null(), null(), 0, null()) }
}

fn write_blob(a: *mut c_void, path: &str, data: &[u8]) -> i32 {
    let mut len = data.len() as u32;
    unsafe { blob_io(a, w(path).as_ptr(), 0, data.as_ptr() as *mut u8, &mut len, 0x7800_0094, 0) }
}

fn info(a: *mut c_void, path: &str) -> (i32, [u8; 0x80]) {
    let mut out = [0u8; 0x80];
    let hr = unsafe { XCrdQueryXvdInformation(a, w(path).as_ptr(), 0, out.as_mut_ptr(), 0x80) };
    (hr, out)
}

fn prefix_index(tag: &str) -> i32 {
    let mut idx = -1;
    assert!(unsafe { XCrdParsePath(w(&format!("{tag}\\x")).as_ptr(), &mut idx) } >= 0);
    idx
}

#[test]
fn open_adapter_reaches_emulated_miniport() {
    let e = emu("open");
    let a = open();
    assert_eq!(unsafe { adapter_index(a) }, 0);
    let h = unsafe { adapter_handle(a) };
    assert_eq!(h >> 32, 0x5843);
    assert!(e.d().owns(h));
    assert_eq!(unsafe { XCrdCloseAdapter(a) }, 0);
    assert!(!e.d().owns(h));
}

#[test]
fn driver_rejects_unsigned_and_unknown_requests() {
    let e = emu("reject");
    let h = e.d().open("\\\\.\\Scsi0:").unwrap().unwrap();
    let mut m = XvdMsg::new(0x7800_0abc, 0x10);
    assert_eq!(m.send(h), nt_to_hr(ST_BAD_REQUEST));
    let mut bad = XvdMsg::new(0x7800_0004, 0x40);
    bad.buf[4..12].copy_from_slice(b"NOTMSFT!");
    assert_eq!(e.d().nt_ioctl(h, IOCTL_SCSI_MINIPORT, &mut bad.buf), Some(STATUS_INVALID_PARAMETER));
    assert_eq!(e.d().nt_ioctl(h, 0x1234, &mut [0u8; 0x40]), Some(ST_BAD_REQUEST));
}

#[test]
fn full_xvd_lifecycle_through_emulated_driver() {
    let e = emu("life");
    let a = open();
    let path = "[XTE:]\\saves\\slot.xvd";
    let id = [0x11u8; 16];
    assert_eq!(create(a, path, id, 0x1000), 0);
    assert_eq!(create(a, path, id, 0x1000), nt_to_hr(ST_COLLISION));
    let file = e.root.join("XcrdStore").join("XTE").join("saves").join("slot.xvd");
    let hdr = std::fs::read(&file).unwrap();
    assert_eq!(&hdr[0x200..0x208], b"msft-xvd");
    assert_eq!(le32(&hdr, 0x20c), 3);
    assert_eq!(le64(&hdr, 0x218), 0x100_0000);
    let (hr, inf) = info(a, path);
    assert_eq!(hr, 0);
    assert_eq!(u16::from_le_bytes([inf[0], inf[1]]), 3);
    assert_eq!(le64(&inf, 0x18), 0x100_0000);
    assert_eq!(&inf[0x20..0x30], &id);

    let mut dp: *mut DevicePath = null_mut();
    assert_eq!(unsafe { XCrdMount(&mut dp, a, w(path).as_ptr(), 0) }, 0);
    let dev = String::from_utf16_lossy(unsafe { wslice((*dp).path) });
    assert_eq!(dev, "\\\\?\\GLOBALROOT\\Device\\Harddisk64\\Partition1");
    unsafe {
        assert_eq!((*dp).disk, 0x40);
        assert_eq!((*dp).volume_number, 0x40);
        assert_eq!(((*dp).address.lun, (*dp).address.target, (*dp).address.path), (0, SCSI_TARGET, SCSI_BUS));
    }
    let mut s: *mut u16 = null_mut();
    assert_eq!(unsafe { XCrdQueryDevicePathByPath(a, w(path).as_ptr(), &mut s) }, 0);
    assert_eq!(String::from_utf16_lossy(unsafe { wslice(s) }), dev);
    unsafe { XCrdFreeDevicePathBuffer(s as *mut c_void) };
    let content = e.d().resolve_device_path(&format!("{dev}\\data\\f.bin")).unwrap();
    assert_eq!(content, PathBuf::from(format!("{}.content", file.display())).join("data").join("f.bin"));

    let data: Vec<u8> = (0..0x1800u32).map(|i| (i * 13) as u8).collect();
    assert_eq!(write_blob(a, path, &data), 0);
    let mut back = vec![0u8; 0x1800];
    let mut len = 0x1800u32;
    assert_eq!(unsafe { XCrdReadUserDataXVD(a, w(path).as_ptr(), 0, back.as_mut_ptr(), &mut len) }, 0);
    assert_eq!((len, &back), (0x1800, &data));
    let mut need = 0u32;
    assert_eq!(unsafe { XCrdReadUserDataXVD(a, w(path).as_ptr(), 0, null_mut(), &mut need) }, nt_to_hr(ST_TOO_SMALL));
    assert_eq!(need, 0x1800);
    assert_eq!(le32(&std::fs::read(&file).unwrap(), 0x28c), 0x1800);

    assert_eq!(unsafe { XCrdDeleteXVD(a, w(path).as_ptr(), 0) }, nt_to_hr(ST_SHARING));

    assert_eq!(unsafe { XCrdResizeXVD(a, w(path).as_ptr(), 0x10_0000) }, 0);
    let r = e.last(0x7800_00a0);
    let n = path.encode_utf16().count();
    assert_eq!(r.len(), n * 2 + 0x1a);
    assert_eq!(le32(&r, 0), (n * 2 + 0x1a) as u32);
    assert_eq!(le32(&r, 4), 0x18);
    assert_eq!(le32(&r, 8), (n * 2) as u32);
    assert_eq!(le64(&r, 0x10), 0x10_0000);
    assert_eq!(wide_at(&r, 0x18), path);
    assert_eq!(le64(&info(a, path).1, 0x18), 0x110_0000);
    {
        let d = e.d();
        let v = d.luns[0].as_ref().unwrap();
        assert_eq!((v.part_size, v.size), (0x110_0000, 0x110_0000));
    }
    let h = e.d().open(&dev).unwrap().unwrap();
    let mut fs = [0u8; 0x18];
    assert_eq!(e.d().fs_size(h, &mut fs), Some(0));
    assert_eq!(le64(&fs, 0), 0x110_0000 / 512);
    assert_eq!((le32(&fs, 0x10), le32(&fs, 0x14)), (1, 512));
    e.d().close(h);

    assert_eq!(unsafe { crate::xcrd2_xvd::XCrdUnmount(a, dp) }, 0);
    assert!(e.d().luns.iter().all(|l| l.is_none()));
    assert!(e.d().resolve_device_path(&dev).is_none());
    assert_eq!(unsafe { XCrdResizeXVD(a, w(path).as_ptr(), 0x1000) }, nt_to_hr(ST_NOT_FOUND));
    assert_eq!(unsafe { XCrdDeleteXVD(a, w(path).as_ptr(), 0) }, 0);
    assert!(!file.exists());
    assert_eq!(unsafe { XCrdDeleteXVD(a, w(path).as_ptr(), 0) }, nt_to_hr(ST_NOT_FOUND));
}

#[test]
fn resize_full_disk_sequence_and_limits() {
    let e = emu("resize");
    let a = open();
    let path = "[XTE:]\\r.xvd";
    assert_eq!(create(a, path, [2u8; 16], 0x1000), 0);
    let mut dp: *mut DevicePath = null_mut();
    assert_eq!(unsafe { XCrdMount(&mut dp, a, w(path).as_ptr(), 0) }, 0);
    assert_eq!(unsafe { XCrdResizeXVD(a, w(path).as_ptr(), 0) }, 0x8007_0057u32 as i32);
    assert_eq!(unsafe { XCrdResizeXVD(null_mut(), w(path).as_ptr(), 0x1000) }, 0x8007_0057u32 as i32);
    assert_eq!(unsafe { XCrdResizeXVD(a, w(path).as_ptr(), 0x1800) }, 0);
    assert_eq!(le64(&info(a, path).1, 0x18), 0x100_2000);
    let v = e.d().luns[0].clone().unwrap();
    assert_eq!(v.part_size, 0x100_1800);
    assert_eq!(v.size, 0x100_1800);
    assert_eq!(unsafe { XCrdResizeXVD(a, w(path).as_ptr(), 0x200_0000_0000) }, nt_to_hr(STATUS_INVALID_PARAMETER));
    assert_eq!(unsafe { crate::xcrd2_xvd::XCrdUnmount(a, dp) }, 0);
}

#[test]
fn read_only_package_content_mounts_but_rejects_writes() {
    let e = emu("pkg");
    std::fs::create_dir_all(e.root.join("Mount").join("dlc1")).unwrap();
    std::fs::write(e.root.join("Mount").join("dlc1").join("a.txt"), b"hello").unwrap();
    let a = open();
    let path = "[XTE:]\\dlc1";
    let mut dp: *mut DevicePath = null_mut();
    assert_eq!(unsafe { XCrdMount(&mut dp, a, w(path).as_ptr(), 0) }, 0);
    let dev = String::from_utf16_lossy(unsafe { wslice((*dp).path) });
    let f = e.d().resolve_device_path(&format!("{dev}\\a.txt")).unwrap();
    assert_eq!(std::fs::read(f).unwrap(), b"hello");
    assert!(e.d().resolve_device_path(&format!("{dev}\\..\\x")).is_none());
    assert!(e.d().luns[0].as_ref().unwrap().read_only);
    assert_eq!(unsafe { XCrdResizeXVD(a, w(path).as_ptr(), 0x1000) }, nt_to_hr(ST_ACCESS_DENIED));
    assert_eq!(unsafe { crate::xcrd2_xvd::XCrdUnmount(a, dp) }, 0);
    assert_eq!(unsafe { XCrdDeleteXVD(a, w(path).as_ptr(), 0) }, nt_to_hr(ST_ACCESS_DENIED));
    assert!(e.root.join("Mount").join("dlc1").join("a.txt").exists());
    assert_eq!(unsafe { XCrdMount(&mut dp, a, w("[XTE:]\\missing").as_ptr(), 0) }, nt_to_hr(ST_NOT_FOUND));
}

#[test]
fn mount_content_type_mismatch_is_rejected() {
    let _e = emu("ct");
    let a = open();
    let mut out = [0u8; 16];
    assert_eq!(unsafe { XCrdCreateXVD(a, w("[XTE:]\\c.xvd").as_ptr(), [3u8; 16].as_ptr(), 0, 0x1000, 0, 5, out.as_mut_ptr(), null(), null(), 0, null()) }, 0);
    let mut dp: *mut DevicePath = null_mut();
    assert_eq!(unsafe { XCrdMountContentType(&mut dp, a, w("[XTE:]\\c.xvd").as_ptr(), 6, 0) }, nt_to_hr(ST_TYPE_MISMATCH));
    assert_eq!(unsafe { XCrdMountContentType(&mut dp, a, w("[XTE:]\\c.xvd").as_ptr(), 5, 0) }, 0);
    assert_eq!(unsafe { crate::xcrd2_xvd::XCrdUnmount(a, dp) }, 0);
}

#[test]
fn read_bat_info_full_request_layout() {
    let e = emu("bat");
    let a = open();
    let path = "[XTE:]\\b.xvd";
    assert_eq!(create_typed(a, path, 0x1000, 1), 0);
    let mut buf = vec![0u8; 0x1000];
    let mut len = 0x1000u32;
    let mut inf = [0u8; 0x40];
    let hr = unsafe { XCrdReadBatInfoXVD(a, w(path).as_ptr(), buf.as_mut_ptr(), &mut len, inf.as_mut_ptr()) };
    assert_eq!(hr, 0);
    let entries = 0x100_0000u64.div_ceil(BLOCK_SIZE as u64);
    assert_eq!(len as u64, entries * 4);
    assert!(buf[..len as usize].iter().all(|&b| b == 0xff));
    let file = std::fs::read(e.root.join("XcrdStore").join("XTE").join("b.xvd")).unwrap();
    let bat_off = le64(&inf, 0);
    assert_eq!((le64(&inf, 8), le32(&inf, 0x10), le32(&inf, 0x14)), (entries, 4, BLOCK_SIZE));
    assert_eq!((le64(&inf, 0x18), le32(&inf, 0x20), le32(&inf, 0x24)), (0x100_0000, 0, 1));
    assert_eq!((le64(&inf, 0x28), le32(&inf, 0x30)), (file.len() as u64, 0x1000));
    assert_eq!(le32(&file, 0x294), 0x1000);
    assert_eq!(&file[bat_off as usize..bat_off as usize + len as usize], &buf[..len as usize]);
    let n = path.encode_utf16().count();
    let r = e.last(0x7800_002c);
    assert_eq!(r.len(), n * 2 + 0x4a + 0x1000);
    assert_eq!(le32(&r, 0x18), 0x48);
    assert_eq!(le32(&r, 0x1c), (n * 2) as u32);
    assert_eq!(le32(&r, 0x20), 0x1000);
    assert_eq!(wide_at(&r, 0x48), path);
    let mut len = 0x10u32;
    let _ = unsafe { XCrdReadBatInfoXVD(a, w(path).as_ptr(), buf.as_mut_ptr(), &mut len, inf.as_mut_ptr()) };
    assert_eq!(le32(&e.last(0x7800_002c), 0x20), 0x1000);
    assert_eq!(len as u64, entries * 4);
    let content = e.root.join("XcrdStore").join("XTE").join("b.xvd.content");
    std::fs::create_dir_all(&content).unwrap();
    std::fs::write(content.join("d.bin"), vec![1u8; BLOCK_SIZE as usize + 1]).unwrap();
    let mut len = 0x1000u32;
    assert_eq!(unsafe { XCrdReadBatInfoXVD(a, w(path).as_ptr(), buf.as_mut_ptr(), &mut len, inf.as_mut_ptr()) }, 0);
    assert_eq!(le32(&inf, 0x20), 2);
    assert_eq!((le32(&buf, 0), le32(&buf, 4), le32(&buf, 8)), (0, 1, 0xffff_ffff));
    let file = std::fs::read(e.root.join("XcrdStore").join("XTE").join("b.xvd")).unwrap();
    assert_eq!(&file[bat_off as usize..bat_off as usize + 12], &buf[..12]);
}

fn create_typed(a: *mut c_void, path: &str, pages: u64, ty: u32) -> i32 {
    let mut out = [0u8; 16];
    unsafe { XCrdCreateXVD(a, w(path).as_ptr(), [4u8; 16].as_ptr(), 0, pages, ty, 0, out.as_mut_ptr(), null(), null(), 0, null()) }
}

fn query(a: *mut c_void, path: &str, class: i32) -> (i32, Vec<u8>) {
    let mut out = vec![0u8; 0x420];
    let hr = unsafe { XCrdQueryXvdInformation(a, w(path).as_ptr(), class, out.as_mut_ptr(), 0x420) };
    (hr, out)
}

fn set(a: *mut c_void, path: &str, class: i32, data: &[u8]) -> i32 {
    unsafe { XCrdSetXvdInformation(a, w(path).as_ptr(), class, if data.is_empty() { null() } else { data.as_ptr() }, data.len() as u32) }
}

#[test]
fn bat_too_small_reports_needed_length_and_fixed_has_none() {
    let e = emu("bat2");
    let a = open();
    assert_eq!(create_typed(a, "[XTE:]\\big.xvd", 0x10_0000, 1), 0);
    let need = (0x1_0000_0000u64.div_ceil(BLOCK_SIZE as u64) * 4) as u32;
    let mut buf = vec![0u8; 0x1000];
    let mut len = 0x10u32;
    let mut inf = [0u8; 0x40];
    let hr = unsafe { XCrdReadBatInfoXVD(a, w("[XTE:]\\big.xvd").as_ptr(), buf.as_mut_ptr(), &mut len, inf.as_mut_ptr()) };
    assert_eq!(hr, nt_to_hr(ST_TOO_SMALL));
    assert_eq!(len, need);
    let mut big = vec![0u8; need as usize];
    let mut len = need;
    assert_eq!(unsafe { XCrdReadBatInfoXVD(a, w("[XTE:]\\big.xvd").as_ptr(), big.as_mut_ptr(), &mut len, inf.as_mut_ptr()) }, 0);
    assert_eq!(le32(&inf, 0x30) as u64, (need as u64).div_ceil(0x1000) * 0x1000);
    let mut dp: *mut DevicePath = null_mut();
    assert_eq!(unsafe { XCrdMount(&mut dp, a, w("[XTE:]\\big.xvd").as_ptr(), 0) }, 0);
    assert_eq!(unsafe { XCrdResizeXVD(a, w("[XTE:]\\big.xvd").as_ptr(), BLOCK_SIZE as i64) }, 0);
    assert_eq!(unsafe { crate::xcrd2_xvd::XCrdUnmount(a, dp) }, 0);
    let mut len = need + 0x1000;
    let mut big = vec![0u8; len as usize];
    assert_eq!(unsafe { XCrdReadBatInfoXVD(a, w("[XTE:]\\big.xvd").as_ptr(), big.as_mut_ptr(), &mut len, inf.as_mut_ptr()) }, 0);
    assert_eq!(le64(&inf, 8), (0x1_0000_0000u64 + BLOCK_SIZE as u64).div_ceil(BLOCK_SIZE as u64));
    assert_eq!(create_typed(a, "[XTE:]\\fix.xvd", 0x1000, 0), 0);
    let mut len = 0x1000u32;
    assert_eq!(unsafe { XCrdReadBatInfoXVD(a, w("[XTE:]\\fix.xvd").as_ptr(), buf.as_mut_ptr(), &mut len, inf.as_mut_ptr()) }, 0);
    assert_eq!((len, le64(&inf, 8), le32(&inf, 0x24), le32(&inf, 0x30)), (0, 0, 0, 0));
    let file = std::fs::read(e.root.join("XcrdStore").join("XTE").join("fix.xvd")).unwrap();
    assert_eq!(le64(&inf, 0x28), file.len() as u64);
    let mut len = 0x1000u32;
    assert_eq!(unsafe { XCrdReadBatInfoXVD(a, w("[XTE:]\\none.xvd").as_ptr(), buf.as_mut_ptr(), &mut len, inf.as_mut_ptr()) }, nt_to_hr(ST_NOT_FOUND));
}

#[test]
fn xvd_info_classes_follow_console_reply_layouts() {
    let e = emu("classes");
    let a = open();
    let path = "[XTE:]\\q.xvd";
    assert_eq!(create_typed(a, path, 0x1000, 1), 0);
    let file = e.root.join("XcrdStore").join("XTE").join("q.xvd");
    let (hr, b) = query(a, path, 0);
    assert_eq!(hr, 0);
    assert_eq!(le64(&b, 0x78), std::fs::metadata(&file).unwrap().len());
    assert_eq!(le32(&b, 4), 1);
    let (hr, b3) = query(a, path, 3);
    assert_eq!((hr, &b3[..0x80]), (0, &b[..0x80]));
    let (hr, k) = query(a, path, 1);
    assert_eq!((hr, k[0], k[1]), (0, 0, 0));
    for c in [2, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16, 17] {
        let (hr, _) = query(a, path, c);
        assert_eq!(hr, 0, "class {c}");
    }
    assert_eq!(query(a, path, 19).0, nt_to_hr(ST_NOT_SUPPORTED));
    assert_eq!(query(a, path, 20).0, nt_to_hr(ST_NOT_SUPPORTED));

    assert_eq!(set(a, path, 0, &0x0001_0002_0003_0004u64.to_le_bytes()), 0);
    assert_eq!(set(a, path, 1, &[0x77u8; 16]), 0);
    assert_eq!(set(a, path, 2, &[0x66u8; 16]), 0);
    let (hr, v) = query(a, path, 5);
    assert_eq!(hr, 0);
    assert_eq!(le64(&v, 0), 0x0001_0002_0003_0004);
    assert_eq!(&v[8..0x18], &[0x77u8; 16]);
    assert_eq!(&query(a, path, 0).1[0x68..0x78], &[0x66u8; 16]);
    assert_eq!(le64(&query(a, path, 0).1, 0x50), 0x0001_0002_0003_0004);

    let mut pls = [0u8; 8];
    pls[..4].copy_from_slice(&100u32.to_le_bytes());
    pls[4..].copy_from_slice(&200u32.to_le_bytes());
    assert_eq!(set(a, path, 11, &pls), 0);
    let (_, p) = query(a, path, 4);
    assert_eq!((le32(&p, 0), le32(&p, 4)), (100, 200));
    pls[..4].copy_from_slice(&300u32.to_le_bytes());
    assert_eq!(set(a, path, 11, &pls), nt_to_hr(STATUS_INVALID_PARAMETER));

    assert_eq!(set(a, path, 10, &0x55u64.to_le_bytes()), 0);
    assert_eq!(le64(&query(a, path, 18).1, 0), 0x55);
    assert_eq!(set(a, path, 7, &3u32.to_le_bytes()), 0);
    assert_eq!(set(a, path, 3, &[]), 0);
    assert_eq!(set(a, path, 9, &[]), 0);
    assert_eq!(set(a, path, 4, &[]), 0);
    assert_eq!(set(a, path, 5, &1u32.to_le_bytes()), 0);
    assert_eq!(set(a, path, 5, &1u32.to_le_bytes()), nt_to_hr(ST_COLLISION));
    assert_eq!(set(a, path, 6, &1u32.to_le_bytes()), 0);
    assert_eq!(set(a, path, 6, &1u32.to_le_bytes()), nt_to_hr(ST_NOT_FOUND));
    let mut srv = vec![0u8; 0x54];
    srv[..2].copy_from_slice(&443u16.to_le_bytes());
    for (i, c) in "srv".encode_utf16().enumerate() {
        srv[4 + i * 2..6 + i * 2].copy_from_slice(&c.to_le_bytes());
    }
    assert_eq!(set(a, path, 8, &srv), 0);
    let meta = std::fs::read_to_string(format!("{}.meta", file.display())).unwrap();
    for kv in ["blob_tier=3", "unsealed=1", "recreate_if_corrupt=1", "xvcs_port=443", "xvcs_server=srv", "rsm="] {
        assert!(meta.lines().any(|l| l == kv), "{kv} in {meta}");
    }

    let data: Vec<u8> = (0..0x2345u32).map(|i| i as u8).collect();
    assert_eq!(write_blob(a, path, &data), 0);
    let mut back = vec![0u8; 0x2345];
    let mut len = 0x2345u32;
    assert_eq!(unsafe { XCrdReadUserDataXVD(a, w(path).as_ptr(), 0, back.as_mut_ptr(), &mut len) }, 0);
    let (hr, c) = query(a, path, 11);
    assert_eq!(hr, 0);
    assert_eq!((le64(&c, 0), le64(&c, 0x10), le64(&c, 0x20), le64(&c, 0x28)), (1, 1, 0x2345, 0x2345));
    assert_eq!(le64(&c, 0x40), std::fs::metadata(&file).unwrap().len());
    let mut inf = [0u8; 0x40];
    let mut bl = 0x1000u32;
    let mut bat = vec![0u8; 0x1000];
    assert_eq!(unsafe { XCrdReadBatInfoXVD(a, w(path).as_ptr(), bat.as_mut_ptr(), &mut bl, inf.as_mut_ptr()) }, 0);
    let raw = std::fs::read(&file).unwrap();
    assert_eq!(&raw[0x3000..0x3000 + 0x2345], &data[..]);
    assert_eq!(le64(&inf, 0), 0x3000 + 0x3000);

    let mut dp: *mut DevicePath = null_mut();
    assert_eq!(unsafe { XCrdMount(&mut dp, a, w(path).as_ptr(), 0) }, 0);
    assert_eq!(set(a, path, 0, &1u64.to_le_bytes()), nt_to_hr(ST_SHARING));
    assert_eq!(unsafe { crate::xcrd2_xvd::XCrdUnmount(a, dp) }, 0);
    assert_eq!(query(a, "[XTE:]\\none.xvd", 0).0, nt_to_hr(ST_NOT_FOUND));
}

#[test]
fn crd_information_classes_report_store_volume() {
    let _e = emu("crd");
    let a = open();
    let mut out = vec![0u8; 0x670];
    assert_eq!(unsafe { XCrdQueryCrdInformation(a, 1, 0, out.as_mut_ptr(), 0x10) }, 0);
    let (total, free) = (le64(&out, 0), le64(&out, 8));
    assert!(total > 0 && free <= total);
    assert_eq!(unsafe { XCrdQueryCrdInformation(a, 1, 1, out.as_mut_ptr(), 0x670) }, 0);
    assert_eq!(le64(&out, 0), total);
    assert_eq!(le64(&out, 0x10), total);
    assert_eq!((le32(&out, 0x18), le32(&out, 0x1c)), (0, 1));
    assert_eq!((le32(&out, 0x34), le32(&out, 0x34 + 6 * 4)), (1, 1));
    assert_eq!(wide_at(&out, 0x54), DEVICE_NAME);
    assert_eq!(u16::from_le_bytes([out[0x25c], out[0x25d]]), 0);
    out.fill(0xcc);
    assert_eq!(unsafe { XCrdQueryCrdInformation(a, 1, 2, out.as_mut_ptr(), 0x10) }, 0);
    assert_eq!((out[0], out[6], le32(&out, 8)), (0, 0, 0));
    assert_eq!(unsafe { XCrdQueryCrdInformation(a, 7, 0, out.as_mut_ptr(), 0x10) }, nt_to_hr(ST_NO_SUCH_DEVICE));
    let mut log = [0u8; 0x200];
    assert_eq!(unsafe { XCrdQueryCrdLogInformation(a, 1, 2, 0, 0, log.as_mut_ptr(), 0x200) }, nt_to_hr(ST_NOT_SUPPORTED));
}

#[test]
fn query_crd_vendor_information_full_request_layout() {
    let e = emu("vendor");
    let a = open();
    let mut buf = [0u8; 0x20];
    let hr = unsafe { XCrdQueryCrdVendorInformation(a, 3, 7, 0x1234, 0x55, 0x66, buf.as_mut_ptr(), 0x20) };
    assert_eq!(hr, nt_to_hr(ST_NOT_SUPPORTED));
    let r = e.last(0x7800_0050);
    assert_eq!(r.len(), 0x438);
    assert_eq!((le32(&r, 0), le32(&r, 4), le32(&r, 8), le32(&r, 0xc)), (0, 0, 3, 0x11));
    assert_eq!(r[0x10], 7);
    assert_eq!(le32(&r, 0x14), 0x1234);
    assert_eq!(u16::from_le_bytes([r[0x18], r[0x19]]), 0x55);
    assert_eq!(u16::from_le_bytes([r[0x1a], r[0x1b]]), 0x66);
    assert_eq!((le32(&r, 0x1c), le32(&r, 0x20)), (0x408, 0x20));
    assert_eq!(unsafe { XCrdQueryCrdVendorInformation(a, 0x25, 7, 0, 0, 0, buf.as_mut_ptr(), 0x20) }, 0x8007_0057u32 as i32);
}

#[test]
fn blob_enumeration_merges_store_and_package() {
    let e = emu("enum");
    let a = open();
    assert_eq!(write_blob(a, "[XUC:]\\dir\\a.bin", b"aaaa"), 0);
    assert_eq!(write_blob(a, "[XUC:]\\dir\\b.bin", b"bb"), 0);
    std::fs::create_dir_all(e.root.join("Mount").join("dir")).unwrap();
    std::fs::write(e.root.join("Mount").join("dir").join("c.bin"), b"c").unwrap();
    std::fs::write(e.root.join("Mount").join("dir").join("a.bin"), b"shadowed").unwrap();
    let dev = prefix_index("[XUC:]");
    let mut h: *mut u8 = null_mut();
    let mut inf: *mut u8 = null_mut();
    assert_eq!(unsafe { XCrdFindFirstBlob(&mut h, &mut inf, dev, w("dir").as_ptr(), 0) }, 0);
    let name = |h: *mut u8| String::from_utf16_lossy(unsafe { wslice(h.add(0x48) as *const u16) });
    let size = |h: *mut u8| unsafe { (h.add(0x250) as *const u64).read_unaligned() };
    let mut got = vec![(name(h), size(h))];
    loop {
        let hr = unsafe { XCrdFindNextBlob(h, &mut inf) };
        if hr < 0 {
            assert_eq!(hr, nt_to_hr(ST_NO_MORE_FILES));
            break;
        }
        got.push((name(h), size(h)));
    }
    assert_eq!(got, vec![("a.bin".to_string(), 4), ("b.bin".to_string(), 2), ("c.bin".to_string(), 1)]);
    assert_eq!(unsafe { XCrdFindCloseBlob(h) }, 0);
    let r = e.last(0x7800_0084);
    assert_eq!(le32(&r, 0x18), 2);
    let firsts: Vec<(u32, String)> = e.d().log.iter().filter(|(c, p)| *c == 0x7800_0084 && le32(p, 0x18) == 0).map(|(_, p)| (le32(p, 0x10), wide_at(p, 0x20))).collect();
    assert_eq!(firsts, vec![(dev as u32, "dir".to_string())]);
}

#[test]
fn storage_device_enumeration_lists_internal_device() {
    let _e = emu("devs");
    let mut f: *mut StorageFind = null_mut();
    let mut dev = 0u32;
    assert_eq!(unsafe { XCrdFindFirstStorageDevice(&mut f, &mut dev) }, 0);
    assert_eq!(dev, 1);
    assert_eq!(unsafe { XCrdFindNextStorageDevice(f, &mut dev) }, nt_to_hr(ST_NO_MORE_ENTRIES));
    assert_eq!(unsafe { XCrdFindCloseStorageDevice(f) }, 0);
}

#[test]
fn cloud_feature_bits_persist_in_store() {
    let e = emu("bits");
    let a = open();
    let mut v = 7u64;
    assert_eq!(unsafe { XCloudQueryFeatureBits(a, &mut v) }, 0);
    assert_eq!(v, 0);
    assert_eq!(unsafe { XCloudSetFeatureBits(a, &0x8000_0000_0000_0055u64) }, 0);
    assert_eq!(unsafe { XCloudQueryFeatureBits(a, &mut v) }, 0);
    assert_eq!(v, 0x8000_0000_0000_0055);
    assert_eq!(e.d().global_features(), 0x8000_0000_0000_0055);
}

static CORRUPT_HITS: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn on_corrupt(_ctx: *mut c_void, _v: u32) {
    CORRUPT_HITS.fetch_add(1, Ordering::SeqCst);
}

#[test]
fn corrupt_xvd_mount_fires_corruption_callback() {
    let e = emu("corrupt");
    let a = open();
    let dir = e.root.join("XcrdStore").join("XTE");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bad.xvd"), vec![0x5au8; 0x1000]).unwrap();
    let mut reg: *mut c_void = null_mut();
    assert_eq!(unsafe { XCrdRegisterCorruptionCallback(Some(on_corrupt), &mut reg) }, 0);
    let before = CORRUPT_HITS.load(Ordering::SeqCst);
    let mut dp: *mut DevicePath = null_mut();
    assert_eq!(unsafe { XCrdMount(&mut dp, a, w("[XTE:]\\bad.xvd").as_ptr(), 0) }, nt_to_hr(ST_CORRUPT));
    assert_eq!(CORRUPT_HITS.load(Ordering::SeqCst), before + 1);
    assert_eq!(unsafe { XCrdUnregisterCorruptionCallback(reg) }, 0);
    assert_eq!(unsafe { XCrdMount(&mut dp, a, w("[XTE:]\\bad.xvd").as_ptr(), 0) }, nt_to_hr(ST_CORRUPT));
    assert_eq!(CORRUPT_HITS.load(Ordering::SeqCst), before + 1);
}

#[test]
fn control_page_ioctl_returns_shared_flags_and_page() {
    let e = emu("page");
    let h = e.d().open("\\\\.\\Scsi0:").unwrap().unwrap();
    let mut out = [0u8; 0x10];
    assert_eq!(e.d().nt_ioctl(h, CONTROL_PAGE_IOCTL, &mut out), Some(0));
    let (flags, page) = (le64(&out, 0) as usize, le64(&out, 8) as usize);
    let want = {
        let d = e.d();
        (d.control_flags, d.control_page)
    };
    assert_eq!((flags, page), want);
    assert_ne!(page, 0);
    assert!(unsafe { core::slice::from_raw_parts(page as *const u8, 0x1000) }.iter().all(|&b| b == 0));
    assert_eq!(e.d().nt_ioctl(h, CONTROL_PAGE_IOCTL, &mut [0u8; 8]), Some(ST_TOO_SMALL));
}

#[test]
fn dstorage_factory_maps_control_page_and_debug_flags() {
    let mut f: *mut c_void = null_mut();
    let iid: [u8; 16] = [0; 16];
    let _ = unsafe { crate::DStorageGetFactory(iid.as_ptr() as *const c_void, &mut f) };
    let c = control_page().expect("factory init must map the control page");
    let mut p = null_mut();
    assert_eq!(unsafe { DStorageGetMappedControlPage(&mut p) }, 0);
    assert_eq!(p as usize, c.page);
    unsafe { (crate::objects::FACTORY_VTBL.set_debug_flags)(crate::objects::factory_ptr(), 0x5a) };
    assert_eq!(unsafe { (c.flags as *const u32).read_volatile() }, 0x5a);
}

#[test]
fn xvd_xfer_initialize_start_read_through_driver() {
    let _l = crate::xcrd2_tests::HSV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let e = emu("xfer");
    let pipe = format!("\\\\.\\pipe\\xcrd-drv-xfer-{}", std::process::id());
    crate::xcrd2_xfer::PIPE_NAME.with(|p| *p.borrow_mut() = Some(w(&pipe)));
    crate::xcrd2_hsv::set_store(Some(std::sync::Arc::new(DriverStore { drv: e.drv })));
    let a = open();
    let data: Vec<u8> = (0..3000u32).map(|i| (i * 5 + 1) as u8).collect();
    assert_eq!(write_blob(a, "[XUC:]\\x\\game.bin", &data), 0);
    let mut ctx: *mut crate::xcrd2_xfer::XferCtx = null_mut();
    assert_eq!(unsafe { crate::xcrd2_xfer::XCrdXvdXferInitialize(a, &mut ctx) }, 0);
    let s = e.last(crate::xcrd2_hsv::XFER_SESSION_MSG);
    assert_ne!(s[4], 0);
    assert_eq!(unsafe { crate::xcrd2_xfer::XCrdXvdXferStart(ctx, w("[XUC:]\\x\\game.bin").as_ptr()) }, 0);
    let ev = unsafe { CreateEventW(null_mut(), 1, 0, null()) };
    let mut ov = crate::xcrd2_xfer::Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: ev };
    let mut buf = vec![0u8; 1000];
    assert_eq!(unsafe { crate::xcrd2_xfer::XCrdXvdXferRead(ctx, 2500, 1000, buf.as_mut_ptr(), &mut ov) }, crate::xcrd2_xfer::HR_IO_PENDING);
    let mut n = 0u32;
    assert_eq!(unsafe { crate::xcrd2_xfer::XCrdXvdXferGetResult(ctx, &mut ov, &mut n) }, 0);
    assert_eq!(n, 500);
    assert_eq!(&buf[..500], &data[2500..]);
    assert_eq!(unsafe { crate::xcrd2_xfer::XCrdXvdXferStop(ctx) }, 0);
    let session = unsafe { (*ctx).session };
    let mut c = ctx;
    assert_eq!(unsafe { crate::xcrd2_xfer::XCrdXvdXferUninitialize(a, &mut c) }, 0);
    assert!(!crate::xcrd2_hsv::session_open(session));
    let close = e.last(crate::xcrd2_hsv::XFER_SESSION_MSG);
    assert_eq!(close[4], 0);
    crate::xcrd2_hsv::set_store(None);
    crate::xcrd2_xfer::PIPE_NAME.with(|p| *p.borrow_mut() = None);
    unsafe { CloseHandle(ev) };
}

#[test]
fn xfer_store_maps_crd_paths_to_backing_files() {
    use crate::xcrd2_hsv::XferStore;
    let e = emu("xstore");
    std::fs::write(e.root.join("Mount").join("pkg.bin"), b"0123456789").unwrap();
    std::fs::create_dir_all(e.root.join("Mount").join("pdir")).unwrap();
    let s = DriverStore { drv: e.drv };
    let mut b = s.open("[XTE:]\\pkg.bin").ok().unwrap();
    let mut buf = [0u8; 4];
    assert_eq!(b.read_at(6, &mut buf), Ok(4));
    assert_eq!(&buf, b"6789");
    assert_eq!(b.read_at(10, &mut buf), Ok(0));
    assert_eq!(s.open("[XTE:]\\pdir").err(), Some(crate::xcrd2_hsv::STATUS_INVALID_DEVICE_REQUEST));
    assert_eq!(s.open("[XTE:]\\nope").err(), Some(crate::xcrd2_hsv::STATUS_OBJECT_NAME_NOT_FOUND));
    assert_eq!(s.open("C:\\x").err(), Some(crate::xcrd2_hsv::STATUS_OBJECT_NAME_INVALID));
}

#[test]
fn mount_points_and_volume_ioctls_follow_console_layout() {
    let e = emu("mmgr");
    let a = open();
    assert_eq!(create(a, "[XTE:]\\m.xvd", [9u8; 16], 0x1000), 0);
    let mut dp: *mut DevicePath = null_mut();
    assert_eq!(unsafe { XCrdMount(&mut dp, a, w("[XTE:]\\m.xvd").as_ptr(), 0) }, 0);
    let mut d = e.d();
    let mm = d.open("\\\\.\\MountPointManager").unwrap().unwrap();
    let mut small = [0u8; 8];
    let mut ret = 0u32;
    assert_eq!(d.ioctl(mm, 0x6d_0008, &[], &mut small, &mut ret), Err(0xea));
    let need = le32(&small, 0) as usize;
    let mut big = vec![0u8; need];
    assert_eq!(d.ioctl(mm, 0x6d_0008, &[], &mut big, &mut ret), Ok(()));
    assert_eq!(le32(&big, 4), 1);
    let off = le32(&big, 8) as usize;
    let len = u16::from_le_bytes([big[0xc], big[0xd]]) as usize;
    assert_eq!(len, 0x60);
    let link: String = String::from_utf16_lossy(&big[off..off + len].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>());
    assert_eq!(link, "\\??\\Volume{5843524b-0000-0000-0000-000000000000}");
    let vol = d.open("\\\\?\\Volume{5843524b-0000-0000-0000-000000000000}").unwrap().unwrap();
    let mut ext = [0u8; 0x20];
    assert_eq!(d.ioctl(vol, 0x56_0000, &[], &mut ext, &mut ret), Ok(()));
    assert_eq!((le32(&ext, 0), le32(&ext, 8), le64(&ext, 0x18)), (1, 0x40, 0x100_0000));
    let disk = d.open("\\\\.\\PhysicalDrive64").unwrap().unwrap();
    let mut desc = [0u8; 0x28];
    assert_eq!(d.ioctl(disk, 0x2d_1400, &[0u8; 0xc], &mut desc, &mut ret), Ok(()));
    assert_eq!(le32(&desc, 0x1c), 0xe);
    let mut sa = [0u8; 8];
    assert_eq!(d.ioctl(disk, 0x4_1018, &[], &mut sa, &mut ret), Ok(()));
    assert_eq!(&sa[4..8], &[0, SCSI_BUS, SCSI_TARGET, 0]);
    assert!(d.open("\\\\.\\PhysicalDrive65").unwrap().is_err());
    assert!(d.open("\\\\.\\PhysicalDrive3").is_none());
    drop(d);
    assert_eq!(unsafe { crate::xcrd2_xvd::XCrdUnmount(a, dp) }, 0);
    assert!(e.d().open("\\\\.\\PhysicalDrive64").unwrap().is_err());
}

#[test]
fn split_path_rejects_traversal_and_unknown_tags() {
    assert_eq!(split_xcrd_path("[XTE:]\\a\\b"), Some((prefix_index("[XTE:]") as usize, "a\\b".to_string())));
    assert!(split_xcrd_path("[XTE:]\\..\\b").is_none());
    assert!(split_xcrd_path("[ZZZ:]\\b").is_none());
    assert!(split_xcrd_path("C:\\b").is_none());
}

#[test]
fn resolve_path_export_reports_required_length() {
    let mut out = [0u16; 4];
    let p = w("\\\\?\\GLOBALROOT\\Device\\Harddisk64\\Partition1\\x");
    assert_eq!(unsafe { TomBoxXcrdResolvePath(null(), out.as_mut_ptr(), 4) }, 0);
    let r = unsafe { TomBoxXcrdResolvePath(p.as_ptr(), out.as_mut_ptr(), 4) };
    let mounted = global().lock().unwrap().resolve_device_path("\\\\?\\GLOBALROOT\\Device\\Harddisk64\\Partition1\\x");
    match mounted {
        Some(m) => assert_eq!(r as usize, m.to_string_lossy().encode_utf16().count() + 1),
        None => assert_eq!(r, 0),
    }
}

#[test]
fn title_file_io_resolves_xcrd_device_and_volume_paths() {
    extern "system" {
        fn CreateFileW(n: *const u16, a: u32, s: u32, sa: *mut c_void, d: u32, f: u32, t: isize) -> isize;
        fn ReadFile(h: isize, b: *mut u8, n: u32, r: *mut u32, o: *mut c_void) -> i32;
    }
    use compat_common::gdk::{fs::translate, xcrd_paths};
    xcrd_paths::register(TomBoxXcrdResolvePath as usize);
    let a = open();
    let path = format!(r"[XTE:]\title_io_{}.xvd", std::process::id());
    assert_eq!(create(a, &path, [0x21u8; 16], 0x1000), 0);
    let mut dp: *mut DevicePath = null_mut();
    assert_eq!(unsafe { XCrdMount(&mut dp, a, w(&path).as_ptr(), 0) }, 0);
    let dev = String::from_utf16_lossy(unsafe { wslice((*dp).path) });
    let lun = unsafe { (*dp).disk } - DISK_BASE;
    let root = global().lock().unwrap().resolve_device_path(&dev).unwrap();
    std::fs::create_dir_all(root.join("save")).unwrap();
    std::fs::write(root.join("save").join("slot0.dat"), b"title-bytes").unwrap();
    let want = root.join("save").join("slot0.dat").to_string_lossy().into_owned();
    let vol = format!(r"\\?\Volume{{5843524b-0000-0000-0000-0000000000{lun:02x}}}");
    let nt = dev.replacen(r"\\?\", r"\??\", 1);
    for p in [format!(r"{dev}\save\slot0.dat"), format!("{dev}/save/slot0.dat"), format!(r"{vol}\save\slot0.dat"), format!(r"{nt}\save\slot0.dat")] {
        assert_eq!(translate(&p).as_deref(), Some(want.as_str()), "{p}");
    }
    assert_eq!(translate(&format!(r"{dev}\..\x")), None);
    assert_eq!(translate(&format!(r"{dev}\save\slot0.dat:ads")), None);
    assert_eq!(translate(&format!(r"{dev}0\save")), None);
    let t = translate(&format!(r"{dev}\save\slot0.dat")).unwrap();
    let h = unsafe { CreateFileW(w(&t).as_ptr(), 0x8000_0000, 1, null_mut(), 3, 0x80, 0) };
    assert_ne!(h, -1);
    let mut buf = [0u8; 32];
    let mut n = 0u32;
    assert_ne!(unsafe { ReadFile(h, buf.as_mut_ptr(), 32, &mut n, null_mut()) }, 0);
    unsafe { CloseHandle(h) };
    assert_eq!(&buf[..n as usize], b"title-bytes");
    extern "system" {
        fn GetModuleHandleW(n: *const u16) -> *mut c_void;
        fn GetProcAddress(m: *mut c_void, n: *const u8) -> *mut c_void;
    }
    let k32 = unsafe { GetModuleHandleW(w("kernel32.dll").as_ptr()) };
    let hooks = compat_common::gdk::fs::targets();
    assert!(!hooks.is_empty());
    let raw = w(&format!(r"{dev}\save\slot0.dat"));
    for (name, args) in [("CreateFileW", 7usize), ("CreateFile2", 5)] {
        let (_, hook, orig) = *hooks.iter().find(|r| r.0 == name).unwrap();
        let real = unsafe { GetProcAddress(k32, format!("{name}\0").as_ptr()) } as usize;
        assert_ne!(real, 0);
        orig.store(real, std::sync::atomic::Ordering::Release);
        let h = unsafe {
            if args == 7 {
                core::mem::transmute::<usize, unsafe extern "system" fn(*const u16, u32, u32, *mut c_void, u32, u32, isize) -> isize>(hook)(raw.as_ptr(), 0x8000_0000, 1, null_mut(), 3, 0x80, 0)
            } else {
                core::mem::transmute::<usize, unsafe extern "system" fn(*const u16, u32, u32, u32, *mut c_void) -> isize>(hook)(raw.as_ptr(), 0x8000_0000, 1, 3, null_mut())
            }
        };
        assert_ne!(h, -1, "{name}");
        let mut buf = [0u8; 32];
        let mut n = 0u32;
        assert_ne!(unsafe { ReadFile(h, buf.as_mut_ptr(), 32, &mut n, null_mut()) }, 0);
        unsafe { CloseHandle(h) };
        assert_eq!(&buf[..n as usize], b"title-bytes", "{name}");
    }
    assert_eq!(unsafe { crate::xcrd2_xvd::XCrdUnmount(a, dp) }, 0);
    assert_eq!(translate(&format!(r"{dev}\save\slot0.dat")), None);
    assert_eq!(unsafe { XCrdDeleteXVD(a, w(&path).as_ptr(), 0) }, 0);
}
