use crate::com::*;
use crate::json::*;
use crate::server::*;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

#[link(name = "kernel32", kind = "raw-dylib")]
extern "system" {
    fn CreateEventW(sa: *mut c_void, manual: i32, initial: i32, name: *const u16) -> HANDLE;
    fn SetEvent(h: HANDLE) -> i32;
    fn CloseHandle(h: HANDLE) -> i32;
}

struct Vt<const N: usize>([*const (); N]);
unsafe impl<const N: usize> Sync for Vt<N> {}

type HandlerVt = crate::server::HandlerVtbl;
type ServerVt = crate::server::ServerVtbl;

#[repr(C)]
struct MockSession {
    vtbl: *const Vt<7>,
    refs: AtomicU32,
    sent: Mutex<Vec<String>>,
    event: HANDLE,
}

unsafe extern "system" fn ms_qi(_: *mut c_void, _: *const GUID, _: *mut *mut c_void) -> HRESULT {
    E_NOINTERFACE
}

unsafe extern "system" fn ms_add_ref(this: *mut c_void) -> u32 {
    (*(this as *mut MockSession)).refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn ms_release(this: *mut c_void) -> u32 {
    (*(this as *mut MockSession)).refs.fetch_sub(1, Ordering::AcqRel) - 1
}

unsafe extern "system" fn ms_send_more(this: *mut c_void, msg: *mut c_void) -> HRESULT {
    let s = &*(this as *mut MockSession);
    s.sent.lock().unwrap().push(body_text(msg));
    S_OK
}

unsafe extern "system" fn ms_slot4(_: *mut c_void, _: usize, _: usize, _: usize) -> HRESULT {
    E_UNEXPECTED
}

unsafe extern "system" fn ms_receive_more(_: *mut c_void, timeout: u32, out: *mut *mut c_void) -> HRESULT {
    assert_eq!(timeout, 0);
    *out = null_mut();
    E_PENDING
}

unsafe extern "system" fn ms_get_event(this: *mut c_void, out: *mut HANDLE) -> HRESULT {
    *out = (*(this as *mut MockSession)).event;
    S_OK
}

static SESSION_VTBL: Vt<7> = Vt([
    ms_qi as *const (),
    ms_add_ref as *const (),
    ms_release as *const (),
    ms_send_more as *const (),
    ms_slot4 as *const (),
    ms_receive_more as *const (),
    ms_get_event as *const (),
]);

impl MockSession {
    fn new(signaled: bool) -> Box<MockSession> {
        let event = unsafe { CreateEventW(null_mut(), 1, signaled as i32, core::ptr::null()) };
        assert!(!event.is_null());
        Box::new(MockSession { vtbl: &SESSION_VTBL, refs: AtomicU32::new(1), sent: Mutex::new(Vec::new()), event })
    }

    fn ptr(&mut self) -> *mut c_void {
        self as *mut MockSession as *mut c_void
    }

    fn sent(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }
}

impl Drop for MockSession {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.event) };
    }
}

#[repr(C)]
struct EndMessage {
    vtbl: *const Vt<8>,
    refs: AtomicU32,
    inner: *mut c_void,
}

unsafe fn inner(this: *mut c_void) -> *mut c_void {
    (*(this as *mut EndMessage)).inner
}

unsafe extern "system" fn em_get_json(this: *mut c_void, n: *mut u32, b: *mut *mut u8) -> HRESULT {
    msg_get_json_body(inner(this), n, b)
}

unsafe extern "system" fn em_get_binary(this: *mut c_void, n: *mut u32, b: *mut *mut u8) -> HRESULT {
    let i = inner(this);
    let f: unsafe extern "system" fn(*mut c_void, *mut u32, *mut *mut u8) -> HRESULT = core::mem::transmute(slot(i, 4));
    f(i, n, b)
}

unsafe extern "system" fn em_set_json(this: *mut c_void, n: u32, b: *const u8) -> HRESULT {
    let i = inner(this);
    let f: unsafe extern "system" fn(*mut c_void, u32, *const u8) -> HRESULT = core::mem::transmute(slot(i, 5));
    f(i, n, b)
}

