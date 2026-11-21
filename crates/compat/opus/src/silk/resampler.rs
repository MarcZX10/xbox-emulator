use super::macros::*;
use super::tables::*;

const RESAMPLER_MAX_BATCH_SIZE_MS: i32 = 10;
const RESAMPLER_DOWN_ORDER_FIR0: i32 = 18;
const RESAMPLER_DOWN_ORDER_FIR1: i32 = 24;
const RESAMPLER_DOWN_ORDER_FIR2: i32 = 36;
const RESAMPLER_ORDER_FIR_12: usize = 8;

const USE_COPY: i32 = 0;
const USE_UP2_HQ: i32 = 1;
const USE_IIR_FIR: i32 = 2;
const USE_DOWN_FIR: i32 = 3;

const DELAY_MATRIX_ENC: [[i8; 3]; 6] = [[6, 0, 3], [0, 7, 3], [0, 1, 10], [0, 2, 6], [18, 10, 12], [0, 0, 44]];
const DELAY_MATRIX_DEC: [[i8; 6]; 3] = [[4, 0, 2, 0, 0, 0], [0, 9, 4, 7, 4, 4], [0, 3, 12, 7, 7, 7]];

const MAX_BATCH_IN: usize = 480;

fn rate_id(r: i32) -> usize {
    let v = ((((r >> 12) - (r > 16000) as i32) >> (r > 24000) as i32) - 1).min(5);
    v.max(0) as usize
}

#[derive(Clone)]
pub struct Resampler {
    pub s_iir: [i32; 6],
    pub s_fir_i32: [i32; 36],
    pub s_fir_i16: [i16; 36],
    pub delay_buf: [i16; 96],
    pub resampler_function: i32,
    pub batch_size: i32,
    pub inv_ratio_q16: i32,
    pub fir_order: i32,
    pub fir_fracs: i32,
    pub fs_in_khz: i32,
    pub fs_out_khz: i32,
    pub input_delay: i32,
    pub coefs: &'static [i16],
}

impl Default for Resampler {
    fn default() -> Self {
        Resampler {
            s_iir: [0; 6],
            s_fir_i32: [0; 36],
            s_fir_i16: [0; 36],
            delay_buf: [0; 96],
            resampler_function: 0,
            batch_size: 0,
            inv_ratio_q16: 0,
            fir_order: 0,
            fir_fracs: 0,
            fs_in_khz: 0,
            fs_out_khz: 0,
            input_delay: 0,
            coefs: &[],
        }
    }
}

#[inline]
fn put(out: &mut [i16], i: usize, v: i16) {
    if let Some(o) = out.get_mut(i) {
        *o = v;
    }
}

pub fn up2_hq(s: &mut [i32; 6], out: &mut [i16], input: &[i16], len: usize) {
    let len = len.min(input.len());
    for k in 0..len {
        let in32 = lshift(input[k] as i32, 10);
        let y = sub32(in32, s[0]);
        let x = smulwb(y, RESAMPLER_UP2_HQ_0[0] as i32);
        let mut out32_1 = add32(s[0], x);
        s[0] = add32(in32, x);
        let y = sub32(out32_1, s[1]);
        let x = smulwb(y, RESAMPLER_UP2_HQ_0[1] as i32);
        let out32_2 = add32(s[1], x);
        s[1] = add32(out32_1, x);
        let y = sub32(out32_2, s[2]);
        let x = smlawb(y, y, RESAMPLER_UP2_HQ_0[2] as i32);
        out32_1 = add32(s[2], x);
        s[2] = add32(out32_2, x);
        put(out, 2 * k, sat16(rshift_round(out32_1, 10)) as i16);
        let y = sub32(in32, s[3]);
        let x = smulwb(y, RESAMPLER_UP2_HQ_1[0] as i32);
        let mut out32_1 = add32(s[3], x);
        s[3] = add32(in32, x);
        let y = sub32(out32_1, s[4]);
        let x = smulwb(y, RESAMPLER_UP2_HQ_1[1] as i32);
        let out32_2 = add32(s[4], x);
        s[4] = add32(out32_1, x);
        let y = sub32(out32_2, s[5]);
        let x = smlawb(y, y, RESAMPLER_UP2_HQ_1[2] as i32);
        out32_1 = add32(s[5], x);
        s[5] = add32(out32_2, x);
        put(out, 2 * k + 1, sat16(rshift_round(out32_1, 10)) as i16);
    }
}

