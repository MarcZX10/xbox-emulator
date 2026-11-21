//! Redirect-mode policy test. Runs offline: the redirect host is `localhost`, so a
//! redirected lookup must come back as loopback without the requested name ever being
//! resolved, and a connect to any other literal address must be refused locally.
use std::net::{TcpListener, TcpStream, ToSocketAddrs};

#[test]
fn redirect_mode_rewrites_names_and_refuses_other_addresses() {
    std::env::set_var("TOMBOX_NET_REDIRECT", "localhost");
    std::env::set_var("TOMBOX_LOG", std::env::temp_dir().join("tombox_net_test.log"));
    compat_common::record("test", "net_redirect", "install");

    // 1. Any host name resolves to the redirect host (loopback here).
    let addrs: Vec<_> = ("some-production-host.example.com", 443).to_socket_addrs().expect("redirected lookup").collect();
    assert!(!addrs.is_empty());
    assert!(addrs.iter().all(|a| a.ip().is_loopback()), "got {addrs:?}");

    // 1b. The backend host families the project owner listed (exact and as suffixes).
    for h in [
        "ol.epicgames.com", "account-public-service-prod.ol.epicgames.com", "ol.epicgames.net",
        "on.epicgames.com", "game-social.epicgames.com", "ak.epicgames.com", "epicgames.dev",
        "api.epicgames.dev", "superawesome.com", "cdn2.unrealengine.com", "x.akamaized.net",
    ] {
        let a: Vec<_> = (h, 443).to_socket_addrs().unwrap_or_else(|e| panic!("{h}: {e}")).collect();
        assert!(!a.is_empty() && a.iter().all(|x| x.ip().is_loopback()), "{h} -> {a:?}");
    }

    // 2. Connecting to the redirect host's address works.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    TcpStream::connect(("127.0.0.1", port)).expect("loopback connect allowed");

    // 3. A literal non-redirect address is refused locally (TEST-NET-1, never routed).
    let t = std::time::Instant::now();
    let e = TcpStream::connect("192.0.2.1:443").expect_err("must be refused");
    assert_eq!(e.raw_os_error(), Some(10051), "{e:?}");
    assert!(t.elapsed().as_millis() < 500, "refusal must be local, not a network timeout");
}
