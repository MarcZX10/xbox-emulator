mod cng;
mod decode;
pub mod lpc;
pub mod macros;
pub mod misc;
pub mod nlsf;
mod plc;
pub mod resampler;
mod state;
mod stereo;
pub mod tables;

#[cfg(test)]
mod tests;

use crate::entdec::Decoder;
use decode::{decode_frame, decode_indices, decode_pulses};
use macros::{div32, smulbb};
use state::*;
use stereo::{stereo_decode_mid_only, stereo_decode_pred, stereo_ms_to_lr, StereoDecState};

pub const SILK_NO_ERROR: i32 = 0;
pub const SILK_DEC_INVALID_SAMPLING_FREQUENCY: i32 = -200;
pub const SILK_DEC_PAYLOAD_TOO_LARGE: i32 = -201;
pub const SILK_DEC_PAYLOAD_ERROR: i32 = -202;
pub const SILK_DEC_INVALID_FRAME_SIZE: i32 = -203;

const MAX_API_FS_KHZ: i32 = 48;
const TMP_LEN: usize = MAX_FRAME_LENGTH + 2;
const RES_OUT_LEN: usize = (MAX_API_FS_KHZ as usize) * 20 + 64;

#[derive(Clone, Copy, Default)]
pub struct DecControl {
    pub n_channels_api: i32,
    pub n_channels_internal: i32,
    pub api_sample_rate: i32,
    pub internal_sample_rate: i32,
    pub payload_size_ms: i32,
    pub prev_pitch_lag: i32,
}

pub struct SilkDecoder {
    channel_state: Box<[ChannelState; 2]>,
    s_stereo: StereoDecState,
    n_channels_api: i32,
    n_channels_internal: i32,
    prev_decode_only_middle: i32,
}

impl Default for SilkDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl SilkDecoder {
    pub fn new() -> SilkDecoder {
        let mut d = SilkDecoder {
            channel_state: Box::new([ChannelState::default(), ChannelState::default()]),
            s_stereo: StereoDecState::default(),
            n_channels_api: 0,
            n_channels_internal: 0,
            prev_decode_only_middle: 0,
        };
        for ch in d.channel_state.iter_mut() {
            ch.init();
        }
        d
    }

    pub fn reset(&mut self) {
        for ch in self.channel_state.iter_mut() {
            ch.reset();
        }
        self.s_stereo = StereoDecState::default();
        self.prev_decode_only_middle = 0;
    }

