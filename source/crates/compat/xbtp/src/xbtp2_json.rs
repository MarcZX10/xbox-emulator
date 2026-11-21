use crate::sys::*;
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub const E_JSON_NOT_FOUND: HRESULT = 0x8375_0009u32 as i32;
pub const E_JSON_BAD_NUMBER: HRESULT = 0x8375_0008u32 as i32;
pub const E_DATATYPE_MISMATCH: HRESULT = 0x8007_065Du32 as i32;
pub const E_UNEXPECTED_JSON_TRANSLATION: HRESULT = 0x8C11_0401u32 as i32;

pub const IID_IUNKNOWN: GUID = GUID::new(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
pub const IID_XBTP_JSON_OBJECT: GUID = GUID::new(0x49f43917, 0xb1df, 0x41ce, [0x84, 0xd0, 0xe0, 0x5d, 0xc7, 0x6a, 0x9c, 0xf1]);
pub const IID_XBTP_JSON_VALUE_TYPE: GUID = GUID::new(0xb115ac26, 0xaef7, 0x4798, [0xba, 0x2b, 0x22, 0xbc, 0xb9, 0x14, 0x82, 0x73]);
pub const IID_XBTP_JSON_ARRAY_BASE: GUID = GUID::new(0xe3209857, 0xad8a, 0x46ac, [0xab, 0xc0, 0x43, 0xc2, 0xfb, 0xdf, 0xb4, 0xa8]);
pub const IID_XBTP_JSON_ARRAY: GUID = GUID::new(0x5fd3d843, 0xbf4b, 0x4544, [0xb3, 0xb0, 0x58, 0x4c, 0x00, 0xa4, 0x66, 0xf9]);
pub const IID_XBTP_JSON_ARRAY_IMPL: GUID = GUID::new(0xa07d8c36, 0x9a80, 0x4e75, [0x87, 0x2a, 0x57, 0x00, 0x37, 0x61, 0xd2, 0x33]);
pub const IID_XBTP_JSON_OBJECT_BASE: GUID = GUID::new(0xcd4d1f43, 0x90fe, 0x4c86, [0x98, 0xcc, 0xb4, 0x43, 0x85, 0xf5, 0xb3, 0x96]);
pub const IID_XBTP_JSON_OBJECT_IMPL: GUID = GUID::new(0x5b795c7e, 0x7064, 0x49cc, [0xb3, 0xe6, 0x11, 0x5c, 0xfb, 0xb4, 0x74, 0x48]);
pub const IID_XBTP_JSON_VALUE_TYPE_BASE: GUID = GUID::new(0xabda8d2e, 0xe697, 0x41b8, [0xa2, 0x5e, 0x95, 0xa8, 0x87, 0xaa, 0xb4, 0xd5]);

const CP_UTF8: u32 = 65001;

extern "C" {
    fn __stdio_common_vsprintf(options: u64, buf: *mut u8, count: usize, fmt: *const u8, locale: *mut c_void, args: *const c_void) -> i32;
    fn __stdio_common_vsscanf(options: u64, buf: *const u8, count: usize, fmt: *const u8, locale: *mut c_void, args: *const c_void) -> i32;
}

pub enum Val {
    Empty,
    Null,
    Str(Vec<u8>),
    Bool(u32),
    Num(f64),
    Obj(Map),
    Arr(Vec<NodeRef>),
}

pub struct Node(UnsafeCell<Val>);
unsafe impl Send for Node {}
unsafe impl Sync for Node {}
pub type NodeRef = Arc<Node>;

impl Node {
    pub fn new(v: Val) -> NodeRef {
        Arc::new(Node(UnsafeCell::new(v)))
    }
    #[allow(clippy::mut_from_ref)]
    pub fn val(&self) -> &mut Val {
        unsafe { &mut *self.0.get() }
    }
    pub fn kind(&self) -> u32 {
        match self.val() {
            Val::Empty => 0,
            Val::Null => 1,
            Val::Str(_) => 2,
            Val::Bool(_) => 3,
            Val::Num(_) => 4,
            Val::Obj(_) => 5,
            Val::Arr(_) => 6,
        }
    }
}

pub struct Entry {
    pub key: Vec<u8>,
    pub hash: u32,
    pub val: NodeRef,
}

pub struct Map {
    pub buckets: Vec<Vec<Entry>>,
    pub count: u32,
}

impl Default for Map {
    fn default() -> Self {
        Map { buckets: vec![Vec::new(), Vec::new()], count: 0 }
    }
}

fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(i) => &b[..i],
        None => b,
    }
}

fn at(b: &[u8], i: usize) -> u8 {
    b.get(i).copied().unwrap_or(0)
}

pub fn dec8(b: &[u8], i: usize) -> (u32, usize) {
    let b0 = at(b, i);
    if b0 < 0x80 {
        return (b0 as u32, 1);
    }
    let b1 = at(b, i + 1);
    if b0 & 0xe0 == 0xc0 && b1 & 0xc0 == 0x80 {
        return ((((b0 & 0x1f) as u32) << 6) | (b1 & 0x3f) as u32, 2);
    }
    let b2 = at(b, i + 2);
    if b0 & 0xf0 == 0xe0 && b1 & 0xc0 == 0x80 && b2 & 0xc0 == 0x80 {
        let a = (((b0 & 0xf) as u32) << 6) | (b1 & 0x3f) as u32;
        return ((a << 6) | (b2 & 0x3f) as u32, 3);
    }
    let b3 = at(b, i + 3);
    if b0 & 0xf8 == 0xf0 && b1 & 0xc0 == 0x80 && b2 & 0xc0 == 0x80 && b3 & 0xc0 == 0x80 {
        let a = (((b0 & 0xf) as u32) << 6) | (b1 & 0x3f) as u32;
        let a = (a << 6) | (b2 & 0x3f) as u32;
        return ((a << 6) | (b3 & 0x3f) as u32, 4);
    }
    (0x3f, 1)
}

pub unsafe fn dec16(p: *const u16) -> (u32, usize) {
    let w0 = *p;
    if w0 == 0 {
        return (0, 1);
    }
    let w1 = *p.add(1);
    if w1 != 0 && w0 & 0xdc00 == 0xd800 && w1 & 0xdc00 == 0xdc00 {
        return (((((w0 as u32) & 0x3bf) | 0x40) << 10) | ((w1 as u32) & 0x3ff), 2);
    }
    (w0 as u32, 1)
}

