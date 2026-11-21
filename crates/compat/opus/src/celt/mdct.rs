use super::tables::{FFT_BITREV120, FFT_BITREV240, FFT_BITREV480, FFT_BITREV60, FFT_TWIDDLES, MDCT_TWIDDLES};

#[derive(Clone, Copy, Default)]
struct Cpx {
    r: f32,
    i: f32,
}

struct FftState {
    nfft: usize,
    shift: usize,
    factors: &'static [usize],
    bitrev: &'static [i16],
}

static FFT_STATES: [FftState; 4] = [
    FftState { nfft: 480, shift: 0, factors: &[5, 96, 3, 32, 4, 8, 2, 4, 4, 1], bitrev: &FFT_BITREV480 },
    FftState { nfft: 240, shift: 1, factors: &[5, 48, 3, 16, 4, 4, 4, 1], bitrev: &FFT_BITREV240 },
    FftState { nfft: 120, shift: 2, factors: &[5, 24, 3, 8, 2, 4, 4, 1], bitrev: &FFT_BITREV120 },
    FftState { nfft: 60, shift: 3, factors: &[5, 12, 3, 4, 4, 1], bitrev: &FFT_BITREV60 },
];

#[inline]
fn tw(k: usize) -> Cpx {
    Cpx { r: FFT_TWIDDLES[(2 * k) % 960], i: FFT_TWIDDLES[(2 * k + 1) % 960] }
}

#[inline]
fn cmul(a: Cpx, b: Cpx) -> Cpx {
    Cpx { r: a.r * b.r - a.i * b.i, i: a.r * b.i + a.i * b.r }
}

#[inline]
fn cadd(a: Cpx, b: Cpx) -> Cpx {
    Cpx { r: a.r + b.r, i: a.i + b.i }
}

#[inline]
fn csub(a: Cpx, b: Cpx) -> Cpx {
    Cpx { r: a.r - b.r, i: a.i - b.i }
}

fn kf_bfly2(f: &mut [Cpx], n: usize) {
    let twv: f32 = 0.7071067812f32;
    let mut base = 0usize;
    for _ in 0..n {
        let t = f[base + 4];
        f[base + 4] = csub(f[base], t);
        f[base] = cadd(f[base], t);

        let a = f[base + 5];
        let t = Cpx { r: (a.r + a.i) * twv, i: (a.i - a.r) * twv };
        f[base + 5] = csub(f[base + 1], t);
        f[base + 1] = cadd(f[base + 1], t);

        let a = f[base + 6];
        let t = Cpx { r: a.i, i: -a.r };
        f[base + 6] = csub(f[base + 2], t);
        f[base + 2] = cadd(f[base + 2], t);

        let a = f[base + 7];
        let t = Cpx { r: (a.i - a.r) * twv, i: -(a.i + a.r) * twv };
        f[base + 7] = csub(f[base + 3], t);
        f[base + 3] = cadd(f[base + 3], t);
        base += 8;
    }
}

fn kf_bfly4(f: &mut [Cpx], fstride: usize, m: usize, n: usize, mm: usize) {
    if m == 1 {
        let mut b = 0usize;
        for _ in 0..n {
            let scratch0 = csub(f[b], f[b + 2]);
            f[b] = cadd(f[b], f[b + 2]);
            let mut scratch1 = cadd(f[b + 1], f[b + 3]);
            f[b + 2] = csub(f[b], scratch1);
            f[b] = cadd(f[b], scratch1);
            scratch1 = csub(f[b + 1], f[b + 3]);
            f[b + 1] = Cpx { r: scratch0.r + scratch1.i, i: scratch0.i - scratch1.r };
            f[b + 3] = Cpx { r: scratch0.r - scratch1.i, i: scratch0.i + scratch1.r };
            b += 4;
        }
    } else {
        let m2 = 2 * m;
        let m3 = 3 * m;
        for i in 0..n {
            let mut p = i * mm;
            let (mut t1, mut t2, mut t3) = (0usize, 0usize, 0usize);
            for _ in 0..m {
                let s0 = cmul(f[p + m], tw(t1));
                let s1 = cmul(f[p + m2], tw(t2));
                let s2 = cmul(f[p + m3], tw(t3));
                let s5 = csub(f[p], s1);
                f[p] = cadd(f[p], s1);
                let s3 = cadd(s0, s2);
                let s4 = csub(s0, s2);
                f[p + m2] = csub(f[p], s3);
                t1 += fstride;
                t2 += fstride * 2;
                t3 += fstride * 3;
                f[p] = cadd(f[p], s3);
                f[p + m] = Cpx { r: s5.r + s4.i, i: s5.i - s4.r };
                f[p + m3] = Cpx { r: s5.r - s4.i, i: s5.i + s4.r };
                p += 1;
            }
        }
    }
}

