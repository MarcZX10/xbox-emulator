use super::lpc::{bwexpander, lpc_analysis_filter, lpc_inverse_pred_gain};
use super::macros::*;
use super::misc::sum_sqr_shift;
use super::state::*;

const NB_ATT: usize = 2;
const HARM_ATT_Q15: [i16; NB_ATT] = [32440, 31130];
const PLC_RAND_ATTENUATE_V_Q15: [i16; NB_ATT] = [31130, 26214];
const PLC_RAND_ATTENUATE_UV_Q15: [i16; NB_ATT] = [32440, 29491];

const BWE_COEF_Q16: i32 = 64881;
const V_PITCH_GAIN_START_MIN_Q14: i32 = 11469;
const V_PITCH_GAIN_START_MAX_Q14: i32 = 15565;
const MAX_PITCH_LAG_MS: i32 = 18;
const RAND_BUF_SIZE: i32 = 128;
const RAND_BUF_MASK: i32 = RAND_BUF_SIZE - 1;
const LOG2_INV_LPC_GAIN_HIGH_THRES: i32 = 3;
const LOG2_INV_LPC_GAIN_LOW_THRES: i32 = 8;
const PITCH_DRIFT_FAC_Q16: i32 = 655;

pub fn plc_reset(ch: &mut ChannelState) {
    ch.plc.pitch_l_q8 = lshift(ch.frame_length, 8 - 1);
    ch.plc.prev_gain_q16[0] = 1 << 16;
    ch.plc.prev_gain_q16[1] = 1 << 16;
    ch.plc.subfr_length = 20;
    ch.plc.nb_subfr = 2;
}

pub fn plc(ch: &mut ChannelState, ctrl: &mut DecoderControl, frame: &mut [i16], lost: bool) {
    if ch.fs_khz != ch.plc.fs_khz {
        plc_reset(ch);
        ch.plc.fs_khz = ch.fs_khz;
    }
    if lost {
        plc_conceal(ch, ctrl, frame);
        ch.loss_cnt = ch.loss_cnt.wrapping_add(1);
    } else {
        plc_update(ch, ctrl);
    }
}

fn plc_update(ch: &mut ChannelState, ctrl: &DecoderControl) {
    let nb_subfr = (ch.nb_subfr.max(0) as usize).min(MAX_NB_SUBFR);
    let subfr_length = ch.subfr_length;
    ch.prev_signal_type = ch.indices.signal_type as i32;
    let mut ltp_gain_q14: i32 = 0;
    if ch.indices.signal_type as i32 == TYPE_VOICED && nb_subfr > 0 {
        let mut j: usize = 0;
        while (j as i32).wrapping_mul(subfr_length) < ctrl.pitch_l[nb_subfr - 1] {
            if j == nb_subfr {
                break;
            }
            let base = (nb_subfr - 1 - j) * LTP_ORDER;
            let mut temp: i32 = 0;
            for i in 0..LTP_ORDER {
                temp += ctrl.ltp_coef_q14[base + i] as i32;
            }
            if temp > ltp_gain_q14 {
                ltp_gain_q14 = temp;
                ch.plc.ltp_coef_q14.copy_from_slice(&ctrl.ltp_coef_q14[base..base + LTP_ORDER]);
                ch.plc.pitch_l_q8 = lshift(ctrl.pitch_l[nb_subfr - 1 - j], 8);
            }
            j += 1;
        }
        ch.plc.ltp_coef_q14 = [0; LTP_ORDER];
        ch.plc.ltp_coef_q14[LTP_ORDER / 2] = ltp_gain_q14 as i16;
        if ltp_gain_q14 < V_PITCH_GAIN_START_MIN_Q14 {
            let tmp = lshift(V_PITCH_GAIN_START_MIN_Q14, 10);
            let scale_q10 = div32(tmp, ltp_gain_q14.max(1));
            for i in 0..LTP_ORDER {
                ch.plc.ltp_coef_q14[i] = (smulbb(ch.plc.ltp_coef_q14[i] as i32, scale_q10) >> 10) as i16;
            }
        } else if ltp_gain_q14 > V_PITCH_GAIN_START_MAX_Q14 {
            let tmp = lshift(V_PITCH_GAIN_START_MAX_Q14, 14);
            let scale_q14 = div32(tmp, ltp_gain_q14.max(1));
            for i in 0..LTP_ORDER {
                ch.plc.ltp_coef_q14[i] = (smulbb(ch.plc.ltp_coef_q14[i] as i32, scale_q14) >> 14) as i16;
            }
        }
    } else {
        ch.plc.pitch_l_q8 = lshift(smulbb(ch.fs_khz, 18), 8);
        ch.plc.ltp_coef_q14 = [0; LTP_ORDER];
    }
    let order = (ch.lpc_order.max(0) as usize).min(MAX_LPC_ORDER);
    ch.plc.prev_lpc_q12[..order].copy_from_slice(&ctrl.pred_coef_q12[1][..order]);
    ch.plc.prev_ltp_scale_q14 = ctrl.ltp_scale_q14 as i16;
    if nb_subfr >= 2 {
        ch.plc.prev_gain_q16[0] = ctrl.gains_q16[nb_subfr - 2];
        ch.plc.prev_gain_q16[1] = ctrl.gains_q16[nb_subfr - 1];
    }
    ch.plc.subfr_length = ch.subfr_length;
    ch.plc.nb_subfr = ch.nb_subfr;
}

