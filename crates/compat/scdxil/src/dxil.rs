pub const FOURCC_DXIL: u32 = 0x4c49_5844;
pub const FOURCC_DXBC: u32 = 0x4342_5844;
pub const FOURCC_XDAT: u32 = 0x5441_4458;
pub const XDAT_ORIGINAL: u32 = 7;
pub const XDAT_REPLACEMENT: u32 = 0xb;

unsafe fn u32_at(p: *const u8, off: usize) -> u32 {
    (p.add(off) as *const u32).read_unaligned()
}

unsafe fn u16_at(p: *const u8, off: usize) -> u16 {
    (p.add(off) as *const u16).read_unaligned()
}

pub unsafe fn is_dxil_program(p: *const u8, size: u32) -> bool {
    if p.is_null() || u32_at(p, 8) != FOURCC_DXIL {
        return false;
    }
    if size < 0x18 || (size as u64) < ((u32_at(p, 4) as u64) << 2) {
        return false;
    }
    let s8 = size.wrapping_sub(8);
    if s8 <= 0x10 {
        return false;
    }
    let off = u32_at(p, 0x10);
    let end = off.wrapping_add(u32_at(p, 0x14));
    end > off && s8 >= end
}

pub unsafe fn is_valid_program(p: *const u8) -> bool {
    if p.is_null() || u32_at(p, 8) != FOURCC_DXIL {
        return false;
    }
    let n = u32_at(p, 4);
    let size = n.wrapping_mul(4);
    if size < 0x18 || (size as u64) < ((n as u64) << 2) {
        return false;
    }
    let s8 = size.wrapping_sub(8);
    if s8 <= 0x10 {
        return false;
    }
    let off = u32_at(p, 0x10);
    let end = off.wrapping_add(u32_at(p, 0x14));
    end > off && s8 >= end
}

pub unsafe fn program_size(p: *const u8) -> u32 {
    let n = u32_at(p, 4);
    let v = n.wrapping_mul(4);
    if n >> 31 != 0 { v | 0x8000_0000 } else { v }
}

unsafe fn xdat_table(p: *const u8) -> (*const u8, u32) {
    if u32_at(p, 0x10).wrapping_add(8) < 0x24 || u32_at(p, 0x18) != FOURCC_XDAT {
        return (core::ptr::null(), 0);
    }
    let count = u32_at(p, 0x1c);
    if count == 0 {
        return (core::ptr::null(), 0);
    }
    (p.add(u32_at(p, 0x20) as usize), count)
}

