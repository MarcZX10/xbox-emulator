pub(crate) fn mask(n: u32) -> u32 {
    if n == 32 {
        0xffff_ffff
    } else {
        1u32.wrapping_shl(n).wrapping_sub(1)
    }
}

pub(crate) fn log2c(x: u32) -> u32 {
    let (mut c, mut v) = (0u32, x);
    while v > 1 {
        c += 1;
        v = (v + 1) >> 1;
    }
    c
}

pub(crate) fn shr_keep(v: u32, n: u32) -> u32 {
    if n as i32 > 31 {
        0
    } else {
        v.wrapping_shr(n) & mask(32u32.wrapping_sub(n))
    }
}

pub(crate) fn remove_bits(v: u32, pos: u32, width: u32) -> u32 {
    if (pos as i32) < width as i32 {
        return v;
    }
    let low = if (width.wrapping_sub(1) as i32) < 0 { 0 } else { mask(width) & v };
    let p1 = pos.wrapping_add(1);
    let hi = if p1 as i32 > 31 { 0 } else { v.wrapping_shr(p1) & mask(32u32.wrapping_sub(p1)) };
    low | hi.wrapping_shl(width)
}

pub(crate) fn insert_bits(v: u32, ins: u32, pos: u32, width: u32) -> u32 {
    if (pos as i32) < width as i32 {
        return v;
    }
    let low = if (width.wrapping_sub(1) as i32) < 0 { 0 } else { mask(width) & v };
    let d = pos.wrapping_sub(width);
    let mid = if (d as i32) < 0 { 0 } else { mask(d.wrapping_add(1)) & ins };
    let hi = if width as i32 > 31 { 0 } else { v.wrapping_shr(width) & mask(32u32.wrapping_sub(width)) };
    hi.wrapping_shl(pos.wrapping_add(1)) | mid.wrapping_shl(width) | low
}

pub(crate) fn copy_bit(dst: &mut u32, di: u32, src: u32, si: u32) {
    let b = 1u32.wrapping_shl(di);
    if src.wrapping_shr(si) & 1 != 0 {
        *dst |= b;
    } else {
        *dst &= !b;
    }
}

pub(crate) fn popcount_bytes(v: u32) -> u32 {
    v.to_le_bytes().iter().map(|b| b.count_ones()).sum()
}

pub(crate) fn key_decode(key: u32) -> [u32; 16] {
    let mut out = [0u32; 16];
    if key & 0xf == 0 {
        out[..4].fill(0xf);
        return out;
    }
    let bit = |i: u32| key.wrapping_shr(i & 31) & 1 != 0;
    let (mut edi, mut edx) = (0u32, 0u32);
    loop {
        let mut r9 = 1u32;
        if !bit(edx) {
            loop {
                r9 += 1;
                edx += 1;
                if edx >= 8 {
                    return out;
                }
                if bit(edx) {
                    break;
                }
            }
        }
        edx += 1;
        let w = if r9 == 4 {
            let b = bit(edx);
            edx += 1;
            if b {
                return out;
            }
            3
        } else if r9 == 1 {
            1
        } else {
            (r9 != 2) as u32 + 2
        };
        let r10 = w + edx;
        let val = if (r10.wrapping_sub(1) as i32) < edx as i32 { 0 } else { key.wrapping_shr(edx) & mask(r10 - edx) };
        edx = r10;
        let size = (mask(val.wrapping_add(1)).wrapping_shl(edi.wrapping_mul(2)) & 0xff) as u32;
        let sec = mask(r9).wrapping_shl(edi) & 0xf;
        for i in edi..edi + r9 {
            if (i as usize) < 12 {
                out[i as usize + 4] = size;
                out[i as usize] = sec;
            }
        }
        edi += r9;
        if edx >= 8 {
            return out;
        }
    }
}

pub(crate) fn key_overflows(key: u32) -> bool {
    if key & 0xf == 0 {
        return false;
    }
    let bit = |i: u32| key.wrapping_shr(i & 31) & 1 != 0;
    let (mut edi, mut edx) = (0u32, 0u32);
    loop {
        let mut r9 = 1u32;
        if !bit(edx) {
            loop {
                r9 += 1;
                edx += 1;
                if edx >= 8 {
                    return false;
                }
                if bit(edx) {
                    break;
                }
            }
        }
        edx += 1;
        let w = if r9 == 4 {
            let b = bit(edx);
            edx += 1;
            if b {
                return false;
            }
            3
        } else if r9 == 1 {
            1
        } else {
            (r9 != 2) as u32 + 2
        };
        edx += w;
        if edi + r9 > 4 {
            return true;
        }
        edi += r9;
        if edx >= 8 {
            return false;
        }
    }
}

pub(crate) fn sector_compressed(key: u32, offset: u32) -> bool {
    let s = (offset >> 6) as usize;
    let a = key_decode(key);
    let sizes = popcount_bytes(a[4 + s]) << 5;
    let b = key_decode(key);
    let full = popcount_bytes(b[s]) << 6;
    sizes != full
}