unsafe extern "system" fn em_set_json_w(this: *mut c_void, s: *const u16) -> HRESULT {
    msg_set_json_body_w(inner(this), SET_JSON_BODY_W_REQUEST, s)
}

unsafe extern "system" fn em_set_binary(this: *mut c_void, n: u32, b: *const u8) -> HRESULT {
    let i = inner(this);
    let f: unsafe extern "system" fn(*mut c_void, u32, *const u8) -> HRESULT = core::mem::transmute(slot(i, 8));
    f(i, n, b)
}

static END_VTBL: Vt<8> = Vt([
    ms_qi as *const (),
    ms_add_ref as *const (),
    ms_release as *const (),
    em_get_json as *const (),
    em_get_binary as *const (),
    em_set_json as *const (),
    em_set_json_w as *const (),
    em_set_binary as *const (),
]);

impl EndMessage {
    fn new() -> Box<EndMessage> {
        let mut m: *mut c_void = null_mut();
        assert_eq!(unsafe { XbtpCreateMessage(&IID_MESSAGE, &mut m) }, S_OK);
        Box::new(EndMessage { vtbl: &END_VTBL, refs: AtomicU32::new(1), inner: m })
    }

    fn ptr(&mut self) -> *mut c_void {
        self as *mut EndMessage as *mut c_void
    }

    fn text(&self) -> String {
        unsafe { body_text(self.inner) }
    }
}

impl Drop for EndMessage {
    fn drop(&mut self) {
        unsafe { com_release(self.inner) };
    }
}

unsafe fn body_text(msg: *mut c_void) -> String {
    let mut n = 0u32;
    let mut p: *mut u8 = null_mut();
    assert_eq!(msg_get_json_body(msg, &mut n, &mut p), S_OK);
    if p.is_null() {
        return String::new();
    }
    let s = core::slice::from_raw_parts(p, n as usize);
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf8(s[..end].to_vec()).unwrap()
}

fn request(json: &str) -> ComPtr {
    let mut m = ComPtr::null();
    unsafe {
        assert_eq!(XbtpCreateMessage(&IID_MESSAGE, m.out()), S_OK);
        assert_eq!(msg_set_json_body_w(m.0, SET_JSON_BODY_W_REQUEST, w(json).as_ptr()), S_OK);
    }
    m
}

fn empty_request() -> ComPtr {
    let mut m = ComPtr::null();
    unsafe { assert_eq!(XbtpCreateMessage(&IID_MESSAGE, m.out()), S_OK) };
    m
}

fn parse(text: &str) -> ComPtr {
    let mut o = ComPtr::null();
    let mut z = text.as_bytes().to_vec();
    z.push(0);
    unsafe { assert_eq!(XbtpGetJsonObjectFromUtf8(z.as_ptr(), o.out()), S_OK) };
    o
}

fn num(o: &ComPtr, name: &str) -> f64 {
    let mut d = 0.0;
    unsafe { assert_eq!(obj_get_number(o.0, w(name).as_ptr(), &mut d), S_OK, "{name}") };
    d
}

fn string(o: &ComPtr, name: &str) -> String {
    let mut b = Bstr::null();
    unsafe {
        assert_eq!(obj_get_string(o.0, w(name).as_ptr(), &mut b.0), S_OK, "{name}");
        String::from_utf16(xtfcommon::sys::wslice(b.0)).unwrap()
    }
}

fn u64_prop(o: &ComPtr, name: &str) -> u64 {
    let mut v = 0u64;
    unsafe { assert_eq!(get_u64(o.0, name, &mut v), S_OK, "{name}") };
    v
}

fn sub(o: &ComPtr, name: &str) -> ComPtr {
    let mut s = ComPtr::null();
    unsafe { assert_eq!(obj_get_object(o.0, w(name).as_ptr(), s.out()), S_OK, "{name}") };
    s
}

