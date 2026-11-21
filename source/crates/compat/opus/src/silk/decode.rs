use super::cng::cng;
use super::lpc::{bwexpander, lpc_analysis_filter};
use super::macros::*;
use super::misc::log2lin;
use super::nlsf::{nlsf2a, nlsf_decode, nlsf_unpack, NLSF_QUANT_MAX_AMPLITUDE};
use super::plc::{plc, plc_glue_frames};
use super::state::*;
use super::tables::*;
use crate::entdec::Decoder;

const SHELL_CODEC_FRAME_LENGTH: usize = 16;
const MAX_NB_SHELL_BLOCKS: usize = 20;
const SILK_MAX_PULSES: i32 = 16;
const N_RATE_LEVELS: usize = 10;
const PULSES_BUF_LEN: usize = 320;

fn icdf(dec: &mut Decoder, t: &[u8]) -> i32 {
    dec.icdf(t, 8)
}

fn tail<T>(t: &[T], off: usize) -> &[T] {
    t.get(off..).unwrap_or(&[])
}

pub fn decode_indices(ch: &mut ChannelState, dec: &mut Decoder, frame_index: i32, decode_lbrr: bool, cond_coding: i32) {
    let ix = if decode_lbrr || ch.vad_flag(frame_index) != 0 {
        icdf(dec, &TYPE_OFFSET_VAD_ICDF) + 2
    } else {
        icdf(dec, &TYPE_OFFSET_NO_VAD_ICDF)
    };
    ch.indices.signal_type = (ix >> 1) as i8;
    ch.indices.quant_offset_type = (ix & 1) as i8;
    let st = (ch.indices.signal_type as i32).clamp(0, 2) as usize;

    if cond_coding == CODE_CONDITIONALLY {
        ch.indices.gains_indices[0] = icdf(dec, &DELTA_GAIN_ICDF) as i8;
    } else {
        ch.indices.gains_indices[0] = lshift(icdf(dec, &GAIN_ICDF[st * 8..st * 8 + 8]), 3) as i8;
        ch.indices.gains_indices[0] = ch.indices.gains_indices[0].wrapping_add(icdf(dec, &UNIFORM8_ICDF) as i8);
    }
    let nb_subfr = (ch.nb_subfr.max(0) as usize).min(MAX_NB_SUBFR);
    for i in 1..nb_subfr {
        ch.indices.gains_indices[i] = icdf(dec, &DELTA_GAIN_ICDF) as i8;
    }

    let cb = ch.nlsf_cb.unwrap_or(&NLSF_CB_NB_MB);
    let off = (st >> 1) * cb.n_vectors as usize;
    ch.indices.nlsf_indices[0] = icdf(dec, tail(cb.cb1_icdf, off)) as i8;
    let mut ec_ix = [0i16; 16];
    let mut pred_q8 = [0u8; 16];
    nlsf_unpack(&mut ec_ix, &mut pred_q8, cb, ch.indices.nlsf_indices[0] as i32);
    let order = (cb.order as usize).min(MAX_LPC_ORDER);
    for i in 0..order {
        let mut v = icdf(dec, tail(cb.ec_icdf, ec_ix[i].max(0) as usize));
        if v == 0 {
            v -= icdf(dec, &NLSF_EXT_ICDF);
        } else if v == 2 * NLSF_QUANT_MAX_AMPLITUDE {
            v += icdf(dec, &NLSF_EXT_ICDF);
        }
        ch.indices.nlsf_indices[i + 1] = (v - NLSF_QUANT_MAX_AMPLITUDE) as i8;
    }

    if ch.nb_subfr == MAX_NB_SUBFR as i32 {
        ch.indices.nlsf_interp_coef_q2 = icdf(dec, &NLSF_INTERPOLATION_FACTOR_ICDF) as i8;
    } else {
        ch.indices.nlsf_interp_coef_q2 = 4;
    }

    if ch.indices.signal_type as i32 == TYPE_VOICED {
        let mut decode_absolute_lag_index = true;
        if cond_coding == CODE_CONDITIONALLY && ch.ec_prev_signal_type == TYPE_VOICED {
            let mut delta_lag_index = icdf(dec, &PITCH_DELTA_ICDF) as i16 as i32;
            if delta_lag_index > 0 {
                delta_lag_index -= 9;
                ch.indices.lag_index = (ch.ec_prev_lag_index as i32 + delta_lag_index) as i16;
                decode_absolute_lag_index = false;
            }
        }
        if decode_absolute_lag_index {
            ch.indices.lag_index = ((icdf(dec, &PITCH_LAG_ICDF) as i16 as i32).wrapping_mul(ch.fs_khz >> 1)) as i16;
            let low = icdf(dec, ch.pitch_lag_low_bits_icdf) as i16;
            ch.indices.lag_index = ch.indices.lag_index.wrapping_add(low);
        }
        ch.ec_prev_lag_index = ch.indices.lag_index;
        ch.indices.contour_index = icdf(dec, ch.pitch_contour_icdf) as i8;
        ch.indices.per_index = icdf(dec, &LTP_PER_INDEX_ICDF) as i8;
        let per = (ch.indices.per_index as i32).clamp(0, 2) as usize;
        for k in 0..nb_subfr {
            ch.indices.ltp_index[k] = icdf(dec, LTP_GAIN_ICDF_PTRS[per]) as i8;
        }
        if cond_coding == CODE_INDEPENDENTLY {
            ch.indices.ltp_scale_index = icdf(dec, &LTPSCALE_ICDF) as i8;
        } else {
            ch.indices.ltp_scale_index = 0;
        }
    }
    ch.ec_prev_signal_type = ch.indices.signal_type as i32;
    ch.indices.seed = icdf(dec, &UNIFORM4_ICDF) as i8;
}

