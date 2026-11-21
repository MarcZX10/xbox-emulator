use super::macros::*;
use super::tables::*;
use crate::entdec::Decoder;

#[derive(Clone, Copy, Default)]
pub struct StereoDecState {
    pub pred_prev_q13: [i16; 2],
    pub s_mid: [i16; 2],
    pub s_side: [i16; 2],
}

const STEREO_INTERP_LEN_MS: i32 = 8;
const STEP_HALF_Q16: i32 = 6554;

pub fn stereo_decode_pred(dec: &mut Decoder, pred_q13: &mut [i32; 2]) {
    let mut ix = [[0i32; 3]; 2];
    let n = dec.icdf(&STEREO_PRED_JOINT_ICDF, 8);
    ix[0][2] = div32(n, 5);
    ix[1][2] = n - 5 * ix[0][2];
    for row in ix.iter_mut() {
        row[0] = dec.icdf(&UNIFORM3_ICDF, 8);
        row[1] = dec.icdf(&UNIFORM5_ICDF, 8);
    }
    for n in 0..2 {
        ix[n][0] += 3 * ix[n][2];
        let i0 = ix[n][0].clamp(0, 14) as usize;
        let low_q13 = STEREO_PRED_QUANT_Q13[i0] as i32;
        let step_q13 = smulwb(STEREO_PRED_QUANT_Q13[i0 + 1] as i32 - low_q13, STEP_HALF_Q16);
        pred_q13[n] = smlabb(low_q13, step_q13, 2 * ix[n][1] + 1);
    }
    pred_q13[0] = pred_q13[0].wrapping_sub(pred_q13[1]);
}

pub fn stereo_decode_mid_only(dec: &mut Decoder) -> i32 {
    dec.icdf(&STEREO_ONLY_CODE_MID_ICDF, 8)
}

pub fn stereo_ms_to_lr(state: &mut StereoDecState, x1: &mut [i16], x2: &mut [i16], pred_q13: &[i32; 2], fs_khz: i32, frame_length: usize) {
    if x1.len() < frame_length + 2 || x2.len() < frame_length + 2 {
        return;
    }
    x1[0] = state.s_mid[0];
    x1[1] = state.s_mid[1];
    x2[0] = state.s_side[0];
    x2[1] = state.s_side[1];
    state.s_mid = [x1[frame_length], x1[frame_length + 1]];
    state.s_side = [x2[frame_length], x2[frame_length + 1]];

    let mut pred0_q13 = state.pred_prev_q13[0] as i32;
    let mut pred1_q13 = state.pred_prev_q13[1] as i32;
    let denom_q16 = div32(1 << 16, STEREO_INTERP_LEN_MS * fs_khz);
    let delta0_q13 = rshift_round(smulbb(pred_q13[0] - state.pred_prev_q13[0] as i32, denom_q16), 16);
    let delta1_q13 = rshift_round(smulbb(pred_q13[1] - state.pred_prev_q13[1] as i32, denom_q16), 16);
    let interp = ((STEREO_INTERP_LEN_MS * fs_khz).max(0) as usize).min(frame_length);
    let step = |x1: &[i16], x2: &mut [i16], n: usize, p0: i32, p1: i32| {
        let mut sum = lshift(add_lshift32(x1[n] as i32 + x1[n + 2] as i32, x1[n + 1] as i32, 1), 9);
        sum = smlawb(lshift(x2[n + 1] as i32, 8), sum, p0);
        sum = smlawb(sum, lshift(x1[n + 1] as i32, 11), p1);
        x2[n + 1] = sat16(rshift_round(sum, 8)) as i16;
    };
    for n in 0..interp {
        pred0_q13 = pred0_q13.wrapping_add(delta0_q13);
        pred1_q13 = pred1_q13.wrapping_add(delta1_q13);
        step(x1, x2, n, pred0_q13, pred1_q13);
    }
    pred0_q13 = pred_q13[0];
    pred1_q13 = pred_q13[1];
    for n in interp..frame_length {
        step(x1, x2, n, pred0_q13, pred1_q13);
    }
    state.pred_prev_q13[0] = pred_q13[0] as i16;
    state.pred_prev_q13[1] = pred_q13[1] as i16;

    for n in 0..frame_length {
        let sum = x1[n + 1] as i32 + x2[n + 1] as i32;
        let diff = x1[n + 1] as i32 - x2[n + 1] as i32;
        x1[n + 1] = sat16(sum) as i16;
        x2[n + 1] = sat16(diff) as i16;
    }
}
