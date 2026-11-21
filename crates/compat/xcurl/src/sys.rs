use core::ffi::c_void;
use std::sync::OnceLock;

pub type HINTERNET = *mut c_void;

#[link(name = "winhttp")]
extern "system" {
    pub fn WinHttpOpen(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> HINTERNET;
    pub fn WinHttpConnect(session: HINTERNET, server: *const u16, port: u16, reserved: u32) -> HINTERNET;
    pub fn WinHttpOpenRequest(
        connect: HINTERNET, verb: *const u16, object: *const u16, version: *const u16, referrer: *const u16, accept: *const *const u16, flags: u32,
    ) -> HINTERNET;
    pub fn WinHttpAddRequestHeaders(req: HINTERNET, headers: *const u16, len: u32, modifiers: u32) -> i32;
    pub fn WinHttpSendRequest(req: HINTERNET, headers: *const u16, hlen: u32, opt: *const c_void, olen: u32, total: u32, ctx: usize) -> i32;
    pub fn WinHttpReceiveResponse(req: HINTERNET, reserved: *mut c_void) -> i32;
    pub fn WinHttpQueryHeaders(req: HINTERNET, level: u32, name: *const u16, buf: *mut c_void, len: *mut u32, idx: *mut u32) -> i32;
    pub fn WinHttpQueryDataAvailable(req: HINTERNET, n: *mut u32) -> i32;
    pub fn WinHttpReadData(req: HINTERNET, buf: *mut c_void, len: u32, read: *mut u32) -> i32;
    pub fn WinHttpSetOption(h: HINTERNET, opt: u32, buf: *const c_void, len: u32) -> i32;
    pub fn WinHttpSetTimeouts(h: HINTERNET, resolve: i32, connect: i32, send: i32, receive: i32) -> i32;
    pub fn WinHttpCloseHandle(h: HINTERNET) -> i32;
    pub fn WinHttpQueryOption(h: HINTERNET, opt: u32, buf: *mut c_void, len: *mut u32) -> i32;
}

pub const WINHTTP_OPTION_URL: u32 = 34;
pub const WINHTTP_OPTION_CLIENT_CERT_CONTEXT: u32 = 47;
pub const WINHTTP_OPTION_ENABLE_FEATURE: u32 = 79;
pub const WINHTTP_OPTION_SERVER_CERT_CONTEXT: u32 = 78;
pub const WINHTTP_OPTION_SECURE_PROTOCOLS: u32 = 84;
pub const WINHTTP_OPTION_ENABLE_HTTP_PROTOCOL: u32 = 133;
pub const WINHTTP_PROTOCOL_FLAG_HTTP2: u32 = 1;
pub const WINHTTP_ENABLE_SSL_REVOCATION: u32 = 1;
pub const WINHTTP_DISABLE_KEEP_ALIVE: u32 = 8;
pub const SECURITY_FLAG_IGNORE_UNKNOWN_CA: u32 = 0x100;
pub const WINHTTP_OPTION_PROXY: u32 = 38;
pub const WINHTTP_OPTION_PROXY_USERNAME: u32 = 0x1002;
pub const WINHTTP_OPTION_PROXY_PASSWORD: u32 = 0x1003;

#[repr(C)]
pub struct ProxyInfo {
    pub access: u32,
    pub proxy: *const u16,
    pub bypass: *const u16,
}

extern "system" {
    pub fn GetLastError() -> u32;
    pub fn GetProcessHeap() -> *mut c_void;
    pub fn HeapAlloc(heap: *mut c_void, flags: u32, size: usize) -> *mut c_void;
    pub fn HeapFree(heap: *mut c_void, flags: u32, p: *mut c_void) -> i32;
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

pub const WINHTTP_ACCESS_TYPE_NO_PROXY: u32 = 1;
pub const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
pub const WINHTTP_FLAG_ESCAPE_DISABLE: u32 = 0x40;
pub const WINHTTP_FLAG_ESCAPE_DISABLE_QUERY: u32 = 0x80;
pub const WINHTTP_ADDREQ_FLAG_ADD: u32 = 0x2000_0000;
pub const WINHTTP_ADDREQ_FLAG_REPLACE: u32 = 0x8000_0000;
pub const WINHTTP_QUERY_STATUS_CODE: u32 = 19;
pub const WINHTTP_QUERY_RAW_HEADERS_CRLF: u32 = 22;
pub const WINHTTP_QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
pub const WINHTTP_OPTION_SECURITY_FLAGS: u32 = 31;
pub const WINHTTP_OPTION_DISABLE_FEATURE: u32 = 63;
pub const WINHTTP_OPTION_REDIRECT_POLICY: u32 = 88;
pub const WINHTTP_OPTION_DECOMPRESSION: u32 = 118;
pub const WINHTTP_DISABLE_COOKIES: u32 = 1;
pub const WINHTTP_DISABLE_REDIRECTS: u32 = 2;
pub const SECURITY_FLAG_IGNORE_ALL: u32 = 0x100 | 0x200 | 0x1000 | 0x2000;
pub const SECURITY_FLAG_IGNORE_CERT_CN_INVALID: u32 = 0x1000;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn heap_alloc(n: usize) -> *mut u8 {
    unsafe { HeapAlloc(GetProcessHeap(), 0, n.max(1)) as *mut u8 }
}

pub fn heap_free(p: *mut c_void) {
    if !p.is_null() {
        unsafe { HeapFree(GetProcessHeap(), 0, p) };
    }
}

pub fn heap_bytes(b: &[u8]) -> *mut u8 {
    let p = heap_alloc(b.len() + 1);
    if !p.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len());
            *p.add(b.len()) = 0;
        }
    }
    p
}

pub unsafe fn cstr_bytes<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    core::slice::from_raw_parts(p, n)
}

pub unsafe fn cstring(p: *const u8) -> Option<String> {
    if p.is_null() {
        None
    } else {
        Some(String::from_utf8_lossy(cstr_bytes(p)).into_owned())
    }
}

type StdioFn = unsafe extern "C" fn(*mut c_void, usize, usize, *mut c_void) -> usize;

fn ucrt(name: &[u8]) -> Option<StdioFn> {
    static BASE: OnceLock<usize> = OnceLock::new();
    let b = *BASE.get_or_init(|| unsafe { LoadLibraryA(b"ucrtbase.dll\0".as_ptr()) as usize });
    if b == 0 {
        return None;
    }
    let p = unsafe { GetProcAddress(b as *mut c_void, name.as_ptr()) };
    if p.is_null() {
        None
    } else {
        Some(unsafe { core::mem::transmute::<*mut c_void, StdioFn>(p) })
    }
}

pub fn crt_fwrite(buf: *const u8, n: usize, file: *mut c_void) -> usize {
    static F: OnceLock<Option<StdioFn>> = OnceLock::new();
    match F.get_or_init(|| ucrt(b"fwrite\0")) {
        Some(f) if !file.is_null() => unsafe { f(buf as *mut c_void, 1, n, file) },
        _ => n,
    }
}

pub fn crt_fread(buf: *mut u8, n: usize, file: *mut c_void) -> usize {
    static F: OnceLock<Option<StdioFn>> = OnceLock::new();
    match F.get_or_init(|| ucrt(b"fread\0")) {
        Some(f) if !file.is_null() => unsafe { f(buf as *mut c_void, 1, n, file) },
        _ => 0,
    }
}
