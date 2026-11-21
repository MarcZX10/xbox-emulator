use crate::sys::*;
use core::ffi::c_void;

pub type Obj = *mut c_void;

pub unsafe fn slot(o: Obj, i: usize) -> usize {
    *(*(o as *const *const usize)).add(i)
}

pub unsafe fn add_ref(o: Obj) -> u32 {
    let f: unsafe extern "system" fn(Obj) -> u32 = core::mem::transmute(slot(o, 1));
    f(o)
}

pub unsafe fn release(o: Obj) -> u32 {
    let f: unsafe extern "system" fn(Obj) -> u32 = core::mem::transmute(slot(o, 2));
    f(o)
}

pub unsafe fn query_interface(o: Obj, iid: *const GUID, out: *mut Obj) -> HRESULT {
    let f: unsafe extern "system" fn(Obj, *const GUID, *mut Obj) -> HRESULT = core::mem::transmute(slot(o, 0));
    f(o, iid, out)
}

pub struct Com(pub Obj);

impl Com {
    pub fn null() -> Com {
        Com(core::ptr::null_mut())
    }
    pub fn put(&mut self) -> *mut Obj {
        self.reset();
        &mut self.0
    }
    pub fn reset(&mut self) {
        let p = core::mem::replace(&mut self.0, core::ptr::null_mut());
        if !p.is_null() {
            unsafe { release(p) };
        }
    }
    pub fn get(&self) -> Obj {
        self.0
    }
    pub fn detach(&mut self) -> Obj {
        core::mem::replace(&mut self.0, core::ptr::null_mut())
    }
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        self.reset();
    }
}

macro_rules! vcall {
    ($name:ident, $i:expr, ($($a:ident: $t:ty),*)) => {
        pub unsafe fn $name(o: Obj, $($a: $t),*) -> HRESULT {
            let f: unsafe extern "system" fn(Obj, $($t),*) -> HRESULT = core::mem::transmute(slot(o, $i));
            f(o, $($a),*)
        }
    };
}

pub mod json {
    use super::*;
    vcall!(get_number, 3, (name: *const u16, out: *mut f64));
    vcall!(set_number, 4, (name: *const u16, v: f64));
    vcall!(get_string, 5, (name: *const u16, out: *mut BSTR));
    vcall!(set_string, 6, (name: *const u16, v: *const u16));
    vcall!(get_object, 8, (name: *const u16, out: *mut Obj));
    vcall!(set_object, 9, (name: *const u16, v: Obj));
    vcall!(get_array, 10, (name: *const u16, out: *mut Obj));
    vcall!(set_array, 11, (name: *const u16, v: Obj));
    vcall!(stringify, 12, (out: *mut BSTR));
}

pub mod array {
    use super::*;
    vcall!(append_number, 3, (v: f64));
    vcall!(append_string, 4, (v: *const u16));
    vcall!(append_object, 5, (v: Obj));
    vcall!(get_number, 7, (i: u32, out: *mut f64));
    vcall!(get_string, 8, (i: u32, out: *mut BSTR));
    vcall!(get_object, 9, (i: u32, out: *mut Obj));
    vcall!(get_count, 11, (out: *mut u32));
}

pub mod message {
    use super::*;
    vcall!(get_json_body, 3, (len: *mut u32, body: *mut *const u8));
    vcall!(get_binary_body, 4, (len: *mut u32, body: *mut *const u8));
    vcall!(set_json_body, 5, (len: u32, body: *const u8));
    vcall!(set_json_body_copy, 6, (len: u32, body: *const u8));
    vcall!(set_json_body_w, 7, (body: *const u16));
    vcall!(set_binary_body, 8, (len: u32, body: *const u8));
    vcall!(set_binary_body_copy, 9, (len: u32, body: *const u8));
}

pub mod end_message {
    use super::*;
    vcall!(get_json_body, 3, (len: *mut u32, body: *mut *const u8));
    vcall!(get_binary_body, 4, (len: *mut u32, body: *mut *const u8));
    vcall!(set_json_body, 5, (len: u32, body: *const u8));
    vcall!(set_json_body_w, 6, (body: *const u16));
    vcall!(set_binary_body, 7, (len: u32, body: *const u8));
}

pub mod session {
    use super::*;
    vcall!(send_more_message, 3, (msg: Obj));
    vcall!(receive_more_message, 5, (flags: u32, out: *mut Obj));
}

pub mod client_session {
    use super::*;
    vcall!(send_begin_message, 3, (op: u32, msg: Obj, exchange: *mut u32));
    vcall!(receive_end_message, 7, (timeout: u32, exchange: u32, status: *mut u32, out: *mut Obj));
}

pub const IID_XBTP_CLIENT_SESSION: GUID = GUID::new(0xcf84f553, 0xcd9b, 0x4608, [0x96, 0x40, 0x28, 0x71, 0x4e, 0x5e, 0xd4, 0x40]);

pub mod metadata {
    use super::*;
    vcall!(get_handler_count, 3, (out: *mut u32));
    pub type OperationFn = unsafe extern "system" fn(usize, usize, usize) -> HRESULT;
    vcall!(add_function, 6, (id: u32, f: Option<OperationFn>));
}

pub const IID_IUNKNOWN: GUID = GUID::new(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
pub const IID_XBTP_EXTENSION_METADATA: GUID = GUID::new(0xe2b6c063, 0x7d82, 0x414a, [0xbe, 0x86, 0x67, 0xed, 0x1b, 0x44, 0x77, 0x97]);
pub const IID_XBTP_MESSAGE: GUID = GUID::new(0x6965e04a, 0x2829, 0x41a8, [0xb1, 0x94, 0x48, 0x73, 0x15, 0x07, 0x5e, 0x7a]);
