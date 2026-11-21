use crate::abi::*;
use crate::sys::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const E_INVALIDARG: i32 = 0x80070057u32 as i32;
pub const GTL_E_BAD_VARIANT: i32 = 0x80EB0013u32 as i32;
pub const GTL_E_MIXED_VECTOR: i32 = 0x80EB0014u32 as i32;
pub const GTL_E_ARRAY_TYPE: i32 = 0x80EB0015u32 as i32;
pub const GTL_E_ARRAY_SIZE: i32 = 0x80EB0016u32 as i32;
pub const GTL_S_NOT_ENABLED: i32 = 0xEB0001;
pub const CRIT_KW: u64 = 0x800000000000;

pub fn utf8_name(out: &mut Vec<u8>, s: &[u16]) {
    let mut i = 0;
    while i < s.len() {
        let c = s[i] as u32;
        if c < 0x80 {
            out.push(c as u8);
        } else if c < 0x800 {
            out.push((0xc0 | (c >> 6)) as u8);
            out.push((0x80 | (c & 0x3f)) as u8);
        } else if (0xd800..0xdc00).contains(&c) && i + 1 < s.len() && (0xdc00..0xe000).contains(&(s[i + 1] as u32)) {
            let cp = 0x10000 + ((c - 0xd800) << 10) + (s[i + 1] as u32 - 0xdc00);
            out.push((0xf0 | (cp >> 18)) as u8);
            out.push((0x80 | ((cp >> 12) & 0x3f)) as u8);
            out.push((0x80 | ((cp >> 6) & 0x3f)) as u8);
            out.push((0x80 | (cp & 0x3f)) as u8);
            i += 1;
        } else {
            out.push((0xe0 | (c >> 12)) as u8);
            out.push((0x80 | ((c >> 6) & 0x3f)) as u8);
            out.push((0x80 | (c & 0x3f)) as u8);
        }
        i += 1;
    }
    out.push(0);
}

pub fn tags_bytes(out: &mut Vec<u8>, tags: u32) {
    let mut t = tags;
    while t & 0x1fffff != 0 {
        out.push(((t >> 21) | 0x80) as u8);
        t <<= 7;
    }
    out.push(((t >> 21) & 0x7f) as u8);
}

pub struct Enc {
    pub meta: Vec<u8>,
    pub payload: Vec<u8>,
}

impl Enc {
    pub fn new(name: &[u16], tags: u32) -> Enc {
        let mut meta = vec![0u8, 0u8];
        tags_bytes(&mut meta, tags);
        utf8_name(&mut meta, name);
        Enc { meta, payload: Vec::new() }
    }

    fn bump(&mut self, w: usize) {
        if w != 0 {
            self.meta[w] = self.meta[w].wrapping_add(1);
        }
    }

    pub fn field(&mut self, w: usize, name: &[u16], ty: u8) {
        utf8_name(&mut self.meta, name);
        self.meta.push(ty);
        self.bump(w);
    }

    pub fn open(&mut self, w: usize, name: &[u16], ty: u8) -> usize {
        utf8_name(&mut self.meta, name);
        self.meta.push(ty);
        let pos = self.meta.len();
        self.meta.push(0);
        self.bump(w);
        pos
    }

    pub fn finish(&mut self) -> Result<(), i32> {
        let n = self.meta.len();
        self.meta[0] = n as u8;
        self.meta[1] = (n >> 8) as u8;
        if n >= 0x8000 {
            return Err(E_INVALIDARG);
        }
        Ok(())
    }
}

pub fn prim_type(p: &Prim) -> Result<u8, i32> {
    Ok(match p.idx {
        0 | 2 => 2,
        1 | 3 => 1,
        4 => 3,
        5 => 4,
        6 => 5,
        7 => 6,
        8 => 7,
        9 => 8,
        10 => 10,
        11 => 9,
        12 => 11,
        13 => 12,
        14 => 13,
        15 => 15,
        _ => return Err(GTL_E_BAD_VARIANT),
    })
}

