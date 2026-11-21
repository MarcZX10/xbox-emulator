use super::Dcc;
use crate::layout::E_INVALIDARG;

pub type Coord = [u32; 5];

pub fn log2(n: u32) -> u32 {
    let mut n = n;
    let mut r = 0;
    while n > 1 {
        r += 1;
        n = n.wrapping_add(1) >> 1;
    }
    r
}

fn lowmask(n: i32) -> u32 {
    if n == 32 { 0xffff_ffff } else { 1u32.wrapping_shl(n as u32).wrapping_sub(1) }
}

pub fn bits(v: u32, hi: i32, lo: i32) -> u32 {
    if hi < lo {
        return 0;
    }
    let n = hi.wrapping_sub(lo).wrapping_add(1);
    lowmask(n) & v.wrapping_shr(lo as u32)
}

pub fn cut(v: u32, hi: i32, lo: i32) -> u32 {
    if hi < lo {
        return v;
    }
    let low = if lo.wrapping_sub(1) < 0 { 0 } else { lowmask(lo) & v };
    let h1 = hi.wrapping_add(1);
    let up = if h1 > 0x1f { 0 } else { v.wrapping_shr(h1 as u32) & lowmask(32 - h1) };
    low | up.wrapping_shl(lo as u32)
}

pub fn insert(v: u32, ins: u32, hi: i32, lo: i32) -> u32 {
    if hi < lo {
        return v;
    }
    let low = if lo.wrapping_sub(1) < 0 { 0 } else { lowmask(lo) & v };
    let w = hi.wrapping_sub(lo);
    let mid = if w < 0 { 0 } else { lowmask(w.wrapping_add(1)) & ins };
    let up = if lo > 0x1f { 0 } else { v.wrapping_shr(lo as u32) & lowmask(32 - lo) };
    up.wrapping_shl(hi.wrapping_add(1) as u32) | mid.wrapping_shl(lo as u32) | low
}

pub fn bit_copy(dst: &mut u32, dbit: u32, src: u32, sbit: u32) {
    let m = 1u32 << (dbit & 31);
    if (src >> (sbit & 31)) & 1 != 0 {
        *dst |= m;
    } else {
        *dst &= !m;
    }
}

pub fn planes(d: &Dcc) -> u32 {
    match d.r32(0x48).wrapping_sub(1) {
        0..=10 => 4,
        11..=14 => 2,
        _ => d.r32(8),
    }
}

pub fn is_swz(d: &Dcc) -> bool {
    d.r32(0).wrapping_sub(0xf) <= 3
}

fn div(a: u32, b: u32) -> Result<u32, i32> {
    a.checked_div(b).ok_or(E_INVALIDARG)
}

fn rem(a: u32, b: u32) -> Result<u32, i32> {
    a.checked_rem(b).ok_or(E_INVALIDARG)
}

fn swz_index(d: &Dcc) -> u32 {
    let s = d.r32(0);
    match s {
        0xf..=0x12 => s - 0xf,
        _ => s,
    }
}