fn decode_split(dec: &mut Decoder, p: i32, table: &[u8]) -> (i16, i16) {
    if p > 0 {
        let off = SHELL_CODE_TABLE_OFFSETS.get(p as usize).copied().unwrap_or(0) as usize;
        let c1 = icdf(dec, tail(table, off)) as i16;
        (c1, (p - c1 as i32) as i16)
    } else {
        (0, 0)
    }
}

pub fn shell_decoder(pulses0: &mut [i16], dec: &mut Decoder, pulses4: i32) {
    let mut pulses3 = [0i16; 2];
    let mut pulses2 = [0i16; 4];
    let mut pulses1 = [0i16; 8];
    let t0: &[u8] = &SHELL_CODE_TABLE0;
    let t1: &[u8] = &SHELL_CODE_TABLE1;
    let t2: &[u8] = &SHELL_CODE_TABLE2;
    let t3: &[u8] = &SHELL_CODE_TABLE3;
    let (a, b) = decode_split(dec, pulses4, t3);
    pulses3[0] = a;
    pulses3[1] = b;
    let set = |dst: &mut [i16], i: usize, v: (i16, i16)| {
        dst[i] = v.0;
        dst[i + 1] = v.1;
    };
    let v = decode_split(dec, pulses3[0] as i32, t2);
    set(&mut pulses2, 0, v);
    let v = decode_split(dec, pulses2[0] as i32, t1);
    set(&mut pulses1, 0, v);
    let v = decode_split(dec, pulses1[0] as i32, t0);
    set(pulses0, 0, v);
    let v = decode_split(dec, pulses1[1] as i32, t0);
    set(pulses0, 2, v);
    let v = decode_split(dec, pulses2[1] as i32, t1);
    set(&mut pulses1, 2, v);
    let v = decode_split(dec, pulses1[2] as i32, t0);
    set(pulses0, 4, v);
    let v = decode_split(dec, pulses1[3] as i32, t0);
    set(pulses0, 6, v);
    let v = decode_split(dec, pulses3[1] as i32, t2);
    set(&mut pulses2, 2, v);
    let v = decode_split(dec, pulses2[2] as i32, t1);
    set(&mut pulses1, 4, v);
    let v = decode_split(dec, pulses1[4] as i32, t0);
    set(pulses0, 8, v);
    let v = decode_split(dec, pulses1[5] as i32, t0);
    set(pulses0, 10, v);
    let v = decode_split(dec, pulses2[3] as i32, t1);
    set(&mut pulses1, 6, v);
    let v = decode_split(dec, pulses1[6] as i32, t0);
    set(pulses0, 12, v);
    let v = decode_split(dec, pulses1[7] as i32, t0);
    set(pulses0, 14, v);
}

