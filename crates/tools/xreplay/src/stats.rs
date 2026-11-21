use std::fmt::Write as _;
use std::path::Path;

pub fn f16(h: u16) -> f32 {
    let s = ((h >> 15) & 1) as u32;
    let e = ((h >> 10) & 0x1f) as u32;
    let m = (h & 0x3ff) as u32;
    let bits = if e == 0 {
        if m == 0 {
            s << 31
        } else {
            let mut e2 = 127 - 15 + 1;
            let mut m2 = m;
            while m2 & 0x400 == 0 {
                m2 <<= 1;
                e2 -= 1;
            }
            (s << 31) | ((e2 as u32) << 23) | ((m2 & 0x3ff) << 13)
        }
    } else if e == 31 {
        (s << 31) | 0x7f80_0000 | (m << 13)
    } else {
        (s << 31) | ((e + 127 - 15) << 23) | (m << 13)
    };
    f32::from_bits(bits)
}

fn small_float(v: u32, mbits: u32) -> f32 {
    let e = v >> mbits;
    let m = v & ((1 << mbits) - 1);
    if e == 31 {
        return if m == 0 { f32::INFINITY } else { f32::NAN };
    }
    if e == 0 {
        return (m as f32) / ((1u32 << mbits) as f32) * 2f32.powi(-14);
    }
    (1.0 + m as f32 / (1u32 << mbits) as f32) * 2f32.powi(e as i32 - 15)
}

pub fn texel_size(fmt: u32) -> Option<usize> {
    Some(match fmt {
        1 | 2 => 16,
        5 | 6 => 12,
        9 | 10 | 15 | 16 | 19 | 20 => 8,
        23 | 24 | 26 | 27 | 28 | 29 | 33 | 34 | 39 | 40 | 41 | 44 | 45 | 87 | 88 | 90 | 91 => 4,
        53 | 54 | 55 | 56 => 2,
        60 | 61 => 1,
        _ => return None,
    })
}

pub fn is_float(fmt: u32) -> bool {
    matches!(fmt, 1 | 2 | 5 | 6 | 9 | 10 | 15 | 16 | 19 | 20 | 26 | 33 | 34 | 39 | 40 | 41 | 53 | 54)
}

pub fn decode(fmt: u32, t: &[u8]) -> [f32; 4] {
    let u32at = |o: usize| u32::from_le_bytes(t[o..o + 4].try_into().unwrap());
    let f32at = |o: usize| f32::from_bits(u32at(o));
    let h = |o: usize| f16(u16::from_le_bytes([t[o], t[o + 1]]));
    match fmt {
        1 | 2 => [f32at(0), f32at(4), f32at(8), f32at(12)],
        5 | 6 => [f32at(0), f32at(4), f32at(8), 1.0],
        9 | 10 => [h(0), h(2), h(4), h(6)],
        15 | 16 => [f32at(0), f32at(4), 0.0, 1.0],
        19 | 20 => [f32at(0), 0.0, 0.0, 1.0],
        26 => {
            let v = u32at(0);
            [small_float(v & 0x7ff, 6), small_float((v >> 11) & 0x7ff, 6), small_float(v >> 22, 5), 1.0]
        }
        33 | 34 => [h(0), h(2), 0.0, 1.0],
        39 | 40 | 41 => [f32at(0), 0.0, 0.0, 1.0],
        53 | 54 => [h(0), 0.0, 0.0, 1.0],
        23 | 24 => {
            let v = u32at(0);
            [(v & 0x3ff) as f32 / 1023.0, ((v >> 10) & 0x3ff) as f32 / 1023.0, ((v >> 20) & 0x3ff) as f32 / 1023.0, (v >> 30) as f32 / 3.0]
        }
        27 | 28 | 29 => [t[0] as f32 / 255.0, t[1] as f32 / 255.0, t[2] as f32 / 255.0, t[3] as f32 / 255.0],
        87 | 88 | 90 | 91 => [t[2] as f32 / 255.0, t[1] as f32 / 255.0, t[0] as f32 / 255.0, t[3] as f32 / 255.0],
        44 | 45 => [(u32at(0) & 0xff_ffff) as f32 / 16_777_215.0, (t[3]) as f32, 0.0, 1.0],
        55 | 56 => [u16::from_le_bytes([t[0], t[1]]) as f32 / 65535.0, 0.0, 0.0, 1.0],
        60 | 61 => [t[0] as f32 / 255.0, 0.0, 0.0, 1.0],
        _ => [0.0; 4],
    }
}

pub fn channels(fmt: u32) -> usize {
    match fmt {
        1 | 2 | 9 | 10 | 23 | 24 | 27 | 28 | 29 | 87 | 88 | 90 | 91 => 4,
        5 | 6 | 26 => 3,
        15 | 16 | 33 | 34 | 44 | 45 => 2,
        _ => 1,
    }
}

#[derive(Default, Clone)]
pub struct Stats {
    pub texels: u64,
    pub nan: u64,
    pub inf: u64,
    pub nan_texels: u64,
    pub inf_texels: u64,
    pub min: f32,
    pub max: f32,
    pub sum: f64,
    pub finite: u64,
}

impl Stats {
    pub fn line(&self) -> String {
        let mean = if self.finite > 0 { self.sum / self.finite as f64 } else { 0.0 };
        let mut s = format!("texels={} nan={} inf={} (texels with nan={} inf={})", self.texels, self.nan, self.inf, self.nan_texels, self.inf_texels);
        if self.finite > 0 {
            let _ = write!(s, " finite min={:.4} max={:.4} mean={:.4}", self.min, self.max, mean);
        }
        s
    }
}