fn swz_moves(i: u32, dim: u32, bpe: u32) -> &'static [(u8, u8, u8)] {
    match (i, dim, bpe) {
        (0, 0, 1) | (0, 1, 1) | (0, 2, 1) => &[(0, 0, 0), (1, 0, 1), (0, 1, 2), (1, 1, 3), (0, 2, 4), (1, 2, 5), (0, 3, 6), (1, 4, 7)],
        (0, 0, 2) | (0, 1, 2) | (0, 2, 2) => &[(0, 0, 0), (1, 0, 1), (0, 1, 2), (1, 1, 3), (0, 2, 4), (1, 2, 5), (0, 3, 6)],
        (0, 0, 4) | (0, 1, 4) | (0, 2, 4) => &[(0, 0, 0), (1, 0, 1), (0, 1, 2), (1, 1, 3), (0, 2, 4), (1, 2, 5)],
        (0, 0, 8) | (0, 1, 8) | (0, 2, 8) => &[(0, 0, 0), (1, 0, 1), (0, 1, 2), (1, 1, 3), (0, 2, 4)],
        (0, 0, 16) | (0, 1, 16) | (0, 2, 16) | (2, 1, 16) | (3, 0, 16) | (3, 1, 16) | (3, 2, 16) => &[(0, 0, 0), (1, 0, 1), (0, 1, 2), (1, 1, 3)],
        (1, 1, 1) => &[(0, 0, 0), (0, 1, 1), (0, 2, 2), (0, 3, 3), (1, 0, 4), (1, 1, 5), (1, 2, 6), (1, 3, 7)],
        (1, 1, 2) | (2, 1, 2) | (3, 0, 2) | (3, 1, 2) | (3, 2, 2) => &[(0, 0, 0), (0, 1, 1), (0, 2, 2), (1, 0, 3), (1, 1, 4), (1, 2, 5), (0, 3, 6)],
        (1, 1, 4) | (2, 1, 4) | (3, 0, 4) | (3, 1, 4) | (3, 2, 4) => &[(0, 0, 0), (0, 1, 1), (1, 0, 2), (1, 1, 3), (1, 2, 4), (0, 2, 5)],
        (1, 1, 8) => &[(0, 0, 0), (1, 0, 1), (1, 1, 2), (0, 1, 3), (0, 2, 4)],
        (1, 1, 16) => &[(1, 0, 0), (1, 1, 1), (0, 0, 2), (0, 1, 3)],
        (1, 2, 1) => &[(0, 0, 0), (0, 1, 1), (2, 0, 2), (1, 0, 3), (2, 1, 4), (1, 1, 5), (0, 2, 6), (2, 2, 7)],
        (1, 2, 2) => &[(0, 0, 0), (2, 0, 1), (1, 0, 2), (2, 1, 3), (1, 1, 4), (0, 1, 5), (2, 2, 6)],
        (1, 2, 4) => &[(0, 0, 0), (1, 0, 1), (2, 0, 2), (1, 1, 3), (0, 1, 4), (2, 1, 5)],
        (1, 2, 8) => &[(0, 0, 0), (2, 0, 1), (1, 0, 2), (0, 1, 3), (2, 1, 4)],
        (1, 2, 16) => &[(2, 0, 0), (1, 0, 1), (0, 0, 2), (2, 1, 3)],
        (2, 1, 1) => &[(0, 0, 0), (0, 1, 1), (0, 2, 2), (1, 1, 3), (1, 0, 4), (1, 2, 5), (0, 3, 6), (1, 3, 7)],
        (2, 1, 8) | (3, 0, 8) | (3, 1, 8) | (3, 2, 8) => &[(0, 0, 0), (1, 0, 1), (0, 1, 2), (0, 2, 3), (1, 1, 4)],
        (2, 2, 1) => &[(0, 0, 0), (0, 1, 1), (2, 0, 2), (1, 0, 3), (1, 1, 4), (2, 1, 5), (0, 2, 6), (2, 2, 7)],
        (2, 2, 2) => &[(0, 0, 0), (2, 0, 1), (1, 0, 2), (0, 1, 3), (2, 1, 4), (1, 1, 5), (2, 2, 6)],
        (2, 2, 4) => &[(0, 0, 0), (1, 0, 1), (0, 1, 2), (2, 0, 3), (1, 1, 4), (2, 1, 5)],
        (2, 2, 8) => &[(0, 0, 0), (1, 0, 1), (2, 0, 2), (0, 1, 3), (2, 1, 4)],
        (2, 2, 16) => &[(0, 0, 0), (2, 0, 1), (1, 0, 2), (2, 1, 3)],
        (3, 0, 1) | (3, 1, 1) | (3, 2, 1) => &[(0, 0, 0), (0, 1, 1), (0, 2, 2), (1, 1, 3), (1, 0, 4), (1, 2, 5), (0, 3, 6), (1, 4, 7)],
        _ => &[],
    }
}

fn swz_tail(i: u32, dim: u32, bpe: u32) -> &'static [(u8, u8, u8)] {
    match (i, dim, bpe) {
        (2, 1, 1) | (3, 0, 1) | (3, 1, 1) | (3, 2, 1) => &[(0, 0, 0), (1, 0, 1), (0, 1, 2), (1, 1, 3), (0, 2, 4)],
        (2, 1, 2) | (3, 0, 2) | (3, 1, 2) | (3, 2, 2) => &[(0, 0, 0), (1, 0, 1), (0, 1, 2), (0, 2, 3)],
        (2, 1, 4) | (3, 0, 4) | (3, 1, 4) | (3, 2, 4) => &[(0, 0, 0), (1, 0, 1), (0, 1, 2)],
        _ => &[],
    }
}

