use super::rt::{self, Block};
use super::{real_query, Guid, E_ABORT, E_FAIL, E_INVALIDARG, E_NOT_SUFFICIENT_BUFFER, E_PENDING, S_OK};
use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub const XASYNC: Guid = Guid::parse("073b7dcb-1fcf-4030-94be-e3c9eb623428");

#[repr(C)]
pub struct ProviderData {
    pub block: *mut c_void,
    pub buffer_size: usize,
    pub buffer: *mut u8,
    pub context: *mut c_void,
}

pub type Provider = unsafe extern "system" fn(u32, *const ProviderData) -> i32;

static OBJ: AtomicUsize = AtomicUsize::new(0);

unsafe fn obj() -> usize {
    let o = OBJ.load(Ordering::Acquire);
    if o != 0 {
        return o;
    }
    let Some(q) = real_query() else { return 0 };
    let mut out: *mut c_void = core::ptr::null_mut();
    let hr = q(&XASYNC, &XASYNC, &mut out);
    if hr < 0 || out.is_null() {
        super::log("xasync", &format!("real XAsync interface unavailable hr={hr:#010x}"));
        return 0;
    }
    OBJ.store(out as usize, Ordering::Release);
    out as usize
}

pub fn available() -> bool {
    rt::active() || unsafe { obj() != 0 }
}

unsafe fn method(slot: usize) -> Option<(usize, usize)> {
    let o = obj();
    if o == 0 {
        return None;
    }
    let vt = *(o as *const *const usize);
    Some((o, *vt.add(slot)))
}

pub unsafe fn begin(block: *mut c_void, ctx: *mut c_void, identity: usize, name: *const u8, provider: Provider) -> i32 {
    if rt::active() {
        return rt::begin(block as *mut Block, ctx, identity, name, provider);
    }
    match method(7) {
        Some((o, f)) => {
            let f: unsafe extern "system" fn(usize, *mut c_void, *mut c_void, usize, *const u8, Provider) -> i32 = core::mem::transmute(f);
            f(o, block, ctx, identity, name, provider)
        }
        None => E_FAIL,
    }
}

pub unsafe fn schedule(block: *mut c_void, delay_ms: u32) -> i32 {
    if rt::active() {
        return rt::schedule(block as *mut Block, delay_ms);
    }
    match method(9) {
        Some((o, f)) => {
            let f: unsafe extern "system" fn(usize, *mut c_void, u32) -> i32 = core::mem::transmute(f);
            f(o, block, delay_ms)
        }
        None => E_FAIL,
    }
}

pub unsafe fn complete(block: *mut c_void, hr: i32, size: usize) {
    if rt::active() {
        return rt::complete(block as *mut Block, hr, size);
    }
    if let Some((o, f)) = method(10) {
        let f: unsafe extern "system" fn(usize, *mut c_void, i32, usize) = core::mem::transmute(f);
        f(o, block, hr, size)
    }
}

pub unsafe fn get_result(block: *mut c_void, identity: usize, size: usize, buf: *mut c_void, used: *mut usize) -> i32 {
    if rt::active() {
        return rt::get_result(block as *mut Block, identity, size, buf, used);
    }
    match method(11) {
        Some((o, f)) => {
            let f: unsafe extern "system" fn(usize, *mut c_void, usize, usize, *mut c_void, *mut usize) -> i32 = core::mem::transmute(f);
            f(o, block, identity, size, buf, used)
        }
        None => E_FAIL,
    }
}

pub unsafe fn get_status(block: *mut c_void, wait: bool) -> i32 {
    if rt::active() {
        return rt::get_status(block as *mut Block, wait);
    }
    match method(3) {
        Some((o, f)) => {
            let f: unsafe extern "system" fn(usize, *mut c_void, bool) -> i32 = core::mem::transmute(f);
            f(o, block, wait)
        }
        None => E_FAIL,
    }
}

pub unsafe fn get_result_size(block: *mut c_void, size: *mut usize) -> i32 {
    if rt::active() {
        return rt::get_result_size(block as *mut Block, size);
    }
    match method(4) {
        Some((o, f)) => {
            let f: unsafe extern "system" fn(usize, *mut c_void, *mut usize) -> i32 = core::mem::transmute(f);
            f(o, block, size)
        }
        None => E_FAIL,
    }
}

struct JobBox(Box<dyn FnOnce(bool) + Send>);

unsafe extern "system" fn job_tramp(ctx: *mut c_void, canceled: bool) {
    let j = Box::from_raw(ctx as *mut JobBox);
    (j.0)(canceled)
}

pub unsafe fn submit(queue: usize, port: u32, job: Box<dyn FnOnce(bool) + Send>) -> i32 {
    if rt::active() {
        return rt::submit(queue, port, job);
    }
    match method(18) {
        Some((o, f)) => {
            let f: unsafe extern "system" fn(usize, usize, u32, *mut c_void, unsafe extern "system" fn(*mut c_void, bool)) -> i32 = core::mem::transmute(f);
            let ctx = Box::into_raw(Box::new(JobBox(job)));
            let hr = f(o, queue, port, ctx as *mut c_void, job_tramp);
            if hr < 0 {
                drop(Box::from_raw(ctx));
            }
            hr
        }
        None => {
            std::thread::spawn(move || job(false));
            S_OK
        }
    }
}