fn ar2(s: &mut [i32; 6], out_q8: &mut [i32], input: &[i16], a_q14: &[i16], len: usize) {
    let a0 = a_q14.first().copied().unwrap_or(0) as i32;
    let a1 = a_q14.get(1).copied().unwrap_or(0) as i32;
    let len = len.min(input.len()).min(out_q8.len());
    for k in 0..len {
        let mut out32 = add_lshift32(s[0], input[k] as i32, 8);
        out_q8[k] = out32;
        out32 = lshift(out32, 2);
        s[0] = smlawb(s[1], out32, a0);
        s[1] = smulwb(out32, a1);
    }
}

fn iir_fir_interpol(out: &mut [i16], mut o: usize, buf: &[i16], max_index_q16: i32, incr: i32) -> usize {
    if incr <= 0 {
        return o;
    }
    let mut index_q16: i32 = 0;
    while index_q16 < max_index_q16 {
        let ti = smulwb(index_q16 & 0xFFFF, 12).clamp(0, 11) as usize;
        let p = (index_q16 >> 16) as usize;
        if p + 7 >= buf.len() {
            break;
        }
        let f = &RESAMPLER_FRAC_FIR_12;
        let g = |t: usize, j: usize| f[t * 4 + j] as i32;
        let mut res_q15 = smulbb(buf[p] as i32, g(ti, 0));
        res_q15 = smlabb(res_q15, buf[p + 1] as i32, g(ti, 1));
        res_q15 = smlabb(res_q15, buf[p + 2] as i32, g(ti, 2));
        res_q15 = smlabb(res_q15, buf[p + 3] as i32, g(ti, 3));
        res_q15 = smlabb(res_q15, buf[p + 4] as i32, g(11 - ti, 3));
        res_q15 = smlabb(res_q15, buf[p + 5] as i32, g(11 - ti, 2));
        res_q15 = smlabb(res_q15, buf[p + 6] as i32, g(11 - ti, 1));
        res_q15 = smlabb(res_q15, buf[p + 7] as i32, g(11 - ti, 0));
        put(out, o, sat16(rshift_round(res_q15, 15)) as i16);
        o += 1;
        index_q16 = index_q16.wrapping_add(incr);
        if index_q16 < 0 {
            break;
        }
    }
    o
}

fn down_fir_interpol(out: &mut [i16], mut o: usize, buf: &[i32], coefs: &[i16], order: i32, fracs: i32, max_index_q16: i32, incr: i32) -> usize {
    if incr <= 0 {
        return o;
    }
    let c = |i: usize| coefs.get(i).copied().unwrap_or(0) as i32;
    let ord = order.max(0) as usize;
    let mut index_q16: i32 = 0;
    while index_q16 < max_index_q16 {
        let p = (index_q16 >> 16) as usize;
        if ord == 0 || p + ord > buf.len() {
            break;
        }
        let b = &buf[p..p + ord];
        let mut res_q6: i32 = 0;
        if order == RESAMPLER_DOWN_ORDER_FIR0 {
            let ind = smulwb(index_q16 & 0xFFFF, fracs);
            let base1 = (9 * ind).max(0) as usize;
            for j in 0..9 {
                res_q6 = smlawb(res_q6, b[j], c(base1 + j));
            }
            let base2 = (9 * (fracs - 1 - ind)).max(0) as usize;
            for j in 0..9 {
                res_q6 = smlawb(res_q6, b[17 - j], c(base2 + j));
            }
        } else if order == RESAMPLER_DOWN_ORDER_FIR1 || order == RESAMPLER_DOWN_ORDER_FIR2 {
            for j in 0..ord / 2 {
                res_q6 = smlawb(res_q6, add32(b[j], b[ord - 1 - j]), c(j));
            }
        } else {
            break;
        }
        put(out, o, sat16(rshift_round(res_q6, 6)) as i16);
        o += 1;
        index_q16 = index_q16.wrapping_add(incr);
        if index_q16 < 0 {
            break;
        }
    }
    o
}