pub unsafe fn prim_data(out: &mut Vec<u8>, p: &Prim) -> Result<(), i32> {
    let d0 = p.d[0];
    match p.idx {
        0 | 2 => {
            out.extend_from_slice(cstr(d0 as *const u8));
            out.push(0);
        }
        1 | 3 => {
            for c in wstr(d0 as *const u16) {
                out.extend_from_slice(&c.to_le_bytes());
            }
            out.extend_from_slice(&[0, 0]);
        }
        4 | 5 => out.push(d0 as u8),
        6 | 7 => out.extend_from_slice(&(d0 as u16).to_le_bytes()),
        8 | 9 | 12 => out.extend_from_slice(&(d0 as u32).to_le_bytes()),
        10 | 11 | 13 => out.extend_from_slice(&d0.to_le_bytes()),
        14 => out.extend_from_slice(&((d0 as u8) as u32).to_le_bytes()),
        15 => {
            out.extend_from_slice(&d0.to_le_bytes());
            out.extend_from_slice(&p.d[1].to_le_bytes());
        }
        _ => return Err(GTL_E_BAD_VARIANT),
    }
    Ok(())
}

unsafe fn prim_field(e: &mut Enc, w: usize, name: &[u16], p: &Prim) -> Result<(), i32> {
    let t = prim_type(p)?;
    e.field(w, name, t);
    prim_data(&mut e.payload, p)
}

