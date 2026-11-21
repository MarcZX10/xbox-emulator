use super::resampler::Resampler;
use super::tables::*;

pub const MAX_FRAME_LENGTH: usize = 320;
pub const MAX_SUB_FRAME_LENGTH: usize = 80;
pub const MAX_LPC_ORDER: usize = 16;
pub const MAX_NB_SUBFR: usize = 4;
pub const LTP_ORDER: usize = 5;
pub const MAX_FRAMES_PER_PACKET: usize = 3;

pub const TYPE_NO_VOICE_ACTIVITY: i32 = 0;
pub const TYPE_VOICED: i32 = 2;

pub const CODE_INDEPENDENTLY: i32 = 0;
pub const CODE_INDEPENDENTLY_NO_LTP_SCALING: i32 = 1;
pub const CODE_CONDITIONALLY: i32 = 2;

pub const FLAG_DECODE_NORMAL: i32 = 0;
pub const FLAG_PACKET_LOST: i32 = 1;
pub const FLAG_DECODE_LBRR: i32 = 2;

#[derive(Clone, Copy, Default)]
pub struct SideInfoIndices {
    pub gains_indices: [i8; MAX_NB_SUBFR],
    pub ltp_index: [i8; MAX_NB_SUBFR],
    pub nlsf_indices: [i8; MAX_LPC_ORDER + 1],
    pub lag_index: i16,
    pub contour_index: i8,
    pub signal_type: i8,
    pub quant_offset_type: i8,
    pub nlsf_interp_coef_q2: i8,
    pub per_index: i8,
    pub ltp_scale_index: i8,
    pub seed: i8,
}

#[derive(Clone)]
pub struct PlcState {
    pub pitch_l_q8: i32,
    pub ltp_coef_q14: [i16; LTP_ORDER],
    pub prev_lpc_q12: [i16; MAX_LPC_ORDER],
    pub last_frame_lost: i32,
    pub rand_seed: i32,
    pub rand_scale_q14: i16,
    pub conc_energy: i32,
    pub conc_energy_shift: i32,
    pub prev_ltp_scale_q14: i16,
    pub prev_gain_q16: [i32; 2],
    pub fs_khz: i32,
    pub nb_subfr: i32,
    pub subfr_length: i32,
}

impl Default for PlcState {
    fn default() -> Self {
        PlcState {
            pitch_l_q8: 0,
            ltp_coef_q14: [0; LTP_ORDER],
            prev_lpc_q12: [0; MAX_LPC_ORDER],
            last_frame_lost: 0,
            rand_seed: 0,
            rand_scale_q14: 0,
            conc_energy: 0,
            conc_energy_shift: 0,
            prev_ltp_scale_q14: 0,
            prev_gain_q16: [0; 2],
            fs_khz: 0,
            nb_subfr: 0,
            subfr_length: 0,
        }
    }
}

#[derive(Clone)]
pub struct CngState {
    pub cng_exc_buf_q14: [i32; MAX_FRAME_LENGTH],
    pub cng_smth_nlsf_q15: [i16; MAX_LPC_ORDER],
    pub cng_synth_state: [i32; MAX_LPC_ORDER],
    pub cng_smth_gain_q16: i32,
    pub rand_seed: i32,
    pub fs_khz: i32,
}

