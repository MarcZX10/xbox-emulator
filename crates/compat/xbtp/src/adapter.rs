use crate::base::{log_error_args, log_info_args};
use crate::conn::{channel_delete, channel_receive, channel_send, connection_connect, connection_delete, connection_open_redirect, XbtpChannel, XbtpConnection};
use crate::encoding::*;
use crate::link::WSABUF;
use crate::msvc::{cpp_delete, cpp_new, SharedPtr};
use crate::session::{guid_args, status_code_text, status_error, E_REDIRECTED, XBTP_PROTOCOL_V1_SIGNATURE};
use crate::sys::*;
use crate::xbtp2_json::utf8_to_bstr;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU16, Ordering};

pub const ADAPTER_DISCONNECTED: u32 = 0;
pub const ADAPTER_CONNECTED: u32 = 1;
pub const ADAPTER_IN_SESSION: u32 = 2;
pub const E_CONNECT_NOT_OK: HRESULT = 0x8376_0002u32 as i32;
pub const CLIENT_AGENT: &[u8] = b"XbtpClientAgent/221000/GameCore\0";
pub const DEFAULT_TIMEOUT_MS: u32 = 30_000;

static SESSION_ID_SEED: u16 = 0;
static SESSION_ID_COUNTER: AtomicU16 = AtomicU16::new(0);

#[repr(C)]
pub struct AdapterVtbl {
    pub dtor: unsafe extern "C" fn(*mut XbtpClientAdapter, u32) -> *mut XbtpClientAdapter,
}

#[repr(C)]
pub struct XbtpClientAdapter {
    pub vtbl: *const AdapterVtbl,
    pub state: u32,
    pub session_id: GUID,
    pub channel: SharedPtr<XbtpChannel>,
    pub connection: SharedPtr<XbtpConnection>,
    pub quiet: u8,
}

pub static ADAPTER_VTBL: AdapterVtbl = AdapterVtbl { dtor: adapter_deleting_dtor };

unsafe extern "C" fn adapter_deleting_dtor(this: *mut XbtpClientAdapter, flags: u32) -> *mut XbtpClientAdapter {
    XbtpClientAdapter_dtor(this);
    if flags & 1 != 0 {
        cpp_delete(this as *mut c_void);
    }
    this
}

pub unsafe fn new_adapter() -> *mut XbtpClientAdapter {
    let a = cpp_new(core::mem::size_of::<XbtpClientAdapter>()) as *mut XbtpClientAdapter;
    if !a.is_null() {
        XbtpClientAdapter_ctor(a);
    }
    a
}

pub unsafe fn adapter_delete(this: *mut XbtpClientAdapter) {
    if !this.is_null() {
        ((*(*this).vtbl).dtor)(this, 1);
    }
}

unsafe fn err(this: *mut XbtpClientAdapter, fmt: &[u8], args: &[usize]) {
    if (*this).quiet == 0 {
        log_error_args(fmt, args);
    }
}

unsafe fn info(this: *mut XbtpClientAdapter, fmt: &[u8], args: &[usize]) {
    if (*this).quiet == 0 {
        log_info_args(fmt, args);
    }
}

unsafe fn drop_channel(p: *mut c_void) {
    channel_delete(p as *mut XbtpChannel);
}

unsafe fn drop_connection(p: *mut c_void) {
    connection_delete(p as *mut XbtpConnection);
}

unsafe fn send_single(ch: *mut XbtpChannel, len: u32, buf: *mut u8) -> HRESULT {
    channel_send(ch, &[WSABUF { len, buf }], None, null_mut())
}

pub unsafe fn receive_text(ch: *mut XbtpChannel, timeout: u32, len: u32, out: *mut *mut u8) -> HRESULT {
    if out.is_null() || ch.is_null() {
        return E_INVALIDARG;
    }
    if len == 0 {
        *out = null_mut();
        return S_OK;
    }
    let p = malloc(len as usize) as *mut u8;
    if p.is_null() {
        *out = null_mut();
        return E_OUTOFMEMORY;
    }
    let hr = channel_receive(ch, timeout, len, p);
    if hr < 0 {
        free(p as *mut c_void);
        *out = null_mut();
        return hr;
    }
    *out = p;
    *p.add(len as usize - 1) = 0;
    S_OK
}

