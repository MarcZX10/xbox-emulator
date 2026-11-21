use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::OnceLock;
use xtfcommon::bena::Benaphore;
use xtfcommon::sys::*;

pub type AssertHandler = unsafe extern "C" fn(*const u8, *const u8, u32, *const u8, *const c_void) -> HRESULT;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub func: usize,
    pub id: u32,
    pub kind: u8,
}

pub struct ErrSys {
    handlers: UnsafeCell<Vec<Entry>>,
    counter: UnsafeCell<u32>,
    stack: UnsafeCell<Vec<bool>>,
    enabled: AtomicU32,
    lock: Benaphore,
    busy: AtomicI32,
}

unsafe impl Send for ErrSys {}
unsafe impl Sync for ErrSys {}

pub static REENTRANT_TID: AtomicU32 = AtomicU32::new(0);

const ERR_FILE: &[u8] = b"xbox\\devex\\xtf\\dll\\xtfbaseservices\\errormanager.cpp\0";

pub unsafe extern "C" fn break_if_debugging(_f: *const u8, _fn: *const u8, _l: u32, _m: *const u8, _v: *const c_void) -> HRESULT {
    if IsDebuggerPresent() != 0 {
        DebugBreak();
    }
    0
}

static ERRSYS: OnceLock<ErrSys> = OnceLock::new();

pub fn sys() -> &'static ErrSys {
    ERRSYS.get_or_init(|| {
        let s = ErrSys {
            handlers: UnsafeCell::new(Vec::new()),
            counter: UnsafeCell::new(1),
            stack: UnsafeCell::new(Vec::new()),
            enabled: AtomicU32::new(1),
            lock: Benaphore::new(),
            busy: AtomicI32::new(0),
        };
        unsafe {
            s.register(crate::manager::XtfLogAssertMessage as AssertHandler as usize, 2);
            s.register(break_if_debugging as AssertHandler as usize, 2);
        }
        s
    })
}

impl ErrSys {
    pub unsafe fn handlers(&self) -> Vec<Entry> {
        self.lock.lock();
        let v = (*self.handlers.get()).clone();
        self.lock.unlock();
        v
    }

    pub fn stack_len(&self) -> usize {
        self.lock.lock();
        let n = unsafe { (*self.stack.get()).len() };
        self.lock.unlock();
        n
    }

    pub unsafe fn register(&self, func: usize, kind: u8) -> u32 {
        if func == 0 {
            return 0;
        }
        self.lock.lock();
        let hs = &mut *self.handlers.get();
        if let Some(i) = hs.iter().position(|e| e.func == func) {
            if hs[i].kind == kind {
                let id = hs[i].id;
                self.lock.unlock();
                return id;
            }
            hs.remove(i);
        }
        let c = &mut *self.counter.get();
        *c += 1;
        let e = Entry { func, id: *c, kind };
        match kind {
            0 => hs.insert(0, e),
            2 => hs.push(e),
            _ => match hs.iter().position(|x| x.kind == 2) {
                Some(p) => hs.insert(p, e),
                None => hs.push(e),
            },
        }
        let id = *c;
        self.lock.unlock();
        id
    }

    pub unsafe fn remove_where(&self, pred: impl Fn(&Entry) -> bool) -> bool {
        self.lock.lock();
        let hs = &mut *self.handlers.get();
        let r = match hs.iter().position(|e| pred(e)) {
            Some(i) => {
                hs.remove(i);
                true
            }
            None => false,
        };
        self.lock.unlock();
        r
    }
}

pub fn internal_assert(file: &'static [u8], func: &'static [u8], line: u32, expr: &'static [u8]) {
    if XtfErrAreAssertsEnabled() {
        unsafe { XtfErrOnAssertV(file.as_ptr(), func.as_ptr(), line, expr.as_ptr(), core::ptr::null()) };
    }
}

#[no_mangle]
pub extern "C" fn XtfErrAreAssertsEnabled() -> bool {
    sys().enabled.load(Ordering::Acquire) != 0
}

