use super::bands::{anti_collapse, denormalise_bands, quant_all_bands};
use super::energy::{unquant_coarse_energy, unquant_energy_finalise, unquant_fine_energy};
use super::lpc::{celt_autocorr, celt_fir, celt_iir_inplace, celt_lpc, pitch_downsample, pitch_search, CELT_LPC_ORDER};
use super::mathops::{celt_lcg_rand, celt_sqrt, fmax, fmin};
use super::mdct::clt_mdct_backward;
use super::rate::{clt_compute_allocation, init_caps};
use super::tables::{EBANDS, WINDOW120};
use super::vq::{renormalise_vector, SPREAD_NORMAL};
use super::{BITRES, EFF_EBANDS, NB_EBANDS, SHORT_MDCT_SIZE};
use crate::entdec::Decoder;

pub const OPUS_OK: i32 = 0;
pub const OPUS_BAD_ARG: i32 = -1;
pub const OPUS_BUFFER_TOO_SMALL: i32 = -2;
pub const OPUS_INTERNAL_ERROR: i32 = -3;
pub const OPUS_INVALID_PACKET: i32 = -4;

pub const DECODE_BUFFER_SIZE: usize = 2048;
pub const OVERLAP: usize = 120;
pub const MAX_PERIOD: usize = 1024;
const MEM_LEN: usize = DECODE_BUFFER_SIZE + OVERLAP;
const MAX_LM: i32 = 3;
const COMBFILTER_MINPERIOD: i32 = 15;
const PLC_PITCH_LAG_MAX: i32 = 720;
const PLC_PITCH_LAG_MIN: i32 = 100;
const PREEMPH0: f32 = 0.85000610f32;
const VERY_SMALL: f32 = 1e-30f32;

const FRAME_NONE: i32 = 0;
const FRAME_NORMAL: i32 = 1;
const FRAME_PLC_NOISE: i32 = 2;
const FRAME_PLC_PERIODIC: i32 = 3;

static TF_SELECT_TABLE: [[i8; 8]; 4] = [
    [0, -1, 0, -1, 0, -1, 0, -1],
    [0, -1, 0, -2, 1, 0, 1, -1],
    [0, -2, 0, -3, 2, 0, 1, -1],
    [0, -2, 0, -3, 3, 0, 1, -1],
];
static TRIM_ICDF: [u8; 11] = [126, 124, 119, 109, 87, 41, 19, 9, 4, 2, 0];
static SPREAD_ICDF: [u8; 4] = [25, 23, 2, 0];
static TAPSET_ICDF: [u8; 3] = [2, 1, 0];
static COMB_GAINS: [[f32; 3]; 3] = [
    [0.3066406250f32, 0.2170410156f32, 0.1296386719f32],
    [0.4638671875f32, 0.2680664062f32, 0.0f32],
    [0.7998046875f32, 0.1000976562f32, 0.0f32],
];

fn resampling_factor(rate: i32) -> usize {
    match rate {
        48000 => 1,
        24000 => 2,
        16000 => 3,
        12000 => 4,
        8000 => 6,
        _ => 0,
    }
}

#[inline]
fn put(buf: &mut [f32], out: &mut Option<&mut [f32]>, x_off: usize, i: usize, v: f32) {
    match out {
        Some(o) => o[i] = v,
        None => buf[x_off + i] = v,
    }
}

#[allow(clippy::too_many_arguments)]
fn comb_filter(
    buf: &mut [f32],
    x_off: usize,
    mut out: Option<&mut [f32]>,
    t0: i32,
    t1: i32,
    n: usize,
    g0: f32,
    g1: f32,
    tapset0: i32,
    tapset1: i32,
    window: Option<&[f32]>,
    overlap: usize,
) {
    if x_off + n > buf.len() {
        return;
    }
    if let Some(o) = out.as_deref() {
        if o.len() < n {
            return;
        }
    }
    if g0 == 0.0 && g1 == 0.0 {
        if let Some(o) = out.as_deref_mut() {
            o[..n].copy_from_slice(&buf[x_off..x_off + n]);
        }
        return;
    }
    let t0 = t0.max(COMBFILTER_MINPERIOD) as usize;
    let t1 = t1.max(COMBFILTER_MINPERIOD) as usize;
    if x_off < t0.max(t1) + 2 {
        return;
    }
    let ts0 = tapset0.clamp(0, 2) as usize;
    let ts1 = tapset1.clamp(0, 2) as usize;
    let g00 = g0 * COMB_GAINS[ts0][0];
    let g01 = g0 * COMB_GAINS[ts0][1];
    let g02 = g0 * COMB_GAINS[ts0][2];
    let g10 = g1 * COMB_GAINS[ts1][0];
    let g11 = g1 * COMB_GAINS[ts1][1];
    let g12 = g1 * COMB_GAINS[ts1][2];
    let mut x1 = buf[x_off + 1 - t1];
    let mut x2 = buf[x_off - t1];
    let mut x3 = buf[x_off - t1 - 1];
    let mut x4 = buf[x_off - t1 - 2];
    let mut overlap = overlap.min(n);
    if g0 == g1 && t0 == t1 && ts0 == ts1 {
        overlap = 0;
    }
    let window = match window {
        Some(w) => w,
        None => &[][..],
    };
    if window.len() < overlap {
        overlap = 0;
    }
    let mut i = 0usize;
    while i < overlap {
        let b = x_off + i;
        let x0 = buf[b + 2 - t1];
        let f = window[i] * window[i];
        let omf = 1.0f32 - f;
        let v = buf[b]
            + (omf * g00) * buf[b - t0]
            + (omf * g01) * (buf[b + 1 - t0] + buf[b - t0 - 1])
            + (omf * g02) * (buf[b + 2 - t0] + buf[b - t0 - 2])
            + (f * g10) * x2
            + (f * g11) * (x1 + x3)
            + (f * g12) * (x0 + x4);
        put(buf, &mut out, x_off, i, v);
        x4 = x3;
        x3 = x2;
        x2 = x1;
        x1 = x0;
        i += 1;
    }
    if g1 == 0.0 {
        if let Some(o) = out.as_deref_mut() {
            o[overlap..n].copy_from_slice(&buf[x_off + overlap..x_off + n]);
        }
        return;
    }
    let base = x_off + i;
    let mut x4 = buf[base - t1 - 2];
    let mut x3 = buf[base - t1 - 1];
    let mut x2 = buf[base - t1];
    let mut x1 = buf[base + 1 - t1];
    for k in 0..n - i {
        let b = base + k;
        let x0 = buf[b + 2 - t1];
        let v = buf[b] + g10 * x2 + g11 * (x1 + x3) + g12 * (x0 + x4);
        put(buf, &mut out, x_off, i + k, v);
        x4 = x3;
        x3 = x2;
        x2 = x1;
        x1 = x0;
    }
}