const EMPTY_W: [u16; 1] = [0];

pub unsafe fn hash_wide(name: *const u16) -> u32 {
    let mut p = if name.is_null() { EMPTY_W.as_ptr() } else { name };
    let mut h = 0u32;
    let mut i = 0u32;
    loop {
        let (c, n) = dec16(p);
        i = i.wrapping_add(1);
        h ^= c.wrapping_mul(i).wrapping_mul(0x49d);
        p = p.add(n);
        if c == 0 {
            return h;
        }
    }
}

pub fn hash_utf8(key: &[u8]) -> u32 {
    let mut i = 0usize;
    let mut h = 0u32;
    let mut k = 0u32;
    loop {
        let (c, n) = dec8(key, i);
        k = k.wrapping_add(1);
        h ^= c.wrapping_mul(k).wrapping_mul(0x49d);
        i += n;
        if c == 0 {
            return h;
        }
    }
}

unsafe fn key_matches(key: &[u8], name: *const u16) -> bool {
    let mut p = if name.is_null() { EMPTY_W.as_ptr() } else { name };
    let mut i = 0usize;
    let (mut a, n) = dec8(key, i);
    i += n;
    let (mut b, m) = dec16(p);
    p = p.add(m);
    while a != 0 {
        if a != b {
            break;
        }
        let (x, n) = dec8(key, i);
        i += n;
        let (y, m) = dec16(p);
        p = p.add(m);
        a = x;
        b = y;
    }
    a == b
}

impl Map {
    pub unsafe fn lookup(&self, name: *const u16) -> Option<NodeRef> {
        let h = hash_wide(name);
        let chain = &self.buckets[(h % self.buckets.len() as u32) as usize];
        chain.iter().find(|e| key_matches(&e.key, name)).map(|e| e.val.clone())
    }

    pub fn insert(&mut self, key: Vec<u8>, hash: u32, val: NodeRef) {
        let key = cstr(&key).to_vec();
        let n = self.buckets.len() as u32;
        let b = (hash % n) as usize;
        self.buckets[b].retain(|e| cstr(&e.key) != key.as_slice());
        let want = self.count.wrapping_add(1) >> 2;
        if want > n {
            let nn = n * 2;
            let mut fresh: Vec<Vec<Entry>> = (0..nn).map(|_| Vec::new()).collect();
            for chain in self.buckets.drain(..) {
                for e in chain {
                    let t = (e.hash % nn) as usize;
                    fresh[t].insert(0, e);
                }
            }
            self.buckets = fresh;
        }
        self.count = self.count.wrapping_add(1);
        let t = (hash % self.buckets.len() as u32) as usize;
        self.buckets[t].insert(0, Entry { key, hash, val });
    }
}

pub unsafe fn wide_to_utf8(s: *const u16) -> Result<Vec<u8>, HRESULT> {
    if s.is_null() || *s == 0 {
        return Ok(Vec::new());
    }
    let n = WideCharToMultiByte(CP_UTF8, 0, s, -1, null_mut(), 0, core::ptr::null(), null_mut());
    if n <= 0 || n > 0x7f00_0000 {
        return Err(E_OUTOFMEMORY);
    }
    let mut v = vec![0u8; n as usize];
    if WideCharToMultiByte(CP_UTF8, 0, s, -1, v.as_mut_ptr(), n, core::ptr::null(), null_mut()) != n {
        return Err(E_OUTOFMEMORY);
    }
    v.truncate(n as usize - 1);
    Ok(cstr(&v).to_vec())
}

pub unsafe fn utf8_to_bstr(s: &[u8], out: *mut BSTR) -> HRESULT {
    let mut z = cstr(s).to_vec();
    z.push(0);
    let n = MultiByteToWideChar(CP_UTF8, 0, z.as_ptr(), -1, null_mut(), 0);
    if n <= 0 {
        return hr_last_error();
    }
    let b = SysAllocStringLen(core::ptr::null(), (n - 1) as u32);
    *out = b;
    if b.is_null() {
        return E_OUTOFMEMORY;
    }
    if MultiByteToWideChar(CP_UTF8, 0, z.as_ptr(), -1, b, n) != n {
        SysFreeString(b);
        *out = null_mut();
        return E_UNEXPECTED_JSON_TRANSLATION;
    }
    S_OK
}

fn encode(buf: &mut [u8], at: usize, cap: u32, cp: u32) -> u32 {
    if cap >= 1 && cp <= 0x7f {
        buf[at] = cp as u8;
        return 1;
    }
    if cap >= 2 && cp <= 0x7ff {
        buf[at] = 0xc0 | ((cp >> 6) & 0x1f) as u8;
        buf[at + 1] = 0x80 | (cp & 0x3f) as u8;
        return 2;
    }
    let (lead, n) = if cap >= 3 && cp <= 0xffff {
        (0xe0 | ((cp >> 12) & 0xf) as u8, 3)
    } else if cap >= 4 {
        (0xf0 | ((cp >> 18) & 7) as u8, 4)
    } else {
        return 0;
    };
    buf[at] = lead;
    buf[at + 1] = 0x80 | ((cp >> 6) & 0x3f) as u8;
    buf[at + 2] = 0x80 | (cp & 0x3f) as u8;
    n
}

fn hex4(buf: &[u8], i: usize) -> (u32, u32) {
    let digit = |c: u8| -> Option<u32> {
        match c {
            b'0'..=b'9' => Some((c - b'0') as u32),
            b'a'..=b'f' => Some((c - b'a' + 10) as u32),
            b'A'..=b'F' => Some((c - b'A' + 10) as u32),
            _ => None,
        }
    };
    let mut cnt = 0u32;
    let mut v = 0u32;
    for _ in 0..4 {
        let d = match digit(at(buf, i + cnt as usize)) {
            Some(d) => {
                cnt += 1;
                d
            }
            None => 0,
        };
        v = (v << 4) | d;
    }
    (v, cnt)
}

pub struct Parser {
    pub buf: Vec<u8>,
    pub len: u32,
}

impl Parser {
    pub fn new(text: &[u8]) -> Self {
        let mut buf = text.to_vec();
        let len = buf.len() as u32;
        buf.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        Parser { buf, len }
    }

