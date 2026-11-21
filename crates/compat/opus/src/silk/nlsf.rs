use super::lpc::{bwexpander_32, lpc_fit, lpc_inverse_pred_gain};
use super::macros::*;
use super::misc::insertion_sort_increasing_all_values_int16;
use super::tables::{NlsfCb, LSFCOSTAB_FIX_Q12};

pub const NLSF_QUANT_MAX_AMPLITUDE: i32 = 4;
const MAX_LPC_STABILIZE_ITERATIONS: i32 = 16;

pub fn nlsf_unpack(ec_ix: &mut [i16; 16], pred_q8: &mut [u8; 16], cb: &NlsfCb, cb1_index: i32) {
    let order = (cb.order as usize).min(16);
    let base = (cb1_index.max(0) as usize) * order / 2;
    let mut p = base;
    let mut i = 0;
    while i + 1 < order + 1 && i < order {
        let entry = cb.ec_sel.get(p).copied().unwrap_or(0) as i32;
        p += 1;
        ec_ix[i] = smulbb((entry >> 1) & 7, 2 * NLSF_QUANT_MAX_AMPLITUDE + 1) as i16;
        pred_q8[i] = cb.pred_q8.get(i + (entry & 1) as usize * (order - 1)).copied().unwrap_or(0);
        if i + 1 < order {
            ec_ix[i + 1] = smulbb((entry >> 5) & 7, 2 * NLSF_QUANT_MAX_AMPLITUDE + 1) as i16;
            pred_q8[i + 1] = cb.pred_q8.get(i + ((entry >> 4) & 1) as usize * (order - 1) + 1).copied().unwrap_or(0);
        }
        i += 2;
    }
}

fn nlsf_residual_dequant(x_q10: &mut [i16; 16], indices: &[i8], pred_coef_q8: &[u8; 16], quant_step_size_q16: i32, order: usize) {
    let mut out_q10: i32 = 0;
    let mut i = order as isize - 1;
    while i >= 0 {
        let iu = i as usize;
        let pred_q10 = smulbb(out_q10, pred_coef_q8[iu] as i16 as i32) >> 8;
        out_q10 = lshift(indices.get(iu).copied().unwrap_or(0) as i32, 10);
        if out_q10 > 0 {
            out_q10 -= 102;
        } else if out_q10 < 0 {
            out_q10 += 102;
        }
        out_q10 = smlawb(pred_q10, out_q10, quant_step_size_q16);
        x_q10[iu] = out_q10 as i16;
        i -= 1;
    }
}

pub fn nlsf_decode(p_nlsf_q15: &mut [i16; 16], nlsf_indices: &[i8; 17], cb: &NlsfCb) {
    let order = (cb.order as usize).min(16);
    let mut pred_q8 = [0u8; 16];
    let mut ec_ix = [0i16; 16];
    let mut res_q10 = [0i16; 16];
    let cb1 = (nlsf_indices[0] as i32).max(0) as usize;
    nlsf_unpack(&mut ec_ix, &mut pred_q8, cb, cb1 as i32);
    nlsf_residual_dequant(&mut res_q10, &nlsf_indices[1..], &pred_q8, cb.quant_step_size_q16 as i32, order);
    let base = cb1 * order;
    for i in 0..order {
        let el = cb.cb1_nlsf_q8.get(base + i).copied().unwrap_or(0) as i16 as i32;
        let w = cb.cb1_wght_q9.get(base + i).copied().unwrap_or(1) as i32;
        let tmp = add_lshift32(div32(lshift(res_q10[i] as i32, 14), w), el, 7);
        p_nlsf_q15[i] = limit(tmp, 0, 32767) as i16;
    }
    nlsf_stabilize(&mut p_nlsf_q15[..order], cb.delta_min_q15, order);
}

