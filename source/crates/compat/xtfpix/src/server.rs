use crate::com::*;
use crate::json::*;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

pub const OP_TRACK_PROCESSES: u32 = 1;
pub const OP_PROCESS_CREATED: u32 = 2;
pub const OP_CLEAR_RUNNING_PROCESS_LIST: u32 = 3;
pub const OP_CONFIGURE_SYSMON: u32 = 6;
pub const OP_GET_SYSMON_CONFIGURATION: u32 = 8;
pub const OP_TRACK_SYSMON_CONFIGURATION: u32 = 13;
pub const OP_PING: u32 = 19;
pub const OP_VERSION_HANDSHAKE: u32 = 30;
pub const OP_CREATE_EXCLUSIVE_CONNECTION: u32 = 37;
pub const OP_RELEASE_EXCLUSIVE_CONNECTION: u32 = 38;
pub const OP_GET_FRIENDLY_VERSION: u32 = 39;

pub const HANDLER_OPS: [u32; 11] = [
    OP_VERSION_HANDSHAKE,
    OP_TRACK_PROCESSES,
    OP_PROCESS_CREATED,
    OP_CLEAR_RUNNING_PROCESS_LIST,
    OP_CONFIGURE_SYSMON,
    OP_GET_SYSMON_CONFIGURATION,
    OP_TRACK_SYSMON_CONFIGURATION,
    OP_PING,
    OP_CREATE_EXCLUSIVE_CONNECTION,
    OP_RELEASE_EXCLUSIVE_CONNECTION,
    OP_GET_FRIENDLY_VERSION,
];

pub const PIX_SERVER_VERSION: u32 = 0x55f0_114d;
pub const FRIENDLY_VERSION: &str = "June 2022 XDK Update 4";
pub const FRIENDLY_COMPACT_VERSION: &str = "June 2022 Update 4";
pub const XDK_EDITION: f64 = 220604.0;
pub const BUILD_BRANCH: &str = "xb_gdk_2206co";
pub const EXCLUSIVE_CONNECTION_TIMEOUT_MS: u64 = 8000;
pub const SYSMON_DATA_BUFFER_SIZE_BYTES: usize = 0x40000;

type ProcessCallback = unsafe fn(u32, *const u16, *mut Handler);
type ConfigCallback = unsafe fn(u32, *mut Handler);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[repr(C)]
pub struct HandlerVtbl {
    pub query_interface: unsafe extern "system" fn(*mut Handler, *const GUID, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut Handler) -> u32,
    pub release: unsafe extern "system" fn(*mut Handler) -> u32,
    pub get_operation_id: unsafe extern "system" fn(*mut Handler, *mut u32) -> HRESULT,
    pub invoke: unsafe extern "system" fn(*mut Handler, *mut c_void, *mut c_void, *mut c_void) -> HRESULT,
}

#[repr(C)]
pub struct Handler {
    vtbl: *const HandlerVtbl,
    refs: AtomicU32,
    op: u32,
    server: *mut PixServer,
    session: Mutex<ComPtr>,
}

pub struct ProcessList {
    created: Option<ProcessCallback>,
    exited: Option<ProcessCallback>,
    ctx: *mut Handler,
    path: Bstr,
    pid: u32,
}

impl ProcessList {
    unsafe fn add(&mut self, pid: u32, path: *const u16) {
        self.pid = pid;
        let mut copy: BSTR = null_mut();
        if !path.is_null() && *path != 0 {
            copy = SysAllocString(path);
            if copy.is_null() {
                self.pid = 0;
                self.path.reset();
                return;
            }
        }
        self.path.reset();
        self.path.0 = copy;
        if let Some(cb) = self.created {
            cb(pid, path, self.ctx);
        }
    }
}

pub struct SysMon {
    flags: u32,
    buffer: Vec<u8>,
    used: u32,
    config_cb: Option<ConfigCallback>,
    config_ctx: *mut Handler,
}

impl SysMon {
    fn init(&mut self) -> HRESULT {
        if self.buffer.len() < SYSMON_DATA_BUFFER_SIZE_BYTES {
            let mut v = Vec::new();
            if v.try_reserve_exact(SYSMON_DATA_BUFFER_SIZE_BYTES).is_err() {
                return E_OUTOFMEMORY;
            }
            v.resize(SYSMON_DATA_BUFFER_SIZE_BYTES, 0);
            self.buffer = v;
        }
        self.used = 0;
        S_OK
    }
}

