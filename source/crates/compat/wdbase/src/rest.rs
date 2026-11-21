use crate::stl::{AString, WString};
use crate::w;
use core::ffi::c_void;

#[repr(C)]
pub struct RestContext {
    pub host: WString,
    pub csrf_token: WString,
    pub port: u16,
    pub session: *mut c_void,
    pub connection: *mut c_void,
    pub initialized: u8,
    pub ssl_enabled: u8,
}

const _: () = assert!(core::mem::offset_of!(RestContext, port) == 0x40);
const _: () = assert!(core::mem::offset_of!(RestContext, session) == 0x48);
const _: () = assert!(core::mem::offset_of!(RestContext, initialized) == 0x58);

pub const DEFAULT_PORT: u16 = 11443;
pub const ERROR_ALREADY_INITIALIZED: u32 = 0x4df;
pub const ERROR_NOT_INITIALIZED: u32 = 0xfd6;
pub const ERROR_INVALID_PARAMETER: u32 = 0x57;
pub const ERROR_ACCESS_DENIED: u32 = 5;
pub const ERROR_NO_UNICODE_TRANSLATION: u32 = 0x1132;
pub const ERROR_OUTOFMEMORY: u32 = 0xe;
const ERROR_INSUFFICIENT_BUFFER: u32 = 0x7a;
const ERROR_WINHTTP_SECURE_FAILURE: u32 = 12175;
const WINHTTP_QUERY_STATUS_CODE_NUMBER: u32 = 0x2000_0013;
const WINHTTP_QUERY_RAW_HEADERS_CRLF: u32 = 0x16;
const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
const WINHTTP_OPTION_SECURITY_FLAGS: u32 = 31;
const SECURITY_FLAG_IGNORE_ALL: u32 = 0x3300;

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> *mut c_void;
    fn WinHttpSetTimeouts(h: *mut c_void, resolve: i32, connect: i32, send: i32, receive: i32) -> i32;
    fn WinHttpConnect(h: *mut c_void, server: *const u16, port: u16, reserved: u32) -> *mut c_void;
    fn WinHttpCloseHandle(h: *mut c_void) -> i32;
    fn WinHttpOpenRequest(h: *mut c_void, verb: *const u16, object: *const u16, version: *const u16, referrer: *const u16, accept: *const *const u16, flags: u32) -> *mut c_void;
    fn WinHttpSetCredentials(h: *mut c_void, targets: u32, scheme: u32, user: *const u16, password: *const u16, params: *mut c_void) -> i32;
    fn WinHttpSendRequest(h: *mut c_void, headers: *const u16, headers_len: u32, optional: *mut c_void, optional_len: u32, total: u32, context: usize) -> i32;
    fn WinHttpReceiveResponse(h: *mut c_void, reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(h: *mut c_void, level: u32, name: *const u16, buffer: *mut c_void, len: *mut u32, index: *mut u32) -> i32;
    fn WinHttpReadData(h: *mut c_void, buffer: *mut c_void, len: u32, read: *mut u32) -> i32;
    fn WinHttpSetOption(h: *mut c_void, option: u32, buffer: *mut c_void, len: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> u32;
}

#[export_name = "??0RestContext@@AEAA@XZ"]
pub unsafe extern "C" fn rest_context_default_ctor(this: *mut RestContext) -> *mut RestContext {
    (*this).ssl_enabled = 1;
    WString::init(&mut (*this).host);
    WString::init(&mut (*this).csrf_token);
    this
}

#[export_name = "??0RestContext@@QEAA@PEBGG@Z"]
pub unsafe extern "C" fn rest_context_ctor(this: *mut RestContext, _host: *const u16, port: u16) -> *mut RestContext {
    WString::init(&mut (*this).host);
    WString::init(&mut (*this).csrf_token);
    (*this).initialized = 0;
    (*this).ssl_enabled = 1;
    (*this).session = core::ptr::null_mut();
    (*this).connection = core::ptr::null_mut();
    (*this).port = if port == 0 { DEFAULT_PORT } else { port };
    WString::assign_cstr(&mut (*this).host, w!("localhost").as_ptr());
    this
}

#[export_name = "??1RestContext@@QEAA@XZ"]
pub unsafe extern "C" fn rest_context_dtor(this: *mut RestContext) {
    close_handles(this);
    WString::tidy(&mut (*this).csrf_token);
    WString::tidy(&mut (*this).host);
}

#[export_name = "?CloseHandles@RestContext@@AEAAXXZ"]
pub unsafe extern "C" fn close_handles(this: *mut RestContext) {
    if !(*this).connection.is_null() {
        WinHttpCloseHandle((*this).connection);
        (*this).connection = core::ptr::null_mut();
    }
    if !(*this).session.is_null() {
        WinHttpCloseHandle((*this).session);
        (*this).session = core::ptr::null_mut();
    }
}

#[export_name = "?Initialize@RestContext@@QEAAKXZ"]
pub unsafe extern "C" fn initialize(this: *mut RestContext) -> u32 {
    if (*this).initialized != 0 {
        return ERROR_ALREADY_INITIALIZED;
    }
    let mut err = 0u32;
    (*this).session = WinHttpOpen(w!("WinHTTP WDTools/1.0").as_ptr(), 4, core::ptr::null(), core::ptr::null(), 0);
    if (*this).session.is_null() {
        err = GetLastError();
    }
    if err == 0 {
        WinHttpSetTimeouts((*this).session, 750, 750, 750, 750);
        (*this).connection = WinHttpConnect((*this).session, WString::data(&(*this).host), (*this).port, 0);
        if (*this).connection.is_null() {
            err = GetLastError();
        }
    }
    if err != 0 {
        close_handles(this);
        (*this).initialized = 0;
    } else {
        (*this).initialized = 1;
    }
    err
}

#[export_name = "?CacheCsrfToken@RestContext@@AEAAKPEBG@Z"]
pub unsafe extern "C" fn cache_csrf_token(this: *mut RestContext, headers: *const u16) -> u32 {
    if headers.is_null() || *headers == 0 {
        return ERROR_INVALID_PARAMETER;
    }
    let cookie = crate::strings::sub_string_insensitive(headers, w!("Set-Cookie:").as_ptr());
    if cookie.is_null() {
        return 0;
    }
    let token = crate::strings::sub_string_insensitive(cookie, w!("CSRF-Token=").as_ptr());
    if token.is_null() {
        return 0;
    }
    let start = token.add(11);
    let mut end = start;
    while !(*end <= 0xd && (0x2401u32 >> *end) & 1 != 0) {
        end = end.add(1);
    }
    let mut tmp = WString::new();
    WString::assign_cstr(&mut tmp, start);
    let n = ((end as usize - start as usize) >> 1).min(tmp.size);
    WString::assign(&mut (*this).csrf_token, WString::data(&tmp), n);
    WString::tidy(&mut tmp);
    0
}

#[export_name = "?QueryCsrfHeader@RestContext@@AEAAKPEAXAEAK@Z"]
pub unsafe extern "C" fn query_csrf_header(this: *mut RestContext, request: *mut c_void, status: *mut u32) -> u32 {
    let mut code = 0u32;
    let mut len = 4u32;
    *status = 0;
    if WinHttpQueryHeaders(request, WINHTTP_QUERY_STATUS_CODE_NUMBER, core::ptr::null(), &mut code as *mut u32 as *mut c_void, &mut len, core::ptr::null_mut()) == 0 {
        let e = GetLastError();
        if e != 0 {
            return e;
        }
    }
    *status = code;
    if code >= 300 {
        return ERROR_ACCESS_DENIED;
    }
    let mut size = 0u32;
    WinHttpQueryHeaders(request, WINHTTP_QUERY_RAW_HEADERS_CRLF, core::ptr::null(), core::ptr::null_mut(), &mut size, core::ptr::null_mut());
    let mut e = GetLastError();
    let mut buffer: *mut u16 = core::ptr::null_mut();
    if e == ERROR_INSUFFICIENT_BUFFER {
        let bytes = ((size >> 1) as usize).checked_mul(2).unwrap_or(usize::MAX);
        buffer = crate::stl::try_operator_new(bytes) as *mut u16;
        if buffer.is_null() {
            return ERROR_OUTOFMEMORY;
        }
        if WinHttpQueryHeaders(request, WINHTTP_QUERY_RAW_HEADERS_CRLF, core::ptr::null(), buffer as *mut c_void, &mut size, core::ptr::null_mut()) == 0 {
            e = GetLastError();
            if e != 0 {
                crate::stl::operator_delete(buffer as *mut u8);
                return e;
            }
        }
    } else if e != 0 {
        return e;
    }
    e = cache_csrf_token(this, buffer);
    if !buffer.is_null() {
        crate::stl::operator_delete(buffer as *mut u8);
    }
    e
}

unsafe fn destroy_args(user: *mut WString, password: *mut WString) {
    WString::tidy(user);
    WString::tidy(password);
}

#[export_name = "?Get@RestContext@@AEAAKV?$basic_string@GU?$char_traits@G@std@@V?$allocator@G@2@@std@@0PEBGAEAV23@@Z"]
pub unsafe extern "C" fn get(this: *mut RestContext, user: *mut WString, password: *mut WString, path: *const u16, out: *mut WString) -> u32 {
    if (*this).initialized == 0 {
        destroy_args(user, password);
        return ERROR_NOT_INITIALIZED;
    }
    WString::clear(out);
    let request = WinHttpOpenRequest((*this).connection, w!("GET").as_ptr(), path, core::ptr::null(), core::ptr::null(), core::ptr::null(), WINHTTP_FLAG_SECURE);
    let mut err;
    if request.is_null() {
        err = GetLastError();
        if err != 0 {
            destroy_args(user, password);
            return err;
        }
    }
    let mut first = true;
    if (*user).size != 0 {
        if WinHttpSetCredentials(request, 0, 1, WString::data(user), WString::data(password), core::ptr::null_mut()) == 0 {
            err = GetLastError();
            if err != 0 {
                return finish(request, user, password, err);
            }
        }
    }
    loop {
        if WinHttpSendRequest(request, core::ptr::null(), 0, core::ptr::null_mut(), 0, 0, 0) != 0 {
            break;
        }
        err = GetLastError();
        if !first && err != 0 {
            return finish(request, user, password, err);
        }
        if err == ERROR_WINHTTP_SECURE_FAILURE {
            let mut flags = SECURITY_FLAG_IGNORE_ALL;
            first = false;
            if WinHttpSetOption(request, WINHTTP_OPTION_SECURITY_FLAGS, &mut flags as *mut u32 as *mut c_void, 4) == 0 {
                err = GetLastError();
                if err != 0 {
                    return finish(request, user, password, err);
                }
            }
        } else if err != 0 {
            return finish(request, user, password, err);
        }
    }
    if WinHttpReceiveResponse(request, core::ptr::null_mut()) == 0 {
        err = GetLastError();
        return finish(request, user, password, err);
    }
    let mut status = 0u32;
    err = query_csrf_header(this, request, &mut status);
    if status >= 300 {
        destroy_args(user, password);
        return status;
    }
    if err != 0 {
        return finish(request, user, password, err);
    }
    let mut body = AString::new();
    let mut chunk = [0u8; 0x801];
    loop {
        chunk.fill(0);
        let mut read = 0u32;
        if WinHttpReadData(request, chunk.as_mut_ptr() as *mut c_void, 0x800, &mut read) == 0 {
            let e = GetLastError();
            if e != 0 {
                AString::tidy(&mut body);
                return finish(request, user, password, e);
            }
        }
        let n = chunk.iter().position(|&b| b == 0).unwrap_or(0x800);
        AString::append(&mut body, chunk.as_ptr(), n);
        if read == 0 {
            break;
        }
    }
    let hr = crate::strings::convert_utf8_to_wide(AString::data(&body) as *const u8, body.size as u32, out);
    AString::tidy(&mut body);
    if hr < 0 {
        destroy_args(user, password);
        return ERROR_NO_UNICODE_TRANSLATION;
    }
    finish(request, user, password, err)
}

unsafe fn finish(request: *mut c_void, user: *mut WString, password: *mut WString, err: u32) -> u32 {
    if !request.is_null() {
        WinHttpCloseHandle(request);
    }
    destroy_args(user, password);
    err
}

unsafe fn read_body(request: *mut c_void, body: *mut AString) -> u32 {
    let mut chunk = [0u8; 0x801];
    loop {
        chunk.fill(0);
        let mut read = 0u32;
        if WinHttpReadData(request, chunk.as_mut_ptr() as *mut c_void, 0x800, &mut read) == 0 {
            let e = GetLastError();
            if e != 0 {
                return e;
            }
        }
        let n = chunk.iter().position(|&b| b == 0).unwrap_or(0x800);
        AString::append(body, chunk.as_ptr(), n);
        if read == 0 {
            return 0;
        }
    }
}

#[export_name = "?Post@RestContext@@AEAAKV?$basic_string@GU?$char_traits@G@std@@V?$allocator@G@2@@std@@0PEBGPEBDAEAV23@@Z"]
pub unsafe extern "C" fn post(this: *mut RestContext, user: *mut WString, password: *mut WString, path: *const u16, body: *const u8, out: *mut WString) -> u32 {
    if (*this).initialized == 0 {
        destroy_args(user, password);
        return ERROR_NOT_INITIALIZED;
    }
    WString::clear(out);
    if (*this).csrf_token.size == 0 {
        let mut ignored = WString::new();
        let mut password_copy = WString::new();
        WString::copy_construct(&mut password_copy, password);
        let mut user_copy = WString::new();
        WString::copy_construct(&mut user_copy, user);
        let e = get_os_version(this, &mut user_copy, &mut password_copy, &mut ignored);
        WString::tidy(&mut ignored);
        if e != 0 {
            destroy_args(user, password);
            return e;
        }
    }
    let request = WinHttpOpenRequest((*this).connection, w!("POST").as_ptr(), path, core::ptr::null(), core::ptr::null(), core::ptr::null(), WINHTTP_FLAG_SECURE);
    let mut err;
    if request.is_null() {
        err = GetLastError();
        if err != 0 {
            destroy_args(user, password);
            return err;
        }
    }
    if (*user).size != 0 && WinHttpSetCredentials(request, 0, 1, WString::data(user), WString::data(password), core::ptr::null_mut()) == 0 {
        err = GetLastError();
        if err != 0 {
            return finish(request, user, password, err);
        }
    }
    let mut first = true;
    loop {
        let mut headers = WString::new();
        WString::append_cstr(&mut headers, w!("\r\nX-CSRF-Token: ").as_ptr());
        WString::append(&mut headers, WString::data(&(*this).csrf_token), (*this).csrf_token.size);
        WString::append_cstr(&mut headers, w!("\r\nContent-Type: application/json").as_ptr());
        let len = core::ffi::CStr::from_ptr(body as *const core::ffi::c_char).to_bytes().len() as u32;
        let sent = WinHttpSendRequest(request, WString::data(&headers), u32::MAX, body as *mut c_void, len, len, 0);
        if sent != 0 {
            WString::tidy(&mut headers);
            break;
        }
        err = GetLastError();
        if first && err == ERROR_WINHTTP_SECURE_FAILURE {
            let mut flags = SECURITY_FLAG_IGNORE_ALL;
            if WinHttpSetOption(request, WINHTTP_OPTION_SECURITY_FLAGS, &mut flags as *mut u32 as *mut c_void, 4) == 0 {
                err = GetLastError();
                if err != 0 {
                    WString::tidy(&mut headers);
                    return finish(request, user, password, err);
                }
            }
            first = false;
        } else if err != 0 {
            WString::tidy(&mut headers);
            return finish(request, user, password, err);
        }
        WString::tidy(&mut headers);
    }
    if WinHttpReceiveResponse(request, core::ptr::null_mut()) == 0 {
        return finish(request, user, password, GetLastError());
    }
    let mut response = AString::new();
    let e = read_body(request, &mut response);
    if e != 0 {
        AString::tidy(&mut response);
        return finish(request, user, password, e);
    }
    let hr = crate::strings::convert_utf8_to_wide(AString::data(&response) as *const u8, response.size as u32, out);
    AString::tidy(&mut response);
    if hr < 0 {
        destroy_args(user, password);
        return ERROR_NO_UNICODE_TRANSLATION;
    }
    finish(request, user, password, 0)
}

#[export_name = "?GetOSVersion@RestContext@@QEAAKV?$basic_string@GU?$char_traits@G@std@@V?$allocator@G@2@@std@@0AEAV23@@Z"]
pub unsafe extern "C" fn get_os_version(this: *mut RestContext, user: *mut WString, password: *mut WString, out: *mut WString) -> u32 {
    let mut password_copy = WString::new();
    WString::copy_construct(&mut password_copy, password);
    let mut user_copy = WString::new();
    WString::copy_construct(&mut user_copy, user);
    let r = get(this, &mut user_copy, &mut password_copy, w!("/api/os/info").as_ptr(), out);
    WString::tidy(user);
    WString::tidy(password);
    r
}
