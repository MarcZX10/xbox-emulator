use crate::adapter::{adapter_close_session, XbtpClientAdapter};
use crate::base::{log_error_args, log_info_args, log_warning_args, main_level};
use crate::conn::{channel_next_exchange_id, channel_receive, channel_send, SendCallback, XbtpChannel, MID_MESSAGE_TIMEOUT};
use crate::encoding::*;
use crate::link::WSABUF;
use crate::msvc::{cpp_delete, cpp_new, SharedPtr};
use crate::sys::*;
use crate::xbtp2_json::{hash_utf8, parse, serialize, utf8_to_wide, Map, Node, NodeRef, Val};
use core::ffi::c_void;
use core::ptr::null_mut;

pub const XBTP_PROTOCOL_V1_SIGNATURE: i16 = 0x5850;
pub const XBTP_MESSAGE_TYPE_ONEWAY: i16 = 1;
pub const XBTP_MESSAGE_TYPE_BEGIN: i16 = 2;
pub const XBTP_MESSAGE_TYPE_MORE: i16 = 3;
pub const XBTP_MESSAGE_TYPE_END: i16 = 4;
pub const DEFAULT_RECEIVE_TIMEOUT_MS: u32 = 30_000;

pub const XBTP_STATUS_SERVER_AGENT_ERROR: i32 = -7;

pub const E_SESSION_NOT_FOUND: HRESULT = 0x8c11_01fcu32 as i32;
pub const E_REDIRECTED: HRESULT = 0x8c11_01fau32 as i32;
pub const E_UNEXPECTED_CLOSE_SESSION: HRESULT = 0x8c11_02fcu32 as i32;
pub const E_UNEXPECTED_EXCHANGE_ID: HRESULT = 0x8c11_0306u32 as i32;
pub const E_REMOTE_DISCONNECT: HRESULT = 0x8c11_0403u32 as i32;
pub const E_REMOTE_CLOSE_SESSION: HRESULT = 0x8c11_0404u32 as i32;
pub const E_SESSION_CLOSED: HRESULT = 0x8c11_0405u32 as i32;

const GUID_FMT: &str = "{%08X-%04X-%04X-%02X%02X-%02X%02X%02X%02X%02X%02X}";

pub fn status_error(status: i32) -> HRESULT {
    (0x8c11_0100u32 | (status as u32 & 0xff)) as i32
}

pub fn unexpected_message_error(message_type: i16) -> HRESULT {
    (0x8c11_0200u32 | (message_type as u32 & 0xff)) as i32
}

pub fn guid_args(g: &GUID) -> [usize; 11] {
    let d = g.data4;
    [
        g.data1 as usize,
        g.data2 as usize,
        g.data3 as usize,
        d[0] as usize,
        d[1] as usize,
        d[2] as usize,
        d[3] as usize,
        d[4] as usize,
        d[5] as usize,
        d[6] as usize,
        d[7] as usize,
    ]
}

