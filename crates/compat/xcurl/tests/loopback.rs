mod common;

use common::*;
use core::ffi::c_void;
use std::ffi::CString;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use xcurl::easy::*;
use xcurl::misc::*;
use xcurl::multi::*;
use xcurl::url::*;

fn port() -> u16 {
    static P: OnceLock<u16> = OnceLock::new();
    *P.get_or_init(|| {
        for k in ["TOMBOX_NET_BACKEND", "TOMBOX_ALLOW_NETWORK", "TOMBOX_NET_LOCAL_PORT", "TOMBOX_NET_GUARD_OWNER"] {
            std::env::remove_var(k);
        }
        std::env::set_var("TOMBOX_NET_REDIRECT", "127.0.0.1");
        std::env::set_var("TOMBOX_LOG", log_file("xcurl_loopback.log"));
        let p = server();
        assert_eq!(curl_global_init(3), 0);
        p
    })
}

fn url(path: &str) -> String {
    format!("http://127.0.0.1:{}{path}", port())
}

unsafe fn set_str(h: *mut c_void, opt: u32, s: &str) -> i32 {
    let c = CString::new(s).unwrap();
    let r = curl_easy_setopt(h, opt, c.as_ptr() as usize);
    drop(c);
    r
}

unsafe fn easy_with_sink(path: &str, sink: &mut Sink) -> *mut c_void {
    let h = curl_easy_init();
    assert!(!h.is_null());
    assert_eq!(set_str(h, CURLOPT_URL, &url(path)), 0);
    assert_eq!(curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize), 0);
    assert_eq!(curl_easy_setopt(h, CURLOPT_WRITEDATA, sink.ptr()), 0);
    assert_eq!(curl_easy_setopt(h, CURLOPT_HEADERFUNCTION, header_cb as *const () as usize), 0);
    assert_eq!(curl_easy_setopt(h, CURLOPT_HEADERDATA, sink.ptr()), 0);
    h
}

unsafe fn long_info(h: *mut c_void, info: u32) -> (i32, u32) {
    let mut v: u64 = 0xAAAA_AAAA_AAAA_AAAA;
    assert_eq!(curl_easy_getinfo(h, info, &mut v as *mut u64 as *mut c_void), 0);
    (v as u32 as i32, (v >> 32) as u32)
}

unsafe fn str_info(h: *mut c_void, info: u32) -> Option<String> {
    let mut p: *const i8 = core::ptr::null();
    assert_eq!(curl_easy_getinfo(h, info, &mut p as *mut *const i8 as *mut c_void), 0);
    if p.is_null() {
        None
    } else {
        Some(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned())
    }
}

#[test]
fn easy_get_with_callbacks_and_infos() {
    port();
    unsafe {
        let mut sink = Sink::new();
        let h = easy_with_sink("/hello?x=1", &mut sink);
        let marker = 0x1234_5678usize;
        assert_eq!(curl_easy_setopt(h, CURLOPT_PRIVATE, marker), 0);
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(sink.body, b"hello world");
        assert!(sink.headers[0].starts_with(b"HTTP/1.1 200"), "{:?}", String::from_utf8_lossy(&sink.headers[0]));
        assert_eq!(sink.headers.last().unwrap(), b"\r\n");
        assert!(sink.headers.iter().all(|l| l.ends_with(b"\r\n")));
        assert_eq!(sink.header("x-test").as_deref(), Some("1"));
        let (code, hi) = long_info(h, CURLINFO_RESPONSE_CODE);
        assert_eq!(code, 200);
        assert_eq!(hi, 0xAAAA_AAAA, "LONG getinfo must write exactly 4 bytes");
        let (ver, _) = long_info(h, CURLINFO_HTTP_VERSION);
        assert_eq!(ver, 2);
        assert_eq!(str_info(h, CURLINFO_CONTENT_TYPE).as_deref(), Some("text/plain"));
        assert_eq!(str_info(h, CURLINFO_EFFECTIVE_URL), Some(url("/hello?x=1")));
        let mut p: usize = 0;
        curl_easy_getinfo(h, CURLINFO_PRIVATE, &mut p as *mut usize as *mut c_void);
        assert_eq!(p, marker);
        let mut d: f64 = 0.0;
        curl_easy_getinfo(h, CURLINFO_SIZE_DOWNLOAD, &mut d as *mut f64 as *mut c_void);
        assert_eq!(d, 11.0);
        let mut n: i64 = 0;
        curl_easy_getinfo(h, CURLINFO_SIZE_DOWNLOAD_T, &mut n as *mut i64 as *mut c_void);
        assert_eq!(n, 11);
        curl_easy_getinfo(h, CURLINFO_CONTENT_LENGTH_DOWNLOAD_T, &mut n as *mut i64 as *mut c_void);
        assert_eq!(n, 11);
        let mut t: f64 = -1.0;
        curl_easy_getinfo(h, CURLINFO_TOTAL_TIME, &mut t as *mut f64 as *mut c_void);
        assert!(t >= 0.0);
        let (os, _) = long_info(h, CURLINFO_OS_ERRNO);
        assert_eq!(os, 0);
        curl_easy_cleanup(h);
    }
}