#[allow(clippy::too_many_arguments)]
fn celt_synthesis(
    mem: &mut [Vec<f32>],
    x: &[f32],
    old_band_e: &[f32],
    start: i32,
    eff_end: i32,
    c: usize,
    cc: usize,
    is_transient: bool,
    lm: i32,
    downsample: usize,
    silence: bool,
) {
    let n = SHORT_MDCT_SIZE << lm;
    let m = 1i32 << lm;
    let (b_cnt, nb, shift) = if is_transient {
        (m as usize, SHORT_MDCT_SIZE, MAX_LM as usize)
    } else {
        (1usize, SHORT_MDCT_SIZE << lm, (MAX_LM - lm) as usize)
    };
    let out_base = DECODE_BUFFER_SIZE - n;
    let ds = downsample as i32;
    let mut freq = vec![0.0f32; n];
    if cc == 2 && c == 1 {
        denormalise_bands(&x[..n], &mut freq, &old_band_e[..NB_EBANDS], start, eff_end, m, ds, silence);
        for ch in 0..2 {
            for b in 0..b_cnt {
                clt_mdct_backward(&freq, b, &mut mem[ch][out_base + nb * b..], &WINDOW120, OVERLAP, shift, b_cnt);
            }
        }
    } else if cc == 1 && c == 2 {
        let mut freq2 = vec![0.0f32; n];
        denormalise_bands(&x[..n], &mut freq, &old_band_e[..NB_EBANDS], start, eff_end, m, ds, silence);
        denormalise_bands(&x[n..2 * n], &mut freq2, &old_band_e[NB_EBANDS..2 * NB_EBANDS], start, eff_end, m, ds, silence);
        for i in 0..n {
            freq[i] = 0.5f32 * freq[i] + 0.5f32 * freq2[i];
        }
        for b in 0..b_cnt {
            clt_mdct_backward(&freq, b, &mut mem[0][out_base + nb * b..], &WINDOW120, OVERLAP, shift, b_cnt);
        }
    } else {
        for ch in 0..cc {
            denormalise_bands(
                &x[ch * n..(ch + 1) * n],
                &mut freq,
                &old_band_e[ch * NB_EBANDS..(ch + 1) * NB_EBANDS],
                start,
                eff_end,
                m,
                ds,
                silence,
            );
            for b in 0..b_cnt {
                clt_mdct_backward(&freq, b, &mut mem[ch][out_base + nb * b..], &WINDOW120, OVERLAP, shift, b_cnt);
            }
        }
    }
}

fn tf_decode(start: i32, end: i32, is_transient: bool, tf_res: &mut [i32], lm: i32, dec: &mut Decoder) {
    let mut budget: u32 = dec.storage.wrapping_mul(8);
    let mut tell: u32 = dec.tell() as u32;
    let mut logp: u32 = if is_transient { 2 } else { 4 };
    let tf_select_rsv: u32 = (lm > 0 && tell.wrapping_add(logp).wrapping_add(1) <= budget) as u32;
    budget = budget.wrapping_sub(tf_select_rsv);
    let mut tf_changed = 0i32;
    let mut curr = 0i32;
    for i in start..end {
        if tell.wrapping_add(logp) <= budget {
            curr ^= dec.bit_logp(logp) as i32;
            tell = dec.tell() as u32;
            tf_changed |= curr;
        }
        tf_res[i as usize] = curr;
        logp = if is_transient { 4 } else { 5 };
    }
    let lmu = lm.clamp(0, 3) as usize;
    let tr = 4 * is_transient as usize;
    let mut tf_select = 0usize;
    if tf_select_rsv != 0
        && TF_SELECT_TABLE[lmu][tr + tf_changed as usize] != TF_SELECT_TABLE[lmu][tr + 2 + tf_changed as usize]
    {
        tf_select = dec.bit_logp(1) as usize;
    }
    for i in start..end {
        let iu = i as usize;
        tf_res[iu] = TF_SELECT_TABLE[lmu][tr + 2 * tf_select + (tf_res[iu] & 1) as usize] as i32;
    }
}