struct RefState {
    count: u32,
    destroyed: bool,
}

#[repr(C)]
pub struct ServerVtbl {
    pub query_interface: unsafe extern "system" fn(*mut PixServer, *const GUID, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut PixServer) -> u32,
    pub release: unsafe extern "system" fn(*mut PixServer) -> u32,
    pub get_handler_count: unsafe extern "system" fn(*mut PixServer, *mut u32) -> HRESULT,
    pub get_handlers: unsafe extern "system" fn(*mut PixServer, u32, *mut *mut c_void) -> HRESULT,
    pub add_handlers: unsafe extern "system" fn(*mut PixServer, u32, *const *mut c_void) -> HRESULT,
    pub add_function: unsafe extern "system" fn(*mut PixServer, u32, usize) -> HRESULT,
    pub add_handler: unsafe extern "system" fn(*mut PixServer, *mut c_void) -> HRESULT,
}

#[repr(C)]
pub struct PixServer {
    vtbl: *const ServerVtbl,
    refs: Mutex<RefState>,
    handlers: [Handler; 11],
    metadata: ComPtr,
    sysmon: Mutex<SysMon>,
    processes: Mutex<ProcessList>,
    tick: AtomicU64,
    token: AtomicU64,
}

unsafe extern "system" fn srv_qi(this: *mut PixServer, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    let iid = *iid;
    let p = if iid == IID_PIX_SERVER || iid == IID_IUNKNOWN {
        this as *mut c_void
    } else if iid == IID_EXTENSION_METADATA {
        (*this).metadata.0
    } else {
        return E_NOINTERFACE;
    };
    *out = p;
    ((*(*this).vtbl).add_ref)(this);
    S_OK
}

unsafe extern "system" fn srv_add_ref(this: *mut PixServer) -> u32 {
    let mut r = lock(&(*this).refs);
    r.count += 1;
    r.count
}

unsafe extern "system" fn srv_release(this: *mut PixServer) -> u32 {
    let mut r = lock(&(*this).refs);
    r.count -= 1;
    let n = r.count;
    if n == 0 && !r.destroyed {
        r.destroyed = true;
        drop(r);
        destroy(this);
    }
    n
}

unsafe fn destroy(this: *mut PixServer) {
    core::ptr::drop_in_place(this);
    std::alloc::dealloc(this as *mut u8, std::alloc::Layout::new::<PixServer>());
}

unsafe fn md(this: *mut PixServer) -> *mut c_void {
    (*this).metadata.0
}

unsafe extern "system" fn srv_get_handler_count(this: *mut PixServer, out: *mut u32) -> HRESULT {
    let m = md(this);
    let f: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT = core::mem::transmute(slot(m, 3));
    f(m, out)
}

unsafe extern "system" fn srv_get_handlers(this: *mut PixServer, size: u32, out: *mut *mut c_void) -> HRESULT {
    let m = md(this);
    let f: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT = core::mem::transmute(slot(m, 4));
    f(m, size, out)
}

unsafe extern "system" fn srv_add_handlers(this: *mut PixServer, n: u32, list: *const *mut c_void) -> HRESULT {
    let m = md(this);
    let f: unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void) -> HRESULT = core::mem::transmute(slot(m, 5));
    f(m, n, list)
}

unsafe extern "system" fn srv_add_function(this: *mut PixServer, id: u32, func: usize) -> HRESULT {
    let m = md(this);
    let f: unsafe extern "system" fn(*mut c_void, u32, usize) -> HRESULT = core::mem::transmute(slot(m, 6));
    f(m, id, func)
}

unsafe extern "system" fn srv_add_handler(this: *mut PixServer, h: *mut c_void) -> HRESULT {
    let m = md(this);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT = core::mem::transmute(slot(m, 7));
    f(m, h)
}

pub static SERVER_VTBL: ServerVtbl = ServerVtbl {
    query_interface: srv_qi,
    add_ref: srv_add_ref,
    release: srv_release,
    get_handler_count: srv_get_handler_count,
    get_handlers: srv_get_handlers,
    add_handlers: srv_add_handlers,
    add_function: srv_add_function,
    add_handler: srv_add_handler,
};