#[test]
fn options_used_by_xdefiant_are_accepted() {
    port();
    unsafe {
        let h = curl_easy_init();
        for (opt, v) in [
            (13u32, 30usize), (14, 0), (41, 0), (42, 0), (46, 0), (47, 0), (60, 0), (64, 0), (81, 0), (155, 1000),
            (10001, 0), (10009, 0), (10010, 0), (10023, 0), (10029, 0), (20011, 0), (20012, 0), (20079, 0), (20094, 0),
        ] {
            assert_eq!(curl_easy_setopt(h, opt, v), 0, "option {opt}");
        }
        for opt in [CURLOPT_URL, CURLOPT_POSTFIELDS, CURLOPT_SSLCERT, CURLOPT_CUSTOMREQUEST, CURLOPT_SSL_CIPHER_LIST, CURLOPT_SSLKEY] {
            assert_eq!(set_str(h, opt, "x"), 0, "option {opt}");
        }
        let sl = curl_slist_append(core::ptr::null_mut(), b"a:1:127.0.0.1:1\0".as_ptr());
        assert_eq!(curl_easy_setopt(h, CURLOPT_CONNECT_TO, sl as usize), 0);
        curl_easy_cleanup(h);
        curl_slist_free_all(sl);
    }
}

#[test]
fn postfields_strlen_and_binary_and_headers() {
    port();
    unsafe {
        let mut sink = Sink::new();
        let h = easy_with_sink("/echo", &mut sink);
        let body = b"a=1&b=2\0";
        let mut sl = curl_slist_append(core::ptr::null_mut(), b"X-Custom: yes\0".as_ptr());
        sl = curl_slist_append(sl, b"Accept:\0".as_ptr());
        sl = curl_slist_append(sl, b"X-Empty;\0".as_ptr());
        curl_easy_setopt(h, CURLOPT_HTTPHEADER, sl as usize);
        curl_easy_setopt(h, CURLOPT_POSTFIELDS, body.as_ptr() as usize);
        curl_easy_setopt(h, CURLOPT_POSTFIELDSIZE, 0xFFFF_FFFFusize);
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(sink.body, b"a=1&b=2");
        assert_eq!(sink.header("x-method").as_deref(), Some("POST"));
        assert_eq!(sink.header("x-got-content-type").as_deref(), Some("application/x-www-form-urlencoded"));
        assert_eq!(sink.header("x-got-custom").as_deref(), Some("yes"));
        assert_eq!(sink.header("x-got-accept").as_deref(), Some(""));
        assert_eq!(sink.header("x-got-empty").as_deref(), Some("present"));
        let bin = [9u8, 0, 0, 1, 0, 2, 3, 0, 4, 5];
        let mut sink2 = Sink::new();
        curl_easy_setopt(h, CURLOPT_WRITEDATA, sink2.ptr());
        curl_easy_setopt(h, CURLOPT_HEADERDATA, sink2.ptr());
        curl_easy_setopt(h, CURLOPT_POSTFIELDS, bin.as_ptr() as usize);
        curl_easy_setopt(h, CURLOPT_POSTFIELDSIZE, bin.len());
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(sink2.body, bin);
        curl_easy_cleanup(h);
        curl_slist_free_all(sl);
    }
}