pub unsafe fn encode_coll(e: &mut Enc, w: usize, name: &[u16], c: &Coll) -> Result<(), i32> {
    match c.idx {
        0 => {
            let sw = e.open(w, name, 0x98);
            for n in c.as_map().nodes() {
                prim_field(e, sw, (*nkey(n)).wtext(), &*nval::<Prim>(n))?;
            }
            Ok(())
        }
        1 => {
            let items = vec_items::<Prim>(&c.d);
            if items.is_empty() {
                return Ok(());
            }
            let t0 = prim_type(&items[0])?;
            e.field(w, name, t0 | 0x40);
            e.payload.extend_from_slice(&(items.len() as u16).to_le_bytes());
            for p in items {
                if prim_type(p)? != t0 {
                    return Err(GTL_E_MIXED_VECTOR);
                }
                prim_data(&mut e.payload, p)?;
            }
            Ok(())
        }
        2 => {
            let maps = vec_items::<RawMap>(&c.d);
            let sw = e.open(w, name, 0xd8);
            e.payload.extend_from_slice(&(maps.len() as u16).to_le_bytes());
            let mut types: std::collections::BTreeMap<usize, u8> = std::collections::BTreeMap::new();
            let mut size0 = 0usize;
            for (i, m) in maps.iter().enumerate() {
                if i == 0 {
                    size0 = m.size;
                    for n in m.nodes() {
                        let p = &*nval::<Prim>(n);
                        let t = prim_type(p)?;
                        types.insert((*nkey(n)).ptr as usize, t);
                        e.field(sw, (*nkey(n)).wtext(), t);
                        prim_data(&mut e.payload, p)?;
                    }
                } else {
                    if m.size != size0 {
                        return Err(GTL_E_ARRAY_SIZE);
                    }
                    for n in m.nodes() {
                        let p = &*nval::<Prim>(n);
                        let t = prim_type(p)?;
                        if let Some(t0) = types.get(&((*nkey(n)).ptr as usize)) {
                            if *t0 != t {
                                return Err(GTL_E_ARRAY_TYPE);
                            }
                        }
                        prim_data(&mut e.payload, p)?;
                    }
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub unsafe fn encode_b(e: &mut Enc, w: usize, name: &[u16], b: &BVar) -> Result<(), i32> {
    match b.idx {
        0 => prim_field(e, w, name, b.prim()),
        1 => encode_coll(e, w, name, b.coll()),
        _ => Err(GTL_E_BAD_VARIANT),
    }
}

pub unsafe fn is_tag_name(v: &Value) -> bool {
    !v.name.ptr.is_null() && v.name.wtext() == w("GTL__TlgEventTag").as_slice()
}

pub unsafe fn encode_value(e: &mut Enc, v: &Value) -> Result<(), i32> {
    match v.aidx {
        0 => {
            if is_tag_name(v) {
                return Ok(());
            }
            encode_b(e, 0, v.name.wtext(), v.bvar())
        }
        1 => {
            let sw = e.open(0, v.name.wtext(), 0x98);
            for n in v.map().nodes() {
                encode_b(e, sw, (*nkey(n)).wtext(), &*nval::<BVar>(n))?;
            }
            Ok(())
        }
        _ => Err(GTL_E_BAD_VARIANT),
    }
}

pub unsafe fn build_event(name: &[u16], tags: u32, privacy: u64, vals: &[Value]) -> Result<Enc, i32> {
    let mut e = Enc::new(name, tags);
    e.field(0, &w("PartA_PrivTags"), 0x0a);
    e.payload.extend_from_slice(&privacy.to_le_bytes());
    for v in vals {
        encode_value(&mut e, v)?;
    }
    e.finish()?;
    Ok(e)
}

pub struct Prov {
    pub level1: AtomicU32,
    pub any: AtomicU64,
    pub all: AtomicU64,
    pub handle: AtomicU64,
    pub traits: Vec<u8>,
    pub status: i32,
}

pub unsafe extern "system" fn enable_cb(_src: *const Guid, enabled: u32, level: u8, any: u64, all: u64, _f: *const c_void, ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let p = &*(ctx as *const Prov);
    match enabled {
        1 => {
            p.any.store(any, Ordering::SeqCst);
            p.all.store(all, Ordering::SeqCst);
            p.level1.store(if level == 0 { 0x100 } else { level as u32 + 1 }, Ordering::SeqCst);
        }
        0 => p.level1.store(0, Ordering::SeqCst),
        _ => {}
    }
}

pub fn traits_blob(name: &[u16], group: Option<&Guid>) -> Vec<u8> {
    let mut t = vec![0u8, 0u8];
    utf8_name(&mut t, name);
    if let Some(g) = group {
        t.extend_from_slice(&[0x13, 0x00, 0x01]);
        t.extend_from_slice(&g.bytes());
    }
    let n = t.len();
    t[0] = n as u8;
    t[1] = (n >> 8) as u8;
    t
}

impl Prov {
    pub fn new(traits: Vec<u8>) -> Box<Prov> {
        Box::new(Prov { level1: AtomicU32::new(0), any: AtomicU64::new(0), all: AtomicU64::new(0), handle: AtomicU64::new(0), traits, status: 0 })
    }

    pub fn register(&mut self, guid: &Guid) -> i32 {
        let mut h = 0u64;
        let r = unsafe { EventRegister(guid, Some(enable_cb), self as *mut Prov as *mut c_void, &mut h) };
        if r != 0 {
            self.status = hr_from_win32(r);
            return self.status;
        }
        self.handle.store(h, Ordering::SeqCst);
        unsafe { EventSetInformation(h, 2, self.traits.as_ptr() as *const c_void, self.traits.len() as u32) };
        0
    }

    pub fn unregister(&self) {
        let h = self.handle.swap(0, Ordering::SeqCst);
        if h != 0 {
            unsafe { EventUnregister(h) };
        }
        self.level1.store(0, Ordering::SeqCst);
    }

    pub fn registered(&self) -> bool {
        self.handle.load(Ordering::SeqCst) != 0
    }

    pub fn enabled(&self, level: u8, kw: u64) -> bool {
        if (level as u32) >= self.level1.load(Ordering::SeqCst) {
            return false;
        }
        if kw == 0 {
            return true;
        }
        let all = self.all.load(Ordering::SeqCst);
        (kw & self.any.load(Ordering::SeqCst)) != 0 && (kw & all) == all
    }

    pub fn crit_enabled(&self) -> bool {
        if self.level1.load(Ordering::SeqCst) <= 5 {
            return false;
        }
        let all = self.all.load(Ordering::SeqCst);
        (CRIT_KW & self.any.load(Ordering::SeqCst)) != 0 && (CRIT_KW & all) == all
    }

    fn transfer(&self, desc: &EventDescriptor, meta: &[u8], payload: &[u8]) -> u32 {
        let data = [
            EventData { ptr: self.traits.as_ptr() as u64, size: u16::from_le_bytes([self.traits[0], self.traits[1]]) as u32, kind: 2 },
            EventData { ptr: meta.as_ptr() as u64, size: u16::from_le_bytes([meta[0], meta[1]]) as u32, kind: 1 },
            EventData { ptr: if payload.is_empty() { 0 } else { payload.as_ptr() as u64 }, size: payload.len() as u32, kind: 0 },
        ];
        let n = if payload.is_empty() { 2 } else { 3 };
        unsafe { EventWriteTransfer(self.handle.load(Ordering::SeqCst), desc, std::ptr::null(), std::ptr::null(), n, data.as_ptr()) }
    }

    pub unsafe fn write(&self, name: &[u16], level: u8, kw: u64, tags: u32, privacy: u64, vals: &[Value]) -> i32 {
        let e = match build_event(name, tags, privacy, vals) {
            Ok(e) => e,
            Err(hr) => return hr,
        };
        let desc = EventDescriptor { id: 0, version: 0, channel: 11, level, opcode: 0, task: 0, keyword: kw };
        let hr = hr_from_win32(self.transfer(&desc, &e.meta, &e.payload));
        if hr < 0 {
            return hr;
        }
        if self.enabled(level, kw) {
            0
        } else {
            GTL_S_NOT_ENABLED
        }
    }

    pub fn write_static(&self, meta: &[u8], payload: &[u8]) -> u32 {
        let desc = EventDescriptor { id: 0, version: 0, channel: 11, level: 5, opcode: 0, task: 0, keyword: CRIT_KW };
        self.transfer(&desc, meta, payload)
    }
}

pub fn static_meta(name: &str, fields: &[(&str, u8)]) -> Vec<u8> {
    let mut m = vec![0u8, 0u8, 0u8];
    m.extend_from_slice(name.as_bytes());
    m.push(0);
    for (f, t) in fields {
        m.extend_from_slice(f.as_bytes());
        m.push(0);
        m.push(*t);
    }
    let n = m.len();
    m[0] = n as u8;
    m[1] = (n >> 8) as u8;
    m
}

pub fn meta_error_short() -> Vec<u8> {
    static_meta("Error", &[("PartA_PrivTags", 0x0a), ("componentName", 1), ("extendedData", 1), ("identifier", 1), ("errorCode", 7), ("message", 1), ("properties", 7), ("featureName", 1), ("customAttributes", 1)])
}

pub fn meta_provider_unregistered() -> Vec<u8> {
    static_meta("ProviderUnregistered", &[("PartA_PrivTags", 0x0a), ("providerNamespace", 1), ("uptimeMs", 0x0a)])
}

pub fn meta_provider_registered() -> Vec<u8> {
    static_meta("ProviderRegistered", &[("PartA_PrivTags", 0x0a), ("providerNamespace", 1)])
}

pub fn meta_active_user() -> Vec<u8> {
    static_meta("ActiveUserUpdated", &[("PartA_PrivTags", 0x0a), ("__TlgCV__", 2), ("componentName", 1), ("extendedData", 1)])
}

pub fn meta_error_full() -> Vec<u8> {
    static_meta(
        "Error",
        &[("PartA_PrivTags", 0x0a), ("__TlgCV__", 2), ("componentName", 1), ("extendedData", 1), ("identifier", 1), ("errorCode", 9), ("message", 1), ("properties", 0x0a), ("callStack", 1), ("featureName", 1)],
    )
}

pub fn meta_session_started() -> Vec<u8> {
    static_meta("SessionStarted", &[("PartA_PrivTags", 0x0a), ("__TlgCV__", 2), ("componentName", 1), ("extendedData", 1), ("customAttributes", 1)])
}

pub fn meta_session_stopped() -> Vec<u8> {
    static_meta(
        "SessionStopped",
        &[
            ("PartA_PrivTags", 0x0a),
            ("__TlgCV__", 2),
            ("componentName", 1),
            ("extendedData", 1),
            ("duration", 0x0a),
            ("state", 1),
            ("endStatus", 7),
            ("totalErrorCount", 8),
            ("customAttributes", 1),
        ],
    )
}

#[derive(Default)]
pub struct Pay(pub Vec<u8>);

impl Pay {
    pub fn u64(mut self, v: u64) -> Pay {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u32(mut self, v: u32) -> Pay {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn wide(mut self, s: &[u16]) -> Pay {
        for c in s {
            self.0.extend_from_slice(&c.to_le_bytes());
        }
        self.0.extend_from_slice(&[0, 0]);
        self
    }
    pub fn ansi(mut self, s: &[u8]) -> Pay {
        self.0.extend_from_slice(s);
        self.0.push(0);
        self
    }
}

pub const CRIT_GUID: Guid = Guid::new(0xcc4ce0cd, 0xdcde, 0x4ed2, [0x8f, 0xc3, 0xbd, 0x97, 0x59, 0x21, 0xce, 0x17]);
pub const GROUP_C7DE: Guid = Guid::new(0xc7de053a, 0x0c2e, 0x4a44, [0x91, 0xa2, 0x52, 0x22, 0xec, 0x2e, 0xcd, 0xf1]);
pub const GROUP_4F50: Guid = Guid::new(0x4f50731a, 0x89cf, 0x4782, [0xb3, 0xe0, 0xdc, 0xe8, 0xc9, 0x04, 0x76, 0xba]);
pub const GROUP_5ECB: Guid = Guid::new(0x5ecb0bac, 0xb930, 0x47f5, [0xa8, 0xa4, 0xe8, 0x25, 0x35, 0x29, 0xed, 0xb7]);

pub fn crit_traits(i: usize) -> Vec<u8> {
    traits_blob(&w("Microsoft.Gaming.Critical"), Some(if i % 2 == 0 { &GROUP_C7DE } else { &GROUP_4F50 }))
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
    let mut m = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&bits.to_be_bytes());
    for chunk in m.chunks(64) {
        let mut wv = [0u32; 80];
        for i in 0..16 {
            wv[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            wv[i] = (wv[i - 3] ^ wv[i - 8] ^ wv[i - 14] ^ wv[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, wi) in wv.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a827999),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6u32),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for i in 0..5 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

const NS: [u8; 16] = [0x48, 0x2c, 0x2d, 0xb2, 0xc3, 0x90, 0x47, 0xc8, 0x87, 0xf8, 0x1a, 0x15, 0xbf, 0xc1, 0x30, 0xfb];

pub fn name_guid(name: &[u16]) -> Guid {
    let mut data = NS.to_vec();
    let mut i = 0;
    while i < name.len() {
        let mut n = 0;
        let mut chunk = [0u16; 16];
        while i < name.len() {
            let c = name[i];
            chunk[n] = c;
            n += 1;
            i += 1;
            if (0xd800..0xdc00).contains(&c) && i < name.len() && (0xdc00..0xe000).contains(&name[i]) {
                chunk[n] = name[i];
                n += 1;
                i += 1;
            }
            if n >= 15 {
                break;
            }
        }
        let mut dst = [0u16; 16];
        unsafe { LCMapStringEx(EMPTY_W.as_ptr(), 0xa00, chunk.as_ptr(), n as i32, dst.as_mut_ptr(), n as i32, std::ptr::null_mut(), std::ptr::null_mut(), 0) };
        for c in &dst[..n] {
            data.extend_from_slice(&c.to_le_bytes());
        }
    }
    let h = sha1(&data);
    let mut b: [u8; 16] = h[..16].try_into().unwrap();
    b[7] = (b[7] & 0x0f) | 0x50;
    Guid::from_bytes(&b)
}