pub fn decode_signs(dec: &mut Decoder, pulses: &mut [i16; PULSES_BUF_LEN], length: i32, signal_type: i32, quant_offset_type: i32, sum_pulses: &[i32; MAX_NB_SHELL_BLOCKS]) {
    let i0 = smulbb(7, quant_offset_type.wrapping_add(lshift(signal_type, 1))).clamp(0, 35) as usize;
    let icdf_ptr = &SIGN_ICDF[i0..];
    let nblocks = (((length + 8) >> 4).max(0) as usize).min(MAX_NB_SHELL_BLOCKS);
    for i in 0..nblocks {
        let p = sum_pulses[i];
        if p > 0 {
            let tab = [icdf_ptr[((p & 0x1F).min(6)) as usize], 0u8];
            let q = &mut pulses[i * SHELL_CODEC_FRAME_LENGTH..(i + 1) * SHELL_CODEC_FRAME_LENGTH];
            for v in q.iter_mut() {
                if *v > 0 {
                    let s = (icdf(dec, &tab) << 1) - 1;
                    *v = (*v as i32).wrapping_mul(s) as i16;
                }
            }
        }
    }
}

pub fn decode_pulses(dec: &mut Decoder, pulses: &mut [i16; PULSES_BUF_LEN], signal_type: i32, quant_offset_type: i32, frame_length: i32) {
    let st = (signal_type >> 1).clamp(0, 1) as usize;
    let rate_level_index = icdf(dec, &RATE_LEVELS_ICDF[st * 9..st * 9 + 9]).clamp(0, 9) as usize;
    let mut iter = (frame_length >> 4).max(0) as usize;
    if (iter as i32) * 16 < frame_length {
        iter += 1;
    }
    let iter = iter.min(MAX_NB_SHELL_BLOCKS);
    let mut sum_pulses = [0i32; MAX_NB_SHELL_BLOCKS];
    let mut n_lshifts = [0i32; MAX_NB_SHELL_BLOCKS];
    let cdf = &PULSES_PER_BLOCK_ICDF[rate_level_index * 18..rate_level_index * 18 + 18];
    for i in 0..iter {
        n_lshifts[i] = 0;
        sum_pulses[i] = icdf(dec, cdf);
        while sum_pulses[i] == SILK_MAX_PULSES + 1 {
            n_lshifts[i] += 1;
            let off = (N_RATE_LEVELS - 1) * 18 + (n_lshifts[i] == 10) as usize;
            sum_pulses[i] = icdf(dec, &PULSES_PER_BLOCK_ICDF[off..]);
            if n_lshifts[i] > 10 {
                break;
            }
        }
    }
    for i in 0..iter {
        let blk = &mut pulses[i * SHELL_CODEC_FRAME_LENGTH..(i + 1) * SHELL_CODEC_FRAME_LENGTH];
        if sum_pulses[i] > 0 {
            shell_decoder(blk, dec, sum_pulses[i]);
        } else {
            for v in blk.iter_mut() {
                *v = 0;
            }
        }
    }
    for i in 0..iter {
        if n_lshifts[i] > 0 {
            let nls = n_lshifts[i];
            let blk = &mut pulses[i * SHELL_CODEC_FRAME_LENGTH..(i + 1) * SHELL_CODEC_FRAME_LENGTH];
            for v in blk.iter_mut() {
                let mut abs_q = *v as i32;
                for _ in 0..nls {
                    abs_q = lshift(abs_q, 1).wrapping_add(icdf(dec, &LSB_ICDF));
                }
                *v = abs_q as i16;
            }
            sum_pulses[i] |= nls << 5;
        }
    }
    decode_signs(dec, pulses, frame_length, signal_type, quant_offset_type, &sum_pulses);
}

