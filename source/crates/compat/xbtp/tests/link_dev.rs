use core::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use xbtp::link::*;
use xbtp::link_dev;
use xbtp::sys::*;

struct P(*mut u8);
unsafe impl Send for P {}

fn sockaddr(port: u16) -> [u8; 16] {
    let mut a = [0u8; 16];
    a[0] = 2;
    a[2..4].copy_from_slice(&port.to_be_bytes());
    a[4..8].copy_from_slice(&[127, 0, 0, 1]);
    a
}

unsafe fn listener(port: u16) -> (*mut u8, HANDLE) {
    let h = XbtpLinkCreateHandle();
    assert!(!h.is_null());
    let mut ready: HANDLE = null_mut();
    assert_eq!(XbtpLinkListen(h, null_mut(), port, &mut ready), S_OK);
    assert!(!ready.is_null());
    (h, ready)
}

fn wait_until(f: impl Fn() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn listen_blocks_without_a_peer_and_wakes_on_shutdown() {
    unsafe {
        let (h, ready) = listener(0x1080);
        assert!(link_dev::listening(0x1080));
        assert!(wait_until(|| link_dev::pending_recvs(0x1080) == 1), "accept thread is not parked on the device");
        assert_eq!(WaitForSingleObject(ready, 300), 0x102);
        assert_eq!(link_dev::pending_recvs(0x1080), 1);
        let t = Instant::now();
        XbtpLinkCloseHandle(h);
        assert!(t.elapsed() < Duration::from_secs(2), "shutdown did not wake the accept thread");
        assert!(!link_dev::listening(0x1080));
    }
}

#[test]
fn in_process_peer_connects_accepts_and_exchanges_data() {
    unsafe {
        let (l, _ready) = listener(0x1081);
        let peer = std::thread::spawn(|| {
            let c = XbtpLinkCreateHandle();
            let a = sockaddr(0x1081);
            let hr = XbtpLinkConnect(c, a.as_ptr(), 16, 5000);
            let mut msg = *b"pix hello";
            let b = WSABUF { len: msg.len() as u32, buf: msg.as_mut_ptr() };
            let mut sent = 0u32;
            let s = XbtpLinkSend(c, &b, 1, &mut sent, 0, null_mut());
            (hr, s, P(c))
        });
        let mut nh: *mut u8 = null_mut();
        assert_eq!(XbtpLinkAccept(l, &mut nh, 5000), S_OK);
        assert!(!nh.is_null());
        let (hr, s, c) = peer.join().unwrap();
        assert_eq!(hr, S_OK);
        assert_eq!(s, S_OK);
        let mut buf = [0u8; 32];
        let mut got = 0u32;
        assert_eq!(XbtpLinkRecv(nh, buf.as_mut_ptr(), 32, &mut got, 0, null_mut()), S_OK);
        assert_eq!(&buf[..got as usize], b"pix hello");
        assert_eq!(XbtpLinkShutdownHandle(c.0), S_OK);
        assert_eq!(XbtpLinkRecv(nh, buf.as_mut_ptr(), 32, &mut got, 0, null_mut()), S_OK);
        assert_eq!(got, 0);
        XbtpLinkCloseHandle(c.0);
        XbtpLinkCloseHandle(nh);
        XbtpLinkCloseHandle(l);
        assert!(!link_dev::listening(0x1081));
    }
}

#[test]
fn connect_without_listener_is_refused_at_once() {
    unsafe {
        let c = XbtpLinkCreateHandle();
        let a = sockaddr(0x1082);
        let t = Instant::now();
        assert_eq!(XbtpLinkConnect(c, a.as_ptr(), 16, 0), LINK_E_CONNECT_REFUSED);
        assert!(t.elapsed() < Duration::from_millis(500));
        XbtpLinkCloseHandle(c);
    }
}

#[test]
fn peek_keeps_bytes_and_cancel_completes_pending() {
    unsafe {
        let h = link_dev::DEVICE.open;
        let d = h();
        assert!(link_dev::is_in_process(d));
        let mut bind = [0u8; MSG_SIZE];
        bind[..4].copy_from_slice(&MSG_BIND.to_le_bytes());
        bind[0x10..0x14].copy_from_slice(&0x1083u32.to_le_bytes());
        let arr = [WSABUF { len: MSG_SIZE as u32, buf: bind.as_mut_ptr() }];
        let zero = 0u32;
        let mut ret = 0u32;
        let mut ov = OVERLAPPED::default();
        assert_eq!((link_dev::DEVICE.ioctl)(d, IOCTL_SEND, &zero as *const u32 as *const _, 4, arr.as_ptr() as *mut _, 0x10, &mut ret, &mut ov), 1);
        let mut rep = [0u8; MSG_SIZE];
        let mut wb = WSABUF { len: 4, buf: rep.as_mut_ptr() };
        let peek = 2u32;
        assert_eq!((link_dev::DEVICE.ioctl)(d, IOCTL_RECV, &peek as *const u32 as *const _, 4, &mut wb as *mut WSABUF as *mut _, 0x10, &mut ret, &mut ov), 1);
        assert_eq!(ret, 4);
        assert_eq!(u32::from_le_bytes(rep[..4].try_into().unwrap()), MSG_BIND);
        wb.len = MSG_SIZE as u32;
        assert_eq!((link_dev::DEVICE.ioctl)(d, IOCTL_RECV, &zero as *const u32 as *const _, 4, &mut wb as *mut WSABUF as *mut _, 0x10, &mut ret, &mut ov), 1);
        assert_eq!(ret, MSG_SIZE as u32);
        assert_eq!(u32::from_le_bytes(rep[0x10..0x14].try_into().unwrap()), 0x1083);
        assert_eq!(rep[0x23], 1);
        let ev = CreateEventW(null_mut(), 1, 0, null());
        let mut pov = OVERLAPPED { event: ev, ..OVERLAPPED::default() };
        assert_eq!((link_dev::DEVICE.ioctl)(d, IOCTL_RECV, &zero as *const u32 as *const _, 4, &mut wb as *mut WSABUF as *mut _, 0x10, &mut ret, &mut pov), 0);
        assert_eq!(GetLastError(), 0x3e5);
        assert_eq!(pov.internal, 0x103);
        assert_eq!(WaitForSingleObject(ev, 50), 0x102);
        assert_eq!((link_dev::DEVICE.cancel_io)(d), 1);
        assert_eq!(WaitForSingleObject(ev, 0), 0);
        assert_eq!(pov.internal, 0xc000_0120);
        assert_eq!((link_dev::DEVICE.ioctl)(d, 0x0022_0012, null(), 0, rep.as_mut_ptr() as *mut _, 0x18, &mut ret, &mut ov), 0);
        assert_eq!(GetLastError(), 1);
        assert_eq!((link_dev::DEVICE.close)(d), 1);
        assert_eq!((link_dev::DEVICE.close)(d), 0);
        CloseHandle(ev);
    }
}