#[test]
fn readfunction_post_put_and_custom_verb() {
    port();
    unsafe {
        for (mode, expect) in [("post", "POST"), ("put", "PUT"), ("patch", "PATCH")] {
            let mut sink = Sink::new();
            let h = easy_with_sink("/echo", &mut sink);
            let data: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
            let mut src = Box::new(Source { data: data.clone(), pos: 0, threads: Vec::new() });
            curl_easy_setopt(h, CURLOPT_READFUNCTION, read_cb as *const () as usize);
            curl_easy_setopt(h, CURLOPT_READDATA, &mut *src as *mut Source as usize);
            match mode {
                "post" => {
                    curl_easy_setopt(h, CURLOPT_POST, 1);
                    curl_easy_setopt(h, CURLOPT_POSTFIELDSIZE, data.len());
                }
                "put" => {
                    curl_easy_setopt(h, CURLOPT_UPLOAD, 1);
                    curl_easy_setopt(h, CURLOPT_INFILESIZE, data.len());
                }
                _ => {
                    set_str(h, CURLOPT_CUSTOMREQUEST, "PATCH");
                    curl_easy_setopt(h, CURLOPT_UPLOAD, 1);
                    curl_easy_setopt(h, CURLOPT_INFILESIZE, data.len());
                }
            }
            assert_eq!(curl_easy_perform(h), 0, "{mode}");
            assert_eq!(sink.header("x-method").as_deref(), Some(expect));
            assert_eq!(sink.body.len(), data.len(), "{mode}");
            assert!(sink.body == data, "{mode}");
            let me = std::thread::current().id();
            assert!(src.threads.iter().all(|t| *t == me));
            curl_easy_cleanup(h);
        }
    }
}

#[test]
fn header_option_puts_headers_in_body() {
    port();
    unsafe {
        let mut sink = Sink::new();
        let h = curl_easy_init();
        set_str(h, CURLOPT_URL, &url("/hello"));
        curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
        curl_easy_setopt(h, CURLOPT_WRITEDATA, sink.ptr());
        curl_easy_setopt(h, CURLOPT_HEADER, 1);
        assert_eq!(curl_easy_perform(h), 0);
        let s = String::from_utf8_lossy(&sink.body).into_owned();
        assert!(s.starts_with("HTTP/1.1 200"), "{s}");
        assert!(s.ends_with("\r\n\r\nhello world"), "{s}");
        curl_easy_cleanup(h);
    }
}

#[test]
fn redirects_with_and_without_follow() {
    port();
    unsafe {
        let mut sink = Sink::new();
        let h = easy_with_sink("/redirect", &mut sink);
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(long_info(h, CURLINFO_RESPONSE_CODE).0, 302);
        assert_eq!(sink.body, b"moved");
        assert_eq!(str_info(h, CURLINFO_REDIRECT_URL), Some(url("/hello")));
        let mut sink2 = Sink::new();
        curl_easy_setopt(h, CURLOPT_WRITEDATA, sink2.ptr());
        curl_easy_setopt(h, CURLOPT_HEADERDATA, sink2.ptr());
        curl_easy_setopt(h, CURLOPT_FOLLOWLOCATION, 1);
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(long_info(h, CURLINFO_RESPONSE_CODE).0, 200);
        assert_eq!(sink2.body, b"hello world");
        assert_eq!(long_info(h, CURLINFO_REDIRECT_COUNT).0, 1);
        assert_eq!(str_info(h, CURLINFO_EFFECTIVE_URL), Some(url("/hello")));
        let status_lines = sink2.headers.iter().filter(|l| l.starts_with(b"HTTP/")).count();
        assert_eq!(status_lines, 2);
        set_str(h, CURLOPT_URL, &url("/redirect-post"));
        curl_easy_setopt(h, CURLOPT_POSTFIELDS, b"q=1\0".as_ptr() as usize);
        curl_easy_setopt(h, CURLOPT_POSTFIELDSIZE, 3);
        let mut sink3 = Sink::new();
        curl_easy_setopt(h, CURLOPT_WRITEDATA, sink3.ptr());
        curl_easy_setopt(h, CURLOPT_HEADERDATA, sink3.ptr());
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(sink3.header("x-method").as_deref(), Some("GET"));
        set_str(h, CURLOPT_URL, &url("/loop"));
        curl_easy_setopt(h, CURLOPT_MAXREDIRS, 3);
        assert_eq!(curl_easy_perform(h), 47);
        curl_easy_cleanup(h);
    }
}

