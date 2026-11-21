use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

type H = *mut c_void;

extern "system" {
    fn LoadLibraryW(name: *const u16) -> H;
    fn GetProcAddress(h: H, name: *const u8) -> *mut c_void;
    fn GetModuleHandleW(name: *const u16) -> H;
    fn SetDllDirectoryW(dir: *const u16) -> i32;
    fn GetCurrentThreadId() -> u32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Guid {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}

const IID_IUNKNOWN: Guid = Guid { d1: 0, d2: 0, d3: 0, d4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46] };
const IID_IMFASYNCCALLBACK: Guid = Guid { d1: 0xa27003cf, d2: 0x2354, d3: 0x4f2a, d4: [0x8d, 0x6a, 0xab, 0x7c, 0xff, 0x15, 0x43, 0x7e] };
const KEY_A: Guid = Guid { d1: 0x1111_1111, d2: 1, d3: 2, d4: [1, 2, 3, 4, 5, 6, 7, 8] };
const KEY_B: Guid = Guid { d1: 0x2222_2222, d2: 3, d3: 4, d4: [8, 7, 6, 5, 4, 3, 2, 1] };

const MF_VERSION: u32 = 0x0002_0070;
const MFSTARTUP_FULL: u32 = 0;
const MFASYNC_CALLBACK_QUEUE_STANDARD: u32 = 1;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
const E_FAIL: i32 = 0x8000_4005u32 as i32;

struct Proxy {
    dir: PathBuf,
    proxy: usize,
}

static PROXY: OnceLock<Proxy> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn wp(p: &Path) -> Vec<u16> {
    w(&p.to_string_lossy())
}

fn built_dll() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap().to_path_buf();
    let mut found: Vec<PathBuf> = ["release", "debug"]
        .iter()
        .flat_map(|p| [target.join(p).join("mfplat.dll"), target.join(p).join("deps").join("mfplat.dll")])
        .filter(|p| p.exists())
        .collect();
    found.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    found.pop().expect("built mfplat.dll (cdylib) not found")
}

fn proxy() -> &'static Proxy {
    PROXY.get_or_init(|| {
        assert!(unsafe { GetModuleHandleW(w("mfplat.dll").as_ptr()) }.is_null(), "system mfplat.dll loaded before the proxy");
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mfplat_fortnite_async_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(built_dll(), dir.join("mfplat.dll")).unwrap();
        let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        std::fs::copy(Path::new(&sysroot).join("System32").join("mfplat.dll"), dir.join("mfplat_sys.dll")).unwrap();
        std::env::set_var("TOMBOX_LOG", dir.join("tombox_compat.log"));
        assert!(unsafe { SetDllDirectoryW(wp(&dir).as_ptr()) } != 0);
        let h = unsafe { LoadLibraryW(wp(&dir.join("mfplat.dll")).as_ptr()) };
        assert!(!h.is_null(), "load proxy mfplat.dll");
        unsafe { CoInitializeEx(core::ptr::null_mut(), 0) };
        Proxy { dir, proxy: h as usize }
    })
}

fn px(name: &str) -> usize {
    let c = format!("{name}\0");
    let a = unsafe { GetProcAddress(proxy().proxy as H, c.as_ptr()) } as usize;
    assert!(a != 0, "proxy does not export {name}");
    a
}

macro_rules! export {
    ($addr:expr, fn($($t:ty),*) -> $r:ty) => {{
        let f: unsafe extern "system" fn($($t),*) -> $r = unsafe { core::mem::transmute($addr) };
        f
    }};
}

unsafe fn vt(obj: *mut c_void, i: usize) -> usize {
    unsafe { *(*(obj as *const *const usize)).add(i) }
}

macro_rules! call {
    ($obj:expr, $slot:expr, fn($($t:ty),*) -> $r:ty, $($a:expr),*) => {{
        let o: *mut c_void = $obj;
        let f: unsafe extern "system" fn(*mut c_void $(, $t)*) -> $r = unsafe { core::mem::transmute(vt(o, $slot)) };
        unsafe { f(o $(, $a)*) }
    }};
}