impl Default for CngState {
    fn default() -> Self {
        CngState {
            cng_exc_buf_q14: [0; MAX_FRAME_LENGTH],
            cng_smth_nlsf_q15: [0; MAX_LPC_ORDER],
            cng_synth_state: [0; MAX_LPC_ORDER],
            cng_smth_gain_q16: 0,
            rand_seed: 0,
            fs_khz: 0,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct DecoderControl {
    pub pitch_l: [i32; MAX_NB_SUBFR],
    pub gains_q16: [i32; MAX_NB_SUBFR],
    pub pred_coef_q12: [[i16; MAX_LPC_ORDER]; 2],
    pub ltp_coef_q14: [i16; LTP_ORDER * MAX_NB_SUBFR],
    pub ltp_scale_q14: i32,
}

#[derive(Clone)]
pub struct ChannelState {
    pub prev_gain_q16: i32,
    pub exc_q14: [i32; MAX_FRAME_LENGTH],
    pub slpc_q14_buf: [i32; MAX_LPC_ORDER],
    pub out_buf: [i16; MAX_FRAME_LENGTH + 2 * MAX_SUB_FRAME_LENGTH],
    pub lag_prev: i32,
    pub last_gain_index: i8,
    pub fs_khz: i32,
    pub fs_api_hz: i32,
    pub nb_subfr: i32,
    pub frame_length: i32,
    pub subfr_length: i32,
    pub ltp_mem_length: i32,
    pub lpc_order: i32,
    pub prev_nlsf_q15: [i16; MAX_LPC_ORDER],
    pub first_frame_after_reset: i32,
    pub pitch_lag_low_bits_icdf: &'static [u8],
    pub pitch_contour_icdf: &'static [u8],
    pub n_frames_decoded: i32,
    pub n_frames_per_packet: i32,
    pub ec_prev_signal_type: i32,
    pub ec_prev_lag_index: i16,
    pub vad_flags: [i32; MAX_FRAMES_PER_PACKET],
    pub lbrr_flag: i32,
    pub lbrr_flags: [i32; MAX_FRAMES_PER_PACKET],
    pub resampler_state: Resampler,
    pub nlsf_cb: Option<&'static NlsfCb>,
    pub indices: SideInfoIndices,
    pub cng: CngState,
    pub loss_cnt: i32,
    pub prev_signal_type: i32,
    pub plc: PlcState,
}

impl Default for ChannelState {
    fn default() -> Self {
        ChannelState {
            prev_gain_q16: 0,
            exc_q14: [0; MAX_FRAME_LENGTH],
            slpc_q14_buf: [0; MAX_LPC_ORDER],
            out_buf: [0; MAX_FRAME_LENGTH + 2 * MAX_SUB_FRAME_LENGTH],
            lag_prev: 0,
            last_gain_index: 0,
            fs_khz: 0,
            fs_api_hz: 0,
            nb_subfr: 0,
            frame_length: 0,
            subfr_length: 0,
            ltp_mem_length: 0,
            lpc_order: 0,
            prev_nlsf_q15: [0; MAX_LPC_ORDER],
            first_frame_after_reset: 0,
            pitch_lag_low_bits_icdf: &[],
            pitch_contour_icdf: &[],
            n_frames_decoded: 0,
            n_frames_per_packet: 0,
            ec_prev_signal_type: 0,
            ec_prev_lag_index: 0,
            vad_flags: [0; MAX_FRAMES_PER_PACKET],
            lbrr_flag: 0,
            lbrr_flags: [0; MAX_FRAMES_PER_PACKET],
            resampler_state: Resampler::default(),
            nlsf_cb: None,
            indices: SideInfoIndices::default(),
            cng: CngState::default(),
            loss_cnt: 0,
            prev_signal_type: 0,
            plc: PlcState::default(),
        }
    }
}

impl ChannelState {
    pub fn reset(&mut self) -> i32 {
        *self = ChannelState::default();
        self.first_frame_after_reset = 1;
        self.prev_gain_q16 = 65536;
        super::cng::cng_reset(self);
        super::plc::plc_reset(self);
        0
    }

    pub fn init(&mut self) -> i32 {
        self.reset()
    }

    pub fn vad_flag(&self, i: i32) -> i32 {
        if i < 0 {
            return 0;
        }
        self.vad_flags.get(i as usize).copied().unwrap_or(0)
    }

    pub fn lbrr_flag_at(&self, i: i32) -> i32 {
        if i < 0 {
            return 0;
        }
        self.lbrr_flags.get(i as usize).copied().unwrap_or(0)
    }

    pub fn set_fs(&mut self, fs_khz: i32, fs_api_hz: i32) -> i32 {
        let mut ret = 0;
        self.subfr_length = 5 * fs_khz;
        let frame_length = self.nb_subfr * self.subfr_length;
        if self.fs_khz != fs_khz || self.fs_api_hz != fs_api_hz {
            ret += self.resampler_state.init(fs_khz * 1000, fs_api_hz, false);
            self.fs_api_hz = fs_api_hz;
        }
        if self.fs_khz != fs_khz || frame_length != self.frame_length {
            if fs_khz == 8 {
                if self.nb_subfr == MAX_NB_SUBFR as i32 {
                    self.pitch_contour_icdf = &PITCH_CONTOUR_NB_ICDF;
                } else {
                    self.pitch_contour_icdf = &PITCH_CONTOUR_10_MS_NB_ICDF;
                }
            } else if self.nb_subfr == MAX_NB_SUBFR as i32 {
                self.pitch_contour_icdf = &PITCH_CONTOUR_ICDF;
            } else {
                self.pitch_contour_icdf = &PITCH_CONTOUR_10_MS_ICDF;
            }
            if self.fs_khz != fs_khz {
                self.ltp_mem_length = 20 * fs_khz;
                if fs_khz == 8 || fs_khz == 12 {
                    self.lpc_order = 10;
                    self.nlsf_cb = Some(&NLSF_CB_NB_MB);
                } else {
                    self.lpc_order = 16;
                    self.nlsf_cb = Some(&NLSF_CB_WB);
                }
                if fs_khz == 16 {
                    self.pitch_lag_low_bits_icdf = &UNIFORM8_ICDF;
                } else if fs_khz == 12 {
                    self.pitch_lag_low_bits_icdf = &UNIFORM6_ICDF;
                } else {
                    self.pitch_lag_low_bits_icdf = &UNIFORM4_ICDF;
                }
                self.first_frame_after_reset = 1;
                self.lag_prev = 100;
                self.last_gain_index = 10;
                self.prev_signal_type = TYPE_NO_VOICE_ACTIVITY;
                self.out_buf = [0; MAX_FRAME_LENGTH + 2 * MAX_SUB_FRAME_LENGTH];
                self.slpc_q14_buf = [0; MAX_LPC_ORDER];
            }
            self.fs_khz = fs_khz;
            self.frame_length = frame_length;
        }
        ret
    }
}
