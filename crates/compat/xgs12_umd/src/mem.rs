use crate::host;
use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};

pub type AllocFn = unsafe extern "system" fn(usize, u64) -> *mut c_void;
pub type FreeFn = unsafe extern "system" fn(*mut c_void, u64);

extern "C" {
    fn _aligned_malloc(size: usize, align: usize) -> *mut c_void;
    fn _aligned_free(p: *mut c_void);
}

extern "system" {
    fn GetProcessHeap() -> *mut c_void;
    fn HeapAlloc(h: *mut c_void, flags: u32, n: usize) -> *mut c_void;
    fn HeapFree(h: *mut c_void, flags: u32, p: *mut c_void) -> i32;
}

pub const ATTR_OBJECT: u64 = 0x0480_0040;
pub const ATTR_BUFFER: u64 = 0x0480_0078;

pub fn attributes(category: u32) -> u64 {
    let align: u32 = match category {
        0x46 | 0x66 | 0x67 | 0x72 | 0x75 | 0x9f => 0x20,
        0x89 => 8,
        _ => 0x10,
    };
    let hi: u64 = if category == 0x4b || category == 0x9f { 3 } else { 0 };
    let log2 = 31 - align.leading_zeros() as u64;
    (((log2 & 0x1f) | (hi << 5)) << 24) | (category as u64 & 0x3fff) | (1 << 23)
}

unsafe extern "system" fn heap_alloc(size: usize, _attributes: u64) -> *mut c_void {
    HeapAlloc(GetProcessHeap(), 0, size.max(1))
}

unsafe extern "system" fn heap_free(p: *mut c_void, _attributes: u64) {
    if !p.is_null() {
        HeapFree(GetProcessHeap(), 0, p);
    }
}

pub fn xmem_alloc() -> AllocFn {
    let p = host::proc_in(host::xmem(), b"XMemAlloc\0");
    if p == 0 { heap_alloc } else { unsafe { core::mem::transmute::<usize, AllocFn>(p) } }
}

pub fn xmem_free() -> FreeFn {
    let p = host::proc_in(host::xmem(), b"XMemFree\0");
    if p == 0 { heap_free } else { unsafe { core::mem::transmute::<usize, FreeFn>(p) } }
}

static ALLOC_HOOK: AtomicUsize = AtomicUsize::new(0);
static FREE_HOOK: AtomicUsize = AtomicUsize::new(0);

pub fn set_allocation_hooks(alloc: usize, free: usize) {
    if alloc == 0 {
        ALLOC_HOOK.store(xmem_alloc() as usize, Ordering::Release);
        FREE_HOOK.store(xmem_free() as usize, Ordering::Release);
    } else {
        ALLOC_HOOK.store(alloc, Ordering::Release);
        FREE_HOOK.store(free, Ordering::Release);
    }
}

pub fn allocation_hooks() -> (usize, usize) {
    (ALLOC_HOOK.load(Ordering::Acquire), FREE_HOOK.load(Ordering::Acquire))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    XMem,
    Hooks,
}

pub fn alloc(src: Source, size: usize, attributes: u64) -> *mut c_void {
    match src {
        Source::XMem => unsafe { xmem_alloc()(size, attributes) },
        Source::Hooks => {
            let mut a = ALLOC_HOOK.load(Ordering::Acquire);
            if a == 0 {
                set_allocation_hooks(0, 0);
                a = ALLOC_HOOK.load(Ordering::Acquire);
            }
            let f: AllocFn = unsafe { core::mem::transmute(a) };
            unsafe { f(size, attributes) }
        }
    }
}

pub fn free(src: Source, p: *mut c_void, attributes: u64) {
    match src {
        Source::XMem => unsafe { xmem_free()(p, attributes) },
        Source::Hooks => {
            let mut f = FREE_HOOK.load(Ordering::Acquire);
            if f == 0 && ALLOC_HOOK.load(Ordering::Acquire) == 0 {
                set_allocation_hooks(0, 0);
                f = FREE_HOOK.load(Ordering::Acquire);
            }
            if f != 0 {
                let f: FreeFn = unsafe { core::mem::transmute(f) };
                unsafe { f(p, attributes) }
            }
        }
    }
}

pub unsafe fn cpu_mem_alloc(size: usize, align: u32) -> *mut c_void {
    let align = align.max(8) as usize;
    let raw = _aligned_malloc(size.wrapping_add(align), 16);
    if raw.is_null() {
        return raw;
    }
    let p = (raw as usize).wrapping_add(align) & align.wrapping_neg();
    *((p - 8) as *mut *mut c_void) = raw;
    p as *mut c_void
}

pub unsafe fn cpu_mem_free(p: *mut c_void) {
    if !p.is_null() {
        _aligned_free(*((p as usize - 8) as *const *mut c_void));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_attribute_encoding() {
        assert_eq!(attributes(0x40), ATTR_OBJECT);
        assert_eq!(attributes(0x78), ATTR_BUFFER);
        assert_eq!(attributes(0x89), 0x0380_0089);
        assert_eq!(attributes(0x9f), 0x6580_009f);
    }

    #[test]
    fn cpu_mem_alignment_and_header() {
        unsafe {
            for &(size, align) in &[(1usize, 0u32), (100, 8), (64, 64), (4096, 4096), (7, 3)] {
                let p = cpu_mem_alloc(size, align);
                assert!(!p.is_null());
                let a = (align.max(8)) as usize;
                if a.is_power_of_two() {
                    assert_eq!(p as usize % a, 0);
                }
                let raw = *((p as usize - 8) as *const usize);
                assert!(raw < p as usize && p as usize - raw <= a);
                core::ptr::write_bytes(p as *mut u8, 0xab, size);
                cpu_mem_free(p);
            }
            cpu_mem_free(core::ptr::null_mut());
        }
    }
}