impl Resampler {
    pub fn init(&mut self, fs_hz_in: i32, fs_hz_out: i32, for_enc: bool) -> i32 {
        *self = Resampler::default();
        if for_enc {
            if ![8000, 12000, 16000, 24000, 48000].contains(&fs_hz_in) || ![8000, 12000, 16000].contains(&fs_hz_out) {
                return -1;
            }
            self.input_delay = DELAY_MATRIX_ENC[rate_id(fs_hz_in)][rate_id(fs_hz_out).min(2)] as i32;
        } else {
            if ![8000, 12000, 16000].contains(&fs_hz_in) || ![8000, 12000, 16000, 24000, 48000].contains(&fs_hz_out) {
                return -1;
            }
            self.input_delay = DELAY_MATRIX_DEC[rate_id(fs_hz_in).min(2)][rate_id(fs_hz_out)] as i32;
        }
        self.fs_in_khz = fs_hz_in / 1000;
        self.fs_out_khz = fs_hz_out / 1000;
        self.batch_size = self.fs_in_khz * RESAMPLER_MAX_BATCH_SIZE_MS;
        let mut up2x = 0;
        if fs_hz_out > fs_hz_in {
            if fs_hz_out == fs_hz_in * 2 {
                self.resampler_function = USE_UP2_HQ;
            } else {
                self.resampler_function = USE_IIR_FIR;
                up2x = 1;
            }
        } else if fs_hz_out < fs_hz_in {
            self.resampler_function = USE_DOWN_FIR;
            if fs_hz_out * 4 == fs_hz_in * 3 {
                self.fir_fracs = 3;
                self.fir_order = RESAMPLER_DOWN_ORDER_FIR0;
                self.coefs = &RESAMPLER_3_4_COEFS;
            } else if fs_hz_out * 3 == fs_hz_in * 2 {
                self.fir_fracs = 2;
                self.fir_order = RESAMPLER_DOWN_ORDER_FIR0;
                self.coefs = &RESAMPLER_2_3_COEFS;
            } else if fs_hz_out * 2 == fs_hz_in {
                self.fir_fracs = 1;
                self.fir_order = RESAMPLER_DOWN_ORDER_FIR1;
                self.coefs = &RESAMPLER_1_2_COEFS;
            } else if fs_hz_out * 3 == fs_hz_in {
                self.fir_fracs = 1;
                self.fir_order = RESAMPLER_DOWN_ORDER_FIR2;
                self.coefs = &RESAMPLER_1_3_COEFS;
            } else if fs_hz_out * 4 == fs_hz_in {
                self.fir_fracs = 1;
                self.fir_order = RESAMPLER_DOWN_ORDER_FIR2;
                self.coefs = &RESAMPLER_1_4_COEFS;
            } else if fs_hz_out * 6 == fs_hz_in {
                self.fir_fracs = 1;
                self.fir_order = RESAMPLER_DOWN_ORDER_FIR2;
                self.coefs = &RESAMPLER_1_6_COEFS;
            } else {
                return -1;
            }
        } else {
            self.resampler_function = USE_COPY;
        }
        self.inv_ratio_q16 = lshift(div32(lshift(fs_hz_in, 14 + up2x), fs_hz_out), 2);
        while smulww(self.inv_ratio_q16, fs_hz_out) < lshift(fs_hz_in, up2x) {
            self.inv_ratio_q16 += 1;
        }
        0
    }

