#![allow(dead_code)]

pub const I16_MAX: i32 = i16::MAX as i32;
pub const I16_MIN: i32 = i16::MIN as i32;

#[inline(always)]
pub fn smulwb(a: i32, b: i32) -> i32 {
    ((a as i64 * (b as i16) as i64) >> 16) as i32
}

#[inline(always)]
pub fn smlawb(a: i32, b: i32, c: i32) -> i32 {
    (a as i64 + ((b as i64 * (c as i16) as i64) >> 16)) as i32
}

#[inline(always)]
pub fn smulwt(a: i32, b: i32) -> i32 {
    ((a as i64 * (b >> 16) as i64) >> 16) as i32
}

#[inline(always)]
pub fn smlawt(a: i32, b: i32, c: i32) -> i32 {
    (a as i64 + ((b as i64 * ((c as i64) >> 16)) >> 16)) as i32
}

#[inline(always)]
pub fn smulbb(a: i32, b: i32) -> i32 {
    ((a as i16) as i32).wrapping_mul((b as i16) as i32)
}

#[inline(always)]
pub fn smlabb(a: i32, b: i32, c: i32) -> i32 {
    a.wrapping_add(smulbb(b, c))
}

#[inline(always)]
pub fn smlabb_ovflw(a: i32, b: i32, c: i32) -> i32 {
    a.wrapping_add(smulbb(b, c))
}

#[inline(always)]
pub fn smulbt(a: i32, b: i32) -> i32 {
    ((a as i16) as i32).wrapping_mul(b >> 16)
}

#[inline(always)]
pub fn smlabt(a: i32, b: i32, c: i32) -> i32 {
    a.wrapping_add(((b as i16) as i32).wrapping_mul(c >> 16))
}

#[inline(always)]
pub fn smultt(a: i32, b: i32) -> i32 {
    (a >> 16).wrapping_mul(b >> 16)
}

#[inline(always)]
pub fn smull(a: i32, b: i32) -> i64 {
    a as i64 * b as i64
}

#[inline(always)]
pub fn smulww(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64) >> 16) as i32
}

#[inline(always)]
pub fn smlaww(a: i32, b: i32, c: i32) -> i32 {
    (a as i64 + ((b as i64 * c as i64) >> 16)) as i32
}

#[inline(always)]
pub fn smmul(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64) >> 32) as i32
}

