use core::ffi::c_void;
use core::ptr::null_mut;
use std::sync::{Mutex, OnceLock};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Guid(pub u32, pub u16, pub u16, pub [u8; 8]);

pub const IID_IAGILEOBJECT: Guid = Guid(0x94ea2b94, 0xe9cc, 0x49e0, [0xc0, 0xff, 0xee, 0x64, 0xca, 0x8f, 0x5b, 0x90]);
pub const IID_SCALE_HANDLER: Guid = Guid(0xabadfe7f, 0x04c1, 0x4a9f, [0x9b, 0x3c, 0xa3, 0x23, 0xf1, 0xc8, 0x01, 0x78]);
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY: i32 = 0x8007_000eu32 as i32;
pub const E_FAIL: i32 = 0x8000_4005u32 as i32;
pub const WM_APP: u32 = 0x8000;
pub const MSG_SPAN: u32 = 0x3fff;
const HWND_MESSAGE: isize = -3;
const COINIT_MULTITHREADED: u32 = 0;

#[repr(C)]
pub struct UnknownVtbl {
    pub query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct AgileRefVtbl {
    base: UnknownVtbl,
    resolve: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
}

#[repr(C)]
struct HandlerVtbl {
    base: UnknownVtbl,
    invoke: unsafe extern "system" fn(*mut c_void, i32) -> i32,
}

unsafe fn vt<T>(p: *mut c_void) -> &'static T {
    &**(p as *mut *const T)
}

#[repr(C)]
struct WndClassExW {
    size: u32,
    style: u32,
    proc_: unsafe extern "system" fn(isize, u32, usize, isize) -> isize,
    cls: i32,
    wnd: i32,
    inst: *mut c_void,
    icon: *mut c_void,
    cursor: *mut c_void,
    bg: *mut c_void,
    menu: *const u16,
    name: *const u16,
    icon_sm: *mut c_void,
}

#[repr(C)]
struct Msg {
    hwnd: isize,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    pt: [i32; 2],
    private: u32,
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassExW(c: *const WndClassExW) -> u16;
    fn CreateWindowExW(ex: u32, cls: *const u16, name: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: isize, menu: *mut c_void, inst: *mut c_void, param: *mut c_void) -> isize;
    fn DefWindowProcW(h: isize, m: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(m: *mut Msg, h: isize, lo: u32, hi: u32) -> i32;
    fn DispatchMessageW(m: *const Msg) -> isize;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetSystemDirectoryW(buf: *mut u16, n: u32) -> u32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(r: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
}

type RegisterFn = unsafe extern "system" fn(i32, isize, u32, *mut u32) -> i32;
type RevokeFn = unsafe extern "system" fn(i32, u32) -> i32;
type ScaleFn = unsafe extern "system" fn(i32) -> i32;
type AgileFn = unsafe extern "system" fn(u32, *const Guid, *mut c_void, *mut *mut c_void) -> i32;

pub struct Host {
    pub register: RegisterFn,
    pub revoke: RevokeFn,
    pub scale: ScaleFn,
    agile: AgileFn,
}

fn system_dll(name: &str) -> *mut c_void {
    let mut buf = [0u16; 260];
    let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let mut path: Vec<u16> = buf[..n].to_vec();
    path.extend(format!("\\{name}").encode_utf16());
    path.push(0);
    unsafe { LoadLibraryW(path.as_ptr()) }
}

pub fn host() -> Option<&'static Host> {
    static H: OnceLock<Option<Host>> = OnceLock::new();
    H.get_or_init(|| unsafe {
        let sh = system_dll("shcore.dll");
        let cb = system_dll("combase.dll");
        if sh.is_null() || cb.is_null() {
            return None;
        }
        let r = GetProcAddress(sh, b"RegisterScaleChangeNotifications\0".as_ptr());
        let v = GetProcAddress(sh, b"RevokeScaleChangeNotifications\0".as_ptr());
        let s = GetProcAddress(sh, b"GetScaleFactorForDevice\0".as_ptr());
        let a = GetProcAddress(cb, b"RoGetAgileReference\0".as_ptr());
        if r.is_null() || v.is_null() || s.is_null() || a.is_null() {
            return None;
        }
        Some(Host { register: core::mem::transmute(r), revoke: core::mem::transmute(v), scale: core::mem::transmute(s), agile: core::mem::transmute(a) })
    })
    .as_ref()
}

#[derive(Clone, Copy)]
enum Target {
    Direct(*mut c_void),
    Agile(*mut c_void),
}

unsafe impl Send for Target {}

struct Reg {
    kind: i32,
    cookie: u32,
    msg: u32,
    target: Target,
}

struct State {
    next: u32,
    regs: Vec<Reg>,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State { next: 0, regs: Vec::new() }))
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

unsafe fn release(t: Target) {
    let p = match t {
        Target::Direct(p) | Target::Agile(p) => p,
    };
    (vt::<UnknownVtbl>(p).release)(p);
}

unsafe fn handler_for(t: Target) -> Result<*mut c_void, i32> {
    match t {
        Target::Direct(p) => {
            (vt::<UnknownVtbl>(p).add_ref)(p);
            Ok(p)
        }
        Target::Agile(r) => {
            let mut out = null_mut();
            let hr = (vt::<AgileRefVtbl>(r).resolve)(r, &IID_SCALE_HANDLER, &mut out);
            if hr < 0 {
                Err(hr)
            } else {
                Ok(out)
            }
        }
    }
}