pub fn cfmt(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

pub fn status_code_text(status: i32) -> Result<Vec<u16>, HRESULT> {
    let text = match status {
        0 => "Status OK".to_string(),
        -1 => "There is protocol mismatch between client and server".to_string(),
        -2 => "The agent is prohibited".to_string(),
        -3 => "The extension was not found".to_string(),
        -4 => "The specified session was not found".to_string(),
        -5 => "The operation was not found".to_string(),
        -6 => "The extension was redirected".to_string(),
        -7 => "The server agent encountered an error".to_string(),
        -8 => "The extension encountered an error".to_string(),
        s => format!("The status code ({:X}) is not recognized", s as u32),
    };
    Ok(wide(&text))
}

#[repr(C)]
pub struct SessionVtbl {
    pub dtor: unsafe extern "C" fn(*mut XbtpSession, u32) -> *mut XbtpSession,
}

#[repr(C)]
pub struct XbtpSession {
    pub vtbl: *const SessionVtbl,
    pub session_id: GUID,
    pub channel: SharedPtr<XbtpChannel>,
    pub adapter: SharedPtr<XbtpClientAdapter>,
}

pub static SESSION_VTBL: SessionVtbl = SessionVtbl { dtor: session_deleting_dtor };

unsafe extern "C" fn session_deleting_dtor(this: *mut XbtpSession, flags: u32) -> *mut XbtpSession {
    session_destroy(this);
    if flags & 1 != 0 {
        cpp_delete(this as *mut c_void);
    }
    this
}

pub unsafe fn session_destroy(this: *mut XbtpSession) {
    (*this).vtbl = &SESSION_VTBL;
    let a = (*this).adapter.ptr;
    if !a.is_null() {
        adapter_close_session(a);
    }
    (*this).adapter.release();
    (*this).channel.release();
}

pub unsafe fn new_session() -> *mut XbtpSession {
    let s = cpp_new(core::mem::size_of::<XbtpSession>()) as *mut XbtpSession;
    if !s.is_null() {
        XbtpSession_ctor(s);
    }
    s
}

pub unsafe fn session_delete(this: *mut XbtpSession) {
    if !this.is_null() {
        ((*(*this).vtbl).dtor)(this, 1);
    }
}

#[export_name = "??0XbtpSession@@QEAA@XZ"]
pub unsafe extern "C" fn XbtpSession_ctor(this: *mut XbtpSession) -> *mut XbtpSession {
    (*this).vtbl = &SESSION_VTBL;
    core::ptr::write(&mut (*this).channel, SharedPtr::default());
    core::ptr::write(&mut (*this).adapter, SharedPtr::default());
    this
}

#[export_name = "?Initialize@XbtpSession@@QEAAJAEBU_GUID@@V?$shared_ptr@VXbtpClientAdapter@@@std@@V?$shared_ptr@VXbtpChannel@@@4@@Z"]
pub unsafe extern "C" fn XbtpSession_Initialize(this: *mut XbtpSession, id: *const GUID, adapter: *mut SharedPtr<XbtpClientAdapter>, channel: *mut SharedPtr<XbtpChannel>) -> HRESULT {
    let hr = if (*channel).ptr.is_null() {
        E_INVALIDARG
    } else {
        (*this).session_id = *id;
        (*adapter).clone_ref().assign_into(&mut (*this).adapter);
        (*channel).clone_ref().assign_into(&mut (*this).channel);
        S_OK
    };
    (*adapter).release();
    (*channel).release();
    hr
}

unsafe fn verify_open(this: *mut XbtpSession) -> HRESULT {
    if (*this).channel.ptr.is_null() {
        E_SESSION_CLOSED
    } else {
        S_OK
    }
}

pub unsafe fn session_get_event(this: *mut XbtpSession, out: *mut HANDLE) -> HRESULT {
    if out.is_null() {
        return E_INVALIDARG;
    }
    *out = INVALID_HANDLE_VALUE;
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    let ev = (*(*this).channel.ptr).recv_event;
    if ev == INVALID_HANDLE_VALUE {
        return E_NOTIMPL;
    }
    *out = ev;
    S_OK
}

pub unsafe fn session_get_address(this: *mut XbtpSession, out: *mut BSTR) -> HRESULT {
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    let ch = (*this).channel.ptr;
    ((*(*ch).vtbl).get_address)(ch, out)
}

fn num(v: f64) -> NodeRef {
    Node::new(Val::Num(v))
}

fn object(entries: Vec<(&str, NodeRef)>) -> NodeRef {
    let mut m = Map::default();
    for (k, v) in entries {
        m.insert(k.as_bytes().to_vec(), hash_utf8(k.as_bytes()), v);
    }
    Node::new(Val::Obj(m))
}

pub fn should_log_message(message_type: i16, operation_id: u32) -> bool {
    let level = main_level();
    level >= 5 || (level >= 2 && message_type == XBTP_MESSAGE_TYPE_END && operation_id != 0)
}

pub unsafe fn message_json(m: &XBTP_BEGIN_MESSAGE) -> Option<Vec<u8>> {
    let header = object(vec![
        ("Signature", num(m.header.signature as f64)),
        ("MessageType", num(m.header.message_type as f64)),
        ("MessageLength", num(m.header.message_length as f64)),
    ]);
    let d4: Vec<NodeRef> = m.session_id.data4.iter().map(|b| num(*b as f64)).collect();
    let sid = object(vec![
        ("Data1", num(m.session_id.data1 as f64)),
        ("Data2", num(m.session_id.data2 as f64)),
        ("Data3", num(m.session_id.data3 as f64)),
        ("Data4", Node::new(Val::Arr(d4))),
    ]);
    let mut body_entries = vec![("JsonBodyLength", num(m.body.json_body_length as f64)), ("BinaryBodyLength", num(m.body.binary_body_length as f64))];
    if !m.body.json_body.is_null() {
        let raw = core::slice::from_raw_parts(m.body.json_body, m.body.json_body_length as usize);
        let parsed = parse(raw).ok()?;
        if parsed.kind() != 5 {
            return None;
        }
        body_entries.push(("JsonBody", parsed));
    }
    let body = object(body_entries);
    let msg = object(vec![
        ("Header", header),
        ("SessionId", sid),
        ("P1", num(m.exchange_id as f64)),
        ("P2", num(m.operation_id as f64)),
        ("Body", body),
    ]);
    let mut out = Vec::new();
    if serialize(&msg, &mut out) < 0 {
        return None;
    }
    Some(out)
}

pub unsafe fn log_message(m: &XBTP_BEGIN_MESSAGE) {
    let Some(mut text) = message_json(m) else { return };
    text.push(0);
    let Ok(w) = utf8_to_wide(text.as_ptr()) else { return };
    let args = [w.as_ptr() as usize];
    if m.header.message_type as i16 == XBTP_MESSAGE_TYPE_END && m.operation_id != 0 {
        log_error_args(b"%ws\0", &args);
    } else {
        log_info_args(b"%ws\0", &args);
    }
}

unsafe fn alloc_bytes(n: u32) -> *mut u8 {
    malloc((n as usize).max(1)) as *mut u8
}

unsafe fn free_bytes(p: *mut u8) {
    if !p.is_null() {
        free(p as *mut c_void);
    }
}

pub unsafe fn receive_fixed_extension_message(session_id: &GUID, header: &XBTP_MESSAGE_HEADER, ch: *mut XbtpChannel, timeout: u32, msg: *mut XBTP_BEGIN_MESSAGE) -> HRESULT {
    let t = header.message_type as i16;
    if !(1..=4).contains(&t) {
        return E_INVALIDARG;
    }
    let mut fixed = [0u8; 0x20];
    let hr = channel_receive(ch, timeout, 0x20, fixed.as_mut_ptr());
    if hr < 0 {
        log_error_args(b"Failed 0x%X failed to read remaining fixed message\0", &[hr as u32 as usize]);
        return hr;
    }
    let hr = DeserializeFixedExtensionMessage(header, 0x20, fixed.as_mut_ptr(), msg);
    if hr < 0 {
        return hr;
    }
    let mut json: *mut u8 = null_mut();
    let mut bin: *mut u8 = null_mut();
    let hr = 'run: {
        if (*msg).session_id != *session_id {
            let mut args = guid_args(&(*msg).session_id).to_vec();
            args.extend_from_slice(&guid_args(session_id));
            let f = cfmt(&format!("Incorrect session id - \nActual:\n{}\nExpected\n{}", GUID_FMT, GUID_FMT));
            log_error_args(&f, &args);
            break 'run E_SESSION_NOT_FOUND;
        }
        let jl = (*msg).body.json_body_length;
        if jl != 0 {
            json = alloc_bytes(jl);
            if json.is_null() {
                break 'run E_OUTOFMEMORY;
            }
            let hr = channel_receive(ch, timeout, jl, json);
            if hr < 0 {
                log_error_args(b"Failed to receive remaining json message (Type %d Length %d)\0", &[t as i32 as usize, jl as usize]);
                break 'run hr;
            }
        }
        let bl = (*msg).body.binary_body_length;
        if bl != 0 {
            bin = alloc_bytes(bl);
            if bin.is_null() {
                break 'run E_OUTOFMEMORY;
            }
            let hr = channel_receive(ch, timeout, bl, bin);
            if hr < 0 {
                log_error_args(b"Failed to receive remaining binary message (Type %d Length %d)\0", &[t as i32 as usize, bl as usize]);
                break 'run hr;
            }
        }
        (*msg).body.binary_body = bin;
        (*msg).body.json_body = json;
        return S_OK;
    };
    free_bytes(bin);
    free_bytes(json);
    (*msg).body.binary_body = null_mut();
    (*msg).body.json_body = null_mut();
    hr
}