#[test]
fn connection_refused_sets_code_and_os_errno() {
    port();
    unsafe {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let p = l.local_addr().unwrap().port();
        drop(l);
        let h = curl_easy_init();
        let mut eb = [0x55u8; 256];
        curl_easy_setopt(h, CURLOPT_ERRORBUFFER, eb.as_mut_ptr() as usize);
        set_str(h, CURLOPT_URL, &format!("http://127.0.0.1:{p}/x"));
        port();
        assert_eq!(curl_easy_perform(h), 7);
        assert_ne!(long_info(h, CURLINFO_OS_ERRNO).0, 0);
        assert_eq!(long_info(h, CURLINFO_RESPONSE_CODE).0, 0);
        let msg = std::ffi::CStr::from_ptr(eb.as_ptr() as *const i8).to_string_lossy().into_owned();
        assert!(!msg.is_empty());
        curl_easy_cleanup(h);
    }
}

#[test]
fn timeout_returns_28_and_fills_errorbuffer() {
    port();
    unsafe {
        let mut sink = Sink::new();
        let h = easy_with_sink("/slow", &mut sink);
        let mut eb = [0x55u8; 256];
        curl_easy_setopt(h, CURLOPT_ERRORBUFFER, eb.as_mut_ptr() as usize);
        curl_easy_setopt(h, CURLOPT_TIMEOUT_MS, 300);
        let t = Instant::now();
        assert_eq!(curl_easy_perform(h), 28);
        assert!(t.elapsed() < Duration::from_millis(1500), "{:?}", t.elapsed());
        let msg = std::ffi::CStr::from_ptr(eb.as_ptr() as *const i8).to_string_lossy().into_owned();
        assert!(msg.contains("timed out"), "{msg}");
        let s = std::ffi::CStr::from_ptr(curl_easy_strerror(28) as *const i8).to_string_lossy().into_owned();
        assert_eq!(s, "Timeout was reached");
        curl_easy_cleanup(h);
    }
}

#[test]
fn failonerror_binary_chunked_big_head() {
    port();
    unsafe {
        let mut sink = Sink::new();
        let h = easy_with_sink("/missing", &mut sink);
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(long_info(h, CURLINFO_RESPONSE_CODE).0, 404);
        assert_eq!(sink.body, b"nope");
        curl_easy_setopt(h, CURLOPT_FAILONERROR, 1);
        assert_eq!(curl_easy_perform(h), 22);
        curl_easy_cleanup(h);

        let mut s = Sink::new();
        let h = easy_with_sink("/binary", &mut s);
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(s.body, [0, 1, 2, 0, 255, 0, 7]);
        curl_easy_cleanup(h);

        let mut s = Sink::new();
        let h = easy_with_sink("/chunked", &mut s);
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(s.body, b"hello world");
        curl_easy_cleanup(h);

        let mut s = Sink::new();
        let h = easy_with_sink("/big", &mut s);
        assert_eq!(curl_easy_perform(h), 0);
        let want: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        assert!(s.body == want);
        curl_easy_cleanup(h);

        let mut s = Sink::new();
        let h = easy_with_sink("/hello", &mut s);
        curl_easy_setopt(h, CURLOPT_NOBODY, 1);
        assert_eq!(curl_easy_perform(h), 0);
        assert!(s.body.is_empty());
        assert_eq!(long_info(h, CURLINFO_RESPONSE_CODE).0, 200);
        curl_easy_cleanup(h);
    }
}

