use crate::sys::{cstring, heap_bytes};
use core::ffi::c_void;

pub const CURLUE_OK: i32 = 0;
pub const CURLUE_BAD_HANDLE: i32 = 1;
pub const CURLUE_BAD_PARTPOINTER: i32 = 2;
pub const CURLUE_MALFORMED_INPUT: i32 = 3;
pub const CURLUE_BAD_PORT_NUMBER: i32 = 4;
pub const CURLUE_UNSUPPORTED_SCHEME: i32 = 5;
pub const CURLUE_URLDECODE: i32 = 6;
pub const CURLUE_UNKNOWN_PART: i32 = 9;
pub const CURLUE_NO_SCHEME: i32 = 10;
pub const CURLUE_NO_USER: i32 = 11;
pub const CURLUE_NO_PASSWORD: i32 = 12;
pub const CURLUE_NO_OPTIONS: i32 = 13;
pub const CURLUE_NO_HOST: i32 = 14;
pub const CURLUE_NO_PORT: i32 = 15;
pub const CURLUE_NO_QUERY: i32 = 16;
pub const CURLUE_NO_FRAGMENT: i32 = 17;

const CURLU_DEFAULT_PORT: u32 = 1;
const CURLU_NO_DEFAULT_PORT: u32 = 2;
const CURLU_DEFAULT_SCHEME: u32 = 4;
const CURLU_NON_SUPPORT_SCHEME: u32 = 8;
const CURLU_URLDECODE: u32 = 64;
const CURLU_URLENCODE: u32 = 128;
const CURLU_APPENDQUERY: u32 = 256;
const CURLU_GUESS_SCHEME: u32 = 512;

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Url {
    pub scheme: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub options: Option<String>,
    pub host: Option<String>,
    pub zoneid: Option<String>,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

pub fn default_port(scheme: &str) -> Option<u16> {
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        _ => None,
    }
}

fn valid_scheme(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b[0].is_ascii_alphabetic() && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
}

fn split_scheme(s: &str) -> Option<(&str, &str)> {
    let i = s.find("://")?;
    let sch = &s[..i];
    if valid_scheme(sch) {
        Some((sch, &s[i + 3..]))
    } else {
        None
    }
}

pub fn has_scheme(s: &str) -> bool {
    split_scheme(s.trim()).is_some()
}

