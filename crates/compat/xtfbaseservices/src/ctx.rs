use core::ffi::c_void;
use xtfcommon::msvc::{operator_delete, operator_new};
use xtfcommon::printf::format_alloc;
use xtfcommon::sys::*;

#[repr(C)]
pub struct RoutingPtr {
    pub ptr: *mut c_void,
    pub rc: *mut i32,
    pub tid: i32,
    pub pad: u32,
}

impl RoutingPtr {
    pub const fn empty() -> Self {
        RoutingPtr { ptr: core::ptr::null_mut(), rc: core::ptr::null_mut(), tid: -1, pad: 0 }
    }
}

pub type Formatter = unsafe extern "C" fn(*mut Ctx, *mut *mut u8, *const u8, *const c_void) -> i32;

#[repr(C)]
pub struct Ctx {
    pub cfg: RoutingPtr,
    pub level: u32,
    pub pid: u32,
    pub tid: u32,
    pub error: u32,
    pub time: u64,
    pub prefix: *const u8,
    pub prefix_len: u32,
    pub pad1: u32,
    pub func: *const u8,
    pub r48: u64,
    pub line: u32,
    pub color: u16,
    pub pad2: u16,
    pub flags: u32,
    pub pad3: u32,
    pub text: *mut u8,
    pub text_len: i32,
    pub pad4: u32,
    pub msg: *mut u8,
    pub msg_len: i32,
    pub pad5: u32,
}

const HELPERS_FILE: &[u8] = b"xbox\\devex\\xtf\\lib\\xtfbase\\XtfBaseHelpersNoSTL.h\0";

unsafe fn check_single_thread(p: *const RoutingPtr, func: &'static [u8], line: u32) {
    if (*p).rc.is_null() {
        return;
    }
    let cur = GetCurrentThreadId() as i32;
    if (*p).tid != cur && (*p).tid != -1 && cur != -1 {
        crate::errsys::internal_assert(HELPERS_FILE, func, line, b"EnsureSingleThreaded()\0");
    }
}

pub unsafe fn cfg_get(p: *const RoutingPtr) -> *mut c_void {
    check_single_thread(p, b"xtfbase::SharedPtrSingleThreaded<class xtfbase::log::MessageRoutingAndConfig>::get\0", 0xec);
    if (*p).rc.is_null() {
        core::ptr::null_mut()
    } else {
        (*p).ptr
    }
}

pub unsafe fn cfg_release(p: *mut RoutingPtr) {
    let obj = (*p).ptr;
    let rc = (*p).rc;
    if obj.is_null() || rc.is_null() {
        return;
    }
    if *rc == 1 {
        let vt = *(obj as *const *const unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void);
        (*vt)(obj, 1);
    }
    *rc -= 1;
    if *rc == 0 {
        operator_delete(rc as *mut c_void);
    }
    (*p).rc = core::ptr::null_mut();
    (*p).ptr = core::ptr::null_mut();
}

pub unsafe fn cfg_assign(dst: *mut RoutingPtr, src: *const RoutingPtr) {
    if dst as *const RoutingPtr == src {
        return;
    }
    cfg_release(dst);
    (*dst).ptr = (*src).ptr;
    (*dst).rc = (*src).rc;
    (*dst).tid = (*src).tid;
    check_single_thread(dst, b"xtfbase::SharedPtrSingleThreaded<class xtfbase::log::MessageRoutingAndConfig>::operator =\0", 0xcc);
    if !(*dst).rc.is_null() {
        *(*dst).rc += 1;
    }
}

unsafe fn cfg_duplicate(src: *mut c_void, out: *mut RoutingPtr) {
    let vt = *(src as *const *const usize);
    let clone: unsafe extern "C" fn(*mut c_void) -> *mut c_void = core::mem::transmute(*vt.add(2));
    (*out).ptr = clone(src);
    let rc = operator_new(4) as *mut i32;
    *rc = 1;
    (*out).rc = rc;
    (*out).tid = -1;
    if cfg_get(out).is_null() {
        crate::errsys::internal_assert(b"xbox\\devex\\xtf\\lib\\xtfbase\\xtfbaseloggingsystem.cpp\0", b"xtfbase::log::MessageRoutingAndConfig::Duplicate\0", 0x188, b"result\0");
    }
    let n = cfg_get(out);
    let nvt = *(n as *const *const usize);
    let copy: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = core::mem::transmute(*nvt.add(3));
    copy(n, src);
}