#[test]
fn write_callback_short_count_aborts() {
    unsafe extern "C" fn short(_p: *mut u8, _s: usize, _n: usize, _u: *mut c_void) -> usize {
        0
    }
    port();
    unsafe {
        let h = curl_easy_init();
        set_str(h, CURLOPT_URL, &url("/hello"));
        curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, short as *const () as usize);
        assert_eq!(curl_easy_perform(h), 23);
        curl_easy_cleanup(h);
    }
}

#[test]
fn unknown_hosts_are_redirected_by_net_guard() {
    port();
    unsafe {
        let mut sink = Sink::new();
        let h = curl_easy_init();
        set_str(h, CURLOPT_URL, &format!("http://pdc-public-ubiservices.ubi.invalid:{}/hello", port()));
        curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
        curl_easy_setopt(h, CURLOPT_WRITEDATA, sink.ptr());
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(sink.body, b"hello world");
        curl_easy_cleanup(h);
    }
}

#[test]
fn connect_to_and_basic_auth() {
    port();
    unsafe {
        let mut sink = Sink::new();
        let h = curl_easy_init();
        set_str(h, CURLOPT_URL, "http://user%40x:p%3Ass@service.invalid:1/echo");
        let line = CString::new(format!("service.invalid:1:127.0.0.1:{}", port())).unwrap();
        let sl = curl_slist_append(core::ptr::null_mut(), line.as_ptr() as *const u8);
        curl_easy_setopt(h, CURLOPT_CONNECT_TO, sl as usize);
        curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
        curl_easy_setopt(h, CURLOPT_WRITEDATA, sink.ptr());
        curl_easy_setopt(h, CURLOPT_HEADERFUNCTION, header_cb as *const () as usize);
        curl_easy_setopt(h, CURLOPT_HEADERDATA, sink.ptr());
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(sink.header("x-got-authorization").as_deref(), Some("Basic dXNlckB4OnA6c3M="));
        curl_easy_cleanup(h);
        curl_slist_free_all(sl);
    }
}