fn plc_energy(exc_q14: &[i32; MAX_FRAME_LENGTH], prev_gain_q10: &[i32; 2], subfr_length: usize, nb_subfr: usize) -> (i32, i32, i32, i32) {
    let mut exc_buf = [0i16; 2 * MAX_SUB_FRAME_LENGTH];
    let subfr_length = subfr_length.min(MAX_SUB_FRAME_LENGTH);
    let mut p = 0usize;
    for k in 0..2 {
        for i in 0..subfr_length {
            let idx = i as isize + (k as isize + nb_subfr as isize - 2) * subfr_length as isize;
            let e = if idx >= 0 && (idx as usize) < MAX_FRAME_LENGTH { exc_q14[idx as usize] } else { 0 };
            exc_buf[p + i] = sat16(smulww(e, prev_gain_q10[k]) >> 8) as i16;
        }
        p += subfr_length;
    }
    let (e1, s1) = sum_sqr_shift(&exc_buf[..subfr_length], subfr_length);
    let (e2, s2) = sum_sqr_shift(&exc_buf[subfr_length..2 * subfr_length], subfr_length);
    (e1, s1, e2, s2)
}

fn plc_conceal(ch: &mut ChannelState, ctrl: &mut DecoderControl, frame: &mut [i16]) {
    let ltp_mem_length = (ch.ltp_mem_length.max(0) as usize).min(MAX_FRAME_LENGTH);
    let frame_length = (ch.frame_length.max(0) as usize).min(MAX_FRAME_LENGTH).min(frame.len());
    let lpc_order = (ch.lpc_order.max(0) as usize).min(MAX_LPC_ORDER);
    let nb_subfr = (ch.nb_subfr.max(0) as usize).min(MAX_NB_SUBFR);
    let subfr_length = (ch.subfr_length.max(0) as usize).min(MAX_SUB_FRAME_LENGTH);
    if ltp_mem_length < MAX_LPC_ORDER || lpc_order == 0 {
        for v in frame.iter_mut().take(frame_length) {
            *v = 0;
        }
        return;
    }
    let mut s_ltp_q14 = [0i32; 2 * MAX_FRAME_LENGTH];
    let mut s_ltp = [0i16; MAX_FRAME_LENGTH];
    let prev_gain_q10 = [ch.plc.prev_gain_q16[0] >> 6, ch.plc.prev_gain_q16[1] >> 6];
    if ch.first_frame_after_reset != 0 {
        ch.plc.prev_lpc_q12 = [0; MAX_LPC_ORDER];
    }
    let (energy1, shift1, energy2, shift2) = plc_energy(&ch.exc_q14, &prev_gain_q10, subfr_length, nb_subfr);

    let rand_off = if rshift(energy1, shift2) < rshift(energy2, shift1) {
        0.max((ch.plc.nb_subfr - 1).wrapping_mul(ch.plc.subfr_length) - RAND_BUF_SIZE)
    } else {
        0.max(ch.plc.nb_subfr.wrapping_mul(ch.plc.subfr_length) - RAND_BUF_SIZE)
    };
    let rand_off = (rand_off as usize).min(MAX_FRAME_LENGTH - RAND_BUF_SIZE as usize);

    let mut rand_scale_q14 = ch.plc.rand_scale_q14;
    let lc = (ch.loss_cnt.max(0) as usize).min(NB_ATT - 1);
    let harm_gain_q15 = HARM_ATT_Q15[lc] as i32;
    let mut rand_gain_q15 = if ch.prev_signal_type == TYPE_VOICED {
        PLC_RAND_ATTENUATE_V_Q15[lc] as i32
    } else {
        PLC_RAND_ATTENUATE_UV_Q15[lc] as i32
    };

    bwexpander(&mut ch.plc.prev_lpc_q12, lpc_order, BWE_COEF_Q16);
    let mut a_q12 = [0i16; MAX_LPC_ORDER];
    a_q12[..lpc_order].copy_from_slice(&ch.plc.prev_lpc_q12[..lpc_order]);

    if ch.loss_cnt == 0 {
        rand_scale_q14 = 1 << 14;
        if ch.prev_signal_type == TYPE_VOICED {
            for i in 0..LTP_ORDER {
                rand_scale_q14 = rand_scale_q14.wrapping_sub(ch.plc.ltp_coef_q14[i]);
            }
            rand_scale_q14 = rand_scale_q14.max(3277);
            rand_scale_q14 = (smulbb(rand_scale_q14 as i32, ch.plc.prev_ltp_scale_q14 as i32) >> 14) as i16;
        } else {
            let inv_gain_q30 = lpc_inverse_pred_gain(&ch.plc.prev_lpc_q12, lpc_order);
            let mut down_scale_q30 = ((1i32 << 30) >> LOG2_INV_LPC_GAIN_HIGH_THRES).min(inv_gain_q30);
            down_scale_q30 = ((1i32 << 30) >> LOG2_INV_LPC_GAIN_LOW_THRES).max(down_scale_q30);
            down_scale_q30 = lshift(down_scale_q30, LOG2_INV_LPC_GAIN_HIGH_THRES);
            rand_gain_q15 = smulwb(down_scale_q30, rand_gain_q15) >> 14;
        }
    }

    let mut rand_seed = ch.plc.rand_seed;
    let max_lag = ltp_mem_length as i32 - lpc_order as i32 - 3;
    let clamp_lag = |l: i32| -> usize { l.clamp(3, max_lag.max(3)) as usize };
    let mut lag = clamp_lag(rshift_round(ch.plc.pitch_l_q8, 8));
    let mut s_ltp_buf_idx = ltp_mem_length;

    let idx = ltp_mem_length.saturating_sub(lag + lpc_order + LTP_ORDER / 2);
    lpc_analysis_filter(&mut s_ltp[idx..ltp_mem_length], &ch.out_buf[idx..ltp_mem_length], &a_q12, ltp_mem_length - idx, lpc_order);
    let mut inv_gain_q30 = inverse32_varq(ch.plc.prev_gain_q16[1], 46);
    inv_gain_q30 = inv_gain_q30.min(i32::MAX >> 1);
    for i in (idx + lpc_order)..ltp_mem_length {
        s_ltp_q14[i] = smulwb(inv_gain_q30, s_ltp[i] as i32);
    }

    for _k in 0..nb_subfr {
        let mut pl = s_ltp_buf_idx + LTP_ORDER / 2 - lag;
        for _i in 0..subfr_length {
            let b = &ch.plc.ltp_coef_q14;
            let mut ltp_pred_q12: i32 = 2;
            ltp_pred_q12 = smlawb(ltp_pred_q12, s_ltp_q14[pl], b[0] as i32);
            ltp_pred_q12 = smlawb(ltp_pred_q12, s_ltp_q14[pl - 1], b[1] as i32);
            ltp_pred_q12 = smlawb(ltp_pred_q12, s_ltp_q14[pl - 2], b[2] as i32);
            ltp_pred_q12 = smlawb(ltp_pred_q12, s_ltp_q14[pl - 3], b[3] as i32);
            ltp_pred_q12 = smlawb(ltp_pred_q12, s_ltp_q14[pl - 4], b[4] as i32);
            pl += 1;
            rand_seed = rand(rand_seed);
            let ri = ((rand_seed >> 25) & RAND_BUF_MASK) as usize;
            s_ltp_q14[s_ltp_buf_idx] = lshift(smlawb(ltp_pred_q12, ch.exc_q14[rand_off + ri], rand_scale_q14 as i32), 2);
            s_ltp_buf_idx += 1;
        }
        for j in 0..LTP_ORDER {
            ch.plc.ltp_coef_q14[j] = (smulbb(harm_gain_q15, ch.plc.ltp_coef_q14[j] as i32) >> 15) as i16;
        }
        rand_scale_q14 = (smulbb(rand_scale_q14 as i32, rand_gain_q15) >> 15) as i16;
        ch.plc.pitch_l_q8 = smlawb(ch.plc.pitch_l_q8, ch.plc.pitch_l_q8, PITCH_DRIFT_FAC_Q16);
        ch.plc.pitch_l_q8 = ch.plc.pitch_l_q8.min(lshift(smulbb(MAX_PITCH_LAG_MS, ch.fs_khz), 8));
        lag = clamp_lag(rshift_round(ch.plc.pitch_l_q8, 8));
    }

    let base = ltp_mem_length - MAX_LPC_ORDER;
    s_ltp_q14[base..base + MAX_LPC_ORDER].copy_from_slice(&ch.slpc_q14_buf);
    for i in 0..frame_length {
        let mut lpc_pred_q10 = (lpc_order as i32) >> 1;
        for j in 0..lpc_order {
            lpc_pred_q10 = smlawb(lpc_pred_q10, s_ltp_q14[base + MAX_LPC_ORDER + i - j - 1], a_q12[j] as i32);
        }
        let p = base + MAX_LPC_ORDER + i;
        s_ltp_q14[p] = add_sat32(s_ltp_q14[p], lshift_sat32(lpc_pred_q10, 4));
        frame[i] = sat16(sat16(rshift_round(smulww(s_ltp_q14[p], prev_gain_q10[1]), 8))) as i16;
    }
    ch.slpc_q14_buf.copy_from_slice(&s_ltp_q14[base + frame_length..base + frame_length + MAX_LPC_ORDER]);

    ch.plc.rand_seed = rand_seed;
    ch.plc.rand_scale_q14 = rand_scale_q14;
    for i in 0..MAX_NB_SUBFR {
        ctrl.pitch_l[i] = lag as i32;
    }
}