fn operation_error(reply: &EndMessage) -> (i32, u32, Vec<String>) {
    let o = parse(&reply.text());
    let e = sub(&o, "XtfOperationError");
    let mut arr = ComPtr::null();
    let mut out = Vec::new();
    unsafe {
        let ga: unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> HRESULT = core::mem::transmute(slot(e.0, 10));
        assert_eq!(ga(e.0, w("Errors").as_ptr(), arr.out()), S_OK);
        let count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT = core::mem::transmute(slot(arr.0, 11));
        let mut n = 0u32;
        assert_eq!(count(arr.0, &mut n), S_OK);
        let gs: unsafe extern "system" fn(*mut c_void, u32, *mut BSTR) -> HRESULT = core::mem::transmute(slot(arr.0, 8));
        for i in 0..n {
            let mut b = Bstr::null();
            assert_eq!(gs(arr.0, i, &mut b.0), S_OK);
            out.push(String::from_utf16(xtfcommon::sys::wslice(b.0)).unwrap());
        }
    }
    (num(&e, "ReturnCode") as i32, num(&e, "Checkpoint") as u32, out)
}

fn err_line(hr: HRESULT, line: u32) -> String {
    format!("Error 0x{:X} File xbox\\devex\\xtf\\dll\\pix\\server.cpp : {}", hr as u32, line)
}

struct Server(*mut c_void);

impl Server {
    fn new() -> Server {
        let mut p: *mut c_void = null_mut();
        assert_eq!(unsafe { XtfCreatePixServer(&IID_PIX_SERVER, &mut p) }, S_OK);
        assert!(!p.is_null());
        Server(p)
    }

    fn invoke(&self, op: u32, session: *mut c_void, received: *mut c_void, reply: *mut c_void) -> HRESULT {
        unsafe {
            let h = handler_ptr(self.0, op);
            assert!(!h.is_null());
            let v = &*(*(h as *const *const HandlerVt));
            (v.invoke)(h as *mut _, session, received, reply)
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        assert_eq!(unsafe { com_release(self.0) }, 0);
    }
}

#[test]
fn export_table_matches_console_dll() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/exports.txt")).unwrap();
    xtfcommon::exports::check_against_titles("xtfpix.dll", &text);
    for e in xtfcommon::exports::parse(&text) {
        if e.status != "missing" {
            assert!(xtfcommon::exports::source_defines(concat!(env!("CARGO_MANIFEST_DIR"), "/src"), &e), "{}", e.name);
        }
    }
}

#[test]
fn json_number_conversions_match_cvttsd2si() {
    assert_eq!(cvtt64(3.9), 3);
    assert_eq!(cvtt64(-3.9), -3);
    assert_eq!(cvtt64(f64::NAN), i64::MIN);
    assert_eq!(cvtt64(1e19), i64::MIN);
    assert_eq!(cvtt32(4294967295.0), i32::MIN);
    assert_eq!(cvtt32(-2147483648.5), -2147483648);
    assert_eq!(cvtt32(-2147483649.0), i32::MIN);
    unsafe {
        let mut o = ComPtr::null();
        assert_eq!(XbtpCreateJsonObject(o.out()), S_OK);
        assert_eq!(set_u64(o.0, "T", 0xFFFF_FFFF_8000_0001), S_OK);
        let t = sub(&o, "T");
        assert_eq!(num(&t, "L"), 2147483649.0);
        assert_eq!(num(&t, "H"), -1.0);
        assert_eq!(u64_prop(&o, "T"), 0xFFFF_FFFF_8000_0001);
        let mut v = 7u64;
        assert_eq!(get_u64(o.0, "Missing", &mut v), E_JSON_NOT_FOUND);
        assert_eq!(v, 7);
        assert_eq!(set_dword(o.0, "D", 0xFFFF_FFFF), S_OK);
        assert_eq!(num(&o, "D"), 4294967295.0);
        assert_eq!(set_int(o.0, "I", -5), S_OK);
        let mut d = 0u32;
        assert_eq!(get_dword(o.0, "I", &mut d), S_OK);
        assert_eq!(d, 0xFFFF_FFFB);
        let mut b = 0i32;
        assert_eq!(get_bool(o.0, "D", &mut b), S_OK);
        assert_eq!(b, i32::MIN);
        assert_eq!(get_dword(null_mut(), "D", &mut d), E_INVALIDARG);
        assert_eq!(set_u64(null_mut(), "D", 1), E_INVALIDARG);
        assert_eq!(serialize(null_mut(), o.0, 6), E_INVALIDARG);
        let mut body = ComPtr::null();
        assert_eq!(deserialize(null_mut(), &mut body), E_INVALIDARG);
    }
}