    fn ws(&self, from: u32) -> u32 {
        if from > self.len {
            return 0;
        }
        let mut k = 0;
        while matches!(at(&self.buf, (from + k) as usize), b'\t' | b'\n' | b'\r' | b' ') {
            k += 1;
        }
        k
    }

    fn next(&mut self, start: &mut u32, toklen: &mut u32, pos: &mut u32) -> u8 {
        let len = self.len;
        let r8 = *start + self.ws(*start);
        *start = r8;
        if r8 > len {
            return 0;
        }
        let c = at(&self.buf, r8 as usize);
        if matches!(c, b',' | b':' | b'[' | b']' | b'{' | b'}') {
            *toklen = 1;
            *pos = r8 + 1;
            return c;
        }
        if c == b'"' {
            let mut ebp = r8 + 1;
            *start = ebp;
            let mut pending = 0u32;
            let mut ebx = ebp;
            let mut esc = false;
            loop {
                let ch = at(&self.buf, ebp as usize);
                if ch == 0 || (ch == b'"' && !esc) {
                    break;
                }
                let (cp, n) = dec8(&self.buf, ebp as usize);
                ebp += n as u32;
                if esc {
                    let m = match cp {
                        0x22 => Some(0x22u8),
                        0x5c => Some(0x5c),
                        0x2f => Some(0x2f),
                        0x62 => Some(8),
                        0x66 => Some(0xc),
                        0x6e => Some(0xa),
                        0x72 => Some(0xd),
                        0x74 => Some(9),
                        _ => None,
                    };
                    if let Some(m) = m {
                        self.buf[ebx as usize] = m;
                        ebx += 1;
                    } else if cp == 0x75 {
                        let (mut v, cnt) = hex4(&self.buf, ebp as usize);
                        ebp += cnt;
                        if cnt != 4 {
                            *pos = *start;
                            *toklen = 0;
                            return b'?';
                        }
                        if v.wrapping_sub(0xd800) <= 0x3ff {
                            pending = v;
                        } else {
                            let lo = v.wrapping_sub(0xdc00) <= 0x3ff;
                            if lo && pending == 0 {
                            } else {
                                if lo {
                                    v = (((pending & 0x3ff) | 0x400) << 10) | (v & 0x3ff);
                                }
                                ebx += encode(&mut self.buf, ebx as usize, len.wrapping_sub(ebx), v);
                            }
                            pending = 0;
                        }
                    } else {
                        *pos = *start;
                        *toklen = 0;
                        return b'?';
                    }
                    esc = false;
                } else if cp == 0x5c {
                    esc = true;
                } else {
                    ebx += encode(&mut self.buf, ebx as usize, len.wrapping_sub(ebx), cp);
                }
            }
            if at(&self.buf, ebp as usize) == 0 {
                *pos = *start;
                *toklen = 0;
                return b'?';
            }
            ebp += 1;
            *toklen = ebx - *start;
            *pos = ebp;
            self.buf[ebx as usize] = 0;
            return b'"';
        }
        if c == b'-' || c.is_ascii_digit() {
            *toklen = 0;
            *pos = r8;
            return b'0';
        }
        let s = &self.buf[r8 as usize..];
        let (word, tok): (&[u8], u8) = match c {
            b't' => (b"true", b't'),
            b'f' => (b"false", b'f'),
            b'n' => (b"null", b'n'),
            _ => (b"", 0),
        };
        if tok != 0 && s.starts_with(word) {
            *toklen = word.len() as u32;
            *pos = r8 + word.len() as u32;
            return tok;
        }
        *pos = r8;
        *toklen = 0;
        b'?'
    }

    fn sub(&self, start: u32, len: u32) -> Vec<u8> {
        cstr(&self.buf[start as usize..(start + len) as usize]).to_vec()
    }

    pub fn value(&mut self, pos: &mut u32) -> Result<NodeRef, HRESULT> {
        let mut start = *pos;
        let mut tl = 0u32;
        let tok = self.next(&mut start, &mut tl, pos);
        match tok {
            b'[' => {
                let mut items: Vec<NodeRef> = Vec::new();
                *pos += self.ws(*pos);
                if at(&self.buf, *pos as usize) == b']' {
                    *pos += 1;
                    return Ok(Node::new(Val::Arr(items)));
                }
                loop {
                    items.push(self.value(pos)?);
                    let mut s = *pos;
                    match self.next(&mut s, &mut tl, pos) {
                        b']' => return Ok(Node::new(Val::Arr(items))),
                        b',' => continue,
                        _ => return Err(E_FAIL),
                    }
                }
            }
            b'{' => {
                let mut map = Map::default();
                *pos += self.ws(*pos);
                if at(&self.buf, *pos as usize) == b'}' {
                    *pos += 1;
                    return Ok(Node::new(Val::Obj(map)));
                }
                loop {
                    let mut s = *pos;
                    if self.next(&mut s, &mut tl, pos) != b'"' {
                        return Err(E_FAIL);
                    }
                    let key = self.sub(s, tl);
                    let mut s2 = *pos;
                    if self.next(&mut s2, &mut tl, pos) != b':' {
                        return Err(E_FAIL);
                    }
                    let v = self.value(pos)?;
                    let h = hash_utf8(&key);
                    map.insert(key, h, v);
                    let mut s3 = *pos;
                    match self.next(&mut s3, &mut tl, pos) {
                        b'}' => return Ok(Node::new(Val::Obj(map))),
                        b',' => continue,
                        _ => return Err(E_FAIL),
                    }
                }
            }
            b'0' => {
                let mut v = 0f64;
                let mut n = 0i32;
                let args: [*mut c_void; 2] = [&mut v as *mut f64 as *mut c_void, &mut n as *mut i32 as *mut c_void];
                let r = unsafe { __stdio_common_vsscanf(0, self.buf.as_ptr().add(start as usize), usize::MAX, b"%lf%n\0".as_ptr(), null_mut(), args.as_ptr() as *const c_void) };
                if r < 1 {
                    return Err(E_JSON_BAD_NUMBER);
                }
                *pos += n as u32;
                Ok(Node::new(Val::Num(v)))
            }
            b'"' => Ok(Node::new(Val::Str(self.sub(start, tl)))),
            b't' => Ok(Node::new(Val::Bool(1))),
            b'f' => Ok(Node::new(Val::Bool(0))),
            b'n' => Ok(Node::new(Val::Null)),
            _ => Err(E_FAIL),
        }
    }
}