pub struct CeltDecoder {
    channels: usize,
    stream_channels: usize,
    downsample: usize,
    start: i32,
    end: i32,
    signalling: bool,
    disable_inv: bool,
    complexity: i32,
    rng: u32,
    error: i32,
    last_pitch_index: i32,
    plc_decay: [f32; 2],
    loss_duration: i32,
    plc_duration: i32,
    last_frame_type: i32,
    skip_plc: bool,
    postfilter_period: i32,
    postfilter_period_old: i32,
    postfilter_gain: f32,
    postfilter_gain_old: f32,
    postfilter_tapset: i32,
    postfilter_tapset_old: i32,
    prefilter_and_fold: bool,
    preemph_mem_d: [f32; 2],
    decode_mem: Vec<Vec<f32>>,
    old_band_e: [f32; 2 * NB_EBANDS],
    old_log_e: [f32; 2 * NB_EBANDS],
    old_log_e2: [f32; 2 * NB_EBANDS],
    background_log_e: [f32; 2 * NB_EBANDS],
    lpc: [f32; 2 * CELT_LPC_ORDER],
}

impl CeltDecoder {
    pub fn new(sampling_rate: i32, channels: usize) -> Result<CeltDecoder, i32> {
        if channels < 1 || channels > 2 {
            return Err(OPUS_BAD_ARG);
        }
        let downsample = resampling_factor(sampling_rate);
        if downsample == 0 {
            return Err(OPUS_BAD_ARG);
        }
        let mut st = CeltDecoder {
            channels,
            stream_channels: channels,
            downsample,
            start: 0,
            end: EFF_EBANDS as i32,
            signalling: true,
            disable_inv: channels == 1,
            complexity: 0,
            rng: 0,
            error: 0,
            last_pitch_index: 0,
            plc_decay: [0.0; 2],
            loss_duration: 0,
            plc_duration: 0,
            last_frame_type: FRAME_NONE,
            skip_plc: true,
            postfilter_period: 0,
            postfilter_period_old: 0,
            postfilter_gain: 0.0,
            postfilter_gain_old: 0.0,
            postfilter_tapset: 0,
            postfilter_tapset_old: 0,
            prefilter_and_fold: false,
            preemph_mem_d: [0.0; 2],
            decode_mem: vec![vec![0.0f32; MEM_LEN]; channels],
            old_band_e: [0.0; 2 * NB_EBANDS],
            old_log_e: [0.0; 2 * NB_EBANDS],
            old_log_e2: [0.0; 2 * NB_EBANDS],
            background_log_e: [0.0; 2 * NB_EBANDS],
            lpc: [0.0; 2 * CELT_LPC_ORDER],
        };
        st.reset();
        Ok(st)
    }

    pub fn reset(&mut self) {
        self.rng = 0;
        self.error = 0;
        self.last_pitch_index = 0;
        self.plc_decay = [0.0; 2];
        self.loss_duration = 0;
        self.plc_duration = 0;
        self.postfilter_period = 0;
        self.postfilter_period_old = 0;
        self.postfilter_gain = 0.0;
        self.postfilter_gain_old = 0.0;
        self.postfilter_tapset = 0;
        self.postfilter_tapset_old = 0;
        self.prefilter_and_fold = false;
        self.preemph_mem_d = [0.0; 2];
        for m in self.decode_mem.iter_mut() {
            for v in m.iter_mut() {
                *v = 0.0;
            }
        }
        self.old_band_e = [0.0; 2 * NB_EBANDS];
        self.old_log_e = [-28.0; 2 * NB_EBANDS];
        self.old_log_e2 = [-28.0; 2 * NB_EBANDS];
        self.background_log_e = [0.0; 2 * NB_EBANDS];
        self.lpc = [0.0; 2 * CELT_LPC_ORDER];
        self.skip_plc = true;
        self.last_frame_type = FRAME_NONE;
    }

    pub fn set_start_band(&mut self, v: i32) {
        if (0..NB_EBANDS as i32).contains(&v) {
            self.start = v;
        }
    }

    pub fn set_end_band(&mut self, v: i32) {
        if v >= 1 && v <= NB_EBANDS as i32 {
            self.end = v;
        }
    }

    pub fn set_stream_channels(&mut self, c: usize) {
        if c == 1 || c == 2 {
            self.stream_channels = c;
        }
    }

    pub fn set_signalling(&mut self, on: bool) {
        self.signalling = on;
    }

    pub fn signalling(&self) -> bool {
        self.signalling
    }

    pub fn set_complexity(&mut self, v: i32) {
        if (0..=10).contains(&v) {
            self.complexity = v;
        }
    }

    pub fn set_phase_inversion_disabled(&mut self, v: bool) {
        self.disable_inv = v;
    }

    pub fn final_range(&self) -> u32 {
        self.rng
    }

    pub fn pitch(&self) -> i32 {
        self.postfilter_period
    }

    pub fn lookahead(&self) -> i32 {
        (OVERLAP / self.downsample) as i32
    }

