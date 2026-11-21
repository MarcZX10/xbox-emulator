use crate::xcrd2_storage::*;
use crate::xcrd2_stream::*;
use crate::xcrd2_xfer::*;
use crate::xcrd2_xvd::*;
use crate::xcrd_core::*;
use crate::xcrd_testkit::*;
use core::ffi::c_void;
use core::ptr::{null, null_mut};

fn mock(f: impl FnMut(Handle, u32, &mut [u8]) -> i32 + 'static) -> (Mock, Guard) {
    let m = Mock::new();
    m.on_xvd(f);
    let g = m.install();
    (m, g)
}

fn ok_mock() -> (Mock, Guard) {
    mock(|_, _, _| 0)
}

fn io_err() -> i32 {
    nt_to_hr(STATUS_IO_DEVICE_ERROR)
}

fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

fn put_wide(b: &mut [u8], o: usize, s: &str) {
    for (i, c) in s.encode_utf16().enumerate() {
        b[o + i * 2..o + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }
}

fn wstring(p: *const u16) -> String {
    String::from_utf16_lossy(unsafe { wslice(p) })
}

#[test]
fn storage_attach_external_device_packs_address() {
    let (m, _g) = mock(|_, _, p| {
        p[0x24..0x28].copy_from_slice(&[1, 2, 3, 4]);
        0
    });
    let a = adapter(0);
    let mut out = 0u64;
    assert_eq!(unsafe { XCrdStorageAttachExternalDevice(&mut out, a, 0xc, 1) }, 0);
    assert_eq!(out, 0x0403_0201 | (0xcu64 << 32));
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_0088);
    assert_eq!(c[0].1.len(), STORAGE_REPLY);
    assert_eq!(le32(&c[0].1, 0), 0xa);
    assert_eq!(le32(&c[0].1, 4), 0xc);
    assert_eq!(le64(&c[0].1, 0x20), 1);
    assert_eq!(unsafe { XCrdStorageAttachExternalDevice(&mut out, a, 0x30, 0) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdStorageAttachExternalDevice(&mut out, null_mut(), 0xc, 0) }, E_INVALIDARG);
}

#[test]
fn storage_detach_external_device_sends_detach_even_if_disk_unknown() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let addr = 0x0403_0201u64 | (0xdu64 << 32);
    assert_eq!(unsafe { XCrdStorageDetachExternalDevice(a, addr) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c.len(), 1);
    assert_eq!(le32(&c[0].1, 0), 0xb);
    assert_eq!(le32(&c[0].1, 4), 0xd);
    assert_eq!(unsafe { XCrdStorageDetachExternalDevice(a, 5u64 << 32) }, E_INVALIDARG);
}

#[test]
fn storage_query_external_device_address_unpacks() {
    let mut out = [0u8; 8];
    assert_eq!(unsafe { XCrdStorageQueryExternalDeviceAddress(out.as_mut_ptr(), 0x0403_0201) }, 0);
    assert_eq!(out, [8, 0, 0, 0, 4, 3, 2, 1]);
}

#[test]
fn storage_check_requires_internal_device_and_maps_flags() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    assert_eq!(unsafe { XCrdStorageCheck(a, 1, 3) }, 0);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0), 0xd);
    assert_eq!(le32(&c[0].1, 4), 1);
    assert_eq!(le32(&c[0].1, 0x20), 3);
    assert_eq!(unsafe { XCrdStorageCheck(a, 0xc, 1) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdStorageCheck(a, 1, 4) }, E_INVALIDARG);
}

#[test]
fn storage_connect_device_sends_volume() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let v = w("\\\\?\\Volume{1}");
    assert_eq!(unsafe { XCrdStorageConnectDevice(a, v.as_ptr()) }, 0);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0), 8);
    assert_eq!(le32(&c[0].1, 4), 0x25);
    assert_eq!(wide_at(&c[0].1, 0x20), "\\\\?\\Volume{1}");
    assert_eq!(unsafe { XCrdStorageConnectDevice(a, null()) }, E_INVALIDARG);
}

#[test]
fn storage_disconnect_device_returns_volume_or_reconnects() {
    let (m, _g) = mock(|_, _, p| {
        if le32(p, 0) == 7 {
            put_wide(p, 0x20, "VOL");
        }
        0
    });
    let a = adapter(0);
    let mut buf = [0u16; 8];
    let mut n = 8u32;
    assert_eq!(unsafe { XCrdStorageDisconnectDevice(a, 0x14, buf.as_mut_ptr(), &mut n) }, 0);
    assert_eq!(n, 4);
    assert_eq!(wstring(buf.as_ptr()), "VOL");
    let mut small = 2u32;
    assert_eq!(unsafe { XCrdStorageDisconnectDevice(a, 0x14, buf.as_mut_ptr(), &mut small) }, HR_INSUFFICIENT);
    assert_eq!(small, 4);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[2].1, 0), 8);
    assert_eq!(wide_at(&c[2].1, 0x20), "VOL");
    assert_eq!(unsafe { XCrdStorageDisconnectDevice(a, 0x30, buf.as_mut_ptr(), &mut n) }, E_INVALIDARG);
}

#[test]
fn storage_format_rejects_internal_guid_and_returns_id() {
    let (m, _g) = mock(|_, _, p| {
        p[0xc..0x1c].copy_from_slice(&[9u8; 16]);
        0
    });
    let a = adapter(0);
    let name = w("Drive");
    let mut id = [0x11u8; 16];
    assert_eq!(unsafe { XCrdStorageFormat(a, 0xb, 1, name.as_ptr(), id.as_mut_ptr()) }, 0);
    assert_eq!(id, [9u8; 16]);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0), 0xc);
    assert_eq!(wide_at(&c[0].1, 0x20), "Drive");
    let mut internal = INTERNAL_DEVICES[0].0;
    assert_eq!(unsafe { XCrdStorageFormat(a, 0xb, 0, name.as_ptr(), internal.as_mut_ptr()) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdStorageFormat(a, 0xb, 2, name.as_ptr(), id.as_mut_ptr()) }, E_INVALIDARG);
}

#[test]
fn storage_create_blob_builds_descriptor() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let p = w("[XTE:]\\blob");
    assert_eq!(unsafe { XCrdStorageCreateBlob(a, p.as_ptr(), 0x1234, 0) }, 0);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0), 0xe);
    assert_eq!(le32(&c[0].1, 4), 0x25);
    assert_eq!(wide_at(&c[0].1, 0x20), "[XTE:]\\blob");
    assert_eq!(le64(&c[0].1, 0x20 + 0x208), 0x1234);
    assert_eq!(unsafe { XCrdStorageCreateBlob(a, p.as_ptr(), 0, 1) }, E_INVALIDARG);
}

#[test]
fn storage_create_directory_sends_op_0x12() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let p = w("dir");
    assert_eq!(unsafe { XCrdStorageCreateDirectory(a, p.as_ptr(), 0) }, 0);
    assert_eq!(le32(&m.xvd_calls()[0].1, 0), 0x12);
    assert_eq!(wide_at(&m.xvd_calls()[0].1, 0x20), "dir");
    assert_eq!(unsafe { XCrdStorageCreateDirectory(a, null(), 0) }, E_INVALIDARG);
}

#[test]
fn storage_delete_blob_sends_op_9() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let p = w("b");
    assert_eq!(unsafe { XCrdStorageDeleteBlob(a, p.as_ptr()) }, 0);
    assert_eq!(le32(&m.xvd_calls()[0].1, 0), 9);
    assert_eq!(unsafe { XCrdStorageDeleteBlob(a, null()) }, E_INVALIDARG);
}

#[test]
fn storage_delete_directory_sends_op_0x13() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let p = w("d");
    assert_eq!(unsafe { XCrdStorageDeleteDirectory(a, p.as_ptr()) }, 0);
    assert_eq!(le32(&m.xvd_calls()[0].1, 0), 0x13);
}

#[test]
fn storage_move_blob_packs_both_paths() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let s = w("src");
    let d = w("dst");
    assert_eq!(unsafe { XCrdStorageMoveBlob(a, s.as_ptr(), d.as_ptr(), 0) }, 0);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0), 0x15);
    assert_eq!(wide_at(&c[0].1, 0x20), "src");
    assert_eq!(wide_at(&c[0].1, 0x228), "dst");
    assert_eq!(unsafe { XCrdStorageMoveBlob(a, s.as_ptr(), d.as_ptr(), 1) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdStorageMoveBlob(a, s.as_ptr(), null(), 0) }, E_INVALIDARG);
}

