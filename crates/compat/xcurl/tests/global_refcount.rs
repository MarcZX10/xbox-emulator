mod common;

use common::*;
use std::ffi::CString;
use xcurl::easy::*;
use xcurl::global::{init_count, CURLE_BAD_FUNCTION_ARGUMENT, CURLE_FAILED_INIT};
use xcurl::misc::*;
use xcurl::net::Pool;

fn keep_alive_server() -> u16 {
    use std::io::{BufRead, BufReader, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut w = s.try_clone().unwrap();
                let mut r = BufReader::new(s);
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    loop {
                        let mut h = String::new();
                        if r.read_line(&mut h).unwrap_or(0) == 0 {
                            return;
                        }
                        if h.trim_end().is_empty() {
                            break;
                        }
                    }
                    if w.write_all(b"HTTP/1.1 200 OK
Content-Length: 11
Connection: keep-alive

hello world").is_err() {
                        return;
                    }
                }
            });
        }
    });
    port
}

unsafe fn get(port: u16) {
    let mut sink = Sink::new();
    let h = curl_easy_init();
    assert!(!h.is_null());
    let u = CString::new(format!("http://127.0.0.1:{port}/hello")).unwrap();
    assert_eq!(curl_easy_setopt(h, CURLOPT_URL, u.as_ptr() as usize), 0);
    assert_eq!(curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize), 0);
    assert_eq!(curl_easy_setopt(h, CURLOPT_WRITEDATA, sink.ptr()), 0);
    assert_eq!(curl_easy_perform(h), 0);
    assert_eq!(sink.body, b"hello world");
    curl_easy_cleanup(h);
}

#[test]
fn global_init_and_cleanup_are_reference_counted_like_console() {
    for k in ["TOMBOX_NET_BACKEND", "TOMBOX_ALLOW_NETWORK", "TOMBOX_NET_LOCAL_PORT", "TOMBOX_NET_GUARD_OWNER"] {
        std::env::remove_var(k);
    }
    std::env::set_var("TOMBOX_NET_REDIRECT", "127.0.0.1");
    std::env::set_var("TOMBOX_LOG", log_file("xcurl_global_refcount.log"));
    let port = keep_alive_server();
    let cb = [1usize, 2, 3, 4, 5];
    let f = |n: [usize; 5]| curl_global_init_mem(3, n[0], n[1], n[2], n[3], n[4]);
    let xf = |n: [usize; 5]| xcurl_global_init_mem(3, n[0], n[1], n[2], n[3], n[4]);
    unsafe {
        assert_eq!(init_count(), 0);
        curl_global_cleanup();
        assert_eq!(init_count(), 0);
        for i in 0..5 {
            let mut bad = cb;
            bad[i] = 0;
            assert_eq!(f(bad), CURLE_BAD_FUNCTION_ARGUMENT);
            assert_eq!(xf(bad), CURLE_BAD_FUNCTION_ARGUMENT);
            assert_eq!(init_count(), 0);
        }
        assert_eq!(curl_global_init(3), 0);
        assert_eq!(curl_global_init(0), 0);
        assert_eq!(init_count(), 2);
        assert_eq!(f([0; 5]), CURLE_FAILED_INIT);
        assert_eq!(xf(cb), CURLE_FAILED_INIT);
        assert_eq!(init_count(), 4);
        get(port);
        assert!(Pool::global().idle() >= 1, "keep-alive connection is pooled");
        for left in [3, 2, 1] {
            curl_global_cleanup();
            assert_eq!(init_count(), left);
            assert!(Pool::global().idle() >= 1, "not the last reference: pool kept");
        }
        curl_global_cleanup();
        assert_eq!(init_count(), 0);
        assert_eq!(Pool::global().idle(), 0, "last cleanup tears the connection cache down");
        curl_global_cleanup();
        assert_eq!(init_count(), 0);
        let h = curl_easy_init();
        assert!(!h.is_null());
        assert_eq!(init_count(), 1, "curl_easy_init runs curl_global_init(3) when nothing is initialised");
        curl_easy_cleanup(h);
        let h2 = curl_easy_init();
        assert_eq!(init_count(), 1, "no extra reference when already initialised");
        curl_easy_cleanup(h2);
        curl_global_cleanup();
        assert_eq!(f(cb), 0);
        assert_eq!(init_count(), 1);
        get(port);
        curl_global_cleanup();
        assert_eq!((init_count(), Pool::global().idle()), (0, 0));
        assert_eq!(xf(cb), 0);
        curl_global_cleanup();
    }
}
