use crate::com::*;
use core::ffi::c_void;
use core::ptr::null_mut;

pub const SET_JSON_BODY_W_REQUEST: usize = 7;
pub const SET_JSON_BODY_W_REPLY: usize = 6;
pub const SERVER_SOURCE: &str = "xbox\\devex\\xtf\\dll\\pix\\server.cpp";

type GetNumber = unsafe extern "system" fn(*mut c_void, *const u16, *mut f64) -> HRESULT;
type SetNumber = unsafe extern "system" fn(*mut c_void, *const u16, f64) -> HRESULT;
type GetString = unsafe extern "system" fn(*mut c_void, *const u16, *mut BSTR) -> HRESULT;
type SetString = unsafe extern "system" fn(*mut c_void, *const u16, *const u16) -> HRESULT;
type GetObject = unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> HRESULT;
type SetObject = unsafe extern "system" fn(*mut c_void, *const u16, *mut c_void) -> HRESULT;
type Serialize = unsafe extern "system" fn(*mut c_void, *mut BSTR) -> HRESULT;
type AppendString = unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT;
type GetJsonBody = unsafe extern "system" fn(*mut c_void, *mut u32, *mut *mut u8) -> HRESULT;
type SetJsonBodyW = unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT;

pub fn cvtt64(d: f64) -> i64 {
    if d.is_nan() || !(-9.223_372_036_854_775_808e18..9.223_372_036_854_775_808e18).contains(&d) {
        i64::MIN
    } else {
        d as i64
    }
}

pub fn cvtt32(d: f64) -> i32 {
    if d.is_nan() || d <= -2_147_483_649.0 || d >= 2_147_483_648.0 {
        i32::MIN
    } else {
        d as i32
    }
}

pub unsafe fn obj_get_number(o: *mut c_void, name: *const u16, out: *mut f64) -> HRESULT {
    let f: GetNumber = core::mem::transmute(slot(o, 3));
    f(o, name, out)
}

pub unsafe fn obj_set_number(o: *mut c_void, name: *const u16, v: f64) -> HRESULT {
    let f: SetNumber = core::mem::transmute(slot(o, 4));
    f(o, name, v)
}

pub unsafe fn obj_get_string(o: *mut c_void, name: *const u16, out: *mut BSTR) -> HRESULT {
    let f: GetString = core::mem::transmute(slot(o, 5));
    f(o, name, out)
}

pub unsafe fn obj_set_string(o: *mut c_void, name: *const u16, v: *const u16) -> HRESULT {
    let f: SetString = core::mem::transmute(slot(o, 6));
    f(o, name, v)
}

pub unsafe fn obj_get_object(o: *mut c_void, name: *const u16, out: *mut *mut c_void) -> HRESULT {
    let f: GetObject = core::mem::transmute(slot(o, 8));
    f(o, name, out)
}

pub unsafe fn obj_set_object(o: *mut c_void, name: *const u16, v: *mut c_void) -> HRESULT {
    let f: SetObject = core::mem::transmute(slot(o, 9));
    f(o, name, v)
}

pub unsafe fn obj_set_array(o: *mut c_void, name: *const u16, v: *mut c_void) -> HRESULT {
    let f: SetObject = core::mem::transmute(slot(o, 11));
    f(o, name, v)
}

pub unsafe fn obj_serialize(o: *mut c_void, out: *mut BSTR) -> HRESULT {
    let f: Serialize = core::mem::transmute(slot(o, 12));
    f(o, out)
}

pub unsafe fn arr_append_string(a: *mut c_void, s: *const u16) -> HRESULT {
    let f: AppendString = core::mem::transmute(slot(a, 4));
    f(a, s)
}

pub unsafe fn msg_get_json_body(m: *mut c_void, n: *mut u32, body: *mut *mut u8) -> HRESULT {
    let f: GetJsonBody = core::mem::transmute(slot(m, 3));
    f(m, n, body)
}

pub unsafe fn msg_set_json_body_w(m: *mut c_void, index: usize, s: *const u16) -> HRESULT {
    let f: SetJsonBodyW = core::mem::transmute(slot(m, index));
    f(m, s)
}

pub unsafe fn get_dword(body: *mut c_void, name: &str, out: &mut u32) -> HRESULT {
    if body.is_null() {
        return E_INVALIDARG;
    }
    let mut d = 0.0f64;
    let hr = obj_get_number(body, w(name).as_ptr(), &mut d);
    if hr < 0 {
        return hr;
    }
    *out = cvtt64(d) as u32;
    hr
}

pub unsafe fn get_bool(body: *mut c_void, name: &str, out: &mut i32) -> HRESULT {
    if body.is_null() {
        return E_INVALIDARG;
    }
    let mut d = 0.0f64;
    let hr = obj_get_number(body, w(name).as_ptr(), &mut d);
    if hr < 0 {
        return hr;
    }
    *out = cvtt32(d);
    hr
}