unsafe fn free_text(p: *mut u8) {
    if !p.is_null() {
        free(p as *mut c_void);
    }
}

#[export_name = "??0XbtpClientAdapter@@QEAA@XZ"]
pub unsafe extern "C" fn XbtpClientAdapter_ctor(this: *mut XbtpClientAdapter) -> *mut XbtpClientAdapter {
    (*this).vtbl = &ADAPTER_VTBL;
    (*this).state = ADAPTER_DISCONNECTED;
    core::ptr::write(&mut (*this).channel, SharedPtr::default());
    core::ptr::write(&mut (*this).connection, SharedPtr::default());
    (*this).quiet = 0;
    this
}

#[export_name = "??1XbtpClientAdapter@@UEAA@XZ"]
pub unsafe extern "C" fn XbtpClientAdapter_dtor(this: *mut XbtpClientAdapter) {
    (*this).vtbl = &ADAPTER_VTBL;
    if (*this).state == ADAPTER_IN_SESSION {
        adapter_close_session(this);
    }
    adapter_disconnect(this);
    (*this).connection.release();
    (*this).channel.release();
}

#[export_name = "?Initialize@XbtpClientAdapter@@QEAAJV?$shared_ptr@VXbtpConnection@@@std@@@Z"]
pub unsafe extern "C" fn XbtpClientAdapter_Initialize(this: *mut XbtpClientAdapter, connection: *mut SharedPtr<XbtpConnection>) -> HRESULT {
    let hr = if (*connection).ptr.is_null() {
        E_INVALIDARG
    } else {
        (*connection).clone_ref().assign_into(&mut (*this).connection);
        S_OK
    };
    (*connection).release();
    hr
}

pub unsafe fn adapter_close_session(this: *mut XbtpClientAdapter) -> HRESULT {
    if (*this).state != ADAPTER_IN_SESSION {
        return S_OK;
    }
    let mut buf = [0u8; 0x18];
    let mut len = 0u32;
    let hr = SerializeCloseSession(XBTP_PROTOCOL_V1_SIGNATURE, &(*this).session_id, 0x18, &mut len, buf.as_mut_ptr());
    if hr < 0 {
        return hr;
    }
    let hr = send_single((*this).channel.ptr, len, buf.as_mut_ptr());
    if hr < 0 {
        err(this, b"Failed while sending close session\0", &[]);
        return hr;
    }
    (*this).state = ADAPTER_CONNECTED;
    hr
}

pub unsafe fn adapter_disconnect(this: *mut XbtpClientAdapter) {
    if !(*this).channel.ptr.is_null() {
        let mut buf = [0u8; 0xc];
        let mut len = 0u32;
        if SerializeDisconnect(XBTP_PROTOCOL_V1_SIGNATURE, 0, 0xc, &mut len, buf.as_mut_ptr()) >= 0 {
            send_single((*this).channel.ptr, len, buf.as_mut_ptr());
        }
    }
    (*this).state = ADAPTER_DISCONNECTED;
}