pub unsafe fn receive_extension_message(session_id: &GUID, ch: *mut XbtpChannel, timeout: u32, msg: *mut XBTP_BEGIN_MESSAGE) -> HRESULT {
    let intermediate = timeout.max(MID_MESSAGE_TIMEOUT);
    if ch.is_null() {
        return E_INVALIDARG;
    }
    let mut hb = [0u8; 8];
    let hr = channel_receive(ch, timeout, 8, hb.as_mut_ptr());
    if hr < 0 {
        if hr == E_PENDING {
            log_info_args(b"Read of extension message header is pending...\0", &[]);
        } else {
            log_error_args(b"Failed 0x%X to read extension message header\0", &[hr as u32 as usize]);
        }
        return hr;
    }
    let mut header = match parse_header(&hb) {
        Some(h) => h,
        None => return E_INVALIDARG,
    };
    let t = header.message_type as i16;
    if (1..=4).contains(&t) {
        return receive_fixed_extension_message(session_id, &header, ch, intermediate, msg);
    }
    let mut rest = [0u8; 0x7c];
    let n = match t {
        -4 => 0x10,
        -1 => 4,
        _ => {
            log_error_args(b"Received a invalid extension message (type %d)\0", &[t as i32 as usize]);
            return unexpected_message_error(t);
        }
    };
    let hr = channel_receive(ch, intermediate, n, rest.as_mut_ptr());
    if hr < 0 {
        log_error_args(b"Failed 0x%X reading remaining agent message (type %X)\0", &[hr as u32 as usize, t as i32 as usize]);
        return hr;
    }
    if t == -4 {
        let mut close = XBTP_CLOSE_SESSION_MESSAGE::default();
        let hr = DeserializeCloseSessionMessage(&mut header, 0x7c, rest.as_mut_ptr(), &mut close);
        if hr < 0 {
            return hr;
        }
        if close.session_id == *session_id {
            let f = cfmt(&format!("Received close session for session {}", GUID_FMT));
            log_warning_args(&f, &guid_args(session_id));
            return E_REMOTE_CLOSE_SESSION;
        }
        let mut args = guid_args(session_id).to_vec();
        args.extend_from_slice(&guid_args(&close.session_id));
        let f = cfmt(&format!("Received invalid close session message\nCurrent session\n{}\nRequested close\n{}", GUID_FMT, GUID_FMT));
        log_error_args(&f, &args);
        return E_UNEXPECTED_CLOSE_SESSION;
    }
    let mut disc = XBTP_DISCONNECT_MESSAGE::default();
    let hr = DeserializeDisconnectMessage(&mut header, 0x7c, rest.as_mut_ptr(), &mut disc);
    if hr < 0 {
        return hr;
    }
    let text = match status_code_text(disc.status) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let args = [text.as_ptr() as usize];
    if disc.status == 0 {
        log_warning_args(b"Received disconnect message.  Status: %ws\0", &args);
        E_REMOTE_DISCONNECT
    } else {
        log_error_args(b"Received disconnect message.  Status: %ws\0", &args);
        status_error(disc.status)
    }
}

unsafe fn drop_user_body(p: *mut c_void) {
    let b = p as *mut XBTP_MESSAGE_BODY;
    free_bytes((*b).binary_body);
    free_bytes((*b).json_body);
    free(p);
}