#[test]
fn entry_point_checks_extension_and_metadata_ids() {
    unsafe {
        let mut p: *mut c_void = null_mut();
        assert_eq!(XbtpServerEntryPoint(&IID_PIX_SERVER, &IID_EXTENSION_METADATA, &mut p), E_UNEXPECTED);
        assert_eq!(XbtpServerEntryPoint(&PIX_EXTENSION_ID, &IID_PIX_SERVER, &mut p), E_INVALIDARG);
        assert!(p.is_null());
        assert_eq!(XbtpServerEntryPoint(&PIX_EXTENSION_ID, &IID_EXTENSION_METADATA, &mut p), S_OK);
        assert!(!p.is_null());
        let count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT = core::mem::transmute(slot(p, 3));
        let mut n = 0u32;
        assert_eq!(count(p, &mut n), S_OK);
        assert_eq!(n, 11);
        let get: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT = core::mem::transmute(slot(p, 4));
        let mut hs = [null_mut::<c_void>(); 11];
        assert_eq!(get(p, 88, hs.as_mut_ptr()), S_OK);
        let mut ops: Vec<u32> = hs
            .iter()
            .map(|&h| {
                let v = &*(*(h as *const *const HandlerVt));
                let mut id = 0u32;
                assert_eq!((v.get_operation_id)(h as *mut _, &mut id), S_OK);
                let mut q: *mut c_void = null_mut();
                assert_eq!((v.query_interface)(h as *mut _, &IID_OPERATION_HANDLER, &mut q), S_OK);
                assert_eq!(q, h);
                com_release(q);
                assert_eq!((v.query_interface)(h as *mut _, &IID_PIX_SERVER, &mut q), E_NOINTERFACE);
                id
            })
            .collect();
        ops.sort();
        let mut want = HANDLER_OPS.to_vec();
        want.sort();
        assert_eq!(ops, want);
        assert_eq!(com_release(p), 0);
    }
}

#[test]
fn pix_server_interfaces_and_lifetime() {
    unsafe {
        let mut p: *mut c_void = null_mut();
        assert_eq!(XtfCreatePixServer(&IID_OPERATION_HANDLER, &mut p), E_NOINTERFACE);
        assert!(p.is_null());
        let s = Server::new();
        assert_eq!(sysmon_buffer_len(s.0), SYSMON_DATA_BUFFER_SIZE_BYTES);
        let v = &*(*(s.0 as *const *const ServerVt));
        let mut n = 0u32;
        assert_eq!((v.get_handler_count)(s.0 as *mut _, &mut n), S_OK);
        assert_eq!(n, 11);
        assert_eq!((v.add_handler)(s.0 as *mut _, handler_ptr(s.0, OP_PING)), 0x8007_1392u32 as i32);
        let mut m: *mut c_void = null_mut();
        assert_eq!(com_qi(s.0, &IID_EXTENSION_METADATA, &mut m), S_OK);
        assert!(!m.is_null() && m != s.0);
        assert_eq!(com_add_ref(s.0), 3);
        assert_eq!(com_release(s.0), 2);
        assert_eq!(com_release(s.0), 1);
        let mut u: *mut c_void = null_mut();
        assert_eq!(com_qi(s.0, &IID_IUNKNOWN, &mut u), S_OK);
        assert_eq!(u, s.0);
        assert_eq!(com_release(s.0), 1);
        assert_eq!(com_qi(s.0, &IID_MESSAGE, &mut u), E_NOINTERFACE);
    }
}