pub fn parse(text: &[u8]) -> Result<NodeRef, HRESULT> {
    let mut p = Parser::new(cstr(text));
    let mut pos = 0u32;
    p.value(&mut pos)
}

fn escape(out: &mut Vec<u8>, s: &[u8]) {
    let s = cstr(s);
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c < 0x80 {
            let e = match c {
                b'"' | b'\\' | b'/' => Some(c),
                8 => Some(b'b'),
                0xc => Some(b'f'),
                0xa => Some(b'n'),
                0xd => Some(b'r'),
                9 => Some(b't'),
                _ => None,
            };
            match e {
                Some(e) => {
                    out.push(b'\\');
                    out.push(e);
                }
                None => out.push(c),
            }
            i += 1;
        } else {
            let n = if c < 0xe0 { 2 } else if c < 0xf0 { 3 } else { 4 };
            let end = (i + n).min(s.len());
            out.extend_from_slice(&s[i..end]);
            i = end;
        }
    }
}

pub fn format_number(v: f64) -> Option<Vec<u8>> {
    let mut b = [0u8; 64];
    let args: [f64; 1] = [v];
    let r = unsafe { __stdio_common_vsprintf(2, b.as_mut_ptr(), 64, b"%.15lg\0".as_ptr(), null_mut(), args.as_ptr() as *const c_void) };
    if r <= 0 {
        return None;
    }
    Some(b[..r as usize].to_vec())
}

pub fn serialize(n: &Node, out: &mut Vec<u8>) -> HRESULT {
    match n.val() {
        Val::Null => out.extend_from_slice(b"null"),
        Val::Str(s) => {
            out.push(b'"');
            escape(out, s);
            out.push(b'"');
        }
        Val::Bool(b) => out.extend_from_slice(if *b != 0 { b"true" } else { b"false" }),
        Val::Num(v) => match format_number(*v) {
            Some(t) => escape(out, &t),
            None => return E_FAIL,
        },
        Val::Obj(m) => {
            out.push(b'{');
            let mut first = true;
            for chain in &m.buckets {
                for e in chain {
                    if !first {
                        out.push(b',');
                    }
                    first = false;
                    out.push(b'"');
                    escape(out, &e.key);
                    out.push(b'"');
                    out.push(b':');
                    let hr = serialize(&e.val, out);
                    if hr < 0 {
                        return hr;
                    }
                }
            }
            out.push(b'}');
        }
        Val::Arr(a) => {
            out.push(b'[');
            for (i, e) in a.iter().enumerate() {
                if i != 0 {
                    out.push(b',');
                }
                let hr = serialize(e, out);
                if hr < 0 {
                    return hr;
                }
            }
            out.push(b']');
        }
        Val::Empty => return E_FAIL,
    }
    S_OK
}

type Qi = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;
type Ref = unsafe extern "system" fn(*mut c_void) -> u32;

#[repr(C)]
pub struct ObjVtbl {
    pub query_interface: Qi,
    pub add_ref: Ref,
    pub release: Ref,
    pub get_number: unsafe extern "system" fn(*mut JsonObject, *const u16, *mut f64) -> HRESULT,
    pub set_number: unsafe extern "system" fn(*mut JsonObject, *const u16, f64) -> HRESULT,
    pub get_string: unsafe extern "system" fn(*mut JsonObject, *const u16, *mut BSTR) -> HRESULT,
    pub set_string: unsafe extern "system" fn(*mut JsonObject, *const u16, *const u16) -> HRESULT,
    pub get_object_alias: unsafe extern "system" fn(*mut JsonObject, *const u16, *mut *mut c_void) -> HRESULT,
    pub get_object: unsafe extern "system" fn(*mut JsonObject, *const u16, *mut *mut c_void) -> HRESULT,
    pub set_object: unsafe extern "system" fn(*mut JsonObject, *const u16, *mut c_void) -> HRESULT,
    pub get_array: unsafe extern "system" fn(*mut JsonObject, *const u16, *mut *mut c_void) -> HRESULT,
    pub set_array: unsafe extern "system" fn(*mut JsonObject, *const u16, *mut c_void) -> HRESULT,
    pub serialize: unsafe extern "system" fn(*mut JsonObject, *mut BSTR) -> HRESULT,
}

#[repr(C)]
pub struct ArrVtbl {
    pub query_interface: Qi,
    pub add_ref: Ref,
    pub release: Ref,
    pub add_number: unsafe extern "system" fn(*mut JsonArray, f64) -> HRESULT,
    pub add_string: unsafe extern "system" fn(*mut JsonArray, *const u16) -> HRESULT,
    pub add_object: unsafe extern "system" fn(*mut JsonArray, *mut c_void) -> HRESULT,
    pub add_json: unsafe extern "system" fn(*mut JsonArray, *const u16) -> HRESULT,
    pub get_number: unsafe extern "system" fn(*mut JsonArray, u32, *mut f64) -> HRESULT,
    pub get_string: unsafe extern "system" fn(*mut JsonArray, u32, *mut BSTR) -> HRESULT,
    pub get_object: unsafe extern "system" fn(*mut JsonArray, u32, *mut *mut c_void) -> HRESULT,
    pub get_value_type: unsafe extern "system" fn(*mut JsonArray, u32, *mut *mut c_void) -> HRESULT,
    pub get_count: unsafe extern "system" fn(*mut JsonArray, *mut u32) -> HRESULT,
}

#[repr(C)]
pub struct TypeVtbl {
    pub query_interface: Qi,
    pub add_ref: Ref,
    pub release: Ref,
    pub get_type: unsafe extern "system" fn(*mut JsonValueType, *mut u32) -> HRESULT,
}

#[repr(C)]
pub struct JsonObject {
    pub vtbl: *const ObjVtbl,
    pub node: Option<NodeRef>,
    pub refs: AtomicU32,
}

#[repr(C)]
pub struct JsonArray {
    pub vtbl: *const ArrVtbl,
    pub node: Option<NodeRef>,
    pub refs: AtomicU32,
}

#[repr(C)]
pub struct JsonValueType {
    pub vtbl: *const TypeVtbl,
    pub kind: u32,
    pub refs: AtomicU32,
}

