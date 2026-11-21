use crate::sys::*;
use crate::url::{encode_request_target, Url};
use core::ffi::c_void;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const CURLE_OK: i32 = 0;
pub const CURLE_UNSUPPORTED_PROTOCOL: i32 = 1;
pub const CURLE_FAILED_INIT: i32 = 2;
pub const CURLE_URL_MALFORMAT: i32 = 3;
pub const CURLE_COULDNT_RESOLVE_HOST: i32 = 6;
pub const CURLE_COULDNT_CONNECT: i32 = 7;
pub const CURLE_WEIRD_SERVER_REPLY: i32 = 8;
pub const CURLE_HTTP_RETURNED_ERROR: i32 = 22;
pub const CURLE_WRITE_ERROR: i32 = 23;
pub const CURLE_READ_ERROR: i32 = 26;
pub const CURLE_OUT_OF_MEMORY: i32 = 27;
pub const CURLE_OPERATION_TIMEDOUT: i32 = 28;
pub const CURLE_ABORTED_BY_CALLBACK: i32 = 42;
pub const CURLE_BAD_FUNCTION_ARGUMENT: i32 = 43;
pub const CURLE_TOO_MANY_REDIRECTS: i32 = 47;
pub const CURLE_UNKNOWN_OPTION: i32 = 48;
pub const CURLE_GOT_NOTHING: i32 = 52;
pub const CURLE_SEND_ERROR: i32 = 55;
pub const CURLE_RECV_ERROR: i32 = 56;
pub const CURLE_SSL_CERTPROBLEM: i32 = 58;
pub const CURLE_PEER_FAILED_VERIFICATION: i32 = 60;
pub const CURLE_SSL_CONNECT_ERROR: i32 = 35;

pub struct Signal {
    seq: Mutex<u64>,
    cv: Condvar,
    woken: AtomicBool,
}

impl Signal {
    pub fn new() -> Arc<Signal> {
        Arc::new(Signal { seq: Mutex::new(0), cv: Condvar::new(), woken: AtomicBool::new(false) })
    }

    pub fn notify(&self) {
        let mut g = self.seq.lock().unwrap_or_else(|e| e.into_inner());
        *g = g.wrapping_add(1);
        self.cv.notify_all();
    }

    pub fn wake(&self) {
        self.woken.store(true, Ordering::SeqCst);
        self.notify();
    }

    pub fn take_woken(&self) -> bool {
        self.woken.swap(false, Ordering::SeqCst)
    }

    pub fn seq(&self) -> u64 {
        *self.seq.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn wait_change(&self, seq0: u64, timeout: Duration) -> bool {
        let g = self.seq.lock().unwrap_or_else(|e| e.into_inner());
        let (g, _) = self.cv.wait_timeout_while(g, timeout, |s| *s == seq0).unwrap_or_else(|e| e.into_inner());
        *g != seq0
    }
}

pub enum Ev {
    Response { status: i32, version: i32, raw: Vec<u8>, url: String, redirect: bool, primary_ip: String, port: u16, reused: bool },
    Data(Vec<u8>),
    Done { code: i32, os_errno: i32, msg: String },
}

pub struct Shared {
    pub q: Mutex<VecDeque<Ev>>,
    pub cancel: AtomicBool,
    req: Mutex<usize>,
    pub sig: Arc<Signal>,
}

impl Shared {
    fn push(&self, e: Ev) {
        self.q.lock().unwrap_or_else(|e| e.into_inner()).push_back(e);
        self.sig.notify();
    }

    pub fn pending(&self) -> bool {
        !self.q.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }

    pub fn take(&self) -> VecDeque<Ev> {
        core::mem::take(&mut *self.q.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn abort(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        let mut g = self.req.lock().unwrap_or_else(|e| e.into_inner());
        let h = core::mem::replace(&mut *g, 0);
        if h != 0 {
            unsafe { WinHttpCloseHandle(h as HINTERNET) };
        }
    }

    fn set_req(&self, h: HINTERNET) {
        *self.req.lock().unwrap_or_else(|e| e.into_inner()) = h as usize;
    }

    fn close_req(&self) {
        let mut g = self.req.lock().unwrap_or_else(|e| e.into_inner());
        let h = core::mem::replace(&mut *g, 0);
        if h != 0 {
            unsafe { WinHttpCloseHandle(h as HINTERNET) };
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
pub struct ConnectTo {
    pub host: String,
    pub port: Option<u16>,
    pub to_host: String,
    pub to_port: Option<u16>,
}

pub fn parse_connect_to(s: &str) -> Option<ConnectTo> {
    let p: Vec<&str> = s.splitn(4, ':').collect();
    if p.len() != 4 {
        return None;
    }
    let port = |x: &str| if x.trim().is_empty() { None } else { x.trim().parse::<u16>().ok() };
    Some(ConnectTo { host: p[0].trim().to_string(), port: port(p[1]), to_host: p[2].trim().to_string(), to_port: port(p[3]) })
}

pub struct Request {
    pub url: Url,
    pub method: String,
    pub headers: Vec<String>,
    pub body: Vec<u8>,
    pub follow: bool,
    pub maxredirs: i64,
    pub postredir: u32,
    pub timeout_ms: u64,
    pub connect_timeout_ms: u64,
    pub verify_peer: bool,
    pub verify_host: bool,
    pub nobody: bool,
    pub decompress: u32,
    pub connect_to: Vec<ConnectTo>,
    pub tls: crate::tls::Tls,
    pub http2: bool,
    pub forbid_reuse: bool,
    pub fresh: bool,
    pub pool: Arc<Pool>,
    pub proxy: Option<ProxyPlan>,
}

#[derive(Clone)]
pub struct ProxyPlan {
    pub host: String,
    pub port: u16,
    pub auth: Option<String>,
    pub headers: Vec<String>,
    pub tunnel: bool,
    pub user: Option<String>,
    pub password: Option<String>,
}

pub struct Pool {
    idle: Mutex<Vec<(String, usize, Instant)>>,
    cap: std::sync::atomic::AtomicUsize,
    opened: std::sync::atomic::AtomicUsize,
    reused: std::sync::atomic::AtomicUsize,
}

impl Pool {
    pub fn new(cap: usize) -> Arc<Pool> {
        Arc::new(Pool { idle: Mutex::new(Vec::new()), cap: std::sync::atomic::AtomicUsize::new(cap), opened: Default::default(), reused: Default::default() })
    }

    pub fn global() -> Arc<Pool> {
        static P: OnceLock<Arc<Pool>> = OnceLock::new();
        P.get_or_init(|| Pool::new(5)).clone()
    }

    pub fn close_idle(&self) {
        let drained: Vec<_> = self.idle.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect();
        for (_, h, _) in drained {
            unsafe { WinHttpCloseHandle(h as HINTERNET) };
        }
    }

    pub fn set_cap(&self, cap: usize) {
        self.cap.store(cap, Ordering::SeqCst);
        self.trim();
    }

    pub fn cap(&self) -> usize {
        self.cap.load(Ordering::SeqCst)
    }

    pub fn idle(&self) -> usize {
        self.idle.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn stats(&self) -> (usize, usize) {
        (self.opened.load(Ordering::SeqCst), self.reused.load(Ordering::SeqCst))
    }

    fn take(&self, key: &str) -> Option<HINTERNET> {
        let mut g = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        let i = g.iter().rposition(|(k, _, _)| k == key)?;
        let (_, h, _) = g.remove(i);
        self.reused.fetch_add(1, Ordering::SeqCst);
        Some(h as HINTERNET)
    }

    fn give(&self, key: String, h: HINTERNET) {
        if self.cap() == 0 {
            unsafe { WinHttpCloseHandle(h) };
            return;
        }
        self.idle.lock().unwrap_or_else(|e| e.into_inner()).push((key, h as usize, Instant::now()));
        self.trim();
    }

    fn trim(&self) {
        let cap = self.cap();
        let mut g = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        while g.len() > cap {
            let (_, h, _) = g.remove(0);
            unsafe { WinHttpCloseHandle(h as HINTERNET) };
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        for (_, h, _) in self.idle.get_mut().unwrap_or_else(|e| e.into_inner()).drain(..) {
            unsafe { WinHttpCloseHandle(h as HINTERNET) };
        }
    }
}

pub fn is_loopback(host: &str) -> bool {
    matches!(host.trim_matches(['[', ']']).to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
}

pub fn network_allowed() -> bool {
    std::env::var("TOMBOX_ALLOW_NETWORK").map_or(false, |v| v.trim() == "1")
}

fn session(protocols: u32, proxy: Option<String>) -> Option<HINTERNET> {
    static S: OnceLock<Mutex<Vec<(u32, Option<String>, usize)>>> = OnceLock::new();
    let m = S.get_or_init(|| Mutex::new(Vec::new()));
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, _, h)) = g.iter().find(|(p, x, _)| *p == protocols && *x == proxy) {
        return Some(*h as HINTERNET);
    }
    let h = match &proxy {
        Some(name) => {
            let wn = wide(name);
            let wb = wide("<-loopback>");
            unsafe { WinHttpOpen(core::ptr::null(), 3, wn.as_ptr(), wb.as_ptr(), 0) }
        }
        None => unsafe { WinHttpOpen(core::ptr::null(), WINHTTP_ACCESS_TYPE_NO_PROXY, core::ptr::null(), core::ptr::null(), 0) },
    };
    if h.is_null() {
        return None;
    }
    if protocols != 0 && unsafe { WinHttpSetOption(h, WINHTTP_OPTION_SECURE_PROTOCOLS, &protocols as *const u32 as *const c_void, 4) } == 0 {
        unsafe { WinHttpCloseHandle(h) };
        return None;
    }
    g.push((protocols, proxy, h as usize));
    Some(h)
}

unsafe fn request_url(r: HINTERNET) -> String {
    let mut len: u32 = 0;
    WinHttpQueryOption(r, WINHTTP_OPTION_URL, core::ptr::null_mut(), &mut len);
    if len == 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize / 2 + 1];
    let mut l2 = (buf.len() * 2) as u32;
    if WinHttpQueryOption(r, WINHTTP_OPTION_URL, buf.as_mut_ptr() as *mut c_void, &mut l2) == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(l2 as usize / 2).min(buf.len())]).trim_end_matches('\0').to_string()
}

unsafe fn check_server_ca(r: HINTERNET, req: &Request) -> Result<(), (i32, u32, String)> {
    let Some(ca) = req.tls.ca.as_ref() else { return Ok(()) };
    let mut ctx: *const c_void = core::ptr::null();
    let mut n = core::mem::size_of::<usize>() as u32;
    if WinHttpQueryOption(r, WINHTTP_OPTION_SERVER_CERT_CONTEXT, &mut ctx as *mut *const c_void as *mut c_void, &mut n) == 0 || ctx.is_null() {
        let e = GetLastError();
        return Err((CURLE_PEER_FAILED_VERIFICATION, e, "SSL certificate problem: the server certificate is not available for CURLOPT_CAINFO verification".into()));
    }
    let res = crate::tls::verify_with_ca(ctx, ca, req.tls.revocation);
    crate::tls::free_cert(ctx);
    res.map_err(|(c, m)| (c, 0, m))
}

pub fn map_error(err: u32, stage: u8) -> i32 {
    match err {
        12002 => CURLE_OPERATION_TIMEDOUT,
        12005 => CURLE_URL_MALFORMAT,
        12006 => CURLE_UNSUPPORTED_PROTOCOL,
        12007 => CURLE_COULDNT_RESOLVE_HOST,
        12017 => CURLE_ABORTED_BY_CALLBACK,
        12029 => CURLE_COULDNT_CONNECT,
        12030 | 12031 => if stage < 2 { CURLE_SEND_ERROR } else { CURLE_RECV_ERROR },
        12044 => CURLE_SSL_CERTPROBLEM,
        12152 => CURLE_WEIRD_SERVER_REPLY,
        12157 => CURLE_SSL_CONNECT_ERROR,
        12175 | 12037 | 12038 | 12045 | 12169 | 12179 => CURLE_PEER_FAILED_VERIFICATION,
        8 | 14 => CURLE_OUT_OF_MEMORY,
        _ => match stage {
            0 => CURLE_COULDNT_CONNECT,
            1 => CURLE_SEND_ERROR,
            _ => CURLE_RECV_ERROR,
        },
    }
}

fn winhttp_message(err: u32) -> &'static str {
    match err {
        12002 => "Operation timed out",
        12007 => "Could not resolve host",
        12017 => "Operation cancelled",
        12029 => "Failed to connect to server",
        12030 => "Connection was terminated",
        12031 => "Connection was reset",
        12152 => "Invalid server response",
        12175 => "TLS security failure",
        _ => "WinHTTP failure",
    }
}

pub fn start(req: Request, sig: Arc<Signal>) -> Arc<Shared> {
    let sh = Arc::new(Shared { q: Mutex::new(VecDeque::new()), cancel: AtomicBool::new(false), req: Mutex::new(0), sig });
    let w = sh.clone();
    let spawned = std::thread::Builder::new().name("xcurl-transfer".into()).spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&req, &w)));
        if r.is_err() {
            w.push(Ev::Done { code: CURLE_FAILED_INIT, os_errno: 0, msg: "internal transfer failure".into() });
        }
    });
    if spawned.is_err() {
        sh.push(Ev::Done { code: CURLE_OUT_OF_MEMORY, os_errno: 0, msg: "could not start transfer thread".into() });
    }
    sh
}