const LIN_MOVES: [(u8, u8, u8); 8] = [(0, 0, 0), (1, 0, 1), (0, 1, 2), (0, 2, 3), (0, 3, 4), (0, 4, 5), (0, 5, 6), (0, 6, 7)];

fn pre_perm(mode: u32, bpe: u32) -> &'static [u8] {
    match (mode, bpe) {
        (8, 1) | (8, 2) | (8, 4) | (8, 8) => &[4, 5, 0, 1, 2, 3],
        (9, 1) | (9, 2) => &[0, 5, 1, 3, 2, 4],
        (9, 4) | (9, 8) => &[0, 5, 1, 2, 3, 4],
        (10, 1) => &[5, 0, 3, 1, 4, 2],
        (10, 2) | (10, 4) => &[5, 0, 2, 1, 3, 4],
        (10, 8) => &[5, 0, 1, 2, 3, 4],
        _ => &[],
    }
}

fn post_perm(mode: u32, bpe: u32) -> &'static [u8] {
    match (mode, bpe) {
        (8, 1) | (8, 2) | (8, 4) | (8, 8) => &[2, 3, 4, 5, 0, 1],
        (9, 1) | (9, 2) => &[0, 2, 4, 3, 5, 1],
        (9, 4) | (9, 8) => &[0, 2, 3, 4, 5, 1],
        (10, 1) => &[1, 3, 5, 2, 4, 0],
        (10, 2) | (10, 4) => &[1, 3, 2, 4, 5, 0],
        (10, 8) => &[1, 2, 3, 4, 5, 0],
        _ => &[],
    }
}

fn scatter(src: u32, perm: &[u8]) -> u32 {
    let mut v = 0u32;
    for (dbit, &sbit) in perm.iter().enumerate() {
        bit_copy(&mut v, dbit as u32, src, sbit as u32);
    }
    v
}

fn coord_from(a: u32, moves: &[(u8, u8, u8)], xyz: &mut [u32; 3]) {
    for &(f, fb, ab) in moves {
        bit_copy(&mut xyz[f as usize], fb as u32, a, ab as u32);
    }
}

fn addr_from(xyz: &[u32; 3], moves: &[(u8, u8, u8)], a: &mut u32) {
    for &(f, fb, ab) in moves {
        bit_copy(a, ab as u32, xyz[f as usize], fb as u32);
    }
}

pub fn get_swz(d: &Dcc, addr: u32, mode: u32) -> Result<Coord, i32> {
    let i = swz_index(d);
    let bpe = d.r32(4);
    let mut out = [0u32; 5];
    let mut f77 = false;
    let mut f67 = false;
    let mut r8 = 0x20u32;
    let (r13, r15);
    if mode <= 1 {
        r13 = bpe;
        r15 = 1;
    } else {
        r15 = planes(d);
        r13 = div(bpe, r15)?;
        if mode == 2 {
            f77 = true;
        } else if mode.wrapping_sub(3) <= 1 {
            f77 = true;
            f67 = true;
            if mode == 4 {
                r8 = 0x40;
            }
        }
    }
    let mut ebx = log2(r8) as i32;
    let edx = ebx - 1;
    let mut r12 = addr;
    let mut esi = 0u32;
    let mut r14 = 0u32;
    if f67 {
        let lg = log2(r13) as i32;
        let lo = ebx - lg;
        r14 = bits(r12, edx, lo);
        r12 = cut(r12, edx, lo);
        ebx -= lg;
    }
    if f77 {
        let lg = log2(r15) as i32;
        let hi = ebx - 1;
        let lo = ebx - lg;
        esi = bits(r12, hi, lo);
        ebx = lo;
        r12 = cut(r12, hi, ebx);
    }
    if !f67 {
        let lg = log2(r13) as i32;
        r14 = bits(r12, lg - 1, 0);
        r12 = cut(r12, lg - 1, 0);
    }
    if !f77 {
        let lg = log2(r15) as i32;
        esi = bits(r12, lg - 1, 0);
        r12 = cut(r12, lg - 1, 0);
    }
    out[3] = esi.wrapping_mul(r13).wrapping_add(r14);
    let dim = d.r32(0x14);
    let mut xyz = [0u32; 3];
    coord_from(r12, swz_moves(i, dim, bpe), &mut xyz);
    if mode != 0 {
        coord_from(r12, swz_tail(i, dim, bpe), &mut xyz);
    }
    out[0] = xyz[0];
    out[1] = xyz[1];
    out[2] = xyz[2];
    Ok(out)
}