unsafe fn receive_core(this: *mut XbtpSession, timeout: u32, msg: *mut XBTP_BEGIN_MESSAGE, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    (*msg).body.binary_body = null_mut();
    (*msg).body.json_body = null_mut();
    if body.is_null() {
        return E_INVALIDARG;
    }
    let hr = receive_extension_message(&(*this).session_id, (*this).channel.ptr, timeout, msg);
    if hr == E_REMOTE_DISCONNECT || hr == E_REMOTE_CLOSE_SESSION {
        log_error_args(b"Session closed (0x%X)\0", &[hr as u32 as usize]);
        let a = (*this).adapter.ptr;
        if !a.is_null() {
            (*a).channel.release();
            (*a).state = 0;
            (*this).adapter.release();
        }
        (*this).channel.release();
        return E_SESSION_CLOSED;
    }
    if hr < 0 {
        return hr;
    }
    if should_log_message((*msg).header.message_type as i16, (*msg).operation_id) {
        log_message(&*msg);
    }
    let ub = malloc(core::mem::size_of::<XBTP_MESSAGE_BODY>()) as *mut XBTP_MESSAGE_BODY;
    if ub.is_null() {
        free_bytes((*msg).body.binary_body);
        free_bytes((*msg).body.json_body);
        (*msg).body.binary_body = null_mut();
        (*msg).body.json_body = null_mut();
        return E_OUTOFMEMORY;
    }
    ub.write((*msg).body);
    match SharedPtr::new_with(ub, drop_user_body) {
        Some(sp) => {
            sp.assign_into(body);
            S_OK
        }
        None => {
            drop_user_body(ub as *mut c_void);
            (*msg).body.binary_body = null_mut();
            (*msg).body.json_body = null_mut();
            E_OUTOFMEMORY
        }
    }
}

unsafe fn receive_ext_impl(this: *mut XbtpSession, message_type: i16, timeout: u32, msg: *mut XBTP_BEGIN_MESSAGE, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    let hr = receive_core(this, timeout, msg, body);
    if hr < 0 {
        let args = [hr as u32 as usize, message_type as i32 as usize];
        if hr == E_PENDING {
            log_warning_args(b"Failed 0x%X to receive message (requested type %X)\0", &args);
        } else {
            log_error_args(b"Failed 0x%X to receive message (requested type %X)\0", &args);
        }
        return hr;
    }
    let got = (*msg).header.message_type as i16;
    if got != message_type {
        log_error_args(b"Received complete message but incorrect messagetype (Expected %d Received %d)\0", &[message_type as i32 as usize, got as i32 as usize]);
        SharedPtr::take(body).release();
        return unexpected_message_error(got);
    }
    hr
}

pub unsafe fn send_extension_message(ch: *mut XbtpChannel, len: u32, header: *mut u8, body: *const XBTP_MESSAGE_BODY, cb: Option<SendCallback>, ctx: *mut c_void) -> HRESULT {
    if header.is_null() || ch.is_null() {
        return E_INVALIDARG;
    }
    let mut bufs = vec![WSABUF { len, buf: header }];
    if !(*body).json_body.is_null() {
        bufs.push(WSABUF { len: (*body).json_body_length, buf: (*body).json_body });
    }
    if !(*body).binary_body.is_null() {
        bufs.push(WSABUF { len: (*body).binary_body_length, buf: (*body).binary_body });
    }
    let hr = channel_send(ch, &bufs, cb, ctx);
    if hr < 0 {
        if cb.is_some() {
            log_error_args(b"Failed 0x%X to begin send message\0", &[hr as u32 as usize]);
        } else {
            log_error_args(b"Failed 0x%X to synchronously send message\0", &[hr as u32 as usize]);
        }
    }
    hr
}

unsafe fn send_ext_impl(this: *mut XbtpSession, message_type: i16, operation_id: u32, exchange_id: i32, body: *const XBTP_MESSAGE_BODY, cb: Option<SendCallback>, ctx: *mut c_void) -> HRESULT {
    let mut buf = [0u8; 0x28];
    let mut len = 0u32;
    let hr = SerializeFixedExtensionMessage(XBTP_PROTOCOL_V1_SIGNATURE, message_type, &(*this).session_id, exchange_id, operation_id, body, 0x28, &mut len, buf.as_mut_ptr());
    if hr < 0 {
        return hr;
    }
    if should_log_message(message_type, operation_id) {
        if let Some(h) = parse_header(&buf[..8]) {
            let mut m = XBTP_BEGIN_MESSAGE::default();
            if DeserializeFixedExtensionMessage(&h, len, buf.as_mut_ptr().add(8), &mut m) >= 0 {
                m.body.binary_body = (*body).binary_body;
                m.body.json_body = (*body).json_body;
                log_message(&m);
            }
        }
    }
    let hr = send_extension_message((*this).channel.ptr, len, buf.as_mut_ptr(), body, cb, ctx);
    if hr < 0 {
        log_error_args(b"Failed to send message (Type %d exchange %d Op %d)\0", &[message_type as i32 as usize, exchange_id as usize, operation_id as usize]);
    }
    hr
}

#[export_name = "?SendOnewayMessage@XbtpSession@@QEAAJKAEBUXBTP_MESSAGE_BODY@@@Z"]
pub unsafe extern "C" fn XbtpSession_SendOnewayMessage(this: *mut XbtpSession, operation_id: u32, body: *const XBTP_MESSAGE_BODY) -> HRESULT {
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    send_ext_impl(this, XBTP_MESSAGE_TYPE_ONEWAY, operation_id, 0, body, None, null_mut())
}

