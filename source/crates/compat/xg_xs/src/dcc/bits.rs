pub fn mask(n: u32) -> u32 {
    if n == 32 { 0xffff_ffff } else { 1u32.wrapping_shl(n).wrapping_sub(1) }
}

pub fn is_const_key(meta: u32) -> bool {
    (meta >> 4) & 1 != 0 && meta & 0xf == 0
}

pub fn seg_info_checked(meta: u32) -> Option<[u32; 8]> {
    let mut out = [0u32; 8];
    if meta & 0xf == 0 {
        let c = is_const_key(meta) as u32;
        for i in 0..4 {
            out[4 + i] = c;
            out[i] = 0xf;
        }
        return Some(out);
    }
    let bit = |p: u32| (meta >> (p & 31)) & 1 != 0;
    let mut edi = 0u32;
    let mut edx = 0u32;
    loop {
        let mut r9 = 1u32;
        loop {
            if bit(edx) {
                break;
            }
            r9 += 1;
            edx += 1;
            if edx >= 8 {
                return Some(out);
            }
        }
        edx += 1;
        let w = if r9 == 4 {
            let p = edx;
            edx += 1;
            if bit(p) {
                return Some(out);
            }
            3
        } else if r9 == 1 {
            1
        } else if r9 == 2 {
            2
        } else {
            3
        };
        let field = (meta.checked_shr(edx).unwrap_or(0)) & mask(w);
        let next = w + edx;
        edx = next;
        let end = r9 + edi;
        if edi < end {
            let hi = (mask(field + 1) << ((edi * 2) & 31)) & 0xff;
            let lo = (mask(r9) << (edi & 31)) & 0xf;
            for k in edi..end {
                let k = k as usize;
                if k + 4 >= 8 {
                    return None;
                }
                out[k + 4] = hi;
                out[k] = lo;
            }
        }
        edi = end;
        if edx >= 8 {
            return Some(out);
        }
    }
}

pub fn seg_info(meta: u8) -> [u32; 8] {
    seg_info_checked(meta as u32).unwrap_or([0; 8])
}

pub fn valid_meta(meta: u32) -> bool {
    seg_info_checked(meta).is_some()
}

pub fn sizes_differ(meta: u32, byte_off: u32) -> Option<bool> {
    let pc = |v: u32| v.count_ones();
    let a = if meta & 0xf == 0 {
        ((meta >> 4) & 1) << 5
    } else {
        let s = seg_info_checked(meta)?;
        let i = (byte_off >> 6) as usize;
        pc(*s.get(4 + i)?) << 5
    };
    let s = seg_info_checked(meta)?;
    let i = (byte_off >> 6) as usize;
    let b = pc(*s.get(i)?) << 6;
    Some(a != b)
}