#[test]
fn multi_runs_transfers_on_caller_thread() {
    port();
    unsafe {
        port();
        let m = curl_multi_init();
        let paths = ["/hello", "/missing", "/big", "/echo"];
        let mut sinks: Vec<Box<Sink>> = paths.iter().map(|_| Sink::new()).collect();
        let mut handles = Vec::new();
        for (i, p) in paths.iter().enumerate() {
            let h = easy_with_sink(p, &mut sinks[i]);
            if *p == "/echo" {
                curl_easy_setopt(h, CURLOPT_POSTFIELDS, b"multi-body\0".as_ptr() as usize);
            }
            assert_eq!(curl_multi_add_handle(m, h), 0);
            handles.push(h);
        }
        assert_eq!(curl_multi_add_handle(m, handles[0]), 7);
        let mut running = 1;
        let mut done = Vec::new();
        let t = Instant::now();
        while running > 0 || done.len() < paths.len() {
            assert_eq!(curl_multi_perform(m, &mut running), 0);
            loop {
                let mut left = 0;
                let msg = curl_multi_info_read(m, &mut left) as *const u8;
                if msg.is_null() {
                    break;
                }
                assert_eq!(*(msg as *const i32), 1);
                let e = *(msg.add(8) as *const *mut c_void);
                let r = *(msg.add(16) as *const i32);
                done.push((e, r));
            }
            if running > 0 {
                let mut n = 0;
                assert_eq!(curl_multi_poll(m, core::ptr::null_mut(), 0, 1000, &mut n), 0);
            }
            assert!(t.elapsed() < Duration::from_secs(20));
        }
        assert_eq!(done.len(), paths.len());
        assert!(done.iter().all(|(_, r)| *r == 0));
        assert_eq!(long_info(handles[0], CURLINFO_RESPONSE_CODE).0, 200);
        assert_eq!(long_info(handles[1], CURLINFO_RESPONSE_CODE).0, 404);
        assert_eq!(sinks[0].body, b"hello world");
        assert_eq!(sinks[2].body.len(), 300_000);
        assert_eq!(sinks[3].body, b"multi-body");
        let me = std::thread::current().id();
        for s in &sinks {
            assert!(!s.threads.is_empty());
            assert!(s.threads.iter().all(|t| *t == me));
        }
        assert_eq!(curl_multi_remove_handle(m, handles[0]), 0);
        sinks[0].body.clear();
        assert_eq!(curl_multi_add_handle(m, handles[0]), 0);
        let mut got = None;
        let t = Instant::now();
        while got.is_none() {
            curl_multi_perform(m, &mut running);
            let mut left = 0;
            let msg = curl_multi_info_read(m, &mut left) as *const u8;
            if !msg.is_null() {
                got = Some(*(msg.add(16) as *const i32));
            } else {
                let mut n = 0;
                curl_multi_poll(m, core::ptr::null_mut(), 0, 500, &mut n);
            }
            assert!(t.elapsed() < Duration::from_secs(10));
        }
        assert_eq!(got, Some(0));
        assert_eq!(sinks[0].body, b"hello world");
        for h in &handles {
            curl_multi_remove_handle(m, *h);
            curl_easy_cleanup(*h);
        }
        assert_eq!(curl_multi_cleanup(m), 0);
    }
}

#[test]
fn multi_poll_waits_and_wakes() {
    port();
    unsafe {
        port();
        let m = curl_multi_init();
        let t = Instant::now();
        let mut n = -1;
        assert_eq!(curl_multi_poll(m, core::ptr::null_mut(), 0, 300, &mut n), 0);
        assert!(t.elapsed() >= Duration::from_millis(250), "{:?}", t.elapsed());
        assert_eq!(n, 0);
        let mu = m as usize;
        let th = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            curl_multi_wakeup(mu as *mut c_void);
        });
        let t = Instant::now();
        assert_eq!(curl_multi_poll(m, core::ptr::null_mut(), 0, 5000, &mut n), 0);
        assert!(t.elapsed() < Duration::from_millis(2000), "{:?}", t.elapsed());
        th.join().unwrap();
        curl_multi_cleanup(m);
    }
}

#[test]
fn multi_remove_mid_flight_and_timeout() {
    port();
    unsafe {
        let m = curl_multi_init();
        let mut s = Sink::new();
        let h = easy_with_sink("/slow", &mut s);
        curl_multi_add_handle(m, h);
        let mut running = 0;
        curl_multi_perform(m, &mut running);
        assert_eq!(running, 1);
        let t = Instant::now();
        assert_eq!(curl_multi_remove_handle(m, h), 0);
        curl_easy_cleanup(h);
        assert!(t.elapsed() < Duration::from_millis(500));

        let mut s2 = Sink::new();
        let h2 = easy_with_sink("/slow", &mut s2);
        curl_easy_setopt(h2, CURLOPT_TIMEOUT_MS, 300);
        curl_multi_add_handle(m, h2);
        let t = Instant::now();
        let mut result = None;
        while result.is_none() {
            curl_multi_perform(m, &mut running);
            let mut left = 0;
            let msg = curl_multi_info_read(m, &mut left) as *const u8;
            if !msg.is_null() {
                result = Some(*(msg.add(16) as *const i32));
            } else {
                let mut n = 0;
                curl_multi_poll(m, core::ptr::null_mut(), 0, 1000, &mut n);
            }
            assert!(t.elapsed() < Duration::from_secs(5));
        }
        assert_eq!(result, Some(28));
        assert!(t.elapsed() < Duration::from_millis(1500), "{:?}", t.elapsed());
        curl_multi_remove_handle(m, h2);
        curl_easy_cleanup(h2);
        curl_multi_cleanup(m);
    }
}

