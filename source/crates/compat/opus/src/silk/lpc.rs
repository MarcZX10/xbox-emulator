use super::macros::*;

pub const MAX_PREDICTION_POWER_GAIN: f32 = 1e4;

pub fn bwexpander(ar: &mut [i16], d: usize, chirp_q16_in: i32) {
    let d = d.min(ar.len());
    if d == 0 {
        return;
    }
    let mut chirp_q16 = chirp_q16_in;
    let chirp_minus_one_q16 = chirp_q16.wrapping_sub(65536);
    for i in 0..d - 1 {
        ar[i] = rshift_round(mul(chirp_q16, ar[i] as i32), 16) as i16;
        chirp_q16 = chirp_q16.wrapping_add(rshift_round(mul(chirp_q16, chirp_minus_one_q16), 16));
    }
    ar[d - 1] = rshift_round(mul(chirp_q16, ar[d - 1] as i32), 16) as i16;
}

pub fn bwexpander_32(ar: &mut [i32], d: usize, chirp_q16_in: i32) {
    let d = d.min(ar.len());
    if d == 0 {
        return;
    }
    let mut chirp_q16 = chirp_q16_in;
    let chirp_minus_one_q16 = chirp_q16.wrapping_sub(65536);
    for i in 0..d - 1 {
        ar[i] = smulww(chirp_q16, ar[i]);
        chirp_q16 = chirp_q16.wrapping_add(rshift_round(mul(chirp_q16, chirp_minus_one_q16), 16));
    }
    ar[d - 1] = smulww(chirp_q16, ar[d - 1]);
}

pub fn lpc_fit(a_qout: &mut [i16], a_qin: &mut [i32], qout: i32, qin: i32, d: usize) {
    let d = d.min(a_qout.len()).min(a_qin.len());
    let mut idx = 0usize;
    let mut i = 0;
    while i < 10 {
        let mut maxabs: i32 = 0;
        for k in 0..d {
            let absval = abs32(a_qin[k]);
            if absval > maxabs {
                maxabs = absval;
                idx = k;
            }
        }
        maxabs = rshift_round(maxabs, qin - qout);
        if maxabs > I16_MAX {
            maxabs = maxabs.min(163838);
            let chirp_q16 = 65470i32.wrapping_sub(div32(
                lshift(maxabs - I16_MAX, 14),
                rshift(mul(maxabs, idx as i32 + 1), 2),
            ));
            bwexpander_32(a_qin, d, chirp_q16);
        } else {
            break;
        }
        i += 1;
    }
    if i == 10 {
        for k in 0..d {
            a_qout[k] = sat16(rshift_round(a_qin[k], qin - qout)) as i16;
            a_qin[k] = lshift(a_qout[k] as i32, qin - qout);
        }
    } else {
        for k in 0..d {
            a_qout[k] = rshift_round(a_qin[k], qin - qout) as i16;
        }
    }
}

const QA: i32 = 24;
const A_LIMIT: i32 = 16773022;
const INV_GAIN_MIN_Q30: i32 = 107374;

fn mul32_frac_q(a: i32, b: i32, q: i32) -> i32 {
    rshift_round64(smull(a, b), q) as i32
}

fn lpc_inverse_pred_gain_qa(a_qa: &mut [i32; 24], order: usize) -> i32 {
    let mut inv_gain_q30: i32 = 1 << 30;
    let mut k = order as isize - 1;
    while k > 0 {
        let ku = k as usize;
        if a_qa[ku] > A_LIMIT || a_qa[ku] < -A_LIMIT {
            return 0;
        }
        let rc_q31 = lshift(a_qa[ku], 31 - QA).wrapping_neg();
        let rc_mult1_q30 = (1i32 << 30).wrapping_sub(smmul(rc_q31, rc_q31));
        inv_gain_q30 = lshift(smmul(inv_gain_q30, rc_mult1_q30), 2);
        if inv_gain_q30 < INV_GAIN_MIN_Q30 {
            return 0;
        }
        let mult2q = 32 - clz32(abs32(rc_mult1_q30));
        let rc_mult2 = inverse32_varq(rc_mult1_q30, mult2q + 30);
        for n in 0..((ku + 1) >> 1) {
            let tmp1 = a_qa[n];
            let tmp2 = a_qa[ku - n - 1];
            let tmp64 = rshift_round64(smull(sub_sat32(tmp1, mul32_frac_q(tmp2, rc_q31, 31)), rc_mult2), mult2q);
            if tmp64 > i32::MAX as i64 || tmp64 < i32::MIN as i64 {
                return 0;
            }
            a_qa[n] = tmp64 as i32;
            let tmp64 = rshift_round64(smull(sub_sat32(tmp2, mul32_frac_q(tmp1, rc_q31, 31)), rc_mult2), mult2q);
            if tmp64 > i32::MAX as i64 || tmp64 < i32::MIN as i64 {
                return 0;
            }
            a_qa[ku - n - 1] = tmp64 as i32;
        }
        k -= 1;
    }
    if a_qa[0] > A_LIMIT || a_qa[0] < -A_LIMIT {
        return 0;
    }
    let rc_q31 = lshift(a_qa[0], 31 - QA).wrapping_neg();
    let rc_mult1_q30 = (1i32 << 30).wrapping_sub(smmul(rc_q31, rc_q31));
    inv_gain_q30 = lshift(smmul(inv_gain_q30, rc_mult1_q30), 2);
    if inv_gain_q30 < INV_GAIN_MIN_Q30 {
        return 0;
    }
    inv_gain_q30
}

pub fn lpc_inverse_pred_gain(a_q12: &[i16], order: usize) -> i32 {
    let order = order.min(a_q12.len()).min(24);
    if order == 0 {
        return 1 << 30;
    }
    let mut atmp = [0i32; 24];
    let mut dc_resp: i32 = 0;
    for k in 0..order {
        dc_resp = dc_resp.wrapping_add(a_q12[k] as i32);
        atmp[k] = lshift(a_q12[k] as i32, QA - 12);
    }
    if dc_resp >= 4096 {
        return 0;
    }
    lpc_inverse_pred_gain_qa(&mut atmp, order)
}

pub fn lpc_analysis_filter(out: &mut [i16], input: &[i16], b: &[i16], len: usize, d: usize) {
    let len = len.min(out.len()).min(input.len());
    if d < 6 || d > b.len() {
        for o in out.iter_mut().take(len) {
            *o = 0;
        }
        return;
    }
    for ix in d..len {
        let p = ix - 1;
        let mut out32_q12 = smulbb(input[p] as i32, b[0] as i32);
        for j in 1..d {
            out32_q12 = smlabb_ovflw(out32_q12, input[p - j] as i32, b[j] as i32);
        }
        out32_q12 = lshift(input[p + 1] as i32, 12).wrapping_sub(out32_q12);
        let out32 = rshift_round(out32_q12, 12);
        out[ix] = sat16(out32) as i16;
    }
    for o in out.iter_mut().take(d.min(len)) {
        *o = 0;
    }
}
