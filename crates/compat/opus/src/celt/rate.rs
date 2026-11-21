use super::mathops::celt_udiv;
use super::tables::*;
use super::{BITRES, NB_EBANDS};
use crate::entdec::Decoder;

pub const MAX_FINE_BITS: i32 = 8;
pub const FINE_OFFSET: i32 = 21;
pub const QTHETA_OFFSET: i32 = 4;
pub const QTHETA_OFFSET_TWOPHASE: i32 = 16;
const LOG_MAX_PSEUDO: i32 = 6;
const ALLOC_STEPS: i32 = 6;
const NB_ALLOC_VECTORS: i32 = 11;

static LOG2_FRAC_TABLE: [u8; 24] = [
    0, 8, 13, 16, 19, 21, 23, 24, 26, 27, 28, 29, 30, 31, 32, 32, 33, 34, 34, 35, 36, 36, 37, 37,
];

#[inline]
fn eb(i: i32) -> i32 {
    EBANDS.get(i.max(0) as usize).copied().unwrap_or(100) as i32
}

#[inline]
fn logn(i: i32) -> i32 {
    LOGN400.get(i.max(0) as usize).copied().unwrap_or(0) as i32
}

#[inline]
pub fn get_pulses(i: i32) -> i32 {
    if i < 8 {
        i
    } else {
        (8 + (i & 7)) << ((i >> 3) - 1)
    }
}

#[inline]
pub fn cache_start(band: i32, lm: i32) -> Option<usize> {
    let idx = (lm + 1) * NB_EBANDS as i32 + band;
    if idx < 0 {
        return None;
    }
    let v = *CACHE_INDEX50.get(idx as usize)?;
    if v < 0 {
        None
    } else {
        Some(v as usize)
    }
}

#[inline]
pub fn cache_at(start: usize, i: i32) -> i32 {
    if i < 0 {
        return 0;
    }
    CACHE_BITS50.get(start + i as usize).copied().unwrap_or(0) as i32
}