#[test]
fn slist_escape_date_url_version() {
    port();
    unsafe {
        let mut l = curl_slist_append(core::ptr::null_mut(), b"A: 1\0".as_ptr());
        l = curl_slist_append(l, b"B: 2\0".as_ptr());
        let s = slist_strings(l as usize);
        assert_eq!(s, vec!["A: 1".to_string(), "B: 2".to_string()]);
        curl_slist_free_all(l);

        let e = curl_easy_escape(core::ptr::null_mut(), b"a b&c/\xff\0".as_ptr(), 0);
        assert_eq!(std::ffi::CStr::from_ptr(e as *const i8).to_str().unwrap(), "a%20b%26c%2F%FF");
        curl_free(e as *mut c_void);
        let mut n = 0;
        let u = curl_easy_unescape(core::ptr::null_mut(), b"x%00y%41\0".as_ptr(), 0, &mut n);
        assert_eq!(n, 4);
        assert_eq!(core::slice::from_raw_parts(u, 4), b"x\0yA");
        curl_free(u as *mut c_void);

        assert_eq!(curl_getdate(b"Sun, 06 Nov 1994 08:49:37 GMT\0".as_ptr(), core::ptr::null()), 784111777);
        assert_eq!(curl_getdate(b"Sunday, 06-Nov-94 08:49:37 GMT\0".as_ptr(), core::ptr::null()), 784111777);
        assert_eq!(curl_getdate(b"Sun Nov  6 08:49:37 1994\0".as_ptr(), core::ptr::null()), 784111777);
        assert_eq!(curl_getdate(b"garbage\0".as_ptr(), core::ptr::null()), -1);

        let h = curl_url();
        assert_eq!(curl_url_set(h, 0, b"https://user:pw@Example.com:8443/a/b/c?q=1#frag\0".as_ptr(), 0), 0);
        let get = |part: u32, flags: u32| -> Result<String, i32> {
            let mut p: *mut u8 = core::ptr::null_mut();
            let r = curl_url_get(h, part, &mut p, flags);
            if r != 0 {
                return Err(r);
            }
            let s = std::ffi::CStr::from_ptr(p as *const i8).to_string_lossy().into_owned();
            curl_free(p as *mut c_void);
            Ok(s)
        };
        assert_eq!(get(1, 0).unwrap(), "https");
        assert_eq!(get(5, 0).unwrap(), "Example.com");
        assert_eq!(get(6, 0).unwrap(), "8443");
        assert_eq!(get(7, 0).unwrap(), "/a/b/c");
        assert_eq!(get(8, 0).unwrap(), "q=1");
        assert_eq!(get(9, 0).unwrap(), "frag");
        assert_eq!(get(2, 0).unwrap(), "user");
        assert_eq!(curl_url_set(h, 0, b"../d?z=2\0".as_ptr(), 0), 0);
        assert_eq!(get(0, 0).unwrap(), "https://user:pw@Example.com:8443/a/d?z=2");
        assert_eq!(curl_url_set(h, 8, b"k=v w\0".as_ptr(), 256 | 128), 0);
        assert_eq!(get(8, 0).unwrap(), "z=2&k=v%20w");
        let h2 = curl_url_dup(h);
        assert_eq!(curl_url_set(h2, 6, core::ptr::null(), 0), 0);
        let mut p: *mut u8 = core::ptr::null_mut();
        assert_eq!(curl_url_get(h2, 6, &mut p, 0), CURLUE_NO_PORT);
        assert_eq!(curl_url_get(h2, 6, &mut p, 1), 0);
        assert_eq!(std::ffi::CStr::from_ptr(p as *const i8).to_str().unwrap(), "443");
        curl_free(p as *mut c_void);
        assert_eq!(curl_url_set(h2, 6, b"99999\0".as_ptr(), 0), CURLUE_BAD_PORT_NUMBER);
        curl_url_cleanup(h2);
        curl_url_cleanup(h);

        let v = std::ffi::CStr::from_ptr(curl_version() as *const i8).to_str().unwrap();
        assert!(v.starts_with("2206.4.0.0"));
        let vi = curl_version_info(9) as *const u8;
        assert_eq!(*(vi as *const i32), 5);
        let host = *(vi.add(0x18) as *const *const i8);
        assert_eq!(std::ffi::CStr::from_ptr(host).to_str().unwrap(), "Microsoft GDK");
        assert_eq!(curl_global_sslset(0, core::ptr::null(), core::ptr::null_mut()), 3);
        assert!(curl_share_init().is_null());
        assert_eq!(curl_mime_name(core::ptr::null_mut(), core::ptr::null()), 4);
        assert_eq!(xcurl::misc::easy_strerror(0), "No error");
        let ms = std::ffi::CStr::from_ptr(curl_multi_strerror(7) as *const i8).to_str().unwrap();
        assert!(ms.contains("already added"));
    }
}

