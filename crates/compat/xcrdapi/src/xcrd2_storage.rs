use crate::xcrd_a::blob_io;
use crate::xcrd_core::*;
use core::ffi::c_void;
use core::ptr::null;

pub const HR_NOTIMPL: i32 = 0x8000_4001u32 as i32;
pub const HR_INSUFFICIENT: i32 = 0x8007_007au32 as i32;
pub const STATUS_BUFFER_TOO_SMALL: i32 = 0xc000_0023u32 as i32;
pub const STORAGE_REPLY: usize = 0x430;
pub const BLOB_CHUNK: u32 = 0x10_0000;

pub fn fail(msg: &str) -> i32 {
    trace(1, || msg.to_string());
    E_INVALIDARG
}

pub const INTERNAL_DEVICES: [([u8; 16], u32); 2] = [
    ([0x52, 0xce, 0x9e, 0xa8, 0x8e, 0x7e, 0x4f, 0x44, 0xbb, 0xd0, 0xc6, 0x8b, 0x76, 0xc2, 0xec, 0xa4], 1),
    ([0x55, 0x2f, 0x2b, 0xeb, 0xb3, 0x83, 0x1e, 0x40, 0xad, 0x7c, 0x13, 0x96, 0xd7, 0xd5, 0xb3, 0x0d], 0x24),
];

pub fn internal_device(id: &[u8; 16]) -> Option<u32> {
    INTERNAL_DEVICES.iter().find(|(g, _)| g == id).map(|(_, i)| *i)
}

#[repr(C)]
pub struct StorageReq {
    pub device: u32,
    pub op: u32,
    pub name: *const u16,
    pub path1: *const u16,
    pub path2: *const u16,
    pub path3: *const u16,
    pub value: *const u32,
    pub id: *const u8,
    pub external: *const u64,
    pub check: *const u32,
    pub blob: *const u8,
    pub association: *const u32,
    pub reply: *mut u8,
    pub extra: *const u8,
}

impl StorageReq {
    pub fn new(device: u32, op: u32) -> StorageReq {
        StorageReq {
            device,
            op,
            name: null(),
            path1: null(),
            path2: null(),
            path3: null(),
            value: null(),
            id: null(),
            external: null(),
            check: null(),
            blob: null(),
            association: null(),
            reply: core::ptr::null_mut(),
            extra: null(),
        }
    }
}

fn is_external_id(d: u32) -> bool {
    d.wrapping_sub(0x13) <= 7
}

fn is_storage_id(d: u32) -> bool {
    d.wrapping_sub(0x13) <= 7 || d.wrapping_sub(0xb) <= 7
}

pub const BLOB_DESC: usize = 0x218;

