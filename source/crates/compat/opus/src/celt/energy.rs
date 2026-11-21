use super::rate::MAX_FINE_BITS;
use super::tables::E_PROB_MODEL;
use super::NB_EBANDS;
use crate::entdec::Decoder;

pub static E_MEANS: [f32; 25] = [
    6.437500, 6.250000, 5.750000, 5.312500, 5.062500, 4.812500, 4.500000, 4.375000, 4.875000, 4.687500,
    4.562500, 4.437500, 4.875000, 4.625000, 4.312500, 4.500000, 4.375000, 4.625000, 4.750000, 4.437500,
    3.750000, 3.750000, 3.750000, 3.750000, 3.750000,
];

static PRED_COEF: [f64; 4] = [29440.0 / 32768.0, 26112.0 / 32768.0, 21248.0 / 32768.0, 16384.0 / 32768.0];
static BETA_COEF: [f64; 4] = [30147.0 / 32768.0, 22282.0 / 32768.0, 12124.0 / 32768.0, 6554.0 / 32768.0];
const BETA_INTRA: f64 = 4915.0 / 32768.0;
static SMALL_ENERGY_ICDF: [u8; 3] = [2, 1, 0];

pub fn safe_update(dec: &mut Decoder, fl: u32, fh: u32, ft: u32) {
    let fh = fh.min(ft);
    let fl = fl.min(fh);
    dec.update(fl, fh, ft);
}

fn laplace_get_freq1(fs0: u32, decay: i32) -> u32 {
    let ft = (32768u32 - 32).wrapping_sub(fs0);
    ft.wrapping_mul((16384 - decay) as u32) >> 15
}

pub fn laplace_decode(dec: &mut Decoder, fs: u32, decay: i32) -> i32 {
    let mut val: i32 = 0;
    let mut fs = fs;
    let fm = dec.decode_bin(15);
    let mut fl: u32 = 0;
    if fm >= fs {
        val += 1;
        fl = fs;
        fs = laplace_get_freq1(fs, decay).wrapping_add(1);
        while fs > 1 && fm >= fl.wrapping_add(fs.wrapping_mul(2)) {
            fs = fs.wrapping_mul(2);
            fl = fl.wrapping_add(fs);
            fs = (fs.wrapping_sub(2)).wrapping_mul(decay as u32) >> 15;
            fs = fs.wrapping_add(1);
            val += 1;
        }
        if fs <= 1 {
            let di = (fm.wrapping_sub(fl)) >> 1;
            val = val.wrapping_add(di as i32);
            fl = fl.wrapping_add(di.wrapping_mul(2));
        }
        if fm < fl.wrapping_add(fs) {
            val = -val;
        } else {
            fl = fl.wrapping_add(fs);
        }
    }
    let fh = fl.wrapping_add(fs).min(32768);
    safe_update(dec, fl, fh, 32768);
    val
}

pub fn unquant_coarse_energy(
    start: usize,
    end: usize,
    old_ebands: &mut [f32],
    intra: bool,
    dec: &mut Decoder,
    c: usize,
    lm: usize,
) {
    let base = (lm.min(3) * 2 + intra as usize) * 42;
    let prob_model = &E_PROB_MODEL[base..base + 42];
    let mut prev = [0.0f32; 2];
    let (coef, beta) = if intra {
        (0.0f32, BETA_INTRA as f32)
    } else {
        (PRED_COEF[lm.min(3)] as f32, BETA_COEF[lm.min(3)] as f32)
    };
    let budget = dec.storage.wrapping_mul(8) as i32;
    for i in start..end {
        for ch in 0..c {
            let tell = dec.tell();
            let qi: i32 = if budget.wrapping_sub(tell) >= 15 {
                let pi = 2 * i.min(20);
                laplace_decode(dec, (prob_model[pi] as u32) << 7, (prob_model[pi + 1] as i32) << 6)
            } else if budget.wrapping_sub(tell) >= 2 {
                let q = dec.icdf(&SMALL_ENERGY_ICDF, 2);
                (q >> 1) ^ -(q & 1)
            } else if budget.wrapping_sub(tell) >= 1 {
                -(dec.bit_logp(1) as i32)
            } else {
                -1
            };
            let q = qi as f32;
            let idx = i + ch * NB_EBANDS;
            old_ebands[idx] = super::mathops::fmax(-9.0, old_ebands[idx]);
            let tmp = coef * old_ebands[idx] + prev[ch] + q;
            old_ebands[idx] = tmp;
            prev[ch] = prev[ch] + q - beta * q;
        }
    }
}

pub fn unquant_fine_energy(
    start: usize,
    end: usize,
    old_ebands: &mut [f32],
    extra_quant: &[i32],
    dec: &mut Decoder,
    c: usize,
) {
    for i in start..end {
        let extra = extra_quant[i];
        if extra <= 0 {
            continue;
        }
        if dec.tell().wrapping_add((c as i32).wrapping_mul(extra)) > dec.storage.wrapping_mul(8) as i32 {
            continue;
        }
        let extra = extra.min(14);
        for ch in 0..c {
            let q2 = dec.dec_bits(extra as u32);
            let mut offset = (q2 as f32 + 0.5f32) * ((1i32 << (14 - extra)) as f32) * (1.0f32 / 16384.0) - 0.5f32;
            offset *= ((1i32 << 14) as f32) * (1.0f32 / 16384.0);
            old_ebands[i + ch * NB_EBANDS] += offset;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn unquant_energy_finalise(
    start: usize,
    end: usize,
    old_ebands: &mut [f32],
    fine_quant: &[i32],
    fine_priority: &[i32],
    mut bits_left: i32,
    dec: &mut Decoder,
    c: usize,
) {
    for prio in 0..2 {
        let mut i = start;
        while i < end && bits_left >= c as i32 {
            if fine_quant[i] >= MAX_FINE_BITS || fine_priority[i] != prio {
                i += 1;
                continue;
            }
            for ch in 0..c {
                let q2 = dec.dec_bits(1);
                let sh = (14 - fine_quant[i] - 1).clamp(0, 30);
                let offset = (q2 as f32 - 0.5f32) * ((1i32 << sh) as f32) * (1.0f32 / 16384.0);
                old_ebands[i + ch * NB_EBANDS] += offset;
                bits_left -= 1;
            }
            i += 1;
        }
    }
}