pub fn dispatch(msg: u32, flags: usize) -> Option<i32> {
    if flags & 1 == 0 {
        return None;
    }
    let (kind, t) = {
        let g = lock();
        let r = g.regs.iter().find(|r| r.msg == msg)?;
        let t = r.target;
        let p = match t {
            Target::Direct(p) | Target::Agile(p) => p,
        };
        unsafe { (vt::<UnknownVtbl>(p).add_ref)(p) };
        (r.kind, t)
    };
    let h = host()?;
    let co = unsafe { CoInitializeEx(null_mut(), COINIT_MULTITHREADED) } >= 0;
    let res = unsafe {
        let r = match handler_for(t) {
            Ok(p) => {
                let scale = (h.scale)(kind);
                let _ = (vt::<HandlerVtbl>(p).invoke)(p, scale);
                (vt::<UnknownVtbl>(p).release)(p);
                scale
            }
            Err(hr) => hr,
        };
        release(t);
        r
    };
    if co {
        unsafe { CoUninitialize() };
    }
    Some(res)
}

unsafe extern "system" fn wndproc(h: isize, m: u32, w: usize, l: isize) -> isize {
    if (WM_APP..=WM_APP + MSG_SPAN).contains(&m) {
        dispatch(m, w);
        return 0;
    }
    DefWindowProcW(h, m, w, l)
}

fn window() -> isize {
    static W: OnceLock<isize> = OnceLock::new();
    *W.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("tombox-shcore-scale".into())
            .spawn(move || unsafe {
                let name: Vec<u16> = "TomBoxShcoreScaleSink".encode_utf16().chain(Some(0)).collect();
                let inst = GetModuleHandleW(core::ptr::null());
                let wc = WndClassExW {
                    size: core::mem::size_of::<WndClassExW>() as u32,
                    style: 0,
                    proc_: wndproc,
                    cls: 0,
                    wnd: 0,
                    inst,
                    icon: null_mut(),
                    cursor: null_mut(),
                    bg: null_mut(),
                    menu: core::ptr::null(),
                    name: name.as_ptr(),
                    icon_sm: null_mut(),
                };
                RegisterClassExW(&wc);
                let hwnd = CreateWindowExW(0, name.as_ptr(), name.as_ptr(), 0, 0, 0, 0, 0, HWND_MESSAGE, null_mut(), inst, null_mut());
                let _ = tx.send(hwnd);
                if hwnd == 0 {
                    return;
                }
                let mut m: Msg = core::mem::zeroed();
                while GetMessageW(&mut m, 0, 0, 0) > 0 {
                    DispatchMessageW(&m);
                }
            })
            .ok()
            .and_then(|_| rx.recv().ok())
            .unwrap_or(0)
    })
}

pub fn sink_hwnd() -> isize {
    window()
}

pub fn message_for(kind: i32, cookie: u32) -> Option<u32> {
    lock().regs.iter().find(|r| r.kind == kind && r.cookie == cookie).map(|r| r.msg)
}

pub unsafe fn register(kind: i32, handler: *mut c_void, cookie: *mut u32) -> i32 {
    if cookie.is_null() {
        return E_INVALIDARG;
    }
    *cookie = 0;
    if !(0..=2).contains(&kind) || handler.is_null() {
        return E_INVALIDARG;
    }
    let Some(h) = host() else { return E_OUTOFMEMORY };
    let mut agile = null_mut();
    let target = if (vt::<UnknownVtbl>(handler).query_interface)(handler, &IID_IAGILEOBJECT, &mut agile) >= 0 {
        (vt::<UnknownVtbl>(agile).release)(agile);
        (vt::<UnknownVtbl>(handler).add_ref)(handler);
        Target::Direct(handler)
    } else {
        let mut r = null_mut();
        let hr = (h.agile)(0, &IID_SCALE_HANDLER, handler, &mut r);
        if hr < 0 {
            return hr;
        }
        Target::Agile(r)
    };
    let hwnd = window();
    if hwnd == 0 {
        release(target);
        return E_OUTOFMEMORY;
    }
    let mut g = lock();
    let used: Vec<u32> = g.regs.iter().map(|r| r.msg).collect();
    let mut msg = None;
    for i in 0..=MSG_SPAN {
        let m = WM_APP + (g.next + i) % (MSG_SPAN + 1);
        if !used.contains(&m) {
            g.next = (m - WM_APP + 1) % (MSG_SPAN + 1);
            msg = Some(m);
            break;
        }
    }
    let Some(msg) = msg else {
        drop(g);
        release(target);
        return E_OUTOFMEMORY;
    };
    let mut c = 0u32;
    let hr = (h.register)(kind, hwnd, msg, &mut c);
    if hr < 0 {
        drop(g);
        release(target);
        return hr;
    }
    g.regs.push(Reg { kind, cookie: c, msg, target });
    *cookie = c;
    0
}

pub unsafe fn revoke(kind: i32, cookie: u32) -> i32 {
    let Some(h) = host() else { return E_FAIL };
    let hr = (h.revoke)(kind, cookie);
    if hr >= 0 {
        let gone = {
            let mut g = lock();
            g.regs.iter().position(|r| r.kind == kind && r.cookie == cookie).map(|i| g.regs.remove(i))
        };
        if let Some(r) = gone {
            release(r.target);
        }
    }
    hr
}