impl Url {
    pub fn parse(input: &str, guess: bool) -> Result<Url, i32> {
        let s = input.trim();
        if s.is_empty() || s.bytes().any(|c| c < 0x20 && c != b'\t') {
            return Err(CURLUE_MALFORMED_INPUT);
        }
        let (scheme, rest) = match split_scheme(s) {
            Some((sc, r)) => (sc.to_ascii_lowercase(), r),
            None => {
                if !guess {
                    return Err(CURLUE_MALFORMED_INPUT);
                }
                let r = s.strip_prefix("//").unwrap_or(s);
                let h = r.split(|c| c == '/' || c == ':' || c == '?' || c == '#').next().unwrap_or("").to_ascii_lowercase();
                (if h.starts_with("ftp.") { "ftp".to_string() } else { "http".to_string() }, r)
            }
        };
        let end = rest.find(|c| c == '/' || c == '?' || c == '#').unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(end);
        let mut u = Url { scheme: Some(scheme), ..Default::default() };
        let hostport = match authority.rfind('@') {
            Some(i) => {
                let info = &authority[..i];
                let (up, opts) = match info.find(';') {
                    Some(j) => (&info[..j], Some(info[j + 1..].to_string())),
                    None => (info, None),
                };
                match up.find(':') {
                    Some(j) => {
                        u.user = Some(up[..j].to_string());
                        u.password = Some(up[j + 1..].to_string());
                    }
                    None => u.user = Some(up.to_string()),
                }
                u.options = opts;
                &authority[i + 1..]
            }
            None => authority,
        };
        let (host, port) = if let Some(r) = hostport.strip_prefix('[') {
            let close = r.find(']').ok_or(CURLUE_MALFORMED_INPUT)?;
            let inner = &r[..close];
            let after = &r[close + 1..];
            let (h, z) = match inner.find("%25").map(|i| (i, 3)).or_else(|| inner.find('%').map(|i| (i, 1))) {
                Some((i, l)) => (&inner[..i], Some(inner[i + l..].to_string())),
                None => (inner, None),
            };
            u.zoneid = z;
            let p = match after.strip_prefix(':') {
                Some(p) => Some(p),
                None if after.is_empty() => None,
                None => return Err(CURLUE_MALFORMED_INPUT),
            };
            (format!("[{h}]"), p)
        } else {
            match hostport.rfind(':') {
                Some(i) => (hostport[..i].to_string(), Some(&hostport[i + 1..])),
                None => (hostport.to_string(), None),
            }
        };
        if let Some(p) = port {
            if !p.is_empty() {
                if !p.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(CURLUE_BAD_PORT_NUMBER);
                }
                match p.parse::<u32>() {
                    Ok(n) if n > 0 && n <= 65535 => u.port = Some(n as u16),
                    _ => return Err(CURLUE_BAD_PORT_NUMBER),
                }
            }
        }
        if host.is_empty() {
            return Err(CURLUE_NO_HOST);
        }
        if host.bytes().any(|c| c == b' ' || c == b'<' || c == b'>' || c == b'\\' || c == b'"') {
            return Err(CURLUE_MALFORMED_INPUT);
        }
        u.host = Some(host);
        u.set_tail(tail);
        Ok(u)
    }

    fn set_tail(&mut self, tail: &str) {
        let (before_frag, frag) = match tail.find('#') {
            Some(i) => (&tail[..i], Some(tail[i + 1..].to_string())),
            None => (tail, None),
        };
        let (path, query) = match before_frag.find('?') {
            Some(i) => (&before_frag[..i], Some(before_frag[i + 1..].to_string())),
            None => (before_frag, None),
        };
        self.path = path.to_string();
        self.query = query;
        self.fragment = frag;
    }

    pub fn path_or_root(&self) -> String {
        if self.path.is_empty() {
            "/".into()
        } else {
            self.path.clone()
        }
    }

    pub fn effective_port(&self) -> u16 {
        self.port.or_else(|| self.scheme.as_deref().and_then(default_port)).unwrap_or(80)
    }

    pub fn host_bare(&self) -> String {
        let h = self.host.clone().unwrap_or_default();
        h.strip_prefix('[').and_then(|x| x.strip_suffix(']')).map(|x| x.to_string()).unwrap_or(h)
    }

    pub fn compose(&self, flags: u32) -> String {
        let scheme = self.scheme.clone().unwrap_or_else(|| "http".into());
        let mut s = format!("{scheme}://");
        if let Some(u) = &self.user {
            s.push_str(u);
            if let Some(p) = &self.password {
                s.push(':');
                s.push_str(p);
            }
            if let Some(o) = &self.options {
                s.push(';');
                s.push_str(o);
            }
            s.push('@');
        }
        let host = self.host.clone().unwrap_or_default();
        match (&self.zoneid, host.strip_suffix(']')) {
            (Some(z), Some(h)) => s.push_str(&format!("{h}%25{z}]")),
            _ => s.push_str(&host),
        }
        let def = default_port(&scheme);
        match self.port {
            Some(p) if flags & CURLU_NO_DEFAULT_PORT != 0 && Some(p) == def => {}
            Some(p) => s.push_str(&format!(":{p}")),
            None if flags & CURLU_DEFAULT_PORT != 0 => {
                if let Some(d) = def {
                    s.push_str(&format!(":{d}"));
                }
            }
            None => {}
        }
        s.push_str(&self.path_or_root());
        if let Some(q) = &self.query {
            s.push('?');
            s.push_str(q);
        }
        if let Some(f) = &self.fragment {
            s.push('#');
            s.push_str(f);
        }
        s
    }

    pub fn join(&self, rel: &str) -> Result<Url, i32> {
        let rel = rel.trim();
        if has_scheme(rel) {
            return Url::parse(rel, false);
        }
        if let Some(r) = rel.strip_prefix("//") {
            return Url::parse(&format!("{}://{r}", self.scheme.as_deref().unwrap_or("http")), false);
        }
        let mut u = self.clone();
        if rel.is_empty() {
            u.fragment = None;
            return Ok(u);
        }
        if let Some(f) = rel.strip_prefix('#') {
            u.fragment = Some(f.to_string());
            return Ok(u);
        }
        if rel.starts_with('?') {
            let p = u.path.clone();
            u.set_tail(rel);
            u.path = p;
            return Ok(u);
        }
        let joined = if rel.starts_with('/') {
            rel.to_string()
        } else {
            let base = self.path_or_root();
            let dir = &base[..base.rfind('/').map(|i| i + 1).unwrap_or(0)];
            format!("{dir}{rel}")
        };
        u.set_tail(&joined);
        u.path = remove_dot_segments(&u.path);
        Ok(u)
    }
}

