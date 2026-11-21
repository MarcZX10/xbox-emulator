#![allow(clippy::missing_safety_doc)]

pub mod easy;
pub mod gate;
pub mod global;
pub mod misc;
pub mod tls;
pub mod multi;
pub mod net;
pub mod sys;
pub mod url;

pub fn guard() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        compat_common::record("xcurl", "init", "XCurl 2206.4.0.0 compat layer on WinHTTP; network guard installed before first transfer");
    });
    compat_common::net_guard::ensure();
}
