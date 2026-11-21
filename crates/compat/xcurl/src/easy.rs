use crate::net::*;
use crate::sys::*;
use crate::url::Url;
use core::ffi::c_void;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub type DataCb = unsafe extern "C" fn(*mut u8, usize, usize, *mut c_void) -> usize;

const CURL_READFUNC_ABORT: usize = 0x1000_0000;
const CURL_READFUNC_PAUSE: usize = 0x1000_0001;
const CURL_WRITEFUNC_PAUSE: usize = 0x1000_0001;
const CURL_PROGRESSFUNC_CONTINUE: i32 = 0x1000_0001;
const CURL_ERROR_SIZE: usize = 256;
const CURL_MAX_WRITE_SIZE: usize = 16384;
const CURLE_FILESIZE_EXCEEDED: i32 = 63;
const CURLE_COULDNT_RESOLVE_PROXY: i32 = 5;
const PAUSE_RETRY: Duration = Duration::from_millis(10);
const CURLINFO_TEXT: i32 = 0;
const CURLINFO_HEADER_IN: i32 = 1;
const CURLINFO_HEADER_OUT: i32 = 2;
const CURLINFO_DATA_IN: i32 = 3;
const CURLINFO_DATA_OUT: i32 = 4;

type DebugCb = unsafe extern "C" fn(*mut c_void, i32, *const u8, usize, *mut c_void) -> i32;
type XferInfoCb = unsafe extern "C" fn(*mut c_void, i64, i64, i64, i64) -> i32;
type ProgressCb = unsafe extern "C" fn(*mut c_void, f64, f64, f64, f64) -> i32;

#[repr(C)]
struct CurlBlob {
    data: *const u8,
    len: usize,
    flags: u32,
}

enum Held {
    Ev(Ev),
    Lines(Vec<u8>),
    Bytes(Vec<u8>),
}

enum Delivery {
    Done,
    Paused,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HttpReq {
    Get,
    Post,
    Put,
    Head,
}

#[derive(Clone)]
pub struct Opts {
    pub url: Option<String>,
    pub custom_request: Option<String>,
    pub httpreq: HttpReq,
    pub postfields: usize,
    pub copypostfields: Option<Vec<u8>>,
    pub postfieldsize: i64,
    pub infilesize: i64,
    pub httpheader: usize,
    pub connect_to: usize,
    pub write_fn: usize,
    pub write_data: usize,
    pub header_fn: usize,
    pub header_data: usize,
    pub read_fn: usize,
    pub read_data: usize,
    pub header_in_body: bool,
    pub follow: bool,
    pub maxredirs: i64,
    pub postredir: u32,
    pub timeout_ms: u64,
    pub connect_timeout_ms: u64,
    pub verify_peer: bool,
    pub verify_host: bool,
    pub useragent: Option<String>,
    pub referer: Option<String>,
    pub cookie: Option<String>,
    pub range: Option<String>,
    pub accept_encoding: Option<String>,
    pub userpwd: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub private: usize,
    pub errorbuffer: usize,
    pub fail_on_error: bool,
    pub port: Option<u16>,
    pub verbose: bool,
    pub ssl_cert: Option<String>,
    pub ssl_cert_blob: Option<Vec<u8>>,
    pub ssl_cert_type: Option<String>,
    pub ssl_key: Option<String>,
    pub ssl_key_blob: Option<Vec<u8>>,
    pub ssl_key_type: Option<String>,
    pub key_passwd: Option<String>,
    pub ssl_version: i64,
    pub cipher_list: Option<String>,
    pub tls13_ciphers: Option<String>,
    pub cainfo: Option<String>,
    pub cainfo_blob: Option<Vec<u8>>,
    pub ssl_options: i64,
    pub verify_status: bool,
    pub http_version: i64,
    pub forbid_reuse: bool,
    pub fresh_connect: bool,
    pub buffersize: usize,
    pub maxfilesize: i64,
    pub low_speed_limit: i64,
    pub low_speed_time: i64,
    pub debug_fn: usize,
    pub debug_data: usize,
    pub noprogress: bool,
    pub progress_fn: usize,
    pub xferinfo_fn: usize,
    pub progress_data: usize,
    pub tcp_keepalive: bool,
    pub tcp_keepidle: i64,
    pub tcp_keepintvl: i64,
    pub ipresolve: i64,
    pub dns_cache_timeout: i64,
    pub proxy: Option<String>,
    pub proxy_port: i64,
    pub proxy_type: i64,
    pub proxy_userpwd: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub noproxy: Option<String>,
    pub proxy_headers: usize,
    pub proxy_tunnel: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            url: None,
            custom_request: None,
            httpreq: HttpReq::Get,
            postfields: 0,
            copypostfields: None,
            postfieldsize: -1,
            infilesize: -1,
            httpheader: 0,
            connect_to: 0,
            write_fn: 0,
            write_data: 0,
            header_fn: 0,
            header_data: 0,
            read_fn: 0,
            read_data: 0,
            header_in_body: false,
            follow: false,
            maxredirs: 30,
            postredir: 0,
            timeout_ms: 0,
            connect_timeout_ms: 0,
            verify_peer: true,
            verify_host: true,
            useragent: None,
            referer: None,
            cookie: None,
            range: None,
            accept_encoding: None,
            userpwd: None,
            username: None,
            password: None,
            private: 0,
            errorbuffer: 0,
            fail_on_error: false,
            port: None,
            verbose: false,
            ssl_cert: None,
            ssl_cert_blob: None,
            ssl_cert_type: None,
            ssl_key: None,
            ssl_key_blob: None,
            ssl_key_type: None,
            key_passwd: None,
            ssl_version: 0,
            cipher_list: None,
            tls13_ciphers: None,
            cainfo: None,
            cainfo_blob: None,
            ssl_options: 0,
            verify_status: false,
            http_version: 0,
            forbid_reuse: false,
            fresh_connect: false,
            buffersize: CURL_MAX_WRITE_SIZE,
            maxfilesize: 0,
            low_speed_limit: 0,
            low_speed_time: 0,
            debug_fn: 0,
            debug_data: 0,
            noprogress: true,
            progress_fn: 0,
            xferinfo_fn: 0,
            progress_data: 0,
            tcp_keepalive: false,
            tcp_keepidle: 60,
            tcp_keepintvl: 60,
            ipresolve: 0,
            dns_cache_timeout: 60,
            proxy: None,
            proxy_port: 0,
            proxy_type: 0,
            proxy_userpwd: None,
            proxy_username: None,
            proxy_password: None,
            noproxy: None,
            proxy_headers: 0,
            proxy_tunnel: false,
        }
    }
}