pub struct Image {
    pub fmt: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: usize,
    pub data: Vec<u8>,
}

impl Image {
    pub fn stats(&self) -> Stats {
        let mut st = Stats { min: f32::MAX, max: f32::MIN, ..Default::default() };
        let Some(ts) = texel_size(self.fmt) else { return st };
        let nc = channels(self.fmt);
        for y in 0..self.height as usize {
            for x in 0..self.width as usize {
                let o = y * self.pitch + x * ts;
                if o + ts > self.data.len() {
                    continue;
                }
                let v = decode(self.fmt, &self.data[o..o + ts]);
                st.texels += 1;
                let (mut tn, mut ti) = (false, false);
                for &c in &v[..nc] {
                    if c.is_nan() {
                        st.nan += 1;
                        tn = true;
                    } else if c.is_infinite() {
                        st.inf += 1;
                        ti = true;
                    } else {
                        st.min = st.min.min(c);
                        st.max = st.max.max(c);
                        st.sum += c as f64;
                        st.finite += 1;
                    }
                }
                st.nan_texels += tn as u64;
                st.inf_texels += ti as u64;
            }
        }
        st
    }

    pub fn write_bmp(&self, path: &Path) -> std::io::Result<()> {
        let ts = texel_size(self.fmt).unwrap_or(4);
        let st = self.stats();
        let float = is_float(self.fmt);
        let nc = channels(self.fmt);
        let (lo, hi) = if nc == 1 && st.finite > 0 { (st.min, st.max) } else { (0.0, 1.0) };
        let map = |c: f32| -> u8 {
            if c.is_nan() {
                return 255;
            }
            let c = if nc == 1 {
                if hi > lo { (c - lo) / (hi - lo) } else { c }
            } else if float && st.max > 1.0 {
                if c.is_infinite() { 1.0 } else { c.max(0.0) / (1.0 + c.max(0.0)) }
            } else {
                c
            };
            (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
        };
        let (w, h) = (self.width as usize, self.height as usize);
        let row = (w * 3 + 3) & !3;
        let mut out = Vec::with_capacity(54 + row * h);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&((54 + row * h) as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&54u32.to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(w as i32).to_le_bytes());
        out.extend_from_slice(&(-(h as i32)).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        out.extend_from_slice(&[0u8; 24]);
        for y in 0..h {
            let start = out.len();
            for x in 0..w {
                let o = y * self.pitch + x * ts;
                let v = if o + ts <= self.data.len() { decode(self.fmt, &self.data[o..o + ts]) } else { [0.0; 4] };
                let nanpix = v[..nc].iter().any(|c| c.is_nan());
                let infpix = v[..nc].iter().any(|c| c.is_infinite());
                if nanpix {
                    out.extend_from_slice(&[255, 0, 255]);
                } else if infpix && float {
                    out.extend_from_slice(&[0, 255, 255]);
                } else if nc == 1 {
                    let g = map(v[0]);
                    out.extend_from_slice(&[g, g, g]);
                } else {
                    out.extend_from_slice(&[map(v[2]), map(v[1]), map(v[0])]);
                }
            }
            while out.len() - start < row {
                out.push(0);
            }
        }
        std::fs::write(path, out)
    }
}

pub fn max_abs_diff(a: &Image, b: &Image) -> (f32, u64, u64) {
    let Some(ts) = texel_size(a.fmt) else { return (0.0, 0, 0) };
    let nc = channels(a.fmt);
    let (mut m, mut differing, mut n) = (0f32, 0u64, 0u64);
    for y in 0..a.height.min(b.height) as usize {
        for x in 0..a.width.min(b.width) as usize {
            let (oa, ob) = (y * a.pitch + x * ts, y * b.pitch + x * ts);
            if oa + ts > a.data.len() || ob + ts > b.data.len() {
                continue;
            }
            n += 1;
            if a.data[oa..oa + ts] == b.data[ob..ob + ts] {
                continue;
            }
            differing += 1;
            let (va, vb) = (decode(a.fmt, &a.data[oa..oa + ts]), decode(b.fmt, &b.data[ob..ob + ts]));
            for c in 0..nc {
                let d = (va[c] - vb[c]).abs();
                if d.is_nan() || d.is_infinite() {
                    m = f32::INFINITY;
                } else {
                    m = m.max(d);
                }
            }
        }
    }
    (m, differing, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_and_r11g11b10_decode() {
        assert_eq!(f16(0x3c00), 1.0);
        assert!(f16(0x7c00).is_infinite());
        assert!(f16(0x7e00).is_nan());
        assert_eq!(f16(0xc000), -2.0);
        let one = 15u32 << 6;
        let v = one | (one << 11) | ((15u32 << 5) << 22);
        let d = decode(26, &v.to_le_bytes());
        assert_eq!(d[0], 1.0);
        assert_eq!(d[1], 1.0);
        assert_eq!(d[2], 1.0);
        let inf = 31u32 << 6;
        assert!(decode(26, &inf.to_le_bytes())[0].is_infinite());
    }

    #[test]
    fn stats_count_bad_texels() {
        let mut data = Vec::new();
        for v in [1.0f32, f32::NAN, f32::INFINITY, 3.0] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let img = Image { fmt: 41, width: 2, height: 2, pitch: 8, data };
        let s = img.stats();
        assert_eq!((s.texels, s.nan, s.inf, s.finite), (4, 1, 1, 2));
        assert_eq!(s.max, 3.0);
    }
}