#[test]
fn storage_query_blob_attributes_copies_reply() {
    let (m, _g) = mock(|_, _, p| {
        for i in 0..0x34 {
            p[0x10 + i] = i as u8 + 1;
        }
        0
    });
    let a = adapter(0);
    let p = w("x");
    let mut out = [0u8; 0x34];
    assert_eq!(unsafe { XCrdStorageQueryBlobAttributes(a, p.as_ptr(), out.as_mut_ptr()) }, 0);
    assert_eq!(out[0], 1);
    assert_eq!(out[0x33], 0x34);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_00c4);
    assert_eq!(le32(&c[0].1, 4), 0x48);
    assert_eq!(wide_at(&c[0].1, 0x48), "x");
    assert_eq!(unsafe { XCrdStorageQueryBlobAttributes(a, p.as_ptr(), null_mut()) }, E_INVALIDARG);
}

#[test]
fn storage_read_blob_reads_from_offset_zero() {
    let (m, _g) = mock(|_, _, p| {
        let at = 0x20 + 4;
        p[at..at + 4].copy_from_slice(b"DATA");
        0
    });
    let a = adapter(0);
    let p = w("b");
    let mut buf = [0u8; 4];
    let mut n = 4u32;
    assert_eq!(unsafe { XCrdStorageReadBlob(a, p.as_ptr(), buf.as_mut_ptr(), &mut n) }, 0);
    assert_eq!(&buf, b"DATA");
    assert_eq!(n, 4);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_0090);
    assert_eq!(le64(&c[0].1, 0x10), 0);
}

#[test]
fn storage_read_blob_ex_chunks_large_reads() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let p = w("b");
    let mut buf = vec![0u8; BLOB_CHUNK as usize + 0x10];
    let mut n = buf.len() as u32;
    assert_eq!(unsafe { XCrdStorageReadBlobEx(a, p.as_ptr(), 0x100, buf.as_mut_ptr(), &mut n) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c.len(), 2);
    assert_eq!(le64(&c[0].1, 0x10), 0x100);
    assert_eq!(le32(&c[0].1, 0xc), BLOB_CHUNK);
    assert_eq!(le64(&c[1].1, 0x10), 0x100 + BLOB_CHUNK as u64);
    assert_eq!(le32(&c[1].1, 0xc), 0x10);
    assert_eq!(n, BLOB_CHUNK + 0x10);
    let mut z = 0u32;
    assert_eq!(unsafe { XCrdStorageReadBlobEx(a, p.as_ptr(), 0, buf.as_mut_ptr(), &mut z) }, E_INVALIDARG);
}

#[test]
fn storage_write_blob_sets_truncate() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let p = w("b");
    let data = *b"abcd";
    assert_eq!(unsafe { XCrdStorageWriteBlob(a, p.as_ptr(), data.as_ptr(), 4) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_0094);
    assert_eq!(le32(&c[0].1, 0x18), 1);
    assert_eq!(&c[0].1[0x24..0x28], b"abcd");
}

#[test]
fn storage_write_blob_ex_truncates_only_last_chunk() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let p = w("b");
    let data = vec![7u8; BLOB_CHUNK as usize + 2];
    assert_eq!(unsafe { XCrdStorageWriteBlobEx(a, p.as_ptr(), 8, data.as_ptr(), data.len() as u32, 3) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c.len(), 2);
    assert_eq!(le32(&c[0].1, 0x18), 2);
    assert_eq!(le32(&c[1].1, 0x18), 3);
    assert_eq!(le64(&c[1].1, 0x10), 8 + BLOB_CHUNK as u64);
    assert_eq!(unsafe { XCrdStorageWriteBlobEx(a, p.as_ptr(), 0, data.as_ptr(), 4, 4) }, E_INVALIDARG);
}

#[test]
fn storage_remap_device_validates_ids() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    assert_eq!(unsafe { XCrdStorageRemapDevice(a, 0xb, 0x13) }, 0);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0), 5);
    assert_eq!(le32(&c[0].1, 4), 0xb);
    assert_eq!(le32(&c[0].1, 8), 0x13);
    assert_eq!(unsafe { XCrdStorageRemapDevice(a, 1, 0x13) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdStorageRemapDevice(a, 0xb, 0x40) }, E_INVALIDARG);
}

#[test]
fn storage_set_console_association_requires_external() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    assert_eq!(unsafe { XCrdStorageSetConsoleAssociation(a, 0x13, 2) }, 0);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0), 0x14);
    assert_eq!(le32(&c[0].1, 0x20), 2);
    assert_eq!(unsafe { XCrdStorageSetConsoleAssociation(a, 0x13, 3) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdStorageSetConsoleAssociation(a, 0xb, 1) }, E_INVALIDARG);
}

#[test]
fn storage_set_device_name_limits_length() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let n = w("Name");
    assert_eq!(unsafe { XCrdStorageSetDeviceName(a, 0xb, n.as_ptr()) }, 0);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0), 2);
    assert_eq!(wide_at(&c[0].1, 0x20), "Name");
    let long = w(&"x".repeat(0x21));
    assert_eq!(unsafe { XCrdStorageSetDeviceName(a, 0xb, long.as_ptr()) }, E_INVALIDARG);
}

#[test]
fn storage_set_last_used_console_sends_op_3() {
    let (m, _g) = mock(|_, _, _| STATUS_IO_DEVICE_ERROR);
    let a = adapter(0);
    assert_eq!(unsafe { XCrdStorageSetLastUsedConsole(a, 0x14) }, io_err());
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0), 3);
    assert_eq!(le32(&c[0].1, 4), 0x14);
    assert_eq!(unsafe { XCrdStorageSetLastUsedConsole(null_mut(), 0x14) }, E_INVALIDARG);
}

#[test]
fn set_trace_flags_validates_mask() {
    let before = TRACE_FLAGS.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(XCrdSetTraceFlags(8), nt_to_hr(STATUS_INVALID_PARAMETER));
    assert_eq!(XCrdSetTraceFlags(before & 7), 0);
    assert_eq!(TRACE_FLAGS.load(std::sync::atomic::Ordering::Relaxed), before & 7);
}

#[test]
fn shuffle_returns_console_not_implemented() {
    assert_eq!(XCrdShuffle(), HR_NOTIMPL);
}

#[test]
fn wait_for_all_removable_disk_removal_returns_not_implemented() {
    assert_eq!(XCrdWaitForAllRemovableDiskRemoval(), HR_NOTIMPL);
}

#[test]
fn wait_for_any_removable_disk_arrival_returns_not_implemented() {
    assert_eq!(XCrdWaitForAnyRemovableDiskArrival(), HR_NOTIMPL);
}

#[test]
fn trim_deallocated_clusters_returns_not_supported() {
    assert_eq!(XCrdTrimDeallocatedClusters(), E_NOTSUPPORTED);
}

#[test]
fn streaming_start_lays_out_paths_and_specifiers() {
    let (m, _g) = mock(|_, _, p| {
        put64(p, 0x18, 0xabc);
        put64(p, 0x38, 0x55);
        put32(p, 0x40, 7);
        0
    });
    let a = adapter(0);
    let src = w("S");
    let dst = w("DD");
    let k = w("k");
    let v = w("vv");
    let specs = [[k.as_ptr(), v.as_ptr()]];
    let mut h = 0u64;
    let mut size = 1u64;
    let mut st = 1u32;
    assert_eq!(unsafe { XCrdStreamingStart(&mut h, a, src.as_ptr(), null(), dst.as_ptr(), null(), null(), 1, specs.as_ptr(), 1, &mut size, &mut st) }, 0);
    assert_eq!((h, size, st), (0xabc, 0x55, 7));
    let c = m.xvd_calls();
    let p = &c[0].1;
    assert_eq!(c[0].0, 0x7800_0030);
    assert_eq!(le64(p, 0x10), 0x10);
    assert_eq!(le32(p, 0), 0x68);
    assert_eq!(wide_at(p, 0x68), "S");
    assert_eq!(wide_at(p, le32(p, 8) as usize), "DD");
    let table = le32(p, 0x2c) as usize;
    assert_eq!(le32(p, 0x34), 1);
    assert_eq!(wide_at(p, le32(p, table) as usize), "k");
    assert_eq!(wide_at(p, le32(p, table + 8) as usize), "vv");
    assert_eq!(le32(p, table + 12), 4);
    assert_eq!(unsafe { XCrdStreamingStart(&mut h, a, src.as_ptr(), null(), dst.as_ptr(), null(), null(), 0, null(), 0x800, null_mut(), null_mut()) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdStreamingStart(&mut h, a, src.as_ptr(), null(), dst.as_ptr(), src.as_ptr(), null(), 0, null(), 0, null_mut(), null_mut()) }, E_INVALIDARG);
}

