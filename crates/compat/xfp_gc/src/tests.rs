use super::*;
use std::sync::mpsc;

#[link(name = "kernel32")]
extern "system" {
    fn CreateNamedPipeW(name: *const u16, open: u32, mode: u32, max: u32, outb: u32, inb: u32, timeout: u32, sa: *const c_void) -> isize;
    fn ConnectNamedPipe(h: isize, ov: *mut c_void) -> i32;
    fn DisconnectNamedPipe(h: isize) -> i32;
    fn GetCurrentProcessId() -> u32;
}

static SERIAL: Mutex<()> = Mutex::new(());

struct Server {
    msgs: mpsc::Receiver<Vec<u8>>,
    join: std::thread::JoinHandle<()>,
}

fn unique(tag: &str) -> String {
    format!("\\\\.\\pipe\\tombox_xfp_test_{}_{}", unsafe { GetCurrentProcessId() }, tag)
}

fn serve(name: &str, grant: u16, led: u16, keys: u32) -> Server {
    let w = wide(name);
    let h = unsafe { CreateNamedPipeW(w.as_ptr(), 3, 4 | 2, 1, 0x4000, 0x4000, 0, core::ptr::null()) };
    assert_ne!(h, INVALID);
    let (tx, rx) = mpsc::channel();
    let join = std::thread::spawn(move || unsafe {
        ConnectNamedPipe(h, core::ptr::null_mut());
        let mut buf = vec![0u8; 0x4000];
        let mut n = 0u32;
        if ReadFile(h, buf.as_mut_ptr(), buf.len() as u32, &mut n, core::ptr::null_mut()) == 0 {
            CloseHandle(h);
            return;
        }
        tx.send(buf[..n as usize].to_vec()).ok();
        let mut reply = header(0, 0x12);
        reply[0x10..0x12].copy_from_slice(&grant.to_le_bytes());
        WriteFile(h, reply.as_ptr(), 0x12, &mut n, core::ptr::null_mut());
        if grant != 0 {
            let mut k = header(MSG_KEYS, 0x14);
            k[0x10..0x14].copy_from_slice(&keys.to_le_bytes());
            WriteFile(h, k.as_ptr(), 0x14, &mut n, core::ptr::null_mut());
            loop {
                if ReadFile(h, buf.as_mut_ptr(), buf.len() as u32, &mut n, core::ptr::null_mut()) == 0 {
                    break;
                }
                let m = buf[..n as usize].to_vec();
                if u32::from_le_bytes(m[..4].try_into().unwrap()) == MSG_GET_LED {
                    let mut r = header(MSG_GET_LED, 0x12);
                    r[0x10..0x12].copy_from_slice(&led.to_le_bytes());
                    WriteFile(h, r.as_ptr(), 0x12, &mut n, core::ptr::null_mut());
                }
                tx.send(m).ok();
            }
        }
        DisconnectNamedPipe(h);
        CloseHandle(h);
    });
    Server { msgs: rx, join }
}

fn reset() {
    XfpCleanup();
    for _ in 0..500 {
        if st().pipe == INVALID && st().stop == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let mut s = st();
    s.denied = false;
    s.keys = 0;
    s.led = 0;
}

fn connected(tag: &str, led: u16, keys: u32) -> Server {
    let name = unique(tag);
    set_pipe_path(Some(&name));
    let srv = serve(&name, 1, led, keys);
    assert_eq!(XfpTryConnect(), 0);
    let hello = srv.msgs.recv().unwrap();
    assert_eq!(hello.len(), 0x22);
    assert_eq!(&hello[..0x10], &[0u8; 0x10]);
    assert_eq!(&hello[0x10..0x12], &2u16.to_le_bytes());
    assert_eq!(&hello[0x12..0x1a], CLIENT_NAME);
    srv
}

fn finish(srv: Server) {
    reset();
    srv.join.join().unwrap();
    set_pipe_path(None);
}

#[test]
fn check_availability_reports_pipe_presence() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    set_pipe_path(Some(&unique("absent")));
    assert_eq!(XfpCheckAvailability(), 0x8007_0002u32 as i32);
    let name = unique("avail");
    set_pipe_path(Some(&name));
    let srv = serve(&name, 0, 0, 0);
    assert_eq!(XfpCheckAvailability(), 0);
    drop(srv.msgs);
    srv.join.join().unwrap();
    set_pipe_path(None);
}