fn ms_left(deadline: Option<Instant>) -> Option<u64> {
    deadline.map(|d| d.saturating_duration_since(Instant::now()).as_millis() as u64)
}

fn clamp_ms(v: u64) -> i32 {
    v.clamp(1, i32::MAX as u64) as i32
}

unsafe fn query_raw_headers(r: HINTERNET) -> Vec<u8> {
    let mut len: u32 = 0;
    WinHttpQueryHeaders(r, WINHTTP_QUERY_RAW_HEADERS_CRLF, core::ptr::null(), core::ptr::null_mut(), &mut len, core::ptr::null_mut());
    if len == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u16; (len as usize) / 2 + 1];
    let mut l2 = (buf.len() * 2) as u32;
    if WinHttpQueryHeaders(r, WINHTTP_QUERY_RAW_HEADERS_CRLF, core::ptr::null(), buf.as_mut_ptr() as *mut c_void, &mut l2, core::ptr::null_mut()) == 0 {
        return Vec::new();
    }
    let n = (l2 as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..n]);
    let mut out: Vec<u8> = s.trim_end_matches('\0').chars().map(|c| if (c as u32) < 256 { c as u32 as u8 } else { b'?' }).collect();
    while out.ends_with(b"\r\n\r\n\r\n") {
        out.truncate(out.len() - 2);
    }
    if !out.ends_with(b"\r\n\r\n") {
        if out.ends_with(b"\r\n") {
            out.extend_from_slice(b"\r\n");
        } else {
            out.extend_from_slice(b"\r\n\r\n");
        }
    }
    out
}