#[test]
fn streaming_stop_sends_handle() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    assert_eq!(unsafe { XCrdStreamingStop(a, 0x77, 0) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_0034);
    assert_eq!(c[0].1.len(), 0x58);
    assert_eq!(le64(&c[0].1, 0), 0x77);
    assert_eq!(unsafe { XCrdStreamingStop(a, 0x77, 1) }, E_INVALIDARG);
}

#[test]
fn streaming_adjust_flags_maps_bits() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    assert_eq!(unsafe { XCrdStreamingAdjustStreamingFlags(a, 5, 1, 8) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_00dc);
    assert_eq!(le32(&c[0].1, 8), 0x10);
    assert_eq!(le32(&c[0].1, 0xc), 0x100);
    assert_eq!(unsafe { XCrdStreamingAdjustStreamingFlags(a, 5, 1, 1) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdStreamingAdjustStreamingFlags(a, 5, 2, 0) }, E_INVALIDARG);
}

#[test]
fn streaming_query_active_instance_id_reads_reply() {
    let (m, _g) = mock(|_, _, p| {
        put64(p, 0, 0x1122);
        0
    });
    let a = adapter(0);
    let mut id = 0u64;
    assert_eq!(unsafe { XCrdStreamingQueryActiveInstanceId(a, &mut id) }, 0);
    assert_eq!(id, 0x1122);
    assert_eq!(m.xvd_calls()[0].0, 0x7800_003c);
    assert_eq!(m.xvd_calls()[0].1.len(), 0x60);
}

#[test]
fn streaming_query_information_unpacks_reply() {
    let (m, _g) = mock(|_, _, p| {
        put64(p, 0x58, 0x5858);
        put32(p, 0xac, 0xacac);
        put64(p, 0xf0, 0xf0f0);
        put32(p, 0xa8, 0xfff);
        0
    });
    let a = adapter(0);
    let mut out = [0u8; STREAM_INFO_SIZE];
    assert_eq!(unsafe { XCrdStreamingQueryInformation(out.as_mut_ptr(), a, 9) }, 0);
    assert_eq!(le64(&out, 0x58), 0x5858);
    assert_eq!(le32(&out, 0x9c), 0xacac);
    assert_eq!(le64(&out, 0xe0), 0xf0f0);
    assert_eq!(le32(&out, 0x98) & 0x3ff, 0x3ff);
    assert_eq!(le64(&m.xvd_calls()[0].1, 0), 9);
    assert_eq!(m.xvd_calls()[0].1.len(), 0x110);
}

#[test]
fn streaming_query_information_by_path_sets_path_fields() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let p = w("abc");
    let mut out = [0u8; STREAM_INFO_SIZE];
    assert_eq!(unsafe { XCrdStreamingQueryInformationByPath(out.as_mut_ptr(), a, p.as_ptr(), 1) }, 0);
    let c = m.xvd_calls();
    assert_eq!(le64(&c[0].1, 0), u64::MAX);
    assert_eq!(le32(&c[0].1, 0x98), 3);
    assert_eq!(le32(&c[0].1, 0x90), 0x110);
    assert_eq!(le32(&c[0].1, 0x94), 8);
    assert_eq!(wide_at(&c[0].1, 0x110), "abc");
    let e = w("");
    assert_eq!(unsafe { XCrdStreamingQueryInformationByPath(out.as_mut_ptr(), a, e.as_ptr(), 0) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdStreamingQueryInformationByPath(out.as_mut_ptr(), a, p.as_ptr(), 2) }, E_INVALIDARG);
}

#[test]
fn streaming_query_region_id_by_offset_unpacks() {
    let (m, _g) = mock(|_, _, p| {
        put32(p, 0x20, 0x42);
        put32(p, 0x24, 0xff);
        put64(p, 0x28, 1);
        put64(p, 0x30, 2);
        put64(p, 0x38, 3);
        0
    });
    let a = adapter(0);
    let p = w("pp");
    let mut out = [0u8; 0x20];
    assert_eq!(unsafe { XCrdStreamingQueryRegionIdByOffset(out.as_mut_ptr(), a, p.as_ptr(), 1, 0x999) }, 0);
    assert_eq!(le32(&out, 0), 0x42);
    assert_eq!(le32(&out, 4), 0x7f);
    assert_eq!((le64(&out, 8), le64(&out, 0x10), le64(&out, 0x18)), (1, 2, 3));
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_0048);
    assert_eq!(le64(&c[0].1, 0x10), 0x999);
    assert_eq!(wide_at(&c[0].1, 0x58), "pp");
    assert_eq!(unsafe { XCrdStreamingQueryRegionIdByOffset(out.as_mut_ptr(), a, p.as_ptr(), 2, 0) }, E_INVALIDARG);
}

#[test]
fn streaming_query_region_information_converts_entries() {
    let (m, _g) = mock(|_, _, p| {
        let at = 0x18 + 2 * 4;
        put32(p, at, 0x11);
        put64(p, at + 8, 0x88);
        put32(p, at + REGION_REPLY_SIZE, 0x22);
        0
    });
    let a = adapter(0);
    let ids = [5u32, 6];
    let mut out = [0u8; REGION_INFO_SIZE * 2];
    assert_eq!(unsafe { XCrdStreamingQueryRegionInformation(out.as_mut_ptr(), a, 3, 2, ids.as_ptr()) }, 0);
    assert_eq!(le32(&out, 0), 0x11);
    assert_eq!(le64(&out, 8), 0x88);
    assert_eq!(le32(&out, REGION_INFO_SIZE), 0x22);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_0040);
    assert_eq!(le64(&c[0].1, 0), 3);
    assert_eq!(le32(&c[0].1, 8), 2);
    assert_eq!(le32(&c[0].1, 0x18), 5);
    assert_eq!(unsafe { XCrdStreamingQueryRegionInformation(out.as_mut_ptr(), a, 3, 0, ids.as_ptr()) }, E_INVALIDARG);
}

#[test]
fn streaming_query_region_information_by_path_places_ids_after_path() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let p = w("ab");
    let ids = [9u32];
    let mut out = [0u8; REGION_INFO_SIZE];
    assert_eq!(unsafe { XCrdStreamingQueryRegionInformationByPath(out.as_mut_ptr(), a, p.as_ptr(), 1, ids.as_ptr()) }, 0);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0xc), 1);
    assert_eq!(le32(&c[0].1, 0x10), 0x18);
    assert_eq!(le32(&c[0].1, 0x14), 6);
    assert_eq!(wide_at(&c[0].1, 0x18), "ab");
    assert_eq!(le32(&c[0].1, 0x1e), 9);
}

#[test]
fn streaming_queue_query_region_list_reads_ids() {
    let (m, _g) = mock(|_, _, p| {
        put32(p, 8, 2);
        put32(p, 0x28, 0x31);
        put32(p, 0x2c, 0x32);
        0
    });
    let a = adapter(0);
    let mut list = [0u32; 4];
    let mut n = 0u32;
    assert_eq!(unsafe { XCrdStreamingQueueQueryRegionList(list.as_mut_ptr(), &mut n, a, 4, 4, 2) }, 0);
    assert_eq!(n, 2);
    assert_eq!(&list[..2], &[0x31, 0x32]);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_0080);
    assert_eq!(le32(&c[0].1, 0x1c), 2);
    assert_eq!(unsafe { XCrdStreamingQueueQueryRegionList(list.as_mut_ptr(), &mut n, a, 4, 4, 4) }, E_INVALIDARG);
}

#[test]
fn streaming_queue_query_region_list_by_path_rejects_overflow() {
    let (m, _g) = mock(|_, _, p| {
        put32(p, 8, 9);
        0
    });
    let a = adapter(0);
    let p = w("q");
    let mut list = [0u32; 2];
    let mut n = 0u32;
    assert_eq!(unsafe { XCrdStreamingQueueQueryRegionListByPath(list.as_mut_ptr(), &mut n, a, p.as_ptr(), 2, 0) }, HR_INSUFFICIENT);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0x10), 0x28);
    assert_eq!(wide_at(&c[0].1, 0x28), "q");
    assert_eq!(unsafe { XCrdStreamingQueueQueryRegionListByPath(list.as_mut_ptr(), &mut n, a, null(), 2, 0) }, E_INVALIDARG);
}