pub struct Info {
    pub response_code: i32,
    pub http_version: i32,
    pub effective_url: Vec<u8>,
    pub effective_method: Vec<u8>,
    pub content_type: Option<Vec<u8>>,
    pub redirect_url: Option<Vec<u8>>,
    pub primary_ip: Vec<u8>,
    pub primary_port: i32,
    pub scheme: Vec<u8>,
    pub total_time: f64,
    pub connect_time: f64,
    pub starttransfer_time: f64,
    pub redirect_time: f64,
    pub size_download: f64,
    pub size_upload: f64,
    pub content_length_download: f64,
    pub content_length_upload: f64,
    pub header_size: i32,
    pub request_size: i32,
    pub os_errno: i32,
    pub redirect_count: i32,
    pub num_connects: i32,
    pub retry_after: i64,
}

impl Default for Info {
    fn default() -> Self {
        Info {
            response_code: 0,
            http_version: 0,
            effective_url: vec![0],
            effective_method: b"GET\0".to_vec(),
            content_type: None,
            redirect_url: None,
            primary_ip: vec![0],
            primary_port: 0,
            scheme: vec![0],
            total_time: 0.0,
            connect_time: 0.0,
            starttransfer_time: 0.0,
            redirect_time: 0.0,
            size_download: 0.0,
            size_upload: 0.0,
            content_length_download: -1.0,
            content_length_upload: -1.0,
            header_size: 0,
            request_size: 0,
            os_errno: 0,
            redirect_count: 0,
            num_connects: 0,
            retry_after: 0,
        }
    }
}

fn read_err(c: i32) -> (i32, String) {
    let m = match c {
        CURLE_OPERATION_TIMEDOUT => "Operation timed out while the read callback kept the upload paused",
        CURLE_ABORTED_BY_CALLBACK => "Operation aborted by the read callback",
        CURLE_OUT_OF_MEMORY => "Upload body exceeds 512 MiB",
        _ => "Read callback returned more data than requested",
    };
    (c, m.to_string())
}

