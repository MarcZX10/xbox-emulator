use crate::xcrd_core::*;
use core::ffi::c_void;

#[repr(C)]
pub struct StorageRequest {
    pub id: u32,
    pub op: u32,
    pub s8: *const u16,
    pub s10: *const u16,
    pub s18: *const u16,
    pub s20: *const u16,
    pub p28: *const u32,
    pub p30: *const u8,
    pub p38: *const u32,
    pub p40: *const u32,
    pub p48: *const u8,
    pub p50: *const i32,
    pub out58: *mut u8,
    pub p60: *const u8,
}

impl Default for StorageRequest {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

pub const STORAGE_REPLY: usize = 0x430;

pub const INTERNAL_DEVICES: [([u8; 16], u32); 2] = [
    ([0x52, 0xce, 0x9e, 0xa8, 0x8e, 0x7e, 0x4f, 0x44, 0xbb, 0xd0, 0xc6, 0x8b, 0x76, 0xc2, 0xec, 0xa4], 1),
    ([0x55, 0x2f, 0x2b, 0xeb, 0xb3, 0x83, 0x1e, 0x40, 0xad, 0x7c, 0x13, 0x96, 0xd7, 0xd5, 0xb3, 0x0d], 0x24),
];

fn bad(msg: &str) -> i32 {
    trace(1, || msg.to_string());
    E_INVALIDARG
}

unsafe fn guid_at(p: *const u8) -> [u8; 16] {
    let mut g = [0u8; 16];
    unsafe { core::ptr::copy_nonoverlapping(p, g.as_mut_ptr(), 16) };
    g
}

pub unsafe fn storage_request(adapter: *const c_void, r: *const StorageRequest) -> i32 {
    if r.is_null() {
        return E_INVALIDARG;
    }
    let r = unsafe { &*r };
    let long = |p: *const u16, max: usize| !p.is_null() && unsafe { wlen(p) } >= max;
    if long(r.s8, 0x21) || long(r.s10, 0x104) || long(r.s18, 0x104) || long(r.s20, 0x104) {
        return E_INVALIDARG;
    }
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if !r.p38.is_null() && unsafe { *r.p38 } >= 2 {
        return bad("Invalid External Storage Flags");
    }
    let id = r.id;
    let ok = match r.op {
        1 | 0xc => !r.p30.is_null() && unsafe { XCrdIsInternalStorageDevice(r.p30, core::ptr::null_mut()) } == 0,
        5 => !r.p28.is_null(),
        6 => !r.p30.is_null(),
        8 => !r.s10.is_null(),
        9 | 0x12 | 0x13 => !r.s18.is_null(),
        0xd => !r.p40.is_null() && !INTERNAL_DEVICES.iter().any(|d| d.1 == id),
        0xe => !r.p48.is_null(),
        0xf => !r.s18.is_null() && (id.wrapping_sub(0x13) <= 7 || id == 0xffff_ffff),
        0x10 => !r.p30.is_null() && !r.p60.is_null(),
        0x11 => !r.p30.is_null() && !r.out58.is_null(),
        0x14 => !r.p50.is_null() && unsafe { *r.p50 } < 3 && id.wrapping_sub(0x13) <= 7,
        0x15 => !r.s18.is_null() && !r.s20.is_null(),
        _ => true,
    };
    if !ok {
        return bad("Invalid parameter.");
    }
    let op = r.op;
    let mut m = XvdMsg::new(0x7800_0088, STORAGE_REPLY);
    m.set_u32(4, id);
    m.set_u32(0, op);
    let raw = |m: &mut XvdMsg, off: usize, s: *const u16| {
        let n = unsafe { wlen(s) };
        unsafe { m.set_raw(off, s as *const u8, n * 2 + 2) };
    };
    if (op.wrapping_sub(1) <= 1 || op == 0xc) && !r.s8.is_null() {
        raw(&mut m, 0x20, r.s8);
    }
    if op <= 0x13 && (0xc8210u32 >> op) & 1 != 0 && !r.s18.is_null() {
        raw(&mut m, 0x20, r.s18);
    }
    if op == 0x15 {
        if !r.s18.is_null() && !r.s20.is_null() {
            raw(&mut m, 0x20, r.s18);
            raw(&mut m, 0x228, r.s20);
        }
    } else if op == 5 {
        if !r.p28.is_null() {
            m.set_u32(8, unsafe { *r.p28 });
        }
    } else {
        if op <= 0xc && (0x1042u32 >> op) & 1 != 0 && !r.p30.is_null() {
            m.set_bytes(0xc, &unsafe { guid_at(r.p30) });
        }
        match op {
            8 => {
                if !r.s10.is_null() {
                    raw(&mut m, 0x20, r.s10);
                }
            }
            0xa => m.set_u64(0x20, unsafe { (r.p38 as *const u64).read_unaligned() }),
            0xd => m.set_u32(0x20, unsafe { *r.p40 }),
            0xe => unsafe { m.set_raw(0x20, r.p48, 0x218) },
            0x10 => {
                m.set_bytes(0xc, &unsafe { guid_at(r.p30) });
                m.set_bytes(0x20, &unsafe { guid_at(r.p60) });
            }
            _ => {}
        }
    }
    if op == 0x11 {
        m.set_bytes(0xc, &unsafe { guid_at(r.p30) });
    } else if op == 0x14 {
        m.set_u32(0x20, unsafe { *r.p50 } as u32);
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 && !r.out58.is_null() {
        unsafe { m.get_raw(0, r.out58, STORAGE_REPLY) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdIsInternalStorageDevice(guid: *const u8, out_id: *mut u32) -> u32 {
    let g = unsafe { guid_at(guid) };
    for (dg, id) in INTERNAL_DEVICES.iter() {
        if *dg == g {
            if !out_id.is_null() {
                unsafe { *out_id = *id };
            }
            return 1;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdHasInternalDeviceId(id: u32, out_guid: *mut u8) -> u32 {
    for (dg, did) in INTERNAL_DEVICES.iter() {
        if *did == id {
            if !out_guid.is_null() {
                unsafe { core::ptr::copy_nonoverlapping(dg.as_ptr(), out_guid, 16) };
            }
            return 1;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdSetEraLaunchMapping(adapter: *mut c_void, path: *const u16, id: u32) -> i32 {
    let r = StorageRequest { id, op: 4, s18: path, ..Default::default() };
    unsafe { storage_request(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdSetExternalPlsMapping(adapter: *mut c_void, path: *const u16, id: u32) -> i32 {
    let r = StorageRequest { id, op: 0xf, s18: path, ..Default::default() };
    unsafe { storage_request(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdFindStorageDevice(adapter: *mut c_void, guid: *const u8, out: *mut u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if out.is_null() {
        return bad("Invalid CRD ID pointer.");
    }
    let mut reply = [0u8; STORAGE_REPLY];
    let r = StorageRequest { id: 0x25, op: 6, p30: guid, out58: reply.as_mut_ptr(), ..Default::default() };
    let hr = unsafe { storage_request(adapter, &r) };
    if hr >= 0 {
        unsafe { *out = u32::from_le_bytes(reply[8..12].try_into().unwrap()) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdSetDeviceIdMapping(adapter: *mut c_void, guid: *const u8, target: *const u8) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    let r = StorageRequest { id: 0x25, op: 0x10, p30: guid, p60: target, ..Default::default() };
    unsafe { storage_request(adapter, &r) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdGetDeviceIdMapping(adapter: *mut c_void, guid: *const u8, out: *mut u8) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if out.is_null() {
        return bad("Target DeviceId is null.");
    }
    let mut reply = [0u8; STORAGE_REPLY];
    let r = StorageRequest { id: 0x25, op: 0x11, p30: guid, out58: reply.as_mut_ptr(), ..Default::default() };
    let hr = unsafe { storage_request(adapter, &r) };
    if hr >= 0 {
        unsafe { core::ptr::copy_nonoverlapping(reply[0x20..0x30].as_ptr(), out, 16) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdQueryAttestationBlob(adapter: *mut c_void, guid: *const u8, out: *mut u8) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    let mut m = XvdMsg::new(0x7800_009c, 0x80);
    m.set_u8(0, 1);
    m.set_bytes(4, &unsafe { guid_at(guid) });
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe {
            m.get_raw(0x40, out.add(0x40), 0x40);
            *out = 1;
            m.get_raw(0x14, out.add(0x14), 0x2c);
        }
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdResizeXVD(adapter: *mut c_void, path: *const u16, size: i64) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if size <= 0 {
        return bad("Invalid resize size.");
    }
    let mut dev: *mut u16 = core::ptr::null_mut();
    let mut hr = unsafe { crate::xcrd_a::XCrdQueryDevicePathByPath(adapter, path, &mut dev) };
    if hr < 0 {
        if !dev.is_null() {
            unsafe { crate::xcrd_a::XCrdFreeDevicePathBuffer(dev as *mut c_void) };
        }
        return hr;
    }
    let devw = unsafe { wslice(dev) }.to_vec();
    let h = host(|x| x.create_file(&devw, GENERIC_READ | GENERIC_WRITE, SHARE_RW, OPEN_EXISTING, 0));
    let mut go = true;
    if h == INVALID_HANDLE {
        hr = last_error_hr();
        go = hr >= 0;
    }
    let mut fs = [0u8; 0x18];
    if go {
        if host(|x| x.query_fs_size(h, &mut fs)) < 0 {
            hr = last_error_hr();
            go = hr >= 0;
        }
    }
    if go {
        hr = 'body: {
            let n = unsafe { wlen(path) };
            let len = n * 2 + 0x1a;
            let mut m = XvdMsg::new(0x7800_00a0, len);
            m.set_u32(0, len as u32);
            m.set_u32(4, 0x18);
            m.set_u32(8, (n * 2) as u32);
            m.set_u64(0x10, size as u64);
            let st = unsafe { m.copy_wstr(0x18, (n + 1) * 2, path) };
            if st < 0 {
                break 'body st;
            }
            let mut r = unsafe { m.send_adapter(adapter) };
            if r < 0 {
                break 'body r;
            }
            let mut ret = 0u32;
            if !host(|x| x.ioctl(h, 0x7_0140, &[], &mut [], &mut ret)) {
                r = last_error_hr();
                if r < 0 {
                    break 'body r;
                }
            }
            let mut grow = [0u8; 0x10];
            grow[0..4].copy_from_slice(&1u32.to_le_bytes());
            grow[8..16].copy_from_slice(&size.to_le_bytes());
            if !host(|x| x.ioctl(h, 0x7_c0d0, &grow, &mut [], &mut ret)) {
                r = last_error_hr();
                if r < 0 {
                    break 'body r;
                }
            }
            let bps = u32::from_le_bytes(fs[0x14..0x18].try_into().unwrap()) as u64;
            let total = u64::from_le_bytes(fs[0..8].try_into().unwrap());
            let sectors = (size as u64).checked_div(bps).unwrap_or(0).wrapping_add(total);
            if !host(|x| x.ioctl(h, 0x9_00f0, &sectors.to_le_bytes(), &mut [], &mut ret)) {
                r = last_error_hr();
            }
            r
        };
    }
    if h != INVALID_HANDLE {
        host(|x| x.close_handle(h));
    }
    if !dev.is_null() {
        unsafe { crate::xcrd_a::XCrdFreeDevicePathBuffer(dev as *mut c_void) };
    }
    hr
}