#[test]
fn streaming_queue_insert_region_copies_ids() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let ids = [1u32, 2, 3];
    assert_eq!(unsafe { XCrdStreamingQueueInsertRegion(a, 0x10, 3, ids.as_ptr(), 0) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_0044);
    assert_eq!(le32(&c[0].1, 0x10), 3);
    assert_eq!(le32(&c[0].1, 0x14), 0x38);
    assert_eq!(le32(&c[0].1, 0x40), 3);
    assert_eq!(unsafe { XCrdStreamingQueueInsertRegion(a, 0x10, 3, ids.as_ptr(), 1) }, E_INVALIDARG);
}

#[test]
fn streaming_query_region_specifiers_copies_strings() {
    let (m, _g) = mock(|_, _, p| {
        put32(p, 0x14, 1);
        put32(p, 0x20, 0x77);
        put_wide(p, 0x28, "key");
        put_wide(p, 0xa8, "value");
        0
    });
    let a = adapter(0);
    let mut specs = [0u8; SPECIFIER_SIZE];
    let mut n = 0u32;
    assert_eq!(unsafe { XCrdStreamingQueryRegionSpecifiers(specs.as_mut_ptr(), &mut n, a, 5, 1) }, 0);
    assert_eq!(n, 1);
    assert_eq!(le32(&specs, 0), 0x77);
    assert_eq!(wide_at(&specs, 8), "key");
    assert_eq!(wide_at(&specs, 0x88), "value");
    assert_eq!(m.xvd_calls()[0].0, 0x7800_00a8);
    assert_eq!(unsafe { XCrdStreamingQueryRegionSpecifiers(specs.as_mut_ptr(), &mut n, a, 0, 1) }, E_INVALIDARG);
}

#[test]
fn streaming_query_region_specifiers_by_path_counts_only() {
    let (m, _g) = mock(|_, _, p| {
        put32(p, 0x14, 6);
        0
    });
    let a = adapter(0);
    let p = w("zz");
    let mut n = 0u32;
    assert_eq!(unsafe { XCrdStreamingQueryRegionSpecifiersByPath(null_mut(), &mut n, a, p.as_ptr(), 0) }, 0);
    assert_eq!(n, 6);
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0x1c), 1);
    assert_eq!(wide_at(&c[0].1, 0x20), "zz");
    let mut s = [0u8; SPECIFIER_SIZE];
    assert_eq!(unsafe { XCrdStreamingQueryRegionSpecifiersByPath(s.as_mut_ptr(), &mut n, a, p.as_ptr(), 0) }, E_INVALIDARG);
}

#[test]
fn suspend_resume_io_balancer_sends_flag() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    assert_eq!(unsafe { XCrdSuspendResumeIoBalancer(a, 1) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_00a4);
    assert_eq!(c[0].1, vec![1]);
    assert_eq!(unsafe { XCrdSuspendResumeIoBalancer(null_mut(), 1) }, E_INVALIDARG);
}

#[test]
fn set_xrsc_information_maps_classes() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let v = 0x1234_5678_9abc_def0u64;
    assert_eq!(unsafe { XCrdSetXRscInformation(a, 4, &v as *const u64 as *const u8, 8) }, 0);
    assert_eq!(unsafe { XCrdSetXRscInformation(a, 2, &v as *const u64 as *const u8, 4) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_0054);
    assert_eq!(le32(&c[0].1, 8), 0xf);
    assert_eq!(le64(&c[0].1, 0x10), v);
    assert_eq!(le32(&c[0].1, 0), 0x110);
    assert_eq!(wide_at(&c[0].1, 0x110), "[XUC:]\\ANYXVD");
    assert_eq!(le32(&c[1].1, 8), 0xd);
    assert_eq!(le32(&c[1].1, 0x10), v as u32);
    assert_eq!(unsafe { XCrdSetXRscInformation(a, 0, &v as *const u64 as *const u8, 4) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdSetXRscInformation(a, 6, &v as *const u64 as *const u8, 8) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdSetXRscInformation(a, 0, null(), 8) }, E_INVALIDARG);
}

#[test]
fn trim_xvc_builds_specifier_table_and_returns_outputs() {
    let (m, _g) = mock(|_, _, p| {
        p[0x1c] = 1;
        put64(p, 0x20, 0x4000);
        0
    });
    let a = adapter(0);
    let p = w("X");
    let k = w("a");
    let v = w("bc");
    let specs = [[k.as_ptr(), v.as_ptr()]];
    let mut size = 0u64;
    let mut done = 9u8;
    assert_eq!(unsafe { XCrdTrimXvc(a, p.as_ptr(), 1, specs.as_ptr(), 1, &mut size, &mut done) }, 0);
    assert_eq!((size, done), (0x4000, 1));
    let c = m.xvd_calls();
    let b = &c[0].1;
    assert_eq!(c[0].0, 0x7800_00ac);
    assert_eq!(le32(b, 0x14), 1);
    assert_eq!(wide_at(b, 0x48), "X");
    assert_eq!(le32(b, 0x10), 1);
    let table = le32(b, 8) as usize;
    assert_eq!(table, 0x4c);
    assert_eq!(wide_at(b, le32(b, table) as usize), "a");
    assert_eq!(wide_at(b, le32(b, table + 8) as usize), "bc");
    assert_eq!(le32(b, 0xc), 0x10 + 4 + 6);
    assert_eq!(unsafe { XCrdTrimXvc(a, p.as_ptr(), 0, null(), 1, null_mut(), &mut done) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdTrimXvc(a, p.as_ptr(), 0, null(), 2, &mut size, &mut done) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdTrimXvc(a, p.as_ptr(), 1, null(), 0, &mut size, &mut done) }, E_INVALIDARG);
}

#[test]
fn trim_xvd_sends_mode_2_and_reports_flag_on_failure() {
    let (m, _g) = mock(|_, _, p| {
        p[0x1c] = 1;
        STATUS_IO_DEVICE_ERROR
    });
    let a = adapter(0);
    let p = w("Y");
    let mut size = 5u64;
    let mut done = 0u8;
    assert_eq!(unsafe { XCrdTrimXvd(a, p.as_ptr(), &mut size, &mut done) }, io_err());
    assert_eq!((size, done), (5, 1));
    let c = m.xvd_calls();
    assert_eq!(le32(&c[0].1, 0x14), 2);
    assert_eq!(c[0].1.len(), 2 + 0x4a);
    assert_eq!(unsafe { XCrdTrimXvd(a, null(), &mut size, &mut done) }, E_INVALIDARG);
}

unsafe fn device_path(path: &str, disk: u32) -> *mut DevicePath {
    let wide = w(path);
    let cb = DEVICE_PATH_HEADER + wide.len() * 2;
    let dp = unsafe { heap_alloc_zeroed(cb) };
    let s = unsafe { dp.add(DEVICE_PATH_HEADER) } as *mut u16;
    unsafe { core::ptr::copy_nonoverlapping(wide.as_ptr(), s, wide.len()) };
    let d = dp as *mut DevicePath;
    unsafe {
        (*d).path = s;
        (*d).disk = disk;
        (*d).address = ScsiAddress { length: 8, port: 0, path: 1, target: 2, lun: 3 };
    }
    d
}

fn dismount_mock(lun_status: i32) -> (Mock, Guard) {
    let m = Mock::new();
    m.on_xvd(move |_, code, _| if code == 0x7800_0008 { lun_status } else { 0 });
    m.file("\\\\?\\GLOBALROOT\\Device\\Harddisk4\\Partition1", Ok(0x77));
    m.on_ioctl(|_, code, _, out| {
        if code == 0x56_0000 && out.len() >= 12 {
            out[8..12].copy_from_slice(&4u32.to_le_bytes());
        }
        Ok(0)
    });
    let g = m.install();
    (m, g)
}

