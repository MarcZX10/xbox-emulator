use crate::sys::{free, malloc};
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};

pub unsafe fn operator_new(n: usize) -> *mut c_void {
    malloc(n)
}

pub unsafe fn operator_delete(p: *mut c_void) {
    if !p.is_null() {
        free(p);
    }
}

pub unsafe fn allocate_bytes(n: usize) -> *mut c_void {
    if n == 0 {
        return core::ptr::null_mut();
    }
    if n >= 0x1000 {
        let block = operator_new(n + 0x27) as usize;
        if block == 0 {
            return core::ptr::null_mut();
        }
        let aligned = (block + 0x27) & !0x1F;
        *((aligned - 8) as *mut usize) = block;
        aligned as *mut c_void
    } else {
        operator_new(n)
    }
}

pub unsafe fn deallocate_bytes(p: *mut c_void, n: usize) {
    if p.is_null() {
        return;
    }
    if n >= 0x1000 {
        let block = *((p as usize - 8) as *const usize);
        operator_delete(block as *mut c_void);
    } else {
        operator_delete(p);
    }
}

#[repr(C)]
pub struct WString {
    pub bx: [u16; 8],
    pub size: usize,
    pub cap: usize,
}

const WMAX: usize = 0x7FFF_FFFF_FFFF_FFFE;

impl WString {
    pub fn empty() -> Self {
        WString { bx: [0; 8], size: 0, cap: 7 }
    }

    pub fn is_large(&self) -> bool {
        self.cap >= 8
    }

    pub fn data_ptr(&self) -> *const u16 {
        if self.is_large() {
            usize::from_ne_bytes(self.ptr_bytes()) as *const u16
        } else {
            self.bx.as_ptr()
        }
    }

    fn ptr_bytes(&self) -> [u8; 8] {
        let mut b = [0u8; 8];
        for i in 0..4 {
            b[i * 2..i * 2 + 2].copy_from_slice(&self.bx[i].to_ne_bytes());
        }
        b
    }

    fn set_ptr(&mut self, p: *mut u16) {
        let b = (p as usize).to_ne_bytes();
        for i in 0..4 {
            self.bx[i] = u16::from_ne_bytes([b[i * 2], b[i * 2 + 1]]);
        }
    }

    pub fn as_slice(&self) -> &[u16] {
        unsafe { core::slice::from_raw_parts(self.data_ptr(), self.size) }
    }

    pub fn c_str(&self) -> *const u16 {
        self.data_ptr()
    }

    fn growth(&self, req: usize) -> usize {
        let masked = req | 7;
        if masked > WMAX {
            return WMAX;
        }
        let old = self.cap;
        if old > WMAX - old / 2 {
            return WMAX;
        }
        masked.max(old + old / 2)
    }

    pub unsafe fn assign(&mut self, s: &[u16]) {
        if s.len() <= self.cap {
            let dst = self.data_ptr() as *mut u16;
            core::ptr::copy(s.as_ptr(), dst, s.len());
            *dst.add(s.len()) = 0;
            self.size = s.len();
            return;
        }
        let ncap = self.growth(s.len());
        let np = allocate_bytes((ncap + 1) * 2) as *mut u16;
        core::ptr::copy_nonoverlapping(s.as_ptr(), np, s.len());
        *np.add(s.len()) = 0;
        if self.is_large() {
            deallocate_bytes(self.data_ptr() as *mut c_void, (self.cap + 1) * 2);
        }
        self.set_ptr(np);
        self.size = s.len();
        self.cap = ncap;
    }

    pub unsafe fn from_slice(s: &[u16]) -> Self {
        let mut w = WString::empty();
        w.assign(s);
        w
    }

    pub unsafe fn clear(&mut self) {
        *(self.data_ptr() as *mut u16) = 0;
        self.size = 0;
    }

    pub unsafe fn destroy(&mut self) {
        if self.is_large() {
            deallocate_bytes(self.data_ptr() as *mut c_void, (self.cap + 1) * 2);
        }
        self.bx = [0; 8];
        self.size = 0;
        self.cap = 7;
    }
}

#[repr(C)]
pub struct AString {
    pub bx: [u8; 16],
    pub size: usize,
    pub cap: usize,
}