pub unsafe fn storage_op(adapter: *mut c_void, r: &StorageReq) -> i32 {
    let len_ok = |p: *const u16, max: usize| p.is_null() || unsafe { wlen(p) } < max;
    if !len_ok(r.name, 0x21) || !len_ok(r.path1, 0x104) || !len_ok(r.path2, 0x104) || !len_ok(r.path3, 0x104) {
        return E_INVALIDARG;
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if !r.external.is_null() && unsafe { *(r.external as *const u32) } >= 2 {
        return fail("Invalid External Storage Flags\n");
    }
    let op = r.op;
    let bad = match op {
        1 | 0xc => {
            if r.id.is_null() {
                true
            } else {
                let mut g = [0u8; 16];
                unsafe { core::ptr::copy_nonoverlapping(r.id, g.as_mut_ptr(), 16) };
                internal_device(&g).is_some()
            }
        }
        5 => r.value.is_null(),
        6 => r.id.is_null(),
        8 => r.path1.is_null(),
        9 | 0x12 | 0x13 => r.path2.is_null(),
        0xd => r.check.is_null() || !INTERNAL_DEVICES.iter().any(|(_, i)| *i == r.device),
        0xe => r.blob.is_null(),
        0xf => r.path2.is_null() || !(is_external_id(r.device) || r.device == u32::MAX),
        0x10 => r.id.is_null() || r.extra.is_null(),
        0x11 => r.id.is_null() || r.reply.is_null(),
        0x14 => r.association.is_null() || unsafe { *(r.association as *const i32) } >= 3 || !is_external_id(r.device),
        0x15 => r.path2.is_null() || r.path3.is_null(),
        _ => false,
    };
    if bad {
        return fail("Invalid parameter.\n");
    }
    let mut m = XvdMsg::new(0x7800_0088, STORAGE_REPLY);
    m.set_u32(4, r.device);
    m.set_u32(0, op);
    let copyw = |m: &mut XvdMsg, off: usize, p: *const u16| {
        let n = unsafe { wlen(p) };
        unsafe { m.set_raw(off, p as *const u8, n * 2 + 2) };
    };
    if (op.wrapping_sub(1) <= 1 || op == 0xc) && !r.name.is_null() {
        copyw(&mut m, 0x20, r.name);
    }
    if op <= 0x13 && (0xc8210u32 >> op) & 1 != 0 && !r.path2.is_null() {
        copyw(&mut m, 0x20, r.path2);
    }
    if op == 0x15 {
        if !r.path2.is_null() && !r.path3.is_null() {
            copyw(&mut m, 0x20, r.path2);
            copyw(&mut m, 0x228, r.path3);
        }
    } else if op == 5 {
        if !r.value.is_null() {
            m.set_u32(8, unsafe { *r.value });
        }
    } else {
        if op <= 0xc && (0x1042u32 >> op) & 1 != 0 && !r.id.is_null() {
            unsafe { m.set_raw(0xc, r.id, 16) };
        }
        match op {
            8 => {
                if !r.path1.is_null() {
                    copyw(&mut m, 0x20, r.path1);
                }
            }
            0xa => m.set_u64(0x20, unsafe { *r.external }),
            0xd => m.set_u32(0x20, unsafe { *r.check }),
            0xe => unsafe { m.set_raw(0x20, r.blob, BLOB_DESC) },
            0x10 => {
                unsafe { m.set_raw(0xc, r.id, 16) };
                unsafe { m.set_raw(0x20, r.extra, 16) };
            }
            0x14 => m.set_u32(0x20, unsafe { *r.association }),
            _ => {}
        }
    }
    if op == 0x11 {
        unsafe { m.set_raw(0xc, r.id, 16) };
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 && !r.reply.is_null() {
        unsafe { m.get_raw(0, r.reply, STORAGE_REPLY) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageAttachExternalDevice(out: *mut u64, adapter: *mut c_void, device: u32, flags: u8) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if device.wrapping_sub(0xb) > 7 {
        return fail("Invalid XCrdId.\n");
    }
    let ext = (flags & 1) as u64;
    let mut reply = [0u8; STORAGE_REPLY];
    let mut r = StorageReq::new(device, 0xa);
    r.external = &ext;
    r.reply = reply.as_mut_ptr();
    let hr = unsafe { storage_op(adapter, &r) };
    if hr >= 0 {
        let (lun, path, target, port) = (reply[0x24], reply[0x25], reply[0x26], reply[0x27]);
        trace(4, || format!("[XCRD]: Attached device address Lun:{lun}, Path:{path}, Target:{target}, Port:{port}\n"));
        let v = u32::from_le_bytes([lun, path, target, port]) as u64 | ((device as u64) << 32);
        unsafe { *out = v };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageDetachExternalDevice(adapter: *mut c_void, address: u64) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    let b = address.to_le_bytes();
    let device = (address >> 32) as u32;
    if device.wrapping_sub(0xb) > 7 {
        return fail("Invalid XCrdId.\n");
    }
    let addr = ScsiAddress { length: 8, port: b[3], path: b[2], target: b[1], lun: b[0] };
    let mut disk = 0u32;
    let hr = disk_number_for_address(&mut disk, &addr);
    if hr >= 0 {
        let r = dismount_disk_volumes(disk);
        if r < 0 {
            trace(1, || format!("[XCRD]: XCrdStorageDetachExternalDevice - Failed to dismount disk volumes {disk} - {:x}\n", r as u32));
        }
    } else {
        trace(1, || format!("[XCRD]: XCrdStorageDetachExternalDevice - Failed to get disk number {:x}\n", hr as u32));
    }
    let r = StorageReq::new(device, 0xb);
    let hr = unsafe { storage_op(adapter, &r) };
    trace(4, || format!("[XCRD]: Detached device address Lun:{}, Path:{}, Target:{}, Port:{}. Status {:x}\n", b[0], b[1], b[2], b[3], hr as u32));
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageQueryExternalDeviceAddress(out: *mut u8, address: u64) -> i32 {
    let b = address.to_le_bytes();
    unsafe {
        *out.add(7) = b[0];
        *out.add(6) = b[1];
        *out.add(5) = b[2];
        *out.add(4) = b[3];
        (out as *mut u32).write_unaligned(8);
    }
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageCheck(adapter: *mut c_void, device: u32, flags: u32) -> i32 {
    if flags & !3 != 0 {
        return fail("Invalid Check Flags\n");
    }
    let mut v = 0u32;
    if flags & 2 != 0 {
        v = 2;
    }
    if flags & 1 != 0 {
        v |= 1;
    }
    let mut reply = [0u8; STORAGE_REPLY];
    let mut r = StorageReq::new(device, 0xd);
    r.check = &v;
    r.reply = reply.as_mut_ptr();
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageConnectDevice(adapter: *mut c_void, volume: *const u16) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if volume.is_null() {
        return fail("Invalid VolumePath.\n");
    }
    let mut r = StorageReq::new(0x25, 8);
    r.path1 = volume;
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageDisconnectDevice(adapter: *mut c_void, device: u32, volume: *mut u16, count: *mut u32) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if !is_storage_id(device) {
        return fail("Invalid XCrdId.\n");
    }
    if volume.is_null() {
        return fail("Invalid VolumePath.\n");
    }
    if count.is_null() {
        return fail("Invalid VolumePathCount.\n");
    }
    let mut reply = [0u8; STORAGE_REPLY];
    let mut r = StorageReq::new(device, 7);
    r.reply = reply.as_mut_ptr();
    let mut hr = unsafe { storage_op(adapter, &r) };
    if hr < 0 {
        return hr;
    }
    let name = unsafe { reply.as_ptr().add(0x20) } as *const u16;
    let n = unsafe { wlen(name) } + 1;
    if n <= unsafe { *count } as usize {
        unsafe { core::ptr::copy_nonoverlapping(name, volume, n) };
    } else {
        let mut keep = vec![0u16; n];
        unsafe { core::ptr::copy_nonoverlapping(name, keep.as_mut_ptr(), n) };
        unsafe { XCrdStorageConnectDevice(adapter, keep.as_ptr()) };
        hr = HR_INSUFFICIENT;
    }
    unsafe { *count = n as u32 };
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageFormat(adapter: *mut c_void, device: u32, flags: u32, name: *const u16, id: *mut u8) -> i32 {
    if flags & !1 != 0 {
        return fail("Invalid Format Flags\n");
    }
    let mut reply = [0u8; STORAGE_REPLY];
    let mut r = StorageReq::new(device, if flags & 1 != 0 { 0xc } else { 1 });
    r.name = name;
    r.id = id;
    r.reply = reply.as_mut_ptr();
    let hr = unsafe { storage_op(adapter, &r) };
    if hr >= 0 && !id.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(reply.as_ptr().add(0xc), id, 16) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageCreateBlob(adapter: *mut c_void, path: *const u16, size: u64, flags: u32) -> i32 {
    if path.is_null() {
        return fail("Invalid Input Path.\n");
    }
    let n = unsafe { wlen(path) };
    if n >= 0x104 {
        return fail("XCrdPath length is too long.\n");
    }
    if flags != 0 {
        return fail("Flags must be zero.\n");
    }
    let mut desc = [0u8; BLOB_DESC];
    unsafe { core::ptr::copy_nonoverlapping(path as *const u8, desc.as_mut_ptr(), n * 2 + 2) };
    desc[0x208..0x210].copy_from_slice(&size.to_le_bytes());
    let mut r = StorageReq::new(0x25, 0xe);
    r.blob = desc.as_ptr();
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageCreateDirectory(adapter: *mut c_void, path: *const u16, flags: u32) -> i32 {
    if flags != 0 {
        return fail("Flags must be zero.\n");
    }
    let mut r = StorageReq::new(0x25, 0x12);
    r.path2 = path;
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageDeleteBlob(adapter: *mut c_void, path: *const u16) -> i32 {
    let mut r = StorageReq::new(0x25, 9);
    r.path2 = path;
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageDeleteDirectory(adapter: *mut c_void, path: *const u16) -> i32 {
    let mut r = StorageReq::new(0x25, 0x13);
    r.path2 = path;
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageMoveBlob(adapter: *mut c_void, src: *const u16, dst: *const u16, flags: u32) -> i32 {
    let mut r = StorageReq::new(0x25, 0x15);
    r.path2 = src;
    r.path3 = dst;
    if flags != 0 {
        return fail("Flags must be zero.\n");
    }
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageQueryBlobAttributes(adapter: *mut c_void, path: *const u16, out: *mut u8) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if path.is_null() {
        return fail("Invalid Input Path.\n");
    }
    if out.is_null() {
        return fail("Attributes must not be NULL\n");
    }
    let n = unsafe { wlen(path) };
    let len = n * 2 + 0x4a;
    let mut m = XvdMsg::new(0x7800_00c4, len);
    m.set_u32(0, len as u32);
    m.set_u32(4, 0x48);
    m.set_u32(8, (n * 2) as u32);
    let st = unsafe { m.copy_wstr(0x48, (n + 1) * 2, path) };
    if st < 0 {
        return st;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe { m.get_raw(0x10, out, 0x34) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageReadBlob(adapter: *mut c_void, path: *const u16, buffer: *mut u8, plen: *mut u32) -> i32 {
    unsafe { XCrdStorageReadBlobEx(adapter, path, 0, buffer, plen) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageReadBlobEx(adapter: *mut c_void, path: *const u16, offset: u64, buffer: *mut u8, plen: *mut u32) -> i32 {
    if !buffer.is_null() && unsafe { *plen } == 0 {
        return fail("Can't specify buffer with zero length\n");
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if path.is_null() {
        return fail("Invalid Input Path.\n");
    }
    if buffer.is_null() && plen.is_null() {
        return fail("BufferLength must not be NULL if Buffer is NULL\n");
    }
    let mut left = unsafe { *plen };
    if left == 0 {
        return unsafe { blob_io(adapter, path, 0, buffer, plen, 0x7800_0090, 0) };
    }
    let mut total = 0u32;
    let mut off = offset;
    let mut buf = buffer;
    loop {
        let want = left.min(BLOB_CHUNK);
        let mut got = want;
        let hr = unsafe { blob_io(adapter, path, off, buf, &mut got, 0x7800_0090, 0) };
        if hr < 0 {
            if hr == nt_to_hr(STATUS_BUFFER_TOO_SMALL) {
                unsafe { *plen = total };
            }
            return hr;
        }
        left -= want;
        total = total.wrapping_add(got);
        off = off.wrapping_add(got as u64);
        buf = unsafe { buf.add(got as usize) };
        if got != want || left == 0 {
            unsafe { *plen = total };
            return hr;
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageWriteBlob(adapter: *mut c_void, path: *const u16, buffer: *const u8, len: u32) -> i32 {
    unsafe { XCrdStorageWriteBlobEx(adapter, path, 0, buffer, len, 1) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageWriteBlobEx(adapter: *mut c_void, path: *const u16, offset: u64, buffer: *const u8, len: u32, flags: u32) -> i32 {
    if buffer.is_null() || len == 0 {
        return fail("Can't specify buffer with zero length\n");
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if path.is_null() {
        return fail("Invalid Input Path.\n");
    }
    if flags & !3 != 0 {
        return fail("Invalid Write Blob Flags\n");
    }
    let truncate = flags & 1 != 0;
    let mut f = flags & 2;
    let mut left = len;
    let mut off = offset;
    let mut buf = buffer;
    loop {
        let mut chunk = if left > BLOB_CHUNK {
            BLOB_CHUNK
        } else {
            if truncate {
                f |= 1;
            }
            left
        };
        let hr = unsafe { blob_io(adapter, path, off, buf as *mut u8, &mut chunk, 0x7800_0094, f) };
        if hr < 0 {
            return hr;
        }
        buf = unsafe { buf.add(chunk as usize) };
        off = off.wrapping_add(chunk as u64);
        left -= chunk;
        if left == 0 {
            return hr;
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageRemapDevice(adapter: *mut c_void, existing: u32, new: u32) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if !is_storage_id(existing) {
        return fail("Invalid XCrdIdExisting.\n");
    }
    if !is_storage_id(new) {
        return fail("Invalid XCrdIdNew.\n");
    }
    let mut r = StorageReq::new(existing, 5);
    r.value = &new;
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageSetConsoleAssociation(adapter: *mut c_void, device: u32, association: u32) -> i32 {
    let mut r = StorageReq::new(device, 0x14);
    r.association = &association;
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageSetDeviceName(adapter: *mut c_void, device: u32, name: *const u16) -> i32 {
    let mut r = StorageReq::new(device, 2);
    r.name = name;
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStorageSetLastUsedConsole(adapter: *mut c_void, device: u32) -> i32 {
    let r = StorageReq::new(device, 3);
    unsafe { storage_op(adapter, &r) }
}

#[no_mangle]
pub extern "system" fn XCrdSetTraceFlags(flags: u32) -> i32 {
    if flags & !7 != 0 {
        return nt_to_hr(STATUS_INVALID_PARAMETER);
    }
    TRACE_FLAGS.store(flags, std::sync::atomic::Ordering::Relaxed);
    0
}

#[no_mangle]
pub extern "system" fn XCrdShuffle() -> i32 {
    HR_NOTIMPL
}

#[no_mangle]
pub extern "system" fn XCrdWaitForAllRemovableDiskRemoval() -> i32 {
    HR_NOTIMPL
}

#[no_mangle]
pub extern "system" fn XCrdWaitForAnyRemovableDiskArrival() -> i32 {
    HR_NOTIMPL
}

#[no_mangle]
pub extern "system" fn XCrdTrimDeallocatedClusters() -> i32 {
    E_NOTSUPPORTED
}