pub fn get(d: &Dcc, addr: u32, mode: u32) -> Result<Coord, i32> {
    if is_swz(d) {
        return get_swz(d, addr, mode);
    }
    let bpe = d.r32(4);
    let mut esi = addr;
    let mut r15 = false;
    let mut f7f = false;
    let (mut pl, mut q) = (1u32, bpe);
    if mode > 1 {
        pl = planes(d);
        q = div(bpe, pl)?;
        r15 = matches!(mode, 2 | 3 | 4 | 8 | 9 | 10);
        f7f = r15 && mode != 2;
    }
    let width = if matches!(mode, 4 | 8 | 9 | 10) { 0x40 } else { 0x20 };
    let r13 = log2(width) as i32;
    let mut r12 = r13;
    let hi = r13 - 1;
    if matches!(mode, 8 | 9 | 10) {
        let src = bits(esi, hi, 0);
        let v = scatter(src, pre_perm(mode, bpe));
        esi = (esi & !0x3f) | (v & 0x3f);
    }
    let mut v5f = 0u32;
    if f7f {
        let lo = r13 - log2(q) as i32;
        v5f = bits(esi, hi, lo);
        esi = cut(esi, hi, lo);
        r12 = lo;
    }
    let mut r14 = 0u32;
    if r15 {
        let h = r12 - 1;
        let lo = r12 - log2(pl) as i32;
        r14 = bits(esi, h, lo);
        r12 = lo;
        esi = cut(esi, h, r12);
    }
    let low;
    if !f7f {
        let lg = log2(q) as i32;
        low = bits(esi, lg - 1, 0);
        esi = cut(esi, lg - 1, 0);
    } else {
        low = v5f;
    }
    if !r15 {
        let lg = log2(pl) as i32;
        r14 = bits(esi, lg - 1, 0);
        esi = cut(esi, lg - 1, 0);
    }
    let plane = r14.wrapping_mul(q).wrapping_add(low);
    let mut xyz = [0u32; 3];
    if d.r32(0) == 0x13 {
        coord_from(esi, &LIN_MOVES, &mut xyz);
    }
    Ok([xyz[0], xyz[1], xyz[2], plane, 0])
}

pub fn set_swz(d: &Dcc, c: &Coord, mode: u32) -> Result<u32, i32> {
    let i = swz_index(d);
    let bpe = d.r32(4);
    let dim = d.r32(0x14);
    let xyz = [c[0], c[1], c[2]];
    let mut r14 = 0u32;
    addr_from(&xyz, swz_moves(i, dim, bpe), &mut r14);
    if mode != 0 {
        addr_from(&xyz, swz_tail(i, dim, bpe), &mut r14);
    }
    let (edi, esi, r12, r15);
    if mode <= 1 {
        edi = 1;
        esi = bpe;
        r12 = false;
        r15 = false;
    } else {
        edi = planes(d);
        esi = div(bpe, edi)?;
        r12 = matches!(mode, 2 | 3 | 4);
        r15 = matches!(mode, 3 | 4);
    }
    let width = if mode == 4 { 0x40 } else { 0x20 };
    let mut ebx = log2(width) as i32;
    let qrem = rem(c[3], esi)?;
    let prem = div(c[3], esi)? % edi;
    let lp = log2(edi) as i32;
    let lq = log2(esi) as i32;
    ebx -= lp;
    ebx -= lq;
    if !r12 {
        ebx += lp;
        r14 = insert(r14, prem, lp - 1, 0);
    }
    if !r15 {
        ebx += lq;
        r14 = insert(r14, qrem, lq - 1, 0);
    }
    if r12 {
        ebx += lp;
        r14 = insert(r14, prem, ebx - 1, ebx - lp);
    }
    if r15 {
        ebx += lq;
        r14 = insert(r14, qrem, ebx - 1, ebx - lq);
    }
    Ok(r14)
}