#[no_mangle]
pub unsafe extern "C" fn XtfErrOnAssertV(file: *const u8, func: *const u8, line: u32, fmt: *const u8, va: *const c_void) {
    let s = sys();
    if s.busy.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_err() {
        let msg: &[u8] = if GetCurrentThreadId() == REENTRANT_TID.load(Ordering::Acquire) { b"Re-entrant call to assert.\0" } else { b"Multiple calls to assert.\0" };
        OutputDebugStringA(msg.as_ptr());
        DebugBreak();
        return;
    }
    let list = s.handlers();
    for e in list {
        let h: AssertHandler = core::mem::transmute(e.func);
        h(file, func, line, fmt, va);
    }
    let _ = s.busy.compare_exchange(1, 0, Ordering::AcqRel, Ordering::Acquire);
}

#[no_mangle]
pub unsafe extern "C" fn XtfErrRegisterAssertHandlerPtr(func: Option<AssertHandler>, kind: u8) -> u32 {
    sys().register(func.map_or(0, |f| f as usize), kind)
}

#[no_mangle]
pub unsafe extern "C" fn XtfErrRemoveAssertHandler(id: u32) -> bool {
    sys().remove_where(|e| e.id == id)
}

#[no_mangle]
pub unsafe extern "C" fn XtfErrRemoveAssertHandlerByPtr(func: Option<AssertHandler>) -> bool {
    let f = func.map_or(0, |f| f as usize);
    sys().remove_where(|e| e.func == f)
}

#[no_mangle]
pub unsafe extern "C" fn XtfErrSetBreakOnAssertIfDebugging(on: u8) {
    let s = sys();
    if on != 0 {
        s.register(break_if_debugging as AssertHandler as usize, 2);
    } else {
        s.remove_where(|e| e.func == break_if_debugging as AssertHandler as usize);
    }
}

unsafe fn log5(level: u32, func: &[u8], line: u32, fmt: &[u8], args: &[u64]) {
    let empty = crate::ctx::RoutingPtr::empty();
    crate::manager::XtfLogMessage(level, &empty, ERR_FILE.as_ptr(), func.as_ptr(), line, fmt.as_ptr(), args.as_ptr() as *const c_void, 1);
}

#[no_mangle]
pub unsafe extern "C" fn XtfErrPushAssertsEnabled(on: u8) {
    let s = sys();
    s.lock.lock();
    let st = &mut *s.stack.get();
    let cur = s.enabled.load(Ordering::Acquire) != 0;
    st.push(cur);
    if crate::manager::XtfLogGetMainLevel() as i32 > 0 {
        log5(5, b"xtfbaseservices::ErrorSystemImpl::PushAssertsEnabled\0", 0xb8, b"Assert level pushed to %d, storage is %d count\0", &[cur as u64, st.len() as u64]);
    }
    s.enabled.store(on as u32, Ordering::Release);
    s.lock.unlock();
}

#[no_mangle]
pub unsafe extern "C" fn XtfErrPopAssertsEnabled() {
    let s = sys();
    s.lock.lock();
    let st = &mut *s.stack.get();
    if let Some(v) = st.pop() {
        let old = s.enabled.load(Ordering::Acquire) != 0;
        s.enabled.store(v as u32, Ordering::Release);
        if crate::manager::XtfLogGetMainLevel() as i32 > 0 {
            log5(5, b"xtfbaseservices::ErrorSystemImpl::PopAssertsEnabled\0", 0xc5, b"Error: Assert level POPPED to from %d to %d (%d left in storage)\0", &[old as u64, v as u64, st.len() as u64]);
        }
    } else if crate::manager::XtfLogGetMainLevel() as i32 > 0 {
        log5(2, b"xtfbaseservices::ErrorSystemImpl::PopAssertsEnabled\0", 0xc9, b"Error: Assert level POPPED with nothing in storage\0", &[]);
    }
    s.lock.unlock();
}