pub struct Payload {
    pub hr: i32,
    pub bytes: Vec<u8>,
    pub fixups: Vec<usize>,
}

impl Payload {
    pub fn status(hr: i32) -> Payload {
        Payload { hr, bytes: Vec::new(), fixups: Vec::new() }
    }
    pub fn data(bytes: Vec<u8>) -> Payload {
        Payload { hr: S_OK, bytes, fixups: Vec::new() }
    }
    pub fn value<T: Copy>(v: T) -> Payload {
        let n = core::mem::size_of::<T>();
        let mut b = vec![0u8; n];
        unsafe { core::ptr::write_unaligned(b.as_mut_ptr() as *mut T, v) };
        Payload::data(b)
    }
}

pub type Work = Box<dyn FnOnce(Arc<Cancel>) -> Payload + Send>;

#[derive(Default)]
pub struct Cancel {
    pub requested: AtomicBool,
    pub window: Arc<AtomicUsize>,
}

impl Cancel {
    pub fn is_set(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

struct Job {
    payload: Mutex<Option<Payload>>,
    work: Mutex<Option<Work>>,
    cancel: Arc<Cancel>,
}

struct SendPtr(usize);
unsafe impl Send for SendPtr {}

unsafe fn finish(block: *mut c_void, job: &Job, p: Payload) {
    let hr = if job.cancel.is_set() { E_ABORT } else { p.hr };
    let n = if hr >= 0 { p.bytes.len() } else { 0 };
    *job.payload.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    complete(block, hr, n);
}

unsafe extern "system" fn provider(op: u32, data: *const ProviderData) -> i32 {
    let j = (*data).context as *const Job;
    if j.is_null() {
        return S_OK;
    }
    match op {
        1 => {
            let work = (*j).work.lock().unwrap_or_else(|e| e.into_inner()).take();
            match work {
                Some(w) => {
                    let b = SendPtr((*data).block as usize);
                    let jp = SendPtr(j as usize);
                    let cancel = (*j).cancel.clone();
                    std::thread::spawn(move || {
                        let p = w(cancel);
                        finish(b.0 as *mut c_void, &*(jp.0 as *const Job), p);
                    });
                }
                None => {
                    let p = (*j).payload.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_else(|| Payload::status(S_OK));
                    finish((*data).block, &*j, p);
                }
            }
            E_PENDING
        }
        2 => {
            let g = (*j).payload.lock().unwrap_or_else(|e| e.into_inner());
            let Some(p) = g.as_ref() else { return E_FAIL };
            let n = p.bytes.len();
            if (*data).buffer_size < n || ((*data).buffer.is_null() && n > 0) {
                return E_NOT_SUFFICIENT_BUFFER;
            }
            let buf = (*data).buffer;
            core::ptr::copy_nonoverlapping(p.bytes.as_ptr(), buf, n);
            for &off in &p.fixups {
                let at = buf.add(off) as *mut u64;
                let v = core::ptr::read_unaligned(at);
                core::ptr::write_unaligned(at, buf as u64 + v);
            }
            S_OK
        }
        3 => {
            let job = &*j;
            job.cancel.requested.store(true, Ordering::Release);
            super::win::close_window(job.cancel.window.load(Ordering::Acquire));
            S_OK
        }
        4 => {
            drop(Box::from_raw(j as *mut Job));
            S_OK
        }
        _ => S_OK,
    }
}

unsafe fn start(block: *mut c_void, op: &'static [u8], job: Job) -> i32 {
    if block.is_null() {
        return E_INVALIDARG;
    }
    let ctx = Box::into_raw(Box::new(job));
    let hr = begin(block, ctx as *mut c_void, op.as_ptr() as usize, op.as_ptr(), provider);
    if hr < 0 {
        drop(Box::from_raw(ctx));
        return hr;
    }
    schedule(block, 0)
}

pub unsafe fn run(block: *mut c_void, op: &'static [u8], payload: Payload) -> i32 {
    start(block, op, Job { payload: Mutex::new(Some(payload)), work: Mutex::new(None), cancel: Arc::new(Cancel::default()) })
}

pub unsafe fn run_with(block: *mut c_void, op: &'static [u8], work: Work) -> i32 {
    start(block, op, Job { payload: Mutex::new(None), work: Mutex::new(Some(work)), cancel: Arc::new(Cancel::default()) })
}

pub unsafe fn status(block: *mut c_void) -> i32 {
    get_status(block, false)
}

pub unsafe fn value<T>(block: *mut c_void, op: &'static [u8], out: *mut T) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    get_result(block, op.as_ptr() as usize, core::mem::size_of::<T>(), out as *mut c_void, core::ptr::null_mut())
}

pub unsafe fn bytes(block: *mut c_void, op: &'static [u8], size: usize, buf: *mut c_void, used: *mut usize) -> i32 {
    get_result(block, op.as_ptr() as usize, size, buf, used)
}

pub unsafe fn size(block: *mut c_void, out: *mut usize) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    get_result_size(block, out)
}
