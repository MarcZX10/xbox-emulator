mod common;

use common::*;
use core::ffi::c_void;
use std::ffi::CString;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use xcurl::easy::*;
use xcurl::misc::*;
use xcurl::multi::*;

const PAUSE: usize = 0x1000_0001;

fn port() -> u16 {
    static P: OnceLock<u16> = OnceLock::new();
    *P.get_or_init(|| {
        for k in ["TOMBOX_NET_BACKEND", "TOMBOX_ALLOW_NETWORK", "TOMBOX_NET_LOCAL_PORT", "TOMBOX_NET_GUARD_OWNER"] {
            std::env::remove_var(k);
        }
        std::env::set_var("TOMBOX_NET_REDIRECT", "127.0.0.1");
        std::env::set_var("TOMBOX_LOG", log_file("xcurl_semantics.log"));
        let p = server();
        assert_eq!(curl_global_init(3), 0);
        p
    })
}

fn serial() -> MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

fn url(path: &str) -> String {
    format!("http://127.0.0.1:{}{path}", port())
}

unsafe fn set_str(h: *mut c_void, opt: u32, s: &str) -> i32 {
    let c = CString::new(s).unwrap();
    curl_easy_setopt(h, opt, c.as_ptr() as usize)
}

struct Chunks {
    body: Vec<u8>,
    max: usize,
    pauses_left: u32,
    paused: u32,
}

unsafe extern "C" fn chunk_cb(p: *mut u8, size: usize, n: usize, ud: *mut c_void) -> usize {
    let c = &mut *(ud as *mut Chunks);
    if c.pauses_left > 0 {
        c.pauses_left -= 1;
        c.paused += 1;
        return PAUSE;
    }
    c.max = c.max.max(size * n);
    c.body.extend_from_slice(core::slice::from_raw_parts(p, size * n));
    size * n
}

fn big() -> Vec<u8> {
    (0..300_000u32).map(|i| (i % 251) as u8).collect()
}

#[test]
fn write_pause_redelivers_and_respects_buffer_size() {
    port();
    let _g = serial();
    unsafe {
        for (bufsize, expect_max) in [(0usize, 16384usize), (2048, 2048)] {
            let mut c = Box::new(Chunks { body: Vec::new(), max: 0, pauses_left: 3, paused: 0 });
            let h = curl_easy_init();
            set_str(h, CURLOPT_URL, &url("/big"));
            curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, chunk_cb as *const () as usize);
            curl_easy_setopt(h, CURLOPT_WRITEDATA, &mut *c as *mut Chunks as usize);
            if bufsize != 0 {
                assert_eq!(curl_easy_setopt(h, 98, bufsize), 0);
            }
            assert_eq!(curl_easy_perform(h), 0);
            assert_eq!(c.paused, 3);
            assert_eq!(c.body, big(), "paused bytes must be redelivered once, in order");
            assert!(c.max <= expect_max && c.max > 0, "max chunk {} > {expect_max}", c.max);
            curl_easy_cleanup(h);
        }
    }
}

struct Dbg {
    kinds: Vec<i32>,
    out: Vec<u8>,
}

unsafe extern "C" fn dbg_cb(_h: *mut c_void, kind: i32, p: *const u8, n: usize, ud: *mut c_void) -> i32 {
    let d = &mut *(ud as *mut Dbg);
    d.kinds.push(kind);
    if kind == 2 {
        d.out.extend_from_slice(core::slice::from_raw_parts(p, n));
    }
    0
}

#[test]
fn debug_callback_sees_request_and_response() {
    port();
    let _g = serial();
    unsafe {
        let mut d = Box::new(Dbg { kinds: Vec::new(), out: Vec::new() });
        let mut sink = Sink::new();
        let h = curl_easy_init();
        set_str(h, CURLOPT_URL, &url("/hello"));
        curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
        curl_easy_setopt(h, CURLOPT_WRITEDATA, sink.ptr());
        assert_eq!(curl_easy_setopt(h, CURLOPT_DEBUGFUNCTION, dbg_cb as *const () as usize), 0);
        assert_eq!(curl_easy_setopt(h, 10095, &mut *d as *mut Dbg as usize), 0);
        assert_eq!(curl_easy_perform(h), 0);
        assert!(d.kinds.is_empty(), "debug callback runs only with CURLOPT_VERBOSE");
        curl_easy_setopt(h, CURLOPT_VERBOSE, 1);
        assert_eq!(curl_easy_perform(h), 0);
        for k in [0, 1, 2, 3] {
            assert!(d.kinds.contains(&k), "missing debug kind {k}: {:?}", d.kinds);
        }
        assert!(String::from_utf8_lossy(&d.out).starts_with("GET /hello HTTP/1.1\r\n"));
        curl_easy_cleanup(h);
    }
}