pub unsafe fn find_xdat(p: *const u8, kind: u32) -> Option<*const u8> {
    let (table, count) = xdat_table(p);
    (0..count as usize).find_map(|i| {
        let e = table.add(i * 12);
        (u32_at(e, 0) == kind).then(|| p.add(u32_at(e, 4) as usize))
    })
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShaderRef {
    pub data: *const u8,
    pub size: u32,
    pub pad: u32,
}

pub const EMPTY: ShaderRef = ShaderRef { data: core::ptr::null(), size: 0, pad: 0 };

pub unsafe fn original_shader(hdr: *const u8) -> ShaderRef {
    if !is_valid_program(hdr) {
        return EMPTY;
    }
    let mut r = ShaderRef { data: hdr, size: program_size(hdr), pad: 0 };
    if let Some(p2) = find_xdat(hdr, XDAT_ORIGINAL) {
        if is_valid_program(p2) || u32_at(p2, 4) >> 31 != 0 {
            let mut x = p2;
            if let Some(y) = find_xdat(p2, XDAT_REPLACEMENT) {
                if !is_valid_program(y) {
                    return EMPTY;
                }
                x = y;
            }
            r = ShaderRef { data: x, size: program_size(x), pad: 0 };
        }
    }
    r
}

pub unsafe fn is_valid_container(c: *const u8, len: u64) -> bool {
    if c.is_null() || len < 0x20 || u32_at(c, 0) != FOURCC_DXBC || u16_at(c, 0x14) != 1 {
        return false;
    }
    let cs = u32_at(c, 0x18) as u64;
    if cs > len || cs > 0x8000_0000 {
        return false;
    }
    let pc = u32_at(c, 0x1c) as u64;
    if pc * 4 + 0x20 > cs {
        return false;
    }
    let mut next = 0x20 + pc * 4;
    for i in 0..pc {
        let off = u32_at(c, 0x20 + 4 * i as usize) as u64;
        if off > cs - 8 || next != off {
            return false;
        }
        let psz = u32_at(c, off as usize + 4) as u64;
        if off + 8 + psz > cs {
            return false;
        }
        next += 8 + psz;
    }
    next == cs
}

pub unsafe fn find_part(c: *const u8, fourcc: u32) -> *const u8 {
    if c.is_null() || u32_at(c, 0x18) < 0x20 || u32_at(c, 0) != FOURCC_DXBC {
        return core::ptr::null();
    }
    for i in 0..u32_at(c, 0x1c) as usize {
        let part = c.add(u32_at(c, 0x20 + 4 * i) as usize);
        if u32_at(part, 0) == fourcc {
            return part;
        }
    }
    core::ptr::null()
}

pub unsafe fn driver_shader_code(c: *const u8, out: *mut *const u8) -> u32 {
    if out.is_null() || c.is_null() || u32_at(c, 0) != FOURCC_DXBC || !is_valid_container(c, u32_at(c, 0x18) as u64) {
        return 3;
    }
    let part = find_part(c, FOURCC_DXIL);
    if part.is_null() {
        return 3;
    }
    *out = part.add(8);
    0
}

pub unsafe fn stage_supported_by_new_be(desc: *const u8) -> u32 {
    let mode = (u32_at(desc, 0xc) >> 27) & 3;
    let stage = (u32_at(desc, 0x18) >> 21) & 0x1f;
    ((mode == 1 || mode == 3) && stage < 25) as u32
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn program(bitcode: usize, xdat: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let table_len = if xdat.is_empty() { 0 } else { 12 + 12 * xdat.len() };
        let payload: usize = xdat.iter().map(|(_, b)| b.len()).sum();
        let bitcode_offset = 0x10 + table_len + payload;
        let total = (8 + bitcode_offset + bitcode + 3) & !3;
        let mut v = vec![0u8; total];
        let w = |v: &mut Vec<u8>, o: usize, x: u32| v[o..o + 4].copy_from_slice(&x.to_le_bytes());
        w(&mut v, 0, 0x0006_0060);
        w(&mut v, 4, (total / 4) as u32);
        w(&mut v, 8, FOURCC_DXIL);
        w(&mut v, 0xc, 0x106);
        w(&mut v, 0x10, bitcode_offset as u32);
        w(&mut v, 0x14, bitcode as u32);
        if !xdat.is_empty() {
            w(&mut v, 0x18, FOURCC_XDAT);
            w(&mut v, 0x1c, xdat.len() as u32);
            w(&mut v, 0x20, 0x24);
            let mut data = 0x24 + 12 * xdat.len();
            for (i, (k, b)) in xdat.iter().enumerate() {
                let e = 0x24 + 12 * i;
                w(&mut v, e, *k);
                w(&mut v, e + 4, data as u32);
                w(&mut v, e + 8, b.len() as u32);
                v[data..data + b.len()].copy_from_slice(b);
                data += b.len();
            }
        }
        v
    }

    pub fn container(parts: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut v = vec![0u8; 0x20 + 4 * parts.len()];
        v[0..4].copy_from_slice(&FOURCC_DXBC.to_le_bytes());
        v[0x14..0x16].copy_from_slice(&1u16.to_le_bytes());
        v[0x1c..0x20].copy_from_slice(&(parts.len() as u32).to_le_bytes());
        for (i, (k, b)) in parts.iter().enumerate() {
            let off = v.len() as u32;
            v[0x20 + 4 * i..0x24 + 4 * i].copy_from_slice(&off.to_le_bytes());
            v.extend_from_slice(&k.to_le_bytes());
            v.extend_from_slice(&(b.len() as u32).to_le_bytes());
            v.extend_from_slice(b);
        }
        let n = v.len() as u32;
        v[0x18..0x1c].copy_from_slice(&n.to_le_bytes());
        v
    }

    #[test]
    fn dxil_program_header_rules() {
        unsafe {
            let p = program(32, &[]);
            let n = p.len() as u32;
            assert!(is_dxil_program(p.as_ptr(), n));
            assert!(is_valid_program(p.as_ptr()));
            assert!(!is_dxil_program(p.as_ptr(), n - 4));
            assert!(!is_dxil_program(p.as_ptr(), 0x17));
            assert!(!is_dxil_program(core::ptr::null(), n));
            let mut bad = p.clone();
            bad[8] = b'X';
            assert!(!is_dxil_program(bad.as_ptr(), n));
            let mut empty = p.clone();
            empty[0x14..0x18].copy_from_slice(&0u32.to_le_bytes());
            assert!(!is_dxil_program(empty.as_ptr(), n));
            let mut over = p.clone();
            over[0x14..0x18].copy_from_slice(&(n).to_le_bytes());
            assert!(!is_dxil_program(over.as_ptr(), n));
            assert!(!is_valid_program(over.as_ptr()));
            assert_eq!(program_size(p.as_ptr()), n);
        }
    }

    #[test]
    fn original_shader_follows_xdat_parts() {
        unsafe {
            let plain = program(16, &[]);
            assert_eq!(original_shader(plain.as_ptr()), ShaderRef { data: plain.as_ptr(), size: plain.len() as u32, pad: 0 });
            let inner = program(24, &[]);
            let outer = program(16, &[(3, vec![1, 2, 3, 4]), (XDAT_ORIGINAL, inner.clone())]);
            let r = original_shader(outer.as_ptr());
            let off = outer.windows(inner.len()).position(|w| w == &inner[..]).unwrap();
            assert_eq!(r.data, outer.as_ptr().add(off));
            assert_eq!(r.size, inner.len() as u32);
            let leaf = program(8, &[]);
            let mid = program(12, &[(XDAT_REPLACEMENT, leaf.clone())]);
            let top = program(16, &[(XDAT_ORIGINAL, mid.clone())]);
            let r = original_shader(top.as_ptr());
            assert_eq!(core::slice::from_raw_parts(r.data, r.size as usize), &leaf[..]);
            let junk = program(16, &[(XDAT_ORIGINAL, vec![0u8; 32])]);
            assert_eq!(original_shader(junk.as_ptr()).data, junk.as_ptr());
            assert_eq!(original_shader(core::ptr::null()), EMPTY);
        }
    }

    #[test]
    fn container_and_driver_code() {
        unsafe {
            let dxil = program(16, &[]);
            let c = container(&[(0x4e475349, vec![0; 8]), (FOURCC_DXIL, dxil.clone())]);
            assert!(is_valid_container(c.as_ptr(), c.len() as u64));
            assert!(!is_valid_container(c.as_ptr(), c.len() as u64 - 1));
            let mut out: *const u8 = core::ptr::null();
            assert_eq!(driver_shader_code(c.as_ptr(), &mut out), 0);
            assert_eq!(core::slice::from_raw_parts(out, dxil.len()), &dxil[..]);
            assert_eq!(driver_shader_code(c.as_ptr(), core::ptr::null_mut()), 3);
            let no_dxil = container(&[(0x4e475349, vec![0; 8])]);
            assert_eq!(driver_shader_code(no_dxil.as_ptr(), &mut out), 3);
            let mut gap = c.clone();
            let off = u32::from_le_bytes(gap[0x24..0x28].try_into().unwrap()) + 4;
            gap[0x24..0x28].copy_from_slice(&off.to_le_bytes());
            assert!(!is_valid_container(gap.as_ptr(), gap.len() as u64));
            let mut v2 = c.clone();
            v2[0x14] = 2;
            assert_eq!(driver_shader_code(v2.as_ptr(), &mut out), 3);
            assert_eq!(find_part(c.as_ptr(), FOURCC_DXIL), c.as_ptr().add(u32::from_le_bytes(c[0x24..0x28].try_into().unwrap()) as usize));
            assert!(find_part(c.as_ptr(), 0x1234).is_null());
        }
    }

    #[test]
    fn new_backend_stage_table() {
        unsafe {
            let mut d = [0u8; 0x20];
            for mode in 0u32..4 {
                for stage in 0u32..32 {
                    d[0xc..0x10].copy_from_slice(&(mode << 27 | 0x07ff_ffff).to_le_bytes());
                    d[0x18..0x1c].copy_from_slice(&(stage << 21 | 0xc01f_ffff).to_le_bytes());
                    let want = (mode == 1 || mode == 3) && stage < 25;
                    assert_eq!(stage_supported_by_new_be(d.as_ptr()), want as u32, "mode {mode} stage {stage}");
                }
            }
        }
    }
}