    pub fn get_and_clear_error(&mut self) -> i32 {
        let e = self.error;
        self.error = 0;
        e
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn window() -> &'static [f32] {
        &WINDOW120
    }

    pub fn overlap() -> usize {
        OVERLAP
    }

    fn deemphasis(&mut self, n: usize, pcm: &mut [f32], accum: bool) {
        let cc = self.channels;
        let ds = self.downsample;
        let nd = n / ds;
        let base = DECODE_BUFFER_SIZE - n;
        let coef0 = PREEMPH0;
        let mut scratch = vec![0.0f32; n];
        for c in 0..cc {
            let x = &self.decode_mem[c][base..base + n];
            let mut m = self.preemph_mem_d[c];
            if ds > 1 {
                for j in 0..n {
                    let tmp = x[j] + VERY_SMALL + m;
                    m = coef0 * tmp;
                    scratch[j] = tmp;
                }
                if accum {
                    for j in 0..nd {
                        pcm[j * cc + c] += (1.0f32 / 32768.0f32) * scratch[j * ds];
                    }
                } else {
                    for j in 0..nd {
                        pcm[j * cc + c] = (1.0f32 / 32768.0f32) * scratch[j * ds];
                    }
                }
            } else if accum {
                for j in 0..n {
                    let tmp = x[j] + m + VERY_SMALL;
                    m = coef0 * tmp;
                    pcm[j * cc + c] += (1.0f32 / 32768.0f32) * tmp;
                }
            } else {
                for j in 0..n {
                    let tmp = x[j] + VERY_SMALL + m;
                    m = coef0 * tmp;
                    pcm[j * cc + c] = (1.0f32 / 32768.0f32) * tmp;
                }
            }
            self.preemph_mem_d[c] = m;
        }
    }

    fn prefilter_and_fold(&mut self, n: usize) {
        let mut etmp = [0.0f32; OVERLAP];
        let x_off = DECODE_BUFFER_SIZE - n;
        for c in 0..self.channels {
            let buf = &mut self.decode_mem[c];
            comb_filter(
                buf,
                x_off,
                Some(&mut etmp[..]),
                self.postfilter_period_old,
                self.postfilter_period,
                OVERLAP,
                -self.postfilter_gain_old,
                -self.postfilter_gain,
                self.postfilter_tapset_old,
                self.postfilter_tapset,
                None,
                0,
            );
            for i in 0..OVERLAP / 2 {
                buf[x_off + i] = WINDOW120[i] * etmp[OVERLAP - 1 - i] + WINDOW120[OVERLAP - i - 1] * etmp[i];
            }
        }
    }

    fn plc_pitch_search(&self) -> i32 {
        let mut lp = vec![0.0f32; DECODE_BUFFER_SIZE >> 1];
        let chans: Vec<&[f32]> = self.decode_mem.iter().map(|v| &v[..]).collect();
        pitch_downsample(&chans, &mut lp, DECODE_BUFFER_SIZE >> 1, self.channels);
        let pitch = pitch_search(
            &lp[(PLC_PITCH_LAG_MAX >> 1) as usize..],
            &lp,
            DECODE_BUFFER_SIZE - PLC_PITCH_LAG_MAX as usize,
            (PLC_PITCH_LAG_MAX - PLC_PITCH_LAG_MIN) as usize,
        );
        PLC_PITCH_LAG_MAX - pitch
    }

    fn postfilter_frame(&mut self, n: usize, lm: i32, new_period: i32, new_gain: f32, new_tapset: i32) {
        let x_off = DECODE_BUFFER_SIZE - n;
        for c in 0..self.channels {
            self.postfilter_period = self.postfilter_period.max(COMBFILTER_MINPERIOD);
            self.postfilter_period_old = self.postfilter_period_old.max(COMBFILTER_MINPERIOD);
            let buf = &mut self.decode_mem[c];
            comb_filter(
                buf,
                x_off,
                None,
                self.postfilter_period_old,
                self.postfilter_period,
                SHORT_MDCT_SIZE,
                self.postfilter_gain_old,
                self.postfilter_gain,
                self.postfilter_tapset_old,
                self.postfilter_tapset,
                Some(&WINDOW120),
                OVERLAP,
            );
            if lm != 0 {
                comb_filter(
                    buf,
                    x_off + SHORT_MDCT_SIZE,
                    None,
                    self.postfilter_period,
                    new_period,
                    n - SHORT_MDCT_SIZE,
                    self.postfilter_gain,
                    new_gain,
                    self.postfilter_tapset,
                    new_tapset,
                    Some(&WINDOW120),
                    OVERLAP,
                );
            }
        }
    }