#[test]
fn bad_url_and_scheme() {
    port();
    unsafe {
        port();
        let h = curl_easy_init();
        assert_eq!(curl_easy_perform(h), 3);
        set_str(h, CURLOPT_URL, "ftp://127.0.0.1/x");
        assert_eq!(curl_easy_perform(h), 1);
        set_str(h, CURLOPT_URL, &format!("127.0.0.1:{}/hello", port()));
        let mut s = Sink::new();
        curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
        curl_easy_setopt(h, CURLOPT_WRITEDATA, s.ptr());
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(s.body, b"hello world");
        let d = curl_easy_duphandle(h);
        let mut s2 = Sink::new();
        curl_easy_setopt(d, CURLOPT_WRITEDATA, s2.ptr());
        assert_eq!(curl_easy_perform(d), 0);
        assert_eq!(s2.body, b"hello world");
        curl_easy_reset(d);
        assert_eq!(curl_easy_perform(d), 3);
        curl_easy_cleanup(d);
        curl_easy_cleanup(h);
    }
}

#[test]
fn upload_with_postfields_and_msg_survives_remove() {
    port();
    unsafe {
        let mut sink = Sink::new();
        let h = easy_with_sink("/echo", &mut sink);
        let body = b"put-body-xyz";
        curl_easy_setopt(h, CURLOPT_POSTFIELDS, body.as_ptr() as usize);
        curl_easy_setopt(h, CURLOPT_POSTFIELDSIZE, body.len());
        curl_easy_setopt(h, CURLOPT_UPLOAD, 1);
        assert_eq!(curl_easy_perform(h), 0);
        assert_eq!(sink.header("x-method").as_deref(), Some("PUT"));
        assert_eq!(sink.body, body);
        curl_easy_cleanup(h);

        let m = curl_multi_init();
        let mut s2 = Sink::new();
        let h2 = easy_with_sink("/slow", &mut s2);
        curl_easy_setopt(h2, CURLOPT_TIMEOUT_MS, 200);
        curl_multi_add_handle(m, h2);
        let t = Instant::now();
        let mut running = 0;
        loop {
            curl_multi_perform(m, &mut running);
            let mut left = 0;
            let msg = curl_multi_info_read(m, &mut left) as *const u8;
            if !msg.is_null() {
                assert_eq!(curl_multi_remove_handle(m, h2), 0);
                assert_eq!(*(msg.add(8) as *const *mut c_void), h2);
                assert_eq!(*(msg.add(16) as *const i32), 28);
                break;
            }
            let mut n = 0;
            curl_multi_poll(m, core::ptr::null_mut(), 0, 500, &mut n);
            assert!(t.elapsed() < Duration::from_secs(5));
        }
        curl_easy_cleanup(h2);
        curl_multi_cleanup(m);
    }
}
