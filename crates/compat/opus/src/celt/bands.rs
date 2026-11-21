use super::energy::{safe_update, E_MEANS};
use super::mathops::*;
use super::rate::{bits2pulses, cache_at, cache_start, get_pulses, pulses2bits, QTHETA_OFFSET, QTHETA_OFFSET_TWOPHASE};
use super::tables::{EBANDS, LOGN400};
use super::vq::{alg_unquant, renormalise_vector, SPREAD_AGGRESSIVE};
use super::{BITRES, NB_EBANDS, SHORT_MDCT_SIZE};
use crate::entdec::Decoder;

#[inline]
fn eb(i: i32) -> i32 {
    EBANDS.get(i.max(0) as usize).copied().unwrap_or(100) as i32
}

pub fn denormalise_bands(
    x: &[f32],
    freq: &mut [f32],
    band_log_e: &[f32],
    start: i32,
    end: i32,
    m: i32,
    downsample: i32,
    silence: bool,
) {
    let n = (m * SHORT_MDCT_SIZE as i32) as usize;
    let mut bound = (m * eb(end)) as usize;
    if downsample != 1 && downsample > 0 {
        bound = bound.min(n / downsample as usize);
    }
    let (mut start, mut end) = (start, end);
    if silence {
        bound = 0;
        start = 0;
        end = 0;
    }
    if start != 0 {
        let s = ((m * eb(start)) as usize).min(n);
        for v in freq[..s].iter_mut() {
            *v = 0.0;
        }
    }
    for i in start..end {
        let j0 = (m * eb(i)) as usize;
        let j1 = ((m * eb(i + 1)) as usize).min(n);
        let lg = band_log_e[i as usize] + E_MEANS[i as usize];
        let g = celt_exp2(fmin(32.0, lg));
        for j in j0..j1 {
            freq[j] = x[j] * g;
        }
    }
    let bound = bound.min(n);
    for v in freq[bound..n].iter_mut() {
        *v = 0.0;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn anti_collapse(
    x_: &mut [f32],
    collapse_masks: &[u8],
    lm: i32,
    c: usize,
    size: usize,
    start: i32,
    end: i32,
    log_e: &[f32],
    prev1log_e: &[f32],
    prev2log_e: &[f32],
    pulses: &[i32],
    mut seed: u32,
) {
    for i in start..end {
        let iu = i as usize;
        let n0 = eb(i + 1) - eb(i);
        let depth = celt_udiv(1 + pulses[iu], n0) >> lm;
        let thresh = 0.5f32 * celt_exp2(-0.125f32 * depth as f32);
        let sqrt_1 = celt_rsqrt((n0 << lm) as f32);
        for ch in 0..c {
            let mut prev1 = prev1log_e[ch * NB_EBANDS + iu];
            let mut prev2 = prev2log_e[ch * NB_EBANDS + iu];
            if c == 1 {
                prev1 = fmax(prev1, prev1log_e[NB_EBANDS + iu]);
                prev2 = fmax(prev2, prev2log_e[NB_EBANDS + iu]);
            }
            let mut ediff = log_e[ch * NB_EBANDS + iu] - fmin(prev1, prev2);
            ediff = fmax(0.0, ediff);
            let mut r = 2.0f32 * celt_exp2(-ediff);
            if lm == 3 {
                r *= 1.41421356f32;
            }
            r = fmin(thresh, r);
            r *= sqrt_1;
            let off = ch * size + ((eb(i) << lm) as usize);
            let blen = (n0 << lm) as usize;
            if off + blen > x_.len() {
                continue;
            }
            let xs = &mut x_[off..off + blen];
            let mut renormalize = false;
            for k in 0..(1usize << lm) {
                if (collapse_masks[iu * c + ch] as u32) & (1u32 << k) == 0 {
                    for j in 0..n0 as usize {
                        seed = celt_lcg_rand(seed);
                        xs[(j << lm) + k] = if seed & 0x8000 != 0 { r } else { -r };
                    }
                    renormalize = true;
                }
            }
            if renormalize {
                renormalise_vector(xs, blen, 1.0);
            }
        }
    }
}

static ORDERY_TABLE: [usize; 30] = [
    1, 0, 3, 0, 2, 1, 7, 0, 4, 3, 6, 1, 5, 2, 15, 0, 8, 7, 12, 3, 11, 4, 14, 1, 9, 6, 13, 2, 10, 5,
];

fn ordery(stride: usize, i: usize) -> usize {
    ORDERY_TABLE.get(stride.wrapping_sub(2).wrapping_add(i)).copied().unwrap_or(i)
}

fn deinterleave_hadamard(x: &mut [f32], n0: usize, stride: usize, hadamard: bool) {
    let n = n0 * stride;
    if n > x.len() || n > 176 {
        return;
    }
    let mut tmp = [0.0f32; 176];
    if hadamard {
        for i in 0..stride {
            let o = ordery(stride, i);
            for j in 0..n0 {
                if o * n0 + j < n {
                    tmp[o * n0 + j] = x[j * stride + i];
                }
            }
        }
    } else {
        for i in 0..stride {
            for j in 0..n0 {
                tmp[i * n0 + j] = x[j * stride + i];
            }
        }
    }
    x[..n].copy_from_slice(&tmp[..n]);
}

fn interleave_hadamard(x: &mut [f32], n0: usize, stride: usize, hadamard: bool) {
    let n = n0 * stride;
    if n > x.len() || n > 176 {
        return;
    }
    let mut tmp = [0.0f32; 176];
    if hadamard {
        for i in 0..stride {
            let o = ordery(stride, i);
            for j in 0..n0 {
                if o * n0 + j < n {
                    tmp[j * stride + i] = x[o * n0 + j];
                }
            }
        }
    } else {
        for i in 0..stride {
            for j in 0..n0 {
                tmp[j * stride + i] = x[i * n0 + j];
            }
        }
    }
    x[..n].copy_from_slice(&tmp[..n]);
}

pub fn haar1(x: &mut [f32], n0: i32, stride: i32) {
    let n0 = (n0 >> 1) as usize;
    let stride = stride.max(0) as usize;
    for i in 0..stride {
        for j in 0..n0 {
            let a = stride * 2 * j + i;
            let b = stride * (2 * j + 1) + i;
            if b >= x.len() {
                continue;
            }
            let tmp1 = 0.70710678f32 * x[a];
            let tmp2 = 0.70710678f32 * x[b];
            x[a] = tmp1 + tmp2;
            x[b] = tmp1 - tmp2;
        }
    }
}

fn compute_qn(n: i32, b: i32, offset: i32, pulse_cap: i32, stereo: bool) -> i32 {
    const EXP2_TABLE8: [i32; 8] = [16384, 17866, 19483, 21247, 23170, 25267, 27554, 30048];
    let mut n2 = 2 * n - 1;
    if stereo && n == 2 {
        n2 -= 1;
    }
    let mut qb = celt_sudiv(b.wrapping_add(n2.wrapping_mul(offset)), n2);
    qb = qb.min(b.wrapping_sub(pulse_cap).wrapping_sub(4 << BITRES));
    qb = qb.min(8 << BITRES);
    if qb < (1 << BITRES >> 1) {
        1
    } else {
        let qn = EXP2_TABLE8[(qb & 0x7) as usize] >> (14 - (qb >> BITRES));
        (qn + 1) >> 1 << 1
    }
}

pub struct BandCtx<'a, 'b> {
    pub i: i32,
    pub intensity: i32,
    pub spread: i32,
    pub tf_change: i32,
    pub ec: &'a mut Decoder<'b>,
    pub remaining_bits: i32,
    pub seed: u32,
    pub disable_inv: bool,
    pub avoid_split_noise: bool,
}

struct SplitCtx {
    inv: bool,
    imid: i32,
    iside: i32,
    delta: i32,
    itheta: i32,
    qalloc: i32,
}

#[allow(clippy::too_many_arguments)]
fn compute_theta(ctx: &mut BandCtx, n: i32, b: &mut i32, bb: i32, b0: i32, lm: i32, stereo: bool, fill: &mut i32) -> SplitCtx {
    let i = ctx.i;
    let logn = LOGN400.get(i.max(0) as usize).copied().unwrap_or(0) as i32;
    let pulse_cap = logn + lm * (1 << BITRES);
    let offset = (pulse_cap >> 1) - if stereo && n == 2 { QTHETA_OFFSET_TWOPHASE } else { QTHETA_OFFSET };
    let mut qn = compute_qn(n, *b, offset, pulse_cap, stereo);
    if stereo && i >= ctx.intensity {
        qn = 1;
    }
    let mut itheta: i32 = 0;
    let mut inv = false;
    let tell = ctx.ec.tell_frac();
    if qn != 1 {
        if stereo && n > 2 {
            let p0: i32 = 3;
            let x0 = qn / 2;
            let ft = p0 * (x0 + 1) + x0;
            let fs = ctx.ec.decode(ft as u32) as i32;
            let x = if fs < (x0 + 1) * p0 { fs / p0 } else { x0 + 1 + (fs - (x0 + 1) * p0) };
            let fl = if x <= x0 { p0 * x } else { (x - 1 - x0) + (x0 + 1) * p0 };
            let fh = if x <= x0 { p0 * (x + 1) } else { (x - x0) + (x0 + 1) * p0 };
            safe_update(ctx.ec, fl.max(0) as u32, fh.max(0) as u32, ft as u32);
            itheta = x;
        } else if b0 > 1 || stereo {
            itheta = ctx.ec.dec_uint((qn + 1) as u32) as i32;
        } else {
            let half = qn >> 1;
            let ft = (half + 1) * (half + 1);
            let fm = ctx.ec.decode(ft as u32) as i32;
            let fs;
            let fl;
            if fm < ((half * (half + 1)) >> 1) {
                let r = isqrt32((8u32).wrapping_mul(fm as u32).wrapping_add(1));
                itheta = (r.wrapping_sub(1) >> 1) as i32;
                fs = itheta + 1;
                fl = (itheta * (itheta + 1)) >> 1;
            } else {
                let r = isqrt32((8u32).wrapping_mul((ft - fm - 1) as u32).wrapping_add(1));
                itheta =(((2 * (qn + 1)) as u32).wrapping_sub(r) >> 1) as i32;
                fs = qn + 1 - itheta;
                fl = ft - (((qn + 1 - itheta) * (qn + 2 - itheta)) >> 1);
            }
            safe_update(ctx.ec, fl.max(0) as u32, (fl + fs).max(0) as u32, ft as u32);
        }
        itheta = celt_udiv(itheta.wrapping_mul(16384), qn);
    } else if stereo {
        if *b > 2 << BITRES && ctx.remaining_bits > 2 << BITRES {
            inv = ctx.ec.bit_logp(2);
        } else {
            inv = false;
        }
        if ctx.disable_inv {
            inv = false;
        }
        itheta = 0;
    }
    let qalloc = ctx.ec.tell_frac().wrapping_sub(tell) as i32;
    *b -= qalloc;
    let (imid, iside, delta);
    if itheta == 0 {
        imid = 32767;
        iside = 0;
        *fill &= (1i32.wrapping_shl(bb as u32)).wrapping_sub(1);
        delta = -16384;
    } else if itheta == 16384 {
        imid = 0;
        iside = 32767;
        *fill &= (1i32.wrapping_shl(bb as u32)).wrapping_sub(1).wrapping_shl(bb as u32);
        delta = 16384;
    } else {
        imid = bitexact_cos(itheta as i16) as i32;
        iside = bitexact_cos((16384 - itheta) as i16) as i32;
        delta = frac_mul16((n - 1) << 7, bitexact_log2tan(iside, imid));
    }
    SplitCtx { inv, imid, iside, delta, itheta, qalloc }
}

fn quant_band_n1(ctx: &mut BandCtx, x: &mut [f32], y: Option<&mut [f32]>, lowband_out: Option<&mut [f32]>) -> u32 {
    let mut sign = 0u32;
    if ctx.remaining_bits >= 1 << BITRES {
        sign = ctx.ec.dec_bits(1);
        ctx.remaining_bits -= 1 << BITRES;
    }
    x[0] = if sign != 0 { -1.0 } else { 1.0 };
    if let Some(y) = y {
        let mut sign = 0u32;
        if ctx.remaining_bits >= 1 << BITRES {
            sign = ctx.ec.dec_bits(1);
            ctx.remaining_bits -= 1 << BITRES;
        }
        y[0] = if sign != 0 { -1.0 } else { 1.0 };
    }
    if let Some(lo) = lowband_out {
        lo[0] = x[0];
    }
    1
}

#[allow(clippy::too_many_arguments)]
fn quant_partition(
    ctx: &mut BandCtx,
    x: &mut [f32],
    n: i32,
    mut b: i32,
    mut bb: i32,
    lowband: Option<&[f32]>,
    mut lm: i32,
    gain: f32,
    mut fill: i32,
) -> u32 {
    let b0 = bb;
    let i = ctx.i;
    let mut cm: u32 = 0;
    let split = if lm != -1 && n > 2 {
        match cache_start(i, lm) {
            Some(st) => b > cache_at(st, cache_at(st, 0)) + 12,
            None => false,
        }
    } else {
        false
    };
    if split {
        let n = n >> 1;
        let nu = n as usize;
        let (xs, ys) = x.split_at_mut(nu.min(x.len()));
        lm -= 1;
        if bb == 1 {
            fill = (fill & 1) | (fill << 1);
        }
        bb = (bb + 1) >> 1;
        let sctx = compute_theta(ctx, n, &mut b, bb, b0, lm, false, &mut fill);
        let imid = sctx.imid;
        let iside = sctx.iside;
        let mut delta = sctx.delta;
        let itheta = sctx.itheta;
        let qalloc = sctx.qalloc;
        let mid = (1.0f32 / 32768.0) * imid as f32;
        let side = (1.0f32 / 32768.0) * iside as f32;
        if b0 > 1 && (itheta & 0x3fff) != 0 {
            if itheta > 8192 {
                delta -= delta >> (4 - lm).clamp(0, 31);
            } else {
                delta = 0.min(delta + ((n << BITRES) >> (5 - lm).clamp(0, 31)));
            }
        }
        let mut mbits = 0.max(b.min((b - delta) / 2));
        let mut sbits = b - mbits;
        ctx.remaining_bits -= qalloc;
        let next_lowband2 = lowband.and_then(|l| l.get(nu..));
        let mut rebalance = ctx.remaining_bits;
        if mbits >= sbits {
            cm = quant_partition(ctx, xs, n, mbits, bb, lowband, lm, gain * mid, fill);
            rebalance = mbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && itheta != 0 {
                sbits += rebalance - (3 << BITRES);
            }
            cm |= quant_partition(ctx, ys, n, sbits, bb, next_lowband2, lm, gain * side, fill >> bb.clamp(0, 31))
                .wrapping_shl((b0 >> 1) as u32);
        } else {
            cm = quant_partition(ctx, ys, n, sbits, bb, next_lowband2, lm, gain * side, fill >> bb.clamp(0, 31))
                .wrapping_shl((b0 >> 1) as u32);
            rebalance = sbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && itheta != 16384 {
                mbits += rebalance - (3 << BITRES);
            }
            cm |= quant_partition(ctx, xs, n, mbits, bb, lowband, lm, gain * mid, fill);
        }
    } else {
        let nu = (n.max(0) as usize).min(x.len());
        let mut q = bits2pulses(i, lm, b);
        let mut curr_bits = pulses2bits(i, lm, q);
        ctx.remaining_bits -= curr_bits;
        while ctx.remaining_bits < 0 && q > 0 {
            ctx.remaining_bits += curr_bits;
            q -= 1;
            curr_bits = pulses2bits(i, lm, q);
            ctx.remaining_bits -= curr_bits;
        }
        if q != 0 {
            let k = get_pulses(q);
            cm = alg_unquant(&mut x[..nu], nu as i32, k, ctx.spread, bb, ctx.ec, gain);
        } else {
            let cm_mask: u32 = (1u64.wrapping_shl(bb.clamp(0, 63) as u32) as u32).wrapping_sub(1);
            fill &= cm_mask as i32;
            if fill == 0 {
                for v in x[..nu].iter_mut() {
                    *v = 0.0;
                }
            } else {
                match lowband {
                    None => {
                        for v in x[..nu].iter_mut() {
                            ctx.seed = celt_lcg_rand(ctx.seed);
                            *v = ((ctx.seed as i32) >> 20) as f32;
                        }
                        cm = cm_mask;
                    }
                    Some(lb) => {
                        for j in 0..nu {
                            ctx.seed = celt_lcg_rand(ctx.seed);
                            let mut tmp = 1.0f32 / 256.0;
                            tmp = if ctx.seed & 0x8000 != 0 { tmp } else { -tmp };
                            x[j] = lb.get(j).copied().unwrap_or(0.0) + tmp;
                        }
                        cm = fill as u32;
                    }
                }
                renormalise_vector(&mut x[..nu], nu, gain);
            }
        }
    }
    cm
}

static BIT_INTERLEAVE_TABLE: [i32; 16] = [0, 1, 1, 1, 2, 3, 3, 3, 2, 3, 3, 3, 2, 3, 3, 3];
static BIT_DEINTERLEAVE_TABLE: [u32; 16] = [
    0x00, 0x03, 0x0C, 0x0F, 0x30, 0x33, 0x3C, 0x3F, 0xC0, 0xC3, 0xCC, 0xCF, 0xF0, 0xF3, 0xFC, 0xFF,
];

#[allow(clippy::too_many_arguments)]
fn quant_band(
    ctx: &mut BandCtx,
    x: &mut [f32],
    n: i32,
    b: i32,
    mut bb: i32,
    lowband: Option<&[f32]>,
    lm: i32,
    lowband_out: Option<&mut [f32]>,
    gain: f32,
    mut fill: i32,
) -> u32 {
    let n0 = n;
    let mut n_b = n;
    let mut b0 = bb;
    let mut time_divide = 0;
    let mut recombine = 0;
    let long_blocks = b0 == 1;
    let mut tf_change = ctx.tf_change;
    n_b = celt_udiv(n_b, bb);
    if n == 1 {
        return quant_band_n1(ctx, x, None, lowband_out);
    }
    if tf_change > 0 {
        recombine = tf_change;
    }
    let nu = (n.max(0) as usize).min(176).min(x.len());
    let mut lbuf = [0.0f32; 176];
    let has_lowband = match lowband {
        Some(l) => {
            let m = nu.min(l.len());
            lbuf[..m].copy_from_slice(&l[..m]);
            true
        }
        None => false,
    };
    for k in 0..recombine {
        if has_lowband {
            haar1(&mut lbuf[..nu], n >> k, 1 << k);
        }
        fill = BIT_INTERLEAVE_TABLE[(fill & 0xF) as usize] | BIT_INTERLEAVE_TABLE[((fill >> 4) & 0xF) as usize] << 2;
    }
    bb >>= recombine;
    n_b <<= recombine;
    while (n_b & 1) == 0 && tf_change < 0 && n_b > 0 {
        if has_lowband {
            haar1(&mut lbuf[..nu], n_b, bb);
        }
        fill |= fill.wrapping_shl(bb as u32);
        bb <<= 1;
        n_b >>= 1;
        time_divide += 1;
        tf_change += 1;
    }
    b0 = bb;
    let n_b0 = n_b;
    if b0 > 1 && has_lowband {
        deinterleave_hadamard(&mut lbuf[..nu], (n_b >> recombine) as usize, (b0 << recombine) as usize, long_blocks);
    }
    let lb = if has_lowband { Some(&lbuf[..nu]) } else { None };
    let mut cm = quant_partition(ctx, &mut x[..nu], n, b, bb, lb, lm, gain, fill);
    if b0 > 1 {
        interleave_hadamard(&mut x[..nu], (n_b >> recombine) as usize, (b0 << recombine) as usize, long_blocks);
    }
    n_b = n_b0;
    bb = b0;
    for _ in 0..time_divide {
        bb >>= 1;
        n_b <<= 1;
        cm |= cm >> bb;
        haar1(&mut x[..nu], n_b, bb);
    }
    for k in 0..recombine {
        cm = BIT_DEINTERLEAVE_TABLE.get(cm as usize).copied().unwrap_or(0);
        haar1(&mut x[..nu], n0 >> k, 1 << k);
    }
    bb <<= recombine;
    if let Some(lo) = lowband_out {
        let nn = celt_sqrt(n0 as f32);
        for j in 0..nu.min(lo.len()) {
            lo[j] = nn * x[j];
        }
    }
    cm & (1u32.wrapping_shl(bb as u32)).wrapping_sub(1)
}

fn stereo_merge(x: &mut [f32], y: &mut [f32], mid: f32, n: usize) {
    let mut xp = inner_prod(y, x, n);
    let side = inner_prod(y, y, n);
    xp *= mid;
    let el = mid * mid + side - 2.0 * xp;
    let er = mid * mid + side + 2.0 * xp;
    if er < 6e-4f32 || el < 6e-4f32 {
        y[..n].copy_from_slice(&x[..n]);
        return;
    }
    let lgain = celt_rsqrt(el);
    let rgain = celt_rsqrt(er);
    for j in 0..n {
        let l = mid * x[j];
        let r = y[j];
        x[j] = lgain * (l - r);
        y[j] = rgain * (l + r);
    }
}

#[allow(clippy::too_many_arguments)]
fn quant_band_stereo(
    ctx: &mut BandCtx,
    x: &mut [f32],
    y: &mut [f32],
    n: i32,
    mut b: i32,
    bb: i32,
    lowband: Option<&[f32]>,
    lm: i32,
    lowband_out: Option<&mut [f32]>,
    mut fill: i32,
) -> u32 {
    if n == 1 {
        return quant_band_n1(ctx, x, Some(y), lowband_out);
    }
    let nu = (n as usize).min(x.len()).min(y.len());
    let orig_fill = fill;
    let sctx = compute_theta(ctx, n, &mut b, bb, bb, lm, true, &mut fill);
    let inv = sctx.inv;
    let delta = sctx.delta;
    let itheta = sctx.itheta;
    let qalloc = sctx.qalloc;
    let mid = (1.0f32 / 32768.0) * sctx.imid as f32;
    let side = (1.0f32 / 32768.0) * sctx.iside as f32;
    let mut cm: u32;
    if n == 2 {
        let mut mbits = b;
        let mut sbits = 0;
        if itheta != 0 && itheta != 16384 {
            sbits = 1 << BITRES;
        }
        mbits -= sbits;
        let c = itheta > 8192;
        ctx.remaining_bits -= qalloc + sbits;
        let mut sign = 0i32;
        if sbits != 0 {
            sign = ctx.ec.dec_bits(1) as i32;
        }
        sign = 1 - 2 * sign;
        if c {
            cm = quant_band(ctx, y, n, mbits, bb, lowband, lm, lowband_out, 1.0, orig_fill);
            x[0] = (-sign) as f32 * y[1];
            x[1] = sign as f32 * y[0];
        } else {
            cm = quant_band(ctx, x, n, mbits, bb, lowband, lm, lowband_out, 1.0, orig_fill);
            y[0] = (-sign) as f32 * x[1];
            y[1] = sign as f32 * x[0];
        }
        x[0] *= mid;
        x[1] *= mid;
        y[0] *= side;
        y[1] *= side;
        let tmp = x[0];
        x[0] = tmp - y[0];
        y[0] = tmp + y[0];
        let tmp = x[1];
        x[1] = tmp - y[1];
        y[1] = tmp + y[1];
    } else {
        let mut mbits = 0.max(b.min((b - delta) / 2));
        let mut sbits = b - mbits;
        ctx.remaining_bits -= qalloc;
        let mut rebalance = ctx.remaining_bits;
        if mbits >= sbits {
            cm = quant_band(ctx, x, n, mbits, bb, lowband, lm, lowband_out, 1.0, fill);
            rebalance = mbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && itheta != 0 {
                sbits += rebalance - (3 << BITRES);
            }
            cm |= quant_band(ctx, y, n, sbits, bb, None, lm, None, side, fill >> bb.clamp(0, 31));
        } else {
            cm = quant_band(ctx, y, n, sbits, bb, None, lm, None, side, fill >> bb.clamp(0, 31));
            rebalance = sbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && itheta != 16384 {
                mbits += rebalance - (3 << BITRES);
            }
            cm |= quant_band(ctx, x, n, mbits, bb, lowband, lm, lowband_out, 1.0, fill);
        }
    }
    if n != 2 {
        stereo_merge(x, y, mid, nu);
    }
    if inv {
        for v in y[..nu].iter_mut() {
            *v = -*v;
        }
    }
    cm
}

fn special_hybrid_folding(norm: &mut [f32], norm2: &mut [f32], start: i32, m: i32, dual_stereo: bool) {
    let n1 = (m * (eb(start + 1) - eb(start))) as usize;
    let n2 = (m * (eb(start + 2) - eb(start + 1))) as usize;
    if n2 > n1 && 2 * n1 >= n2 && n2 <= norm.len() {
        norm.copy_within(2 * n1 - n2..n1, n1);
        if dual_stereo && n2 <= norm2.len() {
            norm2.copy_within(2 * n1 - n2..n1, n1);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn quant_all_bands(
    start: i32,
    end: i32,
    x_: &mut [f32],
    y_: Option<&mut [f32]>,
    collapse_masks: &mut [u8],
    pulses: &[i32],
    short_blocks: bool,
    spread: i32,
    mut dual_stereo: bool,
    intensity: i32,
    tf_res: &[i32],
    total_bits: i32,
    mut balance: i32,
    ec: &mut Decoder,
    lm: i32,
    coded_bands: i32,
    seed: &mut u32,
    disable_inv: bool,
) {
    let m = 1i32 << lm;
    let bb = if short_blocks { m } else { 1 };
    let norm_offset = m * eb(start);
    let c: usize = if y_.is_some() { 2 } else { 1 };
    let norm_len = (m * eb(NB_EBANDS as i32 - 1) - norm_offset).max(0) as usize;
    let mut norm_buf = vec![0.0f32; 2 * norm_len];
    let (norm, norm2) = norm_buf.split_at_mut(norm_len);
    let mut y_ = y_;
    let mut ctx = BandCtx {
        i: 0,
        intensity,
        spread,
        tf_change: 0,
        ec,
        remaining_bits: 0,
        seed: *seed,
        disable_inv,
        avoid_split_noise: bb > 1,
    };
    let mut lowband_offset: i32 = 0;
    let mut update_lowband = true;
    let mut lbx = [0.0f32; 176];
    let mut lby = [0.0f32; 176];
    for i in start..end {
        ctx.i = i;
        let last = i == end - 1;
        let xo = (m * eb(i)) as usize;
        let n = m * eb(i + 1) - m * eb(i);
        let nu = n as usize;
        if xo + nu > x_.len() {
            break;
        }
        let tell = ctx.ec.tell_frac() as i32;
        if i != start {
            balance = balance.wrapping_sub(tell);
        }
        let remaining_bits = total_bits.wrapping_sub(tell).wrapping_sub(1);
        ctx.remaining_bits = remaining_bits;
        let b = if i < coded_bands {
            let curr_balance = celt_sudiv(balance, 3.min(coded_bands - i));
            0.max(16383.min((remaining_bits.wrapping_add(1)).min(pulses[i as usize].wrapping_add(curr_balance))))
        } else {
            0
        };
        if (m * eb(i) - n >= m * eb(start) || i == start + 1) && (update_lowband || lowband_offset == 0) {
            lowband_offset = i;
        }
        if i == start + 1 {
            special_hybrid_folding(norm, norm2, start, m, dual_stereo);
        }
        let tf_change = tf_res[i as usize];
        ctx.tf_change = tf_change;
        let mut effective_lowband: i32 = -1;
        let mut x_cm: u32;
        let mut y_cm: u32;
        if lowband_offset != 0 && (spread != SPREAD_AGGRESSIVE || bb > 1 || tf_change < 0) {
            effective_lowband = 0.max(m * eb(lowband_offset) - norm_offset - n);
            let mut fold_start = lowband_offset;
            loop {
                fold_start -= 1;
                if fold_start <= 0 || m * eb(fold_start) <= effective_lowband + norm_offset {
                    break;
                }
            }
            let mut fold_end = lowband_offset - 1;
            loop {
                fold_end += 1;
                if !(fold_end < i && m * eb(fold_end) < effective_lowband + norm_offset + n) {
                    break;
                }
            }
            x_cm = 0;
            y_cm = 0;
            let mut fold_i = fold_start.max(0);
            loop {
                let fi = fold_i as usize;
                x_cm |= collapse_masks.get(fi * c).copied().unwrap_or(0) as u32;
                y_cm |= collapse_masks.get(fi * c + c - 1).copied().unwrap_or(0) as u32;
                fold_i += 1;
                if fold_i >= fold_end {
                    break;
                }
            }
        } else {
            x_cm = (1u32 << bb) - 1;
            y_cm = x_cm;
        }
        if dual_stereo && i == intensity {
            dual_stereo = false;
            let lim = ((m * eb(i) - norm_offset).max(0) as usize).min(norm_len);
            for j in 0..lim {
                norm[j] = 0.5f32 * (norm[j] + norm2[j]);
            }
        }
        let out_off = (m * eb(i) - norm_offset).max(0) as usize;
        let have_lb = effective_lowband != -1;
        let el = effective_lowband.max(0) as usize;
        let mut lbx_ok = false;
        let mut lby_ok = false;
        if have_lb {
            for j in 0..nu.min(176) {
                lbx[j] = norm.get(el + j).copied().unwrap_or(0.0);
                lby[j] = norm2.get(el + j).copied().unwrap_or(0.0);
            }
            lbx_ok = true;
            lby_ok = true;
        }
        let out_ok = !last && out_off + nu <= norm_len;
        let xs = &mut x_[xo..xo + nu];
        if dual_stereo {
            let ys = match y_.as_deref_mut() {
                Some(y) if xo + nu <= y.len() => &mut y[xo..xo + nu],
                _ => break,
            };
            let lo = if out_ok { Some(&mut norm[out_off..out_off + nu]) } else { None };
            x_cm = quant_band(
                &mut ctx,
                xs,
                n,
                b / 2,
                bb,
                if lbx_ok { Some(&lbx[..nu.min(176)]) } else { None },
                lm,
                lo,
                1.0,
                x_cm as i32,
            );
            let lo2 = if out_ok { Some(&mut norm2[out_off..out_off + nu]) } else { None };
            y_cm = quant_band(
                &mut ctx,
                ys,
                n,
                b / 2,
                bb,
                if lby_ok { Some(&lby[..nu.min(176)]) } else { None },
                lm,
                lo2,
                1.0,
                y_cm as i32,
            );
        } else {
            let lo = if out_ok { Some(&mut norm[out_off..out_off + nu]) } else { None };
            let lbs = if lbx_ok { Some(&lbx[..nu.min(176)]) } else { None };
            match y_.as_deref_mut() {
                Some(y) => {
                    if xo + nu > y.len() {
                        break;
                    }
                    let ys = &mut y[xo..xo + nu];
                    x_cm = quant_band_stereo(&mut ctx, xs, ys, n, b, bb, lbs, lm, lo, (x_cm | y_cm) as i32);
                }
                None => {
                    x_cm = quant_band(&mut ctx, xs, n, b, bb, lbs, lm, lo, 1.0, (x_cm | y_cm) as i32);
                }
            }
            y_cm = x_cm;
        }
        let iu = i as usize;
        if let Some(v) = collapse_masks.get_mut(iu * c) {
            *v = x_cm as u8;
        }
        if let Some(v) = collapse_masks.get_mut(iu * c + c - 1) {
            *v = y_cm as u8;
        }
        balance = balance.wrapping_add(pulses[iu]).wrapping_add(tell);
        update_lowband = b > (n << BITRES);
        ctx.avoid_split_noise = false;
    }
    *seed = ctx.seed;
}