pub fn remove_dot_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let segs: Vec<&str> = path.split('/').collect();
    let n = segs.len();
    for (i, seg) in segs.iter().enumerate() {
        match *seg {
            "." => {
                if i == n - 1 {
                    out.push("");
                }
            }
            ".." => {
                if out.len() > 1 {
                    out.pop();
                }
                if i == n - 1 {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let r = out.join("/");
    if path.starts_with('/') && !r.starts_with('/') {
        format!("/{r}")
    } else {
        r
    }
}

pub fn escape(input: &[u8], keep: &[u8]) -> String {
    let mut s = String::with_capacity(input.len());
    for &c in input {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_' | b'~') || keep.contains(&c) {
            s.push(c as char);
        } else {
            s.push_str(&format!("%{c:02X}"));
        }
    }
    s
}

pub fn unescape(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < input.len() {
        if input[i] == b'%' && i + 2 < input.len() {
            if let (Some(a), Some(b)) = (hex(input[i + 1]), hex(input[i + 2])) {
                out.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

pub fn encode_request_target(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.bytes() {
        if c <= 0x20 || c >= 0x7f || c == b'"' || c == b'<' || c == b'>' || c == b'\\' || c == b'^' || c == b'`' || c == b'{' || c == b'|' || c == b'}' {
            o.push_str(&format!("%{c:02X}"));
        } else {
            o.push(c as char);
        }
    }
    o
}

const PART_URL: u32 = 0;
const PART_SCHEME: u32 = 1;
const PART_USER: u32 = 2;
const PART_PASSWORD: u32 = 3;
const PART_OPTIONS: u32 = 4;
const PART_HOST: u32 = 5;
const PART_PORT: u32 = 6;
const PART_PATH: u32 = 7;
const PART_QUERY: u32 = 8;
const PART_FRAGMENT: u32 = 9;
const PART_ZONEID: u32 = 10;

#[derive(Clone, Default)]
pub struct CurlU {
    u: Url,
}

#[no_mangle]
pub extern "C" fn curl_url() -> *mut c_void {
    Box::into_raw(Box::new(CurlU::default())) as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn curl_url_cleanup(h: *mut c_void) {
    if !h.is_null() {
        drop(Box::from_raw(h as *mut CurlU));
    }
}

#[no_mangle]
pub unsafe extern "C" fn curl_url_dup(h: *const c_void) -> *mut c_void {
    if h.is_null() {
        return core::ptr::null_mut();
    }
    Box::into_raw(Box::new((*(h as *const CurlU)).clone())) as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn curl_url_set(h: *mut c_void, part: u32, content: *const u8, flags: u32) -> i32 {
    if h.is_null() {
        return CURLUE_BAD_HANDLE;
    }
    let cu = &mut *(h as *mut CurlU);
    let u = &mut cu.u;
    let Some(raw) = cstring(content) else {
        match part {
            PART_URL => *u = Url::default(),
            PART_SCHEME => u.scheme = None,
            PART_USER => u.user = None,
            PART_PASSWORD => u.password = None,
            PART_OPTIONS => u.options = None,
            PART_HOST => u.host = None,
            PART_PORT => u.port = None,
            PART_PATH => u.path.clear(),
            PART_QUERY => u.query = None,
            PART_FRAGMENT => u.fragment = None,
            PART_ZONEID => u.zoneid = None,
            _ => return CURLUE_UNKNOWN_PART,
        }
        return CURLUE_OK;
    };
    let enc = |s: &str, keep: &[u8]| if flags & CURLU_URLENCODE != 0 { escape(s.as_bytes(), keep) } else { s.to_string() };
    match part {
        PART_URL => {
            let r = if u.host.is_some() && u.scheme.is_some() && !has_scheme(&raw) {
                u.join(&raw)
            } else {
                Url::parse(&raw, flags & (CURLU_DEFAULT_SCHEME | CURLU_GUESS_SCHEME) != 0)
            };
            match r {
                Ok(n) => {
                    if flags & CURLU_NON_SUPPORT_SCHEME == 0 && n.scheme.as_deref().and_then(default_port).is_none() {
                        return CURLUE_UNSUPPORTED_SCHEME;
                    }
                    *u = n
                }
                Err(e) => return e,
            }
        }
        PART_SCHEME => {
            if !valid_scheme(&raw) {
                return CURLUE_MALFORMED_INPUT;
            }
            if flags & CURLU_NON_SUPPORT_SCHEME == 0 && default_port(&raw).is_none() {
                return CURLUE_UNSUPPORTED_SCHEME;
            }
            u.scheme = Some(raw.to_ascii_lowercase());
        }
        PART_USER => u.user = Some(enc(&raw, b"")),
        PART_PASSWORD => u.password = Some(enc(&raw, b"")),
        PART_OPTIONS => u.options = Some(enc(&raw, b"")),
        PART_HOST => {
            if raw.is_empty() || raw.bytes().any(|c| c <= 0x20 || c == b'/' || c == b'?' || c == b'#' || c == b'@') {
                return CURLUE_MALFORMED_INPUT;
            }
            u.host = Some(raw)
        }
        PART_PORT => match raw.parse::<u32>() {
            Ok(n) if n > 0 && n <= 65535 && raw.bytes().all(|c| c.is_ascii_digit()) => u.port = Some(n as u16),
            _ => return CURLUE_BAD_PORT_NUMBER,
        },
        PART_PATH => {
            let p = enc(&raw, b"/");
            u.path = if p.starts_with('/') { p } else { format!("/{p}") };
        }
        PART_QUERY => {
            let q = enc(&raw, b"=&");
            if flags & CURLU_APPENDQUERY != 0 {
                u.query = Some(match u.query.take() {
                    Some(old) if !old.is_empty() => format!("{old}&{q}"),
                    _ => q,
                });
            } else {
                u.query = Some(q);
            }
        }
        PART_FRAGMENT => u.fragment = Some(enc(&raw, b"")),
        PART_ZONEID => u.zoneid = Some(raw),
        _ => return CURLUE_UNKNOWN_PART,
    }
    CURLUE_OK
}

#[no_mangle]
pub unsafe extern "C" fn curl_url_get(h: *const c_void, part: u32, out: *mut *mut u8, flags: u32) -> i32 {
    if h.is_null() {
        return CURLUE_BAD_HANDLE;
    }
    if out.is_null() {
        return CURLUE_BAD_PARTPOINTER;
    }
    *out = core::ptr::null_mut();
    let u = &(*(h as *const CurlU)).u;
    let r: Result<String, i32> = match part {
        PART_URL => {
            if u.scheme.is_none() {
                Err(CURLUE_NO_SCHEME)
            } else if u.host.is_none() {
                Err(CURLUE_NO_HOST)
            } else {
                Ok(u.compose(flags))
            }
        }
        PART_SCHEME => u.scheme.clone().ok_or(CURLUE_NO_SCHEME),
        PART_USER => u.user.clone().ok_or(CURLUE_NO_USER),
        PART_PASSWORD => u.password.clone().ok_or(CURLUE_NO_PASSWORD),
        PART_OPTIONS => u.options.clone().ok_or(CURLUE_NO_OPTIONS),
        PART_HOST => u.host.clone().ok_or(CURLUE_NO_HOST),
        PART_ZONEID => u.zoneid.clone().ok_or(CURLUE_NO_HOST),
        PART_PORT => {
            let def = u.scheme.as_deref().and_then(default_port);
            match u.port {
                Some(p) if flags & CURLU_NO_DEFAULT_PORT != 0 && Some(p) == def => Err(CURLUE_NO_PORT),
                Some(p) => Ok(p.to_string()),
                None if flags & CURLU_DEFAULT_PORT != 0 => def.map(|d| d.to_string()).ok_or(CURLUE_NO_PORT),
                None => Err(CURLUE_NO_PORT),
            }
        }
        PART_PATH => Ok(u.path_or_root()),
        PART_QUERY => u.query.clone().ok_or(CURLUE_NO_QUERY),
        PART_FRAGMENT => u.fragment.clone().ok_or(CURLUE_NO_FRAGMENT),
        _ => Err(CURLUE_UNKNOWN_PART),
    };
    match r {
        Ok(s) => {
            let bytes = if flags & CURLU_URLDECODE != 0 && part != PART_URL {
                let d = unescape(s.as_bytes());
                if d.contains(&0) {
                    return CURLUE_URLDECODE;
                }
                d
            } else {
                s.into_bytes()
            };
            *out = heap_bytes(&bytes);
            CURLUE_OK
        }
        Err(e) => e,
    }
}