    fn decode_lost(&mut self, n: usize, lm: i32) {
        let cc = self.channels;
        let loss_duration = self.loss_duration;
        let start = self.start;
        let mut curr_frame_type = FRAME_PLC_PERIODIC;
        if self.plc_duration >= 40 || start != 0 || self.skip_plc {
            curr_frame_type = FRAME_PLC_NOISE;
        }
        let out_base = DECODE_BUFFER_SIZE - n;
        if curr_frame_type == FRAME_PLC_NOISE {
            let end = self.end;
            let eff_end = start.max(end.min(EFF_EBANDS as i32));
            let mut x = vec![0.0f32; cc * n];
            for c in 0..cc {
                self.decode_mem[c].copy_within(n..MEM_LEN, 0);
            }
            if self.prefilter_and_fold {
                self.prefilter_and_fold(n);
            }
            let mut decay = (1i32 << lm) as f32 * 0.0625f32;
            if self.last_frame_type != FRAME_PLC_NOISE {
                decay += fmax(1.0f32, loss_duration.min(160) as f32 * 0.0625f32);
            }
            for c in 0..cc {
                for i in start..end {
                    let idx = c * NB_EBANDS + i as usize;
                    self.old_band_e[idx] = fmax(self.background_log_e[idx], self.old_band_e[idx] - decay);
                }
            }
            let mut seed = self.rng;
            for c in 0..cc {
                for i in start..eff_end {
                    let iu = i as usize;
                    let boffs = n * c + ((EBANDS[iu] as usize) << lm);
                    let blen = ((EBANDS[iu + 1] - EBANDS[iu]) as usize) << lm;
                    for j in 0..blen {
                        seed = celt_lcg_rand(seed);
                        x[boffs + j] = ((seed as i32) >> 20) as f32;
                    }
                    renormalise_vector(&mut x[boffs..boffs + blen], blen, 1.0f32);
                }
            }
            self.rng = seed;
            let obe = self.old_band_e;
            celt_synthesis(&mut self.decode_mem, &x, &obe, start, eff_end, cc, cc, false, lm, self.downsample, false);
            let (p, g, t) = (self.postfilter_period, self.postfilter_gain, self.postfilter_tapset);
            self.postfilter_frame(n, lm, p.max(COMBFILTER_MINPERIOD), g, t);
            self.postfilter_period_old = self.postfilter_period;
            self.postfilter_gain_old = self.postfilter_gain;
            self.postfilter_tapset_old = self.postfilter_tapset;
            self.prefilter_and_fold = false;
            self.skip_plc = true;
        } else {
            let mut fade = 1.0f32;
            let mut pitch_index;
            let first = self.last_frame_type != FRAME_PLC_PERIODIC;
            if first {
                pitch_index = self.plc_pitch_search();
                self.last_pitch_index = pitch_index;
            } else {
                pitch_index = self.last_pitch_index;
                if self.plc_duration.wrapping_mul(SHORT_MDCT_SIZE as i32) >= pitch_index {
                    fade = 0.8f32;
                }
            }
            pitch_index = pitch_index.clamp(1, PLC_PITCH_LAG_MAX);
            let exc_length = (MAX_PERIOD).max(2 * pitch_index as usize);
            let mut sig = vec![0.0f32; exc_length + CELT_LPC_ORDER];
            let mut exc = vec![0.0f32; exc_length];
            let mut old_overlap = [0.0f32; OVERLAP / 2];
            let mut etmp = [0.0f32; OVERLAP];
            let window = &WINDOW120;
            for c in 0..cc {
                let mut s1 = 0.0f32;
                let mut pitch_index = self.last_pitch_index.clamp(1, PLC_PITCH_LAG_MAX) as usize;
                {
                    let buf = &self.decode_mem[c];
                    let b0 = DECODE_BUFFER_SIZE - exc_length - CELT_LPC_ORDER;
                    sig.copy_from_slice(&buf[b0..b0 + exc_length + CELT_LPC_ORDER]);
                }
                let lpc_c = c * CELT_LPC_ORDER;
                if first {
                    let mut ac = [0.0f32; CELT_LPC_ORDER + 1];
                    let s_off = CELT_LPC_ORDER + exc_length - MAX_PERIOD;
                    celt_autocorr(&sig[s_off..s_off + MAX_PERIOD], &mut ac, Some(&window[..]), OVERLAP, CELT_LPC_ORDER, MAX_PERIOD);
                    ac[0] *= 1.0001f32;
                    for i in 1..=CELT_LPC_ORDER {
                        ac[i] -= ac[i] * (0.008f32 * 0.008f32) * i as f32 * i as f32;
                    }
                    celt_lpc(&mut self.lpc[lpc_c..lpc_c + CELT_LPC_ORDER], &ac, CELT_LPC_ORDER);
                }
                celt_fir(&sig, CELT_LPC_ORDER, &self.lpc[lpc_c..lpc_c + CELT_LPC_ORDER], &mut exc, exc_length, CELT_LPC_ORDER);
                let decay;
                if first {
                    let mut e1 = 1.0f32;
                    let mut e2 = 1.0f32;
                    for i in 0..pitch_index {
                        let e = exc[exc_length - pitch_index + i];
                        e1 += e * e;
                        let e = exc[exc_length - 2 * pitch_index + i];
                        e2 += e * e;
                    }
                    e1 = fmin(e1, e2);
                    decay = celt_sqrt(e1 / e2);
                    self.plc_decay[c] = decay;
                } else {
                    decay = self.plc_decay[c];
                }
                {
                    let mut e0 = 1.0f32;
                    let kk = exc_length / pitch_index;
                    for i in 0..pitch_index {
                        let e = exc[exc_length - pitch_index + i];
                        e0 += e * e;
                    }
                    let mut k = 2usize;
                    while k <= kk {
                        let mut ek = 1.0f32;
                        for i in 0..pitch_index {
                            let e = exc[exc_length - k * pitch_index + i];
                            ek += e * e;
                        }
                        if ek < 0.25f32 * e0 || e0 < 0.25f32 * ek {
                            break;
                        }
                        k += 1;
                    }
                    pitch_index *= k - 1;
                }
                let buf = &mut self.decode_mem[c];
                if self.last_frame_type == FRAME_NORMAL {
                    old_overlap.copy_from_slice(&buf[DECODE_BUFFER_SIZE..DECODE_BUFFER_SIZE + OVERLAP / 2]);
                }
                buf.copy_within(n..DECODE_BUFFER_SIZE, 0);
                let extrapolation_offset = exc_length - pitch_index;
                let extrapolation_len = n + OVERLAP;
                let mut attenuation = fade * decay;
                let mut j = 0usize;
                for i in 0..extrapolation_len {
                    if j >= pitch_index {
                        j -= pitch_index;
                        attenuation *= decay;
                    }
                    buf[out_base + i] = attenuation * exc[extrapolation_offset + j];
                    let tmp = attenuation * sig[CELT_LPC_ORDER + extrapolation_offset + j];
                    s1 += tmp * tmp;
                    j += 1;
                }
                let mut lpc_mem = [0.0f32; CELT_LPC_ORDER];
                for i in 0..CELT_LPC_ORDER {
                    lpc_mem[i] = buf[out_base - 1 - i];
                }
                celt_iir_inplace(
                    &mut buf[out_base..out_base + extrapolation_len],
                    &self.lpc[lpc_c..lpc_c + CELT_LPC_ORDER],
                    extrapolation_len,
                    CELT_LPC_ORDER,
                    &mut lpc_mem,
                );
                let mut s2 = 0.0f32;
                for i in 0..extrapolation_len {
                    let tmp = buf[out_base + i];
                    s2 += tmp * tmp;
                }
                #[allow(clippy::neg_cmp_op_on_partial_ord)]
                let explode = !(s1 > 0.2f32 * s2);
                if explode {
                    for i in 0..extrapolation_len {
                        buf[out_base + i] = 0.0;
                    }
                } else if s1 < s2 {
                    let ratio = celt_sqrt((s1 + 1.0f32) / (s2 + 1.0f32));
                    for i in 0..OVERLAP {
                        let tmp_g = 1.0f32 - window[i] * (1.0f32 - ratio);
                        buf[out_base + i] *= tmp_g;
                    }
                    for i in OVERLAP..extrapolation_len {
                        buf[out_base + i] *= ratio;
                    }
                }
                if self.last_frame_type == FRAME_NORMAL {
                    comb_filter(
                        buf,
                        out_base,
                        Some(&mut etmp[..]),
                        self.postfilter_period_old,
                        self.postfilter_period_old,
                        OVERLAP,
                        -self.postfilter_gain_old,
                        -self.postfilter_gain_old,
                        self.postfilter_tapset_old,
                        self.postfilter_tapset_old,
                        None,
                        0,
                    );
                    for i in 0..OVERLAP / 2 {
                        let w1 = window[i];
                        let w2 = window[OVERLAP - 1 - i];
                        let x2 = old_overlap[i];
                        let x1 = w2 * etmp[OVERLAP - 1 - i] - w1 * etmp[i];
                        buf[out_base + i] = w2 * x2 - w1 * x1;
                        buf[out_base + OVERLAP - 1 - i] = w1 * x2 + w2 * x1;
                    }
                    comb_filter(
                        buf,
                        out_base,
                        None,
                        self.postfilter_period_old,
                        self.postfilter_period_old,
                        OVERLAP,
                        self.postfilter_gain_old,
                        self.postfilter_gain_old,
                        self.postfilter_tapset_old,
                        self.postfilter_tapset_old,
                        None,
                        0,
                    );
                }
            }
            self.prefilter_and_fold = true;
        }
        self.loss_duration = 10000.min(loss_duration.wrapping_add(1 << lm));
        self.plc_duration = 10000.min(self.plc_duration.wrapping_add(1 << lm));
        self.last_frame_type = curr_frame_type;
    }