unsafe extern "system" fn h_qi(this: *mut Handler, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if *iid == IID_IUNKNOWN || *iid == IID_OPERATION_HANDLER {
        *out = this as *mut c_void;
        h_add_ref(this);
        S_OK
    } else {
        E_NOINTERFACE
    }
}

unsafe extern "system" fn h_add_ref(this: *mut Handler) -> u32 {
    (*this).refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn h_release(this: *mut Handler) -> u32 {
    (*this).refs.fetch_sub(1, Ordering::AcqRel) - 1
}

unsafe extern "system" fn h_get_operation_id(this: *mut Handler, out: *mut u32) -> HRESULT {
    *out = (*this).op;
    S_OK
}

unsafe extern "system" fn h_invoke(this: *mut Handler, session: *mut c_void, received: *mut c_void, reply: *mut c_void) -> HRESULT {
    let h = &*this;
    match h.op {
        OP_TRACK_PROCESSES => track_processes(this, session),
        OP_PROCESS_CREATED => process_created(h, received, reply),
        OP_CLEAR_RUNNING_PROCESS_LIST => clear_running_process_list(h, received, reply),
        OP_CONFIGURE_SYSMON => configure_sysmon(h, session, received, reply),
        OP_GET_SYSMON_CONFIGURATION => get_sysmon_configuration(h, reply),
        OP_TRACK_SYSMON_CONFIGURATION => track_sysmon_configuration(this, session),
        OP_PING => ping(h, received),
        OP_VERSION_HANDSHAKE => version_handshake(received, reply),
        OP_CREATE_EXCLUSIVE_CONNECTION => create_exclusive_connection(h, received, reply),
        OP_RELEASE_EXCLUSIVE_CONNECTION => release_exclusive_connection(h, received, reply),
        _ => get_friendly_version(reply),
    }
}

pub static HANDLER_VTBL: HandlerVtbl = HandlerVtbl {
    query_interface: h_qi,
    add_ref: h_add_ref,
    release: h_release,
    get_operation_id: h_get_operation_id,
    invoke: h_invoke,
};

unsafe fn session_send_more(session: *mut c_void, msg: *mut c_void) -> HRESULT {
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT = core::mem::transmute(slot(session, 3));
    f(session, msg)
}

unsafe fn session_receive_more(session: *mut c_void, timeout: u32, out: *mut *mut c_void) -> HRESULT {
    let f: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT = core::mem::transmute(slot(session, 5));
    f(session, timeout, out)
}

unsafe fn session_get_event(session: *mut c_void, out: *mut HANDLE) -> HRESULT {
    let f: unsafe extern "system" fn(*mut c_void, *mut HANDLE) -> HRESULT = core::mem::transmute(slot(session, 6));
    f(session, out)
}

unsafe fn reset_message_event(session: *mut c_void) -> HRESULT {
    let mut m = ComPtr::null();
    let hr = session_receive_more(session, 0, m.out());
    if hr == E_PENDING {
        S_OK
    } else {
        hr
    }
}

unsafe fn send_initial_response(session: *mut c_void, result: HRESULT) -> HRESULT {
    send_message(session, |body| set_int(body, "Result", result))
}

unsafe fn set_session(h: &Handler, session: *mut c_void) {
    let mut s = lock(&h.session);
    s.reset();
    s.0 = session;
    if !session.is_null() {
        com_add_ref(session);
    }
}

unsafe fn send_message(session: *mut c_void, fill: impl FnOnce(*mut c_void) -> HRESULT) -> HRESULT {
    let mut msg = ComPtr::null();
    let hr = XbtpCreateMessage(&IID_MESSAGE, msg.out());
    if hr < 0 {
        return hr;
    }
    let mut body = ComPtr::null();
    let hr = XbtpCreateJsonObject(body.out());
    if hr < 0 {
        return hr;
    }
    let hr = fill(body.0);
    if hr < 0 {
        return hr;
    }
    let hr = serialize(body.0, msg.0, SET_JSON_BODY_W_REQUEST);
    if hr < 0 {
        return hr;
    }
    session_send_more(session, msg.0)
}

unsafe fn send_process_changed(h: &Handler, pid: u32, path: *const u16, created: i32) -> HRESULT {
    let s = lock(&h.session);
    if s.0.is_null() {
        return S_OK;
    }
    send_message(s.0, |body| {
        let hr = set_dword(body, "ProcessId", pid);
        if hr < 0 {
            return hr;
        }
        let hr = set_string(body, "ProcessPath", path);
        if hr < 0 {
            return hr;
        }
        set_int(body, "ProcessCreated", created)
    })
}

unsafe fn on_process_created(pid: u32, path: *const u16, ctx: *mut Handler) {
    send_process_changed(&*ctx, pid, path, 1);
}

unsafe fn on_process_exited(pid: u32, path: *const u16, ctx: *mut Handler) {
    send_process_changed(&*ctx, pid, path, 0);
}

unsafe fn track_processes(this: *mut Handler, session: *mut c_void) -> HRESULT {
    let h = &*this;
    let srv = &*h.server;
    if send_initial_response(session, S_OK) >= 0 && reset_message_event(session) >= 0 {
        let mut ev: HANDLE = null_mut();
        if session_get_event(session, &mut ev) >= 0 {
            set_session(h, session);
            {
                let mut pl = lock(&srv.processes);
                pl.created = Some(on_process_created);
                pl.exited = Some(on_process_exited);
                pl.ctx = this;
                if pl.pid != 0 {
                    on_process_created(pl.pid, pl.path.0, this);
                }
            }
            WaitForSingleObject(ev, INFINITE);
        }
    }
    {
        let mut pl = lock(&srv.processes);
        pl.created = None;
        pl.exited = None;
        pl.ctx = null_mut();
    }
    lock(&h.session).reset();
    S_OK
}

unsafe fn process_created(h: &Handler, received: *mut c_void, reply: *mut c_void) -> HRESULT {
    let mut ctx = ErrorContext::new(reply);
    let mut body = ComPtr::null();
    let hr = deserialize(received, &mut body);
    if hr < 0 {
        return ctx.fail(hr, 0x129);
    }
    let mut pid = 0u32;
    let hr = get_dword(body.0, "ProcessId", &mut pid);
    if hr < 0 {
        return ctx.fail(hr, 0x12a);
    }
    let mut path = Bstr::null();
    let hr = obj_get_string(body.0, w("ProcessPath").as_ptr(), &mut path.0);
    if hr < 0 {
        return ctx.fail(hr, 0x12b);
    }
    lock(&(*h.server).processes).add(pid, path.0);
    S_OK
}

unsafe fn clear_running_process_list(h: &Handler, received: *mut c_void, reply: *mut c_void) -> HRESULT {
    let mut ctx = ErrorContext::new(reply);
    let mut body = ComPtr::null();
    let hr = deserialize(received, &mut body);
    if hr < 0 {
        return ctx.fail(hr, 0x1b0);
    }
    let mut pl = lock(&(*h.server).processes);
    if let Some(cb) = pl.exited {
        cb(0, pl.path.0, pl.ctx);
    }
    pl.path.reset();
    pl.pid = 0;
    S_OK
}

unsafe fn sysmon_set_flags(srv: &PixServer, flags: u32, out: Option<&mut u32>) -> HRESULT {
    let mut st = lock(&srv.sysmon);
    if st.flags == 0 && flags != 0 {
        st.used = 0;
    }
    if let Some(cb) = st.config_cb {
        cb(st.flags, st.config_ctx);
    }
    if let Some(o) = out {
        *o = st.flags;
    }
    S_OK
}

unsafe fn sysmon_track_configuration(srv: &PixServer, cb: Option<ConfigCallback>, ctx: *mut Handler) {
    let mut st = lock(&srv.sysmon);
    st.config_cb = cb;
    st.config_ctx = ctx;
    if let Some(f) = cb {
        f(st.flags, ctx);
    }
}

unsafe fn configure_sysmon(h: &Handler, session: *mut c_void, received: *mut c_void, reply: *mut c_void) -> HRESULT {
    let mut ctx = ErrorContext::new(reply);
    let mut body = ComPtr::null();
    let hr = deserialize(received, &mut body);
    if hr < 0 {
        return ctx.fail(hr, 0x210);
    }
    let mut flags = 0u32;
    let hr = get_dword(body.0, "SysMonFlags", &mut flags);
    if hr < 0 {
        return ctx.fail(hr, 0x211);
    }
    ctx.checkpoint();
    let srv = &*h.server;
    let mut current = 0u32;
    let set = sysmon_set_flags(srv, flags, Some(&mut current));
    let mut hr = send_initial_response(session, set);
    if hr < 0 || set < 0 || flags == 0 {
        return hr;
    }
    let mut ev: HANDLE = null_mut();
    hr = session_get_event(session, &mut ev);
    if hr < 0 {
        return hr;
    }
    set_session(h, session);
    hr = reset_message_event(session);
    if hr < 0 {
        return hr;
    }
    WaitForSingleObject(ev, INFINITE);
    let cleared = {
        let mut s = lock(&h.session);
        if s.0 == session {
            s.reset();
            true
        } else {
            false
        }
    };
    if cleared {
        hr = sysmon_set_flags(srv, 0, None);
    }
    hr
}

unsafe fn get_sysmon_configuration(h: &Handler, reply: *mut c_void) -> HRESULT {
    let mut ctx = ErrorContext::new(reply);
    let flags = lock(&(*h.server).sysmon).flags;
    let mut body = ComPtr::null();
    let hr = XbtpCreateJsonObject(body.out());
    if hr < 0 {
        return ctx.fail(hr, 0x35b);
    }
    let hr = set_dword(body.0, "SysMonFlags", flags);
    if hr < 0 {
        return ctx.fail(hr, 0x35c);
    }
    let hr = serialize(body.0, reply, SET_JSON_BODY_W_REPLY);
    if hr < 0 {
        return ctx.fail(hr, 0x35d);
    }
    S_OK
}

unsafe fn send_sysmon_configuration_changed(h: &Handler, flags: u32) -> HRESULT {
    let s = lock(&h.session);
    if s.0.is_null() {
        return S_OK;
    }
    send_message(s.0, |body| set_dword(body, "SysMonFlags", flags))
}

unsafe fn on_sysmon_configuration(flags: u32, ctx: *mut Handler) {
    send_sysmon_configuration_changed(&*ctx, flags);
}

unsafe fn track_sysmon_configuration(this: *mut Handler, session: *mut c_void) -> HRESULT {
    let h = &*this;
    let srv = &*h.server;
    let mut hr = send_initial_response(session, S_OK);
    if hr >= 0 {
        hr = reset_message_event(session);
        if hr >= 0 {
            let mut ev: HANDLE = null_mut();
            hr = session_get_event(session, &mut ev);
            if hr >= 0 {
                set_session(h, session);
                sysmon_track_configuration(srv, Some(on_sysmon_configuration), this);
                WaitForSingleObject(ev, INFINITE);
            }
        }
    }
    sysmon_track_configuration(srv, None, null_mut());
    lock(&h.session).reset();
    hr
}

unsafe fn ping(h: &Handler, received: *mut c_void) -> HRESULT {
    let srv = &*h.server;
    let mut body = ComPtr::null();
    let mut token = 0u64;
    if deserialize(received, &mut body) >= 0 && get_u64(body.0, "ConnectionToken", &mut token) >= 0 && token != srv.token.load(Ordering::Acquire) {
        return E_ACCESSDENIED;
    }
    srv.tick.store(GetTickCount64(), Ordering::Release);
    S_OK
}

unsafe fn version_handshake(received: *mut c_void, reply: *mut c_void) -> HRESULT {
    let mut ctx = ErrorContext::new(reply);
    let mut body = ComPtr::null();
    let hr = deserialize(received, &mut body);
    if hr < 0 {
        return ctx.fail(hr, 0x3da);
    }
    let mut client = 0u32;
    let hr = get_dword(body.0, "Version", &mut client);
    if hr < 0 {
        return ctx.fail(hr, 0x3db);
    }
    ctx.checkpoint();
    let mut freq = 0i64;
    QueryPerformanceFrequency(&mut freq);
    let system_type = XGetSystemType() as u16;
    let hr = XbtpCreateJsonObject(body.out());
    if hr < 0 {
        return ctx.fail(hr, 0x3f5);
    }
    let hr = set_dword(body.0, "Version", PIX_SERVER_VERSION);
    if hr < 0 {
        return ctx.fail(hr, 0x3f6);
    }
    let hr = set_int(body.0, "Result", S_OK);
    if hr < 0 {
        return ctx.fail(hr, 0x3f7);
    }
    let hr = set_u64(body.0, "CpuFrequency", freq as u64);
    if hr < 0 {
        return ctx.fail(hr, 0x3f8);
    }
    let hr = set_dword(body.0, "SystemType", system_type as u32);
    if hr < 0 {
        return ctx.fail(hr, 0x3f9);
    }
    let hr = serialize(body.0, reply, SET_JSON_BODY_W_REPLY);
    if hr < 0 {
        return ctx.fail(hr, 0x3fa);
    }
    ctx.checkpoint();
    S_OK
}

unsafe fn get_friendly_version(reply: *mut c_void) -> HRESULT {
    let mut ctx = ErrorContext::new(reply);
    let mut body = ComPtr::null();
    let hr = XbtpCreateJsonObject(body.out());
    if hr < 0 {
        return ctx.fail(hr, 0x382);
    }
    let hr = set_string(body.0, "FriendlyVersionProperty", w(FRIENDLY_VERSION).as_ptr());
    if hr < 0 {
        return ctx.fail(hr, 0x383);
    }
    let hr = set_string(body.0, "FriendlyCompactVersionProperty", w(FRIENDLY_COMPACT_VERSION).as_ptr());
    if hr < 0 {
        return ctx.fail(hr, 0x384);
    }
    let hr = obj_set_number(body.0, w("XDKEditionProperty").as_ptr(), XDK_EDITION);
    if hr < 0 {
        return ctx.fail(hr, 0x385);
    }
    let hr = set_string(body.0, "BuildBranchProperty", w(BUILD_BRANCH).as_ptr());
    if hr < 0 {
        return ctx.fail(hr, 0x386);
    }
    let hr = serialize(body.0, reply, SET_JSON_BODY_W_REPLY);
    if hr < 0 {
        return ctx.fail(hr, 0x388);
    }
    S_OK
}

unsafe fn create_exclusive_connection(h: &Handler, received: *mut c_void, reply: *mut c_void) -> HRESULT {
    let srv = &*h.server;
    let mut ctx = ErrorContext::new(reply);
    let mut body = ComPtr::null();
    let mut force = 0i32;
    let mut token = 0u64;
    if deserialize(received, &mut body) >= 0 {
        get_bool(body.0, "ForceConnect", &mut force);
        get_u64(body.0, "ConnectionToken", &mut token);
    }
    let now = GetTickCount64();
    let mut result = S_OK;
    if token == srv.token.load(Ordering::Acquire) {
        srv.tick.store(now, Ordering::Release);
    } else if now.wrapping_sub(srv.tick.load(Ordering::Acquire)) <= EXCLUSIVE_CONNECTION_TIMEOUT_MS && force == 0 {
        result = HR_EXCLUSIVE_CONNECTION_HELD;
    } else {
        srv.tick.store(now, Ordering::Release);
        if token != 0 {
            srv.token.store(token, Ordering::Release);
        } else {
            token = srv.token.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        }
    }
    let hr = XbtpCreateJsonObject(body.out());
    if hr < 0 {
        return ctx.fail(hr, 0x44e);
    }
    let hr = set_int(body.0, "Result", result);
    if hr < 0 {
        return ctx.fail(hr, 0x44f);
    }
    let hr = set_u64(body.0, "ConnectionToken", token);
    if hr < 0 {
        return ctx.fail(hr, 0x450);
    }
    let hr = serialize(body.0, reply, SET_JSON_BODY_W_REPLY);
    if hr < 0 {
        return ctx.fail(hr, 0x451);
    }
    ctx.checkpoint();
    S_OK
}

unsafe fn release_exclusive_connection(h: &Handler, received: *mut c_void, reply: *mut c_void) -> HRESULT {
    let srv = &*h.server;
    let mut ctx = ErrorContext::new(reply);
    let mut body = ComPtr::null();
    let hr = deserialize(received, &mut body);
    if hr < 0 {
        return ctx.fail(hr, 0x479);
    }
    let mut token = 0u64;
    let hr = get_u64(body.0, "ConnectionToken", &mut token);
    if hr < 0 {
        return ctx.fail(hr, 0x47a);
    }
    ctx.checkpoint();
    if token == srv.token.load(Ordering::Acquire) {
        srv.tick.store(0, Ordering::Release);
    }
    S_OK
}

unsafe fn construct(this: *mut PixServer, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    let handler = |op: u32| Handler { vtbl: &HANDLER_VTBL, refs: AtomicU32::new(1), op, server: this, session: Mutex::new(ComPtr::null()) };
    core::ptr::write(
        this,
        PixServer {
            vtbl: &SERVER_VTBL,
            refs: Mutex::new(RefState { count: 1, destroyed: false }),
            handlers: HANDLER_OPS.map(handler),
            metadata: ComPtr::null(),
            sysmon: Mutex::new(SysMon { flags: 0, buffer: Vec::new(), used: 0, config_cb: None, config_ctx: null_mut() }),
            processes: Mutex::new(ProcessList { created: None, exited: None, ctx: null_mut(), path: Bstr::null(), pid: 0 }),
            tick: AtomicU64::new(0),
            token: AtomicU64::new(0),
        },
    );
    *ppv = null_mut();
    let s = &mut *this;
    let hr = XbtpCreateExtensionMetadata(&IID_EXTENSION_METADATA, s.metadata.out());
    if hr < 0 {
        return hr;
    }
    let hr = lock(&s.sysmon).init();
    if hr < 0 {
        return hr;
    }
    let list: Vec<*mut c_void> = s.handlers.iter_mut().map(|h| h as *mut Handler as *mut c_void).collect();
    let m = s.metadata.0;
    let add: unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void) -> HRESULT = core::mem::transmute(slot(m, 5));
    let hr = add(m, list.len() as u32, list.as_ptr());
    if hr < 0 {
        return hr;
    }
    srv_qi(this, riid, ppv)
}