pub fn bits2pulses(band: i32, lm: i32, bits: i32) -> i32 {
    let st = match cache_start(band, lm) {
        Some(s) => s,
        None => return 0,
    };
    let mut lo = 0i32;
    let mut hi = cache_at(st, 0);
    let bits = bits - 1;
    for _ in 0..LOG_MAX_PSEUDO {
        let mid = (lo + hi + 1) >> 1;
        if cache_at(st, mid) >= bits {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let lov = if lo == 0 { -1 } else { cache_at(st, lo) };
    if bits - lov <= cache_at(st, hi) - bits {
        lo
    } else {
        hi
    }
}

pub fn pulses2bits(band: i32, lm: i32, pulses: i32) -> i32 {
    let st = match cache_start(band, lm) {
        Some(s) => s,
        None => return 0,
    };
    if pulses == 0 {
        0
    } else {
        cache_at(st, pulses) + 1
    }
}

pub fn init_caps(cap: &mut [i32], lm: i32, c: i32) {
    for i in 0..NB_EBANDS {
        let n = (eb(i as i32 + 1) - eb(i as i32)) << lm;
        let idx = NB_EBANDS as i32 * (2 * lm + c - 1) + i as i32;
        let cc = CACHE_CAPS50.get(idx.max(0) as usize).copied().unwrap_or(0) as i32;
        cap[i] = (cc + 64) * c * n >> 2;
    }
}

#[allow(clippy::too_many_arguments)]
fn interp_bits2pulses(
    start: i32,
    end: i32,
    skip_start: i32,
    bits1: &[i32],
    bits2: &[i32],
    thresh: &[i32],
    cap: &[i32],
    mut total: i32,
    balance_out: &mut i32,
    skip_rsv: i32,
    intensity: &mut i32,
    mut intensity_rsv: i32,
    dual_stereo: &mut i32,
    mut dual_stereo_rsv: i32,
    bits: &mut [i32],
    ebits: &mut [i32],
    fine_priority: &mut [i32],
    c: i32,
    lm: i32,
    ec: &mut Decoder,
) -> i32 {
    let alloc_floor = c << BITRES;
    let stereo = (c > 1) as i32;
    let log_m = lm << BITRES;
    let mut lo = 0i32;
    let mut hi = 1i32 << ALLOC_STEPS;
    for _ in 0..ALLOC_STEPS {
        let mid = (lo + hi) >> 1;
        let mut psum: i32 = 0;
        let mut done = false;
        let mut j = end;
        while j > start {
            j -= 1;
            let ju = j as usize;
            let tmp = bits1[ju].wrapping_add(mid.wrapping_mul(bits2[ju]) >> ALLOC_STEPS);
            if tmp >= thresh[ju] || done {
                done = true;
                psum = psum.wrapping_add(tmp.min(cap[ju]));
            } else if tmp >= alloc_floor {
                psum = psum.wrapping_add(alloc_floor);
            }
        }
        if psum > total {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let mut psum: i32 = 0;
    let mut done = false;
    let mut j = end;
    while j > start {
        j -= 1;
        let ju = j as usize;
        let mut tmp = bits1[ju].wrapping_add(lo.wrapping_mul(bits2[ju]) >> ALLOC_STEPS);
        if tmp < thresh[ju] && !done {
            if tmp >= alloc_floor {
                tmp = alloc_floor;
            } else {
                tmp = 0;
            }
        } else {
            done = true;
        }
        tmp = tmp.min(cap[ju]);
        bits[ju] = tmp;
        psum = psum.wrapping_add(tmp);
    }

    let mut coded_bands = end;
    loop {
        let j = coded_bands - 1;
        if j <= skip_start {
            total = total.wrapping_add(skip_rsv);
            break;
        }
        let ju = j as usize;
        let mut left = total.wrapping_sub(psum);
        let width_all = eb(coded_bands) - eb(start);
        let percoeff = celt_udiv(left, width_all);
        left = left.wrapping_sub(width_all.wrapping_mul(percoeff));
        let rem = (left - (eb(j) - eb(start))).max(0);
        let band_width = eb(coded_bands) - eb(j);
        let mut band_bits = bits[ju]
            .wrapping_add(percoeff.wrapping_mul(band_width))
            .wrapping_add(rem);
        if band_bits >= thresh[ju].max(alloc_floor + (1 << BITRES)) {
            if ec.bit_logp(1) {
                break;
            }
            psum = psum.wrapping_add(1 << BITRES);
            band_bits -= 1 << BITRES;
        }
        psum = psum.wrapping_sub(bits[ju].wrapping_add(intensity_rsv));
        if intensity_rsv > 0 {
            intensity_rsv = LOG2_FRAC_TABLE.get((j - start).max(0) as usize).copied().unwrap_or(0) as i32;
        }
        psum = psum.wrapping_add(intensity_rsv);
        if band_bits >= alloc_floor {
            psum = psum.wrapping_add(alloc_floor);
            bits[ju] = alloc_floor;
        } else {
            bits[ju] = 0;
        }
        coded_bands -= 1;
    }

    if intensity_rsv > 0 {
        *intensity = start + ec.dec_uint((coded_bands + 1 - start) as u32) as i32;
    } else {
        *intensity = 0;
    }
    if *intensity <= start {
        total = total.wrapping_add(dual_stereo_rsv);
        dual_stereo_rsv = 0;
    }
    if dual_stereo_rsv > 0 {
        *dual_stereo = ec.bit_logp(1) as i32;
    } else {
        *dual_stereo = 0;
    }

    let mut left = total.wrapping_sub(psum);
    let width_all = eb(coded_bands) - eb(start);
    let percoeff = celt_udiv(left, width_all);
    left = left.wrapping_sub(width_all.wrapping_mul(percoeff));
    for j in start..coded_bands {
        let ju = j as usize;
        bits[ju] = bits[ju].wrapping_add(percoeff.wrapping_mul(eb(j + 1) - eb(j)));
    }
    for j in start..coded_bands {
        let ju = j as usize;
        let tmp = left.min(eb(j + 1) - eb(j));
        bits[ju] = bits[ju].wrapping_add(tmp);
        left = left.wrapping_sub(tmp);
    }

    let mut balance: i32 = 0;
    let mut j = start;
    while j < coded_bands {
        let ju = j as usize;
        let n0 = eb(j + 1) - eb(j);
        let n = n0 << lm;
        let bit = bits[ju].wrapping_add(balance);
        let mut excess;
        if n > 1 {
            excess = (bit.wrapping_sub(cap[ju])).max(0);
            bits[ju] = bit.wrapping_sub(excess);
            let den = c * n + ((c == 2 && n > 2 && *dual_stereo == 0 && j < *intensity) as i32);
            let nclogn = den * (logn(j) + log_m);
            let mut offset = (nclogn >> 1) - den * FINE_OFFSET;
            if n == 2 {
                offset += den << BITRES >> 2;
            }
            if bits[ju].wrapping_add(offset) < den * 2 << BITRES {
                offset += nclogn >> 2;
            } else if bits[ju].wrapping_add(offset) < den * 3 << BITRES {
                offset += nclogn >> 3;
            }
            ebits[ju] = (bits[ju].wrapping_add(offset).wrapping_add(den << (BITRES - 1))).max(0);
            ebits[ju] = celt_udiv(ebits[ju], den) >> BITRES;
            if c * ebits[ju] > (bits[ju] >> BITRES) {
                ebits[ju] = bits[ju] >> stereo >> BITRES;
            }
            ebits[ju] = ebits[ju].min(MAX_FINE_BITS);
            fine_priority[ju] =
                (ebits[ju].wrapping_mul(den << BITRES) >= bits[ju].wrapping_add(offset)) as i32;
            bits[ju] = bits[ju].wrapping_sub((c * ebits[ju]) << BITRES);
        } else {
            excess = (bit.wrapping_sub(c << BITRES)).max(0);
            bits[ju] = bit.wrapping_sub(excess);
            ebits[ju] = 0;
            fine_priority[ju] = 1;
        }
        if excess > 0 {
            let extra_fine = (excess >> (stereo + BITRES)).min(MAX_FINE_BITS - ebits[ju]);
            ebits[ju] += extra_fine;
            let extra_bits = (extra_fine * c) << BITRES;
            fine_priority[ju] = (extra_bits >= excess.wrapping_sub(balance)) as i32;
            excess -= extra_bits;
        }
        balance = excess;
        j += 1;
    }
    *balance_out = balance;
    while j < end {
        let ju = j as usize;
        ebits[ju] = bits[ju] >> stereo >> BITRES;
        bits[ju] = 0;
        fine_priority[ju] = (ebits[ju] < 1) as i32;
        j += 1;
    }
    coded_bands
}

#[allow(clippy::too_many_arguments)]
pub fn clt_compute_allocation(
    start: i32,
    end: i32,
    offsets: &[i32],
    cap: &[i32],
    alloc_trim: i32,
    intensity: &mut i32,
    dual_stereo: &mut i32,
    total: i32,
    balance: &mut i32,
    pulses: &mut [i32],
    ebits: &mut [i32],
    fine_priority: &mut [i32],
    c: i32,
    lm: i32,
    ec: &mut Decoder,
) -> i32 {
    let mut total = total.max(0);
    let len = NB_EBANDS as i32;
    let mut skip_start = start;
    let skip_rsv = if total >= 1 << BITRES { 1 << BITRES } else { 0 };
    total -= skip_rsv;
    let mut intensity_rsv = 0;
    let mut dual_stereo_rsv = 0;
    if c == 2 {
        intensity_rsv = LOG2_FRAC_TABLE.get((end - start).max(0) as usize).copied().unwrap_or(0) as i32;
        if intensity_rsv > total {
            intensity_rsv = 0;
        } else {
            total -= intensity_rsv;
            dual_stereo_rsv = if total >= 1 << BITRES { 1 << BITRES } else { 0 };
            total -= dual_stereo_rsv;
        }
    }
    let mut bits1 = [0i32; NB_EBANDS];
    let mut bits2 = [0i32; NB_EBANDS];
    let mut thresh = [0i32; NB_EBANDS];
    let mut trim_offset = [0i32; NB_EBANDS];
    let alloc = |row: i32, j: i32| -> i32 {
        BAND_ALLOCATION.get((row * len + j).max(0) as usize).copied().unwrap_or(0) as i32
    };
    for j in start..end {
        let ju = j as usize;
        let w = eb(j + 1) - eb(j);
        thresh[ju] = (c << BITRES).max((3 * w << lm << BITRES) >> 4);
        trim_offset[ju] = (c * w * (alloc_trim - 5 - lm) * (end - j - 1)).wrapping_mul(1 << (lm + BITRES)) >> 6;
        if w << lm == 1 {
            trim_offset[ju] -= c << BITRES;
        }
    }
    let mut lo = 1i32;
    let mut hi = NB_ALLOC_VECTORS - 1;
    loop {
        let mut done = false;
        let mut psum: i32 = 0;
        let mid = (lo + hi) >> 1;
        let mut j = end;
        while j > start {
            j -= 1;
            let ju = j as usize;
            let n = eb(j + 1) - eb(j);
            let mut bitsj = (c * n * alloc(mid, j)) << lm >> 2;
            if bitsj > 0 {
                bitsj = (bitsj + trim_offset[ju]).max(0);
            }
            bitsj = bitsj.wrapping_add(offsets[ju]);
            if bitsj >= thresh[ju] || done {
                done = true;
                psum = psum.wrapping_add(bitsj.min(cap[ju]));
            } else if bitsj >= c << BITRES {
                psum = psum.wrapping_add(c << BITRES);
            }
        }
        if psum > total {
            hi = mid - 1;
        } else {
            lo = mid + 1;
        }
        if lo > hi {
            break;
        }
    }
    hi = lo;
    lo -= 1;
    for j in start..end {
        let ju = j as usize;
        let n = eb(j + 1) - eb(j);
        let mut bits1j = (c * n * alloc(lo, j)) << lm >> 2;
        let mut bits2j = if hi >= NB_ALLOC_VECTORS {
            cap[ju]
        } else {
            (c * n * alloc(hi, j)) << lm >> 2
        };
        if bits1j > 0 {
            bits1j = (bits1j + trim_offset[ju]).max(0);
        }
        if bits2j > 0 {
            bits2j = (bits2j + trim_offset[ju]).max(0);
        }
        if lo > 0 {
            bits1j = bits1j.wrapping_add(offsets[ju]);
        }
        bits2j = bits2j.wrapping_add(offsets[ju]);
        if offsets[ju] > 0 {
            skip_start = j;
        }
        bits2j = (bits2j.wrapping_sub(bits1j)).max(0);
        bits1[ju] = bits1j;
        bits2[ju] = bits2j;
    }
    interp_bits2pulses(
        start,
        end,
        skip_start,
        &bits1,
        &bits2,
        &thresh,
        cap,
        total,
        balance,
        skip_rsv,
        intensity,
        intensity_rsv,
        dual_stereo,
        dual_stereo_rsv,
        pulses,
        ebits,
        fine_priority,
        c,
        lm,
        ec,
    )
}
