use crate::xcrd2_storage::fail;
use crate::xcrd_core::*;
use core::ffi::c_void;

pub const HR_CORRUPT: i32 = 0x8007_0570u32 as i32;
pub const HR_DEVICE_NOT_CONNECTED: i32 = 0x8007_01b1u32 as i32;
pub const MAX_XVIO: u32 = 0x10_0000;

#[no_mangle]
pub unsafe extern "system" fn XCrdSetXRscInformation(adapter: *mut c_void, class: i32, info: *const u8, size: u32) -> i32 {
    let path = wz("[XUC:]\\ANYXVD");
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if info.is_null() {
        return fail("Info must be supplied\n");
    }
    if class >= 6 {
        return fail("Invalid InfoClass supplied\n");
    }
    let need = match class {
        0 | 1 | 3 | 4 => 8,
        2 | 5 => 4,
        _ => 0,
    };
    if size < need {
        return fail("Info Buffer too small\n");
    }
    let n = path.len() - 1;
    let mut m = XvdMsg::new(0x7800_0054, n * 2 + 0x112);
    let rd32 = || unsafe { (info as *const u32).read_unaligned() };
    let rd64 = || unsafe { (info as *const u64).read_unaligned() };
    match class {
        0 | 1 | 3 | 4 => {
            m.set_u32(8, [0xb, 0xc, 0, 0xe, 0xf][class as usize]);
            m.set_u64(0x10, rd64());
        }
        2 => {
            m.set_u32(8, 0xd);
            m.set_u32(0x10, rd32());
        }
        5 => {
            m.set_u32(8, 0x10);
            m.set_u32(0x10, rd32());
        }
        _ => {}
    }
    m.set_u32(0, 0x110);
    m.set_u32(4, (n * 2) as u32);
    let st = unsafe { m.copy_wstr(0x110, n * 2 + 2, path.as_ptr()) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

fn pair_len(count: u32, specs: *const [*const u16; 2]) -> u32 {
    let mut s = 0u32;
    for i in 0..count as usize {
        let p = unsafe { *specs.add(i) };
        s = s.wrapping_add((unsafe { wlen(p[1]) as u32 }.wrapping_add(unsafe { wlen(p[0]) } as u32)).wrapping_mul(2)).wrapping_add(4);
    }
    s
}

unsafe fn write_pairs(m: &mut XvdMsg, count: u32, specs: *const [*const u16; 2], table: usize, total_off: usize, mut at: u32) -> i32 {
    for i in 0..count as usize {
        let s = unsafe { *specs.add(i) };
        for k in 0..2 {
            let l = unsafe { wlen(s[k]) } as u32 * 2;
            let st = unsafe { m.copy_wstr(at as usize, l as usize + 2, s[k]) };
            let t = m.u32(total_off) + l + 2;
            m.set_u32(total_off, t);
            m.set_u32(table + i * 16 + k * 8, at);
            m.set_u32(table + i * 16 + k * 8 + 4, l);
            at = at + 2 + l;
            if st < 0 {
                return st;
            }
        }
    }
    0
}

fn check_specs(count: u32, specs: *const [*const u16; 2]) -> Result<(), i32> {
    if count == 0 {
        if !specs.is_null() {
            return Err(fail("Specifiers must be NULL if zero specifiers are passed\n"));
        }
    } else if specs.is_null() {
        return Err(fail("Specifiers must not be NULL\n"));
    }
    Ok(())
}

#[no_mangle]
pub unsafe extern "system" fn XCrdTrimXvc(adapter: *mut c_void, path: *const u16, count: u32, specs: *const [*const u16; 2], flags: u32, trimmed: *mut u64, done: *mut u8) -> i32 {
    if !done.is_null() {
        unsafe { *done = 0 };
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if path.is_null() {
        return fail("XCrdPath must be specified\n");
    }
    if flags & !1 != 0 {
        return fail("Invalid Flags\n");
    }
    if flags & 1 != 0 && trimmed.is_null() {
        return fail("TrimmedSize cannot be null\n");
    }
    if let Err(e) = check_specs(count, specs) {
        return e;
    }
    let n = unsafe { wlen(path) } as u32;
    let size = n.wrapping_add(count.wrapping_mul(8)).wrapping_mul(2).wrapping_add(0x4a).wrapping_add(pair_len(count, specs));
    if size > MAX_XVIO {
        return fail("Payload too large for XVIO\n");
    }
    let mut m = XvdMsg::new(0x7800_00ac, size as usize);
    m.set_u32(0x14, flags & 1);
    m.set_u32(0, 0x48);
    m.set_u32(4, n * 2);
    let st = unsafe { m.copy_wstr(0x48, n as usize * 2 + 2, path) };
    m.set_u32(0x10, count);
    m.set_u32(8, m.u32(4) + 0x4a);
    m.set_u32(0xc, count << 4);
    if st < 0 {
        return st;
    }
    if count != 0 {
        let table = m.u32(8) as usize;
        let at = m.u32(0xc) + m.u32(8);
        let st = unsafe { write_pairs(&mut m, count, specs, table, 0xc, at) };
        if st < 0 {
            return st;
        }
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 && !trimmed.is_null() {
        unsafe { *trimmed = m.u64(0x20) };
    }
    if !done.is_null() {
        unsafe { *done = m.u8(0x1c) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdTrimXvd(adapter: *mut c_void, path: *const u16, trimmed: *mut u64, done: *mut u8) -> i32 {
    if !done.is_null() {
        unsafe { *done = 0 };
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if path.is_null() {
        return fail("XCrdPath must be specified\n");
    }
    let n = unsafe { wlen(path) } as u32;
    let size = n.wrapping_mul(2).wrapping_add(0x4a);
    if size > MAX_XVIO {
        return fail("Payload too large for XVIO\n");
    }
    let mut m = XvdMsg::new(0x7800_00ac, size as usize);
    m.set_u32(0x14, 2);
    m.set_u32(0, 0x48);
    m.set_u32(4, n * 2);
    let st = unsafe { m.copy_wstr(0x48, n as usize * 2 + 2, path) };
    if st < 0 {
        return st;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 && !trimmed.is_null() {
        unsafe { *trimmed = m.u64(0x20) };
    }
    if !done.is_null() {
        unsafe { *done = m.u8(0x1c) };
    }
    hr
}

fn dismount_ok(hr: i32) -> bool {
    hr >= 0 || hr == HR_DEVICE_NOT_CONNECTED
}

#[no_mangle]
pub unsafe extern "system" fn XCrdUnmountEx(adapter: *mut c_void, dp: *mut DevicePath, stage: *mut u32) -> i32 {
    let mut at = 0u32;
    let hr;
    if adapter.is_null() {
        at = 1;
        hr = E_INVALIDARG;
    } else if dp.is_null() {
        at = 2;
        hr = E_INVALIDARG;
    } else {
        let r = unsafe { dismount_device_path(dp) };
        if !dismount_ok(r) {
            at = 3;
            hr = r;
        } else {
            let addr = unsafe { (*dp).address };
            let r = unsafe { remove_lun(adapter, &addr) };
            if r < 0 {
                at = 4;
            } else {
                unsafe { heap_free(dp as *mut c_void) };
            }
            hr = r;
        }
    }
    if !stage.is_null() {
        unsafe { *stage = at };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdUnmount(adapter: *mut c_void, dp: *mut DevicePath) -> i32 {
    unsafe { XCrdUnmountEx(adapter, dp, core::ptr::null_mut()) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdUnmountByPath(adapter: *mut c_void, path: *const u16) -> i32 {
    if adapter.is_null() || path.is_null() {
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
    let hr = dismount_disk_volumes(disk);
    if !dismount_ok(hr) {
        return hr;
    }
    unsafe { remove_lun(adapter, &addr) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdUpdatePath(path: *mut u16, index: i32) -> i32 {
    if index > 0x24 {
        return E_INVALIDARG;
    }
    let mut cur = 0i32;
    let hr = unsafe { crate::xcrd_a::XCrdParsePath(path, &mut cur) };
    if hr < 0 {
        return hr;
    }
    let from = prefix_wide(cur as usize);
    let to = prefix_wide(index as usize);
    if from.len() != to.len() {
        return E_INVALIDARG;
    }
    unsafe { core::ptr::copy_nonoverlapping(to.as_ptr(), path, to.len()) };
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdUpdateXvc(
    adapter: *mut c_void,
    base: *const u16,
    updated: *const u16,
    extra: *const u16,
    count: u32,
    specs: *const [*const u16; 2],
    flags: u32,
    update_size: *mut u64,
    free_space: *mut u64,
    out_flag: *mut u8,
    out_status: *mut u32,
) -> i32 {
    if !out_flag.is_null() {
        unsafe { *out_flag = 0 };
    }
    if !out_status.is_null() {
        unsafe { *out_status = 0 };
    }
    if !free_space.is_null() {
        unsafe { *free_space = 0 };
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if base.is_null() {
        return fail("BaseXvcPath must be specified\n");
    }
    if updated.is_null() {
        return fail("UpdatedXvcPath must be specified\n");
    }
    if flags & 0xffff_ffc4 != 0 {
        return fail("Invalid Flags\n");
    }
    if flags & 0xa == 0xa {
        return fail("Cannot combine XCRD_UPDATE_XVC_FLAG_SPECIFIER_UPDATE with HYBRID flags\n");
    }
    if flags & 3 != 0 && update_size.is_null() && free_space.is_null() {
        return fail("UpdateSize and FreeSpaceRequired cannot be null\n");
    }
    if let Err(e) = check_specs(count, specs) {
        return e;
    }
    let wl = |p: *const u16| unsafe { wlen(p) } as u32;
    let mut size = wl(base).wrapping_add(wl(updated)).wrapping_mul(2).wrapping_add(0x6c);
    if !extra.is_null() {
        size = size.wrapping_add(wl(extra) * 2 + 2);
    }
    size = size.wrapping_add(count << 4).wrapping_add(pair_len(count, specs));
    if size > MAX_XVIO {
        return fail("Payload too large for XVIO\n");
    }
    let mut m = XvdMsg::new(0x7800_0030, size as usize);
    m.set_u64(0x10, 0);
    m.set_u32(0, 0x68);
    let lu = wl(updated) * 2;
    m.set_u32(4, lu);
    let mut next = lu + 0x6a;
    m.set_u32(8, next);
    let lb = wl(base) * 2;
    m.set_u32(0xc, lb);
    next = next + 2 + lb;
    if !extra.is_null() {
        let l = wl(extra) * 2;
        m.set_u32(0x48, l);
        if l != 0 {
            m.set_u32(0x44, next);
            next += l + 2;
        }
    }
    if count != 0 {
        m.set_u32(0x34, count);
        m.set_u32(0x2c, next);
        m.set_u32(0x30, count << 4);
    }
    let mut st = unsafe { m.copy_wstr(0x68, m.u32(4) as usize + 2, updated) };
    if st < 0 {
        return st;
    }
    st = unsafe { m.copy_wstr(m.u32(8) as usize, m.u32(0xc) as usize + 2, base) };
    if st < 0 {
        return st;
    }
    if m.u32(0x48) != 0 {
        st = unsafe { m.copy_wstr(m.u32(0x44) as usize, m.u32(0x48) as usize + 2, extra) };
        if st < 0 {
            return st;
        }
    }
    if count != 0 {
        let table = m.u32(0x2c) as usize;
        let at = m.u32(0x30) + m.u32(0x2c);
        let k = m.u32(0x34);
        st = unsafe { write_pairs(&mut m, k, specs, table, 0x30, at) };
        if st < 0 {
            return st;
        }
    }
    let mut f: u64 = if flags & 1 != 0 {
        2
    } else if flags & 2 != 0 {
        4
    } else {
        1
    };
    if flags & 8 != 0 {
        f |= 0x20;
    }
    if flags & 0x10 != 0 {
        f |= 0x2000;
    }
    m.set_u64(0x10, f);
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 && !update_size.is_null() {
        unsafe { *update_size = m.u64(0x18) };
    }
    if !out_flag.is_null() {
        unsafe { *out_flag = m.u8(0x28) };
    }
    if !out_status.is_null() {
        unsafe { *out_status = m.u32(0x40) };
    }
    if !free_space.is_null() {
        unsafe { *free_space = m.u64(0x38) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdUpdateXvcEx(
    adapter: *mut c_void,
    base: *const u16,
    updated: *const u16,
    extra: *const u16,
    count: u32,
    specs: *const [*const u16; 2],
    action: i32,
    flags: u32,
    update_size: *mut u64,
    free_space: *mut u64,
    out_status: *mut u32,
) -> i32 {
    if !out_status.is_null() {
        unsafe { *out_status = 0 };
    }
    if !free_space.is_null() {
        unsafe { *free_space = 0 };
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if base.is_null() {
        return fail("BaseXvcPath must be specified\n");
    }
    if action != 2 {
        if updated.is_null() {
            return fail("UpdatedXvcPath must be specified\n");
        }
        if action >= 3 {
            return fail("Invalid update action\n");
        }
        if action == 0 {
            if update_size.is_null() {
                return fail("UpdateSize cannot be null\n");
            }
            if free_space.is_null() {
                return fail("FreeSpaceRequired cannot be null\n");
            }
        }
    }
    if flags & !0x10 != 0 {
        return fail("Invalid flags\n");
    }
    if count == 0 {
        if !specs.is_null() {
            return fail("Specifiers must be NULL\n");
        }
    } else if specs.is_null() {
        return fail("Specifiers must not be NULL\n");
    }
    let wl = |p: *const u16| unsafe { wlen(p) } as u32;
    let mut size = wl(base).wrapping_mul(2).wrapping_add(0x4a);
    if !updated.is_null() {
        size = size.wrapping_add(wl(updated) * 2 + 2);
    }
    if !extra.is_null() {
        size = size.wrapping_add(wl(extra) * 2 + 2);
    }
    size = size.wrapping_add(count << 4).wrapping_add(pair_len(count, specs));
    if size > MAX_XVIO {
        return fail("Payload too large for XVIO\n");
    }
    let mut m = XvdMsg::new(0x7800_0110, size as usize);
    m.set_u32(0x24, if action == 0 { 0 } else if action == 1 { 1 } else { 2 });
    if flags & 0x10 != 0 {
        m.set_u32(0x28, 1);
    }
    m.set_u32(0, 0x48);
    let lb = wl(base) * 2;
    m.set_u32(4, lb);
    let mut next = lb + 0x4a;
    if !updated.is_null() {
        m.set_u32(8, next);
        let l = wl(updated) * 2;
        m.set_u32(0xc, l);
        next = next + 2 + l;
    }
    if !extra.is_null() {
        let l = wl(extra) * 2;
        m.set_u32(0x14, l);
        if l != 0 {
            m.set_u32(0x10, next);
            next = next + 2 + l;
        }
    }
    if count != 0 {
        m.set_u32(0x20, count);
        m.set_u32(0x18, next);
        m.set_u32(0x1c, count << 4);
    }
    let mut st = unsafe { m.copy_wstr(m.u32(0) as usize, m.u32(4) as usize + 2, base) };
    if st < 0 {
        return st;
    }
    if !updated.is_null() {
        st = unsafe { m.copy_wstr(m.u32(8) as usize, m.u32(0xc) as usize + 2, updated) };
        if st < 0 {
            return st;
        }
    }
    if !extra.is_null() {
        st = unsafe { m.copy_wstr(m.u32(0x10) as usize, m.u32(0x14) as usize + 2, extra) };
        if st < 0 {
            return st;
        }
    }
    if count != 0 {
        let table = m.u32(0x18) as usize;
        let at = m.u32(0x1c) + m.u32(0x18);
        let k = m.u32(0x20);
        st = unsafe { write_pairs(&mut m, k, specs, table, 0x1c, at) };
        if st < 0 {
            return st;
        }
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 && !update_size.is_null() {
        unsafe { *update_size = m.u64(0x30) };
    }
    if !free_space.is_null() {
        unsafe { *free_space = m.u64(0x38) };
    }
    if !out_status.is_null() {
        unsafe { *out_status = m.u32(0x40) };
    }
    hr
}

fn div_aa000(x: u64) -> u64 {
    (((x as u128 * 0xc0c0_c0c0_c0c0_c0c1u128) >> 64) as u64) >> 0x13
}

#[no_mangle]
pub unsafe extern "system" fn XCrdValidateUab(uab: *const u8, size: u64, exact: u8, basic: *const u8) -> i32 {
    if uab.is_null() {
        return fail("Invalid UabData.\n");
    }
    if basic.is_null() {
        return fail("Invalid XvdBasicInfo.\n");
    }
    if size < 0x1000 {
        return E_INVALIDARG;
    }
    let u32_at = |p: *const u8, o: usize| unsafe { (p.add(o) as *const u32).read_unaligned() };
    let u64_at = |p: *const u8, o: usize| unsafe { (p.add(o) as *const u64).read_unaligned() };
    let same = |a: usize, b: usize| unsafe { core::slice::from_raw_parts(uab.add(a), 16) == core::slice::from_raw_parts(basic.add(b), 16) };
    if u64_at(uab, 0x200) != 0x2042_4155_582d_534d {
        return HR_CORRUPT;
    }
    if u32_at(uab, 0x284) as u64 > size {
        return HR_CORRUPT;
    }
    if u64_at(uab, 0x2a0) != u64_at(basic, 0x78) {
        return E_INVALIDARG;
    }
    if !same(0x220, 0x30) || !same(0x230, 0x30) || !same(0x240, 0x40) {
        return E_INVALIDARG;
    }
    if u64_at(uab, 0x208) != 0x1000 {
        return HR_CORRUPT;
    }
    let tail = u64_at(uab, 0x2c0);
    let total = u64_at(uab, 0x2a0);
    let a = (div_aa000(total.wrapping_sub(tail).wrapping_add(0xa9fff)).wrapping_add(7)) >> 3;
    let b = (div_aa000(tail.wrapping_add(0xa9fff)).wrapping_add(7)) >> 3;
    let mut page = (a as u32).wrapping_add(0xfff).wrapping_add(b as u32) & 0xffff_f000;
    let both = (b.wrapping_add(0xfff).wrapping_add(a)) & !0xfffu64;
    let full = (both.wrapping_mul(2).wrapping_add(0x1000) as u32) as u64;
    if exact != 0 && size != full {
        return HR_CORRUPT;
    }
    let first = u32_at(uab, 0x218);
    if first != page {
        return HR_CORRUPT;
    }
    if size == full {
        let second = u32_at(uab, 0x21c);
        if second == 0 {
            return HR_CORRUPT;
        }
        let off = u64_at(uab, 0x210);
        if off == 0 || off != u32_at(uab, 0x208).wrapping_add(first) as u64 || second != page {
            return HR_CORRUPT;
        }
        page = page.wrapping_add(page.wrapping_add(0x1000));
    } else {
        page = page.wrapping_add(0x1000);
    }
    if u32_at(uab, 0x284) != page {
        HR_CORRUPT
    } else {
        0
    }
}

fn wstr(p: *const u16) -> String {
    String::from_utf16_lossy(unsafe { wslice(p) })
}

unsafe fn xas_build(
    out: *mut *mut u16,
    adapter: *mut c_void,
    host: *const u16,
    domain: *const u16,
    account: *const u16,
    container: *const u16,
    blob: *const u16,
    sig: *const u16,
    mode: u32,
    flags: u64,
    vcsa: *const u16,
    vcsp: u16,
) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if out.is_null() || flags & !7 != 0 {
        return E_INVALIDARG;
    }
    let (mut host, mut account, mut container, mut sig) = (host, account, container, sig);
    let invalid_acct = wz("invalidACCT");
    let invalid_cont = wz("invalidCONT");
    let invalid_sig = wz("?sig=invalid");
    let any_ip = wz("0.0.0.0");
    if flags & 2 == 0 {
        if host.is_null() || account.is_null() || container.is_null() || sig.is_null() {
            return E_INVALIDARG;
        }
    } else {
        if vcsa.is_null() {
            return E_INVALIDARG;
        }
        account = invalid_acct.as_ptr();
        container = invalid_cont.as_ptr();
        sig = invalid_sig.as_ptr();
        host = any_ip.as_ptr();
    }
    let template = wz("%ws");
    let blob = match mode {
        1 => {
            if blob.is_null() {
                return E_INVALIDARG;
            }
            blob
        }
        0 => template.as_ptr(),
        _ => return E_INVALIDARG,
    };
    let default_domain = wz("blob.core.windows.net");
    let domain = if domain.is_null() { default_domain.as_ptr() } else { domain };
    let wl = |p: *const u16| unsafe { wlen(p) } as u32;
    let sum = wl(domain).wrapping_add(wl(container)).wrapping_add(wl(account)).wrapping_add(wl(host)).wrapping_add(wl(blob)).wrapping_add(wl(sig));
    let mut size = sum.wrapping_mul(2).wrapping_add(0x1e);
    if flags & 1 != 0 {
        size += 0x10;
    }
    if flags & 4 != 0 {
        size += 0xe;
    }
    let mut number = String::new();
    if !vcsa.is_null() {
        size = size.wrapping_add(wl(vcsa) * 2 + 0xc);
        if vcsp != 0 {
            number = format!("{}", vcsp as i32);
            size += 0x18;
        }
    }
    let sig_text: Vec<u16> = unsafe { wslice(sig) }.to_vec();
    let mut singles = 0u32;
    if mode == 0 {
        for i in 0..sig_text.len() {
            if sig_text[i] == 0x25 && sig_text.get(i + 1).copied().unwrap_or(0) != 0x25 {
                singles += 1;
            }
        }
    }
    let bytes = size.wrapping_add((singles + 1) * 2) as usize;
    let buf = unsafe { heap_alloc_zeroed(bytes) } as *mut u16;
    if buf.is_null() {
        return E_OUTOFMEMORY_NT;
    }
    let cch = bytes / 2;
    let dst = unsafe { core::slice::from_raw_parts_mut(buf, cch) };
    let head = format!("[XAS:]{}/{}/{}/", wstr(host), wstr(account), wstr(container));
    let st = cch_printf(dst, &head);
    if st < 0 {
        unsafe { heap_free(buf as *mut c_void) };
        return st;
    }
    let mut text: Vec<u16> = head.encode_utf16().collect();
    text.extend_from_slice(unsafe { wslice(blob) });
    if mode == 0 {
        for i in 0..sig_text.len() {
            text.push(sig_text[i]);
            if sig_text[i] == 0x25 && sig_text.get(i + 1).copied().unwrap_or(0) != 0x25 {
                text.push(0x25);
            }
        }
    } else {
        text.extend_from_slice(&sig_text);
    }
    text.extend(",SN=".encode_utf16());
    text.extend_from_slice(unsafe { wslice(domain) });
    if flags & 1 != 0 {
        text.extend(",PRST=1".encode_utf16());
    }
    if flags & 4 != 0 {
        text.extend(",NRM=1".encode_utf16());
    }
    if !vcsa.is_null() {
        text.extend(",VCSA=".encode_utf16());
        text.extend_from_slice(unsafe { wslice(vcsa) });
        if vcsp != 0 {
            text.extend(",VCSP=".encode_utf16());
            text.extend(number.encode_utf16());
        }
    }
    let n = text.len().min(cch - 1);
    dst[..n].copy_from_slice(&text[..n]);
    dst[n] = 0;
    unsafe { *out = buf };
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXasBuildPath(
    out: *mut *mut u16,
    adapter: *mut c_void,
    host: *const u16,
    domain: *const u16,
    account: *const u16,
    container: *const u16,
    blob: *const u16,
    sig: *const u16,
    mode: u32,
    flags: u64,
    reserved: u64,
) -> i32 {
    if reserved != 0 {
        return E_INVALIDARG;
    }
    unsafe { xas_build(out, adapter, host, domain, account, container, blob, sig, mode, flags, core::ptr::null(), 0) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXasBuildPathEx(
    out: *mut *mut u16,
    adapter: *mut c_void,
    host: *const u16,
    domain: *const u16,
    account: *const u16,
    container: *const u16,
    blob: *const u16,
    sig: *const u16,
    mode: u32,
    flags: u64,
    vcsa: *const u16,
    vcsp: u16,
) -> i32 {
    unsafe { xas_build(out, adapter, host, domain, account, container, blob, sig, mode, flags, vcsa, vcsp) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXasKeyQueryValue(path: *const u16, key: u32) -> u64 {
    if key == 0 || key & (key - 1) != 0 {
        return 0;
    }
    let needle: &str = match key {
        1 => "PRST=1",
        4 => "NRM=1",
        _ => return 0,
    };
    if unsafe { *path } == 0 {
        return 0;
    }
    let hay = unsafe { wslice(path) };
    let nd: Vec<u16> = needle.encode_utf16().collect();
    hay.windows(nd.len()).any(|w| w == nd.as_slice()) as u64
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXBlobCopy(adapter: *mut c_void, src: *const u16, dst: *const u16, offset: u64, length: u64, flags: u32, copied: *mut u64) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if src.is_null() {
        return fail("Invalid Src Input Path.\n");
    }
    if dst.is_null() {
        return fail("Invalid Dst Input Path.\n");
    }
    if offset != 0 || length != 0 {
        return fail("Offset, Length must be zero.\n");
    }
    let ls = unsafe { wlen(src) };
    let ld = unsafe { wlen(dst) };
    let size = ((ls + ld) * 2 + 0x54) as u32;
    let mut m = XvdMsg::new(0x7800_00ec, size as usize);
    m.set_u32(0, size);
    m.set_u32(0x14, if flags & 2 != 0 { (flags & 1) | 2 } else { flags & 1 });
    m.set_u64(0x20, 0);
    m.set_u64(0x18, 0);
    m.set_u32(4, 0x50);
    m.set_u32(8, (ls * 2) as u32);
    let st = unsafe { m.copy_wstr(0x50, (ls + 1) * 2, src) };
    if st < 0 {
        return st;
    }
    let off = m.u32(4) + m.u32(8) + 2;
    m.set_u32(0xc, off);
    m.set_u32(0x10, (ld * 2) as u32);
    let st = unsafe { m.copy_wstr(off as usize, (ld + 1) * 2, dst) };
    if st < 0 {
        return st;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 && !copied.is_null() {
        unsafe { *copied = m.u64(0x28) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXBlobCreate(adapter: *mut c_void, path: *const u16, size: u64, flags: u32) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if path.is_null() {
        return fail("Invalid Input Path.\n");
    }
    if flags != 0 {
        return fail("Flags must be zero.\n");
    }
    let n = unsafe { wlen(path) };
    let len = n * 2 + 0x32;
    let mut m = XvdMsg::new(0x7800_00e4, len);
    m.set_u32(0, len as u32);
    m.set_u32(4, 0x30);
    m.set_u32(8, (n * 2) as u32);
    m.set_u32(0xc, 0);
    m.set_u64(0x10, size);
    let st = unsafe { m.copy_wstr(0x30, (n + 1) * 2, path) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXBlobDelete(adapter: *mut c_void, path: *const u16) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    if path.is_null() {
        return fail("Invalid Input Path.\n");
    }
    let n = unsafe { wlen(path) };
    let len = n * 2 + 0x1a;
    let mut m = XvdMsg::new(0x7800_00e8, len);
    m.set_u32(0, len as u32);
    m.set_u32(4, 0x18);
    m.set_u32(8, (n * 2) as u32);
    m.set_u32(0xc, 0);
    let st = unsafe { m.copy_wstr(0x18, (n + 1) * 2, path) };
    if st < 0 {
        return st;
    }
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXpfOp(adapter: *mut c_void, op: u32, reserved1: u64, reserved2: u64) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if reserved1 != 0 || reserved2 != 0 {
        return fail("Reserved values must be zero\n");
    }
    if op > 2 {
        return fail("Invalid OP value\n");
    }
    let mut m = XvdMsg::new(0x7800_00b4, 0x88);
    m.set_u32(0, op);
    unsafe { m.send_adapter(adapter) }
}
