pub const OVF: u64 = 1 << 63;

#[derive(Clone)]
pub struct Cv {
    pub buf: [u8; 0x81],
    pub base_len: usize,
    pub max: usize,
    pub packed: u64,
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in src.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63]);
        out.push(B64[(n >> 12) as usize & 63]);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] } else { b'=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] } else { b'=' });
    }
    out
}

impl Cv {
    pub fn new_v2(bytes: &[u8; 16]) -> Cv {
        let mut cv = Cv { buf: [0; 0x81], base_len: 0x17, max: 0x81, packed: 0x19u64 << 32 };
        let b = base64(bytes);
        cv.buf[..b.len()].copy_from_slice(&b);
        cv.buf[0x16] = b'.';
        cv.buf[0x17] = 0;
        cv
    }

    pub fn extend(s: &[u8]) -> Option<Cv> {
        let len = s.len();
        let max = if len > 22 && s[22] == b'.' {
            0x81
        } else if len > 16 && s[16] == b'.' {
            0x41
        } else {
            return None;
        };
        if len + 1 > max {
            return None;
        }
        let bang = s[len - 1] == b'!';
        if len + 1 == max && !bang {
            return None;
        }
        let mut cv = Cv { buf: [0; 0x81], base_len: 0, max, packed: 0 };
        let l = len as u64;
        if bang {
            cv.buf[..len - 1].copy_from_slice(&s[..len - 1]);
            cv.base_len = len - 1;
            cv.packed = ((l + 1) << 32) | OVF;
        } else if len + 2 == max {
            cv.buf[..len].copy_from_slice(s);
            cv.base_len = len;
            cv.packed = ((l + 2) << 32) | OVF;
        } else if len + 3 == max {
            cv.buf[..len].copy_from_slice(s);
            cv.buf[len] = b'.';
            cv.base_len = len + 1;
            cv.packed = ((l + 3) << 32) | OVF;
        } else {
            cv.buf[..len].copy_from_slice(s);
            cv.buf[len] = b'.';
            cv.base_len = len + 1;
            cv.packed = (l + 3) << 32;
        }
        Some(cv)
    }

    pub fn format(&self, packed: u64) -> Option<Vec<u8>> {
        let counter = packed & 0x7fff_ffff;
        let ovf = packed & OVF != 0;
        let suffix = if ovf && counter == 0 {
            b"!".to_vec()
        } else if ovf {
            format!("{counter}!").into_bytes()
        } else {
            format!("{counter}").into_bytes()
        };
        if suffix.len() + 1 > self.max - self.base_len {
            return None;
        }
        let mut out = self.buf[..self.base_len].to_vec();
        out.extend_from_slice(&suffix);
        Some(out)
    }

    pub fn increment(&mut self) -> u64 {
        let v = self.packed;
        let c = v & 0x7fff_ffff;
        if c == 0x7fff_ffff || v & OVF != 0 {
            return v;
        }
        let n = c + 1;
        let mut hi = (v >> 32) & 0x7fff_ffff;
        if matches!(n, 10 | 100 | 1000 | 10000 | 100000 | 1000000 | 10000000 | 100000000 | 1000000000) {
            hi += 1;
        }
        let nv = if hi >= self.max as u64 { v | OVF } else { (hi << 32) | n };
        self.packed = nv;
        nv
    }
}