#[test]
fn version_handshake_reports_server_version_and_clock() {
    let s = Server::new();
    let mut reply = EndMessage::new();
    let req = request("{\"Version\":7}");
    assert_eq!(s.invoke(OP_VERSION_HANDSHAKE, null_mut(), req.0, reply.ptr()), S_OK);
    let o = parse(&reply.text());
    assert_eq!(num(&o, "Version"), PIX_SERVER_VERSION as f64);
    assert_eq!(num(&o, "Result"), 0.0);
    let mut freq = 0i64;
    unsafe { QueryPerformanceFrequency(&mut freq) };
    assert_eq!(u64_prop(&o, "CpuFrequency"), freq as u64);
    assert_eq!(num(&o, "SystemType"), unsafe { XGetSystemType() } as u16 as f64);
    let mut bad = EndMessage::new();
    let req = request("{}");
    assert_eq!(s.invoke(OP_VERSION_HANDSHAKE, null_mut(), req.0, bad.ptr()), E_JSON_NOT_FOUND);
    assert_eq!(operation_error(&bad), (E_JSON_NOT_FOUND, 0, vec![err_line(E_JSON_NOT_FOUND, 0x3db)]));
    let mut empty = EndMessage::new();
    let req = empty_request();
    assert_eq!(s.invoke(OP_VERSION_HANDSHAKE, null_mut(), req.0, empty.ptr()), E_JSON_NOT_FOUND);
}

#[test]
fn friendly_version_strings() {
    let s = Server::new();
    let mut reply = EndMessage::new();
    let req = empty_request();
    assert_eq!(s.invoke(OP_GET_FRIENDLY_VERSION, null_mut(), req.0, reply.ptr()), S_OK);
    let o = parse(&reply.text());
    assert_eq!(string(&o, "FriendlyVersionProperty"), "June 2022 XDK Update 4");
    assert_eq!(string(&o, "FriendlyCompactVersionProperty"), "June 2022 Update 4");
    assert_eq!(num(&o, "XDKEditionProperty"), 220604.0);
    assert_eq!(string(&o, "BuildBranchProperty"), "xb_gdk_2206co");
}

#[test]
fn exclusive_connection_token_and_ping() {
    let s = Server::new();
    let create = |json: &str| {
        let mut reply = EndMessage::new();
        let req = request(json);
        assert_eq!(s.invoke(OP_CREATE_EXCLUSIVE_CONNECTION, null_mut(), req.0, reply.ptr()), S_OK);
        let o = parse(&reply.text());
        (num(&o, "Result") as i32, u64_prop(&o, "ConnectionToken"))
    };
    assert_eq!(create("{}"), (S_OK, 0));
    assert_ne!(unsafe { server_ticks(s.0) }.0, 0);
    assert_eq!(create("{\"ConnectionToken\":{\"L\":5,\"H\":0}}"), (HR_EXCLUSIVE_CONNECTION_HELD, 5));
    assert_eq!(unsafe { server_ticks(s.0) }.1, 0);
    assert_eq!(create("{\"ForceConnect\":1,\"ConnectionToken\":{\"L\":5,\"H\":0}}"), (S_OK, 5));
    assert_eq!(unsafe { server_ticks(s.0) }.1, 5);
    unsafe { server_set_ticks(s.0, 0, 5) };
    assert_eq!(create("{}"), (S_OK, 6));
    assert_eq!(unsafe { server_ticks(s.0) }.1, 6);
    let ping = |json: &str| {
        let req = request(json);
        s.invoke(OP_PING, null_mut(), req.0, null_mut())
    };
    unsafe { server_set_ticks(s.0, 0, 6) };
    assert_eq!(ping("{\"ConnectionToken\":{\"L\":5,\"H\":0}}"), E_ACCESSDENIED);
    assert_eq!(unsafe { server_ticks(s.0) }.0, 0);
    assert_eq!(ping("{\"ConnectionToken\":{\"L\":6,\"H\":0}}"), S_OK);
    assert_ne!(unsafe { server_ticks(s.0) }.0, 0);
    unsafe { server_set_ticks(s.0, 0, 6) };
    assert_eq!(ping("{}"), S_OK);
    assert_ne!(unsafe { server_ticks(s.0) }.0, 0);
    let mut reply = EndMessage::new();
    let req = request("{\"ConnectionToken\":{\"L\":3,\"H\":0}}");
    assert_eq!(s.invoke(OP_RELEASE_EXCLUSIVE_CONNECTION, null_mut(), req.0, reply.ptr()), S_OK);
    assert_ne!(unsafe { server_ticks(s.0) }.0, 0);
    let req = request("{\"ConnectionToken\":{\"L\":6,\"H\":0}}");
    assert_eq!(s.invoke(OP_RELEASE_EXCLUSIVE_CONNECTION, null_mut(), req.0, reply.ptr()), S_OK);
    assert_eq!(unsafe { server_ticks(s.0) }.0, 0);
    assert_eq!(reply.text(), "");
    assert_eq!(create("{\"ConnectionToken\":{\"L\":9,\"H\":1}}"), (S_OK, 0x1_0000_0009));
    assert_eq!(unsafe { server_ticks(s.0) }.1, 0x1_0000_0009);
    let mut bad = EndMessage::new();
    let req = request("{}");
    assert_eq!(s.invoke(OP_RELEASE_EXCLUSIVE_CONNECTION, null_mut(), req.0, bad.ptr()), E_JSON_NOT_FOUND);
    assert_eq!(operation_error(&bad), (E_JSON_NOT_FOUND, 0, vec![err_line(E_JSON_NOT_FOUND, 0x47a)]));
}