fn kf_bfly3(f: &mut [Cpx], fstride: usize, m: usize, n: usize, mm: usize) {
    let m2 = 2 * m;
    let epi3 = tw(fstride * m);
    for i in 0..n {
        let mut p = i * mm;
        let (mut t1, mut t2) = (0usize, 0usize);
        for _ in 0..m {
            let s1 = cmul(f[p + m], tw(t1));
            let s2 = cmul(f[p + m2], tw(t2));
            let s3 = cadd(s1, s2);
            let mut s0 = csub(s1, s2);
            t1 += fstride;
            t2 += fstride * 2;
            f[p + m] = Cpx { r: f[p].r - s3.r * 0.5f32, i: f[p].i - s3.i * 0.5f32 };
            s0 = Cpx { r: s0.r * epi3.i, i: s0.i * epi3.i };
            f[p] = cadd(f[p], s3);
            f[p + m2] = Cpx { r: f[p + m].r + s0.i, i: f[p + m].i - s0.r };
            f[p + m] = Cpx { r: f[p + m].r - s0.i, i: f[p + m].i + s0.r };
            p += 1;
        }
    }
}

fn kf_bfly5(f: &mut [Cpx], fstride: usize, m: usize, n: usize, mm: usize) {
    let ya = tw(fstride * m);
    let yb = tw(fstride * 2 * m);
    for i in 0..n {
        let base = i * mm;
        let (mut p0, mut p1, mut p2, mut p3, mut p4) = (base, base + m, base + 2 * m, base + 3 * m, base + 4 * m);
        for u in 0..m {
            let s0 = f[p0];
            let s1 = cmul(f[p1], tw(u * fstride));
            let s2 = cmul(f[p2], tw(2 * u * fstride));
            let s3 = cmul(f[p3], tw(3 * u * fstride));
            let s4 = cmul(f[p4], tw(4 * u * fstride));
            let s7 = cadd(s1, s4);
            let s10 = csub(s1, s4);
            let s8 = cadd(s2, s3);
            let s9 = csub(s2, s3);
            f[p0] = Cpx { r: f[p0].r + (s7.r + s8.r), i: f[p0].i + (s7.i + s8.i) };
            let s5 = Cpx { r: s0.r + (s7.r * ya.r + s8.r * yb.r), i: s0.i + (s7.i * ya.r + s8.i * yb.r) };
            let s6 = Cpx { r: s10.i * ya.i + s9.i * yb.i, i: -(s10.r * ya.i + s9.r * yb.i) };
            f[p1] = csub(s5, s6);
            f[p4] = cadd(s5, s6);
            let s11 = Cpx { r: s0.r + (s7.r * yb.r + s8.r * ya.r), i: s0.i + (s7.i * yb.r + s8.i * ya.r) };
            let s12 = Cpx { r: s9.i * ya.i - s10.i * yb.i, i: s10.r * yb.i - s9.r * ya.i };
            f[p2] = cadd(s11, s12);
            f[p3] = csub(s11, s12);
            p0 += 1;
            p1 += 1;
            p2 += 1;
            p3 += 1;
            p4 += 1;
        }
    }
}

fn opus_fft_impl(st: &FftState, f: &mut [Cpx]) {
    let mut fstride = [0usize; 8];
    let shift = st.shift;
    fstride[0] = 1;
    let mut l = 0usize;
    loop {
        let p = st.factors[2 * l];
        let m = st.factors[2 * l + 1];
        fstride[l + 1] = fstride[l] * p;
        l += 1;
        if m == 1 {
            break;
        }
    }
    let mut m = st.factors[2 * l - 1];
    let mut i = l;
    while i > 0 {
        i -= 1;
        let m2 = if i != 0 { st.factors[2 * i - 1] } else { 1 };
        match st.factors[2 * i] {
            2 => kf_bfly2(f, fstride[i]),
            4 => kf_bfly4(f, fstride[i] << shift, m, fstride[i], m2),
            3 => kf_bfly3(f, fstride[i] << shift, m, fstride[i], m2),
            5 => kf_bfly5(f, fstride[i] << shift, m, fstride[i], m2),
            _ => {}
        }
        m = m2;
    }
}