    pub fn decode(&mut self, ctl: &mut DecControl, lost_flag: i32, new_packet: bool, dec: &mut Decoder, out: &mut [f32], n_samples_out: &mut i32) -> i32 {
        *n_samples_out = 0;
        let mut ret: i32 = 0;
        let mut decode_only_middle: i32 = 0;
        let mut ms_pred_q13 = [0i32; 2];
        let n_int = ctl.n_channels_internal;
        let n_api = ctl.n_channels_api;
        if !(n_int == 1 || n_int == 2) || !(n_api == 1 || n_api == 2) {
            return SILK_DEC_INVALID_FRAME_SIZE;
        }
        if !(0..=2).contains(&lost_flag) {
            return SILK_DEC_PAYLOAD_ERROR;
        }
        let n_int_u = n_int as usize;
        let cs = &mut *self.channel_state;

        if new_packet {
            for ch in cs.iter_mut().take(n_int_u) {
                ch.n_frames_decoded = 0;
            }
        }

        if n_int > self.n_channels_internal {
            ret += cs[1].init();
        }

        let stereo_to_mono = n_int == 1 && self.n_channels_internal == 2 && ctl.internal_sample_rate == 1000 * cs[0].fs_khz;

        if cs[0].n_frames_decoded == 0 {
            for ch in cs.iter_mut().take(n_int_u) {
                match ctl.payload_size_ms {
                    0 | 10 => {
                        ch.n_frames_per_packet = 1;
                        ch.nb_subfr = 2;
                    }
                    20 => {
                        ch.n_frames_per_packet = 1;
                        ch.nb_subfr = 4;
                    }
                    40 => {
                        ch.n_frames_per_packet = 2;
                        ch.nb_subfr = 4;
                    }
                    60 => {
                        ch.n_frames_per_packet = 3;
                        ch.nb_subfr = 4;
                    }
                    _ => return SILK_DEC_INVALID_FRAME_SIZE,
                }
                let fs_khz_dec = (ctl.internal_sample_rate >> 10) + 1;
                if fs_khz_dec != 8 && fs_khz_dec != 12 && fs_khz_dec != 16 {
                    return SILK_DEC_INVALID_SAMPLING_FREQUENCY;
                }
                ret += ch.set_fs(fs_khz_dec, ctl.api_sample_rate);
            }
        }

        if n_api == 2 && n_int == 2 && (self.n_channels_api == 1 || self.n_channels_internal == 1) {
            self.s_stereo.pred_prev_q13 = [0; 2];
            self.s_stereo.s_side = [0; 2];
            cs[1].resampler_state = cs[0].resampler_state.clone();
        }
        self.n_channels_api = n_api;
        self.n_channels_internal = n_int;

        if ctl.api_sample_rate > MAX_API_FS_KHZ * 1000 || ctl.api_sample_rate < 8000 {
            return SILK_DEC_INVALID_SAMPLING_FREQUENCY;
        }

        if cs[0].frame_length <= 0 || cs[0].frame_length as usize > MAX_FRAME_LENGTH || cs[0].fs_khz <= 0 {
            return SILK_DEC_INVALID_FRAME_SIZE;
        }
        if n_int == 2 && (cs[1].frame_length != cs[0].frame_length || cs[1].fs_khz != cs[0].fs_khz) {
            return SILK_DEC_INVALID_FRAME_SIZE;
        }

        if lost_flag != FLAG_PACKET_LOST && cs[0].n_frames_decoded == 0 {
            for ch in cs.iter_mut().take(n_int_u) {
                let nfpp = (ch.n_frames_per_packet.max(0) as usize).min(MAX_FRAMES_PER_PACKET);
                for i in 0..nfpp {
                    ch.vad_flags[i] = dec.bit_logp(1) as i32;
                }
                ch.lbrr_flag = dec.bit_logp(1) as i32;
            }
            for ch in cs.iter_mut().take(n_int_u) {
                ch.lbrr_flags = [0; MAX_FRAMES_PER_PACKET];
                if ch.lbrr_flag != 0 {
                    if ch.n_frames_per_packet == 1 {
                        ch.lbrr_flags[0] = 1;
                    } else if ch.n_frames_per_packet == 2 || ch.n_frames_per_packet == 3 {
                        let tab = tables::LBRR_FLAGS_ICDF_PTR[(ch.n_frames_per_packet - 2) as usize];
                        let sym = dec.icdf(tab, 8) + 1;
                        for i in 0..ch.n_frames_per_packet as usize {
                            ch.lbrr_flags[i] = (sym >> i) & 1;
                        }
                    }
                }
            }
            if lost_flag == FLAG_DECODE_NORMAL {
                let nfpp0 = (cs[0].n_frames_per_packet.max(0) as usize).min(MAX_FRAMES_PER_PACKET);
                for i in 0..nfpp0 {
                    for n in 0..n_int_u {
                        if cs[n].lbrr_flags[i] != 0 {
                            let mut pulses = [0i16; 320];
                            if n_int == 2 && n == 0 {
                                stereo_decode_pred(dec, &mut ms_pred_q13);
                                if cs[1].lbrr_flags[i] == 0 {
                                    decode_only_middle = stereo_decode_mid_only(dec);
                                }
                            }
                            let cond = if i > 0 && cs[n].lbrr_flags[i - 1] != 0 { CODE_CONDITIONALLY } else { CODE_INDEPENDENTLY };
                            decode_indices(&mut cs[n], dec, i as i32, true, cond);
                            let st = cs[n].indices.signal_type as i32;
                            let qot = cs[n].indices.quant_offset_type as i32;
                            let fl = cs[n].frame_length;
                            decode_pulses(dec, &mut pulses, st, qot, fl);
                        }
                    }
                }
            }
        }

        if n_int == 2 {
            let nfd = cs[0].n_frames_decoded;
            if lost_flag == FLAG_DECODE_NORMAL || (lost_flag == FLAG_DECODE_LBRR && cs[0].lbrr_flag_at(nfd) == 1) {
                stereo_decode_pred(dec, &mut ms_pred_q13);
                if (lost_flag == FLAG_DECODE_NORMAL && cs[1].vad_flag(nfd) == 0) || (lost_flag == FLAG_DECODE_LBRR && cs[1].lbrr_flag_at(nfd) == 0) {
                    decode_only_middle = stereo_decode_mid_only(dec);
                } else {
                    decode_only_middle = 0;
                }
            } else {
                for n in 0..2 {
                    ms_pred_q13[n] = self.s_stereo.pred_prev_q13[n] as i32;
                }
            }
        }

        if n_int == 2 && decode_only_middle == 0 && self.prev_decode_only_middle == 1 {
            let c1 = &mut cs[1];
            c1.out_buf = [0; MAX_FRAME_LENGTH + 2 * MAX_SUB_FRAME_LENGTH];
            c1.slpc_q14_buf = [0; MAX_LPC_ORDER];
            c1.lag_prev = 100;
            c1.last_gain_index = 10;
            c1.prev_signal_type = TYPE_NO_VOICE_ACTIVITY;
            c1.first_frame_after_reset = 1;
        }

        let mut tmp = [[0i16; TMP_LEN]; 2];

        let has_side = if lost_flag == FLAG_DECODE_NORMAL {
            decode_only_middle == 0
        } else {
            self.prev_decode_only_middle == 0 || (n_int == 2 && lost_flag == FLAG_DECODE_LBRR && cs[1].lbrr_flag_at(cs[1].n_frames_decoded) == 1)
        };

        let mut n_samples_out_dec: i32 = 0;
        for n in 0..n_int_u {
            if n == 0 || has_side {
                let frame_index = cs[0].n_frames_decoded - n as i32;
                let cond = if frame_index <= 0 {
                    CODE_INDEPENDENTLY
                } else if lost_flag == FLAG_DECODE_LBRR {
                    if cs[n].lbrr_flag_at(frame_index - 1) != 0 { CODE_CONDITIONALLY } else { CODE_INDEPENDENTLY }
                } else if n > 0 && self.prev_decode_only_middle != 0 {
                    CODE_INDEPENDENTLY_NO_LTP_SCALING
                } else {
                    CODE_CONDITIONALLY
                };
                ret += decode_frame(&mut cs[n], dec, &mut tmp[n][2..], &mut n_samples_out_dec, lost_flag, cond);
            } else {
                let cnt = (n_samples_out_dec.max(0) as usize).min(TMP_LEN - 2);
                for v in tmp[n][2..2 + cnt].iter_mut() {
                    *v = 0;
                }
            }
            cs[n].n_frames_decoded = cs[n].n_frames_decoded.wrapping_add(1);
        }
        let nsod = (n_samples_out_dec.max(0) as usize).min(MAX_FRAME_LENGTH);

        if n_api == 2 && n_int == 2 {
            let (a, b) = tmp.split_at_mut(1);
            stereo_ms_to_lr(&mut self.s_stereo, &mut a[0], &mut b[0], &ms_pred_q13, cs[0].fs_khz, nsod);
        } else {
            tmp[0][0] = self.s_stereo.s_mid[0];
            tmp[0][1] = self.s_stereo.s_mid[1];
            self.s_stereo.s_mid = [tmp[0][nsod], tmp[0][nsod + 1]];
        }

        let n_out = div32((nsod as i32).wrapping_mul(ctl.api_sample_rate), smulbb(cs[0].fs_khz, 1000)).max(0);
        let n_out_u = (n_out as usize).min(RES_OUT_LEN);
        let mut res_out = [0i16; RES_OUT_LEN];
        let scale = 1.0f32 / 32768.0f32;

        let nch = n_api.min(n_int) as usize;
        for n in 0..nch {
            ret += cs[n].resampler_state.process(&mut res_out, &tmp[n][1..1 + nsod], nsod);
            if n_api == 2 {
                for i in 0..n_out_u {
                    if let Some(o) = out.get_mut(n + 2 * i) {
                        *o = res_out[i] as f32 * scale;
                    }
                }
            } else {
                for i in 0..n_out_u {
                    if let Some(o) = out.get_mut(i) {
                        *o = res_out[i] as f32 * scale;
                    }
                }
            }
        }

        if n_api == 2 && n_int == 1 {
            if stereo_to_mono {
                ret += cs[1].resampler_state.process(&mut res_out, &tmp[0][1..1 + nsod], nsod);
                for i in 0..n_out_u {
                    if let Some(o) = out.get_mut(1 + 2 * i) {
                        *o = res_out[i] as f32 * scale;
                    }
                }
            } else {
                for i in 0..n_out_u {
                    if 1 + 2 * i < out.len() {
                        out[1 + 2 * i] = out[2 * i];
                    }
                }
            }
        }

        if cs[0].prev_signal_type == TYPE_VOICED {
            let mult_tab = [6, 4, 3];
            let idx = ((cs[0].fs_khz - 8) >> 2).clamp(0, 2) as usize;
            ctl.prev_pitch_lag = cs[0].lag_prev.wrapping_mul(mult_tab[idx]);
        } else {
            ctl.prev_pitch_lag = 0;
        }

        if lost_flag == FLAG_PACKET_LOST {
            for ch in cs.iter_mut().take(self.n_channels_internal as usize) {
                ch.last_gain_index = 10;
            }
        } else {
            self.prev_decode_only_middle = decode_only_middle;
        }
        *n_samples_out = n_out;
        ret
    }
}