fn release(o: *mut c_void) -> u32 {
    call!(o, 2, fn() -> u32,)
}

fn hr_ok(hr: i32, what: &str) {
    assert!(hr >= 0, "{what} failed hr={:#010x}", hr as u32);
}

#[repr(C)]
struct Callback {
    vtbl: *const Vtbl,
    refs: AtomicU32,
    invokes: AtomicU32,
    thread: AtomicU32,
    status: AtomicU32,
    state: AtomicUsize,
    object: AtomicUsize,
}

unsafe extern "system" fn cb_qi(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    let iid = unsafe { *iid };
    if iid == IID_IUNKNOWN || iid == IID_IMFASYNCCALLBACK {
        unsafe { cb_addref(this) };
        unsafe { *out = this };
        0
    } else {
        unsafe { *out = core::ptr::null_mut() };
        E_NOINTERFACE
    }
}

unsafe extern "system" fn cb_addref(this: *mut c_void) -> u32 {
    unsafe { &*(this as *const Callback) }.refs.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn cb_release(this: *mut c_void) -> u32 {
    unsafe { &*(this as *const Callback) }.refs.fetch_sub(1, Ordering::SeqCst) - 1
}

unsafe extern "system" fn cb_get_parameters(_this: *mut c_void, _flags: *mut u32, _queue: *mut u32) -> i32 {
    E_NOTIMPL
}

unsafe extern "system" fn cb_invoke(this: *mut c_void, result: *mut c_void) -> i32 {
    let me = unsafe { &*(this as *const Callback) };
    me.thread.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    me.status.store(call!(result, 4, fn() -> i32,) as u32, Ordering::SeqCst);
    let state = call!(result, 7, fn() -> *mut c_void,);
    me.state.store(state as usize, Ordering::SeqCst);
    let mut obj: *mut c_void = core::ptr::null_mut();
    if call!(result, 6, fn(*mut *mut c_void) -> i32, &mut obj) >= 0 && !obj.is_null() {
        me.object.store(obj as usize, Ordering::SeqCst);
        release(obj);
    }
    me.invokes.fetch_add(1, Ordering::SeqCst);
    0
}

#[repr(C)]
struct Vtbl {
    qi: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    addref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_parameters: unsafe extern "system" fn(*mut c_void, *mut u32, *mut u32) -> i32,
    invoke: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
}

static CB_VTBL: Vtbl = Vtbl { qi: cb_qi, addref: cb_addref, release: cb_release, get_parameters: cb_get_parameters, invoke: cb_invoke };

fn callback() -> Box<Callback> {
    Box::new(Callback {
        vtbl: &CB_VTBL,
        refs: AtomicU32::new(1),
        invokes: AtomicU32::new(0),
        thread: AtomicU32::new(0),
        status: AtomicU32::new(0xffff_ffff),
        state: AtomicUsize::new(0),
        object: AtomicUsize::new(0),
    })
}

fn wait_invoked(cb: &Callback, n: u32) {
    let t = Instant::now();
    while cb.invokes.load(Ordering::SeqCst) < n {
        assert!(t.elapsed() < Duration::from_secs(10), "callback not invoked within 10 s");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_refs(cb: &Callback, n: u32) {
    let t = Instant::now();
    while cb.refs.load(Ordering::SeqCst) != n {
        assert!(t.elapsed() < Duration::from_secs(10), "callback refcount {} never returned to {n}", cb.refs.load(Ordering::SeqCst));
        std::thread::sleep(Duration::from_millis(1));
    }
}

struct Started;

impl Drop for Started {
    fn drop(&mut self) {
        hr_ok(unsafe { export!(px("MFShutdown"), fn() -> i32)() }, "MFShutdown");
    }
}

fn startup() -> Started {
    hr_ok(unsafe { export!(px("MFStartup"), fn(u32, u32) -> i32)(MF_VERSION, MFSTARTUP_FULL) }, "MFStartup(0x20070, MFSTARTUP_FULL)");
    Started
}

fn attributes(n: u32) -> *mut c_void {
    let mut a = core::ptr::null_mut();
    hr_ok(unsafe { export!(px("MFCreateAttributes"), fn(*mut *mut c_void, u32) -> i32)(&mut a, n) }, "MFCreateAttributes");
    assert!(!a.is_null());
    a
}

#[test]
fn put_work_item2_on_standard_queue_as_fortnite_calls_it() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _s = startup();
    let put = export!(px("MFPutWorkItem2"), fn(u32, i32, *mut c_void, *mut c_void) -> i32);
    let cb = callback();
    let state = attributes(1);
    let me = unsafe { GetCurrentThreadId() };
    for i in 1..=3u32 {
        hr_ok(unsafe { put(MFASYNC_CALLBACK_QUEUE_STANDARD, 0, &*cb as *const Callback as *mut c_void, state) }, "MFPutWorkItem2(STANDARD, 0)");
        wait_invoked(&cb, i);
        assert_eq!(cb.state.load(Ordering::SeqCst), state as usize, "Invoke must see the state object passed to MFPutWorkItem2");
        assert_eq!(cb.status.load(Ordering::SeqCst), 0, "work item result status S_OK");
        let t = cb.thread.load(Ordering::SeqCst);
        assert!(t != 0 && t != me, "work item must run on a work-queue thread, not the caller");
    }
    wait_refs(&cb, 1);
    release(state);
}

#[test]
fn create_async_result_and_invoke_callback_deliver_status_object_and_state() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _s = startup();
    let mk = export!(px("MFCreateAsyncResult"), fn(*mut c_void, *mut c_void, *mut c_void, *mut *mut c_void) -> i32);
    let invoke = export!(px("MFInvokeCallback"), fn(*mut c_void) -> i32);
    let cb = callback();
    let obj = attributes(1);
    let state = attributes(1);
    let mut r: *mut c_void = core::ptr::null_mut();
    hr_ok(unsafe { mk(obj, &*cb as *const Callback as *mut c_void, state, &mut r) }, "MFCreateAsyncResult");
    assert!(!r.is_null());
    assert_eq!(call!(r, 4, fn() -> i32,), 0, "new async result status is S_OK");
    hr_ok(call!(r, 5, fn(i32) -> i32, E_FAIL), "IMFAsyncResult::SetStatus");
    hr_ok(unsafe { invoke(r) }, "MFInvokeCallback");
    wait_invoked(&cb, 1);
    assert_eq!(cb.status.load(Ordering::SeqCst) as i32, E_FAIL, "Invoke must see the status set on the result");
    assert_eq!(cb.state.load(Ordering::SeqCst), state as usize);
    assert_eq!(cb.object.load(Ordering::SeqCst), obj as usize);
    release(r);
    wait_refs(&cb, 1);
    release(obj);
    release(state);
}

#[test]
fn create_attributes_with_fortnite_size_hint_grows_past_it() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let a = attributes(1);
    hr_ok(call!(a, 21, fn(*const Guid, u32) -> i32, &KEY_A, 7), "SetUINT32");
    hr_ok(call!(a, 22, fn(*const Guid, u64) -> i32, &KEY_B, 1 << 40), "SetUINT64");
    let mut n = 0u32;
    hr_ok(call!(a, 30, fn(*mut u32) -> i32, &mut n), "GetCount");
    assert_eq!(n, 2);
    let mut v = 0u32;
    hr_ok(call!(a, 7, fn(*const Guid, *mut u32) -> i32, &KEY_A, &mut v), "GetUINT32");
    assert_eq!(v, 7);
    release(a);
    let _ = proxy().dir.clone();
}
