pub const EC_SYM_BITS: u32 = 8;
pub const EC_CODE_BITS: i32 = 32;
pub const EC_SYM_MAX: u32 = (1 << EC_SYM_BITS) - 1;
pub const EC_CODE_TOP: u32 = 1 << (EC_CODE_BITS - 1);
pub const EC_CODE_BOT: u32 = EC_CODE_TOP >> EC_SYM_BITS;
pub const EC_CODE_EXTRA: u32 = ((EC_CODE_BITS - 2) as u32 % EC_SYM_BITS) + 1;
pub const EC_WINDOW_SIZE: i32 = 32;
pub const EC_UINT_BITS: i32 = 8;
pub const BITRES: i32 = 3;

pub fn ec_ilog(v: u32) -> i32 {
    32 - v.leading_zeros() as i32
}

pub struct Decoder<'a> {
    pub buf: &'a [u8],
    pub storage: u32,
    pub end_offs: u32,
    pub end_window: u32,
    pub nend_bits: i32,
    pub nbits_total: i32,
    pub offs: u32,
    pub rng: u32,
    pub val: u32,
    pub ext: u32,
    pub rem: i32,
    pub error: i32,
}

impl<'a> Decoder<'a> {
    pub fn new(buf: &'a [u8]) -> Decoder<'a> {
        let storage = buf.len() as u32;
        let mut d = Decoder {
            buf,
            storage,
            end_offs: 0,
            end_window: 0,
            nend_bits: 0,
            nbits_total: EC_CODE_BITS + 1 - ((EC_CODE_BITS - EC_CODE_EXTRA as i32) / EC_SYM_BITS as i32) * EC_SYM_BITS as i32,
            offs: 0,
            rng: 1 << EC_CODE_EXTRA,
            val: 0,
            ext: 0,
            rem: 0,
            error: 0,
        };
        d.rem = d.read_byte();
        d.val = d.rng - 1 - ((d.rem as u32) >> (EC_SYM_BITS - EC_CODE_EXTRA));
        d.normalize();
        d
    }

    fn read_byte(&mut self) -> i32 {
        if self.offs < self.storage {
            let b = self.buf.get(self.offs as usize).copied().unwrap_or(0) as i32;
            self.offs += 1;
            b
        } else {
            0
        }
    }

    fn read_byte_from_end(&mut self) -> i32 {
        if self.end_offs < self.storage {
            self.end_offs += 1;
            self.buf.get((self.storage - self.end_offs) as usize).copied().unwrap_or(0) as i32
        } else {
            0
        }
    }

    fn normalize(&mut self) {
        while self.rng <= EC_CODE_BOT {
            self.nbits_total += EC_SYM_BITS as i32;
            self.rng <<= EC_SYM_BITS;
            let mut sym = self.rem;
            self.rem = self.read_byte();
            sym = ((sym << EC_SYM_BITS) | self.rem) >> (EC_SYM_BITS - EC_CODE_EXTRA);
            self.val = ((self.val << EC_SYM_BITS).wrapping_add(EC_SYM_MAX & !(sym as u32))) & (EC_CODE_TOP - 1);
        }
    }

    pub fn decode(&mut self, ft: u32) -> u32 {
        self.ext = self.rng / ft;
        let s = self.val / self.ext.max(1);
        ft - (s + 1).min(ft)
    }

    pub fn decode_bin(&mut self, bits: u32) -> u32 {
        self.ext = self.rng >> bits;
        let s = self.val / self.ext.max(1);
        (1u32 << bits) - (s + 1).min(1u32 << bits)
    }

    pub fn update(&mut self, fl: u32, fh: u32, ft: u32) {
        let s = self.ext.wrapping_mul(ft - fh);
        self.val = self.val.wrapping_sub(s);
        self.rng = if fl > 0 { self.ext.wrapping_mul(fh - fl) } else { self.rng.wrapping_sub(s) };
        self.normalize();
    }

    pub fn bit_logp(&mut self, logp: u32) -> bool {
        let r = self.rng;
        let d = self.val;
        let s = r >> logp;
        let ret = d < s;
        if !ret {
            self.val = d - s;
        }
        self.rng = if ret { s } else { r - s };
        self.normalize();
        ret
    }

    pub fn icdf(&mut self, icdf: &[u8], ftb: u32) -> i32 {
        let mut s = self.rng;
        let d = self.val;
        let r = s >> ftb;
        let mut ret: i32 = -1;
        let mut t;
        loop {
            t = s;
            ret += 1;
            s = r.wrapping_mul(icdf.get(ret as usize).copied().unwrap_or(0) as u32);
            if d >= s {
                break;
            }
        }
        self.val = d - s;
        self.rng = t - s;
        self.normalize();
        ret
    }

    pub fn icdf16(&mut self, icdf: &[u16], ftb: u32) -> i32 {
        let mut s = self.rng;
        let d = self.val;
        let r = s >> ftb;
        let mut ret: i32 = -1;
        let mut t;
        loop {
            t = s;
            ret += 1;
            s = r.wrapping_mul(icdf.get(ret as usize).copied().unwrap_or(0) as u32);
            if d >= s {
                break;
            }
        }
        self.val = d - s;
        self.rng = t - s;
        self.normalize();
        ret
    }

    pub fn dec_uint(&mut self, ft: u32) -> u32 {
        if ft <= 1 {
            return 0;
        }
        let ft1 = ft - 1;
        let mut ftb = ec_ilog(ft1);
        if ftb > EC_UINT_BITS {
            ftb -= EC_UINT_BITS;
            let f = (ft1 >> ftb) + 1;
            let s = self.decode(f);
            self.update(s, s + 1, f);
            let t = (s << ftb) | self.dec_bits(ftb as u32);
            if t <= ft1 {
                return t;
            }
            self.error = 1;
            ft1
        } else {
            let s = self.decode(ft);
            self.update(s, s + 1, ft);
            s
        }
    }

    pub fn dec_bits(&mut self, bits: u32) -> u32 {
        let mut window = self.end_window;
        let mut available = self.nend_bits;
        if (available as u32) < bits {
            loop {
                window |= (self.read_byte_from_end() as u32) << available;
                available += EC_SYM_BITS as i32;
                if available > EC_WINDOW_SIZE - EC_SYM_BITS as i32 {
                    break;
                }
            }
        }
        let ret = if bits >= 32 { window } else { window & ((1u32 << bits) - 1) };
        window = if bits >= 32 { 0 } else { window >> bits };
        available -= bits as i32;
        self.end_window = window;
        self.nend_bits = available;
        self.nbits_total += bits as i32;
        ret
    }

    pub fn tell(&self) -> i32 {
        self.nbits_total - ec_ilog(self.rng)
    }

    pub fn tell_frac(&self) -> u32 {
        const CORRECTION: [u32; 8] = [35733, 38967, 42495, 46340, 50535, 55109, 60097, 65535];
        let nbits = (self.nbits_total as u32) << BITRES;
        let mut l = ec_ilog(self.rng);
        let r = self.rng >> (l - 16);
        let mut b = (r >> 12) - 8;
        b += (r > CORRECTION[b as usize & 7]) as u32;
        l = (l << 3) + b as i32;
        nbits.wrapping_sub(l as u32)
    }

    pub fn range_bytes(&self) -> u32 {
        self.offs
    }

    pub fn get_error(&self) -> i32 {
        self.error
    }
}