    pub fn decode_with_ec(
        &mut self,
        data: Option<&[u8]>,
        pcm: &mut [f32],
        frame_size: usize,
        dec: Option<&mut Decoder>,
        accum: bool,
    ) -> i32 {
        let cc = self.channels;
        if self.start < 0 || self.start >= self.end || self.end > NB_EBANDS as i32 {
            return OPUS_BAD_ARG;
        }
        let fs = frame_size.wrapping_mul(self.downsample);
        let mut lm = 0i32;
        while lm <= MAX_LM {
            if SHORT_MDCT_SIZE << lm == fs {
                break;
            }
            lm += 1;
        }
        if lm > MAX_LM {
            return OPUS_BAD_ARG;
        }
        let len = data.map(|d| d.len()).unwrap_or(0);
        if len > 1275 {
            return OPUS_BAD_ARG;
        }
        let n = SHORT_MDCT_SIZE << lm;
        let out_per_ch = n / self.downsample;
        if pcm.len() < cc * out_per_ch {
            return OPUS_BAD_ARG;
        }
        let data = match data {
            Some(d) if len > 1 => d,
            _ => {
                self.decode_lost(n, lm);
                self.deemphasis(n, pcm, accum);
                return out_per_ch as i32;
            }
        };
        if self.loss_duration == 0 {
            self.skip_plc = false;
        }
        match dec {
            Some(d) => self.decode_body(data, len, pcm, n, lm, d, accum),
            None => {
                let mut local = Decoder::new(data);
                self.decode_body(data, len, pcm, n, lm, &mut local, accum)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_body(
        &mut self,
        _data: &[u8],
        len: usize,
        pcm: &mut [f32],
        n: usize,
        lm: i32,
        dec: &mut Decoder,
        accum: bool,
    ) -> i32 {
        let cc = self.channels;
        let c = self.stream_channels;
        let start = self.start;
        let end = self.end;
        let m = 1i32 << lm;
        let eff_end = end.min(EFF_EBANDS as i32);
        let len_i = len as i32;

        if c == 1 {
            for i in 0..NB_EBANDS {
                self.old_band_e[i] = fmax(self.old_band_e[i], self.old_band_e[NB_EBANDS + i]);
            }
        }

        let mut total_bits: i32 = len_i * 8;
        let mut tell = dec.tell();
        let silence = if tell >= total_bits {
            true
        } else if tell == 1 {
            dec.bit_logp(15)
        } else {
            false
        };
        if silence {
            tell = len_i * 8;
            dec.nbits_total = dec.nbits_total.wrapping_add(tell.wrapping_sub(dec.tell()));
        }

        let mut postfilter_gain = 0.0f32;
        let mut postfilter_pitch = 0i32;
        let mut postfilter_tapset = 0i32;
        if start == 0 && tell.wrapping_add(16) <= total_bits {
            if dec.bit_logp(1) {
                let octave = dec.dec_uint(6).min(5);
                postfilter_pitch = ((16u32 << octave).wrapping_add(dec.dec_bits(4 + octave)) as i32).wrapping_sub(1);
                let qg = dec.dec_bits(3) as i32;
                if dec.tell().wrapping_add(2) <= total_bits {
                    postfilter_tapset = dec.icdf(&TAPSET_ICDF, 2);
                }
                postfilter_gain = 0.09375f32 * (qg + 1) as f32;
            }
            tell = dec.tell();
        }

        let is_transient = if lm > 0 && tell.wrapping_add(3) <= total_bits {
            let t = dec.bit_logp(3);
            tell = dec.tell();
            t
        } else {
            false
        };
        let short_blocks = is_transient;

        let intra_ener = if tell.wrapping_add(3) <= total_bits { dec.bit_logp(3) } else { false };
        if !intra_ener && self.loss_duration != 0 {
            for ch in 0..2 {
                let missing = 10.min(self.loss_duration >> lm);
                let safety = if lm == 0 {
                    1.5f32
                } else if lm == 1 {
                    0.5f32
                } else {
                    0.0f32
                };
                for i in start..end {
                    let idx = ch * NB_EBANDS + i as usize;
                    if self.old_band_e[idx] < fmax(self.old_log_e[idx], self.old_log_e2[idx]) {
                        let mut e0 = self.old_band_e[idx];
                        let e1 = self.old_log_e[idx];
                        let e2 = self.old_log_e2[idx];
                        let mut slope = fmax(e1 - e0, 0.5f32 * (e2 - e0));
                        slope = fmin(slope, 2.0f32);
                        e0 -= fmax(0.0f32, (1 + missing) as f32 * slope);
                        self.old_band_e[idx] = fmax(-20.0f32, e0);
                    } else {
                        self.old_band_e[idx] = fmin(fmin(self.old_band_e[idx], self.old_log_e[idx]), self.old_log_e2[idx]);
                    }
                    self.old_band_e[idx] -= safety;
                }
            }
        }

        unquant_coarse_energy(start as usize, end as usize, &mut self.old_band_e, intra_ener, dec, c, lm as usize);

        let mut tf_res = [0i32; NB_EBANDS];
        tf_decode(start, end, is_transient, &mut tf_res, lm, dec);

        tell = dec.tell();
        let mut spread_decision = SPREAD_NORMAL;
        if tell.wrapping_add(4) <= total_bits {
            spread_decision = dec.icdf(&SPREAD_ICDF, 5);
        }

        let mut cap = [0i32; NB_EBANDS];
        init_caps(&mut cap, lm, c as i32);

        let mut offsets = [0i32; NB_EBANDS];
        let mut dynalloc_logp = 6i32;
        total_bits <<= BITRES;
        let mut tellf = dec.tell_frac() as i32;
        for i in start..end {
            let iu = i as usize;
            let width = ((c as i32) * (EBANDS[iu + 1] - EBANDS[iu]) as i32) << lm;
            let quanta = (width << BITRES).min((6 << BITRES).max(width));
            let mut dynalloc_loop_logp = dynalloc_logp;
            let mut boost = 0i32;
            while tellf.wrapping_add(dynalloc_loop_logp << BITRES) < total_bits && boost < cap[iu] {
                let flag = dec.bit_logp(dynalloc_loop_logp as u32);
                tellf = dec.tell_frac() as i32;
                if !flag {
                    break;
                }
                boost += quanta;
                total_bits -= quanta;
                dynalloc_loop_logp = 1;
            }
            offsets[iu] = boost;
            if boost > 0 {
                dynalloc_logp = 2.max(dynalloc_logp - 1);
            }
        }

        let mut fine_quant = [0i32; NB_EBANDS];
        let alloc_trim = if tellf.wrapping_add(6 << BITRES) <= total_bits { dec.icdf(&TRIM_ICDF, 7) } else { 5 };

        let mut bits: i32 = ((len_i * 8) << BITRES).wrapping_sub(dec.tell_frac() as i32).wrapping_sub(1);
        let anti_collapse_rsv = if is_transient && lm >= 2 && bits >= ((lm + 2) << BITRES) { 1 << BITRES } else { 0 };
        bits -= anti_collapse_rsv;

        let mut pulses = [0i32; NB_EBANDS];
        let mut fine_priority = [0i32; NB_EBANDS];
        let mut intensity = 0i32;
        let mut dual_stereo = 0i32;
        let mut balance = 0i32;
        let coded_bands = clt_compute_allocation(
            start,
            end,
            &offsets,
            &cap,
            alloc_trim,
            &mut intensity,
            &mut dual_stereo,
            bits,
            &mut balance,
            &mut pulses,
            &mut fine_quant,
            &mut fine_priority,
            c as i32,
            lm,
            dec,
        );

        unquant_fine_energy(start as usize, end as usize, &mut self.old_band_e, &fine_quant, dec, c);

        let mut x = vec![0.0f32; c * n];

        for ch in 0..cc {
            self.decode_mem[ch].copy_within(n..MEM_LEN, 0);
        }

        let mut collapse_masks = vec![0u8; c * NB_EBANDS];
        {
            let (xa, xb) = x.split_at_mut(n);
            let y = if c == 2 { Some(&mut xb[..n]) } else { None };
            quant_all_bands(
                start,
                end,
                xa,
                y,
                &mut collapse_masks,
                &pulses,
                short_blocks,
                spread_decision,
                dual_stereo != 0,
                intensity,
                &tf_res,
                (len_i * (8 << BITRES)).wrapping_sub(anti_collapse_rsv),
                balance,
                dec,
                lm,
                coded_bands,
                &mut self.rng,
                self.disable_inv,
            );
        }

        let mut anti_collapse_on = false;
        if anti_collapse_rsv > 0 {
            anti_collapse_on = dec.dec_bits(1) != 0;
        }
        let bits_left = (len_i * 8).wrapping_sub(dec.tell());
        unquant_energy_finalise(start as usize, end as usize, &mut self.old_band_e, &fine_quant, &fine_priority, bits_left, dec, c);
        if anti_collapse_on {
            anti_collapse(
                &mut x,
                &collapse_masks,
                lm,
                c,
                n,
                start,
                end,
                &self.old_band_e,
                &self.old_log_e,
                &self.old_log_e2,
                &pulses,
                self.rng,
            );
        }

        if silence {
            for i in 0..c * NB_EBANDS {
                self.old_band_e[i] = -28.0f32;
            }
        }
        if self.prefilter_and_fold {
            self.prefilter_and_fold(n);
        }
        let obe = self.old_band_e;
        celt_synthesis(&mut self.decode_mem, &x, &obe, start, eff_end, c, cc, is_transient, lm, self.downsample, silence);

        self.postfilter_frame(n, lm, postfilter_pitch, postfilter_gain, postfilter_tapset);
        self.postfilter_period_old = self.postfilter_period;
        self.postfilter_gain_old = self.postfilter_gain;
        self.postfilter_tapset_old = self.postfilter_tapset;
        self.postfilter_period = postfilter_pitch;
        self.postfilter_gain = postfilter_gain;
        self.postfilter_tapset = postfilter_tapset;
        if lm != 0 {
            self.postfilter_period_old = self.postfilter_period;
            self.postfilter_gain_old = self.postfilter_gain;
            self.postfilter_tapset_old = self.postfilter_tapset;
        }

        if c == 1 {
            let (a, b) = self.old_band_e.split_at_mut(NB_EBANDS);
            b.copy_from_slice(a);
        }

        if !is_transient {
            self.old_log_e2 = self.old_log_e;
            self.old_log_e = self.old_band_e;
        } else {
            for i in 0..2 * NB_EBANDS {
                self.old_log_e[i] = fmin(self.old_log_e[i], self.old_band_e[i]);
            }
        }
        let max_background_increase = 160.min(self.loss_duration.wrapping_add(m)) as f32 * 0.001f32;
        for i in 0..2 * NB_EBANDS {
            self.background_log_e[i] = fmin(self.background_log_e[i] + max_background_increase, self.old_band_e[i]);
        }
        for ch in 0..2 {
            for i in 0..start as usize {
                self.old_band_e[ch * NB_EBANDS + i] = 0.0;
                self.old_log_e[ch * NB_EBANDS + i] = -28.0;
                self.old_log_e2[ch * NB_EBANDS + i] = -28.0;
            }
            for i in end as usize..NB_EBANDS {
                self.old_band_e[ch * NB_EBANDS + i] = 0.0;
                self.old_log_e[ch * NB_EBANDS + i] = -28.0;
                self.old_log_e2[ch * NB_EBANDS + i] = -28.0;
            }
        }
        self.rng = dec.rng;

        self.deemphasis(n, pcm, accum);
        self.loss_duration = 0;
        self.plc_duration = 0;
        self.last_frame_type = FRAME_NORMAL;
        self.prefilter_and_fold = false;
        if dec.tell() > 8 * len_i {
            return OPUS_INTERNAL_ERROR;
        }
        if dec.get_error() != 0 {
            self.error = 1;
        }
        (n / self.downsample) as i32
    }
}
