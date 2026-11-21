use crate::com::*;
use crate::imports;
use crate::messaging::*;
use crate::sys::*;
use core::ffi::c_void;
use core::ptr::null;
use xtfcommon::msvc::WString;

pub const LOGGING_FILE: &str = "xbox\\devex\\xtf\\dll\\diaginfo\\loggingserver.cpp";
pub const PERFAPI_FILE: &str = "xbox\\devex\\xtf\\dll\\diaginfo\\perfapihandler.cpp";
pub const PROFILING_PATH: &str = "S:\\Profiling";
pub const PROFILING_DEFAULT: &str = "D:";

unsafe fn reply_with(reply: *mut c_void, fill: impl FnOnce(Obj) -> HRESULT) -> HRESULT {
    let i = match imports::get() {
        Ok(i) => i,
        Err(e) => return e,
    };
    let mut msg = Ptr::null();
    let hr = (i.create_json_object)(msg.out());
    if hr < 0 {
        return hr;
    }
    let hr = fill(msg.0);
    if hr < 0 {
        return hr;
    }
    stringify_into_reply(msg.0, reply)
}

pub unsafe extern "system" fn system_up_time_handler(_session: usize, _request: usize, reply: usize) -> HRESULT {
    let up = GetTickCount64();
    reply_with(reply as *mut c_void, |m| {
        let hr = obj_set_number(m, wz("SystemUptimeHDW").as_ptr(), (up >> 32) as i64 as f64);
        if hr < 0 {
            return hr;
        }
        obj_set_number(m, wz("SystemUptimeLDW").as_ptr(), (up as u32) as i64 as f64)
    })
}

pub unsafe extern "system" fn set_system_go_dark_handler(_session: usize, _request: usize, reply: usize) -> HRESULT {
    reply_with(reply as *mut c_void, |_| {
        if let Ok(i) = imports::get() {
            (i.set_system_go_dark)(1);
        }
        S_OK
    })
}

pub unsafe extern "system" fn fetch_system_go_dark_handler(_session: usize, _request: usize, reply: usize) -> HRESULT {
    reply_with(reply as *mut c_void, |m| {
        let dark = match imports::get() {
            Ok(i) => ((i.has_system_gone_dark)() & 0xFF) as u8,
            Err(e) => return e,
        };
        obj_set_number(m, wz("SysIsDark").as_ptr(), dark as i32 as f64)
    })
}

pub unsafe extern "system" fn get_logging_properties_handler(_session: usize, _request: usize, reply: usize) -> HRESULT {
    let i = match imports::get() {
        Ok(i) => i,
        Err(e) => return e,
    };
    let mut level = 6u32;
    let mut path = WString::empty();
    let mut stamps = 0u8;
    let mut msg = Ptr::null();
    let mut hr = (i.create_json_object)(msg.out());
    'done: {
        if hr < 0 {
            break 'done;
        }
        hr = (i.get_logging_properties)((i.logging_instance)(), &mut level, &mut path, &mut stamps);
        if hr < 0 {
            break 'done;
        }
        hr = obj_set_number(msg.0, wz("LogLevel").as_ptr(), level as f64);
        if hr < 0 {
            break 'done;
        }
        hr = obj_set_string(msg.0, wz("LogFile").as_ptr(), path.c_str());
        if hr < 0 {
            break 'done;
        }
        let ts: f32 = if stamps != 0 { 1.0 } else { 0.0 };
        hr = obj_set_number(msg.0, wz("LogTimestamps").as_ptr(), ts as f64);
        if hr < 0 {
            break 'done;
        }
        hr = stringify_into_reply(msg.0, reply as *mut c_void);
    }
    drop(msg);
    path.destroy();
    hr
}

pub unsafe extern "system" fn set_logging_properties_handler(_session: usize, request: usize, reply: usize) -> HRESULT {
    let level: u32;
    let stamps: u8;
    let mut level_p: *const u32 = null();
    let mut file_p: *const u16 = null();
    let mut stamps_p: *const u8 = null();
    let mut file = Bstr::null();
    let mut body = Ptr::null();
    let mut ctx = ErrorContext::new(reply as *mut c_void);
    let mut hr;
    'fail: {
        hr = deserialize_body(request as *mut c_void, body.out());
        if hr < 0 {
            ctx.add_error(hr, LOGGING_FILE, 0x27);
            break 'fail;
        }
        let mut d = 0f64;
        hr = obj_get_number(body.0, wz("LogLevel").as_ptr(), &mut d);
        if hr == S_OK {
            level = cvtt_i64(d) as u32;
            level_p = &level;
        } else if hr != E_JSON_NOT_FOUND && hr < 0 {
            ctx.add_error(hr, LOGGING_FILE, 0x2e);
            break 'fail;
        }
        ctx.checkpoint += 1;
        hr = obj_get_string(body.0, wz("LogFile").as_ptr(), file.out());
        if hr == S_OK {
            file_p = file.0;
        } else if hr != E_JSON_NOT_FOUND && hr < 0 {
            ctx.add_error(hr, LOGGING_FILE, 0x3c);
            break 'fail;
        }
        ctx.checkpoint += 1;
        hr = obj_get_number(body.0, wz("LogTimestamps").as_ptr(), &mut d);
        if hr == S_OK {
            stamps = (d > 0.009_999_999_776_482_582) as u8;
            stamps_p = &stamps;
        } else if hr != E_JSON_NOT_FOUND && hr < 0 {
            ctx.add_error(hr, LOGGING_FILE, 0x4a);
            break 'fail;
        }
        ctx.checkpoint += 1;
        hr = match imports::get() {
            Ok(i) => (i.set_logging_properties)((i.logging_instance)(), level_p, file_p, stamps_p),
            Err(e) => e,
        };
        drop(ctx);
        drop(body);
        drop(file);
        return hr;
    }
    ctx.hr = hr;
    drop(ctx);
    drop(body);
    drop(file);
    hr
}

pub unsafe fn profiling_directory() -> Vec<u16> {
    let p = wz(PROFILING_PATH);
    let a = GetFileAttributesW(p.as_ptr());
    if a != u32::MAX && a & 0x10 != 0 && a & 0x400 != 0 {
        let h = CreateFileW(p.as_ptr(), 0x8000_0000, 1, core::ptr::null_mut(), 3, 0x0200_0000, core::ptr::null_mut());
        if h != INVALID_HANDLE_VALUE {
            CloseHandle(h);
            return p;
        }
    }
    wz(PROFILING_DEFAULT)
}

pub unsafe extern "system" fn query_profiling_directory_handler(_session: usize, _request: usize, reply: usize) -> HRESULT {
    let mut ret = S_OK;
    let mut body = Ptr::null();
    let mut ctx = ErrorContext::new(reply as *mut c_void);
    let created = match imports::get() {
        Ok(i) => (i.create_json_object)(body.out()),
        Err(e) => e,
    };
    if created < 0 {
        ctx.add_error(created, PERFAPI_FILE, 0x13);
        ret = created;
        ctx.hr = created;
    } else {
        ctx.set_body(body.0);
        let path = profiling_directory();
        let hr = obj_set_string(body.0, wz("PD").as_ptr(), path.as_ptr());
        if hr < 0 {
            ctx.add_error(hr, PERFAPI_FILE, 0x36);
            ret = hr;
            ctx.hr = hr;
        }
    }
    ctx.complete(&mut ret);
    drop(body);
    drop(ctx);
    ret
}
