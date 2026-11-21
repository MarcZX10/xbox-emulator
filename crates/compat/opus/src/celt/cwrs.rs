use super::tables::CELT_PVQ_U_DATA;
use crate::entdec::Decoder;

static ROW_OFF: [usize; 15] = [0, 176, 351, 525, 698, 870, 1041, 1131, 1178, 1207, 1226, 1240, 1248, 1254, 1257];

#[inline]
fn row(r: i32, c: i32) -> u32 {
    if !(0..15).contains(&r) || c < 0 {
        return 0;
    }
    CELT_PVQ_U_DATA.get(ROW_OFF[r as usize] + c as usize).copied().unwrap_or(0)
}

#[inline]
fn pvq_u(n: i32, k: i32) -> u32 {
    row(n.min(k), n.max(k))
}

#[inline]
fn pvq_v(n: i32, k: i32) -> u32 {
    pvq_u(n, k).wrapping_add(pvq_u(n, k + 1))
}

fn cwrsi(mut n: i32, mut k: i32, mut i: u32, y: &mut [i32]) -> f32 {
    let mut yy = 0.0f32;
    let mut pos = 0usize;
    if n < 2 || k <= 0 {
        for v in y.iter_mut() {
            *v = 0;
        }
        return 0.0;
    }
    while n > 2 {
        let mut p: u32;
        if k >= n {
            p = row(n, k + 1);
            let s: i32 = -((i >= p) as i32);
            i = i.wrapping_sub(p & s as u32);
            let k0 = k;
            let q = row(n, n);
            if q > i {
                k = n;
                loop {
                    k -= 1;
                    p = row(k, n);
                    if p <= i || k <= 0 {
                        break;
                    }
                }
            } else {
                p = row(n, k);
                while p > i && k > 0 {
                    k -= 1;
                    p = row(n, k);
                }
            }
            i = i.wrapping_sub(p);
            let val = (k0 - k + s) ^ s;
            if let Some(d) = y.get_mut(pos) {
                *d = val;
            }
            pos += 1;
            yy += (val as f32) * (val as f32);
        } else {
            p = row(k, n);
            let q = row(k + 1, n);
            if p <= i && i < q {
                i = i.wrapping_sub(p);
                if let Some(d) = y.get_mut(pos) {
                    *d = 0;
                }
                pos += 1;
            } else {
                let s: i32 = -((i >= q) as i32);
                i = i.wrapping_sub(q & s as u32);
                let k0 = k;
                loop {
                    k -= 1;
                    p = row(k, n);
                    if p <= i || k <= 0 {
                        break;
                    }
                }
                i = i.wrapping_sub(p);
                let val = (k0 - k + s) ^ s;
                if let Some(d) = y.get_mut(pos) {
                    *d = val;
                }
                pos += 1;
                yy += (val as f32) * (val as f32);
            }
        }
        n -= 1;
    }
    let p = (2 * k + 1) as u32;
    let s: i32 = -((i >= p) as i32);
    i = i.wrapping_sub(p & s as u32);
    let k0 = k;
    k = (i.wrapping_add(1) >> 1) as i32;
    if k != 0 {
        i = i.wrapping_sub((2 * k - 1) as u32);
    }
    let val = (k0 - k + s) ^ s;
    if let Some(d) = y.get_mut(pos) {
        *d = val;
    }
    pos += 1;
    yy += (val as f32) * (val as f32);
    let s: i32 = (i as i32).wrapping_neg();
    let val = (k + s) ^ s;
    if let Some(d) = y.get_mut(pos) {
        *d = val;
    }
    yy += (val as f32) * (val as f32);
    yy
}

pub fn decode_pulses(y: &mut [i32], n: i32, k: i32, dec: &mut Decoder) -> f32 {
    let ft = pvq_v(n, k);
    let i = dec.dec_uint(ft);
    cwrsi(n, k, i, y)
}
