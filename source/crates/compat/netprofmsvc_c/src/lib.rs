use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

pub static SVCHOST_GLOBALS: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

#[no_mangle]
pub extern "system" fn SvchostPushServiceGlobalsEx(globals: *mut c_void) {
    SVCHOST_GLOBALS.store(globals, Ordering::Release);
}

pub fn svchost_globals() -> *mut c_void {
    SVCHOST_GLOBALS.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_stores_and_replaces_the_globals_pointer() {
        let mut a = [0u8; 0xe0];
        let mut b = [0u8; 0xe0];
        SvchostPushServiceGlobalsEx(a.as_mut_ptr() as *mut c_void);
        assert_eq!(svchost_globals(), a.as_mut_ptr() as *mut c_void);
        SvchostPushServiceGlobalsEx(b.as_mut_ptr() as *mut c_void);
        assert_eq!(svchost_globals(), b.as_mut_ptr() as *mut c_void);
        SvchostPushServiceGlobalsEx(core::ptr::null_mut());
        assert!(svchost_globals().is_null());
        assert_eq!(a[0], 0);
    }
}
