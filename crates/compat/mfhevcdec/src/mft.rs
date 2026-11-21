use core::ffi::c_void;

pub unsafe fn create_instance(_riid: *const crate::Guid, _ppv: *mut *mut c_void) -> i32 {
    crate::E_OUTOFMEMORY
}