#[inline(always)]
pub fn add32(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

#[inline(always)]
pub fn sub32(a: i32, b: i32) -> i32 {
    a.wrapping_sub(b)
}

#[inline(always)]
pub fn mul(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

#[inline(always)]
pub fn mla(a: i32, b: i32, c: i32) -> i32 {
    a.wrapping_add(b.wrapping_mul(c))
}

#[inline(always)]
pub fn div32(a: i32, b: i32) -> i32 {
    if b == 0 {
        0
    } else {
        a.wrapping_div(b)
    }
}

#[inline(always)]
pub fn lshift(a: i32, s: i32) -> i32 {
    if s <= 0 {
        a
    } else if s >= 32 {
        0
    } else {
        ((a as u32) << s) as i32
    }
}

#[inline(always)]
pub fn lshift64(a: i64, s: i32) -> i64 {
    if s <= 0 {
        a
    } else if s >= 64 {
        0
    } else {
        ((a as u64) << s) as i64
    }
}

#[inline(always)]
pub fn rshift(a: i32, s: i32) -> i32 {
    if s <= 0 {
        a
    } else {
        a >> s.min(31)
    }
}

#[inline(always)]
pub fn rshift64(a: i64, s: i32) -> i64 {
    if s <= 0 {
        a
    } else {
        a >> s.min(63)
    }
}

#[inline(always)]
pub fn rshift_round(a: i32, s: i32) -> i32 {
    if s <= 0 {
        a
    } else if s == 1 {
        (a >> 1).wrapping_add(a & 1)
    } else {
        (rshift(a, s - 1).wrapping_add(1)) >> 1
    }
}

#[inline(always)]
pub fn rshift_round64(a: i64, s: i32) -> i64 {
    if s <= 0 {
        a
    } else if s == 1 {
        (a >> 1).wrapping_add(a & 1)
    } else {
        (rshift64(a, s - 1).wrapping_add(1)) >> 1
    }
}

#[inline(always)]
pub fn add_lshift32(a: i32, b: i32, s: i32) -> i32 {
    a.wrapping_add(lshift(b, s))
}

#[inline(always)]
pub fn add_rshift32(a: i32, b: i32, s: i32) -> i32 {
    a.wrapping_add(rshift(b, s))
}

#[inline(always)]
pub fn sub_lshift32(a: i32, b: i32, s: i32) -> i32 {
    a.wrapping_sub(lshift(b, s))
}

#[inline(always)]
pub fn sat16(a: i32) -> i32 {
    if a > I16_MAX {
        I16_MAX
    } else if a < I16_MIN {
        I16_MIN
    } else {
        a
    }
}

#[inline(always)]
pub fn add_sat16(a: i32, b: i32) -> i16 {
    sat16(a.wrapping_add(b)) as i16
}

#[inline(always)]
pub fn add_sat32(a: i32, b: i32) -> i32 {
    a.saturating_add(b)
}

#[inline(always)]
pub fn sub_sat32(a: i32, b: i32) -> i32 {
    a.saturating_sub(b)
}

#[inline(always)]
pub fn limit(a: i32, l1: i32, l2: i32) -> i32 {
    if l1 > l2 {
        if a > l1 {
            l1
        } else if a < l2 {
            l2
        } else {
            a
        }
    } else if a > l2 {
        l2
    } else if a < l1 {
        l1
    } else {
        a
    }
}

#[inline(always)]
pub fn lshift_sat32(a: i32, s: i32) -> i32 {
    lshift(limit(a, rshift(i32::MIN, s), rshift(i32::MAX, s)), s)
}

#[inline(always)]
pub fn abs32(a: i32) -> i32 {
    if a > 0 {
        a
    } else {
        a.wrapping_neg()
    }
}

#[inline(always)]
pub fn clz32(a: i32) -> i32 {
    (a as u32).leading_zeros() as i32
}

#[inline(always)]
pub fn clz16(a: i16) -> i32 {
    ((((a as u16) as u32) << 16) | 0x8000).leading_zeros() as i32
}

#[inline(always)]
pub fn ror32(a: i32, rot: i32) -> i32 {
    (a as u32).rotate_right(rot.rem_euclid(32) as u32) as i32
}

#[inline(always)]
pub fn rand(seed: i32) -> i32 {
    907633515i32.wrapping_add(seed.wrapping_mul(196314165))
}

#[inline(always)]
pub fn clz_frac(a: i32) -> (i32, i32) {
    let lz = clz32(a);
    (lz, ror32(a, 24 - lz) & 0x7f)
}

pub fn sqrt_approx(x: i32) -> i32 {
    if x <= 0 {
        return 0;
    }
    let (lz, frac_q7) = clz_frac(x);
    let mut y: i32 = if lz & 1 != 0 { 32768 } else { 46214 };
    y = rshift(y, lz >> 1);
    smlawb(y, y, smulbb(213, frac_q7))
}

pub fn div32_varq(a32: i32, b32: i32, qres: i32) -> i32 {
    let a_headrm = clz32(abs32(a32)) - 1;
    let mut a32_nrm = lshift(a32, a_headrm);
    let b_headrm = clz32(abs32(b32)) - 1;
    let b32_nrm = lshift(b32, b_headrm);
    let b32_inv = div32(i32::MAX >> 2, b32_nrm >> 16);
    let mut result = smulwb(a32_nrm, b32_inv);
    a32_nrm = a32_nrm.wrapping_sub(lshift(smmul(b32_nrm, result), 3));
    result = smlawb(result, a32_nrm, b32_inv);
    let ls = 29 + a_headrm - b_headrm - qres;
    if ls < 0 {
        lshift_sat32(result, -ls)
    } else if ls < 32 {
        rshift(result, ls)
    } else {
        0
    }
}

pub fn inverse32_varq(b32: i32, qres: i32) -> i32 {
    let b_headrm = clz32(abs32(b32)) - 1;
    let b32_nrm = lshift(b32, b_headrm);
    let b32_inv = div32(i32::MAX >> 2, b32_nrm >> 16);
    let mut result = lshift(b32_inv, 16);
    let err_q32 = lshift((1i32 << 29).wrapping_sub(smulwb(b32_nrm, b32_inv)), 3);
    result = smlaww(result, err_q32, b32_inv);
    let ls = 61 - b_headrm - qres;
    if ls <= 0 {
        lshift_sat32(result, -ls)
    } else if ls < 32 {
        rshift(result, ls)
    } else {
        0
    }
}