#[export_name = "?SendBeginMessage@XbtpSession@@QEAAJKAEBUXBTP_MESSAGE_BODY@@PEAH@Z"]
pub unsafe extern "C" fn XbtpSession_SendBeginMessage(this: *mut XbtpSession, operation_id: u32, body: *const XBTP_MESSAGE_BODY, exchange_id: *mut i32) -> HRESULT {
    if exchange_id.is_null() {
        return E_INVALIDARG;
    }
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    let id = channel_next_exchange_id((*this).channel.ptr);
    let hr = send_ext_impl(this, XBTP_MESSAGE_TYPE_BEGIN, operation_id, id, body, None, null_mut());
    if hr >= 0 {
        *exchange_id = id;
    }
    hr
}

#[export_name = "?SendMoreMessage@XbtpSession@@QEAAJHAEBUXBTP_MESSAGE_BODY@@P6AXKKKPEAX@Z1@Z"]
pub unsafe extern "C" fn XbtpSession_SendMoreMessageCallback(this: *mut XbtpSession, exchange_id: i32, body: *const XBTP_MESSAGE_BODY, cb: Option<SendCallback>, ctx: *mut c_void) -> HRESULT {
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    send_ext_impl(this, XBTP_MESSAGE_TYPE_MORE, 0, exchange_id, body, cb, ctx)
}

#[export_name = "?SendMoreMessage@XbtpSession@@QEAAJHAEBUXBTP_MESSAGE_BODY@@@Z"]
pub unsafe extern "C" fn XbtpSession_SendMoreMessage(this: *mut XbtpSession, exchange_id: i32, body: *const XBTP_MESSAGE_BODY) -> HRESULT {
    XbtpSession_SendMoreMessageCallback(this, exchange_id, body, None, null_mut())
}

#[export_name = "?SendEndMessage@XbtpSession@@QEAAJHW4XBTP_STATUS_CODE@@AEBUXBTP_MESSAGE_BODY@@@Z"]
pub unsafe extern "C" fn XbtpSession_SendEndMessage(this: *mut XbtpSession, exchange_id: i32, status: i32, body: *const XBTP_MESSAGE_BODY) -> HRESULT {
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    send_ext_impl(this, XBTP_MESSAGE_TYPE_END, status as u32, exchange_id, body, None, null_mut())
}

#[export_name = "?ReceiveOnewayMessage@XbtpSession@@QEAAJKPEAV?$shared_ptr@UXBTP_MESSAGE_BODY@@@std@@@Z"]
pub unsafe extern "C" fn XbtpSession_ReceiveOnewayMessageTimeout(this: *mut XbtpSession, timeout: u32, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    let mut msg = XBTP_BEGIN_MESSAGE::default();
    receive_ext_impl(this, XBTP_MESSAGE_TYPE_ONEWAY, timeout, &mut msg, body)
}

#[export_name = "?ReceiveOnewayMessage@XbtpSession@@QEAAJPEAV?$shared_ptr@UXBTP_MESSAGE_BODY@@@std@@@Z"]
pub unsafe extern "C" fn XbtpSession_ReceiveOnewayMessage(this: *mut XbtpSession, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    XbtpSession_ReceiveOnewayMessageTimeout(this, DEFAULT_RECEIVE_TIMEOUT_MS, body)
}

#[export_name = "?ReceiveBeginMessage@XbtpSession@@QEAAJKPEAHPEAV?$shared_ptr@UXBTP_MESSAGE_BODY@@@std@@@Z"]
pub unsafe extern "C" fn XbtpSession_ReceiveBeginMessageTimeout(this: *mut XbtpSession, timeout: u32, exchange_id: *mut i32, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    if exchange_id.is_null() {
        return E_INVALIDARG;
    }
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    let mut msg = XBTP_BEGIN_MESSAGE::default();
    let hr = receive_ext_impl(this, XBTP_MESSAGE_TYPE_BEGIN, timeout, &mut msg, body);
    if hr >= 0 {
        *exchange_id = msg.exchange_id;
    }
    hr
}

#[export_name = "?ReceiveBeginMessage@XbtpSession@@QEAAJPEAHPEAV?$shared_ptr@UXBTP_MESSAGE_BODY@@@std@@@Z"]
pub unsafe extern "C" fn XbtpSession_ReceiveBeginMessage(this: *mut XbtpSession, exchange_id: *mut i32, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    XbtpSession_ReceiveBeginMessageTimeout(this, DEFAULT_RECEIVE_TIMEOUT_MS, exchange_id, body)
}

unsafe fn verify_exchange(expected: i32, actual: i32) -> HRESULT {
    if expected == actual {
        return S_OK;
    }
    log_error_args(b"Received message with wrong exchange Id.  (Expected %d actual %d)\0", &[expected as usize, actual as usize]);
    E_UNEXPECTED_EXCHANGE_ID
}

#[export_name = "?ReceiveMoreMessage@XbtpSession@@QEAAJKHPEAV?$shared_ptr@UXBTP_MESSAGE_BODY@@@std@@@Z"]
pub unsafe extern "C" fn XbtpSession_ReceiveMoreMessageTimeout(this: *mut XbtpSession, timeout: u32, exchange_id: i32, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    let mut msg = XBTP_BEGIN_MESSAGE::default();
    let hr = receive_ext_impl(this, XBTP_MESSAGE_TYPE_MORE, timeout, &mut msg, body);
    if hr < 0 {
        return hr;
    }
    verify_exchange(exchange_id, msg.exchange_id)
}

