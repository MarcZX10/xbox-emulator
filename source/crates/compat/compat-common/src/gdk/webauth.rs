use super::win::{hr_from_win32, wide};
use super::xasync::Cancel;
use super::{log, E_ABORT, E_INVALIDARG};
use core::ffi::c_void;
use std::sync::Arc;

#[repr(C)]
struct UrlComponents {
    size: u32,
    scheme: *mut u16,
    scheme_len: u32,
    scheme_kind: i32,
    host: *mut u16,
    host_len: u32,
    port: u16,
    user: *mut u16,
    user_len: u32,
    pass: *mut u16,
    pass_len: u32,
    path: *mut u16,
    path_len: u32,
    extra: *mut u16,
    extra_len: u32,
}

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> *mut c_void;
    fn WinHttpCrackUrl(url: *const u16, len: u32, flags: u32, c: *mut UrlComponents) -> i32;
    fn WinHttpConnect(s: *mut c_void, host: *const u16, port: u16, r: u32) -> *mut c_void;
    fn WinHttpOpenRequest(c: *mut c_void, verb: *const u16, path: *const u16, ver: *const u16, referrer: *const u16, accept: *const *const u16, flags: u32) -> *mut c_void;
    fn WinHttpSetOption(h: *mut c_void, opt: u32, buf: *const c_void, len: u32) -> i32;
    fn WinHttpSendRequest(h: *mut c_void, hdr: *const u16, hlen: u32, opt: *const c_void, olen: u32, total: u32, ctx: usize) -> i32;
    fn WinHttpReceiveResponse(h: *mut c_void, r: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(h: *mut c_void, level: u32, name: *const u16, buf: *mut c_void, len: *mut u32, index: *mut u32) -> i32;
    fn WinHttpCloseHandle(h: *mut c_void) -> i32;
    pub fn WinHttpQueryOption(h: *mut c_void, opt: u32, buf: *mut c_void, len: *mut u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> u32;
}

struct H(*mut c_void);
impl Drop for H {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn err() -> i32 {
    hr_from_win32(unsafe { GetLastError() })
}

pub fn resolve(base: &str, loc: &str) -> String {
    if loc.contains("://") {
        return loc.to_string();
    }
    let scheme_end = base.find("://").map(|i| i + 3).unwrap_or(0);
    let host_end = base[scheme_end..].find('/').map(|i| i + scheme_end).unwrap_or(base.len());
    if loc.starts_with('/') {
        format!("{}{}", &base[..host_end], loc)
    } else {
        let dir_end = base.rfind('/').filter(|&i| i >= host_end).map(|i| i + 1).unwrap_or(base.len());
        let mut b = base[..dir_end].to_string();
        if !b.ends_with('/') {
            b.push('/');
        }
        format!("{b}{loc}")
    }
}

pub fn matches_completion(uri: &str, completion: &str) -> bool {
    let strip = |s: &str| s.split(['?', '#']).next().unwrap_or("").trim_end_matches('/').to_ascii_lowercase();
    strip(uri) == strip(completion) || uri.to_ascii_lowercase().starts_with(&completion.to_ascii_lowercase())
}

unsafe fn fetch(session: *mut c_void, url: &str) -> Result<(u32, Option<String>), i32> {
    let w = wide(url);
    let mut host = vec![0u16; 512];
    let mut path = vec![0u16; 4096];
    let mut extra = vec![0u16; 8192];
    let mut c = UrlComponents {
        size: core::mem::size_of::<UrlComponents>() as u32,
        scheme: core::ptr::null_mut(),
        scheme_len: 0,
        scheme_kind: 0,
        host: host.as_mut_ptr(),
        host_len: host.len() as u32,
        port: 0,
        user: core::ptr::null_mut(),
        user_len: 0,
        pass: core::ptr::null_mut(),
        pass_len: 0,
        path: path.as_mut_ptr(),
        path_len: path.len() as u32,
        extra: extra.as_mut_ptr(),
        extra_len: extra.len() as u32,
    };
    if WinHttpCrackUrl(w.as_ptr(), 0, 0, &mut c) == 0 {
        return Err(E_INVALIDARG);
    }
    let host_s = String::from_utf16_lossy(&host[..c.host_len as usize]);
    let mut target: Vec<u16> = path[..c.path_len as usize].to_vec();
    target.extend_from_slice(&extra[..c.extra_len as usize]);
    if target.is_empty() {
        target.push(b'/' as u16);
    }
    target.push(0);
    let conn = H(WinHttpConnect(session, wide(&host_s).as_ptr(), c.port, 0));
    if conn.0.is_null() {
        return Err(err());
    }
    let secure = if c.scheme_kind == 2 { 0x0080_0000 } else { 0 };
    let req = H(WinHttpOpenRequest(conn.0, wide("GET").as_ptr(), target.as_ptr(), core::ptr::null(), core::ptr::null(), core::ptr::null(), secure));
    if req.0.is_null() {
        return Err(err());
    }
    let never: u32 = 2;
    WinHttpSetOption(req.0, 88, &never as *const u32 as *const c_void, 4);
    if WinHttpSendRequest(req.0, core::ptr::null(), 0, core::ptr::null(), 0, 0, 0) == 0 || WinHttpReceiveResponse(req.0, core::ptr::null_mut()) == 0 {
        return Err(err());
    }
    let mut status = 0u32;
    let mut len = 4u32;
    WinHttpQueryHeaders(req.0, 19 | 0x2000_0000, core::ptr::null(), &mut status as *mut u32 as *mut c_void, &mut len, core::ptr::null_mut());
    let mut loc = vec![0u16; 8192];
    let mut llen = (loc.len() * 2) as u32;
    let location = if WinHttpQueryHeaders(req.0, 33, core::ptr::null(), loc.as_mut_ptr() as *mut c_void, &mut llen, core::ptr::null_mut()) != 0 {
        Some(String::from_utf16_lossy(&loc[..llen as usize / 2]))
    } else {
        None
    };
    Ok((status, location))
}

pub fn run(start: &str, completion: &str, cancel: &Arc<Cancel>) -> Result<(String, u32), i32> {
    if matches_completion(start, completion) {
        return Ok((start.to_string(), 200));
    }
    unsafe {
        let session = H(WinHttpOpen(wide("XboxEmulator-WebAuth/1.0").as_ptr(), 4, core::ptr::null(), core::ptr::null(), 0));
        if session.0.is_null() {
            return Err(err());
        }
        let mut url = start.to_string();
        for _ in 0..20 {
            if cancel.is_set() {
                return Err(E_ABORT);
            }
            let (status, location) = fetch(session.0, &url)?;
            log("XGameUiShowWebAuthentication", &format!("GET {url} -> {status}"));
            match location {
                Some(l) if (300..400).contains(&status) => {
                    let next = resolve(&url, &l);
                    if matches_completion(&next, completion) {
                        return Ok((next, status));
                    }
                    url = next;
                }
                _ => return Ok((url, status)),
            }
        }
        Err(hr_from_win32(1156))
    }
}