unsafe fn qi_common(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void, ids: &[GUID], add: Ref) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    if ids.iter().any(|g| *g == *iid) {
        *out = this;
        add(this);
        S_OK
    } else {
        *out = null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn obj_qi(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    qi_common(this, iid, out, &[IID_XBTP_JSON_OBJECT_BASE, IID_IUNKNOWN, IID_XBTP_JSON_OBJECT, IID_XBTP_JSON_OBJECT_IMPL], obj_add_ref)
}

unsafe extern "system" fn obj_add_ref(this: *mut c_void) -> u32 {
    (*(this as *mut JsonObject)).refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn obj_release(this: *mut c_void) -> u32 {
    let n = (*(this as *mut JsonObject)).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        drop(Box::from_raw(this as *mut JsonObject));
    }
    n
}

unsafe extern "system" fn arr_qi(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    qi_common(this, iid, out, &[IID_XBTP_JSON_ARRAY_BASE, IID_IUNKNOWN, IID_XBTP_JSON_ARRAY, IID_XBTP_JSON_ARRAY_IMPL], arr_add_ref)
}

unsafe extern "system" fn arr_add_ref(this: *mut c_void) -> u32 {
    (*(this as *mut JsonArray)).refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn arr_release(this: *mut c_void) -> u32 {
    let n = (*(this as *mut JsonArray)).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        drop(Box::from_raw(this as *mut JsonArray));
    }
    n
}

unsafe extern "system" fn vt_qi(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    qi_common(this, iid, out, &[IID_XBTP_JSON_VALUE_TYPE_BASE, IID_IUNKNOWN, IID_XBTP_JSON_VALUE_TYPE], vt_add_ref)
}

unsafe extern "system" fn vt_add_ref(this: *mut c_void) -> u32 {
    (*(this as *mut JsonValueType)).refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn vt_release(this: *mut c_void) -> u32 {
    let n = (*(this as *mut JsonValueType)).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        drop(Box::from_raw(this as *mut JsonValueType));
    }
    n
}

unsafe extern "system" fn vt_get_type(this: *mut JsonValueType, out: *mut u32) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    *out = (*this).kind;
    S_OK
}

pub static TYPE_VTBL: TypeVtbl = TypeVtbl { query_interface: vt_qi, add_ref: vt_add_ref, release: vt_release, get_type: vt_get_type };

pub fn new_object(node: Option<NodeRef>) -> *mut JsonObject {
    Box::into_raw(Box::new(JsonObject { vtbl: &OBJ_VTBL, node, refs: AtomicU32::new(1) }))
}

pub fn new_array(node: Option<NodeRef>) -> *mut JsonArray {
    Box::into_raw(Box::new(JsonArray { vtbl: &ARR_VTBL, node, refs: AtomicU32::new(1) }))
}

unsafe fn hand_out(p: *mut c_void, iid: &GUID, out: *mut *mut c_void) -> HRESULT {
    let vt = *(p as *const *const [usize; 3]);
    let qi: Qi = core::mem::transmute((*vt)[0]);
    let rel: Ref = core::mem::transmute((*vt)[2]);
    let hr = qi(p, iid, out);
    rel(p);
    hr
}

unsafe fn obj_ensure(o: *mut JsonObject) -> NodeRef {
    if (*o).node.is_none() {
        (*o).node = Some(Node::new(Val::Obj(Map::default())));
    }
    (*o).node.clone().unwrap()
}

unsafe fn arr_ensure(a: *mut JsonArray) -> NodeRef {
    if (*a).node.is_none() {
        (*a).node = Some(Node::new(Val::Arr(Vec::new())));
    }
    (*a).node.clone().unwrap()
}

unsafe fn obj_map(o: *mut JsonObject) -> Result<&'static mut Map, HRESULT> {
    let n = obj_ensure(o);
    match n.val() {
        Val::Obj(m) => Ok(&mut *(m as *mut Map)),
        _ => Err(E_DATATYPE_MISMATCH),
    }
}

unsafe fn arr_vec(a: *mut JsonArray) -> Result<&'static mut Vec<NodeRef>, HRESULT> {
    let n = arr_ensure(a);
    match n.val() {
        Val::Arr(v) => Ok(&mut *(v as *mut Vec<NodeRef>)),
        _ => Err(E_DATATYPE_MISMATCH),
    }
}

unsafe fn cast_impl(p: *mut c_void, iid: &GUID) -> *mut c_void {
    if p.is_null() {
        return null_mut();
    }
    let vt = *(p as *const *const [usize; 3]);
    let qi: Qi = core::mem::transmute((*vt)[0]);
    let rel: Ref = core::mem::transmute((*vt)[2]);
    let mut q: *mut c_void = null_mut();
    if qi(p, iid, &mut q) < 0 {
        return null_mut();
    }
    rel(q);
    p
}

unsafe fn obj_set(o: *mut JsonObject, name: *const u16, v: NodeRef) -> HRESULT {
    let m = match obj_map(o) {
        Ok(m) => m,
        Err(e) => return e,
    };
    let key = match wide_to_utf8(name) {
        Ok(k) => k,
        Err(e) => return e,
    };
    m.insert(key, hash_wide(name), v);
    S_OK
}

unsafe fn obj_get(o: *mut JsonObject, name: *const u16) -> Result<NodeRef, HRESULT> {
    let m = obj_map(o)?;
    m.lookup(name).ok_or(E_JSON_NOT_FOUND)
}

unsafe extern "system" fn obj_get_number(o: *mut JsonObject, name: *const u16, out: *mut f64) -> HRESULT {
    match obj_get(o, name) {
        Ok(n) => match n.val() {
            Val::Num(v) => {
                *out = *v;
                S_OK
            }
            _ => E_DATATYPE_MISMATCH,
        },
        Err(e) => e,
    }
}

unsafe extern "system" fn obj_set_number(o: *mut JsonObject, name: *const u16, v: f64) -> HRESULT {
    if let Err(e) = obj_map(o) {
        return e;
    }
    obj_set(o, name, Node::new(Val::Num(v)))
}

unsafe extern "system" fn obj_get_string(o: *mut JsonObject, name: *const u16, out: *mut BSTR) -> HRESULT {
    match obj_get(o, name) {
        Ok(n) => match n.val() {
            Val::Str(s) => utf8_to_bstr(s, out),
            _ => E_DATATYPE_MISMATCH,
        },
        Err(e) => e,
    }
}

