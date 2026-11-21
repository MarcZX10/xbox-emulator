#![allow(dead_code)]

use super::macros::*;

pub fn sum_sqr_shift(x: &[i16], len: usize) -> (i32, i32) {
    let len = len.min(x.len());
    let mut shft = 31 - clz32(len as i32);
    let mut nrg: i32 = len as i32;
    let mut i = 0usize;
    while i + 1 < len {
        let mut t = smulbb(x[i] as i32, x[i] as i32) as u32;
        t = smlabb_ovflw(t as i32, x[i + 1] as i32, x[i + 1] as i32) as u32;
        nrg = (nrg as u32).wrapping_add(t >> shft.clamp(0, 31)) as i32;
        i += 2;
    }
    if i < len {
        let t = smulbb(x[i] as i32, x[i] as i32) as u32;
        nrg = (nrg as u32).wrapping_add(t >> shft.clamp(0, 31)) as i32;
    }
    shft = 0.max(shft + 3 - clz32(nrg));
    nrg = 0;
    i = 0;
    while i + 1 < len {
        let mut t = smulbb(x[i] as i32, x[i] as i32) as u32;
        t = smlabb_ovflw(t as i32, x[i + 1] as i32, x[i + 1] as i32) as u32;
        nrg = (nrg as u32).wrapping_add(t >> shft.clamp(0, 31)) as i32;
        i += 2;
    }
    if i < len {
        let t = smulbb(x[i] as i32, x[i] as i32) as u32;
        nrg = (nrg as u32).wrapping_add(t >> shft.clamp(0, 31)) as i32;
    }
    (nrg, shft)
}

pub fn lin2log(in_lin: i32) -> i32 {
    let (lz, frac_q7) = clz_frac(in_lin);
    add_lshift32(smlawb(frac_q7, mul(frac_q7, 128 - frac_q7), 179), 31 - lz, 7)
}

pub fn log2lin(in_log_q7: i32) -> i32 {
    if in_log_q7 < 0 {
        return 0;
    } else if in_log_q7 >= 3967 {
        return i32::MAX;
    }
    let mut out = lshift(1, in_log_q7 >> 7);
    let frac_q7 = in_log_q7 & 0x7F;
    if in_log_q7 < 2048 {
        out = add_rshift32(out, mul(out, smlawb(frac_q7, smulbb(frac_q7, 128 - frac_q7), -174)), 7);
    } else {
        out = mla(out, out >> 7, smlawb(frac_q7, smulbb(frac_q7, 128 - frac_q7), -174));
    }
    out
}

const SIGM_LUT_SLOPE_Q10: [i32; 6] = [237, 153, 73, 30, 12, 7];
const SIGM_LUT_POS_Q15: [i32; 6] = [16384, 23955, 28861, 31213, 32178, 32548];
const SIGM_LUT_NEG_Q15: [i32; 6] = [16384, 8812, 3906, 1554, 589, 219];

pub fn sigm_q15(in_q5: i32) -> i32 {
    if in_q5 < 0 {
        let v = in_q5.wrapping_neg();
        if v >= 6 * 32 || v < 0 {
            0
        } else {
            let ind = (v >> 5) as usize;
            SIGM_LUT_NEG_Q15[ind] - smulbb(SIGM_LUT_SLOPE_Q10[ind], v & 0x1F)
        }
    } else if in_q5 >= 6 * 32 {
        32767
    } else {
        let ind = (in_q5 >> 5) as usize;
        SIGM_LUT_POS_Q15[ind] + smulbb(SIGM_LUT_SLOPE_Q10[ind], in_q5 & 0x1F)
    }
}

pub fn insertion_sort_increasing_all_values_int16(a: &mut [i16]) {
    let l = a.len();
    for i in 1..l {
        let value = a[i];
        let mut j = i as isize - 1;
        while j >= 0 && value < a[j as usize] {
            a[(j + 1) as usize] = a[j as usize];
            j -= 1;
        }
        a[(j + 1) as usize] = value;
    }
}

pub fn inner_prod_aligned(a: &[i16], b: &[i16], len: usize) -> i32 {
    let mut sum: i32 = 0;
    for i in 0..len.min(a.len()).min(b.len()) {
        sum = smlabb(sum, a[i] as i32, b[i] as i32);
    }
    sum
}
