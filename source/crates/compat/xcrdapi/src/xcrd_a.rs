use crate::xcrd_core::*;
use core::ffi::c_void;

pub const HR_INSUFFICIENT_BUFFER: i32 = 0x8007_007au32 as i32;
pub const HR_NOT_READY: i32 = 0x8007_0015u32 as i32;
pub const HR_WRITE_PROTECT: i32 = 0x8007_0013u32 as i32;
pub const E_POINTER_HR: i32 = 0x8000_4003u32 as i32;
pub const E_OUTOFMEMORY_HR: i32 = 0x8007_000eu32 as i32;

fn bad(msg: &str) -> i32 {
    trace(1, || msg.to_string());
    E_INVALIDARG
}

pub fn console_is_server() -> bool {
    false
}

#[no_mangle]
pub unsafe extern "system" fn XCrdCreateXVD(
    adapter: *mut c_void,
    path: *const u16,
    id: *const u8,
    flags: u32,
    size: u64,
    xvd_type: u32,
    content_type: u32,
    out_id: *mut u8,
    id2: *const u8,
    sandbox: *const u8,
    sandbox_len: u32,
    path2: *const u16,
) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if path.is_null() {
        return bad("Invalid Input Path.");
    }
    if flags & 0xffcc_0c18 != 0 {
        return bad("Invalid Create Flags.");
    }
    let roam = flags & 0x20000;
    if roam != 0 && !console_is_server() {
        trace(1, || "Streaming roamable XVDs can only be created on server consoles.".into());
        return E_NOTSUPPORTED;
    }
    if flags & 0x30000 == 0x30000 {
        return bad("Incompatible Create Flags.");
    }
    if !sandbox.is_null() && sandbox_len > 0x10 {
        return bad("Invalid Sandbox ID.");
    }
    let bytes = size.wrapping_shl(12);
    if bytes.wrapping_sub(0xa0_0000) > 0xff_ff60_0000 || bytes < size {
        return bad("Invalid Drive Size. Size must be between 1MB and 1TB, and multiples of 512 bytes.");
    }
    if xvd_type > 1 {
        return bad("Invalid XVD type.");
    }
    if content_type > 0x27 {
        return bad("Invalid XVD content type.");
    }
    let trim = flags & 0x10_0000;
    if trim != 0 && !(xvd_type == 1 && content_type == 0x22 && flags & 2 == 0) {
        return bad("Invalid flags. Cannot enable trim unless the XVD is dynamic and thin-provisioned, and has integrity disabled");
    }
    let n = unsafe { wlen(path) };
    let mut len = n * 2 + 0x72;
    let mut p2 = path2;
    if !p2.is_null() {
        if unsafe { *p2 } == 0 {
            p2 = core::ptr::null();
        } else {
            len += unsafe { wlen(p2) } * 2 + 2;
        }
    }
    let mut m = XvdMsg::new(0x7800_0020, len);
    m.set_u32(0, len as u32);
    m.set_u32(0x1c, content_type);
    let mut f = 0u32;
    if flags & 1 != 0 {
        f = 4;
    }
    if flags & 2 != 0 {
        f |= 2;
    }
    if flags & 4 != 0 {
        f |= 1;
    }
    if flags & 0x20 != 0 {
        f |= 8;
    }
    if flags & 0x40 != 0 {
        f |= 0x10;
    }
    if flags & 0x100 != 0 {
        f |= 0x40;
    }
    if flags & 0x200 != 0 {
        f |= 0x80;
    }
    if flags & 0x1_0000 != 0 {
        f |= 0x1000;
    }
    if roam != 0 {
        f |= 0x2000;
    }
    if trim != 0 {
        f |= 0x4000;
    }
    if flags & 0x20_0000 != 0 {
        f |= 0x8000;
    }
    match flags & 0xf080 {
        0 => {}
        0x80 => f |= 0x20,
        0x1000 => f |= 0x100,
        0x1080 => f |= 0x120,
        _ => {
            m.set_u32(4, f);
            return E_INVALIDARG;
        }
    }
    m.set_u32(4, f);
    m.set_u64(0x20, bytes >> 12);
    m.set_u32(0x18, xvd_type);
    m.set_u32(0x28, 0x70);
    m.set_u32(0x2c, (n * 2) as u32);
    m.set_u64(0x30, 0);
    if !id.is_null() {
        unsafe { m.set_raw(8, id, 16) };
    }
    if !id2.is_null() {
        unsafe { m.set_raw(0x40, id2, 16) };
    }
    if !sandbox.is_null() {
        unsafe { m.set_raw(0x50, sandbox, sandbox_len as usize) };
    }
    let st = unsafe { m.copy_wstr(0x70, (n + 1) * 2, path) };
    if st < 0 {
        return st;
    }
    if !p2.is_null() {
        let off = m.u32(0x28) + m.u32(0x2c) + 2;
        m.set_u32(0x30, off);
        let n2 = unsafe { wlen(p2) };
        m.set_u32(0x34, (n2 * 2) as u32);
        let st = unsafe { m.copy_wstr(off as usize, (n2 + 1) * 2, p2) };
        if st < 0 {
            return st;
        }
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 && !out_id.is_null() {
        unsafe { m.get_raw(0x60, out_id, 16) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdOpenAdapter(out: *mut *mut c_void) -> i32 {
    let mut hr = 0;
    for i in 0..0x40u32 {
        let mut name = [0u16; 0x20];
        let st = nt_to_hr(cch_printf(&mut name, &format!("\\\\.\\Scsi{i}:")));
        if st < 0 {
            return st;
        }
        let h = host(|x| x.create_file(zstr(&name), 0xc010_0000, 0, OPEN_EXISTING, 0));
        if h == INVALID_HANDLE {
            hr = last_error_hr();
            continue;
        }
        let mut srb = [0u8; SRB_HEADER];
        srb[0..4].copy_from_slice(&0x1cu32.to_le_bytes());
        srb[0x10..0x14].copy_from_slice(&0x7800_000cu32.to_le_bytes());
        srb[0x14..0x18].copy_from_slice(&STATUS_IO_DEVICE_ERROR.to_le_bytes());
        let nt = host(|x| x.nt_ioctl(h, IOCTL_SCSI_MINIPORT, &mut srb));
        let rc = i32::from_le_bytes(srb[0x14..0x18].try_into().unwrap());
        let status = if nt < 0 {
            nt
        } else if rc < 0 {
            rc
        } else if srb[4..12] == XVD_SIGNATURE {
            0
        } else {
            STATUS_IO_DEVICE_ERROR
        };
        hr = nt_to_hr(status);
        if hr >= 0 {
            let a = unsafe { heap_alloc(core::mem::size_of::<Adapter>()) } as *mut Adapter;
            if !a.is_null() {
                unsafe {
                    a.write(Adapter { handle: h, index: i });
                    *out = a as *mut c_void;
                }
                return hr;
            }
            hr = E_OUTOFMEMORY_NT;
        }
        host(|x| x.close_handle(h));
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdCloseAdapter(adapter: *mut c_void) -> i32 {
    let h = unsafe { adapter_handle(adapter) };
    if !host(|x| x.close_handle(h)) {
        return last_error_hr();
    }
    unsafe { heap_free(adapter) };
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdQueryDevicePath(out: *mut *mut u16, dp: *const DevicePath) -> i32 {
    if dp.is_null() {
        return E_INVALIDARG;
    }
    let src = unsafe { (*dp).path };
    let cb = unsafe { wlen(src) } * 2 + 2;
    let p = unsafe { heap_alloc(cb) } as *mut u16;
    unsafe { *out = p };
    if p.is_null() {
        return E_OUTOFMEMORY_NT;
    }
    let hr = unsafe { cch_copy(p, cb, src) };
    if hr < 0 {
        unsafe {
            heap_free(p as *mut c_void);
            *out = core::ptr::null_mut();
        }
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdQueryDevicePathByPath(adapter: *mut c_void, path: *const u16, out: *mut *mut u16) -> i32 {
    if path.is_null() {
        return E_INVALIDARG;
    }
    let mut addr = ScsiAddress::default();
    let hr = unsafe { scsi_address_for_path(adapter, path, &mut addr) };
    if hr < 0 {
        return hr;
    }
    let mut disk = 0u32;
    let hr = disk_number_for_address(&mut disk, &addr);
    if hr < 0 {
        return hr;
    }
    let mut dp: *mut DevicePath = core::ptr::null_mut();
    let mut hr = unsafe { device_path_for_disk(&mut dp, disk, &addr) };
    if hr >= 0 {
        hr = unsafe { XCrdQueryDevicePath(out, dp) };
    }
    if !dp.is_null() {
        unsafe { heap_free(dp as *mut c_void) };
    }
    hr
}

pub fn is_corruption_error(hr: i32) -> bool {
    let u = hr as u32;
    u == 0x8007_01e7 || u.wrapping_sub(0x8007_0570) <= 1 || u == 0x8007_045d
}

#[no_mangle]
pub extern "system" fn XCrdIsCorruptionError(hr: i32) -> u8 {
    is_corruption_error(hr) as u8
}

unsafe fn mount(out: *mut *mut DevicePath, adapter: *mut c_void, path: *const u16, flags: u32, content_type: *const u32) -> i32 {
    let n = unsafe { wlen(path) };
    if n == 0 {
        return E_INVALIDARG;
    }
    let mut m = XvdMsg::new(0x7800_0004, (n + 1) * 2 + 0x24);
    let mut f = 0u32;
    for (from, to) in [(1u32, 1u32), (4, 8), (0x20, 0x40), (0x40, 0x100), (0x80, 0x200), (0x400, 0x800), (0x1000, 0x2000), (0x800, 0x1000), (0x2000, 0x4000)] {
        if flags & from != 0 {
            f |= to;
        }
    }
    m.set_u32(0x18, f);
    if !content_type.is_null() {
        m.or_u32(0x18, 0x80);
        m.set_u32(0x1c, unsafe { *content_type });
    }
    let st = unsafe { m.copy_wstr(0x24, (n + 1) * 2, path) };
    if st < 0 {
        return st;
    }
    m.set_u32(0, 0x24);
    m.set_u32(4, (n * 2) as u32);
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    let addr = ScsiAddress { length: 8, port: unsafe { adapter_index(adapter) } as u8, path: m.u8(0x22), target: m.u8(0x21), lun: m.u8(0x20) };
    host(|x| x.sleep(0x19));
    let mut disk = 0u32;
    let mut hr;
    let mut tries = 0u32;
    let mut found_disk = false;
    loop {
        hr = disk_number_for_address(&mut disk, &addr);
        if hr >= 0 {
            found_disk = true;
            break;
        }
        if hr == HR_WRITE_PROTECT || is_corruption_error(hr) {
            break;
        }
        host(|x| x.sleep(100));
        tries += 1;
        if tries >= 0x640 {
            break;
        }
    }
    if found_disk {
        let start = host(|x| x.tick64());
        let mut tries = 0u32;
        loop {
            let mut dp: *mut DevicePath = core::ptr::null_mut();
            hr = unsafe { device_path_for_disk(&mut dp, disk, &addr) };
            if hr >= 0 {
                unsafe { *out = dp };
                return hr;
            }
            if hr == HR_WRITE_PROTECT || is_corruption_error(hr) {
                break;
            }
            host(|x| x.sleep(100));
            tries += 1;
            let el = host(|x| x.tick64()).wrapping_sub(start);
            if tries >= 0x320 || el >= 0xea60 {
                break;
            }
        }
    }
    unsafe { remove_lun(adapter, &addr) };
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdMount(out: *mut *mut DevicePath, adapter: *mut c_void, path: *const u16, flags: u32) -> i32 {
    unsafe { mount(out, adapter, path, flags, core::ptr::null()) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdMountContentType(out: *mut *mut DevicePath, adapter: *mut c_void, path: *const u16, content_type: u32, flags: u32) -> i32 {
    unsafe { mount(out, adapter, path, flags, &content_type) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdFreeDevicePathBuffer(p: *mut c_void) -> i32 {
    if p.is_null() {
        return E_INVALIDARG;
    }
    unsafe { heap_free(p) };
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdBuildPath(out: *mut *mut u16, index: i32, path: *const u16, flags: u32) -> i32 {
    if index > 0x24 || flags & !1 != 0 || path.is_null() {
        return E_INVALIDARG;
    }
    let pre = PATH_PREFIXES[index as usize];
    let tail = String::from_utf16_lossy(unsafe { wslice(path) });
    let mut cb = (pre.encode_utf16().count() + unsafe { wlen(path) }) * 2 + 4;
    if flags & 1 != 0 {
        cb += PATH_PREFIXES[10].len() * 2 + 2;
    }
    let p = unsafe { heap_alloc(cb) } as *mut u16;
    if p.is_null() {
        return E_OUTOFMEMORY_NT;
    }
    let text = if flags & 1 != 0 { format!("{}\\{}\\{}", PATH_PREFIXES[10], pre, tail) } else { format!("{pre}\\{tail}") };
    let dst = unsafe { core::slice::from_raw_parts_mut(p, cb / 2) };
    let hr = nt_to_hr(cch_printf(dst, &text));
    if hr >= 0 {
        unsafe { *out = p };
    } else {
        unsafe { heap_free(p as *mut c_void) };
    }
    hr
}

fn wlower(c: u16) -> u16 {
    if (0x41..=0x5a).contains(&c) {
        c + 0x20
    } else {
        c
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdParsePath(path: *const u16, out: *mut i32) -> i32 {
    if path.is_null() || out.is_null() {
        return E_INVALIDARG;
    }
    for (i, pre) in PATH_PREFIXES.iter().enumerate() {
        let w: Vec<u16> = pre.encode_utf16().collect();
        if w.is_empty() {
            continue;
        }
        let mut eq = true;
        for (k, c) in w.iter().enumerate() {
            let pc = unsafe { *path.add(k) };
            if wlower(pc) != wlower(*c) {
                eq = false;
                break;
            }
            if pc == 0 {
                break;
            }
        }
        if eq {
            unsafe { *out = i as i32 };
            return 0;
        }
    }
    E_INVALIDARG
}

#[no_mangle]
pub unsafe extern "system" fn XCrdDeleteXVD(adapter: *mut c_void, path: *const u16, flags: u32) -> i32 {
    if flags & !1 != 0 {
        return bad("Invalid Delete Flags.");
    }
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if path.is_null() {
        return bad("Invalid Input Path.");
    }
    let n = unsafe { wlen(path) };
    let len = n * 2 + 0x1a;
    let mut m = XvdMsg::new(0x7800_0024, len);
    m.set_u32(0, len as u32);
    m.set_u32(4, 0x18);
    m.set_u32(8, (n * 2) as u32);
    m.set_u32(0xc, flags);
    let st = unsafe { m.copy_wstr(0x18, (n + 1) * 2, path) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdMoveSidecarFiles(adapter: *mut c_void, src: *const u16, dst: *const u16, flags: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if src.is_null() {
        return bad("Invalid Src Input Path.");
    }
    if dst.is_null() {
        return bad("Invalid Dst Input Path.");
    }
    if flags & !1 != 0 {
        return bad("Invalid Move Flags");
    }
    let n1 = unsafe { wlen(src) };
    let n2 = unsafe { wlen(dst) };
    let len = (n1 + n2) * 2 + 0x1c;
    let mut m = XvdMsg::new(0x7800_01b0, len);
    m.set_u32(0, len as u32);
    m.set_u32(0x14, flags & 1);
    m.set_u32(4, 0x18);
    m.set_u32(8, (n1 * 2) as u32);
    let st = unsafe { m.copy_wstr(0x18, (n1 + 1) * 2, src) };
    if st < 0 {
        return st;
    }
    let off = m.u32(4) + m.u32(8) + 2;
    m.set_u32(0xc, off);
    m.set_u32(0x10, (n2 * 2) as u32);
    let st = unsafe { m.copy_wstr(off as usize, (n2 + 1) * 2, dst) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

pub unsafe fn blob_io(adapter: *mut c_void, path: *const u16, offset: u64, buffer: *mut u8, plen: *mut u32, code: u32, flags: u32) -> i32 {
    if !buffer.is_null() && unsafe { *plen } == 0 {
        return bad("Can't specify buffer with zero length");
    }
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if path.is_null() {
        return bad("Invalid Input Path.");
    }
    if buffer.is_null() && plen.is_null() {
        return bad("BufferLength must not be NULL if Buffer is NULL");
    }
    if flags & !3 != 0 {
        return bad("Invalid Read Write Flags");
    }
    let write = code == 0x7800_0094;
    if !write && flags & 3 != 0 {
        return bad("Only valid to request Truncate or Hidden file on write");
    }
    let n = unsafe { wlen(path) };
    let base = n * 2 + 0x22;
    let want = unsafe { *plen } as usize;
    let len = if want == 0 { base } else { (want + base) as u32 as usize };
    let mut m = XvdMsg::new(code, len);
    m.set_u32(0, len as u32);
    m.set_u32(4, 0x20);
    m.set_u32(8, (n * 2) as u32);
    m.set_u64(0x10, offset);
    m.set_u32(0xc, want as u32);
    m.set_u32(0x18, flags);
    let st = unsafe { m.copy_wstr(0x20, (n + 1) * 2, path) };
    if st < 0 {
        return st;
    }
    let data = 0x20 + (n + 1) * 2;
    if write {
        unsafe { m.set_raw(data, buffer, want) };
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if write {
        return hr;
    }
    if hr < 0 {
        if hr != HR_INSUFFICIENT_BUFFER {
            return hr;
        }
    } else if want != 0 {
        unsafe { m.get_raw(data, buffer, m.u32(0xc) as usize) };
    }
    unsafe { *plen = m.u32(0xc) };
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdReadUserDataXVD(adapter: *mut c_void, path: *const u16, offset: u64, buffer: *mut u8, plen: *mut u32) -> i32 {
    if !buffer.is_null() && unsafe { *plen } == 0 {
        return bad("Can't specify buffer with zero length");
    }
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if path.is_null() {
        return bad("Invalid Input Path.");
    }
    if buffer.is_null() && plen.is_null() {
        return bad("BufferLength must not be NULL if Buffer is NULL");
    }
    if offset & 0xfff != 0 {
        return bad("Offset has to be page aligned");
    }
    let mut left = unsafe { *plen };
    if left == 0 {
        return unsafe { blob_io(adapter, path, offset, buffer, plen, 0x7800_0028, 0) };
    }
    let mut total = 0u32;
    let mut off = offset;
    let mut buf = buffer;
    loop {
        let mut chunk = left.min(0x10_0000);
        let hr = unsafe { blob_io(adapter, path, off, buf, &mut chunk, 0x7800_0028, 0) };
        if hr < 0 {
            if hr == HR_INSUFFICIENT_BUFFER {
                unsafe { *plen = total };
            }
            return hr;
        }
        total = total.wrapping_add(chunk);
        buf = unsafe { buf.add(chunk as usize) };
        off = off.wrapping_add(chunk as u64);
        left = left.wrapping_sub(chunk);
        if left == 0 {
            unsafe { *plen = total };
            return hr;
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdReadBatInfoXVD(adapter: *mut c_void, path: *const u16, buffer: *mut u8, plen: *mut u32, info: *mut u8) -> i32 {
    if !buffer.is_null() && unsafe { *plen } == 0 {
        return bad("Can't specify buffer with zero length");
    }
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if path.is_null() {
        return bad("Invalid Input Path.");
    }
    if buffer.is_null() {
        if plen.is_null() {
            return bad("BufferLength must not be NULL if Buffer is NULL");
        }
        if unsafe { *plen } != 0 {
            return bad("BufferLength must not be zero if Buffer is NULL");
        }
    }
    let n = unsafe { wlen(path) };
    let want = unsafe { *plen };
    let mut len = (n * 2 + 0x4a) as u32;
    if want != 0 {
        len = len.wrapping_add(want.wrapping_add(0xfff) & 0xffff_f000);
    }
    let mut m = XvdMsg::new(0x7800_002c, len as usize);
    m.set_u32(0x20, want.wrapping_add(0xfff) & 0xffff_f000);
    m.set_u32(0x18, 0x48);
    m.set_u32(0x1c, (n * 2) as u32);
    let st = unsafe { m.copy_wstr(0x48, (n + 1) * 2, path) };
    if st < 0 {
        return st;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        if hr != HR_INSUFFICIENT_BUFFER {
            return hr;
        }
    } else if want != 0 {
        unsafe { m.get_raw(0x48 + (n + 1) * 2, buffer, m.u32(0x20) as usize) };
    }
    let put64 = |o: usize, v: u64| unsafe { (info.add(o) as *mut u64).write_unaligned(v) };
    let put32 = |o: usize, v: u32| unsafe { (info.add(o) as *mut u32).write_unaligned(v) };
    put64(0, m.u64(8));
    put64(8, m.u64(0));
    put32(0x10, m.u32(0x14));
    put32(0x14, m.u32(0x10));
    put64(0x18, m.u64(0x28));
    put32(0x24, m.u32(0x34));
    put32(0x20, m.u32(0x30));
    put64(0x28, m.u64(0x38));
    put32(0x30, m.u32(0x40));
    unsafe { *plen = m.u32(0x20) };
    hr
}

#[no_mangle]
pub unsafe extern "system" fn DStorageGetMappedControlPage(out: *mut *mut c_void) -> i32 {
    let page = crate::xcrd_driver::control_page().map(|c| c.page).unwrap_or(0);
    unsafe { *out = page as *mut c_void };
    0
}

#[no_mangle]
pub extern "system" fn XCrdQueryEncryptedInfo() -> i32 {
    E_NOTSUPPORTED
}

#[no_mangle]
pub extern "system" fn XCrdQueryUnlockInfo() -> i32 {
    E_NOTSUPPORTED
}

#[no_mangle]
pub extern "system" fn XCrdRepairFiles() -> i32 {
    E_NOTSUPPORTED
}

#[no_mangle]
pub unsafe extern "system" fn XCrdGrowThinProvisionedXvd(adapter: *mut c_void, path: *const u16, size: u64) -> i32 {
    if adapter.is_null() {
        return bad("Invalid adapter handle.");
    }
    if path.is_null() {
        return bad("Invalid input path.");
    }
    if size == 0 {
        return bad("Invalid size.");
    }
    let n = unsafe { wlen(path) };
    let len = n * 2 + 0x1a;
    let mut m = XvdMsg::new(0x7800_0170, len);
    m.set_u32(0, len as u32);
    m.set_u32(4, 0x18);
    m.set_u32(8, (n * 2) as u32);
    m.set_u64(0x10, size);
    let st = unsafe { m.copy_wstr(0x18, (n + 1) * 2, path) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdResetStreamedState(adapter: *mut c_void, path: *const u16, flags: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if path.is_null() {
        return bad("Path must be specified");
    }
    if flags == 0 {
        return bad("RegionId must be specified");
    }
    let n = unsafe { wlen(path) };
    let mut m = XvdMsg::new(0x7800_004c, n * 2 + 0xe);
    m.set_u32(0, flags);
    m.set_u32(4, 0xc);
    m.set_u32(8, (n * 2) as u32);
    let st = unsafe { m.copy_wstr(0xc, n * 2 + 2, path) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdOddPmAcquireReleaseReference(out: *mut u32, adapter: *mut c_void, acquire: u8) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    let mut m = XvdMsg::new(0x7800_0098, 8);
    m.set_u8(4, acquire);
    let hr = unsafe { m.send_adapter(adapter) };
    if !out.is_null() {
        unsafe { *out = m.u32(0) };
    }
    hr
}

unsafe fn put32(o: *mut u8, off: usize, v: u32) {
    unsafe { (o.add(off) as *mut u32).write_unaligned(v) }
}

unsafe fn put64(o: *mut u8, off: usize, v: u64) {
    unsafe { (o.add(off) as *mut u64).write_unaligned(v) }
}

unsafe fn put16(o: *mut u8, off: usize, v: u16) {
    unsafe { (o.add(off) as *mut u16).write_unaligned(v) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdQueryCrdInformation(adapter: *mut c_void, crd: i32, class: i32, out: *mut u8, size: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if crd >= 0x25 {
        return bad("Invalid CRD Id");
    }
    if out.is_null() {
        return bad("Output buffer must be supplied");
    }
    if class > 2 {
        return bad("Invalid InfoClass supplied");
    }
    let need = match class {
        0 | 2 => 0x10,
        1 => 0x670,
        _ => 0,
    };
    if size < need {
        return bad("Info Buffer too small");
    }
    let mut m = XvdMsg::new(0x7800_0050, 0x418);
    m.set_u64(0, 0);
    m.set_u32(8, crd as u32);
    match class {
        0 => m.set_u32(0xc, 2),
        1 => m.set_u32(0xc, 4),
        2 => m.set_u32(0xc, 0x10),
        _ => {}
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    unsafe {
        match class {
            0 => {
                put64(out, 0, m.u64(0x10));
                put64(out, 8, m.u64(0x18));
            }
            1 => {
                put64(out, 0, m.u64(0x10));
                put64(out, 8, m.u64(0x18));
                put64(out, 0x10, m.u64(0x20));
                put32(out, 0x18, m.u32(0x28));
                put32(out, 0x1c, m.u32(0x2c));
                put32(out, 0x20, m.u32(0x30));
                m.get_raw(0x34, out.add(0x24), 16);
                for i in 0..8 {
                    put32(out, 0x34 + i * 4, m.u8(0x44 + i) as u32);
                }
                for (src, dst) in [(0x4cusize, 0x54usize), (0x54, 0x25c), (0x5c, 0x464)] {
                    let off = m.u32(src);
                    let len = m.u32(src + 4);
                    if off != 0 && len < 0x104 {
                        m.get_raw(0x10 + off as usize, out.add(dst), len as usize);
                    } else {
                        put16(out, dst, 0);
                    }
                }
            }
            _ => {
                *out = m.u8(0x10);
                *out.add(6) = m.u8(0x16);
                put32(out, 8, m.u32(0x18));
                put16(out, 0xc, m.u16(0x1c));
                put16(out, 0xe, m.u16(0x1e));
            }
        }
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdQueryCrdLogInformation(adapter: *mut c_void, crd: i32, log_id: u8, param: u64, flag: u8, buffer: *mut u8, size: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if crd >= 0x25 {
        return bad("Invalid XCRDID.");
    }
    if buffer.is_null() != (size == 0) {
        return bad("Invalid buffer/length combination.");
    }
    let mut m = XvdMsg::new(0x7800_0050, size as usize + 0x418);
    m.set_u64(0, 0);
    m.set_u32(8, crd as u32);
    m.set_u32(0xc, 8);
    m.set_u8(0x10, log_id);
    m.set_u64(0x18, param);
    m.set_u8(0x20, flag);
    if !buffer.is_null() {
        m.set_u32(0x24, 0x408);
        m.set_u32(0x28, size);
    } else {
        m.set_u64(0x24, 0);
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 && !buffer.is_null() {
        unsafe { m.get_raw(0x10 + m.u32(0x24) as usize, buffer, m.u32(0x28) as usize) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdQueryCrdVendorInformation(adapter: *mut c_void, crd: i32, kind: u8, code: u32, a: u16, b: u16, buffer: *mut u8, size: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if crd >= 0x25 {
        return bad("Invalid XCRDID.");
    }
    if buffer.is_null() || size == 0 {
        return bad("Buffer must be provided.");
    }
    let mut m = XvdMsg::new(0x7800_0050, size as usize + 0x418);
    m.set_u32(0, 0);
    m.set_u32(4, 0);
    m.set_u32(8, crd as u32);
    m.set_u32(0xc, 0x11);
    m.set_u8(0x10, kind);
    m.set_u32(0x14, code);
    m.set_u16(0x18, a);
    m.set_u16(0x1a, b);
    m.set_u32(0x1c, 0x408);
    m.set_u32(0x20, size);
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe { m.get_raw(0x10 + m.u32(0x1c) as usize, buffer, m.u32(0x20) as usize) };
    }
    hr
}

pub const XVD_INFO_MIN: [u32; 21] = [0x80, 0x402, 8, 0x80, 8, 0x18, 1, 0x168, 0x158, 1, 0x38, 0x150, 1, 0x1f8, 0x1f8, 0x1f8, 0x1f8, 1, 8, 0x20a, 0x40];
pub const XVD_INFO_CODE: [u32; 21] = [0, 1, 3, 5, 6, 7, 9, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x1a, 0x1b];

unsafe fn copy_fields(m: &XvdMsg, out: *mut u8, list: &[(usize, usize, usize)]) {
    for &(o, p, n) in list {
        unsafe { m.get_raw(p, out.add(o), n) };
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdQueryXvdInformation(adapter: *mut c_void, path: *const u16, class: i32, out: *mut u8, size: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if path.is_null() {
        return bad("Path must be specified");
    }
    if out.is_null() {
        return bad("XvdInfo must be supplied");
    }
    if class >= 0x15 {
        return bad("Invalid InfoClass supplied");
    }
    if class >= 0 && size < XVD_INFO_MIN[class as usize] {
        return bad("Info Buffer too small");
    }
    let n = unsafe { wlen(path) };
    let mut m = XvdMsg::new(0x7800_0050, n * 2 + 0x41a);
    m.set_u32(0, 0x418);
    m.set_u32(4, (n * 2) as u32);
    let st = unsafe { m.copy_wstr(0x418, n * 2 + 2, path) };
    if class >= 0 {
        m.set_u32(0xc, XVD_INFO_CODE[class as usize]);
    }
    if st < 0 {
        return st;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    unsafe {
        match class {
            0 | 3 => copy_fields(&m, out, &[
                (8, 0x18, 4), (0x10, 0x20, 8), (0x18, 0x28, 8), (0xc, 0x1c, 4), (0, 0x10, 2), (2, 0x12, 1), (0x50, 0x60, 8),
                (4, 0x14, 4), (0x40, 0x50, 16), (0x30, 0x40, 16), (0x20, 0x30, 16), (0x58, 0x68, 16), (0x68, 0x78, 16), (0x78, 0x88, 8),
            ]),
            1 => {
                let c = m.u16(0x10);
                put16(out, 0, c);
                m.get_raw(0x12, out.add(2), (c as usize) << 4);
            }
            2 | 18 => put64(out, 0, m.u64(0x10)),
            4 => copy_fields(&m, out, &[(0, 0x10, 4), (4, 0x14, 4)]),
            5 => copy_fields(&m, out, &[(0, 0x10, 8), (8, 0x18, 16)]),
            6 | 9 | 12 | 17 => *out = m.u8(0x10),
            7 => {
                copy_fields(&m, out, &[(0, 0x10, 0x38), (0x38, 0x48, 4), (0x3c, 0x4c, 4), (0x40, 0x50, 4), (0x44, 0x54, 4)]);
                for k in 0..9 {
                    let o = 0x4c + k * 4;
                    copy_fields(&m, out, &[(o, o + 0x10, 4)]);
                }
            }
            8 => copy_fields(&m, out, &[
                (0, 0x10, 0x20), (0x20, 0x30, 4), (0x24, 0x34, 4), (0x28, 0x38, 4), (0x2c, 0x3c, 4), (0x30, 0x40, 4), (0x34, 0x44, 4),
                (0x38, 0x48, 8), (0x40, 0x50, 8), (0x48, 0x58, 2), (0x4a, 0x5a, 2), (0x4c, 0x5c, 4), (0x50, 0x60, 4), (0x54, 0x64, 4),
                (0x58, 0x68, 8), (0x60, 0x70, 4),
            ]),
            10 => copy_fields(&m, out, &[(0, 0x10, 8), (0x10, 0x20, 8), (0x18, 0x28, 8), (8, 0x18, 8), (0x20, 0x30, 4), (0x24, 0x34, 16)]),
            11 => {
                copy_fields(&m, out, &[
                    (0, 0x10, 0x30), (0x30, 0x40, 4), (0x34, 0x44, 4), (0x38, 0x48, 4), (0x40, 0x50, 8), (0x48, 0x58, 4), (0x4c, 0x5c, 4),
                    (0x50, 0x60, 4), (0x58, 0x68, 8), (0x60, 0x70, 4), (0x64, 0x74, 4), (0x68, 0x78, 8), (0x70, 0x80, 8),
                ]);
                let mut o = 0x78;
                while o <= 0xb0 {
                    copy_fields(&m, out, &[(o, o + 0x10, 4)]);
                    o += 4;
                }
            }
            13..=16 => m.get_raw(0x10, out, 0x15 * 0x18),
            19 => {
                *out = m.u8(0x10);
                let mut k = 0usize;
                while m.u16(0x12 + k * 2) != 0 {
                    k += 1;
                }
                m.get_raw(0x12, out.add(2), k * 2 + 2);
            }
            20 => copy_fields(&m, out, &[(0, 0x10, 8), (8, 0x18, 8), (0x10, 0x20, 8)]),
            _ => {}
        }
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCloudQueryFeatureBits(adapter: *mut c_void, out: *mut u64) -> i32 {
    let p = wz("[XUC:]\\ANYXVC");
    unsafe { XCrdQueryXvdInformation(adapter, p.as_ptr(), 0x12, out as *mut u8, 8) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdCancelUpdate(adapter: *mut c_void, path: *const u16) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if path.is_null() {
        return bad("Path must be specified");
    }
    let n = unsafe { wlen(path) };
    let len = n * 2 + 0xa;
    if len > 0x10_0000 {
        return bad("Payload too large for XVIO");
    }
    let mut m = XvdMsg::new(0x7800_0114, len);
    m.set_u32(0, 8);
    m.set_u32(4, (n * 2) as u32);
    let st = unsafe { m.copy_wstr(8, n * 2 + 2, path) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

#[link(name = "ntdll")]
extern "system" {
    fn RtlQueryWnfStateData(stamp: *mut u32, name: u64, cb: WnfCallback, ctx: *mut c_void, type_id: *const c_void) -> i32;
    fn RtlSubscribeWnfStateChangeNotification(sub: *mut *mut c_void, name: u64, stamp: u32, cb: WnfCallback, ctx: *mut c_void, type_id: *const c_void, group: u32, flags: u32) -> i32;
    fn RtlUnsubscribeWnfNotificationWaitForCompletion(sub: *mut c_void) -> i32;
}

pub type WnfCallback = unsafe extern "system" fn(u64, u32, *const c_void, *mut c_void, *const u8, u32) -> i32;
pub type UserCallback = unsafe extern "system" fn(*mut c_void, u32);

pub const WNF_EXTERNAL_STORAGE: u64 = 0x1989_0c35_a3bc_f075;
pub const WNF_WRITE_BUDGET: u64 = 0x1895_0c35_a3bc_2075;
pub const WNF_CORRUPTION: u64 = 0x1989_0c35_a3bf_8875;

#[repr(C)]
pub struct WnfRegistration {
    pub subscription: *mut c_void,
    pub context: *mut c_void,
    pub callback: Option<UserCallback>,
}

unsafe fn call_user(reg: *mut c_void, value: u32) {
    let r = reg as *const WnfRegistration;
    if let Some(cb) = unsafe { (*r).callback } {
        unsafe { cb((*r).context, value) };
    }
}

pub unsafe extern "system" fn external_storage_thunk(_n: u64, _s: u32, _t: *const c_void, ctx: *mut c_void, buf: *const u8, _len: u32) -> i32 {
    let lo = unsafe { (buf as *const u16).read_unaligned() } as u32;
    let hi = unsafe { (buf.add(2) as *const u16).read_unaligned() } as u32;
    unsafe { call_user(ctx, lo | (hi << 16)) };
    0
}

pub unsafe extern "system" fn write_budget_thunk(_n: u64, _s: u32, _t: *const c_void, ctx: *mut c_void, buf: *const u8, _len: u32) -> i32 {
    let v = unsafe { (buf as *const u16).read_unaligned() } as u32;
    unsafe { call_user(ctx, v) };
    0
}

pub unsafe extern "system" fn corruption_thunk(_n: u64, _s: u32, _t: *const c_void, ctx: *mut c_void, _buf: *const u8, _len: u32) -> i32 {
    unsafe { call_user(ctx, 0) };
    0
}

pub trait Wnf {
    fn query(&mut self, name: u64, cb: WnfCallback, ctx: *mut c_void, stamp: &mut u32) -> i32;
    fn subscribe(&mut self, name: u64, stamp: u32, cb: WnfCallback, ctx: *mut c_void, sub: &mut *mut c_void) -> i32;
    fn unsubscribe(&mut self, sub: *mut c_void) -> i32;
}

pub struct RealWnf;

impl Wnf for RealWnf {
    fn query(&mut self, name: u64, cb: WnfCallback, ctx: *mut c_void, stamp: &mut u32) -> i32 {
        unsafe { RtlQueryWnfStateData(stamp, name, cb, ctx, core::ptr::null()) }
    }
    fn subscribe(&mut self, name: u64, stamp: u32, cb: WnfCallback, ctx: *mut c_void, sub: &mut *mut c_void) -> i32 {
        unsafe { RtlSubscribeWnfStateChangeNotification(sub, name, stamp, cb, ctx, core::ptr::null(), 0, 0) }
    }
    fn unsubscribe(&mut self, sub: *mut c_void) -> i32 {
        unsafe { RtlUnsubscribeWnfNotificationWaitForCompletion(sub) }
    }
}

#[cfg(test)]
thread_local! {
    pub static WNF_OVERRIDE: std::cell::RefCell<Option<Box<dyn Wnf>>> = std::cell::RefCell::new(None);
}

pub fn wnf<R>(f: impl FnOnce(&mut dyn Wnf) -> R) -> R {
    #[cfg(test)]
    {
        let mut f = Some(f);
        let r = WNF_OVERRIDE.with(|o| o.borrow_mut().as_mut().map(|w| (f.take().unwrap())(w.as_mut())));
        if let Some(r) = r {
            return r;
        }
        return (f.take().unwrap())(&mut crate::xcrd_driver::EmuWnf { drv: crate::xcrd_driver::active() });
    }
    #[cfg(not(test))]
    f(&mut crate::xcrd_driver::EmuWnf { drv: crate::xcrd_driver::active() })
}

unsafe fn register(name: u64, thunk: WnfCallback, cb: Option<UserCallback>, ctx: *mut c_void, out: *mut *mut c_void) -> i32 {
    let r = unsafe { heap_alloc(core::mem::size_of::<WnfRegistration>()) } as *mut WnfRegistration;
    if r.is_null() {
        return E_OUTOFMEMORY_NT;
    }
    unsafe { r.write(WnfRegistration { subscription: core::ptr::null_mut(), context: ctx, callback: cb }) };
    let mut stamp = 0u32;
    let mut st = wnf(|w| w.query(name, thunk, r as *mut c_void, &mut stamp));
    if st >= 0 {
        let mut sub = core::ptr::null_mut();
        st = wnf(|w| w.subscribe(name, stamp, thunk, r as *mut c_void, &mut sub));
        unsafe { (*r).subscription = sub };
        if st >= 0 {
            unsafe { *out = r as *mut c_void };
            return nt_to_hr(st);
        }
    }
    unsafe { heap_free(r as *mut c_void) };
    nt_to_hr(st)
}

#[no_mangle]
pub unsafe extern "system" fn XCrdRegisterExternalStorageChangeCallback(cb: Option<UserCallback>, ctx: *mut c_void, out: *mut *mut c_void) -> i32 {
    unsafe { register(WNF_EXTERNAL_STORAGE, external_storage_thunk, cb, ctx, out) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdRegisterWriteBudgetCallback(cb: Option<UserCallback>, ctx: *mut c_void, out: *mut *mut c_void) -> i32 {
    unsafe { register(WNF_WRITE_BUDGET, write_budget_thunk, cb, ctx, out) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdRegisterCorruptionCallback(cb: Option<UserCallback>, out: *mut *mut c_void) -> i32 {
    unsafe { register(WNF_CORRUPTION, corruption_thunk, cb, core::ptr::null_mut(), out) }
}

unsafe fn unregister(reg: *mut c_void) -> i32 {
    let sub = unsafe { (*(reg as *const WnfRegistration)).subscription };
    let st = wnf(|w| w.unsubscribe(sub));
    unsafe { heap_free(reg) };
    nt_to_hr(st)
}

#[no_mangle]
pub unsafe extern "system" fn XCrdUnregisterWriteBudgetCallback(reg: *mut c_void) -> i32 {
    unsafe { unregister(reg) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdUnregisterExternalStorageChangeCallback(reg: *mut c_void) -> i32 {
    unsafe { unregister(reg) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdUnregisterCorruptionCallback(reg: *mut c_void) -> i32 {
    unsafe { unregister(reg) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdDefragXvc(adapter: *mut c_void, path: *const u16, flags: u32, o1: *mut u32, o2: *mut u32, o3: *mut u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if path.is_null() {
        return bad("XCrdPath must be specified");
    }
    if flags & !1 != 0 {
        return bad("Invalid Flags");
    }
    for o in [o1, o2, o3] {
        if !o.is_null() {
            unsafe { *o = 0 };
        }
    }
    let n = unsafe { wlen(path) };
    let len = n * 2 + 0x42;
    if len > 0x10_0000 {
        return bad("Payload too large for XVIO");
    }
    let mut m = XvdMsg::new(0x7800_0150, len);
    m.set_u32(8, flags & 1);
    m.set_u32(0, 0x40);
    m.set_u32(4, (n * 2) as u32);
    let st = unsafe { m.copy_wstr(0x40, n * 2 + 2, path) };
    if st < 0 {
        return st;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    for (o, off) in [(o1, 0x10), (o2, 0x14), (o3, 0x18)] {
        if !o.is_null() {
            unsafe { *o = m.u32(off) };
        }
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdOpenMetadataHandle(out: *mut *mut u32, adapter: *mut c_void, path: *const u16) -> i32 {
    if adapter.is_null() {
        return bad("Invalid adapter handle");
    }
    if out.is_null() {
        return bad("Invalid metadata handle");
    }
    if path.is_null() {
        return bad("Invalid XCrdPath");
    }
    unsafe { *out = core::ptr::null_mut() };
    let h = unsafe { heap_alloc(4) } as *mut u32;
    if h.is_null() {
        return last_error_hr();
    }
    unsafe { *h = 0 };
    let cb = unsafe { wlen(path) } * 2 + 2;
    let mut m = XvdMsg::new(0x7800_00d0, cb + 0x30);
    m.set_u32(0, 0x30);
    m.set_u32(4, cb as u32);
    m.set_u32(8, 0);
    unsafe { m.set_raw(0x30, path as *const u8, cb) };
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe {
            *h = m.u32(8);
            *out = h;
        }
        return 0;
    }
    unsafe { heap_free(h as *mut c_void) };
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdCloseMetadataHandle(handle: *mut *mut u32, adapter: *mut c_void) -> i32 {
    let h = unsafe { *handle };
    if adapter.is_null() {
        return bad("Invalid adapter handle");
    }
    let mut m = XvdMsg::new(0x7800_00d4, 0x28);
    m.set_u32(0, unsafe { *h });
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    unsafe {
        heap_free(h as *mut c_void);
        *handle = core::ptr::null_mut();
    }
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdRemapReload(adapter: *mut c_void) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    let mut m = XvdMsg::new(0x7800_00f0, 0x20);
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdRemapTest(adapter: *mut c_void, path: *const u16, out: *mut u16, out_cb: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if path.is_null() {
        return bad("Invalid Input Path.");
    }
    if out.is_null() || out_cb == 0 {
        return bad("Invalid output buffer.");
    }
    let n = unsafe { wlen(path) };
    let len = n * 2 + 0x42a;
    let mut m = XvdMsg::new(0x7800_00f4, len);
    m.set_u32(0, len as u32);
    m.set_u32(4, 0x428);
    m.set_u32(8, (n * 2) as u32);
    let st = unsafe { m.copy_wstr(0x428, (n + 1) * 2, path) };
    if st < 0 {
        return st;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    let src = unsafe { m.pp().add(0x10) } as *const u16;
    unsafe { cch_copy(out, out_cb as usize, src) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdQueryCorruptXvc(adapter: *mut c_void, out: *mut *mut u16) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if out.is_null() {
        return bad("XCrdPath must be specified");
    }
    unsafe { *out = core::ptr::null_mut() };
    let mut m = XvdMsg::new(0x7800_0100, 0x200);
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    let src = m.pp() as *const u16;
    let cb = unsafe { wlen(src) } * 2 + 2;
    let p = unsafe { heap_alloc(cb) } as *mut u16;
    unsafe { *out = p };
    if p.is_null() {
        return last_error_hr();
    }
    let r = unsafe { cch_copy(p, cb, src) };
    if r < 0 {
        unsafe {
            heap_free(p as *mut c_void);
            *out = core::ptr::null_mut();
        }
    }
    r
}

#[no_mangle]
pub unsafe extern "system" fn XCrdIsCorrupt(adapter: *mut c_void, path: *const u16, out: *mut u8) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if path.is_null() {
        return bad("XCrdPath must be specified");
    }
    if out.is_null() {
        return bad("IsCorrupt must be specified");
    }
    unsafe { *out = 0 };
    let n = unsafe { wlen(path) };
    let mut m = XvdMsg::new(0x7800_0108, n * 2 + 0xa);
    m.set_u32(0, 8);
    m.set_u32(4, (n * 2) as u32);
    let st = unsafe { m.copy_wstr(8, n * 2 + 2, path) };
    if st < 0 {
        return st;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe { *out = 1 };
        return hr;
    }
    if hr == E_NOT_FOUND {
        0
    } else {
        hr
    }
}

#[repr(C)]
pub struct RepairInfo {
    pub kind: u32,
    pub xvc: *const u16,
    pub src: *const u16,
    pub value: u32,
    pub flags: u32,
}

#[no_mangle]
pub unsafe extern "system" fn XCrdRepairXvc(adapter: *mut c_void, info: *const RepairInfo) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if info.is_null() {
        return bad("Invalid Repair Information");
    }
    let i = unsafe { &*info };
    let kind = i.kind;
    if kind <= 2 || kind == 5 {
        if i.xvc.is_null() {
            return bad("XvcPath must be specified");
        }
    }
    if (kind == 0 || kind == 2) && i.src.is_null() && i.flags & 2 == 0 {
        return bad("SrcPath must be specified");
    }
    if i.flags & !3 != 0 {
        return bad("Invalid flags");
    }
    let mut len = 0x1cusize;
    if !i.xvc.is_null() {
        len = unsafe { wlen(i.xvc) } * 2 + 0x1e;
    }
    if !i.src.is_null() {
        len += unsafe { wlen(i.src) } * 2 + 2;
    }
    let mut m = XvdMsg::new(0x7800_0104, len);
    m.set_u32(0, kind);
    m.set_u32(8, i.value);
    if i.flags & 1 != 0 {
        m.or_u32(4, 1);
    }
    if i.flags & 2 != 0 {
        m.or_u32(4, 2);
    }
    let mut next = 0x1cu32;
    if !i.xvc.is_null() {
        let n = unsafe { wlen(i.xvc) };
        m.set_u32(0xc, 0x1c);
        m.set_u32(0x10, (n * 2) as u32);
        let st = unsafe { m.copy_wstr(0x1c, n * 2 + 2, i.xvc) };
        next = m.u32(0x10) + 0x1e;
        if st < 0 {
            return st;
        }
    } else {
        m.set_u64(0xc, 0);
    }
    if !i.src.is_null() {
        let n = unsafe { wlen(i.src) };
        m.set_u32(0x14, next);
        m.set_u32(0x18, (n * 2) as u32);
        let st = unsafe { m.copy_wstr(next as usize, n * 2 + 2, i.src) };
        if st < 0 {
            return st;
        }
    } else {
        m.set_u64(0x14, 0);
    }
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdQueryWriteStats(adapter: *mut c_void, kind: i32, out: *mut u8) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if out.is_null() {
        return bad("Invalid Write Stats Information");
    }
    if kind >= 1 {
        return bad("Invalid Write Stats Type");
    }
    let mut m = XvdMsg::new(0x7800_0168, 0x28);
    if kind == 0 {
        m.set_u32(0, 0);
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 && kind == 0 {
        unsafe { m.get_raw(8, out, 0x20) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdQueryXRscInformation(adapter: *mut c_void, path: *const u16, class: i32, out: *mut u8, size: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if path.is_null() {
        return bad("Path must be specified");
    }
    if out.is_null() {
        return bad("Info must be supplied");
    }
    if class >= 2 {
        return bad("Invalid InfoClass supplied");
    }
    if (class == 0 && size < 0xb8) || (class == 1 && size < 0x90) {
        return bad("Info Buffer too small");
    }
    let n = unsafe { wlen(path) };
    let mut m = XvdMsg::new(0x7800_0050, n * 2 + 0x41a);
    match class {
        0 => m.set_u32(0xc, 0x18),
        1 => m.set_u32(0xc, 0x19),
        _ => {}
    }
    m.set_u32(0, 0x418);
    m.set_u32(4, (n * 2) as u32);
    let st = unsafe { m.copy_wstr(0x418, n * 2 + 2, path) };
    if st < 0 {
        return st;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    unsafe {
        match class {
            0 => m.get_raw(0x10, out, 0x58),
            1 => {
                m.get_raw(0x10, out, 0x2c);
                put32(out, 0x2c, m.u32(0x40));
                put32(out, 0x30, m.u32(0x44));
            }
            _ => {}
        }
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdGrowDynamicXvd(adapter: *mut c_void, path: *const u16, size: u64) -> i32 {
    unsafe { XCrdGrowThinProvisionedXvd(adapter, path, size) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdCreateXvdPointer(
    adapter: *mut c_void,
    path: *const u16,
    pointer: *const u16,
    blob_size: u64,
    flags: u32,
    header: *const u8,
    xvc_info: *const u8,
    embedded: *const u8,
) -> i32 {
    if flags & !3 != 0 {
        return bad("Invalid Create Pointer Flags.");
    }
    let streamed = flags & 1 != 0;
    let skip = flags & 2 != 0;
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if path.is_null() {
        return bad("Invalid XcrdPath.");
    }
    if streamed && header.is_null() != xvc_info.is_null() {
        return bad("Header & XvcInfo must be either NULL or valid.");
    }
    if !xvc_info.is_null() && !streamed {
        return bad("XvcInfo only supported with MarkAsStreamed.");
    }
    if !header.is_null() && blob_size == 0 {
        return bad("BlobSize must not be 0, if Header is supplied.");
    }
    if !embedded.is_null() && header.is_null() {
        return bad("Xvd Header must be supplied, if Embedded Header is supplied.");
    }
    if pointer.is_null() {
        if !skip {
            return bad("Pointer path null is only supported for SkipPointer flag.");
        }
        if !streamed {
            return bad("SkipPointer is only supported for MarkAsStreamable flag.");
        }
    } else if skip {
        return bad("Pointer path should be null for SkipPointer flag.");
    }
    let n = unsafe { wlen(path) };
    let np = if pointer.is_null() { 0 } else { unsafe { wlen(pointer) } };
    if n == 0 {
        return E_INVALIDARG;
    }
    let mut len = ((n + np) * 2 + 0x3c) as u32;
    if !header.is_null() {
        len += 0xe00;
    }
    if !xvc_info.is_null() {
        len += 0xda8;
    }
    if !embedded.is_null() {
        len += 0xe00;
    }
    let mut m = XvdMsg::new(0x7800_00e0, len as usize);
    m.set_u32(0, len);
    if np != 0 {
        m.set_u32(0xc, 0x38);
        m.set_u32(0x10, (np * 2) as u32);
    }
    m.set_u32(4, m.u32(0x10) + 0x3a);
    m.set_u32(8, (n * 2) as u32);
    m.set_u64(0x28, blob_size);
    if !header.is_null() {
        m.set_u32(0x14, m.u32(4) + 2 + m.u32(8));
        m.set_u32(0x18, 0xe00);
    }
    if !xvc_info.is_null() {
        m.set_u32(0x1c, m.u32(0x18) + m.u32(0x14));
        m.set_u32(0x20, 0xda8);
    }
    if !embedded.is_null() {
        m.set_u32(0x30, m.u32(0x20) + m.u32(0x1c));
        m.set_u32(0x34, 0xe00);
    }
    if streamed {
        m.or_u32(0x24, 1);
    }
    if skip {
        m.or_u32(0x24, 2);
    }
    if np != 0 {
        let st = unsafe { m.copy_wstr(0x38, m.u32(0x10) as usize + 2, pointer) };
        if st < 0 {
            return st;
        }
    }
    let st = unsafe { m.copy_wstr(m.u32(4) as usize, m.u32(8) as usize + 2, path) };
    if st < 0 {
        return st;
    }
    unsafe {
        if !header.is_null() {
            m.set_raw(m.u32(0x14) as usize, header, 0xe00);
        }
        if !xvc_info.is_null() {
            m.set_raw(m.u32(0x1c) as usize, xvc_info, 0xda8);
        }
        if !embedded.is_null() {
            m.set_raw(m.u32(0x30) as usize, embedded, 0xe00);
        }
        m.send_adapter(adapter)
    }
}

const XVD_MAGIC: u64 = 0x6476_782d_7466_736d;

pub fn hash_level_blocks(n: u32, level: u32) -> u32 {
    match level {
        0 => n.wrapping_add(0xa9) / 0xaa,
        1 => n.wrapping_add(0x70e3) / 0x70e4,
        2 => n.wrapping_add(0x4a_f767) / 0x4a_f768,
        3 => n.wrapping_add(0x31c8_4b0f) / 0x31c8_4b10,
        _ => 0,
    }
}

fn pages(v: u64) -> u64 {
    v.wrapping_add(0xfff) >> 12
}

unsafe fn hdr32(h: *const u8, off: usize) -> u32 {
    unsafe { (h.add(off) as *const u32).read_unaligned() }
}

unsafe fn hdr64(h: *const u8, off: usize) -> u64 {
    unsafe { (h.add(off) as *const u64).read_unaligned() }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdGetXvcOffsetFromHeader(header: *const u8, out: *mut u64) -> i32 {
    if header.is_null() {
        return bad("Invalid Header.");
    }
    if out.is_null() {
        return bad("Invalid Offset.");
    }
    if unsafe { hdr64(header, 0) } != XVD_MAGIC {
        return bad("Unknown header.");
    }
    let ct = unsafe { hdr32(header, 0x84) };
    if ct > 0x1a || (0x701_8042u32 >> ct) & 1 == 0 {
        return bad("Not a Xvc.");
    }
    unsafe {
        let a = pages(hdr32(header, 0x8c) as u64);
        let b = pages(hdr64(header, 0x18)) as u32;
        let c = pages(hdr32(header, 0x90) as u64) as u32;
        let s = pages(hdr32(header, 0x88) as u64);
        let extra = *header.add(0x270) as u64;
        let data = c.wrapping_add(b.wrapping_add(a as u32));
        let user = if hdr32(header, 0x80) == 1 { pages(hdr32(header, 0x94) as u64) as u32 } else { 0 };
        let mut tree = 0u32;
        if hdr32(header, 8) & 4 == 0 {
            let total = data.wrapping_add(user);
            let first = total.wrapping_add(0xa9);
            let mut sum = first / 0xaa;
            if first >= 0x154 {
                let mut level = 1;
                loop {
                    let r = hash_level_blocks(total, level);
                    sum = sum.wrapping_add(r);
                    level += 1;
                    if r <= 1 {
                        break;
                    }
                }
            }
            tree = sum;
        }
        let off = (tree as u64).wrapping_add(a.wrapping_add(s).wrapping_add(extra + 3));
        *out = off << 12;
    }
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdGetStreamingRoamableXvdConsoleMode(header: *const u8, out: *mut u16) -> i32 {
    if header.is_null() {
        return bad("Invalid Header.");
    }
    if out.is_null() {
        return bad("Invalid Pointer.");
    }
    if unsafe { hdr64(header, 0) } != XVD_MAGIC {
        trace(1, || "Unknown header.".to_string());
        return 0x8007_0570u32 as i32;
    }
    if unsafe { hdr32(header, 8) } & 0x200 == 0 {
        trace(1, || "Only supported for Streaming Roamable Xvd.".to_string());
        return E_NOTSUPPORTED;
    }
    let v = match unsafe { *header.add(0x276) } {
        0 => 0x40,
        1 => 0x60,
        2 => 0x64,
        3 => 0x70,
        _ => {
            trace(1, || "Unsupported console mode.".to_string());
            return 0x8007_0570u32 as i32;
        }
    };
    unsafe { *out = v };
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdIsSideloadedKeyPresent(adapter: *mut c_void, key: *const u8, out: *mut u8) -> i32 {
    if adapter.is_null() {
        return bad("Invalid adapter handle.");
    }
    if out.is_null() {
        return bad("Invalid IsPresent.");
    }
    let mut m = XvdMsg::new(0x7800_0180, 0x14);
    unsafe { m.set_raw(0, key, 16) };
    m.set_u8(0x10, 0);
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe { *out = m.u8(0x10) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdPopulateSparseCacheBlocks(
    adapter: *mut c_void,
    path: *const u16,
    md_bitmap: *const u8,
    md_len: u32,
    data_bitmap: *const u8,
    data_len: u32,
    id: *const u8,
    downloaded: *mut u32,
) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle.");
    }
    if path.is_null() {
        return bad("Invalid XcrdPath.");
    }
    if md_bitmap.is_null() {
        return bad("Invalid MdBitmap.");
    }
    if data_bitmap.is_null() {
        return bad("Invalid DataBitmap.");
    }
    if downloaded.is_null() {
        return bad("Invalid DownloadedBlocks.");
    }
    let n = unsafe { wlen(path) };
    if n as u32 == 0 {
        return E_INVALIDARG;
    }
    let size = md_len as u64 + 0x32 + (n as u32 as u64) * 2 + data_len as u64;
    if size > 0xffff_ffff {
        return HR_INSUFFICIENT_BUFFER;
    }
    let mut m = XvdMsg::new(0x7800_0184, size as usize);
    let pl = (n * 2) as u32;
    m.set_u32(0x2c, size as u32);
    m.set_u32(0x24, 0x30);
    m.set_u32(0x28, pl);
    m.set_u32(0x18, pl + 0x32);
    m.set_u32(0x14, md_len);
    m.set_u32(0x20, md_len + pl + 0x32);
    m.set_u32(0x1c, data_len);
    unsafe { m.set_raw(0, id, 16) };
    let st = unsafe { m.copy_wstr(0x30, pl as usize + 2, path) };
    if st < 0 {
        return st;
    }
    unsafe {
        m.set_raw(m.u32(0x18) as usize, md_bitmap, md_len as usize);
        m.set_raw(m.u32(0x20) as usize, data_bitmap, data_len as usize);
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe { *downloaded = m.u32(0x10) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdGetUabBitmapOffset(
    uab: *mut u8,
    size: u64,
    second: u8,
    md_bitmap: *mut *mut u8,
    md_len: *mut u32,
    data_bitmap: *mut *mut u8,
    data_len: *mut u32,
) -> i32 {
    if uab.is_null() {
        return bad("Invalid UabData.");
    }
    if md_bitmap.is_null() {
        return bad("Invalid MdBitmap.");
    }
    if md_len.is_null() {
        return bad("Invalid MdBitmapLength.");
    }
    if data_bitmap.is_null() {
        return bad("Invalid DataBitmap.");
    }
    if data_len.is_null() {
        return bad("Invalid DataBitmapLength.");
    }
    if size < 0x1000 {
        return 0x8007_0570u32 as i32;
    }
    unsafe {
        let md = hdr64(uab, 0x2c0);
        let total = hdr64(uab, 0x2a0);
        let data_blocks = total.wrapping_sub(md).wrapping_add(0xa_9fff) / 0xa_a000;
        let md_blocks = md.wrapping_add(0xa_9fff) / 0xa_a000;
        *md_len = (md_blocks.wrapping_add(7) >> 3) as u32;
        *data_len = (data_blocks.wrapping_add(7) >> 3) as u32;
        let base = hdr64(uab, if second != 0 { 0x210 } else { 0x208 });
        *md_bitmap = uab.wrapping_add(base as usize);
        *data_bitmap = uab.wrapping_add((*md_len as u64).wrapping_add(base) as usize);
    }
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdSetFlatContentPath(adapter: *mut c_void, crd_path: *const u16, content: *const u16) -> i32 {
    if adapter.is_null() {
        return bad("Invalid adapter handle.");
    }
    if crd_path.is_null() {
        return bad("Invalid CRD path.");
    }
    if content.is_null() {
        return bad("Invalid content path.");
    }
    let n1 = unsafe { wlen(crd_path) } as u32;
    let n2 = unsafe { wlen(content) } as u32;
    let mut m = XvdMsg::new(0x7800_0194, (n1.wrapping_add(n2) * 2 + 0x14) as usize);
    m.set_u32(0, 0x10);
    m.set_u32(4, n1 * 2);
    m.set_u32(8, n1 * 2 + 0x12);
    m.set_u32(0xc, n2 * 2);
    let st = unsafe { m.copy_wstr(0x10, n1 as usize * 2 + 2, crd_path) };
    if st < 0 {
        return st;
    }
    let st = unsafe { m.copy_wstr(m.u32(8) as usize, n2 as usize * 2 + 2, content) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdSetExtendedAttributes(adapter: *mut c_void, path: *const u16, flags: u32, value: u64) -> i32 {
    if adapter.is_null() {
        return bad("Invalid adapter handle.");
    }
    if path.is_null() {
        return bad("Invalid CRD path.");
    }
    if flags & 0x7fff_fff6 != 0 {
        return bad("Invalid EA flags.");
    }
    let n = unsafe { wlen(path) } as u32;
    let mut m = XvdMsg::new(0x7800_01a4, (n * 2 + 0x1a) as usize);
    m.set_u64(0x10, value);
    m.set_u32(8, flags & 0x8000_0009);
    m.set_u32(0, 0x18);
    m.set_u32(4, n * 2);
    let st = unsafe { m.copy_wstr(0x18, n as usize * 2 + 2, path) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdCopyExtendedAttributes(adapter: *mut c_void, src: *const u16, dst: *const u16, flags: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid adapter handle.");
    }
    if src.is_null() {
        return bad("Invalid source CRD path.");
    }
    if dst.is_null() {
        return bad("Invalid destination CRD path.");
    }
    if flags & !0xf != 0 {
        return bad("Invalid EA flags.");
    }
    let n1 = unsafe { wlen(src) } as u32;
    let n2 = unsafe { wlen(dst) } as u32;
    let mut m = XvdMsg::new(0x7800_01a8, (n1.wrapping_add(n2) * 2 + 0x18) as usize);
    m.set_u32(0x10, flags & 7);
    m.set_u32(0, 0x14);
    m.set_u32(4, n1 * 2);
    m.set_u32(8, n1 * 2 + 0x16);
    m.set_u32(0xc, n2 * 2);
    let st = unsafe { m.copy_wstr(0x14, n1 as usize * 2 + 2, src) };
    if st < 0 {
        return st;
    }
    let st = unsafe { m.copy_wstr(m.u32(8) as usize, n2 as usize * 2 + 2, dst) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdSetTitleQuirks(adapter: *mut c_void, flags: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if flags & !1 != 0 {
        return bad("Invalid Flags");
    }
    let mut m = XvdMsg::new(0x7800_01b4, 4);
    m.set_u32(0, flags & 1);
    unsafe { m.send_adapter(adapter) }
}

#[repr(C)]
pub struct StorageFind {
    pub adapter: *mut c_void,
    pub cookie: u32,
    pub device: u32,
}

unsafe fn storage_step(f: *mut StorageFind, op: u32) -> i32 {
    let mut m = XvdMsg::new(0x7800_008c, 0xc);
    unsafe {
        m.set_u32(0, op);
        m.set_u32(8, (*f).cookie);
        m.set_u32(4, (*f).device);
        let hr = m.send_adapter((*f).adapter);
        if hr >= 0 {
            (*f).device = m.u32(4);
            (*f).cookie = m.u32(8);
        }
        hr
    }
}

unsafe fn storage_free(f: *mut StorageFind) {
    unsafe {
        if !(*f).adapter.is_null() {
            XCrdCloseAdapter((*f).adapter);
            (*f).adapter = core::ptr::null_mut();
        }
        heap_free(f as *mut c_void);
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdFindFirstStorageDevice(out: *mut *mut StorageFind, device: *mut u32) -> i32 {
    if out.is_null() || device.is_null() {
        return E_POINTER_HR;
    }
    unsafe { *out = core::ptr::null_mut() };
    let f = unsafe { heap_alloc_zeroed(core::mem::size_of::<StorageFind>()) } as *mut StorageFind;
    if f.is_null() {
        return E_OUTOFMEMORY_HR;
    }
    let mut hr = unsafe { XCrdOpenAdapter(&mut (*f).adapter) };
    if hr >= 0 {
        hr = unsafe { storage_step(f, 0) };
        if hr >= 0 {
            unsafe {
                *device = (*f).device;
                *out = f;
            }
            return hr;
        }
    }
    unsafe { storage_free(f) };
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdFindNextStorageDevice(find: *mut StorageFind, device: *mut u32) -> i32 {
    if find.is_null() {
        return E_INVALIDARG;
    }
    if device.is_null() {
        return E_POINTER_HR;
    }
    let hr = unsafe { storage_step(find, 1) };
    if hr >= 0 {
        unsafe { *device = (*find).device };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdFindCloseStorageDevice(find: *mut StorageFind) -> i32 {
    if find.is_null() {
        return E_INVALIDARG;
    }
    unsafe {
        storage_step(find, 2);
        storage_free(find);
    }
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdSetXvdInformation(adapter: *mut c_void, path: *const u16, class: i32, info: *const u8, size: u32) -> i32 {
    if adapter.is_null() {
        return bad("Invalid Adapter Handle");
    }
    if path.is_null() {
        return bad("Path must be specified");
    }
    if info.is_null() {
        if !(class == 3 || class == 4 || class == 9) {
            return bad("XvdInfo must be supplied");
        }
    } else if class >= 0xc {
        return bad("Invalid InfoClass supplied");
    }
    let need = match class {
        0 | 10 | 11 => 8,
        1 | 2 => 0x10,
        5 | 6 | 7 => 4,
        8 => 0x54,
        3 | 4 | 9 => 0,
        _ => return E_INVALIDARG,
    };
    if !info.is_null() || !(class == 3 || class == 4 || class == 9) {
        if size < need {
            return bad("Info buffer too small");
        }
    }
    let extra = if class == 8 { 0x50usize } else { 0 };
    let n = unsafe { wlen(path) };
    let mut m = XvdMsg::new(0x7800_0054, n * 2 + 0x112 + extra);
    m.set_u32(0, (extra + 0x110) as u32);
    m.set_u32(4, (n * 2) as u32);
    let mut st = unsafe { m.copy_wstr(extra + 0x110, n * 2 + 2, path) };
    unsafe {
        match class {
            0 => {
                m.set_u32(8, 0);
                m.set_u64(0x10, hdr64(info, 0));
            }
            1 | 2 => {
                m.set_u32(8, class as u32);
                m.set_raw(0x10, info, 16);
            }
            3 => m.set_u32(8, 3),
            4 => {
                m.set_u32(8, 4);
                if info.is_null() {
                    m.set_u16(0x10, 0);
                } else {
                    st = m.copy_wstr(0x10, 0x100, info as *const u16);
                }
            }
            5 | 6 | 7 => {
                m.set_u32(8, class as u32);
                m.set_u32(0x10, hdr32(info, 0));
            }
            8 => {
                m.set_u32(8, 8);
                m.set_u16(0x10, (info as *const u16).read_unaligned());
                m.set_u8(0x12, *info.add(2));
                m.set_u8(0x13, extra as u8);
                st = m.copy_wstr(0x14, extra + 2, info.add(4) as *const u16);
            }
            9 => m.set_u32(8, 9),
            10 => {
                m.set_u32(8, 0xa);
                m.set_u64(0x10, hdr64(info, 0));
            }
            _ => {
                m.set_u32(8, 0x11);
                m.set_u32(0x10, hdr32(info, 0));
                m.set_u32(0x14, hdr32(info, 4));
            }
        }
    }
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCloudSetFeatureBits(adapter: *mut c_void, bits: *const u64) -> i32 {
    let v = unsafe { *bits };
    let path = wz("[XUC:]\\ANYXVC");
    unsafe { XCrdSetXvdInformation(adapter, path.as_ptr(), 0xa, &v as *const u64 as *const u8, 8) }
}

pub const BLOB_FIND_SIZE: usize = 0x410;

unsafe fn rd32(p: *const u8, off: usize) -> u32 {
    unsafe { (p.add(off) as *const u32).read_unaligned() }
}

unsafe fn rd64(p: *const u8, off: usize) -> u64 {
    unsafe { (p.add(off) as *const u64).read_unaligned() }
}

unsafe fn wr32(p: *mut u8, off: usize, v: u32) {
    unsafe { (p.add(off) as *mut u32).write_unaligned(v) }
}

unsafe fn wr64(p: *mut u8, off: usize, v: u64) {
    unsafe { (p.add(off) as *mut u64).write_unaligned(v) }
}

unsafe fn rdp(p: *const u8, off: usize) -> *mut u8 {
    unsafe { (p.add(off) as *const *mut u8).read_unaligned() }
}

unsafe fn wrp(p: *mut u8, off: usize, v: *mut u8) {
    unsafe { (p.add(off) as *mut *mut u8).write_unaligned(v) }
}

fn win32_hr(e: u32) -> i32 {
    if (e as i32) <= 0 {
        e as i32
    } else {
        ((e & 0xffff) | 0x8007_0000) as i32
    }
}

unsafe fn blob_step(h: *mut u8, op: u32, path: *const u16) -> i32 {
    let mut m = XvdMsg::new(0x7800_0084, 0x3d0);
    unsafe {
        m.set_u32(0x10, rd32(h, 0x3f8));
        m.set_u32(0x18, op);
        m.set_raw(0, h.add(0x28), 16);
        if !path.is_null() {
            let st = m.copy_wstr(0x20, 0x208, path);
            if st < 0 {
                return st;
            }
        }
        let hr = m.send_adapter(rdp(h, 0) as *mut c_void);
        if hr < 0 {
            return hr;
        }
        let st = cch_copy(h.add(0x48) as *mut u16, 0x208, m.pp().add(0x20) as *const u16);
        if st < 0 {
            return st;
        }
        wr32(h, 0x370, m.u32(0x348));
        wr64(h, 0x258, m.u64(0x230));
        wr64(h, 0x250, m.u64(0x228));
        if op == 0 {
            m.get_raw(0, h.add(0x28), 16);
        }
        st
    }
}

unsafe fn blob_entry(h: *const u8) -> (*const u16, u64, u64, u32) {
    unsafe {
        if *h.add(0x3fc) != 0 {
            let size = ((rd32(h, 0x44) as u64) << 32) | rd32(h, 0x48) as u64;
            (h.add(0x54) as *const u16, rd64(h, 0x2c), size, rd32(h, 0x28))
        } else {
            (h.add(0x48) as *const u16, rd64(h, 0x258), rd64(h, 0x250), rd32(h, 0x370))
        }
    }
}

unsafe fn free_blob_info(info: *mut u8) {
    unsafe {
        let hdr = rdp(info, 0x28);
        if !hdr.is_null() {
            heap_free(hdr as *mut c_void);
            wrp(info, 0x28, core::ptr::null_mut());
        }
        heap_free(info as *mut c_void);
    }
}

unsafe fn read_disc_header(path: *const u16, buf: *mut u8) -> i32 {
    let name = unsafe { wslice(path) };
    let f = host(|x| x.create_file(name, GENERIC_READ, 3, OPEN_EXISTING, 0));
    if f == INVALID_HANDLE {
        return win32_hr(host(|x| x.last_error()));
    }
    let mut got = 0u32;
    let r = if !host(|x| x.seek(f, 0x200)) || !host(|x| x.read(f, unsafe { core::slice::from_raw_parts_mut(buf, 0xe00) }, &mut got)) {
        win32_hr(host(|x| x.last_error()))
    } else if got < 0xe00 {
        E_INVALIDARG
    } else {
        0
    };
    host(|x| x.close_handle(f));
    r
}

pub unsafe fn validate_xvd_header(h: *const u8) -> i32 {
    unsafe {
        let ok = core::slice::from_raw_parts(h, 8) == b"msft-xvd"
            && rd64(h, 0x18) & 0xfff == 0
            && rd64(h, 0x18).wrapping_sub(0x10_0000) <= 0xf_ffff_0_0000
            && rd32(h, 0x80) <= 1
            && rd32(h, 8) < 0x8000
            && (rd32(h, 0x90) == 0 || {
                let ct = rd32(h, 0x84);
                ct <= 0x1a && (0x701_8042u32 >> ct) & 1 != 0
            })
            && (rd32(h, 0xc) >> 16) as u16 == 0
            && rd32(h, 0xc) == 3
            && rd32(h, 0x88) & 0xfff == 0;
        if ok { 0 } else { 0x8007_0001u32 as i32 }
    }
}

unsafe fn attach_header(adapter: *mut c_void, flags: u32, info: *mut u8, device: i32) -> i32 {
    if flags & 1 == 0 {
        return 0;
    }
    unsafe {
        if (rd64(info, 0x30) as i64) < 0xe00 {
            return 0;
        }
        let h = heap_alloc_zeroed(0xe08);
        if h.is_null() {
            return E_OUTOFMEMORY_NT;
        }
        wr32(h, 4, 0);
        wr32(h, 0, 0xe08);
        let hdr = h.add(8);
        let hr;
        if device != 9 {
            let crd = info.add(0x10 + rd32(info, 0x20) as usize) as *const u16;
            hr = XCrdQueryXvdInformation(adapter, crd, 3, hdr, 0x80);
            wr32(info, 0x48, hr as u32);
            if hr < 0 {
                heap_free(h as *mut c_void);
                return 0;
            }
            wr32(h, 4, 1);
        } else {
            let full = info.add(0x10 + rd32(info, 0x18) as usize) as *const u16;
            hr = read_disc_header(full, hdr);
            wr32(info, 0x48, hr as u32);
            if hr < 0 {
                heap_free(h as *mut c_void);
                return hr;
            }
            if validate_xvd_header(hdr) >= 0 {
                wr32(h, 4, 1);
                wr32(h, 0x10, rd32(hdr, 0x84));
                wr64(h, 0x18, rd64(hdr, 0x10));
                wr64(h, 0x20, rd64(hdr, 0x18));
                wr32(h, 0x14, rd32(hdr, 8));
                (hdr as *mut u16).write_unaligned((hdr.add(0xc) as *const u16).read_unaligned());
                *h.add(0xa) = *hdr.add(0x271);
                wr64(h, 0x58, rd64(hdr, 0x1bc));
                core::ptr::copy(hdr.add(0x18c), h.add(0x60), 16);
                wr32(h, 0xc, rd32(hdr, 0x80));
                core::ptr::copy(hdr.add(0x19c), h.add(0x70), 16);
                core::ptr::copy(hdr.add(0x1ac), h.add(0x48), 16);
                core::ptr::copy(hdr.add(0x30), h.add(0x38), 16);
                core::ptr::copy(hdr.add(0x20), h.add(0x28), 16);
            }
        }
        wr32(info, 0x40, rd32(info, 0x40) | 1);
        wrp(info, 0x28, h);
        hr
    }
}

unsafe fn build_blob_info(adapter: *mut c_void, slot: *mut u8, device: i32, find: *mut u8, dir: *const u16, flags: u32, name: *const u16, size: u64, time: u64, attrs: u32) -> i32 {
    unsafe {
        let old = rdp(slot, 0);
        if !old.is_null() {
            free_blob_info(old);
            wrp(slot, 0, core::ptr::null_mut());
        }
        let total = ((wlen(dir) + wlen(name)) as u32).wrapping_mul(4).wrapping_add(0xc8);
        let info = heap_alloc_zeroed(total as usize);
        if info.is_null() {
            return E_OUTOFMEMORY_NT;
        }
        let text = info.add(0xa0) as *mut u16;
        let mut st = cch_copy(text, total as usize - 0xa0, name);
        if st >= 0 {
            wrp(info, 8, find);
            wr32(info, 0, total);
            wr64(info, 0x38, time);
            wr64(info, 0x30, size);
            wr32(info, 0x44, device as u32);
            wr32(info, 0x18, 0x90);
            let full = format!("{}\\{}", String::from_utf16_lossy(wslice(dir)), String::from_utf16_lossy(wslice(name)));
            let mut room = total as usize - 0xa0;
            st = cch_printf(core::slice::from_raw_parts_mut(text, room / 2), &full);
            if st >= 0 {
                let l1 = wlen(text) * 2;
                wr32(info, 0x1c, l1 as u32);
                room = room.wrapping_sub(l1 + 2);
                let base = text as *const u8;
                let mut leaf = text as *const u16;
                for i in (0..l1 / 2).rev() {
                    if *text.add(i) == b'\\' as u16 {
                        leaf = text.add(i + 1);
                        break;
                    }
                }
                wr32(info, 0x10, (leaf as usize - info as usize - 0x10) as u32);
                wr32(info, 0x14, (wlen(leaf) * 2) as u32);
                let crd = base.add(l1 + 2) as *mut u16;
                let d = wslice(dir);
                let n = String::from_utf16_lossy(wslice(name));
                let path = if device == 9 {
                    let tail = match d.iter().position(|&c| c == b'\\' as u16) {
                        Some(i) => String::from_utf16_lossy(&d[i..]),
                        None => String::new(),
                    };
                    format!("[XOD:]\\O:{tail}\\{n}")
                } else {
                    let rest = if d.first() == Some(&(b'\\' as u16)) { &d[1..] } else { d };
                    let sep = if rest.is_empty() || rest.contains(&(b'\\' as u16)) { "" } else { "\\" };
                    match PATH_PREFIXES.get(device as usize) {
                        Some(p) => format!("{p}\\{}{sep}{n}", String::from_utf16_lossy(rest)),
                        None => {
                            free_blob_info(info);
                            return E_INVALIDARG;
                        }
                    }
                };
                st = cch_printf(core::slice::from_raw_parts_mut(crd, room / 2), &path);
                if st >= 0 {
                    wr32(info, 0x20, (crd as usize - info as usize - 0x10) as u32);
                    wr32(info, 0x24, (wlen(crd) * 2) as u32);
                    if attrs & 0x10 != 0 {
                        wr32(info, 0x40, rd32(info, 0x40) | 2);
                    }
                    if device == 9 {
                        wr32(info, 0x40, rd32(info, 0x40) | 4);
                    }
                    if attrs & 0x10 == 0 {
                        st = attach_header(adapter, flags, info, device);
                    }
                    if st >= 0 {
                        wrp(slot, 0, info);
                        return st;
                    }
                }
            }
        }
        free_blob_info(info);
        st
    }
}

unsafe fn blob_publish(h: *mut u8) -> i32 {
    unsafe {
        let (name, time, size, attrs) = blob_entry(h);
        build_blob_info(rdp(h, 0) as *mut c_void, h.add(0x20), rd32(h, 0x3f8) as i32, h, rdp(h, 8) as *const u16, rd32(h, 0x1c), name, size, time, attrs)
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdFindFirstBlob(out: *mut *mut u8, info: *mut *mut u8, device: i32, path: *const u16, flags: u32) -> i32 {
    if flags & !1 != 0 {
        return E_INVALIDARG;
    }
    let optical = device == 9;
    if optical && host(|x| x.drive_type(&wz("O:")[..2])) != 5 {
        return E_NOT_FOUND;
    }
    let size = 2 + if path.is_null() { BLOB_FIND_SIZE } else { (unsafe { wlen(path) }) * 2 + BLOB_FIND_SIZE };
    let h = unsafe { heap_alloc_zeroed(size) };
    if h.is_null() {
        return E_OUTOFMEMORY_NT;
    }
    unsafe {
        let mut a: *mut c_void = core::ptr::null_mut();
        let mut hr = XCrdOpenAdapter(&mut a);
        if hr < 0 {
            heap_free(h as *mut c_void);
            return hr;
        }
        wrp(h, 0, a as *mut u8);
        *h.add(0x3fc) = optical as u8;
        let pat = h.add(0x400) as *mut u16;
        wrp(h, 8, pat as *mut u8);
        let cap = core::slice::from_raw_parts_mut(pat, (size - 0x400) / 2);
        let given = !path.is_null() && *path != 0;
        let stripped = if given && *path == b'\\' as u16 { path.add(1) } else { path };
        let text = if optical {
            if given { format!("O:\\{}", String::from_utf16_lossy(wslice(stripped))) } else { "O:\\*.*".to_string() }
        } else if given {
            format!("\\{}\\", String::from_utf16_lossy(wslice(stripped)))
        } else {
            "\\".to_string()
        };
        hr = cch_printf(cap, &text);
        if hr >= 0 {
            wr32(h, 0x1c, flags);
            wr32(h, 0x3f8, device as u32);
            hr = if optical {
                let mut fd = [0u8; FIND_DATA];
                let f = host(|x| x.find_first(wslice(pat), &mut fd));
                core::ptr::copy_nonoverlapping(fd.as_ptr(), h.add(0x28), FIND_DATA);
                (h.add(0x10) as *mut Handle).write_unaligned(f);
                if f == INVALID_HANDLE { win32_hr(host(|x| x.last_error())) } else { 0 }
            } else {
                blob_step(h, 0, if given { path } else { core::ptr::null() })
            };
            if hr >= 0 {
                let n = wlen(pat);
                let cut = (0..n).rev().find(|&i| *pat.add(i) == b'\\' as u16);
                hr = match cut {
                    Some(i) => {
                        *pat.add(i) = 0;
                        blob_publish(h)
                    }
                    None => E_INVALIDARG,
                };
                if hr >= 0 {
                    *out = h;
                    *info = rdp(h, 0x20).add(0x10);
                    return hr;
                }
                if optical {
                    let f = (h.add(0x10) as *const Handle).read_unaligned();
                    if f != 0 {
                        host(|x| x.find_close(f));
                    }
                } else {
                    blob_step(h, 2, core::ptr::null());
                }
                (h.add(0x10) as *mut Handle).write_unaligned(0);
            }
        }
        XCrdCloseAdapter(rdp(h, 0) as *mut c_void);
        heap_free(h as *mut c_void);
        hr
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdFindNextBlob(h: *mut u8, info: *mut *mut u8) -> i32 {
    unsafe {
        wr32(h, 0x18, rd32(h, 0x18).wrapping_add(1));
        let hr = if *h.add(0x3fc) != 0 {
            let mut fd = [0u8; FIND_DATA];
            let f = (h.add(0x10) as *const Handle).read_unaligned();
            let ok = host(|x| x.find_next(f, &mut fd));
            if ok {
                core::ptr::copy_nonoverlapping(fd.as_ptr(), h.add(0x28), FIND_DATA);
                0
            } else {
                win32_hr(host(|x| x.last_error()))
            }
        } else {
            blob_step(h, 1, core::ptr::null())
        };
        if hr < 0 {
            return hr;
        }
        let hr = blob_publish(h);
        if hr >= 0 {
            *info = rdp(h, 0x20).add(0x10);
        }
        hr
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdFindCloseBlob(h: *mut u8) -> i32 {
    unsafe {
        let info = rdp(h, 0x20);
        if !info.is_null() {
            free_blob_info(info);
            wrp(h, 0x20, core::ptr::null_mut());
        }
        if *h.add(0x3fc) != 0 {
            let f = (h.add(0x10) as *const Handle).read_unaligned();
            if f != 0 {
                host(|x| x.find_close(f));
                (h.add(0x10) as *mut Handle).write_unaligned(0);
            }
        } else {
            blob_step(h, 2, core::ptr::null());
        }
        XCrdCloseAdapter(rdp(h, 0) as *mut c_void);
        heap_free(h as *mut c_void);
    }
    0
}
