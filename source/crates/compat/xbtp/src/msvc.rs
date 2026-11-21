use crate::sys::{free, malloc};
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};

#[repr(C)]
pub struct RefVtbl {
    pub destroy: unsafe extern "C" fn(*mut RefCountBase),
    pub delete_this: unsafe extern "C" fn(*mut RefCountBase),
    pub deleting_dtor: unsafe extern "C" fn(*mut RefCountBase, u32) -> *mut c_void,
    pub get_deleter: unsafe extern "C" fn(*mut RefCountBase, *const c_void) -> *mut c_void,
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
    pub drop_obj: unsafe fn(*mut c_void),
}

#[repr(C)]
pub struct SharedPtr<T> {
    pub ptr: *mut T,
    pub rep: *mut RefCountBase,
}

impl<T> Default for SharedPtr<T> {
    fn default() -> Self {
        SharedPtr { ptr: core::ptr::null_mut(), rep: core::ptr::null_mut() }
    }
}

unsafe extern "C" fn rc_destroy(b: *mut RefCountBase) {
    let rc = b as *mut RefCount;
    let p = (*rc).ptr;
    if !p.is_null() {
        ((*rc).drop_obj)(p);
    }
}

unsafe extern "C" fn rc_delete_this(b: *mut RefCountBase) {
    if !b.is_null() {
        ((*(*b).vtbl).deleting_dtor)(b, 1);
    }
}

unsafe extern "C" fn rc_deleting_dtor(b: *mut RefCountBase, flags: u32) -> *mut c_void {
    if flags & 1 != 0 {
        free(b as *mut c_void);
    }
    b as *mut c_void
}

unsafe extern "C" fn rc_get_deleter(_b: *mut RefCountBase, _t: *const c_void) -> *mut c_void {
    core::ptr::null_mut()
}

pub static REF_VTBL: RefVtbl = RefVtbl {
    destroy: rc_destroy,
    delete_this: rc_delete_this,
    deleting_dtor: rc_deleting_dtor,
    get_deleter: rc_get_deleter,
};

pub unsafe fn virtual_delete(p: *mut c_void) {
    let vt = *(p as *const *const unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void);
    (*vt)(p, 1);
}

impl<T> SharedPtr<T> {
    pub unsafe fn new_with(ptr: *mut T, drop_obj: unsafe fn(*mut c_void)) -> Option<Self> {
        let rc = malloc(core::mem::size_of::<RefCount>()) as *mut RefCount;
        if rc.is_null() {
            return None;
        }
        rc.write(RefCount {
            base: RefCountBase { vtbl: &REF_VTBL, uses: AtomicI32::new(1), weaks: AtomicI32::new(1) },
            ptr: ptr as *mut c_void,
            drop_obj,
        });
        Some(SharedPtr { ptr, rep: rc as *mut RefCountBase })
    }

    pub unsafe fn new_virtual(ptr: *mut T) -> Option<Self> {
        Self::new_with(ptr, virtual_delete)
    }

    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    pub unsafe fn clone_ref(&self) -> Self {
        if !self.rep.is_null() {
            (*self.rep).uses.fetch_add(1, Ordering::AcqRel);
        }
        SharedPtr { ptr: self.ptr, rep: self.rep }
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

    pub unsafe fn take(src: *mut SharedPtr<T>) -> Self {
        let v = SharedPtr { ptr: (*src).ptr, rep: (*src).rep };
        (*src).ptr = core::ptr::null_mut();
        (*src).rep = core::ptr::null_mut();
        v
    }

    pub unsafe fn assign_into(self, dst: *mut SharedPtr<T>) {
        let mut old = SharedPtr { ptr: (*dst).ptr, rep: (*dst).rep };
        (*dst).ptr = self.ptr;
        (*dst).rep = self.rep;
        core::mem::forget(self);
        old.release();
    }

    pub fn use_count(&self) -> i32 {
        if self.rep.is_null() {
            0
        } else {
            unsafe { (*self.rep).uses.load(Ordering::Acquire) }
        }
    }

    pub fn cast<U>(self) -> SharedPtr<U> {
        let v = SharedPtr { ptr: self.ptr as *mut U, rep: self.rep };
        core::mem::forget(self);
        v
    }
}

impl<T> Drop for SharedPtr<T> {
    fn drop(&mut self) {
        unsafe { self.release() }
    }
}

unsafe impl<T> Send for SharedPtr<T> {}
unsafe impl<T> Sync for SharedPtr<T> {}

pub unsafe fn cpp_new(size: usize) -> *mut c_void {
    let p = malloc(size.max(1));
    if !p.is_null() {
        core::ptr::write_bytes(p as *mut u8, 0, size);
    }
    p
}

pub unsafe fn cpp_delete(p: *mut c_void) {
    if !p.is_null() {
        free(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::AtomicUsize;

    static DROPS: AtomicUsize = AtomicUsize::new(0);

    unsafe fn count_drop(p: *mut c_void) {
        DROPS.fetch_add(1, Ordering::SeqCst);
        cpp_delete(p);
    }

    #[test]
    fn shared_ptr_counts_and_destroys_once() {
        unsafe {
            let obj = cpp_new(8);
            let a = SharedPtr::<u8>::new_with(obj as *mut u8, count_drop).unwrap();
            let b = a.clone_ref();
            assert_eq!(a.use_count(), 2);
            drop(a);
            assert_eq!(b.use_count(), 1);
            let before = DROPS.load(Ordering::SeqCst);
            drop(b);
            assert_eq!(DROPS.load(Ordering::SeqCst), before + 1);
        }
    }

    static DROPS2: AtomicUsize = AtomicUsize::new(0);

    unsafe fn count_drop2(p: *mut c_void) {
        DROPS2.fetch_add(1, Ordering::SeqCst);
        cpp_delete(p);
    }

    #[test]
    fn assign_releases_previous_value() {
        unsafe {
            let mut slot = SharedPtr::<u8>::new_with(cpp_new(4) as *mut u8, count_drop2).unwrap();
            let next = SharedPtr::<u8>::new_with(cpp_new(4) as *mut u8, count_drop2).unwrap();
            next.assign_into(&mut slot);
            assert_eq!(DROPS2.load(Ordering::SeqCst), 1);
            drop(slot);
            assert_eq!(DROPS2.load(Ordering::SeqCst), 2);
        }
    }

    #[test]
    fn layout_matches_msvc() {
        assert_eq!(core::mem::size_of::<SharedPtr<u8>>(), 16);
        assert_eq!(core::mem::offset_of!(RefCountBase, uses), 8);
        assert_eq!(core::mem::offset_of!(RefCountBase, weaks), 12);
        assert_eq!(core::mem::offset_of!(RefCount, ptr), 16);
    }
}