fn nul(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

pub struct Xfer {
    pub shared: Arc<Shared>,
    pub start: Instant,
    pub deadline: Option<Instant>,
    pub status: i32,
    pub nobody: bool,
    held: std::collections::VecDeque<Held>,
    retry_at: Option<Instant>,
    speed_mark: (Instant, f64),
    last_progress: Option<Instant>,
    _slot: Option<crate::gate::Slot>,
}

pub struct Easy {
    pub opts: Opts,
    pub info: Info,
    pub multi: usize,
    pub xfer: Option<Xfer>,
    pub result: Option<i32>,
    pub label: String,
    tls_cache: Option<(String, crate::tls::Tls)>,
    pub pool: Option<Arc<Pool>>,
}

pub unsafe fn slist_strings(mut p: usize) -> Vec<String> {
    let mut v = Vec::new();
    let mut guard = 0;
    while p != 0 && guard < 100_000 {
        let node = p as *const crate::misc::Slist;
        if let Some(s) = cstring((*node).data as *const u8) {
            v.push(s);
        }
        p = (*node).next as usize;
        guard += 1;
    }
    v
}

fn b64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in input.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

impl Easy {
    pub fn new() -> Easy {
        Easy { opts: Opts::default(), info: Info::default(), multi: 0, xfer: None, result: None, label: String::new(), tls_cache: None, pool: None }
    }

    pub fn wants_http2(&self) -> bool {
        let https = self.opts.url.as_deref().map_or(false, |u| u.trim_start().to_ascii_lowercase().starts_with("https:"));
        self.opts.http_version >= 3 || (self.opts.http_version == 0 && https)
    }

    pub fn host_key(&self) -> String {
        self.opts
            .url
            .as_deref()
            .and_then(|u| Url::parse(u, true).ok())
            .map(|u| format!("{}:{}", u.host_bare().to_ascii_lowercase(), self.opts.port.unwrap_or(u.effective_port())))
            .unwrap_or_default()
    }

    unsafe fn proxy_plan(&self, url: &Url, scheme: &str) -> Result<Option<ProxyPlan>, (i32, String)> {
        let o = &self.opts;
        let Some(raw) = o.proxy.as_deref().map(str::trim).filter(|p| !p.is_empty()) else { return Ok(None) };
        let host = url.host_bare().to_ascii_lowercase();
        if let Some(np) = o.noproxy.as_deref() {
            let bypass = np.split([',', ' ']).map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|e| !e.is_empty()).any(|e| {
                e == "*" || host == e || host.ends_with(&format!(".{e}"))
            });
            if bypass {
                return Ok(None);
            }
        }
        let (pscheme, rest) = match raw.split_once("://") {
            Some((s, r)) => (s.to_ascii_lowercase(), r),
            None => (
                match o.proxy_type {
                    2 => "https".to_string(),
                    4 => "socks4".to_string(),
                    5 => "socks5".to_string(),
                    6 => "socks4a".to_string(),
                    7 => "socks5h".to_string(),
                    _ => "http".to_string(),
                },
                raw,
            ),
        };
        if pscheme != "http" {
            return Err((4, format!("Unsupported proxy scheme '{pscheme}': XCurl on WinHTTP only speaks HTTP proxies")));
        }
        let rest = rest.split('/').next().unwrap_or("");
        let (creds, hostport) = match rest.rsplit_once('@') {
            Some((c, h)) => (Some(c), h),
            None => (None, rest),
        };
        let (phost, pport) = if let Some(b) = hostport.strip_prefix('[') {
            let (h, tail) = b.split_once(']').unwrap_or((b, ""));
            (h.to_string(), tail.strip_prefix(':').and_then(|p| p.parse::<u16>().ok()))
        } else {
            match hostport.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), p.parse::<u16>().ok()),
                None => (hostport.to_string(), None),
            }
        };
        if phost.is_empty() {
            return Err((CURLE_COULDNT_RESOLVE_PROXY, format!("Could not resolve proxy: {raw}")));
        }
        let port = if o.proxy_port > 0 { o.proxy_port as u16 } else { pport.unwrap_or(1080) };
        let dec = |s: &str| String::from_utf8_lossy(&crate::url::unescape(s.as_bytes())).into_owned();
        let (mut user, mut pass) = (None, None);
        if let Some(c) = creds {
            let (u, p) = c.split_once(':').unwrap_or((c, ""));
            user = Some(dec(u));
            pass = Some(dec(p));
        }
        if let Some(up) = &o.proxy_userpwd {
            let (u, p) = up.split_once(':').unwrap_or((up.as_str(), ""));
            user = Some(dec(u));
            pass = Some(dec(p));
        }
        if o.proxy_username.is_some() {
            user = o.proxy_username.clone();
        }
        if o.proxy_password.is_some() {
            pass = o.proxy_password.clone();
        }
        let auth = user.as_ref().map(|u| format!("Basic {}", b64(format!("{u}:{}", pass.clone().unwrap_or_default()).as_bytes())));
        let headers = slist_strings(o.proxy_headers).into_iter().filter(|h| h.contains(':')).collect();
        Ok(Some(ProxyPlan { host: phost, port, auth, headers, tunnel: o.proxy_tunnel || scheme == "https", user, password: pass }))
    }

    unsafe fn debug(&self, kind: i32, data: &[u8]) {
        if !self.opts.verbose || self.opts.debug_fn == 0 || data.is_empty() {
            return;
        }
        let f: DebugCb = core::mem::transmute(self.opts.debug_fn);
        f(self as *const Easy as *mut c_void, kind, data.as_ptr(), data.len(), self.opts.debug_data as *mut c_void);
    }

    fn tls_key(&self) -> String {
        let o = &self.opts;
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{:?}|{:?}|{:?}|{:?}|{}|{}",
            o.ssl_cert, o.ssl_cert_blob.as_ref().map(|b| b.len()), o.ssl_cert_type, o.ssl_key, o.ssl_key_blob.as_ref().map(|b| b.len()), o.ssl_key_type,
            o.key_passwd.as_ref().map(|p| p.len()), o.ssl_version, o.cipher_list, o.tls13_ciphers, o.cainfo, o.cainfo_blob.as_ref().map(|b| b.len()), o.ssl_options, o.verify_status
        )
    }

    fn tls(&mut self) -> Result<crate::tls::Tls, (i32, String)> {
        use crate::tls::*;
        let key = self.tls_key();
        if let Some((k, t)) = &self.tls_cache {
            if *k == key {
                return Ok(t.clone());
            }
        }
        let o = &self.opts;
        let mut protocols = ssl_version(o.ssl_version).map_err(connect_error)?;
        for list in [o.cipher_list.as_deref(), o.tls13_ciphers.as_deref()].into_iter().flatten() {
            let plan = ciphers(list).map_err(|m| (CURLE_SSL_CIPHER, m))?;
            if plan.tls13_only && o.tls13_ciphers.as_deref() != Some(list) {
                protocols = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3;
            }
            compat_common::record("xcurl", "tls", &format!("cipher list [{}] validated; Schannel orders suites by system policy{}", plan.names.join(":"), if plan.tls13_only { ", TLS 1.3 only" } else { "" }));
        }
        if o.ssl_key.is_some() && o.ssl_cert.is_none() && o.ssl_cert_blob.is_none() {
            return Err((CURLE_SSL_CERTPROBLEM, "CURLOPT_SSLKEY is set without CURLOPT_SSLCERT".into()));
        }
        let client = client_cert(&ClientCertSpec {
            cert: o.ssl_cert.as_deref(),
            cert_blob: o.ssl_cert_blob.as_deref(),
            cert_type: o.ssl_cert_type.as_deref(),
            key: o.ssl_key.as_deref(),
            key_blob: o.ssl_key_blob.as_deref(),
            key_type: o.ssl_key_type.as_deref(),
            pass: o.key_passwd.as_deref(),
        })?;
        if let Some(c) = &client {
            compat_common::record("xcurl", "tls", &format!("client certificate loaded, sha1 {}", c.thumbprint()));
        }
        let ca = ca_store(o.cainfo.as_deref(), o.cainfo_blob.as_deref())?;
        if let Some(c) = &ca {
            compat_common::record("xcurl", "tls", &format!("CA bundle with {} certificates verifies peers", c.len()));
        }
        let no_revoke = o.ssl_options & 2 != 0;
        let t = Tls {
            client: client.map(Arc::new),
            protocols,
            ca: ca.map(Arc::new),
            revocation: o.verify_status && !no_revoke,
            revoke_best_effort: o.ssl_options & 8 != 0,
        };
        self.tls_cache = Some((key, t.clone()));
        Ok(t)
    }

    pub fn set_error(&self, msg: &str) {
        if self.opts.errorbuffer != 0 {
            let b = msg.as_bytes();
            let n = b.len().min(CURL_ERROR_SIZE - 1);
            unsafe {
                core::ptr::copy_nonoverlapping(b.as_ptr(), self.opts.errorbuffer as *mut u8, n);
                *(self.opts.errorbuffer as *mut u8).add(n) = 0;
            }
        }
    }

    unsafe fn read_body(&self, limit: i64) -> Result<Vec<u8>, i32> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut paused_since: Option<Instant> = None;
        loop {
            let want = if limit >= 0 { ((limit as usize).saturating_sub(out.len())).min(buf.len()) } else { buf.len() };
            if want == 0 {
                break;
            }
            let n = if self.opts.read_fn != 0 {
                let f: DataCb = core::mem::transmute(self.opts.read_fn);
                f(buf.as_mut_ptr(), 1, want, self.opts.read_data as *mut c_void)
            } else if self.opts.read_data != 0 {
                crt_fread(buf.as_mut_ptr(), want, self.opts.read_data as *mut c_void)
            } else {
                0
            };
            if n == CURL_READFUNC_ABORT {
                return Err(CURLE_ABORTED_BY_CALLBACK);
            }
            if n == CURL_READFUNC_PAUSE {
                let started = paused_since.get_or_insert_with(Instant::now);
                let limit = if self.opts.timeout_ms > 0 { Duration::from_millis(self.opts.timeout_ms) } else { Duration::from_secs(300) };
                if started.elapsed() >= limit {
                    return Err(CURLE_OPERATION_TIMEDOUT);
                }
                std::thread::sleep(PAUSE_RETRY);
                continue;
            }
            paused_since = None;
            if n == 0 {
                break;
            }
            if n > want {
                return Err(CURLE_READ_ERROR);
            }
            out.extend_from_slice(&buf[..n]);
            if out.len() > 512 * 1024 * 1024 {
                return Err(CURLE_OUT_OF_MEMORY);
            }
        }
        Ok(out)
    }

    unsafe fn build(&mut self) -> Result<Request, (i32, String)> {
        let Some(raw_url) = self.opts.url.clone() else {
            return Err((CURLE_URL_MALFORMAT, "No URL set".into()));
        };
        let mut url = Url::parse(&raw_url, true).map_err(|_| (CURLE_URL_MALFORMAT, format!("URL rejected: Malformed input to a URL function ({raw_url})")))?;
        let scheme = url.scheme.clone().unwrap_or_default();
        if scheme != "http" && scheme != "https" {
            return Err((CURLE_UNSUPPORTED_PROTOCOL, format!("Protocol \"{scheme}\" not supported")));
        }
        let proxy = self.proxy_plan(&url, &scheme)?;
        let tls = if scheme == "https" { self.tls()? } else { crate::tls::Tls::default() };
        let o = &self.opts;
        if let Some(p) = o.port {
            url.port = Some(p);
        }
        let base = match o.httpreq {
            HttpReq::Head => "HEAD",
            HttpReq::Put => "PUT",
            HttpReq::Post => "POST",
            HttpReq::Get => "GET",
        };
        let method = o.custom_request.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| base.to_string());
        let body = match o.httpreq {
            HttpReq::Put if o.read_fn == 0 && o.read_data == 0 && (o.postfields != 0 || o.copypostfields.is_some()) => {
                let size = if o.postfieldsize >= 0 { o.postfieldsize } else { o.infilesize };
                if let Some(c) = &o.copypostfields {
                    let n = if size >= 0 { (size as usize).min(c.len()) } else { c.len() };
                    c[..n].to_vec()
                } else {
                    let p = o.postfields as *const u8;
                    let n = if size >= 0 { size as usize } else { cstr_bytes(p).len() };
                    core::slice::from_raw_parts(p, n).to_vec()
                }
            }
            HttpReq::Put => self.read_body(o.infilesize).map_err(read_err)?,
            HttpReq::Post => {
                if let Some(c) = &o.copypostfields {
                    let n = if o.postfieldsize >= 0 { (o.postfieldsize as usize).min(c.len()) } else { c.len() };
                    c[..n].to_vec()
                } else if o.postfields != 0 {
                    let p = o.postfields as *const u8;
                    let n = if o.postfieldsize >= 0 { o.postfieldsize as usize } else { cstr_bytes(p).len() };
                    core::slice::from_raw_parts(p, n).to_vec()
                } else {
                    self.read_body(o.postfieldsize).map_err(read_err)?
                }
            }
            _ => Vec::new(),
        };
        let mut headers: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut removed: Vec<String> = Vec::new();
        for line in slist_strings(o.httpheader) {
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_ascii_lowercase();
                if key.is_empty() {
                    continue;
                }
                if v.trim().is_empty() {
                    removed.push(key);
                    continue;
                }
                if matches!(key.as_str(), "content-length" | "transfer-encoding" | "expect" | "connection") {
                    seen.push(key);
                    continue;
                }
                seen.push(key);
                headers.push(format!("{}: {}", k.trim(), v.trim()));
            } else if let Some(k) = line.trim().strip_suffix(';') {
                if !k.is_empty() {
                    seen.push(k.trim().to_ascii_lowercase());
                    headers.push(format!("{}:", k.trim()));
                }
            }
        }
        let want = |n: &str, seen: &Vec<String>| !seen.iter().any(|s| s == n) && !removed.iter().any(|s| s == n);
        let mut extra = Vec::new();
        if let Some(ua) = &o.useragent {
            if want("user-agent", &seen) {
                extra.push(format!("User-Agent: {ua}"));
            }
        }
        let user = o.username.clone().or_else(|| o.userpwd.as_ref().map(|u| u.split_once(':').map(|x| x.0.to_string()).unwrap_or(u.clone()))).or_else(|| url.user.clone());
        if let Some(u) = user {
            let pw = o.password.clone().or_else(|| o.userpwd.as_ref().and_then(|u| u.split_once(':').map(|x| x.1.to_string()))).or_else(|| url.password.clone()).unwrap_or_default();
            if want("authorization", &seen) {
                let dec = |s: &str| String::from_utf8_lossy(&crate::url::unescape(s.as_bytes())).into_owned();
                extra.push(format!("Authorization: Basic {}", b64(format!("{}:{}", dec(&u), dec(&pw)).as_bytes())));
            }
        }
        if want("accept", &seen) {
            extra.push("Accept: */*".into());
        }
        if let Some(r) = &o.referer {
            if want("referer", &seen) {
                extra.push(format!("Referer: {r}"));
            }
        }
        if let Some(c) = &o.cookie {
            if want("cookie", &seen) {
                extra.push(format!("Cookie: {c}"));
            }
        }
        if let Some(r) = &o.range {
            if want("range", &seen) {
                extra.push(format!("Range: bytes={r}"));
            }
        }
        if o.httpreq == HttpReq::Post && want("content-type", &seen) {
            extra.push("Content-Type: application/x-www-form-urlencoded".into());
        }
        extra.extend(headers);
        let decompress = match &o.accept_encoding {
            None => 0,
            Some(s) if s.trim().is_empty() => 3,
            Some(s) => {
                let l = s.to_ascii_lowercase();
                (if l.contains("gzip") { 1 } else { 0 }) | (if l.contains("deflate") { 2 } else { 0 })
            }
        };
        let connect_to = slist_strings(o.connect_to).iter().filter_map(|s| parse_connect_to(s)).collect();
        self.info.size_upload = body.len() as f64;
        self.info.content_length_upload = if o.httpreq == HttpReq::Put || o.httpreq == HttpReq::Post { body.len() as f64 } else { -1.0 };
        self.info.effective_method = nul(&method);
        self.info.scheme = nul(&scheme.to_ascii_uppercase());
        let mut eff = url.clone();
        eff.fragment = None;
        self.info.effective_url = nul(&eff.compose(0));
        self.label = format!("{method} {}", eff.compose(0));
        Ok(Request {
            url,
            method,
            headers: extra,
            body,
            follow: o.follow,
            maxredirs: o.maxredirs,
            postredir: o.postredir,
            timeout_ms: o.timeout_ms,
            connect_timeout_ms: o.connect_timeout_ms,
            verify_peer: o.verify_peer,
            verify_host: o.verify_host,
            nobody: o.httpreq == HttpReq::Head,
            decompress,
            connect_to,
            tls,
            http2: o.http_version >= 3 || (o.http_version == 0 && scheme == "https"),
            forbid_reuse: o.forbid_reuse,
            fresh: o.fresh_connect,
            pool: self.pool.clone().unwrap_or_else(Pool::global),
            proxy,
        })
    }

    pub fn begin(&mut self, sig: Arc<Signal>, slot: Option<crate::gate::Slot>) -> Result<(), i32> {
        self.abort();
        self.info = Info::default();
        self.result = None;
        if self.opts.errorbuffer != 0 {
            unsafe { *(self.opts.errorbuffer as *mut u8) = 0 };
        }
        let start = Instant::now();
        let req = match unsafe { self.build() } {
            Ok(r) => r,
            Err((code, msg)) => {
                self.set_error(&msg);
                self.result = Some(code);
                compat_common::record("xcurl", "transfer", &format!("rejected before start: code {code} ({msg})"));
                return Err(code);
            }
        };
        unsafe {
            if self.opts.verbose && self.opts.debug_fn != 0 {
                let host = req.url.host_bare();
                let port = req.url.effective_port();
                self.debug(CURLINFO_TEXT, format!("Connecting to {host} port {port}\n").as_bytes());
                let mut target = req.url.path_or_root();
                if let Some(q) = &req.url.query {
                    target.push('?');
                    target.push_str(q);
                }
                let mut head = format!("{} {target} HTTP/1.1\r\nHost: {host}\r\n", req.method);
                for h in &req.headers {
                    head.push_str(h);
                    head.push_str("\r\n");
                }
                head.push_str("\r\n");
                self.debug(CURLINFO_HEADER_OUT, head.as_bytes());
                self.debug(CURLINFO_DATA_OUT, &req.body);
            }
        }
        let deadline = if req.timeout_ms > 0 { Some(start + Duration::from_millis(req.timeout_ms)) } else { None };
        let nobody = req.nobody;
        crate::gate::watch(&sig);
        let shared = start_transfer(req, sig);
        self.xfer = Some(Xfer {
            shared,
            start,
            deadline,
            status: 0,
            nobody,
            held: std::collections::VecDeque::new(),
            retry_at: None,
            speed_mark: (start, 0.0),
            last_progress: None,
            _slot: slot,
        });
        Ok(())
    }

    pub fn abort(&mut self) {
        if let Some(x) = self.xfer.take() {
            x.shared.abort();
        }
    }

    unsafe fn call_data(f: usize, data: usize, bytes: &[u8]) -> Delivery {
        if bytes.is_empty() {
            return Delivery::Done;
        }
        let n = if f != 0 {
            let cb: DataCb = core::mem::transmute(f);
            cb(bytes.as_ptr() as *mut u8, 1, bytes.len(), data as *mut c_void)
        } else if data != 0 {
            crt_fwrite(bytes.as_ptr(), bytes.len(), data as *mut c_void)
        } else {
            bytes.len()
        };
        if n == bytes.len() {
            Delivery::Done
        } else if f != 0 && n == CURL_WRITEFUNC_PAUSE {
            Delivery::Paused
        } else {
            Delivery::Failed
        }
    }

    unsafe fn deliver_header(&self, line: &[u8]) -> Delivery {
        let o = &self.opts;
        let d = if o.header_fn != 0 {
            Self::call_data(o.header_fn, o.header_data, line)
        } else if o.header_data != 0 {
            Self::call_data(o.write_fn, o.header_data, line)
        } else {
            Delivery::Done
        };
        if !matches!(d, Delivery::Done) {
            return d;
        }
        if o.header_in_body {
            return Self::call_data(o.write_fn, o.write_data, line);
        }
        Delivery::Done
    }

    fn finish(&mut self, code: i32, msg: &str) -> i32 {
        if let Some(x) = self.xfer.take() {
            if code != CURLE_OK {
                x.shared.abort();
            }
            self.info.total_time = x.start.elapsed().as_secs_f64();
        }
        if code != CURLE_OK && !msg.is_empty() {
            self.set_error(msg);
        }
        self.result = Some(code);
        compat_common::record(
            "xcurl",
            "transfer",
            &format!("{} -> HTTP {} code {code} ({:.0} bytes, {:.3}s){}", self.label, self.info.response_code, self.info.size_download, self.info.total_time,
                if msg.is_empty() { String::new() } else { format!(" {msg}") }),
        );
        code
    }

    fn progress_active(&self) -> bool {
        !self.opts.noprogress && (self.opts.xferinfo_fn != 0 || self.opts.progress_fn != 0)
    }

    fn speed_due(&self, x: &Xfer) -> Option<Instant> {
        if self.opts.low_speed_limit > 0 && self.opts.low_speed_time > 0 {
            Some(x.speed_mark.0 + Duration::from_secs(self.opts.low_speed_time as u64))
        } else {
            None
        }
    }

    pub fn pending(&self) -> bool {
        if crate::gate::suspended() {
            return false;
        }
        let now = Instant::now();
        self.xfer.as_ref().map_or(false, |x| {
            if let Some(r) = x.retry_at {
                return now >= r;
            }
            x.shared.pending()
                || x.deadline.map_or(false, |d| now >= d)
                || self.speed_due(x).map_or(false, |d| now >= d)
                || (self.progress_active() && x.last_progress.map_or(true, |p| now >= p + Duration::from_secs(1)))
        })
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        let x = self.xfer.as_ref()?;
        let progress = if self.progress_active() { Some(x.last_progress.map_or_else(Instant::now, |p| p + Duration::from_secs(1))) } else { None };
        [x.deadline, x.retry_at, self.speed_due(x), progress].into_iter().flatten().min()
    }

    fn hold(&mut self, item: Held) {
        if let Some(x) = self.xfer.as_mut() {
            x.held.push_front(item);
            x.retry_at = Some(Instant::now() + PAUSE_RETRY);
        }
    }

    fn too_big(&self) -> bool {
        let m = self.opts.maxfilesize;
        m > 0 && (self.info.size_download > m as f64 || self.info.content_length_download > m as f64)
    }

    pub fn pump(&mut self) -> Option<i32> {
        if self.xfer.is_none() {
            return self.result;
        }
        if crate::gate::suspended() {
            return None;
        }
        let now = Instant::now();
        {
            let x = self.xfer.as_mut()?;
            if let Some(r) = x.retry_at {
                if now < r {
                    return self.check_limits(false);
                }
            }
            x.retry_at = None;
            let fresh = x.shared.take();
            x.held.extend(fresh.into_iter().map(Held::Ev));
        }
        let mut progressed = false;
        while let Some(item) = self.xfer.as_mut().and_then(|x| x.held.pop_front()) {
            let start = self.xfer.as_ref().map(|x| x.start).unwrap_or(now);
            match item {
                Held::Ev(Ev::Response { status, version, raw, url, redirect, primary_ip, port, reused }) => {
                    let t = start.elapsed().as_secs_f64();
                    self.info.response_code = status;
                    self.info.http_version = version;
                    self.info.header_size += raw.len() as i32;
                    self.info.effective_url = nul(&url);
                    self.info.primary_ip = nul(&primary_ip);
                    self.info.primary_port = port as i32;
                    if !reused {
                        self.info.num_connects += 1;
                    }
                    if self.info.connect_time == 0.0 {
                        self.info.connect_time = t;
                    }
                    self.info.starttransfer_time = t;
                    self.info.content_type = header_value(&raw, "content-type").map(|v| nul(&v));
                    self.info.content_length_download = header_value(&raw, "content-length").and_then(|v| v.parse::<f64>().ok()).unwrap_or(-1.0);
                    self.info.retry_after = header_value(&raw, "retry-after").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                    self.info.redirect_url = if (300..400).contains(&status) {
                        header_value(&raw, "location").and_then(|l| {
                            let base = Url::parse(&String::from_utf8_lossy(&self.info.effective_url[..self.info.effective_url.len() - 1]), true).ok()?;
                            base.join(&l).ok().map(|u| nul(&u.compose(0)))
                        })
                    } else {
                        None
                    };
                    if redirect {
                        self.info.redirect_count += 1;
                        self.info.redirect_time = t;
                    }
                    if let Some(x) = self.xfer.as_mut() {
                        x.status = status;
                    }
                    if self.opts.fail_on_error && status >= 400 && !redirect {
                        return Some(self.finish(CURLE_HTTP_RETURNED_ERROR, &format!("The requested URL returned error: {status}")));
                    }
                    if !redirect && self.too_big() {
                        return Some(self.finish(CURLE_FILESIZE_EXCEEDED, "Maximum file size exceeded"));
                    }
                    unsafe { self.debug(CURLINFO_HEADER_IN, &raw) };
                    if let Some(x) = self.xfer.as_mut() {
                        x.held.push_front(Held::Lines(raw));
                    }
                    progressed = true;
                }
                Held::Lines(raw) => {
                    let mut off = 0;
                    while off < raw.len() {
                        let n = raw[off..].windows(2).position(|w| w == b"\r\n").map(|i| i + 2).unwrap_or(raw.len() - off);
                        match unsafe { self.deliver_header(&raw[off..off + n]) } {
                            Delivery::Done => off += n,
                            Delivery::Paused => {
                                self.hold(Held::Lines(raw[off..].to_vec()));
                                return self.check_limits(progressed);
                            }
                            Delivery::Failed => return Some(self.finish(CURLE_WRITE_ERROR, "Failed writing received data to disk/application")),
                        }
                    }
                }
                Held::Ev(Ev::Data(bytes)) => {
                    if self.xfer.as_ref().map_or(false, |x| x.nobody) {
                        continue;
                    }
                    self.info.size_download += bytes.len() as f64;
                    if self.too_big() {
                        return Some(self.finish(CURLE_FILESIZE_EXCEEDED, "Maximum file size exceeded"));
                    }
                    unsafe { self.debug(CURLINFO_DATA_IN, &bytes) };
                    if let Some(x) = self.xfer.as_mut() {
                        x.held.push_front(Held::Bytes(bytes));
                    }
                    progressed = true;
                }
                Held::Bytes(bytes) => {
                    let chunk = self.opts.buffersize.clamp(1024, 10 * 1024 * 1024);
                    let mut off = 0;
                    while off < bytes.len() {
                        let end = (off + chunk).min(bytes.len());
                        match unsafe { Self::call_data(self.opts.write_fn, self.opts.write_data, &bytes[off..end]) } {
                            Delivery::Done => off = end,
                            Delivery::Paused => {
                                self.hold(Held::Bytes(bytes[off..].to_vec()));
                                return self.check_limits(progressed);
                            }
                            Delivery::Failed => return Some(self.finish(CURLE_WRITE_ERROR, "Failure writing output to destination")),
                        }
                    }
                }
                Held::Ev(Ev::Done { code, os_errno, msg }) => {
                    self.info.os_errno = os_errno;
                    if code == CURLE_OK {
                        if let Some(c) = self.call_progress() {
                            return Some(c);
                        }
                    }
                    let code = if code == CURLE_OK && self.info.response_code == 0 { CURLE_GOT_NOTHING } else { code };
                    return Some(self.finish(code, &msg));
                }
            }
        }
        self.check_limits(progressed)
    }

    fn call_progress(&mut self) -> Option<i32> {
        if !self.progress_active() {
            return None;
        }
        let responded = self.info.response_code != 0;
        let dltotal = self.info.content_length_download.max(0.0);
        let dlnow = self.info.size_download;
        let ultotal = self.info.content_length_upload.max(0.0);
        let ulnow = if responded { self.info.size_upload } else { 0.0 };
        if let Some(x) = self.xfer.as_mut() {
            x.last_progress = Some(Instant::now());
        }
        let o = &self.opts;
        let rc = unsafe {
            if o.xferinfo_fn != 0 {
                let f: XferInfoCb = core::mem::transmute(o.xferinfo_fn);
                let r = f(o.progress_data as *mut c_void, dltotal as i64, dlnow as i64, ultotal as i64, ulnow as i64);
                if r == CURL_PROGRESSFUNC_CONTINUE { 0 } else { r }
            } else {
                let f: ProgressCb = core::mem::transmute(o.progress_fn);
                f(o.progress_data as *mut c_void, dltotal, dlnow, ultotal, ulnow)
            }
        };
        if rc != 0 {
            return Some(self.finish(CURLE_ABORTED_BY_CALLBACK, "Callback aborted"));
        }
        None
    }

    fn check_limits(&mut self, progressed: bool) -> Option<i32> {
        let now = Instant::now();
        let tick = self.xfer.as_ref().map_or(false, |x| x.last_progress.map_or(true, |p| now >= p + Duration::from_secs(1)));
        if progressed || tick {
            if let Some(c) = self.call_progress() {
                return Some(c);
            }
        }
        let x = self.xfer.as_ref()?;
        if let Some(d) = x.deadline {
            if now >= d {
                let ms = d.duration_since(x.start).as_millis();
                let got = self.info.size_download;
                return Some(self.finish(CURLE_OPERATION_TIMEDOUT, &format!("Operation timed out after {ms} milliseconds with {got:.0} bytes received")));
            }
        }
        if let Some(due) = self.speed_due(x) {
            if now >= due {
                let (t0, b0) = x.speed_mark;
                let secs = now.duration_since(t0).as_secs_f64().max(0.001);
                let moved = self.info.size_download - b0;
                if moved / secs < self.opts.low_speed_limit as f64 {
                    let (l, t) = (self.opts.low_speed_limit, self.opts.low_speed_time);
                    return Some(self.finish(CURLE_OPERATION_TIMEDOUT, &format!("Operation too slow. Less than {l} bytes/sec transferred the last {t} seconds")));
                }
                let size = self.info.size_download;
                if let Some(x) = self.xfer.as_mut() {
                    x.speed_mark = (now, size);
                }
            }
        }
        None
    }

    pub fn perform(&mut self) -> i32 {
        let sig = Signal::new();
        crate::gate::watch(&sig);
        let deadline = if self.opts.timeout_ms > 0 { Some(Instant::now() + Duration::from_millis(self.opts.timeout_ms)) } else { None };
        let Some(slot) = crate::gate::acquire_until(deadline) else {
            let msg = format!("Operation timed out after {} milliseconds waiting for a request slot (limit {}, suspended {})", self.opts.timeout_ms, crate::gate::limit(), crate::gate::suspended());
            self.set_error(&msg);
            self.result = Some(CURLE_OPERATION_TIMEDOUT);
            compat_common::record("xcurl", "transfer", &msg);
            return CURLE_OPERATION_TIMEDOUT;
        };
        if let Err(c) = self.begin(sig.clone(), Some(slot)) {
            return c;
        }
        loop {
            let s0 = sig.seq();
            if let Some(c) = self.pump() {
                return c;
            }
            let wait = self.next_deadline().map(|d| d.saturating_duration_since(Instant::now())).unwrap_or(Duration::from_secs(1)).min(Duration::from_secs(1));
            sig.wait_change(s0, wait.max(Duration::from_millis(1)));
        }
    }

    pub unsafe fn setopt(&mut self, opt: u32, arg: usize) -> i32 {
        let long = arg as u32 as i32 as i64;
        let s = || cstring(arg as *const u8);
        let o = &mut self.opts;
        match opt {
            10001 => o.write_data = arg,
            10002 => o.url = s(),
            3 => o.port = if long > 0 && long <= 65535 { Some(long as u16) } else { None },
            10004 => o.proxy = s(),
            59 => o.proxy_port = long,
            101 => {
                if ![0, 1, 2, 4, 5, 6, 7].contains(&long) {
                    return CURLE_BAD_FUNCTION_ARGUMENT;
                }
                o.proxy_type = long
            }
            10006 => o.proxy_userpwd = s(),
            10175 => o.proxy_username = s(),
            10176 => o.proxy_password = s(),
            10177 => o.noproxy = s(),
            10228 => o.proxy_headers = arg,
            61 => o.proxy_tunnel = long != 0,
            10025 => o.ssl_cert = s(),
            40291 => o.ssl_cert_blob = blob(arg),
            10086 => o.ssl_cert_type = s(),
            10087 => o.ssl_key = s(),
            40292 => o.ssl_key_blob = blob(arg),
            10088 => o.ssl_key_type = s(),
            10026 => o.key_passwd = s(),
            32 => {
                if (long & 0xffff) > 7 || ((long >> 16) & 0xffff) > 7 || long < 0 {
                    return CURLE_BAD_FUNCTION_ARGUMENT;
                }
                o.ssl_version = long
            }
            10083 => o.cipher_list = s(),
            10276 => o.tls13_ciphers = s(),
            10065 => o.cainfo = s(),
            40309 => o.cainfo_blob = blob(arg),
            216 => o.ssl_options = long,
            232 => o.verify_status = long != 0,
            84 => {
                if !(0..=5).contains(&long) && long != 30 && long != 31 {
                    return CURLE_UNSUPPORTED_PROTOCOL;
                }
                o.http_version = long
            }
            75 => o.forbid_reuse = long != 0,
            74 => o.fresh_connect = long != 0,
            98 => o.buffersize = if long <= 0 { CURL_MAX_WRITE_SIZE } else { (long as usize).clamp(1024, 10 * 1024 * 1024) },
            114 => o.maxfilesize = long,
            30117 => o.maxfilesize = arg as i64,
            19 => o.low_speed_limit = long,
            20 => o.low_speed_time = long,
            20094 => o.debug_fn = arg,
            10095 => o.debug_data = arg,
            43 => o.noprogress = long != 0,
            20056 => o.progress_fn = arg,
            20219 => o.xferinfo_fn = arg,
            10057 => o.progress_data = arg,
            99 | 121 => {}
            213 => o.tcp_keepalive = long != 0,
            214 => o.tcp_keepidle = long,
            215 => o.tcp_keepintvl = long,
            113 => {
                if !(0..=2).contains(&long) {
                    return CURLE_BAD_FUNCTION_ARGUMENT;
                }
                o.ipresolve = long
            }
            92 => o.dns_cache_timeout = long,
            10005 => o.userpwd = s(),
            10173 => o.username = s(),
            10174 => o.password = s(),
            10007 => o.range = s(),
            10009 => o.read_data = arg,
            10010 => o.errorbuffer = arg,
            20011 => o.write_fn = arg,
            20012 => o.read_fn = arg,
            13 => o.timeout_ms = (long.max(0) as u64) * 1000,
            155 => o.timeout_ms = long.max(0) as u64,
            78 => o.connect_timeout_ms = (long.max(0) as u64) * 1000,
            156 => o.connect_timeout_ms = long.max(0) as u64,
            14 => o.infilesize = long,
            30115 => o.infilesize = arg as i64,
            10015 => {
                o.postfields = arg;
                o.copypostfields = None;
                o.httpreq = HttpReq::Post;
            }
            10165 => {
                if arg == 0 {
                    o.copypostfields = Some(Vec::new());
                } else {
                    let p = arg as *const u8;
                    let n = if o.postfieldsize >= 0 { o.postfieldsize as usize } else { cstr_bytes(p).len() };
                    o.copypostfields = Some(core::slice::from_raw_parts(p, n).to_vec());
                }
                o.postfields = 0;
                o.httpreq = HttpReq::Post;
            }
            60 => o.postfieldsize = long,
            30120 => o.postfieldsize = arg as i64,
            10016 => o.referer = s(),
            10018 => o.useragent = s(),
            10022 => o.cookie = s(),
            10023 => o.httpheader = arg,
            10243 => o.connect_to = arg,
            10029 => o.header_data = arg,
            20079 => o.header_fn = arg,
            10036 => o.custom_request = s(),
            41 => o.verbose = long != 0,
            42 => o.header_in_body = long != 0,
            44 => {
                if long != 0 {
                    o.httpreq = HttpReq::Head
                } else if o.httpreq == HttpReq::Head {
                    o.httpreq = HttpReq::Get
                }
            }
            45 => o.fail_on_error = long != 0,
            46 => o.httpreq = if long != 0 { HttpReq::Put } else { HttpReq::Get },
            47 => o.httpreq = if long != 0 { HttpReq::Post } else { HttpReq::Get },
            80 => {
                if long != 0 {
                    o.httpreq = HttpReq::Get
                }
            }
            52 => o.follow = long != 0,
            68 => o.maxredirs = long,
            161 => o.postredir = long as u32,
            64 => o.verify_peer = long != 0,
            81 => o.verify_host = long != 0,
            10102 => o.accept_encoding = s().or(Some(String::new())).filter(|_| arg != 0),
            10103 => o.private = arg,
            _ => {
                note_unknown(opt, arg);
                return CURLE_UNKNOWN_OPTION;
            }
        }
        CURLE_OK
    }

    pub unsafe fn getinfo(&self, info: u32, out: *mut c_void) -> i32 {
        if out.is_null() {
            return CURLE_BAD_FUNCTION_ARGUMENT;
        }
        let i = &self.info;
        let ty = info & 0xf0_0000;
        let s = |v: &Vec<u8>| v.as_ptr();
        let total_us = (i.total_time * 1e6) as i64;
        match ty {
            0x10_0000 => {
                let p: *const u8 = match info & 0xfffff {
                    1 => s(&i.effective_url),
                    18 => i.content_type.as_ref().map_or(core::ptr::null(), |v| v.as_ptr()),
                    21 => self.opts.private as *const u8,
                    31 => i.redirect_url.as_ref().map_or(core::ptr::null(), |v| v.as_ptr()),
                    32 => s(&i.primary_ip),
                    41 => b"127.0.0.1\0".as_ptr(),
                    49 => s(&i.scheme),
                    58 => s(&i.effective_method),
                    _ => core::ptr::null(),
                };
                *(out as *mut *const u8) = p;
            }
            0x20_0000 => {
                let v: i32 = match info & 0xfffff {
                    2 => i.response_code,
                    11 => i.header_size,
                    12 => i.request_size,
                    13 => 0,
                    14 => -1,
                    20 => i.redirect_count,
                    22 => 0,
                    25 => i.os_errno,
                    26 => i.num_connects,
                    29 => -1,
                    40 => i.primary_port,
                    42 => 0,
                    46 => i.http_version,
                    48 => if i.scheme.starts_with(b"HTTPS") { 2 } else { 1 },
                    _ => 0,
                };
                *(out as *mut i32) = v;
            }
            0x30_0000 => {
                let v: f64 = match info & 0xfffff {
                    3 => i.total_time,
                    4 => 0.0,
                    5 | 6 | 33 => i.connect_time,
                    7 => i.size_upload,
                    8 => i.size_download,
                    9 => if i.total_time > 0.0 { i.size_download / i.total_time } else { 0.0 },
                    10 => if i.total_time > 0.0 { i.size_upload / i.total_time } else { 0.0 },
                    15 => i.content_length_download,
                    16 => i.content_length_upload,
                    17 => i.starttransfer_time,
                    19 => i.redirect_time,
                    _ => 0.0,
                };
                *(out as *mut f64) = v;
            }
            0x40_0000 => *(out as *mut *const c_void) = core::ptr::null(),
            0x50_0000 => *(out as *mut usize) = usize::MAX,
            0x60_0000 => {
                let us = |t: f64| (t * 1e6) as i64;
                let v: i64 = match info & 0xfffff {
                    7 => i.size_upload as i64,
                    8 => i.size_download as i64,
                    9 => if i.total_time > 0.0 { (i.size_download / i.total_time) as i64 } else { 0 },
                    10 => if i.total_time > 0.0 { (i.size_upload / i.total_time) as i64 } else { 0 },
                    14 => -1,
                    15 => i.content_length_download as i64,
                    16 => i.content_length_upload as i64,
                    50 => total_us,
                    51 => 0,
                    52 | 53 | 56 => us(i.connect_time),
                    54 => us(i.starttransfer_time),
                    55 => us(i.redirect_time),
                    57 => i.retry_after,
                    _ => 0,
                };
                *(out as *mut i64) = v;
            }
            _ => return CURLE_UNKNOWN_OPTION,
        }
        CURLE_OK
    }
}