pub fn plc_glue_frames(ch: &mut ChannelState, frame: &mut [i16], length: usize) {
    let length = length.min(frame.len());
    if ch.loss_cnt != 0 {
        let (e, s) = sum_sqr_shift(frame, length);
        ch.plc.conc_energy = e;
        ch.plc.conc_energy_shift = s;
        ch.plc.last_frame_lost = 1;
    } else {
        if ch.plc.last_frame_lost != 0 {
            let (mut energy, energy_shift) = sum_sqr_shift(frame, length);
            if energy_shift > ch.plc.conc_energy_shift {
                ch.plc.conc_energy = rshift(ch.plc.conc_energy, energy_shift - ch.plc.conc_energy_shift);
            } else if energy_shift < ch.plc.conc_energy_shift {
                energy = rshift(energy, ch.plc.conc_energy_shift - energy_shift);
            }
            if energy > ch.plc.conc_energy {
                let lz = clz32(ch.plc.conc_energy) - 1;
                ch.plc.conc_energy = lshift(ch.plc.conc_energy, lz);
                energy = rshift(energy, (24 - lz).max(0));
                let frac_q24 = div32(ch.plc.conc_energy, energy.max(1));
                let mut gain_q16 = lshift(sqrt_approx(frac_q24), 4);
                let mut slope_q16 = div32((1i32 << 16) - gain_q16, length as i32);
                slope_q16 = lshift(slope_q16, 2);
                for v in frame.iter_mut().take(length) {
                    *v = smulwb(gain_q16, *v as i32) as i16;
                    gain_q16 = gain_q16.wrapping_add(slope_q16);
                    if gain_q16 > 1 << 16 {
                        break;
                    }
                }
            }
        }
        ch.plc.last_frame_lost = 0;
    }
}