#[export_name = "?ReceiveMoreMessage@XbtpSession@@QEAAJHPEAV?$shared_ptr@UXBTP_MESSAGE_BODY@@@std@@@Z"]
pub unsafe extern "C" fn XbtpSession_ReceiveMoreMessage(this: *mut XbtpSession, exchange_id: i32, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    XbtpSession_ReceiveMoreMessageTimeout(this, DEFAULT_RECEIVE_TIMEOUT_MS, exchange_id, body)
}

#[export_name = "?ReceiveEndMessage@XbtpSession@@QEAAJKHPEAW4XBTP_STATUS_CODE@@PEAV?$shared_ptr@UXBTP_MESSAGE_BODY@@@std@@@Z"]
pub unsafe extern "C" fn XbtpSession_ReceiveEndMessageTimeout(this: *mut XbtpSession, timeout: u32, exchange_id: i32, status: *mut i32, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    if status.is_null() || body.is_null() {
        return E_INVALIDARG;
    }
    SharedPtr::take(body).release();
    *status = XBTP_STATUS_SERVER_AGENT_ERROR;
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    let mut msg = XBTP_BEGIN_MESSAGE::default();
    let hr = receive_ext_impl(this, XBTP_MESSAGE_TYPE_END, timeout, &mut msg, body);
    if hr < 0 {
        return hr;
    }
    let hr = verify_exchange(exchange_id, msg.exchange_id);
    if hr >= 0 {
        *status = msg.operation_id as i32;
    }
    hr
}

#[export_name = "?ReceiveEndMessage@XbtpSession@@QEAAJHPEAW4XBTP_STATUS_CODE@@PEAV?$shared_ptr@UXBTP_MESSAGE_BODY@@@std@@@Z"]
pub unsafe extern "C" fn XbtpSession_ReceiveEndMessage(this: *mut XbtpSession, exchange_id: i32, status: *mut i32, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    XbtpSession_ReceiveEndMessageTimeout(this, DEFAULT_RECEIVE_TIMEOUT_MS, exchange_id, status, body)
}