unsafe extern "system" fn obj_set_string(o: *mut JsonObject, name: *const u16, v: *const u16) -> HRESULT {
    if let Err(e) = obj_map(o) {
        return e;
    }
    let s = match wide_to_utf8(v) {
        Ok(s) => s,
        Err(e) => return e,
    };
    obj_set(o, name, Node::new(Val::Str(s)))
}

unsafe extern "system" fn obj_get_object_alias(o: *mut JsonObject, name: *const u16, out: *mut *mut c_void) -> HRESULT {
    ((*(*o).vtbl).get_object)(o, name, out)
}

unsafe extern "system" fn obj_get_object(o: *mut JsonObject, name: *const u16, out: *mut *mut c_void) -> HRESULT {
    *out = null_mut();
    let n = match obj_get(o, name) {
        Ok(n) => n,
        Err(e) => return e,
    };
    if n.kind() != 5 {
        return E_DATATYPE_MISMATCH;
    }
    hand_out(new_object(Some(n)) as *mut c_void, &IID_XBTP_JSON_OBJECT, out)
}

unsafe extern "system" fn obj_set_object(o: *mut JsonObject, name: *const u16, other: *mut c_void) -> HRESULT {
    let other = cast_impl(other, &IID_XBTP_JSON_OBJECT_IMPL) as *mut JsonObject;
    if other.is_null() {
        return E_INVALIDARG;
    }
    if let Err(e) = obj_map(o) {
        return e;
    }
    let n = obj_ensure(other);
    obj_set(o, name, n)
}

unsafe extern "system" fn obj_get_array(o: *mut JsonObject, name: *const u16, out: *mut *mut c_void) -> HRESULT {
    *out = null_mut();
    let n = match obj_get(o, name) {
        Ok(n) => n,
        Err(e) => return e,
    };
    if n.kind() != 6 {
        return E_DATATYPE_MISMATCH;
    }
    hand_out(new_array(Some(n)) as *mut c_void, &IID_XBTP_JSON_ARRAY, out)
}

unsafe extern "system" fn obj_set_array(o: *mut JsonObject, name: *const u16, arr: *mut c_void) -> HRESULT {
    let a = cast_impl(arr, &IID_XBTP_JSON_ARRAY_IMPL) as *mut JsonArray;
    if a.is_null() {
        return E_INVALIDARG;
    }
    if let Err(e) = obj_map(o) {
        return e;
    }
    let n = arr_ensure(a);
    obj_set(o, name, n)
}

unsafe extern "system" fn obj_serialize(o: *mut JsonObject, out: *mut BSTR) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    let n = obj_ensure(o);
    let mut text = Vec::new();
    let hr = serialize(&n, &mut text);
    if hr < 0 {
        return hr;
    }
    utf8_to_bstr(&text, out)
}

pub static OBJ_VTBL: ObjVtbl = ObjVtbl {
    query_interface: obj_qi,
    add_ref: obj_add_ref,
    release: obj_release,
    get_number: obj_get_number,
    set_number: obj_set_number,
    get_string: obj_get_string,
    set_string: obj_set_string,
    get_object_alias: obj_get_object_alias,
    get_object: obj_get_object,
    set_object: obj_set_object,
    get_array: obj_get_array,
    set_array: obj_set_array,
    serialize: obj_serialize,
};

unsafe fn arr_push(a: *mut JsonArray, n: NodeRef) -> HRESULT {
    match arr_vec(a) {
        Ok(v) => {
            v.push(n);
            S_OK
        }
        Err(e) => e,
    }
}

unsafe fn arr_at(a: *mut JsonArray, i: u32) -> Result<NodeRef, HRESULT> {
    let v = arr_vec(a)?;
    v.get(i as usize).cloned().ok_or(E_JSON_NOT_FOUND)
}

unsafe extern "system" fn arr_add_number(a: *mut JsonArray, v: f64) -> HRESULT {
    if let Err(e) = arr_vec(a) {
        return e;
    }
    arr_push(a, Node::new(Val::Num(v)))
}

unsafe extern "system" fn arr_add_string(a: *mut JsonArray, s: *const u16) -> HRESULT {
    if let Err(e) = arr_vec(a) {
        return e;
    }
    match wide_to_utf8(s) {
        Ok(s) => arr_push(a, Node::new(Val::Str(s))),
        Err(e) => e,
    }
}

unsafe extern "system" fn arr_add_object(a: *mut JsonArray, obj: *mut c_void) -> HRESULT {
    let o = cast_impl(obj, &IID_XBTP_JSON_OBJECT_IMPL) as *mut JsonObject;
    if o.is_null() {
        return E_INVALIDARG;
    }
    if let Err(e) = arr_vec(a) {
        return e;
    }
    let n = obj_ensure(o);
    arr_push(a, n)
}

unsafe extern "system" fn arr_add_json(a: *mut JsonArray, json: *const u16) -> HRESULT {
    if let Err(e) = arr_vec(a) {
        return e;
    }
    let text = match wide_to_utf8(json) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match parse(&text) {
        Ok(n) => arr_push(a, n),
        Err(e) => e,
    }
}

unsafe extern "system" fn arr_get_number(a: *mut JsonArray, i: u32, out: *mut f64) -> HRESULT {
    match arr_at(a, i) {
        Ok(n) => match n.val() {
            Val::Num(v) => {
                *out = *v;
                S_OK
            }
            _ => E_DATATYPE_MISMATCH,
        },
        Err(e) => e,
    }
}

unsafe extern "system" fn arr_get_string(a: *mut JsonArray, i: u32, out: *mut BSTR) -> HRESULT {
    match arr_at(a, i) {
        Ok(n) => match n.val() {
            Val::Str(s) => utf8_to_bstr(s, out),
            _ => E_DATATYPE_MISMATCH,
        },
        Err(e) => e,
    }
}

unsafe extern "system" fn arr_get_object(a: *mut JsonArray, i: u32, out: *mut *mut c_void) -> HRESULT {
    *out = null_mut();
    let n = match arr_at(a, i) {
        Ok(n) => n,
        Err(e) => return e,
    };
    if n.kind() != 5 {
        return E_DATATYPE_MISMATCH;
    }
    hand_out(new_object(Some(n)) as *mut c_void, &IID_XBTP_JSON_OBJECT, out)
}

pub fn value_type_of(kind: u32) -> u32 {
    match kind {
        2 => 3,
        3 => 1,
        4 => 2,
        5 => 5,
        6 => 4,
        _ => 0,
    }
}