const GAIN_OFFSET: i32 = (2 * 128) / 6 + 16 * 128;
const INV_SCALE_Q16: i32 = (65536 * (((88 - 2) * 128) / 6)) / (64 - 1);

pub fn gains_dequant(gain_q16: &mut [i32; MAX_NB_SUBFR], ind: &[i8; MAX_NB_SUBFR], prev_ind: &mut i8, conditional: bool, nb_subfr: usize) {
    for k in 0..nb_subfr.min(MAX_NB_SUBFR) {
        if k == 0 && !conditional {
            *prev_ind = (ind[k] as i32).max(*prev_ind as i32 - 16) as i8;
        } else {
            let ind_tmp = ind[k] as i32 - 4;
            let double_step_size_threshold = 2 * 36 - 64 + *prev_ind as i32;
            if ind_tmp > double_step_size_threshold {
                *prev_ind = (*prev_ind as i32 + (lshift(ind_tmp, 1) - double_step_size_threshold)) as i8;
            } else {
                *prev_ind = (*prev_ind as i32 + ind_tmp) as i8;
            }
        }
        *prev_ind = limit(*prev_ind as i32, 0, 63) as i8;
        gain_q16[k] = log2lin((smulwb(INV_SCALE_Q16, *prev_ind as i32) + GAIN_OFFSET).min(3967));
    }
}

pub fn decode_pitch(lag_index: i16, contour_index: i8, pitch_lags: &mut [i32; MAX_NB_SUBFR], fs_khz: i32, nb_subfr: i32) {
    let (cb, cbk_size): (&[i8], usize) = if fs_khz == 8 {
        if nb_subfr == 4 {
            (&CB_LAGS_STAGE2, 11)
        } else {
            (&CB_LAGS_STAGE2_10_MS, 3)
        }
    } else if nb_subfr == 4 {
        (&CB_LAGS_STAGE3, 34)
    } else {
        (&CB_LAGS_STAGE3_10_MS, 12)
    };
    let min_lag = smulbb(2, fs_khz);
    let max_lag = smulbb(18, fs_khz);
    let lag = min_lag + lag_index as i32;
    let ci = contour_index as i32;
    for k in 0..(nb_subfr.max(0) as usize).min(MAX_NB_SUBFR) {
        let v = if ci >= 0 && (ci as usize) < cbk_size {
            cb.get(k * cbk_size + ci as usize).copied().unwrap_or(0) as i32
        } else {
            0
        };
        pitch_lags[k] = limit(lag + v, min_lag, max_lag);
    }
}