pub unsafe fn get_u64(body: *mut c_void, name: &str, out: &mut u64) -> HRESULT {
    if body.is_null() {
        return E_INVALIDARG;
    }
    let mut num = ComPtr::null();
    let hr = obj_get_object(body, w(name).as_ptr(), num.out());
    if hr < 0 {
        return hr;
    }
    let mut d = 0.0f64;
    let hr = obj_get_number(num.0, w("L").as_ptr(), &mut d);
    if hr < 0 {
        return hr;
    }
    let lo = cvtt64(d) as u32;
    let hr = obj_get_number(num.0, w("H").as_ptr(), &mut d);
    if hr < 0 {
        return hr;
    }
    let hi = cvtt64(d) as u32;
    *out = ((hi as u64) << 32) | lo as u64;
    hr
}

pub unsafe fn set_u64(body: *mut c_void, name: &str, v: u64) -> HRESULT {
    if body.is_null() {
        return E_INVALIDARG;
    }
    let mut num = ComPtr::null();
    let hr = XbtpCreateJsonObject(num.out());
    if hr < 0 {
        return hr;
    }
    let hr = obj_set_number(num.0, w("L").as_ptr(), v as u32 as f64);
    if hr < 0 {
        return hr;
    }
    let hr = obj_set_number(num.0, w("H").as_ptr(), (v >> 32) as u32 as i32 as f64);
    if hr < 0 {
        return hr;
    }
    obj_set_object(body, w(name).as_ptr(), num.0)
}

pub unsafe fn set_dword(body: *mut c_void, name: &str, v: u32) -> HRESULT {
    if body.is_null() {
        return E_INVALIDARG;
    }
    obj_set_number(body, w(name).as_ptr(), v as f64)
}

pub unsafe fn set_int(body: *mut c_void, name: &str, v: i32) -> HRESULT {
    if body.is_null() {
        return E_INVALIDARG;
    }
    obj_set_number(body, w(name).as_ptr(), v as f64)
}

pub unsafe fn set_string(body: *mut c_void, name: &str, v: *const u16) -> HRESULT {
    obj_set_string(body, w(name).as_ptr(), v)
}

pub unsafe fn deserialize(message: *mut c_void, body: &mut ComPtr) -> HRESULT {
    if message.is_null() {
        return E_INVALIDARG;
    }
    let mut n = 0u32;
    let mut p: *mut u8 = null_mut();
    let hr = msg_get_json_body(message, &mut n, &mut p);
    if hr < 0 {
        return hr;
    }
    let hr = if p.is_null() { XbtpCreateJsonObject(body.out()) } else { XbtpGetJsonObjectFromUtf8(p, body.out()) };
    if hr < 0 {
        return hr;
    }
    S_OK
}

pub unsafe fn serialize(body: *mut c_void, message: *mut c_void, index: usize) -> HRESULT {
    if body.is_null() || message.is_null() {
        return E_INVALIDARG;
    }
    let mut s = Bstr::null();
    let hr = obj_serialize(body, &mut s.0);
    if hr < 0 {
        return hr;
    }
    let hr = msg_set_json_body_w(message, index, s.0);
    if hr < 0 {
        return hr;
    }
    S_OK
}

pub struct ErrorContext {
    errors: ComPtr,
    reply: *mut c_void,
    checkpoint: u32,
    hr: HRESULT,
    flushed: bool,
}

impl ErrorContext {
    pub fn new(reply: *mut c_void) -> Self {
        ErrorContext { errors: ComPtr::null(), reply, checkpoint: 0, hr: S_OK, flushed: false }
    }

    pub fn checkpoint(&mut self) {
        self.checkpoint += 1;
    }

    unsafe fn errors_array(&mut self) -> HRESULT {
        if self.errors.0.is_null() {
            return XbtpCreateJsonArray(self.errors.out());
        }
        S_OK
    }

    pub fn fail(&mut self, hr: HRESULT, line: u32) -> HRESULT {
        let text = w(&format!("Error 0x{:X} File {} : {}", hr as u32, SERVER_SOURCE, line));
        unsafe {
            if self.errors_array() >= 0 {
                arr_append_string(self.errors.0, text.as_ptr());
            }
        }
        self.hr = hr;
        hr
    }

    unsafe fn flush(&mut self, status: &mut HRESULT) {
        if self.reply.is_null() {
            return;
        }
        if self.hr >= 0 && *status >= 0 {
            return;
        }
        let mut body = ComPtr::null();
        if deserialize(self.reply, &mut body) < 0 {
            return;
        }
        let mut err = ComPtr::null();
        if XbtpCreateJsonObject(err.out()) < 0 {
            return;
        }
        let code = if self.hr < 0 { self.hr } else { *status };
        if obj_set_number(err.0, w("ReturnCode").as_ptr(), code as f64) < 0 {
            return;
        }
        if obj_set_number(err.0, w("Checkpoint").as_ptr(), self.checkpoint as f64) < 0 {
            return;
        }
        if self.errors_array() < 0 {
            return;
        }
        if obj_set_array(err.0, w("Errors").as_ptr(), self.errors.0) < 0 {
            return;
        }
        if obj_set_object(body.0, w("XtfOperationError").as_ptr(), err.0) < 0 {
            return;
        }
        if serialize(body.0, self.reply, SET_JSON_BODY_W_REPLY) < 0 {
            return;
        }
        *status = S_OK;
        self.flushed = true;
    }
}

impl Drop for ErrorContext {
    fn drop(&mut self) {
        if !self.flushed {
            let mut status = self.hr;
            unsafe { self.flush(&mut status) };
        }
    }
}