#[cfg(test)]
mod fft_tests {
    use super::*;

    #[test]
    fn celt_fft_matches_dft() {
        for st in FFT_STATES.iter() {
            let n = st.nfft;
            let x: Vec<Cpx> = (0..n).map(|i| Cpx { r: ((i * 7 + 3) % 11) as f32 - 5.0, i: ((i * 5 + 1) % 13) as f32 - 6.0 }).collect();
            let mut f = vec![Cpx::default(); n];
            for i in 0..n {
                f[st.bitrev[i] as usize] = x[i];
            }
            opus_fft_impl(st, &mut f);
            let mut max_err = 0.0f64;
            for k in 0..n {
                let (mut sr, mut si) = (0.0f64, 0.0f64);
                for j in 0..n {
                    let a = -2.0 * std::f64::consts::PI * (j * k) as f64 / n as f64;
                    sr += x[j].r as f64 * a.cos() - x[j].i as f64 * a.sin();
                    si += x[j].r as f64 * a.sin() + x[j].i as f64 * a.cos();
                }
                max_err = max_err.max((sr - f[k].r as f64).abs()).max((si - f[k].i as f64).abs());
            }
            println!("fft {} max_err {}", n, max_err);
            assert!(max_err < 1e-2);
        }
    }
}

pub fn clt_mdct_backward(input: &[f32], in_off: usize, out: &mut [f32], window: &[f32], overlap: usize, shift: usize, stride: usize) {
    let mut n = 1920usize;
    let mut trig_off = 0usize;
    for _ in 0..shift {
        n >>= 1;
        trig_off += n;
    }
    let n2 = n >> 1;
    let n4 = n >> 2;
    let trig = &MDCT_TWIDDLES[trig_off..];
    let st = &FFT_STATES[shift.min(3)];
    let mut f = [Cpx::default(); 480];
    let get = |idx: usize| -> f32 { input.get(in_off + idx).copied().unwrap_or(0.0) };
    for i in 0..n4 {
        let x1 = get(2 * stride * i);
        let x2 = get(stride * (n2 - 1) - 2 * stride * i);
        let yr = x2 * trig[2 * i + 1] + x1 * trig[2 * i];
        let yi = x1 * trig[2 * i + 1] - x2 * trig[2 * i];
        let rev = st.bitrev[i] as usize % st.nfft;
        f[rev] = Cpx { r: yi, i: yr };
    }
    opus_fft_impl(st, &mut f[..st.nfft]);
    let ov2 = overlap >> 1;
    let o = &mut out[ov2..ov2 + n2];
    for i in 0..(n4 + 1) >> 1 {
        let a = f[i];
        let bidx = n4 - 1 - i;
        let bv = f[bidx];
        let re = a.i;
        let im = a.r;
        let t0 = trig[2 * i];
        let t1 = trig[2 * i + 1];
        let yr = re * t1 + im * t0;
        let yi = re * t0 - im * t1;
        let re2 = bv.i;
        let im2 = bv.r;
        o[2 * i] = yr;
        o[2 * bidx + 1] = yi;
        let t0 = trig[2 * (n4 - i - 1)];
        let t1 = trig[2 * (n4 - i - 1) + 1];
        let yr = re2 * t1 + im2 * t0;
        let yi = re2 * t0 - im2 * t1;
        o[2 * bidx] = yr;
        o[2 * i + 1] = yi;
    }
    for i in 0..overlap / 2 {
        let x1 = out[overlap - 1 - i];
        let x2 = out[i];
        let w1 = window[i];
        let w2 = window[overlap - 1 - i];
        out[i] = x2 * w2 - x1 * w1;
        out[overlap - 1 - i] = x2 * w1 + x1 * w2;
    }
}