#[test]
fn process_list_tracking_and_clear() {
    let s = Server::new();
    let mut reply = EndMessage::new();
    let req = request("{\"ProcessId\":42,\"ProcessPath\":\"D:\\\\game.exe\"}");
    assert_eq!(s.invoke(OP_PROCESS_CREATED, null_mut(), req.0, reply.ptr()), S_OK);
    let (pid, path) = unsafe { server_process(s.0) };
    assert_eq!(pid, 42);
    assert_eq!(String::from_utf16(&path.unwrap()).unwrap(), "D:\\game.exe");
    let mut bad = EndMessage::new();
    let req = request("{\"ProcessId\":43}");
    assert_eq!(s.invoke(OP_PROCESS_CREATED, null_mut(), req.0, bad.ptr()), E_JSON_NOT_FOUND);
    assert_eq!(operation_error(&bad), (E_JSON_NOT_FOUND, 0, vec![err_line(E_JSON_NOT_FOUND, 0x12b)]));
    assert_eq!(unsafe { server_process(s.0) }.0, 42);
    let mut session = MockSession::new(false);
    let sp = session.ptr() as usize;
    let srv = s.0 as usize;
    let t = std::thread::spawn(move || unsafe {
        let h = handler_ptr(srv as *mut c_void, OP_TRACK_PROCESSES);
        let v = &*(*(h as *const *const HandlerVt));
        (v.invoke)(h as *mut _, sp as *mut c_void, null_mut(), null_mut())
    });
    while session.sent().len() < 2 {
        std::thread::yield_now();
    }
    assert_eq!(session.refs.load(Ordering::Acquire), 2);
    let req = request("{\"ProcessId\":77,\"ProcessPath\":\"x.exe\"}");
    let mut r2 = EndMessage::new();
    assert_eq!(s.invoke(OP_PROCESS_CREATED, null_mut(), req.0, r2.ptr()), S_OK);
    let req = request("{}");
    assert_eq!(s.invoke(OP_CLEAR_RUNNING_PROCESS_LIST, null_mut(), req.0, r2.ptr()), S_OK);
    assert_eq!(unsafe { server_process(s.0) }, (0, None));
    unsafe { SetEvent(session.event) };
    assert_eq!(t.join().unwrap(), S_OK);
    assert_eq!(session.refs.load(Ordering::Acquire), 1);
    let sent: Vec<ComPtr> = session.sent().iter().map(|t| parse(t)).collect();
    assert_eq!(sent.len(), 4);
    assert_eq!(num(&sent[0], "Result"), 0.0);
    assert_eq!((num(&sent[1], "ProcessId"), string(&sent[1], "ProcessPath").as_str(), num(&sent[1], "ProcessCreated")), (42.0, "D:\\game.exe", 1.0));
    assert_eq!((num(&sent[2], "ProcessId"), string(&sent[2], "ProcessPath").as_str(), num(&sent[2], "ProcessCreated")), (77.0, "x.exe", 1.0));
    assert_eq!((num(&sent[3], "ProcessId"), string(&sent[3], "ProcessPath").as_str(), num(&sent[3], "ProcessCreated")), (0.0, "x.exe", 0.0));
    let req = request("{\"ProcessId\":5,\"ProcessPath\":\"\"}");
    assert_eq!(s.invoke(OP_PROCESS_CREATED, null_mut(), req.0, r2.ptr()), S_OK);
    assert_eq!(unsafe { server_process(s.0) }, (5, None));
    assert_eq!(session.sent().len(), 4);
}