#[export_name = "?Connect@XbtpClientAdapter@@QEAAJK@Z"]
pub unsafe extern "C" fn XbtpClientAdapter_ConnectTimeout(this: *mut XbtpClientAdapter, timeout: u32) -> HRESULT {
    if (*this).state == ADAPTER_DISCONNECTED {
        (*this).channel.release();
        if (*this).connection.ptr.is_null() {
            return E_INVALIDARG;
        }
        let mut ch: *mut XbtpChannel = null_mut();
        let hr = connection_connect((*this).connection.ptr, timeout, &mut ch);
        if hr < 0 {
            err(this, b"Failed (0x%X) to connect to server\0", &[hr as u32 as usize]);
            return hr;
        }
        match SharedPtr::new_with(ch, drop_channel) {
            Some(sp) => sp.assign_into(&mut (*this).channel),
            None => {
                channel_delete(ch);
                return E_OUTOFMEMORY;
            }
        }
    }
    let mut agent = UTF8Text { length: CLIENT_AGENT.len() as u32, text: CLIENT_AGENT.as_ptr() as *mut u8 };
    let mut buf = [0u8; 0x29];
    let mut len = 0u32;
    let hr = SerializeConnectRequest(XBTP_PROTOCOL_V1_SIGNATURE, 1, &mut agent, 0x29, &mut len, buf.as_mut_ptr());
    if hr < 0 {
        err(this, b"Failed 0x%X to format connect request\0", &[hr as u32 as usize]);
        return hr;
    }
    let ch = (*this).channel.ptr;
    let hr = send_single(ch, len, buf.as_mut_ptr());
    if hr < 0 {
        err(this, b"Failed 0x%X to send connect request\0", &[hr as u32 as usize]);
        return hr;
    }
    let mut fixed = [0u8; 8];
    let hr = channel_receive(ch, timeout, 8, fixed.as_mut_ptr());
    if hr < 0 {
        return hr;
    }
    let mut resp = XBTP_CONNECT_RESPONSE::default();
    let hr = DeserializeFixedConnectResponse(8, fixed.as_mut_ptr(), &mut resp);
    if hr < 0 {
        err(this, b"Failed (0x%X) to receive valid connect response\0", &[hr as u32 as usize]);
        return hr;
    }
    let mut server_agent: *mut u8 = null_mut();
    let hr = receive_text(ch, timeout, resp.agent.length, &mut server_agent);
    if hr < 0 {
        err(this, b"Failed 0x%X to read agent\0", &[hr as u32 as usize]);
        return hr;
    }
    let hr = if resp.status != 0 {
        err(this, b"Did not receive OK %d\0", &[resp.status as usize]);
        E_CONNECT_NOT_OK
    } else {
        (*this).state = ADAPTER_CONNECTED;
        info(this, b"Client agent = %s\0", &[CLIENT_AGENT.as_ptr() as usize]);
        info(this, b"Server agent = %s\0", &[server_agent as usize]);
        S_OK
    };
    free_text(server_agent);
    hr
}

#[export_name = "?Connect@XbtpClientAdapter@@QEAAJXZ"]
pub unsafe extern "C" fn XbtpClientAdapter_Connect(this: *mut XbtpClientAdapter) -> HRESULT {
    XbtpClientAdapter_ConnectTimeout(this, DEFAULT_TIMEOUT_MS)
}

pub fn next_session_id() -> GUID {
    let n = SESSION_ID_COUNTER.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
    GUID::new((SESSION_ID_SEED as u32) << 16, n, 0, [0; 8])
}

pub unsafe fn adapter_negotiate(this: *mut XbtpClientAdapter, extension: *const GUID, timeout: u32, location: *mut BSTR) -> HRESULT {
    *location = null_mut();
    let mut text: *mut u8 = null_mut();
    let hr = 'run: {
        if (*this).state == ADAPTER_IN_SESSION {
            let hr = adapter_close_session(this);
            if hr < 0 {
                err(this, b"Failed 0x%X to shutdown previous session\0", &[hr as u32 as usize]);
                adapter_disconnect(this);
            }
        }
        if (*this).state == ADAPTER_DISCONNECTED {
            let hr = XbtpClientAdapter_ConnectTimeout(this, timeout);
            if hr < 0 {
                err(this, b"Could not reconnect\0", &[]);
                break 'run hr;
            }
        }
        (*this).session_id = next_session_id();
        let mut buf = [0u8; 0x28];
        let mut len = 0u32;
        let hr = SerializeCreateSessionRequest(XBTP_PROTOCOL_V1_SIGNATURE, &(*this).session_id, extension, 0x28, &mut len, buf.as_mut_ptr());
        if hr < 0 {
            break 'run hr;
        }
        let ch = (*this).channel.ptr;
        let hr = send_single(ch, len, buf.as_mut_ptr());
        if hr < 0 {
            err(this, b"Failed 0x%X to send create session message\0", &[hr as u32 as usize]);
            break 'run hr;
        }
        let mut fixed = [0u8; 0x20];
        let hr = channel_receive(ch, timeout, 0x20, fixed.as_mut_ptr());
        if hr < 0 {
            err(this, b"Failed 0x%X to receive create session response\0", &[hr as u32 as usize]);
            break 'run hr;
        }
        let mut resp = XBTP_CREATE_SESSION_RESPONSE_MESSAGE::default();
        let hr = DeserializeFixedCreateSessionResponse(0x20, fixed.as_mut_ptr(), &mut resp);
        if hr < 0 {
            err(this, b"Failed 0x%X - recieved malformed session response\0", &[hr as u32 as usize]);
            break 'run hr;
        }
        if resp.location.length != 0 {
            let hr = receive_text(ch, timeout, resp.location.length, &mut text);
            if hr < 0 {
                err(this, b"Failed 0x%X to receive location\0", &[hr as u32 as usize]);
                break 'run hr;
            }
            resp.location.text = text;
        }
        match resp.status {
            0 => {
                (*this).session_id = resp.session_id;
                (*this).state = ADAPTER_IN_SESSION;
                hr
            }
            -6 => {
                let src: &[u8] = if resp.location.text.is_null() { &[] } else { core::slice::from_raw_parts(resp.location.text, strlen(resp.location.text)) };
                let mut b: BSTR = null_mut();
                let hr = utf8_to_bstr(src, &mut b);
                if hr < 0 {
                    if !b.is_null() {
                        SysFreeString(b);
                    }
                    break 'run hr;
                }
                *location = b;
                E_REDIRECTED
            }
            s => {
                let reason = match status_code_text(s) {
                    Ok(t) => t,
                    Err(e) => break 'run e,
                };
                err(this, b"Session negotiation failed: %ws\0", &[reason.as_ptr() as usize]);
                status_error(s)
            }
        }
    };
    let f = crate::session::cfmt("Negotiate Session Id: {%08X-%04X-%04X-%02X%02X-%02X%02X%02X%02X%02X%02X}\n");
    log_info_args(&f, &guid_args(&(*this).session_id));
    free_text(text);
    hr
}

