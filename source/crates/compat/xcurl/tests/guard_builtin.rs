mod common;

use common::*;
use core::ffi::c_void;
use std::ffi::CString;
use xcurl::easy::*;
use xcurl::misc::*;

#[test]
fn https_to_any_host_lands_in_the_in_process_online_emulator() {
    for k in ["TOMBOX_NET_REDIRECT", "TOMBOX_ALLOW_NETWORK", "TOMBOX_NET_LOCAL_PORT", "TOMBOX_NET_GUARD_OWNER", "TOMBOX_NET_TITLE"] {
        std::env::remove_var(k);
    }
    std::env::set_var("TOMBOX_NET_BACKEND", "builtin");
    std::env::set_var("TOMBOX_LOG", log_file("xcurl_guard_builtin.log"));
    unsafe {
        assert_eq!(curl_global_init(3), 0);
        let mut sink = Sink::new();
        let h = curl_easy_init();
        let u = CString::new("https://public-ubiservices.ubi.invalid/xcurl/probe?x=1").unwrap();
        curl_easy_setopt(h, CURLOPT_URL, u.as_ptr() as usize);
        curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
        curl_easy_setopt(h, CURLOPT_WRITEDATA, sink.ptr());
        curl_easy_setopt(h, CURLOPT_HEADERFUNCTION, header_cb as *const () as usize);
        curl_easy_setopt(h, CURLOPT_HEADERDATA, sink.ptr());
        curl_easy_setopt(h, CURLOPT_TIMEOUT, 10);
        let r = curl_easy_perform(h);
        let body = String::from_utf8_lossy(&sink.body).into_owned();
        assert_eq!(r, 0, "body={body}");
        let mut code: i32 = 0;
        curl_easy_getinfo(h, CURLINFO_RESPONSE_CODE, &mut code as *mut i32 as *mut c_void);
        assert_eq!(code, 404, "{body}");
        assert!(body.contains("/xcurl/probe"), "{body}");
        curl_easy_cleanup(h);
        let _ = curl_easy_strerror(0);
    }
}