pub fn set(d: &Dcc, c: &Coord, mode: u32) -> Result<u32, i32> {
    if is_swz(d) {
        return set_swz(d, c, mode);
    }
    let bpe = d.r32(4);
    let mut esi = 0u32;
    if d.r32(0) == 0x13 {
        addr_from(&[c[0], c[1], c[2]], &LIN_MOVES, &mut esi);
    }
    let (pl, q, r14b, f48);
    if mode <= 1 {
        pl = 1;
        q = bpe;
        r14b = false;
        f48 = false;
    } else {
        pl = planes(d);
        q = div(bpe, pl)?;
        r14b = matches!(mode, 2 | 3 | 4 | 8 | 9 | 10);
        f48 = r14b && mode != 2;
    }
    let width = if matches!(mode, 4 | 8 | 9 | 10) { 0x40 } else { 0x20 };
    let l = log2(width) as i32;
    let qrem = rem(c[3], q)?;
    let prem = div(c[3], q)? % pl;
    let lp = log2(pl) as i32;
    let lq = log2(q) as i32;
    let mut ebx = l - lp - lq;
    if !r14b {
        ebx += lp;
        esi = insert(esi, prem, lp - 1, 0);
    }
    if !f48 {
        ebx += lq;
        esi = insert(esi, qrem, lq - 1, 0);
    }
    if r14b {
        ebx += lp;
        esi = insert(esi, prem, ebx - 1, ebx - lp);
    }
    if f48 {
        ebx += lq;
        esi = insert(esi, qrem, ebx - 1, ebx - lq);
    }
    if matches!(mode, 8 | 9 | 10) {
        let src = bits(esi, l - 1, 0);
        let v = scatter(src, post_perm(mode, bpe));
        esi = (esi & !0x3f) | (v & 0x3f);
    }
    Ok(esi)
}

pub fn neighbor(d: &Dcc, addr: u32, mode: u32) -> Result<u32, i32> {
    let c = get(d, addr, mode)?;
    let mut cand = [addr; 3];
    for k in 0..3 {
        if c[k] != 0 {
            let mut n = c;
            n[k] -= 1;
            cand[k] = set(d, &n, mode)?;
        }
    }
    let mut eax = if cand[0] < addr { cand[0] } else { addr };
    for &v in &cand[1..] {
        if v < addr {
            eax = if eax == addr { v } else { v.max(eax) };
        }
    }
    Ok(eax.min(addr))
}

pub fn relayout(d: &mut Dcc, dst: usize, src: usize, mode: u32) -> Result<(), i32> {
    let from = d.r32(0xf0);
    let s = d.b[src];
    let mut tmp = [0u8; 64];
    for k in 0..4u32 {
        if (d.r32(0x5c) >> k) & 1 == 0 {
            continue;
        }
        for a in k * 64..k * 64 + 64 {
            let c = get(d, a, from)?;
            let b = set(d, &c, mode)?;
            tmp[(b & 0x3f) as usize] = s[a as usize];
        }
        let o = (k * 64) as usize;
        d.b[dst][o..o + 64].copy_from_slice(&tmp);
    }
    d.w32(0xf0, mode);
    Ok(())
}

pub fn convert(d: &mut Dcc, dst: usize, src: usize, gm: &[u32; 4], sm: &[u32; 4]) -> Result<(), i32> {
    let s = d.b[src];
    let mut tmp = [0u8; 64];
    for k in 0..4usize {
        if (d.r32(0x5c) >> k) & 1 == 0 {
            continue;
        }
        let o = k * 64;
        if gm[k] == sm[k] {
            d.b[dst][o..o + 64].copy_from_slice(&s[o..o + 64]);
            continue;
        }
        for a in o..o + 64 {
            let c = get(d, a as u32, gm[k])?;
            let b = set(d, &c, sm[k])?;
            tmp[(b & 0x3f) as usize] = s[a];
        }
        d.b[dst][o..o + 64].copy_from_slice(&tmp);
    }
    Ok(())
}
