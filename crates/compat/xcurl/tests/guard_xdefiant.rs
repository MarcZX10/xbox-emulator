mod common;

use common::*;
use core::ffi::c_void;
use std::ffi::CString;
use xcurl::easy::*;
use xcurl::misc::*;

unsafe fn call(method: &str, url: &str, body: Option<&[u8]>) -> (i32, i32, serde_json::Value) {
    let mut sink = Sink::new();
    let h = curl_easy_init();
    let u = CString::new(url).unwrap();
    curl_easy_setopt(h, CURLOPT_URL, u.as_ptr() as usize);
    drop(u);
    let mut sl = curl_slist_append(core::ptr::null_mut(), b"Ubi-AppId: 00000000-0000-0000-0000-000000000000\0".as_ptr());
    sl = curl_slist_append(sl, b"Authorization: xbl3.0 x=1;local\0".as_ptr());
    sl = curl_slist_append(sl, b"Content-Type: application/json\0".as_ptr());
    curl_easy_setopt(h, CURLOPT_HTTPHEADER, sl as usize);
    let m = CString::new(method).unwrap();
    curl_easy_setopt(h, CURLOPT_CUSTOMREQUEST, m.as_ptr() as usize);
    if let Some(b) = body {
        curl_easy_setopt(h, CURLOPT_POSTFIELDS, b.as_ptr() as usize);
        curl_easy_setopt(h, CURLOPT_POSTFIELDSIZE, b.len());
    }
    curl_easy_setopt(h, CURLOPT_WRITEFUNCTION, write_cb as *const () as usize);
    curl_easy_setopt(h, CURLOPT_WRITEDATA, sink.ptr());
    curl_easy_setopt(h, CURLOPT_TIMEOUT, 10);
    let r = curl_easy_perform(h);
    let mut code: i32 = 0;
    curl_easy_getinfo(h, CURLINFO_RESPONSE_CODE, &mut code as *mut i32 as *mut c_void);
    curl_easy_cleanup(h);
    curl_slist_free_all(sl);
    (r, code, serde_json::from_slice(&sink.body).unwrap_or(serde_json::Value::Null))
}

#[test]
fn xdefiant_login_flow_runs_against_the_local_ubiservices_emulator() {
    for k in ["TOMBOX_NET_REDIRECT", "TOMBOX_ALLOW_NETWORK", "TOMBOX_NET_LOCAL_PORT", "TOMBOX_NET_GUARD_OWNER", "TOMBOX_ONLINE_NAME"] {
        std::env::remove_var(k);
    }
    std::env::set_var("TOMBOX_NET_BACKEND", "builtin");
    std::env::set_var("TOMBOX_NET_TITLE", "xdefiant");
    std::env::set_var("TOMBOX_LOG", log_file("xcurl_guard_xdefiant.log"));
    unsafe {
        assert_eq!(curl_global_init(3), 0);
        let base = "https://public-ubiservices.ubi.invalid";
        let (r, code, info) = call("GET", &format!("{base}/tombox/request-info"), None);
        assert_eq!((r, code), (0, 200));
        assert_eq!(info["forwardedHost"], "public-ubiservices.ubi.invalid");
        let (r, code, cfg) = call("GET", &format!("{base}/v1/applications/00000000-0000-0000-0000-000000000000/configuration"), None);
        assert_eq!((r, code), (0, 200));
        assert_eq!(cfg["parameters"]["us-sdkClientFeaturesSwitches"]["fields"]["CreateSession"], true);
        let (r, code, s) = call("POST", &format!("{base}/v3/profiles/sessions"), Some(b"{}"));
        assert_eq!((r, code), (0, 200));
        assert_eq!(s["nameOnPlatform"], "XboxEmulator");
        let pid = s["profileId"].as_str().unwrap().to_string();
        let (_, _, p) = call("GET", &format!("{base}/v3/profiles?profileIds={pid}"), None);
        assert_eq!(p["profiles"][0]["profileId"], pid.as_str());
        let (_, code, e) = call("PUT", &format!("{base}/v3/profiles/sessions"), Some(b"{}"));
        assert_eq!(code, 200);
        assert_eq!(e["sessionId"], s["sessionId"]);
        let (r, code, _) = call("DELETE", &format!("{base}/v3/profiles/sessions"), None);
        assert_eq!((r, code), (0, 200));
    }
}