    fn iir_fir(&mut self, out: &mut [i16], input: &[i16], in_len: usize) {
        let mut buf = [0i16; 2 * MAX_BATCH_IN + RESAMPLER_ORDER_FIR_12];
        buf[..RESAMPLER_ORDER_FIR_12].copy_from_slice(&self.s_fir_i16[..RESAMPLER_ORDER_FIR_12]);
        let incr = self.inv_ratio_q16;
        let batch = (self.batch_size.max(1) as usize).min(MAX_BATCH_IN);
        let mut in_len = in_len.min(input.len());
        let mut off = 0usize;
        let mut o = 0usize;
        let mut n;
        loop {
            n = in_len.min(batch);
            up2_hq(&mut self.s_iir, &mut buf[RESAMPLER_ORDER_FIR_12..RESAMPLER_ORDER_FIR_12 + 2 * n], &input[off..off + n], n);
            let max_index_q16 = lshift(n as i32, 17);
            o = iir_fir_interpol(out, o, &buf[..2 * n + RESAMPLER_ORDER_FIR_12], max_index_q16, incr);
            off += n;
            in_len -= n;
            if in_len > 0 && n > 0 {
                buf.copy_within(2 * n..2 * n + RESAMPLER_ORDER_FIR_12, 0);
            } else {
                break;
            }
        }
        self.s_fir_i16[..RESAMPLER_ORDER_FIR_12].copy_from_slice(&buf[2 * n..2 * n + RESAMPLER_ORDER_FIR_12]);
    }

    fn down_fir(&mut self, out: &mut [i16], input: &[i16], in_len: usize) {
        let order = (self.fir_order.max(0) as usize).min(36);
        let mut buf = [0i32; MAX_BATCH_IN + 36];
        buf[..order].copy_from_slice(&self.s_fir_i32[..order]);
        let coefs: &'static [i16] = self.coefs;
        let fir_coefs = coefs.get(2..).unwrap_or(&[]);
        let incr = self.inv_ratio_q16;
        let batch = (self.batch_size.max(1) as usize).min(MAX_BATCH_IN);
        let mut in_len = in_len.min(input.len());
        let mut off = 0usize;
        let mut o = 0usize;
        let mut n;
        loop {
            n = in_len.min(batch);
            ar2(&mut self.s_iir, &mut buf[order..order + n], &input[off..off + n], coefs, n);
            let max_index_q16 = lshift(n as i32, 16);
            o = down_fir_interpol(out, o, &buf[..order + n], fir_coefs, self.fir_order, self.fir_fracs, max_index_q16, incr);
            off += n;
            in_len -= n;
            if in_len > 1 && n > 0 {
                buf.copy_within(n..n + order, 0);
            } else {
                break;
            }
        }
        self.s_fir_i32[..order].copy_from_slice(&buf[n..n + order]);
    }

    pub fn process(&mut self, out: &mut [i16], input: &[i16], in_len: usize) -> i32 {
        let in_len = in_len.min(input.len());
        let fs_in = self.fs_in_khz.max(0) as usize;
        let fs_out = self.fs_out_khz.max(0) as usize;
        let delay = self.input_delay.max(0) as usize;
        if in_len < fs_in || delay > fs_in || fs_in > 96 {
            return -1;
        }
        let n_samples = fs_in - delay;
        self.delay_buf[delay..delay + n_samples].copy_from_slice(&input[..n_samples]);
        let delay_copy = self.delay_buf;
        let rest = &input[n_samples..in_len];
        let rest_len = in_len - fs_in;
        let mut empty: [i16; 0] = [];
        match self.resampler_function {
            USE_UP2_HQ => {
                up2_hq(&mut self.s_iir, out, &delay_copy[..fs_in], fs_in);
                let o2 = match out.get_mut(fs_out..) {
                    Some(s) => s,
                    None => &mut empty[..],
                };
                up2_hq(&mut self.s_iir, o2, rest, rest_len);
            }
            USE_IIR_FIR => {
                self.iir_fir(out, &delay_copy[..fs_in], fs_in);
                let o2 = match out.get_mut(fs_out..) {
                    Some(s) => s,
                    None => &mut empty[..],
                };
                self.iir_fir(o2, rest, rest_len);
            }
            USE_DOWN_FIR => {
                self.down_fir(out, &delay_copy[..fs_in], fs_in);
                let o2 = match out.get_mut(fs_out..) {
                    Some(s) => s,
                    None => &mut empty[..],
                };
                self.down_fir(o2, rest, rest_len);
            }
            _ => {
                for i in 0..fs_in {
                    put(out, i, delay_copy[i]);
                }
                for i in 0..rest_len {
                    put(out, fs_out + i, rest[i]);
                }
            }
        }
        self.delay_buf[..delay].copy_from_slice(&input[in_len - delay..in_len]);
        0
    }
}