unsafe extern "C" fn xfer_abort(_u: *mut c_void, _a: i64, _b: i64, _c: i64, _d: i64) -> i32 {
    1
}

#[test]
fn progress_abort_and_maxfilesize() {
    port();
    let _g = serial();
    unsafe {
        let mut sink = Sink::new();
        let h = curl_easy_init();
        set_str(h, CURLOPT_URL, &url("/big"));
        curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
        curl_easy_setopt(h, CURLOPT_WRITEDATA, sink.ptr());
        curl_easy_setopt(h, 20219, xfer_abort as *const () as usize);
        assert_eq!(curl_easy_perform(h), 0, "progress callback is off while NOPROGRESS is 1");
        curl_easy_setopt(h, 43, 0);
        assert_eq!(curl_easy_perform(h), 42);
        curl_easy_setopt(h, 43, 1);
        curl_easy_setopt(h, 114, 1000);
        assert_eq!(curl_easy_perform(h), 63);
        curl_easy_cleanup(h);
    }
}

#[test]
fn unknown_options_are_errors_and_multi_limits_apply() {
    port();
    let _g = serial();
    unsafe {
        let h = curl_easy_init();
        assert_eq!(curl_easy_setopt(h, 99999, 1), 48);
        assert_eq!(curl_easy_setopt(h, 32, 9), 43);
        curl_easy_cleanup(h);
        let m = curl_multi_init();
        assert_eq!(curl_multi_setopt(m, 424242, 1), CURLM_UNKNOWN_OPTION);
        assert_eq!(curl_multi_setopt(m, 13, 1), 0);
        let mut sinks: Vec<Box<Sink>> = (0..3).map(|_| Sink::new()).collect();
        let hs: Vec<*mut c_void> = sinks
            .iter_mut()
            .map(|s| {
                let h = curl_easy_init();
                set_str(h, CURLOPT_URL, &url("/hello"));
                curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
                curl_easy_setopt(h, CURLOPT_WRITEDATA, s.ptr());
                curl_multi_add_handle(m, h);
                h
            })
            .collect();
        let mut peak = 0;
        let mut done = 0;
        let t = Instant::now();
        while done < 3 && t.elapsed() < Duration::from_secs(10) {
            let mut running = 0;
            curl_multi_perform(m, &mut running);
            peak = peak.max(xcurl::gate::active());
            let mut left = 0;
            while !curl_multi_info_read(m, &mut left).is_null() {
                done += 1;
            }
            let mut n = 0;
            curl_multi_poll(m, core::ptr::null_mut(), 0, 100, &mut n);
        }
        assert_eq!(done, 3);
        assert_eq!(peak, 1, "MAX_TOTAL_CONNECTIONS 1 must serialise transfers");
        assert!(sinks.iter().all(|s| s.body == b"hello world"));
        for h in hs {
            curl_multi_remove_handle(m, h);
            curl_easy_cleanup(h);
        }
        curl_multi_cleanup(m);
    }
}

