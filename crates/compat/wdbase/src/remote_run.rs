use crate::crt::{_getwch, _kbhit, fflush, fprintf, __acrt_iob_func};
use crate::stl::WString;
use crate::strings::Guid;
use crate::w;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};

pub const IID_IUNKNOWN: Guid = Guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
pub const IID_REMOTE_RUN_CALLBACK: Guid = Guid(0xd74b_f3b9, 0x6814, 0x4e32, [0x84, 0xb1, 0xd1, 0x06, 0xf4, 0xa3, 0x97, 0x2b]);
pub const IID_REMOTE_RUN_CLIENT: Guid = Guid(0x8562_7158, 0x492c, 0x4cde, [0x86, 0xe8, 0xaf, 0x22, 0xc3, 0xfb, 0x90, 0x7c]);
pub const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut *mut c_void) -> i32;
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
    fn GetStdHandle(n: u32) -> *mut c_void;
    fn RaiseException(code: u32, flags: u32, n: u32, args: *const usize);
}

#[link(name = "oleaut32")]
extern "system" {
    fn SysAllocString(s: *const u16) -> *mut u16;
}

#[repr(C)]
pub struct Callback {
    pub vtbl: *const CallbackVtbl,
    pub refs: AtomicI32,
}

#[repr(C)]
pub struct CallbackVtbl {
    pub query_interface: unsafe extern "system" fn(*mut Callback, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut Callback) -> u32,
    pub release: unsafe extern "system" fn(*mut Callback) -> u32,
    pub read_input: unsafe extern "system" fn(*mut Callback, *mut *mut u16) -> i32,
    pub write_output: unsafe extern "system" fn(*mut Callback, *const u16) -> i32,
    pub write_error: unsafe extern "system" fn(*mut Callback, *const u16) -> i32,
}

pub static CALLBACK_VTBL: CallbackVtbl = CallbackVtbl {
    query_interface: callback_query_interface,
    add_ref: callback_add_ref,
    release: callback_release,
    read_input: callback_read_input,
    write_output: callback_write_output,
    write_error: callback_write_error,
};

fn guid_eq(a: &Guid, b: &Guid) -> bool {
    a.0 == b.0 && a.1 == b.1 && a.2 == b.2 && a.3 == b.3
}

pub unsafe extern "system" fn callback_query_interface(this: *mut Callback, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    if guid_eq(&*iid, &IID_IUNKNOWN) || guid_eq(&*iid, &IID_REMOTE_RUN_CALLBACK) {
        *out = this as *mut c_void;
        ((*(*this).vtbl).add_ref)(this);
        return 0;
    }
    E_NOINTERFACE
}

pub unsafe extern "system" fn callback_add_ref(this: *mut Callback) -> u32 {
    ((*this).refs.fetch_add(1, Ordering::SeqCst) + 1) as u32
}

pub unsafe extern "system" fn callback_release(this: *mut Callback) -> u32 {
    let n = (*this).refs.fetch_sub(1, Ordering::SeqCst) - 1;
    if n == 0 {
        crate::stl::operator_delete(this as *mut u8);
    }
    n as u32
}

pub unsafe extern "system" fn callback_read_input(_this: *mut Callback, out: *mut *mut u16) -> i32 {
    let mut buf = [0u16; 0x800];
    GetStdHandle(0xffff_fff6);
    let mut n = 0usize;
    while _kbhit() != 0 {
        if n >= 0x7ff {
            break;
        }
        buf[n] = _getwch();
        n += 1;
    }
    buf[n] = 0;
    if n == 0 {
        *out = core::ptr::null_mut();
        return 0;
    }
    let s = SysAllocString(buf.as_ptr());
    *out = s;
    if s.is_null() {
        crate::strings::E_OUTOFMEMORY
    } else {
        0
    }
}

unsafe fn write_to(stream: u32, text: *const u16) -> i32 {
    let mut s = WString::new();
    WString::assign_cstr(&mut s, text);
    if s.size != 0 {
        fprintf(stream, b"%ls\0", &[WString::data(&s) as usize]);
        fflush(__acrt_iob_func(stream));
    }
    WString::tidy(&mut s);
    0
}

pub unsafe extern "system" fn callback_write_output(_this: *mut Callback, text: *const u16) -> i32 {
    write_to(1, text)
}

pub unsafe extern "system" fn callback_write_error(_this: *mut Callback, text: *const u16) -> i32 {
    write_to(2, text)
}

type CreateRemoteRunClient = unsafe extern "system" fn(*const u16, *const Guid, *mut *mut c_void) -> i32;
type Release = unsafe extern "system" fn(*mut c_void) -> u32;
type Run = unsafe extern "system" fn(*mut c_void, *const u16, *const u16, u32, u32, *mut Callback, *mut c_void) -> i32;

unsafe fn release(p: &mut *mut c_void) {
    let obj = *p;
    if !obj.is_null() {
        *p = core::ptr::null_mut();
        let f: Release = core::mem::transmute(*(*(obj as *const *const usize)).add(2));
        f(obj);
    }
}

pub fn run_flags(wait: bool, detach: bool) -> u32 {
    let base = 0x4000_0000u32 + ((wait as u32) << 2);
    if detach {
        base & 0x3fff_fffe
    } else {
        base
    }
}

#[no_mangle]
pub unsafe extern "C" fn ExecuteRemoteRun(target: *const u16, command: *const u16, arguments: *const u16, wait: bool, detach: bool, context: *mut c_void) -> i32 {
    let mut client: *mut c_void = core::ptr::null_mut();
    let mut callback = Callback { vtbl: &CALLBACK_VTBL, refs: AtomicI32::new(1) };
    let mut address = WString::new();
    let flags = run_flags(wait, detach);
    WString::assign_cstr(&mut address, w!("127.0.0.1/").as_ptr());
    WString::append_cstr(&mut address, target);
    let name = w!("XtfRemoteRun.dll");
    let mut module: *mut c_void = core::ptr::null_mut();
    let mut create: usize = 0;
    let mut hr;
    if GetModuleHandleExW(2, name.as_ptr(), &mut module) == 0 {
        module = LoadLibraryExW(name.as_ptr(), core::ptr::null_mut(), 0);
    }
    if !module.is_null() {
        create = GetProcAddress(module, b"XtfCreateRemoteRunClient\0".as_ptr()) as usize;
    }
    if create == 0 {
        let e = GetLastError();
        hr = if e as i32 <= 0 { e as i32 } else { ((e & 0xffff) | 0x8007_0000) as i32 };
        if hr < 0 {
            WString::tidy(&mut address);
            return hr;
        }
        RaiseException(0xc000_0005, 0, 0, core::ptr::null());
    }
    let create: CreateRemoteRunClient = core::mem::transmute(create);
    release(&mut client);
    hr = create(WString::data(&address), &IID_REMOTE_RUN_CLIENT, &mut client);
    if hr >= 0 {
        let run: Run = core::mem::transmute(*(*(client as *const *const usize)).add(3));
        let cb = if flags & 0xc000_0000 != 0 { &mut callback as *mut Callback } else { core::ptr::null_mut() };
        hr = run(client, command, arguments, flags, 100, cb, context);
    }
    WString::tidy(&mut address);
    release(&mut client);
    hr
}