#[export_name = "?BeginSession@XbtpClientAdapter@@QEAAJAEBU_GUID@@K@Z"]
pub unsafe extern "C" fn XbtpClientAdapter_BeginSessionTimeout(this: *mut XbtpClientAdapter, extension: *const GUID, timeout: u32) -> HRESULT {
    let mut location: BSTR = null_mut();
    let mut hr = adapter_negotiate(this, extension, timeout, &mut location);
    if hr == E_REDIRECTED {
        adapter_disconnect(this);
        hr = 'run: {
            if (*this).connection.ptr.is_null() {
                break 'run E_INVALIDARG;
            }
            let mut next: *mut XbtpConnection = null_mut();
            let hr = connection_open_redirect((*this).connection.ptr, location, &mut next);
            if hr < 0 {
                err(this, b"Failed 0x%X to reinitialize connection with redirect %ws\0", &[hr as u32 as usize, location as usize]);
                break 'run hr;
            }
            match SharedPtr::new_with(next, drop_connection) {
                Some(sp) => sp.assign_into(&mut (*this).connection),
                None => {
                    connection_delete(next);
                    break 'run E_OUTOFMEMORY;
                }
            }
            let hr = XbtpClientAdapter_ConnectTimeout(this, timeout);
            if hr < 0 {
                err(this, b"Failed 0x%X to connect after redirect %ws\0", &[hr as u32 as usize, location as usize]);
                break 'run hr;
            }
            let mut again: BSTR = null_mut();
            let hr = adapter_negotiate(this, extension, timeout, &mut again);
            if hr < 0 {
                err(this, b"Failed 0x%X to negotiate session after redirect %ws\0", &[hr as u32 as usize, location as usize]);
            }
            if !again.is_null() {
                SysFreeString(again);
            }
            hr
        };
    }
    if !location.is_null() {
        SysFreeString(location);
    }
    hr
}