pub unsafe fn ctx_construct(c: *mut Ctx, level: u32, cfg: *const RoutingPtr, func: *const u8, line: u32, fmt: *const u8, va: *const c_void, flags: u32) {
    core::ptr::write_bytes(c as *mut u8, 0, core::mem::size_of::<Ctx>());
    (*c).cfg.tid = -1;
    if !fmt.is_null() {
        (*c).msg_len = format_alloc(&mut (*c).msg, fmt, va, false);
    }
    let mine = cfg_get(&(*c).cfg);
    let src = cfg_get(cfg);
    if mine.is_null() != src.is_null() {
        if !src.is_null() {
            let mut tmp = RoutingPtr::empty();
            cfg_duplicate(src, &mut tmp);
            cfg_assign(&mut (*c).cfg, &tmp);
            cfg_release(&mut tmp);
        } else {
            cfg_release(&mut (*c).cfg);
        }
    }
    (*c).level = level;
    (*c).pid = GetCurrentProcessId();
    (*c).tid = GetCurrentThreadId();
    (*c).func = func;
    (*c).line = line;
    (*c).color = crate::manager::XtfLogGetTextColors(level);
    let mut ft = 0u64;
    GetSystemTimePreciseAsFileTime(&mut ft);
    (*c).time = ft;
    (*c).prefix = core::ptr::null();
    (*c).prefix_len = 0;
    (*c).flags = flags;
    let obj = cfg_get(&(*c).cfg);
    let custom: Option<Formatter> = if obj.is_null() { None } else { core::mem::transmute::<usize, Option<Formatter>>(*((obj as *const usize).add(1))) };
    let f: Option<Formatter> = match custom {
        Some(f) => Some(f),
        None if flags & 4 != 0 => Some(default_formatter),
        None => None,
    };
    (*c).text_len = match f {
        Some(f) => f(c, &mut (*c).text, fmt, va),
        None => format_alloc(&mut (*c).text, fmt, va, flags & 1 != 0),
    };
}

pub unsafe fn ctx_destroy(c: *mut Ctx) {
    if !(*c).text.is_null() {
        free((*c).text as *mut c_void);
    }
    if !(*c).msg.is_null() {
        free((*c).msg as *mut c_void);
    }
    cfg_release(&mut (*c).cfg);
}

pub unsafe extern "C" fn default_formatter(c: *mut Ctx, out: *mut *mut u8, fmt: *const u8, _va: *const c_void) -> i32 {
    *out = core::ptr::null_mut();
    if fmt.is_null() {
        return 0;
    }
    let crlf: *const u8 = if (*c).flags & 1 != 0 { b"\r\n\0".as_ptr() } else { b"\0".as_ptr() };
    let tid = (*c).tid as u64;
    let (f, args): (&[u8], [u64; 5]) = match (*c).level {
        1 => (b"[XTFERR:%u] Assertion failed at %hs(%u):  %hs%s\0", [tid, (*c).func as u64, (*c).line as u64, (*c).msg as u64, crlf as u64]),
        2 => (b"[XTFERR:%u] %hs [%hs(%u)]%s\0", [tid, (*c).msg as u64, (*c).func as u64, (*c).line as u64, crlf as u64]),
        3 => (b"[XTFWRN:%u] %hs [%hs(%u)]%s\0", [tid, (*c).msg as u64, (*c).func as u64, (*c).line as u64, crlf as u64]),
        _ => (b"[XTFTRC:%u] %hs%s\0", [tid, (*c).msg as u64, crlf as u64, 0, 0]),
    };
    format_alloc(out, f.as_ptr(), args.as_ptr() as *const c_void, false)
}