#[test]
fn unmount_ex_reports_stage() {
    let a = adapter(0);
    let mut stage = 9u32;
    assert_eq!(unsafe { XCrdUnmountEx(null_mut(), null_mut(), &mut stage) }, E_INVALIDARG);
    assert_eq!(stage, 1);
    assert_eq!(unsafe { XCrdUnmountEx(a, null_mut(), &mut stage) }, E_INVALIDARG);
    assert_eq!(stage, 2);
    let (m, _g) = dismount_mock(STATUS_IO_DEVICE_ERROR);
    let dp = unsafe { device_path("\\\\?\\GLOBALROOT\\Device\\Harddisk4\\Partition1", 4) };
    assert_eq!(unsafe { XCrdUnmountEx(a, dp, &mut stage) }, io_err());
    assert_eq!(stage, 4);
    let lun = m.xvd_calls().into_iter().find(|c| c.0 == 0x7800_0008).unwrap();
    assert_eq!(&lun.1[0..3], &[3, 2, 1]);
    unsafe { heap_free(dp as *mut c_void) };
}

#[test]
fn unmount_frees_device_path_on_success() {
    let (m, _g) = dismount_mock(0);
    let a = adapter(0);
    let dp = unsafe { device_path("\\\\?\\GLOBALROOT\\Device\\Harddisk4\\Partition1", 4) };
    assert_eq!(unsafe { XCrdUnmount(a, dp) }, 0);
    assert!(m.xvd_calls().iter().any(|c| c.0 == 0x7800_0008));
    assert_eq!(unsafe { XCrdUnmount(a, null_mut()) }, E_INVALIDARG);
}

#[test]
fn unmount_by_path_stops_when_address_lookup_fails() {
    let (m, _g) = mock(|_, _, _| STATUS_IO_DEVICE_ERROR);
    let a = adapter(0);
    let p = w("[XUC:]\\x");
    assert_eq!(unsafe { XCrdUnmountByPath(a, p.as_ptr()) }, io_err());
    let c = m.xvd_calls();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, 0x7800_0078);
    assert_eq!(unsafe { XCrdUnmountByPath(a, null()) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdUnmountByPath(null_mut(), p.as_ptr()) }, E_INVALIDARG);
}

#[test]
fn update_path_swaps_equal_length_prefix() {
    let mut p = w("[XUC:]\\file");
    assert_eq!(unsafe { XCrdUpdatePath(p.as_mut_ptr(), 0) }, 0);
    assert_eq!(wstring(p.as_ptr()), "[XTE:]\\file");
    assert_eq!(unsafe { XCrdUpdatePath(p.as_mut_ptr(), 4) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdUpdatePath(p.as_mut_ptr(), 0x25) }, E_INVALIDARG);
    let mut bad = w("C:\\file");
    assert_eq!(unsafe { XCrdUpdatePath(bad.as_mut_ptr(), 0) }, E_INVALIDARG);
}

#[test]
fn update_xvc_lays_out_paths_and_flags() {
    let (m, _g) = mock(|_, _, p| {
        put64(p, 0x18, 0x100);
        p[0x28] = 1;
        put64(p, 0x38, 0x200);
        put32(p, 0x40, 3);
        0
    });
    let a = adapter(0);
    let base = w("B");
    let upd = w("UU");
    let extra = w("E");
    let mut us = 0u64;
    let mut free = 0u64;
    let mut f8 = 0u8;
    let mut st = 0u32;
    assert_eq!(unsafe { XCrdUpdateXvc(a, base.as_ptr(), upd.as_ptr(), extra.as_ptr(), 0, null(), 0x11, &mut us, &mut free, &mut f8, &mut st) }, 0);
    assert_eq!((us, free, f8, st), (0x100, 0x200, 1, 3));
    let c = m.xvd_calls();
    let b = &c[0].1;
    assert_eq!(c[0].0, 0x7800_0030);
    assert_eq!(le64(b, 0x10), 0x2002);
    assert_eq!(wide_at(b, 0x68), "UU");
    assert_eq!(wide_at(b, le32(b, 8) as usize), "B");
    assert_eq!(wide_at(b, le32(b, 0x44) as usize), "E");
    assert_eq!(unsafe { XCrdUpdateXvc(a, base.as_ptr(), upd.as_ptr(), null(), 0, null(), 0xa, &mut us, &mut free, &mut f8, &mut st) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdUpdateXvc(a, base.as_ptr(), upd.as_ptr(), null(), 0, null(), 1, null_mut(), null_mut(), &mut f8, &mut st) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdUpdateXvc(a, base.as_ptr(), upd.as_ptr(), null(), 0, null(), 4, &mut us, &mut free, &mut f8, &mut st) }, E_INVALIDARG);
}

#[test]
fn update_xvc_ex_handles_actions() {
    let (m, _g) = mock(|_, _, p| {
        put64(p, 0x30, 0x300);
        put64(p, 0x38, 0x400);
        put32(p, 0x40, 5);
        0
    });
    let a = adapter(0);
    let base = w("BB");
    let upd = w("U");
    let k = w("k");
    let v = w("v");
    let specs = [[k.as_ptr(), v.as_ptr()]];
    let mut us = 0u64;
    let mut free = 0u64;
    let mut st = 0u32;
    assert_eq!(unsafe { XCrdUpdateXvcEx(a, base.as_ptr(), upd.as_ptr(), null(), 1, specs.as_ptr(), 0, 0x10, &mut us, &mut free, &mut st) }, 0);
    assert_eq!((us, free, st), (0x300, 0x400, 5));
    let c = m.xvd_calls();
    let b = &c[0].1;
    assert_eq!(c[0].0, 0x7800_0110);
    assert_eq!(le32(b, 0x24), 0);
    assert_eq!(le32(b, 0x28), 1);
    assert_eq!(wide_at(b, 0x48), "BB");
    assert_eq!(wide_at(b, le32(b, 8) as usize), "U");
    let table = le32(b, 0x18) as usize;
    assert_eq!(wide_at(b, le32(b, table) as usize), "k");
    assert_eq!(unsafe { XCrdUpdateXvcEx(a, base.as_ptr(), null(), null(), 0, null(), 2, 0, null_mut(), null_mut(), null_mut()) }, 0);
    assert_eq!(le32(&m.xvd_calls()[1].1, 0x24), 2);
    assert_eq!(unsafe { XCrdUpdateXvcEx(a, base.as_ptr(), upd.as_ptr(), null(), 0, null(), 3, 0, &mut us, &mut free, &mut st) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdUpdateXvcEx(a, base.as_ptr(), upd.as_ptr(), null(), 0, null(), 0, 0, null_mut(), &mut free, &mut st) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdUpdateXvcEx(a, base.as_ptr(), upd.as_ptr(), null(), 0, null(), 1, 1, &mut us, &mut free, &mut st) }, E_INVALIDARG);
}

fn uab_fixture(total: u64, tail: u64, full: bool) -> (Vec<u8>, Vec<u8>, u64) {
    let q = |x: u64| (((x as u128 * 0xc0c0_c0c0_c0c0_c0c1u128) >> 64) as u64) >> 0x13;
    let a = (q(total - tail + 0xa9fff) + 7) >> 3;
    let b = (q(tail + 0xa9fff) + 7) >> 3;
    let page = ((a + 0xfff + b) as u32) & 0xffff_f000;
    let big = ((b + 0xfff + a) & !0xfff) * 2 + 0x1000;
    let mut u = vec![0u8; 0x3000.max(big as usize)];
    let mut basic = vec![0u8; 0x100];
    put64(&mut u, 0x200, 0x2042_4155_582d_534d);
    put64(&mut u, 0x208, 0x1000);
    put32(&mut u, 0x218, page);
    put64(&mut u, 0x2a0, total);
    put64(&mut u, 0x2c0, tail);
    put64(&mut basic, 0x78, total);
    for i in 0..16 {
        u[0x220 + i] = i as u8;
        u[0x230 + i] = i as u8;
        basic[0x30 + i] = i as u8;
        u[0x240 + i] = 0x80 | i as u8;
        basic[0x40 + i] = 0x80 | i as u8;
    }
    let size;
    if full {
        put32(&mut u, 0x21c, page);
        put64(&mut u, 0x210, 0x1000 + page as u64);
        put32(&mut u, 0x284, page * 2 + 0x1000);
        size = big;
    } else {
        put32(&mut u, 0x284, page + 0x1000);
        size = page as u64 + 0x1000;
    }
    (u, basic, size)
}

