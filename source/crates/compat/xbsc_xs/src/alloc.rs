use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

pub type AllocFn = unsafe extern "system" fn(*mut c_void, usize) -> *mut c_void;
pub type FreeFn = unsafe extern "system" fn(*mut c_void, *mut c_void);

#[repr(C)]
pub struct XbscAllocator {
    pub alloc: Option<AllocFn>,
    pub free: Option<FreeFn>,
    pub ctx: *mut c_void,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetProcessHeap() -> *mut c_void;
    fn HeapAlloc(h: *mut c_void, flags: u32, n: usize) -> *mut c_void;
    fn HeapFree(h: *mut c_void, flags: u32, p: *mut c_void) -> i32;
}

unsafe extern "system" fn heap_alloc_zero(h: *mut c_void, n: usize) -> *mut c_void {
    HeapAlloc(h, 8, n)
}

unsafe extern "system" fn heap_alloc(h: *mut c_void, n: usize) -> *mut c_void {
    HeapAlloc(h, 0, n)
}

unsafe extern "system" fn heap_free(h: *mut c_void, p: *mut c_void) {
    HeapFree(h, 0, p);
}

#[repr(C)]
pub struct Allocator {
    vtbl: usize,
    refs: AtomicU32,
    ctx: *mut c_void,
    alloc: AllocFn,
    free: FreeFn,
    reserved: usize,
}

impl Allocator {
    pub unsafe fn from_desc(desc: *const XbscAllocator) -> *mut Allocator {
        let (ctx, a, f) = if desc.is_null() {
            (GetProcessHeap(), heap_alloc as AllocFn, heap_free as FreeFn)
        } else {
            let d = &*desc;
            match (d.alloc, d.free) {
                (Some(a), Some(f)) => (d.ctx, a, f),
                (None, None) => return core::ptr::null_mut(),
                _ => (GetProcessHeap(), heap_alloc_zero as AllocFn, heap_free as FreeFn),
            }
        };
        let p = a(ctx, core::mem::size_of::<Allocator>()) as *mut Allocator;
        if p.is_null() {
            return p;
        }
        p.write(Allocator { vtbl: 0, refs: AtomicU32::new(1), ctx, alloc: a, free: f, reserved: 0 });
        p
    }

    pub unsafe fn add_ref(p: *mut Allocator) {
        if !p.is_null() {
            (*p).refs.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub unsafe fn release(p: *mut Allocator) {
        if p.is_null() {
            return;
        }
        if (*p).refs.fetch_sub(1, Ordering::SeqCst) == 1 {
            let (ctx, f) = ((*p).ctx, (*p).free);
            f(ctx, p as *mut c_void);
        }
    }

    pub unsafe fn alloc(p: *mut Allocator, n: usize) -> *mut u8 {
        ((*p).alloc)((*p).ctx, n) as *mut u8
    }

    pub unsafe fn free(p: *mut Allocator, q: *mut u8) {
        if !q.is_null() {
            ((*p).free)((*p).ctx, q as *mut c_void)
        }
    }

    pub unsafe fn refs(p: *mut Allocator) -> u32 {
        (*p).refs.load(Ordering::SeqCst)
    }
}