unsafe extern "system" fn arr_get_value_type(a: *mut JsonArray, i: u32, out: *mut *mut c_void) -> HRESULT {
    *out = null_mut();
    let n = match arr_at(a, i) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let t = Box::into_raw(Box::new(JsonValueType { vtbl: &TYPE_VTBL, kind: value_type_of(n.kind()), refs: AtomicU32::new(1) }));
    hand_out(t as *mut c_void, &IID_XBTP_JSON_VALUE_TYPE, out)
}

unsafe extern "system" fn arr_get_count(a: *mut JsonArray, out: *mut u32) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    match arr_vec(a) {
        Ok(v) => {
            *out = v.len() as u32;
            S_OK
        }
        Err(e) => e,
    }
}

pub static ARR_VTBL: ArrVtbl = ArrVtbl {
    query_interface: arr_qi,
    add_ref: arr_add_ref,
    release: arr_release,
    add_number: arr_add_number,
    add_string: arr_add_string,
    add_object: arr_add_object,
    add_json: arr_add_json,
    get_number: arr_get_number,
    get_string: arr_get_string,
    get_object: arr_get_object,
    get_value_type: arr_get_value_type,
    get_count: arr_get_count,
};

pub unsafe fn object_initialize(o: *mut JsonObject, json: *const u16) -> HRESULT {
    let text = match wide_to_utf8(json) {
        Ok(t) => t,
        Err(e) => return e,
    };
    (*o).node = None;
    match parse(&text) {
        Ok(n) => {
            let k = n.kind();
            (*o).node = Some(n);
            if k == 5 {
                S_OK
            } else {
                E_DATATYPE_MISMATCH
            }
        }
        Err(e) => e,
    }
}

#[no_mangle]
pub unsafe extern "C" fn XbtpCreateJsonArray(out: *mut *mut c_void) -> HRESULT {
    hand_out(new_array(None) as *mut c_void, &IID_XBTP_JSON_ARRAY, out)
}

#[no_mangle]
pub unsafe extern "C" fn XbtpCreateJsonObject(out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    *out = null_mut();
    hand_out(new_object(None) as *mut c_void, &IID_XBTP_JSON_OBJECT, out)
}

#[no_mangle]
pub unsafe extern "C" fn XbtpGetJsonObject(json: *const u16, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    *out = null_mut();
    let o = new_object(None);
    let hr = object_initialize(o, json);
    if hr < 0 {
        obj_release(o as *mut c_void);
        return hr;
    }
    hand_out(o as *mut c_void, &IID_XBTP_JSON_OBJECT, out)
}

pub unsafe fn utf8_to_wide(s: *const u8) -> Result<Vec<u16>, HRESULT> {
    let n = MultiByteToWideChar(CP_UTF8, 0, s, -1, null_mut(), 0);
    if n <= 0 {
        let e = GetLastError();
        if e != 0 {
            return Err(hr_from_win32(e));
        }
    }
    let n = n.max(0);
    let mut w = vec![0u16; n as usize];
    if MultiByteToWideChar(CP_UTF8, 0, s, -1, w.as_mut_ptr(), n) != n {
        return Err(E_UNEXPECTED_JSON_TRANSLATION);
    }
    if w.is_empty() {
        w.push(0);
    }
    Ok(w)
}