pub fn decode_parameters(ch: &mut ChannelState, ctrl: &mut DecoderControl, cond_coding: i32) {
    let nb_subfr = (ch.nb_subfr.max(0) as usize).min(MAX_NB_SUBFR);
    let order = (ch.lpc_order.max(0) as usize).min(MAX_LPC_ORDER);
    let gi = ch.indices.gains_indices;
    gains_dequant(&mut ctrl.gains_q16, &gi, &mut ch.last_gain_index, cond_coding == CODE_CONDITIONALLY, nb_subfr);

    let cb = ch.nlsf_cb.unwrap_or(&NLSF_CB_NB_MB);
    let mut p_nlsf_q15 = [0i16; MAX_LPC_ORDER];
    let mut p_nlsf0_q15 = [0i16; MAX_LPC_ORDER];
    nlsf_decode(&mut p_nlsf_q15, &ch.indices.nlsf_indices, cb);
    nlsf2a(&mut ctrl.pred_coef_q12[1], &p_nlsf_q15, order);

    if ch.first_frame_after_reset == 1 {
        ch.indices.nlsf_interp_coef_q2 = 4;
    }
    if ch.indices.nlsf_interp_coef_q2 < 4 {
        let c = ch.indices.nlsf_interp_coef_q2 as i32;
        for i in 0..order {
            p_nlsf0_q15[i] = (ch.prev_nlsf_q15[i] as i32
                + (mul(c, p_nlsf_q15[i] as i32 - ch.prev_nlsf_q15[i] as i32) >> 2)) as i16;
        }
        nlsf2a(&mut ctrl.pred_coef_q12[0], &p_nlsf0_q15, order);
    } else {
        let src = ctrl.pred_coef_q12[1];
        ctrl.pred_coef_q12[0][..order].copy_from_slice(&src[..order]);
    }
    ch.prev_nlsf_q15[..order].copy_from_slice(&p_nlsf_q15[..order]);

    if ch.loss_cnt != 0 {
        bwexpander(&mut ctrl.pred_coef_q12[0], order, 63570);
        bwexpander(&mut ctrl.pred_coef_q12[1], order, 63570);
    }

    if ch.indices.signal_type as i32 == TYPE_VOICED {
        decode_pitch(ch.indices.lag_index, ch.indices.contour_index, &mut ctrl.pitch_l, ch.fs_khz, ch.nb_subfr);
        let per = (ch.indices.per_index as i32).clamp(0, 2) as usize;
        let cbk = LTP_VQ_PTRS_Q7[per];
        for k in 0..nb_subfr {
            let ix = ch.indices.ltp_index[k] as i32;
            for i in 0..LTP_ORDER {
                let v = if ix >= 0 { cbk.get(ix as usize * LTP_ORDER + i).copied().unwrap_or(0) as i32 } else { 0 };
                ctrl.ltp_coef_q14[k * LTP_ORDER + i] = lshift(v, 7) as i16;
            }
        }
        let ix = (ch.indices.ltp_scale_index as i32).clamp(0, 2) as usize;
        ctrl.ltp_scale_q14 = LTPSCALES_TABLE_Q14[ix] as i32;
    } else {
        for k in 0..nb_subfr {
            ctrl.pitch_l[k] = 0;
        }
        for k in 0..LTP_ORDER * nb_subfr {
            ctrl.ltp_coef_q14[k] = 0;
        }
        ch.indices.per_index = 0;
        ctrl.ltp_scale_q14 = 0;
    }
}

