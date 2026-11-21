use crate::alloc::{Allocator, XbscAllocator};
use core::ffi::c_char;

pub const TAG_ERROR: u32 = 0;
pub const TAG_PIPELINE_OUTPUT: u32 = 6;
pub const TAG_ROOT_SIGNATURE: u32 = 7;
pub const TAG_COMPRESSED: u32 = 0xa;
pub const TAG_DECOMPRESSED: u32 = 0xb;

pub const RESULT_OK: u32 = 0;
pub const RESULT_FAIL: u32 = 1;

pub static UNKNOWN_ERROR: &[u8] = b"unknown error\0";

#[repr(C)]
pub struct Handle {
    pub vtbl: usize,
    pub code: u32,
    pub pad0: u32,
    pub alloc: *mut Allocator,
    pub tag: u32,
    pub pad1: u32,
    pub owner: *mut Allocator,
    pub data: *mut u8,
    pub size: usize,
}

pub enum Fail {
    Msg(String),
    BadAlloc,
}

impl Fail {
    pub fn msg(s: &str) -> Fail {
        Fail::Msg(s.to_string())
    }
}

pub struct Buf {
    pub alloc: *mut Allocator,
    pub ptr: *mut u8,
    pub len: usize,
}

impl Buf {
    pub unsafe fn new(alloc: *mut Allocator, len: usize, what: &str) -> Result<Buf, Fail> {
        let ptr = Allocator::alloc(alloc, len);
        if ptr.is_null() {
            return Err(Fail::msg(what));
        }
        Allocator::add_ref(alloc);
        Ok(Buf { alloc, ptr, len })
    }

    pub fn as_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    pub fn into_raw(self) -> (*mut Allocator, *mut u8) {
        let r = (self.alloc, self.ptr);
        core::mem::forget(self);
        r
    }
}

impl Drop for Buf {
    fn drop(&mut self) {
        unsafe {
            Allocator::free(self.alloc, self.ptr);
            Allocator::release(self.alloc);
        }
    }
}

impl Handle {
    pub unsafe fn new(alloc: *mut Allocator, tag: u32) -> *mut Handle {
        let h = Allocator::alloc(alloc, core::mem::size_of::<Handle>()) as *mut Handle;
        if h.is_null() {
            return h;
        }
        Allocator::add_ref(alloc);
        h.write(Handle {
            vtbl: 0,
            code: RESULT_FAIL,
            pad0: 0,
            alloc,
            tag,
            pad1: 0,
            owner: core::ptr::null_mut(),
            data: core::ptr::null_mut(),
            size: 0,
        });
        h
    }

    pub unsafe fn with_buffer(alloc: *mut Allocator, tag: u32, buf: Buf, size: usize) -> *mut Handle {
        let h = Handle::new(alloc, tag);
        if h.is_null() {
            return h;
        }
        let (owner, data) = buf.into_raw();
        (*h).owner = owner;
        (*h).data = data;
        (*h).size = size;
        (*h).code = RESULT_OK;
        h
    }

    pub unsafe fn error(alloc: *mut Allocator, msg: &str) -> *mut Handle {
        let h = Handle::new(alloc, TAG_ERROR);
        if h.is_null() {
            return h;
        }
        let m = Allocator::alloc(alloc, msg.len() + 1);
        if !m.is_null() {
            core::ptr::copy_nonoverlapping(msg.as_ptr(), m, msg.len());
            *m.add(msg.len()) = 0;
            Allocator::add_ref(alloc);
            (*h).owner = alloc;
            (*h).data = m;
            (*h).size = msg.len();
        }
        h
    }

    pub unsafe fn destroy(h: *mut Handle) {
        if h.is_null() {
            return;
        }
        let owner = (*h).owner;
        if !owner.is_null() {
            Allocator::free(owner, (*h).data);
            Allocator::release(owner);
        }
        let a = (*h).alloc;
        Allocator::free(a, h as *mut u8);
        Allocator::release(a);
    }

    pub unsafe fn message(h: *const Handle) -> *const c_char {
        if h.is_null() || (*h).tag != TAG_ERROR {
            return UNKNOWN_ERROR.as_ptr() as *const c_char;
        }
        (*h).data as *const c_char
    }

    pub unsafe fn code(h: *const Handle) -> u32 {
        if h.is_null() {
            RESULT_FAIL
        } else {
            (*h).code
        }
    }

    pub unsafe fn buffer(h: *const Handle, tag: u32, data: *mut *const u8, size: *mut usize) -> u32 {
        if data.is_null() || size.is_null() {
            return RESULT_FAIL;
        }
        if h.is_null() || (*h).tag != tag {
            return RESULT_FAIL;
        }
        *data = (*h).data;
        *size = (*h).size;
        RESULT_OK
    }
}

pub unsafe fn run(desc: *const XbscAllocator, f: impl FnOnce(*mut Allocator) -> Result<*mut Handle, Fail>) -> *mut Handle {
    let a = Allocator::from_desc(desc);
    if a.is_null() {
        return core::ptr::null_mut();
    }
    let r = match f(a) {
        Ok(h) => h,
        Err(Fail::Msg(m)) => Handle::error(a, &m),
        Err(Fail::BadAlloc) => core::ptr::null_mut(),
    };
    Allocator::release(a);
    r
}