#[test]
fn request_limit_and_suspend_resume() {
    port();
    let _g = serial();
    unsafe {
        xcurl_global_set_request_limit(1);
        let m = curl_multi_init();
        let mut sinks: Vec<Box<Sink>> = (0..2).map(|_| Sink::new()).collect();
        let hs: Vec<*mut c_void> = sinks
            .iter_mut()
            .map(|s| {
                let h = curl_easy_init();
                set_str(h, CURLOPT_URL, &url("/hello"));
                curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
                curl_easy_setopt(h, CURLOPT_WRITEDATA, s.ptr());
                curl_multi_add_handle(m, h);
                h
            })
            .collect();
        let mut peak = 0;
        let mut done = 0;
        let t = Instant::now();
        while done < 2 && t.elapsed() < Duration::from_secs(10) {
            let mut running = 0;
            curl_multi_perform(m, &mut running);
            peak = peak.max(xcurl::gate::active());
            let mut left = 0;
            while !curl_multi_info_read(m, &mut left).is_null() {
                done += 1;
            }
            let mut n = 0;
            curl_multi_poll(m, core::ptr::null_mut(), 0, 100, &mut n);
        }
        assert_eq!((done, peak), (2, 1));
        xcurl_global_set_request_limit(0);

        assert_eq!(xcurl_global_suspend(), 0);
        let e = curl_easy_init();
        set_str(e, CURLOPT_URL, &url("/hello"));
        let mut s = Sink::new();
        curl_easy_setopt(e, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
        curl_easy_setopt(e, CURLOPT_WRITEDATA, s.ptr());
        curl_easy_setopt(e, CURLOPT_TIMEOUT_MS, 300);
        assert_eq!(curl_easy_perform(e), 28, "no transfer may start while suspended");
        curl_easy_setopt(e, CURLOPT_TIMEOUT_MS, 0);

        let h = hs[0];
        curl_multi_remove_handle(m, h);
        sinks[0].body.clear();
        curl_multi_add_handle(m, h);
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(400) {
            let mut running = 0;
            curl_multi_perform(m, &mut running);
            assert_eq!(running, 1);
            let mut left = 0;
            assert!(curl_multi_info_read(m, &mut left).is_null(), "nothing completes while suspended");
            let mut n = 0;
            curl_multi_poll(m, core::ptr::null_mut(), 0, 50, &mut n);
        }
        assert!(sinks[0].body.is_empty());
        let eu = e as usize;
        let th = std::thread::spawn(move || curl_easy_perform(eu as *mut c_void));
        std::thread::sleep(Duration::from_millis(150));
        assert!(!th.is_finished());
        assert_eq!(xcurl_global_resume(), 0);
        assert_eq!(th.join().unwrap(), 0);
        assert_eq!(s.body, b"hello world");
        let t = Instant::now();
        let mut ok = false;
        while t.elapsed() < Duration::from_secs(10) && !ok {
            let mut running = 0;
            curl_multi_perform(m, &mut running);
            let mut left = 0;
            ok = !curl_multi_info_read(m, &mut left).is_null();
            let mut n = 0;
            curl_multi_poll(m, core::ptr::null_mut(), 0, 100, &mut n);
        }
        assert!(ok && sinks[0].body == b"hello world");
        curl_easy_cleanup(e);
        for h in hs {
            curl_multi_remove_handle(m, h);
            curl_easy_cleanup(h);
        }
        curl_multi_cleanup(m);
    }
}

#[test]
fn tls_settings_are_validated_before_connecting() {
    port();
    let _g = serial();
    unsafe {
        let https = format!("https://127.0.0.1:{}/hello", port());
        let mut eb = vec![0u8; 256];
        let h = curl_easy_init();
        set_str(h, CURLOPT_URL, &https);
        curl_easy_setopt(h, CURLOPT_ERRORBUFFER, eb.as_mut_ptr() as usize);
        set_str(h, CURLOPT_SSLCERT, "Z:\\tombox-missing\\client.pem");
        assert_eq!(curl_easy_perform(h), 58);
        assert!(String::from_utf8_lossy(&eb).contains("client certificate"));
        curl_easy_setopt(h, CURLOPT_SSLCERT, 0);
        curl_easy_setopt(h, 32, 3);
        assert_eq!(curl_easy_perform(h), 35);
        curl_easy_setopt(h, 32, 0);
        set_str(h, CURLOPT_SSL_CIPHER_LIST, "not a cipher!!");
        assert_eq!(curl_easy_perform(h), 59);
        curl_easy_setopt(h, CURLOPT_SSL_CIPHER_LIST, 0);
        set_str(h, 10065, "Z:\\tombox-missing\\ca.pem");
        assert_eq!(curl_easy_perform(h), 77);
        curl_easy_setopt(h, 10065, 0);
        set_str(h, CURLOPT_URL, &url("/hello"));
        set_str(h, CURLOPT_SSLCERT, "Z:\\tombox-missing\\client.pem");
        let mut s = Sink::new();
        curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
        curl_easy_setopt(h, CURLOPT_WRITEDATA, s.ptr());
        assert_eq!(curl_easy_perform(h), 0, "TLS settings only apply to https");
        curl_easy_cleanup(h);
    }
}

struct Keep {
    port: u16,
    accepted: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

fn keep_server() -> &'static Keep {
    use std::io::{BufRead, BufReader, Read, Write};
    static K: OnceLock<Keep> = OnceLock::new();
    K.get_or_init(|| {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let accepted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let a = accepted.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                a.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::spawn(move || {
                    let mut w = s.try_clone().unwrap();
                    let mut r = BufReader::new(s);
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        let target = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                        let mut auth = String::new();
                        let mut host = String::new();
                        let mut len = 0usize;
                        loop {
                            let mut h = String::new();
                            if r.read_line(&mut h).unwrap_or(0) == 0 {
                                return;
                            }
                            let h = h.trim_end().to_string();
                            if h.is_empty() {
                                break;
                            }
                            if let Some((k, v)) = h.split_once(':') {
                                match k.trim().to_ascii_lowercase().as_str() {
                                    "proxy-authorization" => auth = v.trim().to_string(),
                                    "host" => host = v.trim().to_string(),
                                    "content-length" => len = v.trim().parse().unwrap_or(0),
                                    _ => {}
                                }
                            }
                        }
                        let mut body = vec![0u8; len];
                        let _ = r.read_exact(&mut body);
                        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-Target: {target}\r\nX-Proxy-Auth: {auth}\r\nX-Host: {host}\r\n\r\nok");
                        if w.write_all(resp.as_bytes()).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        Keep { port, accepted }
    })
}

unsafe fn get_keep(h: *mut c_void, url: &str, sink: &mut Sink) -> i32 {
    set_str(h, CURLOPT_URL, url);
    curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
    curl_easy_setopt(h, CURLOPT_WRITEDATA, sink.ptr());
    curl_easy_setopt(h, CURLOPT_HEADERFUNCTION, header_cb as *const () as usize);
    curl_easy_setopt(h, CURLOPT_HEADERDATA, sink.ptr());
    curl_easy_perform(h)
}

unsafe fn num_connects(h: *mut c_void) -> i32 {
    let mut v: i32 = -1;
    curl_easy_getinfo(h, 0x20001a, &mut v as *mut i32 as *mut c_void);
    v
}

#[test]
fn connections_are_pooled_and_forbid_reuse_closes_them() {
    port();
    let _g = serial();
    let k = keep_server();
    unsafe {
        let url = format!("http://127.0.0.1:{}/pool", k.port);
        let before = k.accepted.load(std::sync::atomic::Ordering::SeqCst);
        let h = curl_easy_init();
        let mut s = Sink::new();
        assert_eq!(get_keep(h, &url, &mut s), 0);
        let first = num_connects(h);
        for _ in 0..3 {
            let mut s = Sink::new();
            assert_eq!(get_keep(h, &url, &mut s), 0);
            assert_eq!(s.body, b"ok");
            assert_eq!(num_connects(h), 0, "a pooled connection is not a new connect; pool stats {:?} idle {}", xcurl::net::Pool::global().stats(), xcurl::net::Pool::global().idle());
        }
        assert_eq!(first, 1);
        assert_eq!(k.accepted.load(std::sync::atomic::Ordering::SeqCst) - before, 1, "four transfers must share one socket");
        curl_easy_setopt(h, 75, 1);
        let mut s = Sink::new();
        assert_eq!(get_keep(h, &url, &mut s), 0);
        assert_eq!(num_connects(h), 0, "FORBID_REUSE still uses the cached connection, then closes it");
        for _ in 0..2 {
            let mut s = Sink::new();
            assert_eq!(get_keep(h, &url, &mut s), 0);
            assert_eq!(num_connects(h), 1, "FORBID_REUSE closed the previous connection");
        }
        assert_eq!(k.accepted.load(std::sync::atomic::Ordering::SeqCst) - before, 3);
        curl_easy_cleanup(h);
    }
}

#[test]
fn http_proxy_forwarding_noproxy_and_socks() {
    port();
    let _g = serial();
    let k = keep_server();
    unsafe {
        let h = curl_easy_init();
        set_str(h, 10004, &format!("http://user:pa%3Ass@127.0.0.1:{}", k.port));
        let mut eb = vec![0u8; 256];
        curl_easy_setopt(h, CURLOPT_ERRORBUFFER, eb.as_mut_ptr() as usize);
        let mut s = Sink::new();
        let rc = get_keep(h, "http://origin.example.invalid:8080/a?b=1", &mut s);
        assert_eq!(rc, 0, "{}", String::from_utf8_lossy(&eb).trim_end_matches('\0'));
        let target = s.header("x-target").unwrap_or_default();
        assert!(target.starts_with("http://") && target.ends_with(":8080/a?b=1"), "absolute-form through the proxy: {target}");
        assert_eq!(s.header("x-proxy-auth").as_deref(), Some("Basic dXNlcjpwYTpzcw=="));
        set_str(h, 10004, "http://127.0.0.1:1");
        set_str(h, 10177, "localhost, 127.0.0.1");
        curl_easy_setopt(h, 75, 1);
        let mut s = Sink::new();
        assert_eq!(get_keep(h, &format!("http://127.0.0.1:{}/direct", k.port), &mut s), 0);
        assert_eq!(s.header("x-target").as_deref(), Some("/direct"));
        curl_easy_setopt(h, 10177, 0);
        set_str(h, 10004, "socks5://127.0.0.1:1080");
        let mut s = Sink::new();
        assert_eq!(get_keep(h, "http://origin.example.invalid/", &mut s), 4);
        assert_eq!(curl_easy_setopt(h, 101, 3), 43);
        curl_easy_cleanup(h);
    }
}