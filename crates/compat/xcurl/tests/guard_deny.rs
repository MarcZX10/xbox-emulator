mod common;

use common::*;
use core::ffi::c_void;
use std::ffi::CString;
use std::time::{Duration, Instant};
use xcurl::easy::*;
use xcurl::misc::*;

#[test]
fn default_policy_blocks_every_transfer_locally() {
    for k in ["TOMBOX_NET_REDIRECT", "TOMBOX_ALLOW_NETWORK", "TOMBOX_NET_LOCAL_PORT", "TOMBOX_NET_GUARD_OWNER", "TOMBOX_NET_BACKEND"] {
        std::env::remove_var(k);
    }
    std::env::set_var("TOMBOX_LOG", log_file("xcurl_guard_deny.log"));
    unsafe {
        assert_eq!(curl_global_init(3), 0);
        for u in ["https://public-ubiservices.ubi.invalid/v3/profiles/sessions", "http://192.0.2.1/x"] {
            let h = curl_easy_init();
            let c = CString::new(u).unwrap();
            curl_easy_setopt(h, CURLOPT_URL, c.as_ptr() as usize);
            let t = Instant::now();
            let r = curl_easy_perform(h);
            assert_eq!(r, 7, "{u}");
            assert!(t.elapsed() < Duration::from_millis(500), "{u} must fail locally");
            let mut e: i32 = 0;
            curl_easy_getinfo(h, CURLINFO_OS_ERRNO, &mut e as *mut i32 as *mut c_void);
            assert_eq!(e, 12029);
            curl_easy_cleanup(h);
        }
    }
}