#[test]
fn validate_uab_checks_layout() {
    let (u, b, size) = uab_fixture(0x4000_0000, 0x1000_0000, false);
    assert_eq!(unsafe { XCrdValidateUab(u.as_ptr(), size, 0, b.as_ptr()) }, 0);
    let (u2, b2, size2) = uab_fixture(0x4000_0000, 0x1000_0000, true);
    assert_eq!(unsafe { XCrdValidateUab(u2.as_ptr(), size2, 1, b2.as_ptr()) }, 0);
    assert_eq!(unsafe { XCrdValidateUab(u.as_ptr(), size, 1, b.as_ptr()) }, HR_CORRUPT);
    let mut bad = u.clone();
    bad[0x200] = 0;
    assert_eq!(unsafe { XCrdValidateUab(bad.as_ptr(), size, 0, b.as_ptr()) }, HR_CORRUPT);
    let mut other = b.clone();
    other[0x40] ^= 1;
    assert_eq!(unsafe { XCrdValidateUab(u.as_ptr(), size, 0, other.as_ptr()) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdValidateUab(u.as_ptr(), 0xfff, 0, b.as_ptr()) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdValidateUab(null(), size, 0, b.as_ptr()) }, E_INVALIDARG);
}

unsafe fn take_path(p: *mut u16) -> String {
    let s = wstring(p);
    unsafe { heap_free(p as *mut c_void) };
    s
}

#[test]
fn xas_build_path_formats_url() {
    let a = adapter(0);
    let (host, acct, cont, blob, sig) = (w("h.example"), w("acct"), w("cont"), w("dir/file"), w("?sv=1%2"));
    let mut out = null_mut();
    assert_eq!(unsafe { XCrdXasBuildPath(&mut out, a, host.as_ptr(), null(), acct.as_ptr(), cont.as_ptr(), blob.as_ptr(), sig.as_ptr(), 1, 1, 0) }, 0);
    assert_eq!(unsafe { take_path(out) }, "[XAS:]h.example/acct/cont/dir/file?sv=1%2,SN=blob.core.windows.net,PRST=1");
    assert_eq!(unsafe { XCrdXasBuildPath(&mut out, a, host.as_ptr(), null(), acct.as_ptr(), cont.as_ptr(), null(), sig.as_ptr(), 0, 4, 0) }, 0);
    assert_eq!(unsafe { take_path(out) }, "[XAS:]h.example/acct/cont/%ws?sv=1%%2,SN=blob.core.windows.net,NRM=1");
    assert_eq!(unsafe { XCrdXasBuildPath(&mut out, a, host.as_ptr(), null(), acct.as_ptr(), cont.as_ptr(), blob.as_ptr(), sig.as_ptr(), 1, 0, 1) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdXasBuildPath(&mut out, a, host.as_ptr(), null(), acct.as_ptr(), cont.as_ptr(), blob.as_ptr(), sig.as_ptr(), 2, 0, 0) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdXasBuildPath(&mut out, a, null(), null(), acct.as_ptr(), cont.as_ptr(), blob.as_ptr(), sig.as_ptr(), 1, 0, 0) }, E_INVALIDARG);
}

#[test]
fn xas_build_path_ex_adds_vcs_fields() {
    let a = adapter(0);
    let blob = w("f");
    let vcsa = w("10.0.0.1");
    let dom = w("d.net");
    let mut out = null_mut();
    assert_eq!(unsafe { XCrdXasBuildPathEx(&mut out, a, null(), dom.as_ptr(), null(), null(), blob.as_ptr(), null(), 1, 2, vcsa.as_ptr(), 443) }, 0);
    assert_eq!(unsafe { take_path(out) }, "[XAS:]0.0.0.0/invalidACCT/invalidCONT/f?sig=invalid,SN=d.net,VCSA=10.0.0.1,VCSP=443");
    assert_eq!(unsafe { XCrdXasBuildPathEx(&mut out, a, null(), null(), null(), null(), blob.as_ptr(), null(), 1, 2, null(), 0) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdXasBuildPathEx(&mut out, null_mut(), null(), null(), null(), null(), blob.as_ptr(), null(), 1, 2, vcsa.as_ptr(), 0) }, E_INVALIDARG);
}

#[test]
fn xas_key_query_value_finds_keys() {
    let p = w("[XAS:]h/a/c/b?s,SN=x,PRST=1,NRM=1");
    let q = w("[XAS:]h/a/c/b?s,SN=x");
    let e = w("");
    assert_eq!(unsafe { XCrdXasKeyQueryValue(p.as_ptr(), 1) }, 1);
    assert_eq!(unsafe { XCrdXasKeyQueryValue(p.as_ptr(), 4) }, 1);
    assert_eq!(unsafe { XCrdXasKeyQueryValue(p.as_ptr(), 2) }, 0);
    assert_eq!(unsafe { XCrdXasKeyQueryValue(q.as_ptr(), 1) }, 0);
    assert_eq!(unsafe { XCrdXasKeyQueryValue(e.as_ptr(), 1) }, 0);
}

#[test]
fn xblob_copy_packs_paths_and_flags() {
    let (m, _g) = mock(|_, _, p| {
        put64(p, 0x28, 0x99);
        0
    });
    let a = adapter(0);
    let s = w("src");
    let d = w("dd");
    let mut out = 0u64;
    assert_eq!(unsafe { XCrdXBlobCopy(a, s.as_ptr(), d.as_ptr(), 0, 0, 2, &mut out) }, 0);
    assert_eq!(out, 0x99);
    let c = m.xvd_calls();
    let b = &c[0].1;
    assert_eq!(c[0].0, 0x7800_00ec);
    assert_eq!(le32(b, 0), (5 * 2 + 0x54) as u32);
    assert_eq!(le32(b, 0x14), 2);
    assert_eq!(wide_at(b, 0x50), "src");
    assert_eq!(le32(b, 0xc), 0x50 + 6 + 2);
    assert_eq!(wide_at(b, 0x58), "dd");
    assert_eq!(unsafe { XCrdXBlobCopy(a, s.as_ptr(), d.as_ptr(), 1, 0, 0, &mut out) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdXBlobCopy(a, s.as_ptr(), null(), 0, 0, 0, &mut out) }, E_INVALIDARG);
}

#[test]
fn xblob_create_sends_size() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    let p = w("blob");
    assert_eq!(unsafe { XCrdXBlobCreate(a, p.as_ptr(), 0x5000, 0) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_00e4);
    assert_eq!(le32(&c[0].1, 4), 0x30);
    assert_eq!(le64(&c[0].1, 0x10), 0x5000);
    assert_eq!(wide_at(&c[0].1, 0x30), "blob");
    assert_eq!(unsafe { XCrdXBlobCreate(a, p.as_ptr(), 0, 1) }, E_INVALIDARG);
}

#[test]
fn xblob_delete_sends_path() {
    let (m, _g) = mock(|_, _, _| STATUS_IO_DEVICE_ERROR);
    let a = adapter(0);
    let p = w("blob");
    assert_eq!(unsafe { XCrdXBlobDelete(a, p.as_ptr()) }, io_err());
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_00e8);
    assert_eq!(le32(&c[0].1, 0), 8 + 0x1a);
    assert_eq!(wide_at(&c[0].1, 0x18), "blob");
    assert_eq!(unsafe { XCrdXBlobDelete(a, null()) }, E_INVALIDARG);
}

#[test]
fn xpf_op_validates_op_and_reserved() {
    let (m, _g) = ok_mock();
    let a = adapter(0);
    assert_eq!(unsafe { XCrdXpfOp(a, 2, 0, 0) }, 0);
    let c = m.xvd_calls();
    assert_eq!(c[0].0, 0x7800_00b4);
    assert_eq!(c[0].1.len(), 0x88);
    assert_eq!(le32(&c[0].1, 0), 2);
    assert_eq!(unsafe { XCrdXpfOp(a, 3, 0, 0) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdXpfOp(a, 0, 1, 0) }, E_INVALIDARG);
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateEventW(sa: *mut c_void, manual: i32, initial: i32, name: *const u16) -> isize;
    fn CloseHandle(h: isize) -> i32;
}

struct Peer(std::fs::File);

impl Peer {
    fn recv(&mut self) -> [u8; XFER_MSG] {
        use std::io::Read;
        let mut m = [0u8; XFER_MSG];
        self.0.read_exact(&mut m).unwrap();
        m
    }
    fn send(&mut self, fields: &[(usize, u32)]) {
        use std::io::Write;
        let mut m = [0u8; XFER_MSG];
        for &(o, v) in fields {
            put32(&mut m, o, v);
        }
        self.0.write_all(&m).unwrap();
    }
    fn drain(&mut self) {
        use std::io::Read;
        let mut b = [0u8; 64];
        while matches!(self.0.read(&mut b), Ok(n) if n > 0) {}
    }
    fn raw(&mut self, b: &[u8]) {
        use std::io::Write;
        self.0.write_all(b).unwrap();
    }
}