pub fn nlsf_stabilize(nlsf_q15: &mut [i16], ndelta_min_q15: &[i16], l: usize) {
    let l = l.min(nlsf_q15.len());
    if l == 0 || ndelta_min_q15.len() < l + 1 {
        return;
    }
    let dm = |i: usize| ndelta_min_q15[i] as i32;
    let mut loops = 0;
    while loops < 20 {
        let mut min_diff_q15 = nlsf_q15[0] as i32 - dm(0);
        let mut ii = 0usize;
        for i in 1..l {
            let diff_q15 = nlsf_q15[i] as i32 - (nlsf_q15[i - 1] as i32 + dm(i));
            if diff_q15 < min_diff_q15 {
                min_diff_q15 = diff_q15;
                ii = i;
            }
        }
        let diff_q15 = (1 << 15) - (nlsf_q15[l - 1] as i32 + dm(l));
        if diff_q15 < min_diff_q15 {
            min_diff_q15 = diff_q15;
            ii = l;
        }
        if min_diff_q15 >= 0 {
            return;
        }
        if ii == 0 {
            nlsf_q15[0] = dm(0) as i16;
        } else if ii == l {
            nlsf_q15[l - 1] = ((1 << 15) - dm(l)) as i16;
        } else {
            let mut min_center_q15: i32 = 0;
            for k in 0..ii {
                min_center_q15 += dm(k);
            }
            min_center_q15 += dm(ii) >> 1;
            let mut max_center_q15: i32 = 1 << 15;
            let mut k = l;
            while k > ii {
                max_center_q15 -= dm(k);
                k -= 1;
            }
            max_center_q15 -= dm(ii) >> 1;
            let center_freq_q15 = limit(
                rshift_round(nlsf_q15[ii - 1] as i32 + nlsf_q15[ii] as i32, 1),
                min_center_q15,
                max_center_q15,
            ) as i16;
            nlsf_q15[ii - 1] = (center_freq_q15 as i32 - (dm(ii) >> 1)) as i16;
            nlsf_q15[ii] = (nlsf_q15[ii - 1] as i32 + dm(ii)) as i16;
        }
        loops += 1;
    }
    insertion_sort_increasing_all_values_int16(&mut nlsf_q15[..l]);
    nlsf_q15[0] = (nlsf_q15[0] as i32).max(dm(0)) as i16;
    for i in 1..l {
        nlsf_q15[i] = (nlsf_q15[i] as i32).max(add_sat16(nlsf_q15[i - 1] as i32, dm(i)) as i32) as i16;
    }
    nlsf_q15[l - 1] = (nlsf_q15[l - 1] as i32).min((1 << 15) - dm(l)) as i16;
    let mut i = l as isize - 2;
    while i >= 0 {
        let iu = i as usize;
        nlsf_q15[iu] = (nlsf_q15[iu] as i32).min(nlsf_q15[iu + 1] as i32 - dm(iu + 1)) as i16;
        i -= 1;
    }
}

const QA: i32 = 16;

fn nlsf2a_find_poly(out: &mut [i32; 13], c_lsf: &[i32], dd: usize) {
    out[0] = lshift(1, QA);
    out[1] = c_lsf[0].wrapping_neg();
    for k in 1..dd {
        let ftmp = c_lsf[2 * k];
        out[k + 1] = lshift(out[k - 1], 1).wrapping_sub(rshift_round64(smull(ftmp, out[k]), QA) as i32);
        let mut n = k;
        while n > 1 {
            out[n] = out[n].wrapping_add(out[n - 2].wrapping_sub(rshift_round64(smull(ftmp, out[n - 1]), QA) as i32));
            n -= 1;
        }
        out[1] = out[1].wrapping_sub(ftmp);
    }
}

const ORDERING16: [usize; 16] = [0, 15, 8, 7, 4, 11, 12, 3, 2, 13, 10, 5, 6, 9, 14, 1];
const ORDERING10: [usize; 10] = [0, 9, 6, 3, 4, 5, 8, 1, 2, 7];

pub fn nlsf2a(a_q12: &mut [i16], nlsf: &[i16], d: usize) {
    let d = if d == 16 { 16 } else { 10 };
    if a_q12.len() < d || nlsf.len() < d {
        return;
    }
    let ordering: &[usize] = if d == 16 { &ORDERING16 } else { &ORDERING10 };
    let mut cos_lsf_qa = [0i32; 24];
    let mut p = [0i32; 13];
    let mut q = [0i32; 13];
    let mut a32_qa1 = [0i32; 24];
    for k in 0..d {
        let v = nlsf[k] as i32;
        let f_int = (v >> 8).clamp(0, 127);
        let f_frac = v - (f_int << 8);
        let cos_val = LSFCOSTAB_FIX_Q12[f_int as usize] as i32;
        let delta = LSFCOSTAB_FIX_Q12[f_int as usize + 1] as i32 - cos_val;
        cos_lsf_qa[ordering[k]] = rshift_round(lshift(cos_val, 8).wrapping_add(mul(delta, f_frac)), 20 - QA);
    }
    let dd = d >> 1;
    nlsf2a_find_poly(&mut p, &cos_lsf_qa[0..], dd);
    nlsf2a_find_poly(&mut q, &cos_lsf_qa[1..], dd);
    for k in 0..dd {
        let ptmp = p[k + 1].wrapping_add(p[k]);
        let qtmp = q[k + 1].wrapping_sub(q[k]);
        a32_qa1[k] = qtmp.wrapping_neg().wrapping_sub(ptmp);
        a32_qa1[d - k - 1] = qtmp.wrapping_sub(ptmp);
    }
    lpc_fit(a_q12, &mut a32_qa1, 12, QA + 1, d);
    let mut i = 0;
    while lpc_inverse_pred_gain(a_q12, d) == 0 && i < MAX_LPC_STABILIZE_ITERATIONS {
        bwexpander_32(&mut a32_qa1, d, 65536 - lshift(2, i));
        for k in 0..d {
            a_q12[k] = rshift_round(a32_qa1[k], QA + 1 - 12) as i16;
        }
        i += 1;
    }
}