#[no_mangle]
pub unsafe extern "C" fn XbtpGetJsonObjectFromUtf8(utf8: *const u8, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    *out = null_mut();
    if utf8.is_null() {
        return E_INVALIDARG;
    }
    match utf8_to_wide(utf8) {
        Ok(w) => XbtpGetJsonObject(w.as_ptr(), out),
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn ov(p: *mut c_void) -> &'static ObjVtbl {
        &**(p as *const *const ObjVtbl)
    }

    unsafe fn av(p: *mut c_void) -> &'static ArrVtbl {
        &**(p as *const *const ArrVtbl)
    }

    unsafe fn text(o: *mut c_void) -> String {
        let mut b: BSTR = null_mut();
        assert_eq!((ov(o).serialize)(o as *mut JsonObject, &mut b), S_OK);
        let s = from_wide(b);
        SysFreeString(b);
        s
    }

    #[test]
    fn create_json_object_roundtrip() {
        unsafe {
            let mut o: *mut c_void = null_mut();
            assert_eq!(XbtpCreateJsonObject(&mut o), S_OK);
            let vt = ov(o);
            let k = wide("Signature");
            assert_eq!((vt.set_number)(o as _, k.as_ptr(), 7.5), S_OK);
            let mut d = 0f64;
            assert_eq!((vt.get_number)(o as _, k.as_ptr(), &mut d), S_OK);
            assert_eq!(d, 7.5);
            let m = wide("Missing");
            assert_eq!((vt.get_number)(o as _, m.as_ptr(), &mut d), E_JSON_NOT_FOUND);
            let mut b: BSTR = null_mut();
            assert_eq!((vt.get_string)(o as _, k.as_ptr(), &mut b), E_DATATYPE_MISMATCH);
            let sk = wide("Name");
            let sv = wide("a\"b/\n");
            assert_eq!((vt.set_string)(o as _, sk.as_ptr(), sv.as_ptr()), S_OK);
            assert_eq!((vt.get_string)(o as _, sk.as_ptr(), &mut b), S_OK);
            assert_eq!(from_wide(b), "a\"b/\n");
            SysFreeString(b);
            let t = text(o);
            assert!(t.contains("\"Signature\":7.5"));
            assert!(t.contains("\"Name\":\"a\\\"b\\/\\n\""));
            assert_eq!(XbtpCreateJsonObject(null_mut()), E_POINTER);
            assert_eq!((vt.release)(o), 0);
        }
    }

    #[test]
    fn create_json_array_items() {
        unsafe {
            let mut a: *mut c_void = null_mut();
            assert_eq!(XbtpCreateJsonArray(&mut a), S_OK);
            let vt = av(a);
            assert_eq!((vt.add_number)(a as _, 1.0), S_OK);
            let s = wide("x");
            assert_eq!((vt.add_string)(a as _, s.as_ptr()), S_OK);
            let j = wide("{\"k\":true}");
            assert_eq!((vt.add_json)(a as _, j.as_ptr()), S_OK);
            let mut c = 0u32;
            assert_eq!((vt.get_count)(a as _, &mut c), S_OK);
            assert_eq!(c, 3);
            let mut d = 0f64;
            assert_eq!((vt.get_number)(a as _, 0, &mut d), S_OK);
            assert_eq!((vt.get_number)(a as _, 3, &mut d), E_JSON_NOT_FOUND);
            assert_eq!((vt.get_number)(a as _, 1, &mut d), E_DATATYPE_MISMATCH);
            let mut t: *mut c_void = null_mut();
            assert_eq!((vt.get_value_type)(a as _, 1, &mut t), S_OK);
            let tv = &**(t as *const *const TypeVtbl);
            let mut k = 0u32;
            assert_eq!((tv.get_type)(t as _, &mut k), S_OK);
            assert_eq!(k, 3);
            (tv.release)(t);
            let mut o: *mut c_void = null_mut();
            assert_eq!((vt.get_object)(a as _, 2, &mut o), S_OK);
            let mut q: *mut c_void = null_mut();
            assert_eq!((ov(o).query_interface)(o, &IID_XBTP_JSON_OBJECT, &mut q), S_OK);
            (ov(o).release)(q);
            assert_eq!(text(o), "{\"k\":true}");
            (ov(o).release)(o);
            let mut root: *mut c_void = null_mut();
            XbtpCreateJsonObject(&mut root);
            let name = wide("arr");
            assert_eq!((ov(root).set_array)(root as _, name.as_ptr(), a), S_OK);
            assert_eq!(text(root), "{\"arr\":[1,\"x\",{\"k\":true}]}");
            (ov(root).release)(root);
            (vt.release)(a);
        }
    }

    #[test]
    fn get_json_object_parses() {
        unsafe {
            let j = wide(" {\"a\" : [1, 2.5e1, \"s\\u0041\"], \"b\": {\"c\": null}, \"d\": false} trailing");
            let mut o: *mut c_void = null_mut();
            assert_eq!(XbtpGetJsonObject(j.as_ptr(), &mut o), S_OK);
            let vt = ov(o);
            let mut arr: *mut c_void = null_mut();
            let a = wide("a");
            assert_eq!((vt.get_array)(o as _, a.as_ptr(), &mut arr), S_OK);
            let mut d = 0f64;
            assert_eq!((av(arr).get_number)(arr as _, 1, &mut d), S_OK);
            assert_eq!(d, 25.0);
            let mut b: BSTR = null_mut();
            assert_eq!((av(arr).get_string)(arr as _, 2, &mut b), S_OK);
            assert_eq!(from_wide(b), "sA");
            SysFreeString(b);
            (av(arr).release)(arr);
            let mut sub: *mut c_void = null_mut();
            let bk = wide("b");
            assert_eq!((vt.get_object_alias)(o as _, bk.as_ptr(), &mut sub), S_OK);
            assert_eq!(text(sub), "{\"c\":null}");
            (ov(sub).release)(sub);
            (vt.release)(o);
            let bad = wide("[1,2]");
            assert_eq!(XbtpGetJsonObject(bad.as_ptr(), &mut o), E_DATATYPE_MISMATCH);
            assert!(o.is_null());
            let bad = wide("{\"a\":}");
            assert_eq!(XbtpGetJsonObject(bad.as_ptr(), &mut o), E_FAIL);
            let bad = wide("{\"a\":-x}");
            assert_eq!(XbtpGetJsonObject(bad.as_ptr(), &mut o), E_JSON_BAD_NUMBER);
            assert_eq!(XbtpGetJsonObject(core::ptr::null(), &mut o), E_FAIL);
        }
    }

    #[test]
    fn get_json_object_from_utf8() {
        unsafe {
            let mut o: *mut c_void = null_mut();
            assert_eq!(XbtpGetJsonObjectFromUtf8(core::ptr::null(), &mut o), E_INVALIDARG);
            assert_eq!(XbtpGetJsonObjectFromUtf8("{\"\u{e9}\":\"\u{20ac}\"}\0".as_ptr(), &mut o), S_OK);
            let k = wide("\u{e9}");
            let mut b: BSTR = null_mut();
            assert_eq!((ov(o).get_string)(o as _, k.as_ptr(), &mut b), S_OK);
            assert_eq!(from_wide(b), "\u{20ac}");
            SysFreeString(b);
            (ov(o).release)(o);
        }
    }

    #[test]
    fn object_key_order_follows_hash_buckets() {
        let mut m = Map::default();
        for k in ["a", "b", "c", "d", "e", "f", "g", "h", "i"] {
            let w = wide(k);
            let h = unsafe { hash_wide(w.as_ptr()) };
            assert_eq!(h, hash_utf8(k.as_bytes()));
            m.insert(k.as_bytes().to_vec(), h, Node::new(Val::Null));
        }
        let mut out = Vec::new();
        serialize(&Node::new(Val::Obj(m)), &mut out);
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.matches(":null").count(), 9);
        let mut m2 = Map::default();
        m2.insert(b"x".to_vec(), hash_utf8(b"x"), Node::new(Val::Bool(1)));
        m2.insert(b"x".to_vec(), hash_utf8(b"x"), Node::new(Val::Bool(0)));
        let mut out = Vec::new();
        serialize(&Node::new(Val::Obj(m2)), &mut out);
        assert_eq!(out, b"{\"x\":false}");
    }

    #[test]
    fn number_format_matches_crt_g15() {
        assert_eq!(format_number(1.0).unwrap(), b"1");
        assert_eq!(format_number(0.1).unwrap(), b"0.1");
        assert_eq!(format_number(1e20).unwrap(), b"1e+20");
        assert_eq!(format_number(4294967295.0).unwrap(), b"4294967295");
    }

    #[test]
    fn surrogate_escape_uses_console_formula() {
        let n = parse(b"\"\\ud83d\\ude00\"").unwrap();
        match n.val() {
            Val::Str(s) => {
                let cp = (((0xd83du32 & 0x3ff) | 0x400) << 10) | (0xde00 & 0x3ff);
                assert_eq!(s[0], 0xf0 | ((cp >> 18) & 7) as u8);
                assert_eq!(s[1], 0x80 | ((cp >> 6) & 0x3f) as u8);
                assert_eq!(s[2], 0x80 | (cp & 0x3f) as u8);
            }
            _ => panic!(),
        }
    }
}