struct Xfer {
    mock: Mock,
    _guard: Guard,
    adapter: *mut c_void,
    ctx: *mut XferCtx,
    peer: Option<std::thread::JoinHandle<()>>,
}

fn xfer(tag: &str, script: impl FnOnce(&mut Peer) + Send + 'static) -> Xfer {
    let name = format!("\\\\.\\pipe\\xcrd2-{tag}-{}", std::process::id());
    PIPE_NAME.with(|p| *p.borrow_mut() = Some(w(&name)));
    let (mock, guard) = mock(|_, code, p| {
        if code == 0x7800_00c0 && p[4] == 1 {
            put32(p, 0, 77);
        }
        0
    });
    let peer = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let f = loop {
            match std::fs::OpenOptions::new().read(true).write(true).open(&name) {
                Ok(f) => break f,
                Err(_) if start.elapsed().as_secs() < 4 => std::thread::sleep(std::time::Duration::from_millis(5)),
                Err(e) => panic!("{e}"),
            }
        };
        script(&mut Peer(f));
    });
    let adapter = adapter(0);
    let mut ctx = null_mut();
    assert_eq!(unsafe { XCrdXvdXferInitialize(adapter, &mut ctx) }, 0);
    Xfer { mock, _guard: guard, adapter, ctx, peer: Some(peer) }
}

impl Xfer {
    fn finish(mut self) {
        let mut c = self.ctx;
        assert_eq!(unsafe { XCrdXvdXferUninitialize(self.adapter, &mut c) }, 0);
        assert!(c.is_null());
        self.peer.take().unwrap().join().unwrap();
        PIPE_NAME.with(|p| *p.borrow_mut() = None);
    }
}

fn start_reply(p: &mut Peer, id: u32) {
    let m = p.recv();
    assert_eq!(le32(&m, 0), 1);
    assert_eq!(wide_at(&m, 8), "[XUC:]\\xfer");
    p.send(&[(0, 2), (8, 0), (0xc, id)]);
}

fn stop_reply(p: &mut Peer, id: u32) {
    let m = p.recv();
    assert_eq!(le32(&m, 0), 5);
    assert_eq!(le32(&m, 8), id);
    p.send(&[(0, 6), (8, 0)]);
}

#[test]
fn xvd_xfer_initialize_opens_session() {
    let x = xfer("init", |p| p.drain());
    let c = x.ctx;
    unsafe {
        assert_eq!((*c).session, 77);
        assert_eq!((*c).transfer, u32::MAX);
        assert_eq!((*c).failed, 1);
        assert_eq!((*c).hr, HR_NOT_READY);
    }
    let open = x.mock.xvd_calls();
    assert_eq!(open[0].0, 0x7800_00c0);
    assert_eq!(open[0].1.len(), 0x20);
    assert_eq!(open[0].1[4], 1);
    x.finish();
    let mut out = null_mut();
    assert_eq!(unsafe { XCrdXvdXferInitialize(null_mut(), &mut out) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdXvdXferInitialize(adapter(0), null_mut()) }, E_INVALIDARG);
}

#[test]
fn xvd_xfer_start_negotiates_transfer_id() {
    let x = xfer("start", |p| {
        start_reply(p, 9);
        stop_reply(p, 9);
    });
    let path = w("[XUC:]\\xfer");
    assert_eq!(unsafe { XCrdXvdXferStart(x.ctx, path.as_ptr()) }, 0);
    unsafe {
        assert_eq!((*x.ctx).transfer, 9);
        assert_eq!((*x.ctx).failed, 0);
        assert_ne!((*x.ctx).thread, 0);
    }
    assert_eq!(unsafe { XCrdXvdXferStart(x.ctx, path.as_ptr()) }, HR_INVALID_CONTEXT);
    assert_eq!(unsafe { XCrdXvdXferStart(x.ctx, null()) }, E_INVALIDARG);
    x.finish();
}

#[test]
fn xvd_xfer_start_rejects_bad_reply() {
    let x = xfer("startbad", |p| {
        p.recv();
        p.send(&[(0, 7)]);
    });
    let path = w("[XUC:]\\xfer");
    assert_eq!(unsafe { XCrdXvdXferStart(x.ctx, path.as_ptr()) }, HR_BAD_COMMAND);
    unsafe { assert_eq!((*x.ctx).transfer, u32::MAX) };
    x.finish();
}

fn read_script(p: &mut Peer) {
    start_reply(p, 4);
    let m = p.recv();
    assert_eq!(le32(&m, 0), 3);
    assert_eq!(le32(&m, 8), 4);
    assert_eq!(le32(&m, 0x10), 6);
    assert_eq!(le64(&m, 0x18), 0x2000);
    let slot = le32(&m, 0xc);
    p.send(&[(0, 4), (8, 0), (0xc, 4), (0x10, slot), (0x14, 6)]);
    p.raw(b"PAYLOD");
    stop_reply(p, 4);
}

#[test]
fn xvd_xfer_read_queues_request_and_returns_pending() {
    let x = xfer("read", read_script);
    let path = w("[XUC:]\\xfer");
    assert_eq!(unsafe { XCrdXvdXferStart(x.ctx, path.as_ptr()) }, 0);
    let mut buf = [0u8; 6];
    let mut ov = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: unsafe { CreateEventW(null_mut(), 1, 0, null()) } };
    assert_eq!(unsafe { XCrdXvdXferRead(x.ctx, 0x2000, 6, buf.as_mut_ptr(), &mut ov) }, HR_IO_PENDING);
    let mut n = 0u32;
    assert_eq!(unsafe { XCrdXvdXferGetResult(x.ctx, &mut ov, &mut n) }, 0);
    assert_eq!(&buf, b"PAYLOD");
    assert_eq!(unsafe { XCrdXvdXferRead(x.ctx, 0, 6, null_mut(), &mut ov) }, E_INVALIDARG);
    assert_eq!(unsafe { XCrdXvdXferRead(x.ctx, 0, 6, buf.as_mut_ptr(), null_mut()) }, E_INVALIDARG);
    x.finish();
    assert_eq!(unsafe { XCrdXvdXferRead(null_mut(), 0, 6, buf.as_mut_ptr(), &mut ov) }, E_INVALIDARG);
    unsafe { CloseHandle(ov.event) };
}

#[test]
fn xvd_xfer_get_result_returns_status_and_bytes() {
    let x = xfer("result", read_script);
    let path = w("[XUC:]\\xfer");
    assert_eq!(unsafe { XCrdXvdXferStart(x.ctx, path.as_ptr()) }, 0);
    let mut buf = [0u8; 6];
    let mut ov = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: unsafe { CreateEventW(null_mut(), 1, 0, null()) } };
    assert_eq!(unsafe { XCrdXvdXferRead(x.ctx, 0x2000, 6, buf.as_mut_ptr(), &mut ov) }, HR_IO_PENDING);
    let mut n = 0u32;
    assert_eq!(unsafe { XCrdXvdXferGetResult(x.ctx, &mut ov, &mut n) }, 0);
    assert_eq!(n, 6);
    x.finish();
    unsafe { CloseHandle(ov.event) };
}

#[test]
fn xvd_xfer_read_fails_before_start() {
    let x = xfer("readidle", |p| p.drain());
    let mut buf = [0u8; 4];
    let mut ov = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: 0 };
    assert_eq!(unsafe { XCrdXvdXferRead(x.ctx, 0, 4, buf.as_mut_ptr(), &mut ov) }, HR_NOT_READY);
    x.finish();
}