#[test]
fn cleanup_stops_reader_and_closes_pipe() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let srv = connected("cleanup", 0, 0);
    assert_ne!(st().pipe, INVALID);
    assert_ne!(st().thread, 0);
    XfpCleanup();
    assert_eq!(st().thread, 0);
    for _ in 0..500 {
        if st().pipe == INVALID {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(st().pipe, INVALID);
    assert_eq!(XfpSetLEDState(1), XFP_NOT_CONNECTED);
    finish(srv);
}

#[test]
fn get_key_state_follows_reader_thread() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let srv = connected("keys", 0, 0x1234_0042);
    let mut k = 0u32;
    for _ in 0..500 {
        unsafe { XfpGetKeyState(&mut k) };
        if k != 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(k, 0x1234_0042);
    finish(srv);
}

#[test]
fn get_led_state_round_trips() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let mut led = 7u16;
    assert_eq!(unsafe { XfpGetLEDState(&mut led) }, XFP_NOT_CONNECTED);
    assert_eq!(led, 0);
    let srv = connected("led", 0x1f, 0);
    assert_eq!(unsafe { XfpGetLEDState(&mut led) }, 0);
    assert_eq!(led, 0x1f);
    let m = srv.msgs.recv().unwrap();
    assert_eq!(m, header(MSG_GET_LED, 0x12));
    finish(srv);
}

#[test]
fn set_display_buffer_sends_4bpp_frames() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    assert_eq!(unsafe { XfpSetDisplayBuffer(core::ptr::null(), 4) }, E_POINTER);
    let px4 = vec![0x5au8; DISPLAY_BYTES];
    assert_eq!(unsafe { XfpSetDisplayBuffer(px4.as_ptr(), 4) }, XFP_NOT_CONNECTED);
    let srv = connected("display", 0, 0);
    assert_eq!(unsafe { XfpSetDisplayBuffer(px4.as_ptr(), 4) }, 0);
    let m = srv.msgs.recv().unwrap();
    assert_eq!(m.len(), HEADER + DISPLAY_BYTES);
    assert_eq!(u32::from_le_bytes(m[..4].try_into().unwrap()), MSG_DISPLAY);
    assert!(m[HEADER..].iter().all(|&b| b == 0x5a));
    let px8: Vec<u8> = (0..2 * DISPLAY_BYTES).map(|i| (i * 37) as u8).collect();
    assert_eq!(unsafe { XfpSetDisplayBuffer(px8.as_ptr(), 8) }, 0);
    let m = srv.msgs.recv().unwrap();
    assert_eq!(m[HEADER], (px8[0] & 0xf0) | (px8[1] >> 4));
    assert_eq!(m[HEADER + 777], (px8[1554] & 0xf0) | (px8[1555] >> 4));
    assert_eq!(&m[HEADER..], &pack_8bpp(&px8)[..]);
    assert_eq!(unsafe { XfpSetDisplayBuffer(px4.as_ptr(), 1) }, E_INVALIDARG);
    finish(srv);
}

#[test]
fn set_display_mode_validates_and_sends() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    assert_eq!(XfpSetDisplayMode(1), XFP_NOT_CONNECTED);
    let srv = connected("mode", 0, 0);
    assert_eq!(XfpSetDisplayMode(12), XFP_BAD_MODE);
    assert_eq!(XfpSetDisplayMode(11), 0);
    let m = srv.msgs.recv().unwrap();
    assert_eq!(m.len(), 0x14);
    assert_eq!(u32::from_le_bytes(m[..4].try_into().unwrap()), MSG_MODE);
    assert_eq!(u32::from_le_bytes(m[0x10..0x14].try_into().unwrap()), 11);
    finish(srv);
}

#[test]
fn set_led_state_sends_message_6() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let srv = connected("setled", 0, 0);
    assert_eq!(XfpSetLEDState(0x15), 0);
    let m = srv.msgs.recv().unwrap();
    assert_eq!(m.len(), 0x12);
    assert_eq!(u32::from_le_bytes(m[..4].try_into().unwrap()), MSG_SET_LED);
    assert_eq!(u16::from_le_bytes([m[0x10], m[0x11]]), 0x15);
    finish(srv);
}

#[test]
fn try_connect_handles_absent_refused_and_granted() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    set_pipe_path(Some(&unique("none")));
    assert_eq!(XfpTryConnect(), 0x8007_0002u32 as i32);
    let name = unique("deny");
    set_pipe_path(Some(&name));
    let srv = serve(&name, 0, 0, 0);
    assert_eq!(XfpTryConnect(), E_ACCESSDENIED);
    assert_eq!(XfpTryConnect(), E_ACCESSDENIED);
    assert_eq!(XfpCheckAvailability(), E_ACCESSDENIED);
    srv.join.join().unwrap();
    reset();
    let srv = connected("grant", 0, 0);
    assert_eq!(XfpTryConnect(), 0);
    assert_eq!(XfpCheckAvailability(), 0);
    finish(srv);
}