impl AString {
    pub fn data_ptr(&self) -> *const u8 {
        if self.cap >= 16 {
            usize::from_ne_bytes(self.bx[..8].try_into().unwrap()) as *const u8
        } else {
            self.bx.as_ptr()
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.data_ptr(), self.size) }
    }

    pub unsafe fn from_bytes(s: &[u8]) -> Self {
        let mut a = AString { bx: [0; 16], size: s.len(), cap: 15 };
        if s.len() < 16 {
            a.bx[..s.len()].copy_from_slice(s);
        } else {
            let cap = s.len() | 15;
            let p = allocate_bytes(cap + 1) as *mut u8;
            core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
            *p.add(s.len()) = 0;
            a.bx[..8].copy_from_slice(&(p as usize).to_ne_bytes());
            a.cap = cap;
        }
        a
    }

    pub unsafe fn destroy(&mut self) {
        if self.cap >= 16 {
            deallocate_bytes(self.data_ptr() as *mut c_void, self.cap + 1);
        }
        self.bx = [0; 16];
        self.size = 0;
        self.cap = 15;
    }
}

#[repr(C)]
pub struct RefVtbl {
    pub destroy: unsafe extern "C" fn(*mut RefCountBase),
    pub delete_this: unsafe extern "C" fn(*mut RefCountBase),
    pub deleting_dtor: unsafe extern "C" fn(*mut RefCountBase, u32) -> *mut RefCountBase,
}

#[repr(C)]
pub struct RefCountBase {
    pub vtbl: *const RefVtbl,
    pub uses: AtomicI32,
    pub weaks: AtomicI32,
}

#[repr(C)]
pub struct RefCount {
    pub base: RefCountBase,
    pub ptr: *mut c_void,
}

pub unsafe fn virtual_delete(p: *mut c_void) {
    let vt = *(p as *const *const unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void);
    (*vt)(p, 1);
}

unsafe extern "C" fn rc_destroy(b: *mut RefCountBase) {
    let p = (*(b as *mut RefCount)).ptr;
    if !p.is_null() {
        virtual_delete(p);
    }
}

unsafe extern "C" fn rc_delete_this(b: *mut RefCountBase) {
    if !b.is_null() {
        ((*(*b).vtbl).deleting_dtor)(b, 1);
    }
}

unsafe extern "C" fn rc_deleting_dtor(b: *mut RefCountBase, flags: u32) -> *mut RefCountBase {
    if flags & 1 != 0 {
        operator_delete(b as *mut c_void);
    }
    b
}

pub static REF_VTBL: RefVtbl = RefVtbl { destroy: rc_destroy, delete_this: rc_delete_this, deleting_dtor: rc_deleting_dtor };

#[repr(C)]
pub struct SharedPtr {
    pub ptr: *mut c_void,
    pub rep: *mut RefCountBase,
}

impl SharedPtr {
    pub const fn null() -> Self {
        SharedPtr { ptr: core::ptr::null_mut(), rep: core::ptr::null_mut() }
    }

    pub unsafe fn adopt(ptr: *mut c_void) -> Option<Self> {
        let rc = operator_new(core::mem::size_of::<RefCount>()) as *mut RefCount;
        if rc.is_null() {
            return None;
        }
        rc.write(RefCount { base: RefCountBase { vtbl: &REF_VTBL, uses: AtomicI32::new(1), weaks: AtomicI32::new(1) }, ptr });
        Some(SharedPtr { ptr, rep: rc as *mut RefCountBase })
    }

    pub unsafe fn copy_from(src: *const SharedPtr) -> Self {
        let rep = (*src).rep;
        if !rep.is_null() {
            (*rep).uses.fetch_add(1, Ordering::AcqRel);
        }
        SharedPtr { ptr: (*src).ptr, rep }
    }

    pub unsafe fn release(&mut self) {
        let rep = self.rep;
        self.rep = core::ptr::null_mut();
        self.ptr = core::ptr::null_mut();
        if rep.is_null() {
            return;
        }
        if (*rep).uses.fetch_sub(1, Ordering::AcqRel) == 1 {
            ((*(*rep).vtbl).destroy)(rep);
            if (*rep).weaks.fetch_sub(1, Ordering::AcqRel) == 1 {
                ((*(*rep).vtbl).delete_this)(rep);
            }
        }
    }

    pub fn use_count(&self) -> i32 {
        if self.rep.is_null() {
            0
        } else {
            unsafe { (*self.rep).uses.load(Ordering::Acquire) }
        }
    }
}
