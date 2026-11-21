use super::mathops::inner_prod;

pub const CELT_LPC_ORDER: usize = 24;

pub fn celt_lpc(lpc: &mut [f32], ac: &[f32], p: usize) {
    let mut error = ac[0];
    for v in lpc.iter_mut().take(p) {
        *v = 0.0;
    }
    if ac[0] > 1e-10f32 {
        for i in 0..p {
            let mut rr = 0.0f32;
            for j in 0..i {
                rr += lpc[j] * ac[i - j];
            }
            rr += ac[i + 1];
            let r = -(rr / error);
            lpc[i] = r;
            for j in 0..(i + 1) >> 1 {
                let tmp1 = lpc[j];
                let tmp2 = lpc[i - 1 - j];
                lpc[j] = tmp1 + r * tmp2;
                lpc[i - 1 - j] = tmp2 + r * tmp1;
            }
            error -= r * r * error;
            if error <= 0.001f32 * ac[0] {
                break;
            }
        }
    }
}

pub fn celt_fir(x: &[f32], x_off: usize, num: &[f32], y: &mut [f32], n: usize, ord: usize) {
    let mut rnum = [0.0f32; CELT_LPC_ORDER];
    for i in 0..ord {
        rnum[i] = num[ord - i - 1];
    }
    for i in 0..n {
        let mut sum = x[x_off + i];
        for j in 0..ord {
            sum += rnum[j] * x[x_off + i + j - ord];
        }
        y[i] = sum;
    }
}

pub fn celt_iir_inplace(buf: &mut [f32], den: &[f32], n: usize, ord: usize, mem: &mut [f32]) {
    let mut rden = [0.0f32; CELT_LPC_ORDER];
    for i in 0..ord {
        rden[i] = den[ord - i - 1];
    }
    let mut y = vec![0.0f32; n + ord];
    for i in 0..ord {
        y[i] = -mem[ord - i - 1];
    }
    let mut i = 0usize;
    while i + 3 < n {
        let mut sum = [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]];
        for j in 0..ord {
            let r = rden[j];
            sum[0] += r * y[i + j];
            sum[1] += r * y[i + j + 1];
            sum[2] += r * y[i + j + 2];
            sum[3] += r * y[i + j + 3];
        }
        y[i + ord] = -sum[0];
        buf[i] = sum[0];
        sum[1] += y[i + ord] * den[0];
        y[i + ord + 1] = -sum[1];
        buf[i + 1] = sum[1];
        sum[2] += y[i + ord + 1] * den[0];
        sum[2] += y[i + ord] * den[1];
        y[i + ord + 2] = -sum[2];
        buf[i + 2] = sum[2];
        sum[3] += y[i + ord + 2] * den[0];
        sum[3] += y[i + ord + 1] * den[1];
        sum[3] += y[i + ord] * den[2];
        y[i + ord + 3] = -sum[3];
        buf[i + 3] = sum[3];
        i += 4;
    }
    while i < n {
        let mut sum = buf[i];
        for j in 0..ord {
            sum -= rden[j] * y[i + j];
        }
        y[i + ord] = sum;
        buf[i] = sum;
        i += 1;
    }
    for k in 0..ord {
        mem[k] = buf[n - k - 1];
    }
}

pub fn celt_autocorr(x: &[f32], ac: &mut [f32], window: Option<&[f32]>, overlap: usize, lag: usize, n: usize) {
    let fast_n = n - lag;
    let mut xx: Vec<f32> = x[..n].to_vec();
    if let Some(w) = window {
        for i in 0..overlap {
            xx[i] = x[i] * w[i];
            xx[n - i - 1] = x[n - i - 1] * w[i];
        }
    }
    for k in 0..=lag {
        ac[k] = inner_prod(&xx[..], &xx[k..], fast_n);
    }
    for k in 0..=lag {
        let mut d = 0.0f32;
        for i in k + fast_n..n {
            d += xx[i] * xx[i - k];
        }
        ac[k] += d;
    }
}

fn celt_fir5(x: &mut [f32], num: &[f32; 5], n: usize) {
    let (mut m0, mut m1, mut m2, mut m3, mut m4) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for i in 0..n {
        let mut sum = x[i];
        sum += num[0] * m0;
        sum += num[1] * m1;
        sum += num[2] * m2;
        sum += num[3] * m3;
        sum += num[4] * m4;
        m4 = m3;
        m3 = m2;
        m2 = m1;
        m1 = m0;
        m0 = x[i];
        x[i] = sum;
    }
}

