use crate::entdec::ec_ilog;

pub const PI: f64 = 3.1415926535897931;

#[inline]
pub fn frac_mul16(a: i32, b: i32) -> i32 {
    (16384i32.wrapping_add((a as i16 as i32).wrapping_mul(b as i16 as i32))) >> 15
}

pub fn isqrt32(mut val: u32) -> u32 {
    if val == 0 {
        return 0;
    }
    let mut g: u32 = 0;
    let mut bshift: i32 = (ec_ilog(val) - 1) >> 1;
    let mut b: u32 = 1u32 << bshift;
    loop {
        let t = ((g << 1).wrapping_add(b)).wrapping_shl(bshift as u32);
        if t <= val {
            g = g.wrapping_add(b);
            val = val.wrapping_sub(t);
        }
        b >>= 1;
        bshift -= 1;
        if bshift < 0 {
            break;
        }
    }
    g
}

pub fn bitexact_cos(x: i16) -> i16 {
    let tmp = (4096i32 + (x as i32) * (x as i32)) >> 13;
    let x2 = tmp as i16 as i32;
    let inner = 8277 + frac_mul16(-626, x2);
    let mid = -7651 + frac_mul16(x2, inner);
    let x2 = ((32767 - x2) + frac_mul16(x2, mid)) as i16 as i32;
    (1 + x2) as i16
}

pub fn bitexact_log2tan(isin: i32, icos: i32) -> i32 {
    let lc = ec_ilog(icos as u32);
    let ls = ec_ilog(isin as u32);
    let icos = icos.wrapping_shl((15 - lc).max(0) as u32);
    let isin = isin.wrapping_shl((15 - ls).max(0) as u32);
    (ls - lc) * (1 << 11) + frac_mul16(isin, frac_mul16(isin, -2597) + 7932)
        - frac_mul16(icos, frac_mul16(icos, -2597) + 7932)
}

#[inline]
pub fn celt_lcg_rand(seed: u32) -> u32 {
    seed.wrapping_mul(1664525).wrapping_add(1013904223)
}

#[inline]
pub fn celt_exp2(x: f32) -> f32 {
    (0.6931471805599453094f64 * x as f64).exp() as f32
}

#[inline]
pub fn celt_sqrt(x: f32) -> f32 {
    (x as f64).sqrt() as f32
}

#[inline]
pub fn celt_rsqrt(x: f32) -> f32 {
    1.0f32 / celt_sqrt(x)
}

#[inline]
pub fn celt_cos_norm(x: f32) -> f32 {
    ((0.5f64 * PI) * x as f64).cos() as f32
}

#[inline]
pub fn celt_udiv(n: i32, d: i32) -> i32 {
    let d = d as u32;
    if d == 0 {
        return 0;
    }
    ((n as u32) / d) as i32
}

#[inline]
pub fn celt_sudiv(n: i32, d: i32) -> i32 {
    if d == 0 {
        return 0;
    }
    n.wrapping_div(d)
}

#[inline]
pub fn fmax(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

#[inline]
pub fn fmin(a: f32, b: f32) -> f32 {
    if a < b {
        a
    } else {
        b
    }
}

#[inline]
pub fn inner_prod(x: &[f32], y: &[f32], n: usize) -> f32 {
    let mut s = 0.0f32;
    for i in 0..n {
        s += x[i] * y[i];
    }
    s
}