#[no_mangle]
pub unsafe extern "system" fn XtfCreatePixServer(riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    let p = std::alloc::alloc(std::alloc::Layout::new::<PixServer>()) as *mut PixServer;
    if p.is_null() {
        return E_OUTOFMEMORY;
    }
    let hr = construct(p, riid, ppv);
    srv_release(p);
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XbtpServerEntryPoint(extension: *const GUID, metadata_iid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if *extension != PIX_EXTENSION_ID {
        return E_UNEXPECTED;
    }
    if *metadata_iid != IID_EXTENSION_METADATA {
        return E_INVALIDARG;
    }
    let mut sp = ComPtr::null();
    let hr = XtfCreatePixServer(&IID_EXTENSION_METADATA, sp.out());
    if hr < 0 {
        return hr;
    }
    com_qi(sp.0, metadata_iid, ppv)
}

#[cfg(test)]
pub unsafe fn handler_ptr(server: *mut c_void, op: u32) -> *mut c_void {
    let s = &mut *(server as *mut PixServer);
    match s.handlers.iter_mut().find(|h| h.op == op) {
        Some(h) => h as *mut Handler as *mut c_void,
        None => null_mut(),
    }
}

#[cfg(test)]
pub unsafe fn server_ticks(server: *mut c_void) -> (u64, u64) {
    let s = &*(server as *mut PixServer);
    (s.tick.load(Ordering::Acquire), s.token.load(Ordering::Acquire))
}

#[cfg(test)]
pub unsafe fn server_set_ticks(server: *mut c_void, tick: u64, token: u64) {
    let s = &*(server as *mut PixServer);
    s.tick.store(tick, Ordering::Release);
    s.token.store(token, Ordering::Release);
}

#[cfg(test)]
pub unsafe fn server_process(server: *mut c_void) -> (u32, Option<Vec<u16>>) {
    let s = &*(server as *mut PixServer);
    let pl = lock(&s.processes);
    let path = if pl.path.0.is_null() { None } else { Some(xtfcommon::sys::wslice(pl.path.0).to_vec()) };
    (pl.pid, path)
}

#[cfg(test)]
pub fn sysmon_buffer_len(server: *mut c_void) -> usize {
    let s = unsafe { &*(server as *mut PixServer) };
    lock(&s.sysmon).buffer.len()
}
