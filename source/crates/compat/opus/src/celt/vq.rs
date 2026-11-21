use super::cwrs::decode_pulses;
use super::mathops::{celt_cos_norm, celt_rsqrt, celt_udiv, inner_prod};
use crate::entdec::Decoder;

pub const SPREAD_NONE: i32 = 0;
pub const SPREAD_LIGHT: i32 = 1;
pub const SPREAD_NORMAL: i32 = 2;
pub const SPREAD_AGGRESSIVE: i32 = 3;
const EPSILON: f32 = 1e-15;

fn exp_rotation1(x: &mut [f32], len: usize, stride: usize, c: f32, s: f32) {
    if len <= stride {
        return;
    }
    let ms = -s;
    for i in 0..len - stride {
        let x1 = x[i];
        let x2 = x[i + stride];
        x[i + stride] = c * x2 + s * x1;
        x[i] = c * x1 + ms * x2;
    }
    if len >= 2 * stride + 1 {
        let mut i = (len - 2 * stride - 1) as isize;
        while i >= 0 {
            let iu = i as usize;
            let x1 = x[iu];
            let x2 = x[iu + stride];
            x[iu + stride] = c * x2 + s * x1;
            x[iu] = c * x1 + ms * x2;
            i -= 1;
        }
    }
}

pub fn exp_rotation(x: &mut [f32], len: i32, dir: i32, stride: i32, k: i32, spread: i32) {
    const SPREAD_FACTOR: [i32; 3] = [15, 10, 5];
    if 2 * k >= len || spread == SPREAD_NONE || !(1..=3).contains(&spread) || stride <= 0 {
        return;
    }
    let factor = SPREAD_FACTOR[(spread - 1) as usize];
    let gain = (1.0f32 * len as f32) / ((len + factor * k) as f32);
    let theta = 0.5f32 * (gain * gain);
    let c = celt_cos_norm(theta);
    let s = celt_cos_norm(1.0f32 - theta);
    let mut stride2 = 0i32;
    if len >= 8 * stride {
        stride2 = 1;
        while (stride2 * stride2 + stride2) * stride + (stride >> 2) < len {
            stride2 += 1;
        }
    }
    let l = celt_udiv(len, stride) as usize;
    for i in 0..stride as usize {
        let end = ((i + 1) * l).min(x.len());
        let seg = &mut x[(i * l).min(end)..end];
        if dir < 0 {
            if stride2 != 0 {
                exp_rotation1(seg, l.min(seg.len()), stride2 as usize, s, c);
            }
            exp_rotation1(seg, l.min(seg.len()), 1, c, s);
        } else {
            exp_rotation1(seg, l.min(seg.len()), 1, c, -s);
            if stride2 != 0 {
                exp_rotation1(seg, l.min(seg.len()), stride2 as usize, s, -c);
            }
        }
    }
}

fn normalise_residual(iy: &[i32], x: &mut [f32], n: usize, ryy: f32, gain: f32) {
    let t = ryy;
    let g = celt_rsqrt(t) * gain;
    for i in 0..n {
        x[i] = (iy[i] as f32) * g;
    }
}

fn extract_collapse_mask(iy: &[i32], n: i32, b: i32) -> u32 {
    if b <= 1 {
        return 1;
    }
    let n0 = celt_udiv(n, b) as usize;
    let mut mask = 0u32;
    for i in 0..b as usize {
        let mut tmp = 0i32;
        for j in 0..n0 {
            tmp |= iy.get(i * n0 + j).copied().unwrap_or(0);
        }
        mask |= ((tmp != 0) as u32) << i;
    }
    mask
}

pub fn alg_unquant(x: &mut [f32], n: i32, k: i32, spread: i32, b: i32, dec: &mut Decoder, gain: f32) -> u32 {
    let nu = n as usize;
    let mut iy = [0i32; 176];
    let iy = &mut iy[..nu.min(176)];
    let ryy = decode_pulses(iy, n, k, dec);
    normalise_residual(iy, x, nu.min(176), ryy, gain);
    exp_rotation(x, n, -1, b, k, spread);
    extract_collapse_mask(iy, n, b)
}

pub fn renormalise_vector(x: &mut [f32], n: usize, gain: f32) {
    let e = EPSILON + inner_prod(x, x, n);
    let t = e;
    let g = celt_rsqrt(t) * gain;
    for v in x.iter_mut().take(n) {
        *v *= g;
    }
}