#[test]
fn xvd_xfer_stop_ends_transfer() {
    let x = xfer("stop", |p| {
        start_reply(p, 3);
        let m = p.recv();
        assert_eq!(le32(&m, 0), 5);
        p.send(&[(0, 4)]);
        p.send(&[(0, 6), (8, STATUS_IO_DEVICE_ERROR as u32)]);
        stop_reply(p, 3);
    });
    let path = w("[XUC:]\\xfer");
    assert_eq!(unsafe { XCrdXvdXferStart(x.ctx, path.as_ptr()) }, 0);
    assert_eq!(unsafe { XCrdXvdXferStop(x.ctx) }, io_err());
    unsafe {
        assert_eq!((*x.ctx).transfer, 3);
        assert_eq!((*x.ctx).thread, 0);
        assert_eq!((*x.ctx).failed, 1);
    }
    assert_eq!(unsafe { XCrdXvdXferStop(x.ctx) }, 0);
    unsafe { assert_eq!((*x.ctx).transfer, u32::MAX) };
    assert_eq!(unsafe { XCrdXvdXferStop(null_mut()) }, E_INVALIDARG);
    x.finish();
}

#[test]
fn xvd_xfer_uninitialize_closes_session() {
    let x = xfer("uninit", |p| p.drain());
    let m = x.mock.clone();
    let a = x.adapter;
    x.finish();
    let calls = m.xvd_calls();
    let close = calls.last().unwrap();
    assert_eq!(close.0, 0x7800_00c0);
    assert_eq!(close.1[4], 0);
    assert_eq!(le32(&close.1, 0), 77);
    assert_eq!(unsafe { XCrdXvdXferUninitialize(a, null_mut()) }, E_INVALIDARG);
    let mut none: *mut XferCtx = null_mut();
    assert_eq!(unsafe { XCrdXvdXferUninitialize(null_mut(), &mut none) }, E_INVALIDARG);
}

pub(crate) static HSV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct StoreDir(std::path::PathBuf);

impl Drop for StoreDir {
    fn drop(&mut self) {
        crate::xcrd2_hsv::set_store(None);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn store_dir(tag: &str) -> StoreDir {
    let root = std::env::temp_dir().join(format!("xcrd2-hsv-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("XUC")).unwrap();
    let data: Vec<u8> = (0..3000u32).map(|i| (i * 7 + 3) as u8).collect();
    std::fs::write(root.join("XUC").join("game.xvd"), data).unwrap();
    crate::xcrd2_hsv::set_store(Some(std::sync::Arc::new(crate::xcrd2_hsv::DirStore::new(root.clone()))));
    StoreDir(root)
}

fn hsv_host(tag: &str) -> (Mock, Guard, *mut c_void, *mut XferCtx) {
    let name = format!("\\\\.\\pipe\\xcrd2-hsv-{tag}-{}", std::process::id());
    PIPE_NAME.with(|p| *p.borrow_mut() = Some(w(&name)));
    let (m, g) = mock(|_, code, p| crate::xcrd2_hsv::dispatch(code, p, pipe_text).unwrap_or(STATUS_IO_DEVICE_ERROR));
    let a = adapter(0);
    let mut ctx = null_mut();
    assert_eq!(unsafe { XCrdXvdXferInitialize(a, &mut ctx) }, 0);
    (m, g, a, ctx)
}

fn hsv_end(a: *mut c_void, ctx: *mut XferCtx) {
    let session = unsafe { (*ctx).session };
    let mut c = ctx;
    assert_eq!(unsafe { XCrdXvdXferUninitialize(a, &mut c) }, 0);
    assert!(c.is_null());
    assert!(!crate::xcrd2_hsv::session_open(session));
    PIPE_NAME.with(|p| *p.borrow_mut() = None);
}

#[test]
fn hsv_dir_store_resolves_crd_paths() {
    let s = crate::xcrd2_hsv::DirStore::new(std::path::PathBuf::from("R"));
    assert_eq!(s.resolve("[xuc:]\\a\\b.xvd").unwrap(), std::path::PathBuf::from("R").join("XUC").join("a").join("b.xvd"));
    assert_eq!(s.resolve("[XUC:]\\..\\x").err(), Some(crate::xcrd2_hsv::STATUS_OBJECT_NAME_INVALID));
    assert_eq!(s.resolve("C:\\x").err(), Some(crate::xcrd2_hsv::STATUS_OBJECT_NAME_INVALID));
    assert_eq!(s.resolve("[XUC:]\\\\").err(), Some(crate::xcrd2_hsv::STATUS_OBJECT_NAME_INVALID));
}

#[test]
fn hsv_dispatch_opens_and_closes_sessions() {
    let mut p = [0u8; 0x20];
    p[4] = 1;
    let name = format!("\\\\.\\pipe\\xcrd2-hsv-nobody-{}", std::process::id());
    assert_eq!(crate::xcrd2_hsv::dispatch(0x7800_00c0, &mut p, || name), Some(0));
    let id = le32(&p, 0);
    assert!(id != 0 && id != u32::MAX);
    assert!(crate::xcrd2_hsv::session_open(id));
    p[4] = 0;
    assert_eq!(crate::xcrd2_hsv::dispatch(0x7800_00c0, &mut p, String::new), Some(0));
    assert_eq!(crate::xcrd2_hsv::dispatch(0x7800_00c0, &mut p, String::new), Some(crate::xcrd2_hsv::STATUS_INVALID_PARAMETER));
    assert_eq!(crate::xcrd2_hsv::dispatch(0x7800_0054, &mut p, String::new), None);
}

#[test]
fn hsv_xfer_end_to_end_reads_store() {
    let _l = HSV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _d = store_dir("e2e");
    let (m, _g, a, ctx) = hsv_host("e2e");
    assert_eq!(m.xvd_calls()[0].0, 0x7800_00c0);
    let path = w("[XUC:]\\game.xvd");
    assert_eq!(unsafe { XCrdXvdXferStart(ctx, path.as_ptr()) }, 0);
    let ev = || unsafe { CreateEventW(null_mut(), 1, 0, null()) };
    let mut b1 = vec![0u8; 1000];
    let mut b2 = vec![0u8; 1000];
    let mut b3 = vec![0u8; 16];
    let mut o1 = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: ev() };
    let mut o2 = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: ev() };
    let mut o3 = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: ev() };
    assert_eq!(unsafe { XCrdXvdXferRead(ctx, 0, 1000, b1.as_mut_ptr(), &mut o1) }, HR_IO_PENDING);
    assert_eq!(unsafe { XCrdXvdXferRead(ctx, 2500, 1000, b2.as_mut_ptr(), &mut o2) }, HR_IO_PENDING);
    assert_eq!(unsafe { XCrdXvdXferRead(ctx, 5000, 16, b3.as_mut_ptr(), &mut o3) }, HR_IO_PENDING);
    let (mut n1, mut n2, mut n3) = (0u32, 0u32, 0u32);
    assert_eq!(unsafe { XCrdXvdXferGetResult(ctx, &mut o1, &mut n1) }, 0);
    assert_eq!(unsafe { XCrdXvdXferGetResult(ctx, &mut o2, &mut n2) }, 0);
    assert_eq!(unsafe { XCrdXvdXferGetResult(ctx, &mut o3, &mut n3) }, nt_to_hr(crate::xcrd2_hsv::STATUS_END_OF_FILE));
    assert_eq!((n1, n2, n3), (1000, 500, 0));
    let expect = |i: u32| (i * 7 + 3) as u8;
    assert!(b1.iter().enumerate().all(|(i, &v)| v == expect(i as u32)));
    assert!(b2[..500].iter().enumerate().all(|(i, &v)| v == expect(2500 + i as u32)));
    assert_eq!(unsafe { XCrdXvdXferStop(ctx) }, 0);
    unsafe { assert_eq!((*ctx).transfer, u32::MAX) };
    hsv_end(a, ctx);
    for e in [o1.event, o2.event, o3.event] {
        unsafe { CloseHandle(e) };
    }
}

#[test]
fn hsv_xfer_start_reports_missing_blob() {
    let _l = HSV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _d = store_dir("missing");
    let (_m, _g, a, ctx) = hsv_host("missing");
    let nope = w("[XUC:]\\nope.xvd");
    assert_eq!(unsafe { XCrdXvdXferStart(ctx, nope.as_ptr()) }, nt_to_hr(crate::xcrd2_hsv::STATUS_OBJECT_NAME_NOT_FOUND));
    let deep = w("[XUC:]\\no\\dir.xvd");
    assert_eq!(unsafe { XCrdXvdXferStart(ctx, deep.as_ptr()) }, nt_to_hr(crate::xcrd2_hsv::STATUS_OBJECT_PATH_NOT_FOUND));
    let ok = w("[XUC:]\\game.xvd");
    assert_eq!(unsafe { XCrdXvdXferStart(ctx, ok.as_ptr()) }, 0);
    hsv_end(a, ctx);
}
