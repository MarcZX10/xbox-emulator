#[link(name = "kernel32")]
extern "system" {
    pub fn SetLastError(e: u32);
    pub fn GetLastError() -> u32;
    pub fn GetFileAttributesW(p: *const u16) -> u32;
    pub fn GetFileAttributesExW(p: *const u16, level: u32, info: *mut FileAttr) -> i32;
    pub fn CreateDirectoryW(p: *const u16, sa: *const u8) -> i32;
    pub fn RemoveDirectoryW(p: *const u16) -> i32;
    pub fn DeleteFileW(p: *const u16) -> i32;
    pub fn CopyFileExW(src: *const u16, dst: *const u16, prog: usize, data: usize, cancel: *mut i32, flags: u32) -> i32;
    pub fn MoveFileW(src: *const u16, dst: *const u16) -> i32;
    pub fn CreateFileW(p: *const u16, access: u32, share: u32, sa: *const u8, disp: u32, flags: u32, tmpl: isize) -> isize;
    pub fn CreateFileA(p: *const u8, access: u32, share: u32, sa: *const u8, disp: u32, flags: u32, tmpl: isize) -> isize;
    pub fn ReadFile(h: isize, buf: *mut u8, n: u32, read: *mut u32, ov: *mut u8) -> i32;
    pub fn WriteFile(h: isize, buf: *const u8, n: u32, written: *mut u32, ov: *mut u8) -> i32;
    pub fn SetFilePointer(h: isize, lo: i32, hi: *mut i32, method: u32) -> u32;
    pub fn GetFileSize(h: isize, hi: *mut u32) -> u32;
    pub fn CloseHandle(h: isize) -> i32;
    pub fn DeleteFileA(p: *const u8) -> i32;
    pub fn MultiByteToWideChar(cp: u32, fl: u32, a: *const u8, an: i32, w: *mut u16, wn: i32) -> i32;
    pub fn WideCharToMultiByte(cp: u32, fl: u32, w: *const u16, wn: i32, a: *mut u8, an: i32, d: *const u8, u: *mut i32) -> i32;
    pub fn GetModuleFileNameW(m: isize, buf: *mut u16, n: u32) -> u32;
    pub fn GetModuleHandleExW(flags: u32, addr: *const u8, out: *mut isize) -> i32;
    pub fn GetModuleHandleW(name: *const u16) -> isize;
    pub fn GetProcAddress(m: isize, name: *const u8) -> usize;
    pub fn GetTempPathW(n: u32, buf: *mut u16) -> u32;
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct FileAttr {
    pub attrs: u32,
    pub created: [u32; 2],
    pub accessed: [u32; 2],
    pub written: [u32; 2],
    pub size_hi: u32,
    pub size_lo: u32,
}

pub const INVALID: isize = -1;
pub const GENERIC_READ: u32 = 0x8000_0000;
pub const GENERIC_WRITE: u32 = 0x4000_0000;

pub unsafe fn wstr<'a>(p: *const u16) -> &'a [u16] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    core::slice::from_raw_parts(p, n)
}

pub unsafe fn astr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    core::slice::from_raw_parts(p, n)
}

pub fn z(v: &[u16]) -> Vec<u16> {
    let mut o = v.to_vec();
    o.push(0);
    o
}

pub fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub unsafe fn a2w(p: *const u8, max: usize) -> Vec<u16> {
    let a = astr(p);
    let a = &a[..a.len().min(max)];
    if a.is_empty() {
        return Vec::new();
    }
    let n = MultiByteToWideChar(0, 0, a.as_ptr(), a.len() as i32, core::ptr::null_mut(), 0);
    let mut o = vec![0u16; n.max(0) as usize];
    MultiByteToWideChar(0, 0, a.as_ptr(), a.len() as i32, o.as_mut_ptr(), n);
    o
}

pub fn w2a(v: &[u16]) -> Vec<u8> {
    if v.is_empty() {
        return Vec::new();
    }
    unsafe {
        let n = WideCharToMultiByte(0, 0, v.as_ptr(), v.len() as i32, core::ptr::null_mut(), 0, core::ptr::null(), core::ptr::null_mut());
        let mut o = vec![0u8; n.max(0) as usize];
        WideCharToMultiByte(0, 0, v.as_ptr(), v.len() as i32, o.as_mut_ptr(), n, core::ptr::null(), core::ptr::null_mut());
        o
    }
}

pub fn exists(p: &[u16]) -> bool {
    unsafe { GetFileAttributesW(z(p).as_ptr()) != u32::MAX }
}

pub fn attr_error(p: &[u16]) -> u32 {
    unsafe {
        if GetFileAttributesW(z(p).as_ptr()) != u32::MAX {
            0
        } else {
            GetLastError()
        }
    }
}

pub fn add_backslash(v: &mut Vec<u16>) {
    if v.len() < 0x105 && v.last() != Some(&(b'\\' as u16)) {
        v.push(b'\\' as u16);
    }
}

pub fn join(a: &[u16], b: &[u16]) -> Vec<u16> {
    let mut v = a.to_vec();
    add_backslash(&mut v);
    v.extend_from_slice(b);
    v
}

pub fn map_missing(e: u32) -> u32 {
    match e {
        87 | 3 => 2,
        0x7b => 87,
        x => x,
    }
}