fn start_transfer(req: Request, sig: Arc<Signal>) -> Arc<Shared> {
    crate::net::start(req, sig)
}

fn note_unknown(opt: u32, arg: usize) {
    use std::sync::Mutex;
    static SEEN: Mutex<Vec<u32>> = Mutex::new(Vec::new());
    let mut g = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if !g.contains(&opt) && g.len() < 512 {
        g.push(opt);
        compat_common::record("xcurl", "unimplemented", &format!("curl_easy_setopt option {opt} (arg 0x{arg:x}) is not implemented; returned CURLE_UNKNOWN_OPTION"));
    }
}

unsafe fn blob(arg: usize) -> Option<Vec<u8>> {
    let b = (arg as *const CurlBlob).as_ref()?;
    if b.data.is_null() {
        return None;
    }
    Some(core::slice::from_raw_parts(b.data, b.len).to_vec())
}

pub unsafe fn easy<'a>(h: *mut c_void) -> Option<&'a mut Easy> {
    (h as *mut Easy).as_mut()
}

#[no_mangle]
pub extern "C" fn curl_easy_init() -> *mut c_void {
    if !crate::global::ensure_for_easy() {
        return core::ptr::null_mut();
    }
    crate::guard();
    Box::into_raw(Box::new(Easy::new())) as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn curl_easy_cleanup(h: *mut c_void) {
    if h.is_null() {
        return;
    }
    let e = h as *mut Easy;
    if (*e).multi != 0 {
        crate::multi::detach((*e).multi, e);
    }
    (*e).abort();
    drop(Box::from_raw(e));
}

#[no_mangle]
pub unsafe extern "C" fn curl_easy_setopt(h: *mut c_void, opt: u32, arg: usize) -> i32 {
    match easy(h) {
        Some(e) => e.setopt(opt, arg),
        None => CURLE_BAD_FUNCTION_ARGUMENT,
    }
}

#[no_mangle]
pub unsafe extern "C" fn curl_easy_getinfo(h: *mut c_void, info: u32, out: *mut c_void) -> i32 {
    match easy(h) {
        Some(e) => e.getinfo(info, out),
        None => CURLE_BAD_FUNCTION_ARGUMENT,
    }
}

#[no_mangle]
pub unsafe extern "C" fn curl_easy_perform(h: *mut c_void) -> i32 {
    crate::guard();
    match easy(h) {
        Some(e) if e.multi != 0 => CURLE_FAILED_INIT,
        Some(e) => e.perform(),
        None => CURLE_BAD_FUNCTION_ARGUMENT,
    }
}

#[no_mangle]
pub unsafe extern "C" fn curl_easy_duphandle(h: *mut c_void) -> *mut c_void {
    match easy(h) {
        Some(e) => {
            let mut n = Easy::new();
            n.opts = e.opts.clone();
            Box::into_raw(Box::new(n)) as *mut c_void
        }
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn curl_easy_reset(h: *mut c_void) {
    if let Some(e) = easy(h) {
        e.abort();
        e.opts = Opts::default();
        e.info = Info::default();
        e.result = None;
    }
}