#[test]
fn sysmon_configuration_handlers() {
    let s = Server::new();
    let mut reply = EndMessage::new();
    let req = empty_request();
    assert_eq!(s.invoke(OP_GET_SYSMON_CONFIGURATION, null_mut(), req.0, reply.ptr()), S_OK);
    assert_eq!(num(&parse(&reply.text()), "SysMonFlags"), 0.0);
    let mut session = MockSession::new(true);
    let req = request("{\"SysMonFlags\":0}");
    assert_eq!(s.invoke(OP_CONFIGURE_SYSMON, session.ptr(), req.0, reply.ptr()), S_OK);
    assert_eq!(session.sent().len(), 1);
    assert_eq!(num(&parse(&session.sent()[0]), "Result"), 0.0);
    let req = request("{\"SysMonFlags\":3}");
    assert_eq!(s.invoke(OP_CONFIGURE_SYSMON, session.ptr(), req.0, reply.ptr()), S_OK);
    assert_eq!(session.sent().len(), 2);
    assert_eq!(session.refs.load(Ordering::Acquire), 1);
    let mut r2 = EndMessage::new();
    assert_eq!(s.invoke(OP_GET_SYSMON_CONFIGURATION, null_mut(), req.0, r2.ptr()), S_OK);
    assert_eq!(num(&parse(&r2.text()), "SysMonFlags"), 0.0);
    let mut bad = EndMessage::new();
    let req = request("{}");
    assert_eq!(s.invoke(OP_CONFIGURE_SYSMON, session.ptr(), req.0, bad.ptr()), E_JSON_NOT_FOUND);
    assert_eq!(operation_error(&bad), (E_JSON_NOT_FOUND, 0, vec![err_line(E_JSON_NOT_FOUND, 0x211)]));
    let mut tracker = MockSession::new(true);
    assert_eq!(s.invoke(OP_TRACK_SYSMON_CONFIGURATION, tracker.ptr(), null_mut(), null_mut()), S_OK);
    let sent = tracker.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(num(&parse(&sent[0]), "Result"), 0.0);
    assert_eq!(num(&parse(&sent[1]), "SysMonFlags"), 0.0);
    assert_eq!(tracker.refs.load(Ordering::Acquire), 1);
    let req = request("{\"SysMonFlags\":1}");
    assert_eq!(s.invoke(OP_CONFIGURE_SYSMON, session.ptr(), req.0, reply.ptr()), S_OK);
    assert_eq!(tracker.sent().len(), 2);
}

#[test]
fn error_context_keeps_existing_reply_body() {
    let mut reply = EndMessage::new();
    unsafe {
        assert_eq!(msg_set_json_body_w(reply.inner, SET_JSON_BODY_W_REQUEST, w("{\"Keep\":1}").as_ptr()), S_OK);
        let mut ctx = ErrorContext::new(reply.ptr());
        ctx.checkpoint();
        ctx.checkpoint();
        assert_eq!(ctx.fail(E_OUTOFMEMORY, 12), E_OUTOFMEMORY);
        ctx.fail(E_INVALIDARG, 13);
        drop(ctx);
    }
    assert_eq!(num(&parse(&reply.text()), "Keep"), 1.0);
    assert_eq!(operation_error(&reply), (E_INVALIDARG, 2, vec![err_line(E_OUTOFMEMORY, 12), err_line(E_INVALIDARG, 13)]));
    let mut clean = EndMessage::new();
    drop(ErrorContext::new(clean.ptr()));
    assert_eq!(clean.text(), "");
}