pub fn decode_core(ch: &mut ChannelState, ctrl: &mut DecoderControl, xq: &mut [i16], pulses: &[i16; PULSES_BUF_LEN]) {
    let ltp_mem_length = (ch.ltp_mem_length.max(0) as usize).min(MAX_FRAME_LENGTH);
    let frame_length = (ch.frame_length.max(0) as usize).min(MAX_FRAME_LENGTH).min(xq.len());
    let subfr_length = (ch.subfr_length.max(0) as usize).min(MAX_SUB_FRAME_LENGTH);
    let nb_subfr = (ch.nb_subfr.max(0) as usize).min(MAX_NB_SUBFR);
    let lpc_order = (ch.lpc_order.max(0) as usize).min(MAX_LPC_ORDER);
    if subfr_length * nb_subfr > frame_length {
        return;
    }
    let mut s_ltp = [0i16; MAX_FRAME_LENGTH];
    let mut s_ltp_q15 = [0i32; 2 * MAX_FRAME_LENGTH];
    let mut res_q14 = [0i32; MAX_SUB_FRAME_LENGTH];
    let mut s_lpc_q14 = [0i32; MAX_SUB_FRAME_LENGTH + MAX_LPC_ORDER];

    let st = (ch.indices.signal_type as i32).clamp(0, 2) as usize;
    let qot = (ch.indices.quant_offset_type as i32).clamp(0, 1) as usize;
    let offset_q10 = QUANTIZATION_OFFSETS_Q10[(st >> 1) * 2 + qot] as i32;
    let nlsf_interpolation_flag = ch.indices.nlsf_interp_coef_q2 < 4;

    let mut rand_seed = ch.indices.seed as i32;
    for i in 0..frame_length {
        rand_seed = rand(rand_seed);
        let mut e = lshift(pulses[i] as i32, 14);
        if e > 0 {
            e -= 80 << 4;
        } else if e < 0 {
            e += 80 << 4;
        }
        e = e.wrapping_add(offset_q10 << 4);
        if rand_seed < 0 {
            e = e.wrapping_neg();
        }
        ch.exc_q14[i] = e;
        rand_seed = rand_seed.wrapping_add(pulses[i] as i32);
    }

    s_lpc_q14[..MAX_LPC_ORDER].copy_from_slice(&ch.slpc_q14_buf);

    let mut pexc = 0usize;
    let mut pxq = 0usize;
    let mut s_ltp_buf_idx = ltp_mem_length;
    let mut lag: usize = 0;
    let max_lag = ltp_mem_length as i64 - lpc_order as i64 - 3;
    for k in 0..nb_subfr {
        let a_q12_tmp = ctrl.pred_coef_q12[k >> 1];
        let mut signal_type = ch.indices.signal_type as i32;
        let gain_q10 = ctrl.gains_q16[k] >> 6;
        let mut inv_gain_q31 = inverse32_varq(ctrl.gains_q16[k], 47);
        let gain_adj_q16: i32;
        if ctrl.gains_q16[k] != ch.prev_gain_q16 {
            gain_adj_q16 = div32_varq(ch.prev_gain_q16, ctrl.gains_q16[k], 16);
            for v in s_lpc_q14[..MAX_LPC_ORDER].iter_mut() {
                *v = smulww(gain_adj_q16, *v);
            }
        } else {
            gain_adj_q16 = 1 << 16;
        }
        ch.prev_gain_q16 = ctrl.gains_q16[k];

        if ch.loss_cnt != 0 && ch.prev_signal_type == TYPE_VOICED && ch.indices.signal_type as i32 != TYPE_VOICED && k < MAX_NB_SUBFR / 2 {
            for i in 0..LTP_ORDER {
                ctrl.ltp_coef_q14[k * LTP_ORDER + i] = 0;
            }
            ctrl.ltp_coef_q14[k * LTP_ORDER + LTP_ORDER / 2] = 4096;
            signal_type = TYPE_VOICED;
            ctrl.pitch_l[k] = ch.lag_prev;
        }

        if signal_type == TYPE_VOICED {
            lag = (ctrl.pitch_l[k] as i64).clamp(2, max_lag.max(2)) as usize;
            if k == 0 || (k == 2 && nlsf_interpolation_flag) {
                let start_idx = ltp_mem_length.saturating_sub(lag + lpc_order + LTP_ORDER / 2);
                if k == 2 {
                    let n = (2 * subfr_length).min(xq.len());
                    ch.out_buf[ltp_mem_length..ltp_mem_length + n].copy_from_slice(&xq[..n]);
                }
                let in_start = start_idx + k * subfr_length;
                let flen = ltp_mem_length - start_idx;
                lpc_analysis_filter(&mut s_ltp[start_idx..ltp_mem_length], &ch.out_buf[in_start..in_start + flen], &a_q12_tmp, flen, lpc_order);
                if k == 0 {
                    inv_gain_q31 = lshift(smulwb(inv_gain_q31, ctrl.ltp_scale_q14), 2);
                }
                for i in 0..(lag + LTP_ORDER / 2) {
                    s_ltp_q15[s_ltp_buf_idx - i - 1] = smulwb(inv_gain_q31, s_ltp[ltp_mem_length - i - 1] as i32);
                }
            } else if gain_adj_q16 != 1 << 16 {
                for i in 0..(lag + LTP_ORDER / 2) {
                    let j = s_ltp_buf_idx - i - 1;
                    s_ltp_q15[j] = smulww(gain_adj_q16, s_ltp_q15[j]);
                }
            }
        }

        let use_res: bool;
        if signal_type == TYPE_VOICED {
            let b = &ctrl.ltp_coef_q14[k * LTP_ORDER..k * LTP_ORDER + LTP_ORDER];
            let mut pl = s_ltp_buf_idx + LTP_ORDER / 2 - lag;
            for i in 0..subfr_length {
                let mut ltp_pred_q13: i32 = 2;
                ltp_pred_q13 = smlawb(ltp_pred_q13, s_ltp_q15[pl], b[0] as i32);
                ltp_pred_q13 = smlawb(ltp_pred_q13, s_ltp_q15[pl - 1], b[1] as i32);
                ltp_pred_q13 = smlawb(ltp_pred_q13, s_ltp_q15[pl - 2], b[2] as i32);
                ltp_pred_q13 = smlawb(ltp_pred_q13, s_ltp_q15[pl - 3], b[3] as i32);
                ltp_pred_q13 = smlawb(ltp_pred_q13, s_ltp_q15[pl - 4], b[4] as i32);
                pl += 1;
                res_q14[i] = add_lshift32(ch.exc_q14[pexc + i], ltp_pred_q13, 1);
                s_ltp_q15[s_ltp_buf_idx] = lshift(res_q14[i], 1);
                s_ltp_buf_idx += 1;
            }
            use_res = true;
        } else {
            use_res = false;
        }

        for i in 0..subfr_length {
            let mut lpc_pred_q10 = (lpc_order as i32) >> 1;
            for j in 0..lpc_order {
                lpc_pred_q10 = smlawb(lpc_pred_q10, s_lpc_q14[MAX_LPC_ORDER + i - j - 1], a_q12_tmp[j] as i32);
            }
            let pres = if use_res { res_q14[i] } else { ch.exc_q14[pexc + i] };
            s_lpc_q14[MAX_LPC_ORDER + i] = add_sat32(pres, lshift_sat32(lpc_pred_q10, 4));
            if let Some(o) = xq.get_mut(pxq + i) {
                *o = sat16(rshift_round(smulww(s_lpc_q14[MAX_LPC_ORDER + i], gain_q10), 8)) as i16;
            }
        }
        s_lpc_q14.copy_within(subfr_length..subfr_length + MAX_LPC_ORDER, 0);
        pexc += subfr_length;
        pxq += subfr_length;
    }
    ch.slpc_q14_buf.copy_from_slice(&s_lpc_q14[..MAX_LPC_ORDER]);
}

