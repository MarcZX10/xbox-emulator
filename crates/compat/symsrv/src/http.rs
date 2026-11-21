use crate::win::*;
use crate::*;

pub const OFFLINE_ERROR: u32 = 12029;

#[link(name = "wininet")]
extern "system" {
    fn InternetQueryDataAvailable(h: isize, n: *mut u32, flags: u32, ctx: usize) -> i32;
    fn InternetReadFile(h: isize, buf: *mut u8, n: u32, read: *mut u32) -> i32;
    fn InternetCloseHandle(h: isize) -> i32;
}

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpQueryDataAvailable(h: isize, n: *mut u32) -> i32;
    fn WinHttpReadData(h: isize, buf: *mut u8, n: u32, read: *mut u32) -> i32;
    fn WinHttpCloseHandle(h: isize) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn AllocateAndInitializeSid(auth: *const [u8; 6], n: u8, s0: u32, s1: u32, s2: u32, s3: u32, s4: u32, s5: u32, s6: u32, s7: u32, out: *mut *mut u8) -> i32;
    fn CheckTokenMembership(tok: isize, sid: *mut u8, member: *mut i32) -> i32;
    fn FreeSid(sid: *mut u8) -> *mut u8;
}

pub fn is_service() -> bool {
    if has(SSRVOPT_SERVICE) {
        return true;
    }
    unsafe {
        let nt = [0u8, 0, 0, 0, 0, 5];
        let mut sid = core::ptr::null_mut();
        if AllocateAndInitializeSid(&nt, 1, 6, 0, 0, 0, 0, 0, 0, 0, &mut sid) == 0 {
            return false;
        }
        let mut m = 0i32;
        let ok = CheckTokenMembership(0, sid, &mut m) != 0 && m != 0;
        FreeSid(sid);
        ok
    }
}

pub fn engine() -> u64 {
    let o = options();
    if o & SSRVOPT_WININET != 0 {
        SSRVOPT_WININET
    } else if o & SSRVOPT_WINHTTP != 0 || is_service() {
        SSRVOPT_WINHTTP
    } else {
        SSRVOPT_WININET
    }
}

pub fn split_url(url: &[u16]) -> Option<(Vec<u16>, Vec<u16>)> {
    let t = classify(url);
    if t >= 2 {
        return None;
    }
    let skip = if t == 0 { 7 } else { 8 };
    let rest = &url[skip..];
    let cut = rest.iter().position(|c| *c == b'/' as u16 || *c == b'\\' as u16).unwrap_or(rest.len());
    let site = url[..skip + cut].to_vec();
    let mut path: Vec<u16> = rest[cut..].iter().map(|c| if *c == b'\\' as u16 { b'/' as u16 } else { *c }).collect();
    while path.first() == Some(&(b'/' as u16)) {
        path.remove(0);
    }
    Some((site, path))
}

pub unsafe fn resolve(site: *const u16, file: *const u16) -> Result<(Vec<u16>, Vec<u16>), u32> {
    let f = wstr(file);
    if site.is_null() || *site == 0 {
        return split_url(f).ok_or(87);
    }
    let s: Vec<u16> = wstr(site).iter().take(MAX_PATH_W - 1).copied().collect();
    let rest = if f.starts_with(&s) && !s.is_empty() { &f[(s.len() + 1).min(f.len())..] } else { f };
    let path: Vec<u16> = rest.iter().take(MAX_PATH_W - 1).map(|c| if *c == b'\\' as u16 { b'/' as u16 } else { *c }).collect();
    if classify(&s) >= 2 {
        return Err(87);
    }
    Ok((s, path))
}

#[no_mangle]
pub unsafe extern "system" fn httpOpenFileHandleW(site: *const u16, file: *const u16, _reserved: u32, file_handle: *mut isize, site_handle: *mut isize) -> i32 {
    if file.is_null() {
        SetLastError(87);
        return 0;
    }
    match resolve(site, file) {
        Err(e) => {
            SetLastError(e);
            0
        }
        Ok(_) => {
            if !file_handle.is_null() {
                *file_handle = 0;
            }
            if !site_handle.is_null() {
                *site_handle = 0;
            }
            SetLastError(OFFLINE_ERROR);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn httpOpenFileHandle(site: *const u8, file: *const u8, reserved: u32, file_handle: *mut isize, site_handle: *mut isize) -> i32 {
    let s = if site.is_null() { Vec::new() } else { z(&a2w(site, 0x40f)) };
    let f = if file.is_null() { Vec::new() } else { z(&a2w(file, 0x40f)) };
    let sp = if s.is_empty() { [0u16].as_ptr() } else { s.as_ptr() };
    let fp = if f.is_empty() { [0u16].as_ptr() } else { f.as_ptr() };
    httpOpenFileHandleW(sp, fp, reserved, file_handle, site_handle)
}

#[no_mangle]
pub unsafe extern "system" fn httpQueryDataAvailable(h: isize, n: *mut u32, flags: u32, ctx: usize) -> i32 {
    if engine() == SSRVOPT_WINHTTP {
        WinHttpQueryDataAvailable(h, n)
    } else {
        InternetQueryDataAvailable(h, n, flags, ctx)
    }
}

#[no_mangle]
pub unsafe extern "system" fn httpReadFile(h: isize, buf: *mut u8, n: u32, read: *mut u32) -> i32 {
    if engine() == SSRVOPT_WINHTTP {
        WinHttpReadData(h, buf, n, read)
    } else {
        InternetReadFile(h, buf, n, read)
    }
}

pub unsafe fn close_handle(h: isize) -> i32 {
    if h == 0 {
        return 0;
    }
    let e = GetLastError();
    let r = if engine() == SSRVOPT_WINHTTP { WinHttpCloseHandle(h) } else { InternetCloseHandle(h) };
    if e != 0 {
        SetLastError(e);
    }
    r
}

#[no_mangle]
pub unsafe extern "system" fn httpCloseHandle(h: isize) -> i32 {
    close_handle(h)
}
