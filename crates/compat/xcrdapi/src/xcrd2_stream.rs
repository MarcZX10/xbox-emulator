use crate::xcrd2_storage::{fail, HR_INSUFFICIENT};
use crate::xcrd_core::*;
use core::ffi::c_void;

pub const MAX_XVIO: u32 = 0x10_0000;
pub const SPECIFIER_SIZE: usize = 0x188;
pub const REGION_INFO_SIZE: usize = 0x30;
pub const REGION_REPLY_SIZE: usize = 0x38;
pub const STREAM_INFO_SIZE: usize = 0x100;

unsafe fn rd32(p: *const u8, o: usize) -> u32 {
    unsafe { (p.add(o) as *const u32).read_unaligned() }
}

unsafe fn wr32(p: *mut u8, o: usize, v: u32) {
    unsafe { (p.add(o) as *mut u32).write_unaligned(v) }
}

unsafe fn wr64(p: *mut u8, o: usize, v: u64) {
    unsafe { (p.add(o) as *mut u64).write_unaligned(v) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingStart(
    out_handle: *mut u64,
    adapter: *mut c_void,
    src: *const u16,
    src_extra: *const u16,
    dst: *const u16,
    dst_content: *const u16,
    dst_extra: *const u16,
    spec_count: u32,
    specs: *const [*const u16; 2],
    flags: u32,
    out_size: *mut u64,
    out_status: *mut u32,
) -> i32 {
    if !out_size.is_null() {
        unsafe { *out_size = 0 };
    }
    if !out_status.is_null() {
        unsafe { *out_status = 0 };
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if src.is_null() {
        return fail("Src Path must be specified\n");
    }
    if dst.is_null() {
        return fail("Dst Path must be specified\n");
    }
    let content = flags & 0x1000;
    if content == 0 && !dst_content.is_null() {
        return fail("Dst Content Path must not be specified\n");
    }
    let flat = flags & 0x1800;
    if flat == 0x800 || (flags & 0x2000 != 0 && flat != 0x1800) || flags & 0x5000 == 0x4000 {
        return fail("Invalid combination of flat file flags\n");
    }
    if flags & 0xffff_8000 != 0 {
        return fail("Invalid flags\n");
    }
    if spec_count == 0 {
        if !specs.is_null() {
            return fail("Specifiers must be NULL if zero specifiers are passed\n");
        }
    } else if specs.is_null() {
        return fail("Specifiers must not be NULL\n");
    }
    let wl = |p: *const u16| unsafe { wlen(p) } as u32;
    let mut size = (wl(src).wrapping_add(wl(dst)).wrapping_add(spec_count.wrapping_mul(8))).wrapping_mul(2).wrapping_add(0x6c);
    for i in 0..spec_count as usize {
        let s = unsafe { *specs.add(i) };
        size = size.wrapping_add((wl(s[1]).wrapping_add(wl(s[0]))).wrapping_mul(2)).wrapping_add(4);
    }
    for p in [src_extra, dst_content, dst_extra] {
        if !p.is_null() {
            size = size.wrapping_add(wl(p) * 2 + 2);
        }
    }
    if size > MAX_XVIO {
        return fail("Payload too large for XVIO\n");
    }
    let mut m = XvdMsg::new(0x7800_0030, size as usize);
    let mut f = 0u64;
    for (bit, to) in [(1u32, 0x10u64), (2, 0x40), (4, 0x80), (8, 0x100), (0x10, 0x200), (0x100, 0x4000), (0x200, 0x8000), (0x400, 0x1_0000), (0x1000, 0x4_0000), (0x800, 0x400), (0x2000, 0x800), (0x4000, 0x1000)] {
        if flags & bit != 0 {
            f |= to;
        }
    }
    m.set_u64(0x10, f);
    m.set_u32(0, 0x68);
    let ls = wl(src) * 2;
    m.set_u32(4, ls);
    let mut next = ls + 0x6a;
    m.set_u32(8, next);
    let ld = wl(dst) * 2;
    m.set_u32(0xc, ld);
    next = next + 2 + ld;
    for (p, len_off, off_off) in [(src_extra, 0x24usize, 0x20usize), (dst_content, 0x50, 0x4c), (dst_extra, 0x48, 0x44)] {
        if !p.is_null() {
            let l = wl(p) * 2;
            m.set_u32(len_off, l);
            if l != 0 {
                m.set_u32(off_off, next);
                next += l + 2;
            }
        }
    }
    if spec_count != 0 {
        m.set_u32(0x34, spec_count);
        m.set_u32(0x2c, next);
        m.set_u32(0x30, spec_count << 4);
    }
    let mut hr = unsafe { m.copy_wstr(0x68, m.u32(4) as usize + 2, src) };
    if hr < 0 {
        return hr;
    }
    hr = unsafe { m.copy_wstr(m.u32(8) as usize, m.u32(0xc) as usize + 2, dst) };
    if hr < 0 {
        return hr;
    }
    for (p, len_off, off_off) in [(src_extra, 0x24usize, 0x20usize), (dst_content, 0x50, 0x4c), (dst_extra, 0x48, 0x44)] {
        if m.u32(len_off) != 0 {
            hr = unsafe { m.copy_wstr(m.u32(off_off) as usize, m.u32(len_off) as usize + 2, p) };
            if hr < 0 {
                return hr;
            }
        }
    }
    if spec_count != 0 {
        let table = m.u32(0x2c) as usize;
        let mut at = m.u32(0x30).wrapping_add(m.u32(0x2c));
        let n = m.u32(0x34) as usize;
        for i in 0..n {
            let s = unsafe { *specs.add(i) };
            for k in 0..2 {
                let l = wl(s[k]) * 2;
                hr = unsafe { m.copy_wstr(at as usize, l as usize + 2, s[k]) };
                let total = m.u32(0x30) + l + 2;
                m.set_u32(0x30, total);
                m.set_u32(table + i * 16 + k * 8, at);
                at = at + 2 + l;
                m.set_u32(table + i * 16 + k * 8 + 4, l);
                if hr < 0 {
                    return hr;
                }
            }
        }
    }
    hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe { *out_handle = m.u64(0x18) };
    }
    if !out_size.is_null() {
        unsafe { *out_size = m.u64(0x38) };
    }
    if !out_status.is_null() {
        unsafe { *out_status = m.u32(0x40) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingStop(adapter: *mut c_void, handle: u64, reserved: u64) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if reserved != 0 {
        return fail("Reserved field must be zero\n");
    }
    let mut m = XvdMsg::new(0x7800_0034, 0x58);
    m.set_u64(0, handle);
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingAdjustStreamingFlags(adapter: *mut c_void, handle: u64, enable: u32, disable: u32) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if enable & 0x7fff != enable {
        return fail("Invalid Enable flags\n");
    }
    if disable & 0x7fff != disable {
        return fail("Invalid Disable flags\n");
    }
    if enable & disable != 0 {
        return fail("Cannot enable and disable same flags\n");
    }
    if enable & 9 != enable {
        return fail("Invalid Enable flags\n");
    }
    if disable & 9 != disable {
        return fail("Invalid Disable flags\n");
    }
    let map = |v: u32| (if v & 1 != 0 { 0x10 } else { 0 }) | (if v & 8 != 0 { 0x100 } else { 0 });
    let mut m = XvdMsg::new(0x7800_00dc, 0x10);
    m.set_u64(0, handle);
    m.set_u32(8, map(enable));
    m.set_u32(0xc, map(disable));
    unsafe { m.send_adapter(adapter) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueryActiveInstanceId(adapter: *mut c_void, out: *mut u64) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    let mut m = XvdMsg::new(0x7800_003c, 0x60);
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe { *out = m.u64(0) };
    }
    hr
}

pub unsafe fn unpack_stream_info(out: *mut u8, m: &XvdMsg) {
    let p = m.pr().as_ptr();
    let c8 = |d: usize, s: usize| unsafe { core::ptr::copy_nonoverlapping(p.add(s), out.add(d), 8) };
    let c4 = |d: usize, s: usize| unsafe { core::ptr::copy_nonoverlapping(p.add(s), out.add(d), 4) };
    for o in [0x58usize, 0x60, 0x50] {
        c8(o, o);
    }
    c4(0x78, 0x78);
    for o in [0x80usize, 0x68, 0x70] {
        c8(o, o);
    }
    c4(0x9c, 0xac);
    c4(0xa8, 0xb8);
    unsafe { *out.add(0xac) = m.u8(0xbc) };
    for o in [8usize, 0x38, 0x18, 0x20, 0x28, 0x30, 0x40, 0x48, 0x10, 0, 0x88] {
        c8(o, o);
    }
    c4(0x90, 0x9c);
    c4(0x7c, 0xa4);
    c4(0x94, 0xa0);
    let old = unsafe { rd32(out, 0x98) };
    unsafe { wr32(out, 0x98, (old & !0x3ff) | (m.u32(0xa8) & 0x3ff)) };
    c4(0xa0, 0xb0);
    c4(0xa4, 0xb4);
    c8(0xb8, 0xc8);
    c8(0xb0, 0xc0);
    c8(0xc0, 0xd0);
    c4(0xc8, 0xd8);
    c4(0xcc, 0xdc);
    unsafe { core::ptr::copy_nonoverlapping(p.add(0xe0), out.add(0xd0), 16) };
    for (d, s) in [(0xe0usize, 0xf0usize), (0xe8, 0xf8), (0xf0, 0x100), (0xf8, 0x108)] {
        c8(d, s);
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueryInformation(out: *mut u8, adapter: *mut c_void, handle: u64) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    let mut m = XvdMsg::new(0x7800_0038, 0x110);
    m.set_u64(0, handle);
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe { unpack_stream_info(out, &m) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueryInformationByPath(out: *mut u8, adapter: *mut c_void, path: *const u16, flags: u32) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if path.is_null() {
        return fail("Invalid XCRD Path\n");
    }
    let l = unsafe { wlen(path) } * 2;
    if l as u32 == 0 {
        return fail("Invalid XCRD String Length\n");
    }
    if flags & !1 != 0 {
        return fail("Invalid Flags\n");
    }
    let mut m = XvdMsg::new(0x7800_0038, l + 0x112);
    m.set_u64(0, u64::MAX);
    m.set_u32(0x98, if flags & 1 != 0 { 3 } else { 1 });
    let st = unsafe { m.copy_wstr(0x110, l + 2, path) };
    if st < 0 {
        return st;
    }
    m.set_u32(0x90, 0x110);
    m.set_u32(0x94, l as u32 + 2);
    let hr = unsafe { m.send_adapter(adapter) };
    if hr >= 0 {
        unsafe { unpack_stream_info(out, &m) };
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueryRegionIdByOffset(out: *mut u8, adapter: *mut c_void, path: *const u16, kind: u32, offset: u64) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if path.is_null() {
        return fail("Invalid XCRD Path\n");
    }
    if unsafe { *path } == 0 {
        return fail("Invalid XCRD Path - 2\n");
    }
    if kind > 1 {
        return fail("Invalid XCRD Type\n");
    }
    let n = unsafe { wlen(path) };
    let mut m = XvdMsg::new(0x7800_0048, n * 2 + 0x5a);
    m.set_u64(0x10, offset);
    m.set_u32(8, kind);
    m.set_u32(0, 0x58);
    m.set_u32(4, (n * 2 + 2) as u32);
    let st = unsafe { m.copy_wstr(0x58, n * 2 + 4, path) };
    if st < 0 {
        return st;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    unsafe {
        let old = rd32(out, 4);
        wr32(out, 4, (old & !0x7f) | (m.u32(0x24) & 0x7f));
        wr64(out, 8, m.u64(0x28));
        wr64(out, 0x10, m.u64(0x30));
        wr32(out, 0, m.u32(0x20));
        wr64(out, 0x18, m.u64(0x38));
    }
    hr
}

pub unsafe fn region_info_chunk(list: *mut u8, adapter: *mut c_void, source: *const c_void, flags: u32, count: u32, ids: *const u32) -> i32 {
    if adapter.is_null() {
        return fail("Adapter Handle should be non zero\n");
    }
    if flags & !1 != 0 {
        return fail("Invalid Query Region Flags\n");
    }
    let by_path = flags & 1 != 0;
    if by_path && source.is_null() {
        return fail("Must Specify Source Path\n");
    }
    let mut size = count.wrapping_mul(0x3c).wrapping_add(0x18);
    let path = source as *const u16;
    if by_path {
        size = size.wrapping_add(unsafe { wlen(path) } as u32 * 2 + 2);
    }
    let mut m = XvdMsg::new(0x7800_0040, size as usize);
    let ids_at;
    if by_path {
        let n = unsafe { wlen(path) };
        let st = unsafe { m.copy_wstr(0x18, n * 2 + 2, path) };
        if st < 0 {
            return st;
        }
        m.set_u32(0x10, 0x18);
        m.set_u32(0x14, (n * 2 + 2) as u32);
        m.set_u64(0, u64::MAX);
        ids_at = 0x18 + m.u32(0x14) as usize;
    } else {
        m.set_u64(0, source as u64);
        ids_at = 0x18;
    }
    let reply_at = ids_at + count as usize * 4;
    m.set_u32(0xc, flags);
    m.set_u32(8, count);
    unsafe { m.set_raw(ids_at, ids as *const u8, count as usize * 4) };
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    for i in 0..count as usize {
        let r = reply_at + i * REGION_REPLY_SIZE;
        let o = unsafe { list.add(i * REGION_INFO_SIZE) };
        unsafe {
            wr32(o, 4, m.u32(r + 4) & 0x7f);
            wr32(o, 0, m.u32(r));
            wr32(o, 0x20, m.u32(r + 0x20));
            wr64(o, 0x10, m.u64(r + 0x10));
            wr64(o, 8, m.u64(r + 8));
            wr64(o, 0x18, m.u64(r + 0x18));
        }
    }
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueryRegionInformation(list: *mut u8, adapter: *mut c_void, handle: u64, count: u32, ids: *const u32) -> i32 {
    if count == 0 {
        return fail("Region Count should not be zero\n");
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if list.is_null() || ids.is_null() {
        return fail("RegionInfoList or RegionList must not be NULL\n");
    }
    let mut chunk = count;
    while chunk.wrapping_mul(0x3c).wrapping_add(0x18) > MAX_XVIO {
        chunk >>= 1;
    }
    let mut left = count;
    let mut n = chunk;
    let mut l = list;
    let mut r = ids;
    loop {
        let hr = unsafe { region_info_chunk(l, adapter, handle as *const c_void, 0, n, r) };
        if hr < 0 {
            return hr;
        }
        left -= n;
        r = unsafe { r.add(chunk as usize) };
        l = unsafe { l.add(chunk as usize * REGION_INFO_SIZE) };
        if left < chunk {
            n = left;
        }
        if left == 0 {
            return hr;
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueryRegionInformationByPath(list: *mut u8, adapter: *mut c_void, path: *const u16, count: u32, ids: *const u32) -> i32 {
    if count == 0 {
        return fail("Region Count should not be zero\n");
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if list.is_null() || ids.is_null() {
        return fail("RegionInfoList or RegionList must not be NULL\n");
    }
    let plen = unsafe { wlen(path) } as u32;
    let mut chunk = count;
    while plen.wrapping_add(chunk.wrapping_mul(0x1e)).wrapping_mul(2).wrapping_add(0x1a) > MAX_XVIO {
        chunk >>= 1;
    }
    let mut left = count;
    let mut n = chunk;
    let mut l = list;
    let mut r = ids;
    loop {
        let hr = unsafe { region_info_chunk(l, adapter, path as *const c_void, 1, n, r) };
        if hr < 0 {
            return hr;
        }
        left -= n;
        r = unsafe { r.add(chunk as usize) };
        l = unsafe { l.add(chunk as usize * REGION_INFO_SIZE) };
        if left < chunk {
            n = left;
        }
        if left == 0 {
            return hr;
        }
    }
}

pub unsafe fn region_list_chunk(list: *mut u32, adapter: *mut c_void, source: *const c_void, flags: u32, start: u32, count: u32, written: *mut u32, queue: u32) -> i32 {
    if adapter.is_null() {
        return fail("Adapter Handle should be non zero\n");
    }
    if flags & !1 != 0 {
        return fail("Invalid Query Region Flags\n");
    }
    let by_path = flags & 1 != 0;
    let path = source as *const u16;
    if by_path && source.is_null() {
        return fail("Must Specify Source Path\n");
    }
    let mut size = count.wrapping_mul(4).wrapping_add(0x28);
    if by_path {
        size = size.wrapping_add(unsafe { wlen(path) } as u32 * 2 + 2);
    }
    let mut m = XvdMsg::new(0x7800_0080, size as usize);
    let mut reply_at = 0x28usize;
    let mut fill = true;
    if by_path {
        let n = unsafe { wlen(path) };
        if unsafe { m.copy_wstr(0x28, n * 2 + 2, path) } < 0 {
            fill = false;
        } else {
            m.set_u32(0x10, 0x28);
            m.set_u32(0x14, (n * 2 + 2) as u32);
            m.set_u64(0, u64::MAX);
            reply_at = 0x28 + m.u32(0x14) as usize;
        }
    } else {
        m.set_u64(0, source as u64);
        m.set_u64(0x10, 0);
    }
    if fill {
        m.set_u32(0xc, flags);
        m.set_u32(8, count);
        m.set_u32(0x1c, queue);
        m.set_u32(0x18, start);
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    let got = m.u32(8);
    if got > count {
        return HR_INSUFFICIENT;
    }
    for i in 0..got as usize {
        unsafe { *list.add(i) = m.u32(reply_at + i * 4) };
    }
    unsafe { *written = got };
    hr
}

unsafe fn region_list(list: *mut u32, written: *mut u32, adapter: *mut c_void, source: *const c_void, flags: u32, count: u32) -> impl FnMut(u32) -> i32 {
    let mut chunk = count;
    while chunk.wrapping_mul(4).wrapping_add(0x28) > MAX_XVIO {
        chunk >>= 1;
    }
    move |queue: u32| {
        let mut total = 0u32;
        let mut left = count;
        let mut n = chunk;
        let mut start = 0u32;
        let mut l = list;
        loop {
            let mut got = 0u32;
            let hr = unsafe { region_list_chunk(l, adapter, source, flags, start, n, &mut got, queue) };
            if hr < 0 {
                return hr;
            }
            if got == 0 {
                unsafe { *written = total };
                return hr;
            }
            total = total.wrapping_add(got);
            left -= n;
            start = start.wrapping_add(chunk);
            if left < chunk {
                n = left;
            }
            l = unsafe { l.add(chunk as usize) };
            if left == 0 {
                unsafe { *written = total };
                return hr;
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueueQueryRegionList(list: *mut u32, written: *mut u32, adapter: *mut c_void, handle: u64, count: u32, queue: u32) -> i32 {
    if count == 0 {
        return fail("Region Count should not be zero\n");
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if list.is_null() {
        return fail(" RegionList must not be NULL\n");
    }
    if queue > 3 {
        return fail(" Invalid Queue Type Value\n");
    }
    let mut run = unsafe { region_list(list, written, adapter, handle as *const c_void, 0, count) };
    run(queue)
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueueQueryRegionListByPath(list: *mut u32, written: *mut u32, adapter: *mut c_void, path: *const u16, count: u32, queue: u32) -> i32 {
    if count == 0 {
        return fail("Region Count should not be zero\n");
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if path.is_null() {
        return fail("Invalid XCrdPath\n");
    }
    if list.is_null() {
        return fail(" RegionList must not be NULL\n");
    }
    if queue > 3 {
        return fail(" Invalid Queue Type Value\n");
    }
    let mut run = unsafe { region_list(list, written, adapter, path as *const c_void, 1, count) };
    run(queue)
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueueInsertRegion(adapter: *mut c_void, handle: u64, count: u32, ids: *const u32, flags: u32) -> i32 {
    if adapter.is_null() {
        return fail("Adapter Handle should be non zero\n");
    }
    if count == 0 {
        return fail("Region count can't be zero\n");
    }
    if flags != 0 {
        return fail("Flags should be zero for now\n");
    }
    let mut m = XvdMsg::new(0x7800_0044, count as usize * 4 + 0x38);
    m.set_u64(8, 0);
    m.set_u32(0x10, count);
    m.set_u32(0x14, 0x38);
    m.set_u64(0, handle);
    unsafe { m.set_raw(0x38, ids as *const u8, count as usize * 4) };
    unsafe { m.send_adapter(adapter) }
}

pub unsafe fn specifier_chunk(specs: *mut u8, written: *mut u32, adapter: *mut c_void, source: *const c_void, flags: u32, start: u32, count: u32) -> i32 {
    if adapter.is_null() {
        return fail("Adapter Handle should be non zero\n");
    }
    if flags & !1 != 0 {
        return fail("Invalid Query Region Specifier Flags\n");
    }
    let by_path = flags & 1 != 0;
    let path = source as *const u16;
    if by_path && source.is_null() {
        return fail("Must Specify Source Path\n");
    }
    if written.is_null() {
        return fail("Must Specify RegionSpecifiersWritten\n");
    }
    if specs.is_null() && count != 0 {
        return fail("Must Specify Specifiers or have 0 SpecifierCount\n");
    }
    let mut size = count.wrapping_mul(SPECIFIER_SIZE as u32).wrapping_add(0x20);
    if by_path {
        size = size.wrapping_add(unsafe { wlen(path) } as u32 * 2 + 2);
    }
    let mut m = XvdMsg::new(0x7800_00a8, size as usize);
    m.set_u32(0x18, start);
    m.set_u32(0x14, count);
    let mut reply_at = 0usize;
    if by_path {
        m.set_u32(0x1c, 1);
        let n = unsafe { wlen(path) };
        let st = unsafe { m.copy_wstr(0x20, n * 2 + 2, path) };
        if st < 0 {
            return st;
        }
        m.set_u32(8, 0x20);
        m.set_u32(0xc, (n * 2 + 2) as u32);
        m.set_u64(0, u64::MAX);
        if count != 0 {
            let at = 0x20 + m.u32(0xc);
            reply_at = at as usize;
            m.set_u32(0x10, at);
        }
    } else {
        m.set_u64(0, source as u64);
        if count != 0 {
            m.set_u32(0x10, 0x20);
            reply_at = 0x20;
        }
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    let got = m.u32(0x14);
    unsafe { *written = got };
    let mut i = 0u32;
    while i < got && i < count {
        let o = i as usize * SPECIFIER_SIZE;
        let d = unsafe { specs.add(o) };
        let r = reply_at + o;
        unsafe {
            wr32(d, 4, 0);
            wr32(d, 0, m.u32(r));
        }
        let mut tmp = [0u16; 0x40];
        unsafe { m.get_raw(r + 8, tmp.as_mut_ptr() as *mut u8, 0x80) };
        let st = unsafe { cch_copy(d.add(8) as *mut u16, 0x80, tmp.as_ptr()) };
        if st < 0 {
            return st;
        }
        let mut tmp2 = [0u16; 0x80];
        unsafe { m.get_raw(r + 0x88, tmp2.as_mut_ptr() as *mut u8, 0x100) };
        let st = unsafe { cch_copy(d.add(0x88) as *mut u16, 0x100, tmp2.as_ptr()) };
        if st < 0 {
            return st;
        }
        i += 1;
    }
    hr
}

unsafe fn specifiers(specs: *mut u8, written: *mut u32, adapter: *mut c_void, source: *const c_void, flags: u32, count: u32, first: u32) -> i32 {
    let mut chunk = count;
    let mut size = first;
    while size > MAX_XVIO {
        chunk >>= 1;
        size = chunk.wrapping_add(0x1a8);
    }
    unsafe { *written = 0 };
    if count == 0 {
        if !specs.is_null() {
            return 0;
        }
        return unsafe { specifier_chunk(core::ptr::null_mut(), written, adapter, source, flags, 0, 0) };
    }
    let mut left = count;
    let mut n = chunk;
    let mut start = 0u32;
    let mut s = specs;
    loop {
        let mut got = 0u32;
        let hr = unsafe { specifier_chunk(s, &mut got, adapter, source, flags, start, n) };
        if hr < 0 {
            return hr;
        }
        left -= n;
        start = start.wrapping_add(chunk);
        unsafe { *written = (*written).wrapping_add(got) };
        s = unsafe { s.add(chunk as usize * SPECIFIER_SIZE) };
        if left < chunk {
            n = left;
        }
        if left == 0 {
            return hr;
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueryRegionSpecifiers(specs: *mut u8, written: *mut u32, adapter: *mut c_void, handle: u64, count: u32) -> i32 {
    if written.is_null() {
        return fail("SpecifiersWritten should not be NULL\n");
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if handle == 0 {
        return fail("Invalid Stream Handle\n");
    }
    if count == 0 {
        if !specs.is_null() {
            return fail("Specifiers should be NULL with a zero specifier count\n");
        }
    } else if specs.is_null() {
        return fail("Specifiers should not be NULL\n");
    }
    let first = count.wrapping_mul(SPECIFIER_SIZE as u32).wrapping_add(0x20);
    unsafe { specifiers(specs, written, adapter, handle as *const c_void, 0, count, if count == 0 { 0 } else { first }) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdStreamingQueryRegionSpecifiersByPath(specs: *mut u8, written: *mut u32, adapter: *mut c_void, path: *const u16, count: u32) -> i32 {
    if written.is_null() {
        return fail("SpecifiersWritten should not be NULL\n");
    }
    if adapter.is_null() {
        return fail("Invalid Adapter Handle\n");
    }
    if path.is_null() {
        return fail("Invalid CRD Path\n");
    }
    if count == 0 {
        if !specs.is_null() {
            return fail("Specifiers should be NULL with a zero specifier count\n");
        }
    } else if specs.is_null() {
        return fail("Specifiers should not be NULL\n");
    }
    let n = unsafe { wlen(path) } as u32;
    let first = n.wrapping_add(count.wrapping_mul(0xc4)).wrapping_mul(2).wrapping_add(0x22);
    unsafe { specifiers(specs, written, adapter, path as *const c_void, 1, count, first) }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdSuspendResumeIoBalancer(adapter: *mut c_void, suspend: u8) -> i32 {
    if adapter.is_null() {
        return fail("Invalid Adapter Handle.\n");
    }
    let mut m = XvdMsg::new(0x7800_00a4, 1);
    m.set_u8(0, suspend);
    unsafe { m.send_adapter(adapter) }
}