#[export_name = "?ReceiveMessage@XbtpSession@@QEAAJKHPEAFPEAW4XBTP_STATUS_CODE@@PEAV?$shared_ptr@UXBTP_MESSAGE_BODY@@@std@@@Z"]
pub unsafe extern "C" fn XbtpSession_ReceiveMessage(this: *mut XbtpSession, timeout: u32, exchange_id: i32, message_type: *mut i16, status: *mut i32, body: *mut SharedPtr<XBTP_MESSAGE_BODY>) -> HRESULT {
    if body.is_null() || message_type.is_null() || status.is_null() {
        return E_INVALIDARG;
    }
    SharedPtr::take(body).release();
    *status = XBTP_STATUS_SERVER_AGENT_ERROR;
    let hr = verify_open(this);
    if hr < 0 {
        return hr;
    }
    let mut msg = XBTP_BEGIN_MESSAGE::default();
    let hr = receive_core(this, timeout, &mut msg, body);
    if hr < 0 {
        if hr == E_PENDING {
            log_info_args(b"Receive of message is pending...\0", &[]);
        } else {
            log_error_args(b"Failed 0x%X to receive message\0", &[hr as u32 as usize]);
        }
        return hr;
    }
    let hr = verify_exchange(exchange_id, msg.exchange_id);
    if hr < 0 {
        SharedPtr::take(body).release();
        return hr;
    }
    *message_type = msg.header.message_type as i16;
    *status = msg.operation_id as i32;
    S_OK
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::conn::tests::{init, np_pair, tcp_pair, NET_LOCK};
    use crate::conn::{channel_delete, connection_delete, XbtpConnection};

    unsafe fn drop_channel(p: *mut c_void) {
        channel_delete(p as *mut XbtpChannel);
    }

    pub(crate) unsafe fn wrap_channel(ch: *mut XbtpChannel) -> SharedPtr<XbtpChannel> {
        SharedPtr::new_with(ch, drop_channel).unwrap()
    }

    pub(crate) const SID: GUID = GUID::new(0x11223344, 0x5566, 0x7788, [1, 2, 3, 4, 5, 6, 7, 8]);

    pub(crate) unsafe fn open(ch: *mut XbtpChannel, id: GUID) -> *mut XbtpSession {
        let s = new_session();
        let mut a = SharedPtr::<XbtpClientAdapter>::default();
        let mut c = wrap_channel(ch);
        assert_eq!(XbtpSession_Initialize(s, &id, &mut a, &mut c), S_OK);
        core::mem::forget(c);
        s
    }

    fn body(json: &mut Vec<u8>, bin: &mut Vec<u8>) -> XBTP_MESSAGE_BODY {
        XBTP_MESSAGE_BODY {
            json_body_length: json.len() as u32,
            binary_body_length: bin.len() as u32,
            json_body: if json.is_empty() { null_mut() } else { json.as_mut_ptr() },
            binary_body: if bin.is_empty() { null_mut() } else { bin.as_mut_ptr() },
        }
    }

    unsafe fn bytes(p: *mut u8, n: u32) -> Vec<u8> {
        core::slice::from_raw_parts(p, n as usize).to_vec()
    }

    #[test]
    fn ctor_and_layout() {
        assert_eq!(core::mem::size_of::<XbtpSession>(), 0x38);
        assert_eq!(core::mem::offset_of!(XbtpSession, session_id), 8);
        assert_eq!(core::mem::offset_of!(XbtpSession, channel), 0x18);
        assert_eq!(core::mem::offset_of!(XbtpSession, adapter), 0x28);
        unsafe {
            let s = new_session();
            assert!((*s).channel.is_null() && (*s).adapter.is_null());
            let mut m = XBTP_MESSAGE_BODY::default();
            assert_eq!(XbtpSession_SendOnewayMessage(s, 1, &mut m), E_SESSION_CLOSED);
            let mut sp = SharedPtr::default();
            assert_eq!(XbtpSession_ReceiveOnewayMessage(s, &mut sp), E_SESSION_CLOSED);
            session_delete(s);
        }
    }

    #[test]
    fn initialize_requires_channel_and_releases_arguments() {
        unsafe {
            let s = new_session();
            let mut a = SharedPtr::<XbtpClientAdapter>::default();
            let mut c = SharedPtr::<XbtpChannel>::default();
            assert_eq!(XbtpSession_Initialize(s, &SID, &mut a, &mut c), E_INVALIDARG);
            let _g = NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            init();
            let (srv, cc, sc) = tcp_pair(47811);
            let mut c = wrap_channel(cc);
            let keep = c.clone_ref();
            assert_eq!(XbtpSession_Initialize(s, &SID, &mut a, &mut c), S_OK);
            assert!(c.is_null());
            assert_eq!(keep.use_count(), 2);
            assert_eq!((*s).session_id, SID);
            session_delete(s);
            assert_eq!(keep.use_count(), 1);
            drop(keep);
            channel_delete(sc);
            connection_delete(srv as *mut XbtpConnection);
        }
    }

    #[test]
    fn status_text_matches_console() {
        let t = |s| String::from_utf16_lossy(&status_code_text(s).unwrap()).trim_end_matches('\0').to_string();
        assert_eq!(t(0), "Status OK");
        assert_eq!(t(-6), "The extension was redirected");
        assert_eq!(t(-8), "The extension encountered an error");
        assert_eq!(t(5), "The status code (5) is not recognized");
        assert_eq!(t(-9), "The status code (FFFFFFF7) is not recognized");
        assert_eq!(status_error(-6), E_REDIRECTED);
        assert_eq!(unexpected_message_error(-4), E_UNEXPECTED_CLOSE_SESSION);
    }

    #[test]
    fn message_log_json_shape() {
        unsafe {
            let mut j = b"{\"a\":1}".to_vec();
            let mut b = Vec::new();
            let m = XBTP_BEGIN_MESSAGE {
                header: XBTP_MESSAGE_HEADER { signature: 0x5850, message_type: 4, message_length: 0x27 },
                session_id: SID,
                exchange_id: -3,
                operation_id: 7,
                body: body(&mut j, &mut b),
            };
            let s = String::from_utf8(message_json(&m).unwrap()).unwrap();
            for k in ["\"Signature\":22608", "\"MessageType\":4", "\"P1\":-3", "\"P2\":7", "\"JsonBody\":{\"a\":1}", "\"Data4\":[1,2,3,4,5,6,7,8]", "\"BinaryBodyLength\":0"] {
                assert!(s.contains(k), "{} missing in {}", k, s);
            }
            let mut bad = b"nope".to_vec();
            let m2 = XBTP_BEGIN_MESSAGE { body: body(&mut bad, &mut b), ..m };
            assert!(message_json(&m2).is_none());
        }
    }

    unsafe fn exchange_round_trip(cc: *mut XbtpChannel, sc: *mut XbtpChannel) {
        let client = open(cc, SID);
        let server = open(sc, SID);
        let mut j = b"{\"x\":true}".to_vec();
        let mut bin = vec![9u8, 8, 7];
        let b = body(&mut j, &mut bin);
        let mut exch = 0;
        assert_eq!(XbtpSession_SendBeginMessage(client, 0x42, &b, &mut exch), S_OK);
        assert_eq!(exch, 1);
        let mut got = SharedPtr::default();
        let mut rexch = 0;
        assert_eq!(XbtpSession_ReceiveBeginMessageTimeout(server, 5000, &mut rexch, &mut got), S_OK);
        assert_eq!(rexch, 1);
        assert_eq!(bytes((*got.ptr).json_body, (*got.ptr).json_body_length), j);
        assert_eq!(bytes((*got.ptr).binary_body, (*got.ptr).binary_body_length), bin);
        let mut e = Vec::new();
        let mut e2 = Vec::new();
        let empty = body(&mut e, &mut e2);
        assert_eq!(XbtpSession_SendMoreMessage(server, rexch, &empty), S_OK);
        let mut more = SharedPtr::default();
        assert_eq!(XbtpSession_ReceiveMoreMessageTimeout(client, 5000, exch, &mut more), S_OK);
        assert!((*more.ptr).json_body.is_null() && (*more.ptr).binary_body.is_null());
        assert_eq!(XbtpSession_SendMoreMessage(server, rexch, &empty), S_OK);
        assert_eq!(XbtpSession_ReceiveMoreMessageTimeout(client, 5000, exch + 5, &mut more), E_UNEXPECTED_EXCHANGE_ID);
        assert_eq!(XbtpSession_SendEndMessage(server, rexch, -5, &empty), S_OK);
        let mut st = 0;
        let mut end = SharedPtr::default();
        assert_eq!(XbtpSession_ReceiveEndMessageTimeout(client, 5000, exch, &mut st, &mut end), S_OK);
        assert_eq!(st, -5);
        assert_eq!(XbtpSession_SendOnewayMessage(client, 9, &b), S_OK);
        assert_eq!(XbtpSession_ReceiveBeginMessageTimeout(server, 5000, &mut rexch, &mut got), unexpected_message_error(1));
        assert!(got.is_null());
        assert_eq!(XbtpSession_SendOnewayMessage(client, 9, &empty), S_OK);
        let mut ty = 0i16;
        assert_eq!(XbtpSession_ReceiveMessage(server, 5000, 0, &mut ty, &mut st, &mut got), S_OK);
        assert_eq!((ty, st), (1, 9));
        assert_eq!(XbtpSession_ReceiveMessage(client, 50, 0, &mut ty, &mut st, &mut got), E_PENDING);
        assert_eq!(st, XBTP_STATUS_SERVER_AGENT_ERROR);
        let mut sexch = 0;
        assert_eq!(XbtpSession_SendBeginMessage(server, 1, &empty, &mut sexch), S_OK);
        assert_eq!(sexch, -1);
        assert_eq!(XbtpSession_ReceiveMessage(client, 5000, 3, &mut ty, &mut st, &mut got), E_UNEXPECTED_EXCHANGE_ID);
        assert!(got.is_null());
        drop((got, more, end));
        session_delete(client);
        session_delete(server);
    }

    #[test]
    fn tcp_send_receive_exchange() {
        let _g = NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init();
        unsafe {
            let (srv, cc, sc) = tcp_pair(47812);
            exchange_round_trip(cc, sc);
            connection_delete(srv as *mut XbtpConnection);
        }
    }

    #[test]
    fn pipe_send_receive_exchange() {
        let _g = NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init();
        unsafe {
            let (srv, cc, sc) = np_pair(r"\\.\pipe\xbtp_session_test");
            exchange_round_trip(cc, sc);
            connection_delete(srv as *mut XbtpConnection);
        }
    }

    unsafe fn raw_send(ch: *mut XbtpChannel, data: &[u8]) {
        let mut v = data.to_vec();
        assert_eq!(channel_send(ch, &[WSABUF { len: v.len() as u32, buf: v.as_mut_ptr() }], None, null_mut()), S_OK);
    }

    #[test]
    fn close_disconnect_and_wrong_session() {
        let _g = NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init();
        unsafe {
            let (srv, cc, sc) = tcp_pair(47813);
            let s = open(cc, SID);
            let other = GUID::new(9, 9, 9, [9; 8]);
            raw_send(sc, &build_extension_message(0x5850, 1, &other, 0, 1, b"", b"").unwrap());
            let mut got = SharedPtr::default();
            assert_eq!(XbtpSession_ReceiveOnewayMessageTimeout(s, 5000, &mut got), E_SESSION_NOT_FOUND);
            raw_send(sc, &build_close_session(0x5850, &other).unwrap());
            assert_eq!(XbtpSession_ReceiveOnewayMessageTimeout(s, 5000, &mut got), E_UNEXPECTED_CLOSE_SESSION);
            raw_send(sc, &build_disconnect(0x5850, -2).unwrap());
            assert_eq!(XbtpSession_ReceiveOnewayMessageTimeout(s, 5000, &mut got), status_error(-2));
            raw_send(sc, &build_create_session_request(0x5850, &SID, &other).unwrap());
            assert_eq!(XbtpSession_ReceiveOnewayMessageTimeout(s, 5000, &mut got), unexpected_message_error(-2));
            let mut rest = [0u8; 0x20];
            assert_eq!(channel_receive(cc, 5000, 0x20, rest.as_mut_ptr()), S_OK);
            raw_send(sc, &build_close_session(0x5850, &SID).unwrap());
            assert_eq!(XbtpSession_ReceiveOnewayMessageTimeout(s, 5000, &mut got), E_SESSION_CLOSED);
            assert!((*s).channel.is_null());
            let mut e = XBTP_MESSAGE_BODY::default();
            assert_eq!(XbtpSession_SendOnewayMessage(s, 1, &mut e), E_SESSION_CLOSED);
            session_delete(s);
            let s2 = open(sc, SID);
            session_delete(s2);
            connection_delete(srv as *mut XbtpConnection);
        }
    }

    #[test]
    fn null_argument_checks() {
        unsafe {
            let s = new_session();
            let mut sp = SharedPtr::default();
            let mut st = 1;
            let mut ty = 0i16;
            let e = XBTP_MESSAGE_BODY::default();
            assert_eq!(XbtpSession_SendBeginMessage(s, 1, &e, null_mut()), E_INVALIDARG);
            assert_eq!(XbtpSession_ReceiveBeginMessage(s, null_mut(), &mut sp), E_INVALIDARG);
            assert_eq!(XbtpSession_ReceiveEndMessage(s, 1, null_mut(), &mut sp), E_INVALIDARG);
            assert_eq!(XbtpSession_ReceiveEndMessage(s, 1, &mut st, &mut sp), E_SESSION_CLOSED);
            assert_eq!(st, -7);
            assert_eq!(XbtpSession_ReceiveMessage(s, 1, 0, null_mut(), &mut st, &mut sp), E_INVALIDARG);
            assert_eq!(XbtpSession_ReceiveMessage(s, 1, 0, &mut ty, &mut st, &mut sp), E_SESSION_CLOSED);
            let mut h = null_mut();
            assert_eq!(session_get_event(s, &mut h), E_SESSION_CLOSED);
            assert_eq!(h, INVALID_HANDLE_VALUE);
            session_delete(s);
        }
    }
}