pub fn decode_frame(ch: &mut ChannelState, dec: &mut Decoder, p_out: &mut [i16], p_n: &mut i32, lost_flag: i32, cond_coding: i32) -> i32 {
    let l = ch.frame_length;
    if l <= 0 || l as usize > MAX_FRAME_LENGTH || (l as usize) > p_out.len() || ch.ltp_mem_length < l || ch.ltp_mem_length as usize > MAX_FRAME_LENGTH {
        *p_n = 0;
        return super::SILK_DEC_INVALID_FRAME_SIZE;
    }
    let lu = l as usize;
    let mut ctrl = DecoderControl::default();
    ctrl.ltp_scale_q14 = 0;
    let ltp_mem = ch.ltp_mem_length as usize;
    let mv_len = ltp_mem - lu;
    if lost_flag == FLAG_DECODE_NORMAL || (lost_flag == FLAG_DECODE_LBRR && ch.lbrr_flag_at(ch.n_frames_decoded) == 1) {
        let mut pulses = [0i16; PULSES_BUF_LEN];
        let nfd = ch.n_frames_decoded;
        decode_indices(ch, dec, nfd, lost_flag != 0, cond_coding);
        decode_pulses(dec, &mut pulses, ch.indices.signal_type as i32, ch.indices.quant_offset_type as i32, ch.frame_length);
        decode_parameters(ch, &mut ctrl, cond_coding);
        decode_core(ch, &mut ctrl, &mut p_out[..lu], &pulses);
        ch.out_buf.copy_within(lu..lu + mv_len, 0);
        ch.out_buf[mv_len..mv_len + lu].copy_from_slice(&p_out[..lu]);
        plc(ch, &mut ctrl, &mut p_out[..lu], false);
        ch.loss_cnt = 0;
        ch.prev_signal_type = ch.indices.signal_type as i32;
        ch.first_frame_after_reset = 0;
    } else {
        plc(ch, &mut ctrl, &mut p_out[..lu], true);
        ch.out_buf.copy_within(lu..lu + mv_len, 0);
        ch.out_buf[mv_len..mv_len + lu].copy_from_slice(&p_out[..lu]);
    }
    cng(ch, &ctrl, &mut p_out[..lu], lu);
    plc_glue_frames(ch, &mut p_out[..lu], lu);
    let last = ((ch.nb_subfr - 1).max(0) as usize).min(MAX_NB_SUBFR - 1);
    ch.lag_prev = ctrl.pitch_l[last];
    *p_n = l;
    0
}