pub fn pitch_downsample(x: &[&[f32]], x_lp: &mut [f32], len: usize, c: usize) {
    let factor = 2usize;
    let offset = factor / 2;
    let x0 = x[0];
    for i in 1..len {
        x_lp[i] = 0.25f32 * x0[factor * i - offset] + 0.25f32 * x0[factor * i + offset] + 0.5f32 * x0[factor * i];
    }
    x_lp[0] = 0.25f32 * x0[offset] + 0.5f32 * x0[0];
    if c == 2 {
        let x1 = x[1];
        for i in 1..len {
            x_lp[i] += 0.25f32 * x1[factor * i - offset] + 0.25f32 * x1[factor * i + offset] + 0.5f32 * x1[factor * i];
        }
        x_lp[0] += 0.25f32 * x1[offset] + 0.5f32 * x1[0];
    }
    let mut ac = [0.0f32; 5];
    celt_autocorr(x_lp, &mut ac, None, 0, 4, len);
    ac[0] *= 1.0001f32;
    for i in 1..=4usize {
        let f = 0.008f32 * i as f32;
        ac[i] -= ac[i] * f * f;
    }
    let mut lpc = [0.0f32; 4];
    celt_lpc(&mut lpc, &ac, 4);
    let mut tmp = 1.0f32;
    for v in lpc.iter_mut() {
        tmp *= 0.9f32;
        *v *= tmp;
    }
    let c1 = 0.8f32;
    let lpc2 = [lpc[0] + 0.8f32, lpc[1] + c1 * lpc[0], lpc[2] + c1 * lpc[1], lpc[3] + c1 * lpc[2], c1 * lpc[3]];
    celt_fir5(x_lp, &lpc2, len);
}

fn find_best_pitch(xcorr: &[f32], y: &[f32], len: usize, max_pitch: usize, best_pitch: &mut [usize; 2]) {
    let mut syy = 1.0f32;
    let mut best_num = [-1.0f32; 2];
    let mut best_den = [0.0f32; 2];
    best_pitch[0] = 0;
    best_pitch[1] = 1;
    for j in 0..len {
        syy += y[j] * y[j];
    }
    for i in 0..max_pitch {
        if xcorr[i] > 0.0 {
            let mut xcorr16 = xcorr[i];
            xcorr16 *= 1e-12f32;
            let num = xcorr16 * xcorr16;
            if num * best_den[1] > best_num[1] * syy {
                if num * best_den[0] > best_num[0] * syy {
                    best_num[1] = best_num[0];
                    best_den[1] = best_den[0];
                    best_pitch[1] = best_pitch[0];
                    best_num[0] = num;
                    best_den[0] = syy;
                    best_pitch[0] = i;
                } else {
                    best_num[1] = num;
                    best_den[1] = syy;
                    best_pitch[1] = i;
                }
            }
        }
        syy += y[i + len] * y[i + len] - y[i] * y[i];
        syy = if 1.0f32 > syy { 1.0 } else { syy };
    }
}

pub fn pitch_search(x_lp: &[f32], y: &[f32], len: usize, max_pitch: usize) -> i32 {
    let lag = len + max_pitch;
    let mut x_lp4 = vec![0.0f32; len >> 2];
    let mut y_lp4 = vec![0.0f32; lag >> 2];
    let mut xcorr = vec![0.0f32; max_pitch >> 1];
    for j in 0..len >> 2 {
        x_lp4[j] = x_lp[2 * j];
    }
    for j in 0..lag >> 2 {
        y_lp4[j] = y[2 * j];
    }
    for i in 0..max_pitch >> 2 {
        xcorr[i] = inner_prod(&x_lp4, &y_lp4[i..], len >> 2);
    }
    let mut best_pitch = [0usize, 0usize];
    find_best_pitch(&xcorr, &y_lp4, len >> 2, max_pitch >> 2, &mut best_pitch);
    for i in 0..max_pitch >> 1 {
        xcorr[i] = 0.0;
        if (i as i32 - 2 * best_pitch[0] as i32).abs() > 2 && (i as i32 - 2 * best_pitch[1] as i32).abs() > 2 {
            continue;
        }
        let sum = inner_prod(x_lp, &y[i..], len >> 1);
        xcorr[i] = if -1.0f32 > sum { -1.0 } else { sum };
    }
    find_best_pitch(&xcorr, y, len >> 1, max_pitch >> 1, &mut best_pitch);
    let offset: i32 = if best_pitch[0] > 0 && best_pitch[0] < (max_pitch >> 1) - 1 {
        let a = xcorr[best_pitch[0] - 1];
        let b = xcorr[best_pitch[0]];
        let c = xcorr[best_pitch[0] + 1];
        if (c - a) > 0.7f32 * (b - a) {
            1
        } else if (a - c) > 0.7f32 * (b - c) {
            -1
        } else {
            0
        }
    } else {
        0
    };
    2 * best_pitch[0] as i32 - offset
}
