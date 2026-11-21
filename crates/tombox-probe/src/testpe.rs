use std::collections::HashMap;

pub const TEXT_RVA: u32 = 0x1000;
pub const RDATA_RVA: u32 = 0x2000;
pub const IMAGE_BASE: u64 = 0x1_8000_0000;
const TEXT_RAW: usize = 0x400;
const TEXT_SIZE: usize = 0x200;
const RDATA_RAW: usize = 0x600;
const RDATA_SIZE: usize = 0x2000;

struct Buf {
    d: Vec<u8>,
}

impl Buf {
    fn put(&mut self, bytes: &[u8]) -> u32 {
        while self.d.len() % 8 != 0 {
            self.d.push(0);
        }
        let rva = RDATA_RVA + self.d.len() as u32;
        self.d.extend_from_slice(bytes);
        rva
    }
    fn reserve(&mut self, n: usize) -> u32 {
        self.put(&vec![0u8; n])
    }
    fn w32(&mut self, rva: u32, v: u32) {
        let o = (rva - RDATA_RVA) as usize;
        self.d[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn w64(&mut self, rva: u32, v: u64) {
        let o = (rva - RDATA_RVA) as usize;
        self.d[o..o + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn w16(&mut self, rva: u32, v: u16) {
        let o = (rva - RDATA_RVA) as usize;
        self.d[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn cstr(&mut self, s: &str) -> u32 {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        self.put(&v)
    }
    fn hint_name(&mut self, s: &str) -> u32 {
        let mut v = vec![0u8, 0u8];
        v.extend_from_slice(s.as_bytes());
        v.push(0);
        self.put(&v)
    }
}

fn thunk_of(b: &mut Buf, f: &str) -> u64 {
    if let Some(o) = f.strip_prefix('#') {
        (1u64 << 63) | o.parse::<u64>().unwrap()
    } else {
        b.hint_name(f) as u64
    }
}

pub fn build(exports: &[&str], imports: &[(&str, &[&str])], delays: &[(&str, &[&str])]) -> Vec<u8> {
    build_full(exports, imports, delays, &[], &[]).0
}

pub fn build_full(
    exports: &[&str],
    imports: &[(&str, &[&str])],
    delays: &[(&str, &[&str])],
    text: &[u8],
    strings: &[(&str, bool)],
) -> (Vec<u8>, HashMap<String, u32>) {
    let mut b = Buf { d: Vec::new() };
    let mut rvas = HashMap::new();
    for (s, wide) in strings {
        let rva = if *wide {
            let mut v: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
            v.extend_from_slice(&[0, 0]);
            b.put(&v)
        } else {
            b.cstr(s)
        };
        rvas.insert(s.to_string(), rva);
    }
    let mut exp_dir = (0u32, 0u32);
    if !exports.is_empty() {
        let n = exports.len() as u32;
        let dir = b.reserve(40);
        let eat = b.reserve(4 * exports.len());
        let names = b.reserve(4 * exports.len());
        let ords = b.reserve(2 * exports.len());
        let dllname = b.cstr("test.dll");
        b.w32(dir + 12, dllname);
        b.w32(dir + 16, 1);
        b.w32(dir + 20, n);
        b.w32(dir + 24, n);
        b.w32(dir + 28, eat);
        b.w32(dir + 32, names);
        b.w32(dir + 36, ords);
        for (i, e) in exports.iter().enumerate() {
            let s = b.cstr(e);
            b.w32(eat + 4 * i as u32, TEXT_RVA + i as u32);
            b.w32(names + 4 * i as u32, s);
            b.w16(ords + 2 * i as u32, i as u16);
        }
        exp_dir = (dir, 40);
    }
    let mut imp_dir = (0u32, 0u32);
    if !imports.is_empty() {
        let descs = b.reserve(20 * (imports.len() + 1));
        for (i, (dll, funcs)) in imports.iter().enumerate() {
            let name = b.cstr(dll);
            let ilt = b.reserve(8 * (funcs.len() + 1));
            let iat = b.reserve(8 * (funcs.len() + 1));
            for (k, f) in funcs.iter().enumerate() {
                let t = thunk_of(&mut b, f);
                b.w64(ilt + 8 * k as u32, t);
                b.w64(iat + 8 * k as u32, t);
            }
            let d = descs + 20 * i as u32;
            b.w32(d, ilt);
            b.w32(d + 12, name);
            b.w32(d + 16, iat);
        }
        imp_dir = (descs, 20 * (imports.len() as u32 + 1));
    }
    let mut delay_dir = (0u32, 0u32);
    if !delays.is_empty() {
        let descs = b.reserve(32 * (delays.len() + 1));
        for (i, (dll, funcs)) in delays.iter().enumerate() {
            let name = b.cstr(dll);
            let hmod = b.reserve(8);
            let int = b.reserve(8 * (funcs.len() + 1));
            let iat = b.reserve(8 * (funcs.len() + 1));
            for (k, f) in funcs.iter().enumerate() {
                let t = thunk_of(&mut b, f);
                b.w64(int + 8 * k as u32, t);
            }
            let d = descs + 32 * i as u32;
            b.w32(d, 1);
            b.w32(d + 4, name);
            b.w32(d + 8, hmod);
            b.w32(d + 12, iat);
            b.w32(d + 16, int);
        }
        delay_dir = (descs, 32 * (delays.len() as u32 + 1));
    }
    assert!(b.d.len() <= RDATA_SIZE);
    b.d.resize(RDATA_SIZE, 0);

    let mut f = vec![0u8; RDATA_RAW + RDATA_SIZE];
    f[0] = b'M';
    f[1] = b'Z';
    f[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    f[0x40..0x44].copy_from_slice(b"PE\0\0");
    let coff = 0x44;
    f[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
    f[coff + 2..coff + 4].copy_from_slice(&2u16.to_le_bytes());
    f[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
    f[coff + 18..coff + 20].copy_from_slice(&0x2022u16.to_le_bytes());
    let opt = coff + 20;
    let put32 = |f: &mut Vec<u8>, o: usize, v: u32| f[o..o + 4].copy_from_slice(&v.to_le_bytes());
    f[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
    f[opt + 24..opt + 32].copy_from_slice(&IMAGE_BASE.to_le_bytes());
    put32(&mut f, opt + 32, 0x1000);
    put32(&mut f, opt + 36, 0x200);
    put32(&mut f, opt + 56, RDATA_RVA + RDATA_SIZE as u32);
    put32(&mut f, opt + 60, 0x400);
    f[opt + 68..opt + 70].copy_from_slice(&2u16.to_le_bytes());
    put32(&mut f, opt + 108, 16);
    let dd = opt + 112;
    for (idx, (rva, size)) in [(0usize, exp_dir), (1, imp_dir), (13, delay_dir)] {
        put32(&mut f, dd + idx * 8, rva);
        put32(&mut f, dd + idx * 8 + 4, size);
    }
    let sec = opt + 240;
    f[sec..sec + 5].copy_from_slice(b".text");
    put32(&mut f, sec + 8, TEXT_SIZE as u32);
    put32(&mut f, sec + 12, TEXT_RVA);
    put32(&mut f, sec + 16, TEXT_SIZE as u32);
    put32(&mut f, sec + 20, TEXT_RAW as u32);
    put32(&mut f, sec + 36, 0x6000_0020);
    let s2 = sec + 40;
    f[s2..s2 + 6].copy_from_slice(b".rdata");
    put32(&mut f, s2 + 8, RDATA_SIZE as u32);
    put32(&mut f, s2 + 12, RDATA_RVA);
    put32(&mut f, s2 + 16, RDATA_SIZE as u32);
    put32(&mut f, s2 + 20, RDATA_RAW as u32);
    put32(&mut f, s2 + 36, 0xC000_0040);
    for i in 0..TEXT_SIZE {
        f[TEXT_RAW + i] = 0xCC;
    }
    assert!(text.len() <= TEXT_SIZE);
    f[TEXT_RAW..TEXT_RAW + text.len()].copy_from_slice(text);
    f[RDATA_RAW..RDATA_RAW + RDATA_SIZE].copy_from_slice(&b.d);
    (f, rvas)
}
