use super::macros::*;
use super::nlsf::nlsf2a;
use super::state::*;

const CNG_BUF_MASK_MAX: i32 = 255;
const CNG_GAIN_SMTH_Q16: i32 = 4634;
const CNG_GAIN_SMTH_THRESHOLD_Q16: i32 = 46396;
const CNG_NLSF_SMTH_Q16: i32 = 16348;

fn cng_exc(exc_q14: &mut [i32], exc_buf_q14: &[i32; MAX_FRAME_LENGTH], length: usize, rand_seed: &mut i32) {
    let mut exc_mask = CNG_BUF_MASK_MAX;
    while exc_mask > length as i32 {
        exc_mask >>= 1;
    }
    let mut seed = *rand_seed;
    for v in exc_q14.iter_mut().take(length) {
        seed = rand(seed);
        let idx = ((seed >> 24) & exc_mask) as usize;
        *v = exc_buf_q14[idx];
    }
    *rand_seed = seed;
}

pub fn cng_reset(ch: &mut ChannelState) {
    let order = (ch.lpc_order.max(0) as usize).min(MAX_LPC_ORDER);
    let nlsf_step_q15 = div32(I16_MAX, ch.lpc_order.wrapping_add(1));
    let mut nlsf_acc_q15: i32 = 0;
    for i in 0..order {
        nlsf_acc_q15 += nlsf_step_q15;
        ch.cng.cng_smth_nlsf_q15[i] = nlsf_acc_q15 as i16;
    }
    ch.cng.cng_smth_gain_q16 = 0;
    ch.cng.rand_seed = 3176576;
}

pub fn cng(ch: &mut ChannelState, ctrl: &DecoderControl, frame: &mut [i16], length: usize) {
    let length = length.min(frame.len()).min(MAX_FRAME_LENGTH);
    if ch.fs_khz != ch.cng.fs_khz {
        cng_reset(ch);
        ch.cng.fs_khz = ch.fs_khz;
    }
    let order = (ch.lpc_order.max(0) as usize).min(MAX_LPC_ORDER);
    let nb_subfr = (ch.nb_subfr.max(0) as usize).min(MAX_NB_SUBFR);
    let subfr_length = (ch.subfr_length.max(0) as usize).min(MAX_SUB_FRAME_LENGTH);
    if ch.loss_cnt == 0 && ch.prev_signal_type == TYPE_NO_VOICE_ACTIVITY {
        for i in 0..order {
            let d = (ch.prev_nlsf_q15[i] as i32).wrapping_sub(ch.cng.cng_smth_nlsf_q15[i] as i32);
            ch.cng.cng_smth_nlsf_q15[i] = (ch.cng.cng_smth_nlsf_q15[i] as i32).wrapping_add(smulwb(d, CNG_NLSF_SMTH_Q16)) as i16;
        }
        let mut max_gain_q16: i32 = 0;
        let mut subfr = 0usize;
        for i in 0..nb_subfr {
            if ctrl.gains_q16[i] > max_gain_q16 {
                max_gain_q16 = ctrl.gains_q16[i];
                subfr = i;
            }
        }
        if nb_subfr >= 1 && nb_subfr * subfr_length <= MAX_FRAME_LENGTH {
            let mv = (nb_subfr - 1) * subfr_length;
            ch.cng.cng_exc_buf_q14.copy_within(0..mv, subfr_length);
            let src = subfr * subfr_length;
            let exc = ch.exc_q14;
            ch.cng.cng_exc_buf_q14[..subfr_length].copy_from_slice(&exc[src..src + subfr_length]);
        }
        for i in 0..nb_subfr {
            ch.cng.cng_smth_gain_q16 = ch.cng.cng_smth_gain_q16.wrapping_add(smulwb(ctrl.gains_q16[i].wrapping_sub(ch.cng.cng_smth_gain_q16), CNG_GAIN_SMTH_Q16));
            if smulww(ch.cng.cng_smth_gain_q16, CNG_GAIN_SMTH_THRESHOLD_Q16) > ctrl.gains_q16[i] {
                ch.cng.cng_smth_gain_q16 = ctrl.gains_q16[i];
            }
        }
    }

    if ch.loss_cnt != 0 {
        let mut cng_sig_q14 = [0i32; MAX_FRAME_LENGTH + MAX_LPC_ORDER];
        let mut gain_q16 = smulww(ch.plc.rand_scale_q14 as i32, ch.plc.prev_gain_q16[1]);
        if gain_q16 >= (1 << 21) || ch.cng.cng_smth_gain_q16 > (1 << 23) {
            gain_q16 = smultt(gain_q16, gain_q16);
            gain_q16 = sub_lshift32(smultt(ch.cng.cng_smth_gain_q16, ch.cng.cng_smth_gain_q16), gain_q16, 5);
            gain_q16 = lshift(sqrt_approx(gain_q16), 16);
        } else {
            gain_q16 = smulww(gain_q16, gain_q16);
            gain_q16 = sub_lshift32(smulww(ch.cng.cng_smth_gain_q16, ch.cng.cng_smth_gain_q16), gain_q16, 5);
            gain_q16 = lshift(sqrt_approx(gain_q16), 8);
        }
        let gain_q10 = gain_q16 >> 6;

        let exc_buf = ch.cng.cng_exc_buf_q14;
        cng_exc(&mut cng_sig_q14[MAX_LPC_ORDER..], &exc_buf, length, &mut ch.cng.rand_seed);

        let mut a_q12 = [0i16; MAX_LPC_ORDER];
        let smth = ch.cng.cng_smth_nlsf_q15;
        nlsf2a(&mut a_q12, &smth, order);

        cng_sig_q14[..MAX_LPC_ORDER].copy_from_slice(&ch.cng.cng_synth_state);
        let ord = if order == 16 { 16 } else { 10 };
        for i in 0..length {
            let mut lpc_pred_q10 = (ch.lpc_order) >> 1;
            for j in 0..ord {
                lpc_pred_q10 = smlawb(lpc_pred_q10, cng_sig_q14[MAX_LPC_ORDER + i - j - 1], a_q12[j] as i32);
            }
            let p = MAX_LPC_ORDER + i;
            cng_sig_q14[p] = add_sat32(cng_sig_q14[p], lshift_sat32(lpc_pred_q10, 4));
            frame[i] = add_sat16(frame[i] as i32, sat16(rshift_round(smulww(cng_sig_q14[p], gain_q10), 8)));
        }
        ch.cng.cng_synth_state.copy_from_slice(&cng_sig_q14[length..length + MAX_LPC_ORDER]);
    } else {
        for v in ch.cng.cng_synth_state.iter_mut().take(order) {
            *v = 0;
        }
    }
}