pub fn header_value(raw: &[u8], name: &str) -> Option<String> {
    let s = String::from_utf8_lossy(raw);
    s.split("\r\n").skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if k.trim().eq_ignore_ascii_case(name) {
            Some(v.trim().to_string())
        } else {
            None
        }
    })
}

fn version_of(raw: &[u8]) -> i32 {
    if raw.starts_with(b"HTTP/1.0") {
        1
    } else if raw.starts_with(b"HTTP/2") {
        3
    } else if raw.starts_with(b"HTTP/3") {
        30
    } else {
        2
    }
}

fn run(req: &Request, sh: &Shared) {
    let (code, errno, msg) = unsafe { transfer(req, sh) };
    sh.close_req();
    let msg = if sh.cancelled() && code != CURLE_OK { "transfer aborted".to_string() } else { msg };
    sh.push(Ev::Done { code, os_errno: errno as i32, msg });
}

unsafe fn transfer(req: &Request, sh: &Shared) -> (i32, u32, String) {
    let native_proxy = req.proxy.as_ref().filter(|p| network_allowed() || is_loopback(&p.host));
    let proxied = native_proxy.and_then(|p| session(req.tls.protocols, Some(format!("{}:{}", p.host, p.port))));
    let Some(plain) = session(req.tls.protocols, None) else {
        let e = GetLastError();
        if req.tls.protocols != 0 {
            return (CURLE_SSL_CONNECT_ERROR, e, format!("the TLS versions requested by CURLOPT_SSLVERSION (WinHTTP flags 0x{:x}) are not available on this system", req.tls.protocols));
        }
        return (CURLE_FAILED_INIT, e, "WinHttpOpen failed".into());
    };
    let deadline = if req.timeout_ms > 0 { Some(Instant::now() + Duration::from_millis(req.timeout_ms)) } else { None };
    let mut url = req.url.clone();
    let mut method = req.method.clone();
    let mut body: &[u8] = &req.body;
    let mut redirects: i64 = 0;
    loop {
        if sh.cancelled() {
            return (CURLE_ABORTED_BY_CALLBACK, 0, "transfer aborted".into());
        }
        let scheme = url.scheme.clone().unwrap_or_else(|| "http".into());
        let https = match scheme.as_str() {
            "https" => true,
            "http" => false,
            _ => return (CURLE_UNSUPPORTED_PROTOCOL, 0, format!("Protocol \"{scheme}\" not supported")),
        };
        let mut host = url.host_bare();
        let mut port = url.effective_port();
        for c in &req.connect_to {
            let host_ok = c.host.is_empty() || c.host.eq_ignore_ascii_case(&host);
            let port_ok = c.port.map_or(true, |p| p == port);
            if host_ok && port_ok {
                if !c.to_host.is_empty() {
                    host = c.to_host.trim_start_matches('[').trim_end_matches(']').to_string();
                }
                if let Some(p) = c.to_port {
                    port = p;
                }
                break;
            }
        }
        let origin_host = host.clone();
        let origin_port = port;
        let native_tunnel = native_proxy;
        let session = match (native_tunnel, proxied) {
            (Some(_), Some(s)) => s,
            (Some(p), None) => return (5, 0, format!("could not open a WinHTTP session for proxy {}:{}", p.host, p.port)),
            _ => plain,
        };
        if let Some(p) = req.proxy.as_ref().filter(|p| !(network_allowed() || is_loopback(&p.host))) {
            compat_common::record("xcurl", "proxy", &format!("proxy {}:{} for {origin_host}:{origin_port} is outside the machine; the network guard is the proxy hop", p.host, p.port));
        }
        let key = format!("{}|{}|{}|{}|{}", req.tls.protocols, https, native_tunnel.map(|p| format!("{}:{}", p.host, p.port)).unwrap_or_default(), host.to_ascii_lowercase(), port);
        let pooled = if req.fresh { None } else { req.pool.take(&key) };
        let reused = pooled.is_some();
        let conn = match pooled {
            Some(c) => c,
            None => {
                let whost = wide(&host);
                let c = WinHttpConnect(session, whost.as_ptr(), port, 0);
                if c.is_null() {
                    let e = GetLastError();
                    return (map_error(e, 0), e, format!("Failed to connect to {host} port {port}: {}", winhttp_message(e)));
                }
                req.pool.opened.fetch_add(1, Ordering::SeqCst);
                c
            }
        };
        let keep = std::cell::Cell::new(false);
        let result = (|| {
            let mut target = url.path_or_root();
            if let Some(q) = &url.query {
                target.push('?');
                target.push_str(q);
            }
            let wtarget = wide(&encode_request_target(&target));
            let wverb = wide(&method);
            let mut flags = WINHTTP_FLAG_ESCAPE_DISABLE | WINHTTP_FLAG_ESCAPE_DISABLE_QUERY;
            if https {
                flags |= WINHTTP_FLAG_SECURE;
            }
            let r = WinHttpOpenRequest(conn, wverb.as_ptr(), wtarget.as_ptr(), core::ptr::null(), core::ptr::null(), core::ptr::null(), flags);
            if r.is_null() {
                let e = GetLastError();
                return Err((map_error(e, 0), e, format!("Failed to open request: {}", winhttp_message(e))));
            }
            sh.set_req(r);
            if sh.cancelled() {
                return Err((CURLE_ABORTED_BY_CALLBACK, 0, "transfer aborted".into()));
            }
            let left = ms_left(deadline);
            let conn_ms = match (req.connect_timeout_ms, left) {
                (0, None) => 300_000,
                (0, Some(l)) => l.min(300_000),
                (c, None) => c,
                (c, Some(l)) => c.min(l),
            };
            let io_ms = left.map(clamp_ms).unwrap_or(i32::MAX);
            WinHttpSetTimeouts(r, clamp_ms(conn_ms), clamp_ms(conn_ms), io_ms, io_ms);
            let never: u32 = 0;
            WinHttpSetOption(r, WINHTTP_OPTION_REDIRECT_POLICY, &never as *const u32 as *const c_void, 4);
            let dis: u32 = WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_REDIRECTS | if req.forbid_reuse { WINHTTP_DISABLE_KEEP_ALIVE } else { 0 };
            WinHttpSetOption(r, WINHTTP_OPTION_DISABLE_FEATURE, &dis as *const u32 as *const c_void, 4);
            let tls_on_wire = https && !request_url(r).to_ascii_lowercase().starts_with("http:");
            let mut extra_headers: Vec<String> = Vec::new();
            if let Some(p) = native_tunnel {

                for (opt, v) in [(WINHTTP_OPTION_PROXY_USERNAME, &p.user), (WINHTTP_OPTION_PROXY_PASSWORD, &p.password)] {
                    if let Some(v) = v {
                        let wv = wide(v);
                        WinHttpSetOption(r, opt, wv.as_ptr() as *const c_void, (wv.len() - 1) as u32);
                    }
                }
                if let Some(a) = &p.auth {
                    extra_headers.push(format!("Proxy-Authorization: {a}"));
                }
                extra_headers.extend(p.headers.iter().cloned());
            }
            if tls_on_wire {
                let mut sf: u32 = 0;
                if !req.verify_peer {
                    sf |= SECURITY_FLAG_IGNORE_ALL;
                } else {
                    if !req.verify_host {
                        sf |= SECURITY_FLAG_IGNORE_CERT_CN_INVALID;
                    }
                    if req.tls.ca.is_some() {
                        sf |= SECURITY_FLAG_IGNORE_UNKNOWN_CA;
                    }
                }
                if sf != 0 {
                    WinHttpSetOption(r, WINHTTP_OPTION_SECURITY_FLAGS, &sf as *const u32 as *const c_void, 4);
                }
                if req.tls.revocation && req.verify_peer {
                    let f: u32 = WINHTTP_ENABLE_SSL_REVOCATION;
                    WinHttpSetOption(r, WINHTTP_OPTION_ENABLE_FEATURE, &f as *const u32 as *const c_void, 4);
                }
                if let Some(c) = &req.tls.client {
                    if WinHttpSetOption(r, WINHTTP_OPTION_CLIENT_CERT_CONTEXT, c.ptr(), 40) == 0 {
                        let e = GetLastError();
                        return Err((CURLE_SSL_CERTPROBLEM, e, format!("unable to set client certificate: {}", winhttp_message(e))));
                    }
                }
            } else if https {
                if let Some(c) = &req.tls.client {
                    extra_headers.push(format!("X-TomBox-Client-Cert: sha1={}", c.thumbprint()));
                }
            }
            if req.http2 {
                let f: u32 = WINHTTP_PROTOCOL_FLAG_HTTP2;
                WinHttpSetOption(r, WINHTTP_OPTION_ENABLE_HTTP_PROTOCOL, &f as *const u32 as *const c_void, 4);
            }
            if req.decompress != 0 {
                WinHttpSetOption(r, WINHTTP_OPTION_DECOMPRESSION, &req.decompress as *const u32 as *const c_void, 4);
            }
            let hdrs = req.headers.iter().chain(extra_headers.iter()).cloned().collect::<Vec<_>>().join("\r\n");
            let whdrs = wide(&hdrs);
            let (hp, hl) = if hdrs.is_empty() { (core::ptr::null(), 0) } else { (whdrs.as_ptr(), u32::MAX) };
            let (bp, bl) = if body.is_empty() { (core::ptr::null(), 0u32) } else { (body.as_ptr() as *const c_void, body.len() as u32) };
            let mut sent = WinHttpSendRequest(r, hp, hl, bp, bl, bl, 0) != 0;
            if !sent && GetLastError() == 12044 && req.tls.client.is_none() {
                WinHttpSetOption(r, WINHTTP_OPTION_CLIENT_CERT_CONTEXT, core::ptr::null(), 0);
                sent = WinHttpSendRequest(r, hp, hl, bp, bl, bl, 0) != 0;
            }
            if !sent {
                let e = GetLastError();
                if sh.cancelled() {
                    return Err((CURLE_ABORTED_BY_CALLBACK, e, "transfer aborted".into()));
                }
                return Err((map_error(e, 0), e, format!("Failed to send request to {host} port {port}: {}", winhttp_message(e))));
            }
            if tls_on_wire && req.verify_peer {
                check_server_ca(r, req)?;
            }
            if WinHttpReceiveResponse(r, core::ptr::null_mut()) == 0 {
                let e = GetLastError();
                if sh.cancelled() {
                    return Err((CURLE_ABORTED_BY_CALLBACK, e, "transfer aborted".into()));
                }
                let code = if e == 12152 || e == 12030 { CURLE_GOT_NOTHING } else { map_error(e, 2) };
                return Err((code, e, format!("Failed to receive response: {}", winhttp_message(e))));
            }
            let mut status: u32 = 0;
            let mut sl: u32 = 4;
            WinHttpQueryHeaders(r, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, core::ptr::null(), &mut status as *mut u32 as *mut c_void, &mut sl, core::ptr::null_mut());
            let raw = query_raw_headers(r);
            let location = header_value(&raw, "location");
            keep.set(version_of(&raw) != 1 && !header_value(&raw, "connection").map_or(false, |v| v.to_ascii_lowercase().contains("close")));
            let is_redirect = req.follow && matches!(status, 301 | 302 | 303 | 307 | 308) && location.is_some();
            let mut eff = url.clone();
            eff.fragment = None;
            sh.push(Ev::Response {
                status: status as i32,
                version: version_of(&raw),
                raw,
                url: eff.compose(0),
                redirect: is_redirect,
                primary_ip: host.clone(),
                port,
                reused,
            });
            if is_redirect {
                return Ok(location.map(|l| (l, status)));
            }
            if req.nobody || method.eq_ignore_ascii_case("HEAD") {
                return Ok(None);
            }
            loop {
                if sh.cancelled() {
                    return Err((CURLE_ABORTED_BY_CALLBACK, 0, "transfer aborted".into()));
                }
                let mut avail: u32 = 0;
                if WinHttpQueryDataAvailable(r, &mut avail) == 0 {
                    let e = GetLastError();
                    return Err((map_error(e, 2), e, format!("Failure when receiving data: {}", winhttp_message(e))));
                }
                if avail == 0 {
                    break;
                }
                let mut buf = vec![0u8; (avail as usize).min(1 << 20)];
                let mut got: u32 = 0;
                if WinHttpReadData(r, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut got) == 0 {
                    let e = GetLastError();
                    return Err((map_error(e, 2), e, format!("Failure when receiving data: {}", winhttp_message(e))));
                }
                if got == 0 {
                    break;
                }
                buf.truncate(got as usize);
                sh.push(Ev::Data(buf));
            }
            Ok(None)
        })();
        sh.close_req();
        if result.is_ok() && keep.get() && !req.forbid_reuse && !sh.cancelled() {
            req.pool.give(key, conn);
        } else {
            WinHttpCloseHandle(conn);
        }
        match result {
            Err(e) => return e,
            Ok(None) => return (CURLE_OK, 0, String::new()),
            Ok(Some((loc, st))) => {
                redirects += 1;
                if req.maxredirs >= 0 && redirects > req.maxredirs {
                    return (CURLE_TOO_MANY_REDIRECTS, 0, format!("Maximum ({}) redirects followed", req.maxredirs));
                }
                let next = match url.join(&loc) {
                    Ok(n) => n,
                    Err(_) => return (CURLE_URL_MALFORMAT, 0, format!("Bad redirect location '{loc}'")),
                };
                let to_get = match st {
                    303 => !method.eq_ignore_ascii_case("HEAD"),
                    301 => method.eq_ignore_ascii_case("POST") && req.postredir & 1 == 0,
                    302 => method.eq_ignore_ascii_case("POST") && req.postredir & 2 == 0,
                    _ => false,
                };
                if to_get {
                    method = "GET".into();
                    body = &[];
                }
                url = next;
            }
        }
    }
}