#[export_name = "?BeginSession@XbtpClientAdapter@@QEAAJAEBU_GUID@@@Z"]
pub unsafe extern "C" fn XbtpClientAdapter_BeginSession(this: *mut XbtpClientAdapter, extension: *const GUID) -> HRESULT {
    XbtpClientAdapter_BeginSessionTimeout(this, extension, DEFAULT_TIMEOUT_MS)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::conn::tests::{init, NET_LOCK};
    use crate::conn::{connection_accept, connection_event, TcpConnection, TcpConnection_InitializeClient, TcpConnection_InitializeServer};
    use std::thread;

    const EXT: GUID = GUID::new(0xabcdef01, 0x2345, 0x6789, [0xa, 0xb, 0xc, 0xd, 0xe, 0xf, 1, 2]);

    #[test]
    fn layout_and_ctor() {
        assert_eq!(core::mem::size_of::<XbtpClientAdapter>(), 0x48);
        assert_eq!(core::mem::offset_of!(XbtpClientAdapter, state), 8);
        assert_eq!(core::mem::offset_of!(XbtpClientAdapter, session_id), 0xc);
        assert_eq!(core::mem::offset_of!(XbtpClientAdapter, channel), 0x20);
        assert_eq!(core::mem::offset_of!(XbtpClientAdapter, connection), 0x30);
        assert_eq!(core::mem::offset_of!(XbtpClientAdapter, quiet), 0x40);
        unsafe {
            let a = new_adapter();
            assert_eq!((*a).state, 0);
            assert!((*a).channel.is_null() && (*a).connection.is_null());
            let mut c = SharedPtr::<XbtpConnection>::default();
            assert_eq!(XbtpClientAdapter_Initialize(a, &mut c), E_INVALIDARG);
            assert_eq!(adapter_close_session(a), S_OK);
            adapter_delete(a);
        }
    }

    #[test]
    fn session_ids_count_up() {
        let a = next_session_id();
        let b = next_session_id();
        assert_eq!(a.data1, 0);
        assert_eq!(b.data2, a.data2.wrapping_add(1));
        assert_eq!(a.data3, 0);
    }

    #[test]
    fn receive_text_terminates() {
        unsafe {
            let mut p = 1 as *mut u8;
            assert_eq!(receive_text(null_mut(), 1, 1, &mut p), E_INVALIDARG);
        }
    }

    struct Srv(*mut TcpConnection);
    unsafe impl Send for Srv {}

    unsafe fn accept(server: *mut TcpConnection) -> *mut XbtpChannel {
        let mut ev: HANDLE = null_mut();
        connection_event(server as *mut XbtpConnection, &mut ev);
        WaitForSingleObject(ev, 10_000);
        let mut ch = null_mut();
        for _ in 0..100 {
            if connection_accept(server as *mut XbtpConnection, &mut ch) != E_PENDING {
                break;
            }
            Sleep(20);
        }
        assert!(!ch.is_null());
        ch
    }

    unsafe fn recv(ch: *mut XbtpChannel, n: usize) -> Vec<u8> {
        let mut v = vec![0u8; n];
        assert_eq!(channel_receive(ch, 10_000, n as u32, v.as_mut_ptr()), S_OK);
        v
    }

    unsafe fn send(ch: *mut XbtpChannel, mut v: Vec<u8>) {
        assert_eq!(send_single(ch, v.len() as u32, v.as_mut_ptr()), S_OK);
    }

    unsafe fn serve_connect(ch: *mut XbtpChannel, status: i32) {
        let req = recv(ch, 8 + CLIENT_AGENT.len());
        assert_eq!(&req[8..], CLIENT_AGENT);
        assert_eq!(u16::from_le_bytes([req[0], req[1]]), 0x5850);
        send(ch, build_connect_response(status, b"TestServer\0").unwrap());
    }

    unsafe fn serve_session(ch: *mut XbtpChannel, status: i32, location: &[u8]) -> GUID {
        let req = recv(ch, 0x28);
        let h = parse_header(&req).unwrap();
        assert_eq!(h.message_type, XBTP_MESSAGE_TYPE_CREATE_SESSION_REQUEST);
        assert_eq!(GUID::from_bytes(&req[0x18..0x28]), EXT);
        let sid = GUID::new(0x5e55, 1, 2, [3; 8]);
        send(ch, build_create_session_response(0x5850, &sid, status, location).unwrap());
        sid
    }

    unsafe fn client(port: u32) -> *mut XbtpClientAdapter {
        let name = wide("localhost");
        let mut c: *mut TcpConnection = null_mut();
        assert_eq!(TcpConnection_InitializeClient(name.as_ptr(), port, &mut c), S_OK);
        let a = new_adapter();
        let mut sp = SharedPtr::new_with(c as *mut XbtpConnection, drop_connection).unwrap();
        assert_eq!(XbtpClientAdapter_Initialize(a, &mut sp), S_OK);
        a
    }

    #[test]
    fn connect_begin_session_and_close() {
        let _g = NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init();
        unsafe {
            let mut server: *mut TcpConnection = null_mut();
            assert_eq!(TcpConnection_InitializeServer(47821, &mut server), S_OK);
            let s = Srv(server);
            let t = thread::spawn(move || {
                let s = s;
                let ch = accept(s.0);
                serve_connect(ch, 0);
                let sid = serve_session(ch, 0, b"");
                let close = recv(ch, 0x18);
                assert_eq!(parse_header(&close).unwrap().message_type, XBTP_MESSAGE_TYPE_CLOSE_SESSION);
                assert_eq!(GUID::from_bytes(&close[8..0x18]), sid);
                let disc = recv(ch, 0xc);
                assert_eq!(parse_header(&disc).unwrap().message_type, XBTP_MESSAGE_TYPE_DISCONNECT);
                channel_delete(ch);
                s
            });
            let a = client(47821);
            assert_eq!(XbtpClientAdapter_BeginSessionTimeout(a, &EXT, 10_000), S_OK);
            assert_eq!((*a).state, ADAPTER_IN_SESSION);
            assert_eq!((*a).session_id, GUID::new(0x5e55, 1, 2, [3; 8]));
            adapter_delete(a);
            let s = t.join().unwrap();
            connection_delete(s.0 as *mut XbtpConnection);
        }
    }

    #[test]
    fn connect_rejected_and_session_refused() {
        let _g = NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init();
        unsafe {
            let mut server: *mut TcpConnection = null_mut();
            assert_eq!(TcpConnection_InitializeServer(47822, &mut server), S_OK);
            let s = Srv(server);
            let t = thread::spawn(move || {
                let s = s;
                let ch = accept(s.0);
                serve_connect(ch, -2);
                serve_connect(ch, 0);
                serve_session(ch, -3, b"");
                channel_delete(ch);
                s
            });
            let a = client(47822);
            assert_eq!(XbtpClientAdapter_ConnectTimeout(a, 10_000), E_CONNECT_NOT_OK);
            assert_eq!((*a).state, ADAPTER_DISCONNECTED);
            (*a).state = ADAPTER_CONNECTED;
            assert_eq!(XbtpClientAdapter_ConnectTimeout(a, 10_000), S_OK);
            assert_eq!(XbtpClientAdapter_BeginSessionTimeout(a, &EXT, 10_000), status_error(-3));
            assert_eq!((*a).state, ADAPTER_CONNECTED);
            let s = t.join().unwrap();
            (*a).quiet = 1;
            adapter_delete(a);
            connection_delete(s.0 as *mut XbtpConnection);
        }
    }

    #[test]
    fn redirect_reconnects_to_new_port() {
        let _g = NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init();
        unsafe {
            let mut s1: *mut TcpConnection = null_mut();
            let mut s2: *mut TcpConnection = null_mut();
            assert_eq!(TcpConnection_InitializeServer(47823, &mut s1), S_OK);
            assert_eq!(TcpConnection_InitializeServer(47824, &mut s2), S_OK);
            let (a1, a2) = (Srv(s1), Srv(s2));
            let t = thread::spawn(move || {
                let (a1, a2) = (a1, a2);
                let ch = accept(a1.0);
                serve_connect(ch, 0);
                serve_session(ch, -6, b"xbtp::tcp:localhost:47824\0");
                let disc = recv(ch, 0xc);
                assert_eq!(parse_header(&disc).unwrap().message_type, XBTP_MESSAGE_TYPE_DISCONNECT);
                let ch2 = accept(a2.0);
                serve_connect(ch2, 0);
                serve_session(ch2, 0, b"");
                channel_delete(ch);
                (a1, a2, ch2 as usize)
            });
            let a = client(47823);
            assert_eq!(XbtpClientAdapter_BeginSessionTimeout(a, &EXT, 10_000), S_OK);
            assert_eq!((*a).state, ADAPTER_IN_SESSION);
            let (a1, a2, ch2) = t.join().unwrap();
            (*a).quiet = 1;
            adapter_delete(a);
            channel_delete(ch2 as *mut XbtpChannel);
            connection_delete(a1.0 as *mut XbtpConnection);
            connection_delete(a2.0 as *mut XbtpConnection);
        }
    }
}
