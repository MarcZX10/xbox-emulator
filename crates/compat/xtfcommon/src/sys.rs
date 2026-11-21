use core::ffi::c_void;

pub type HRESULT = i32;
pub type HANDLE = *mut c_void;
pub type BOOL = i32;

pub const S_OK: HRESULT = 0;
pub const E_FAIL: HRESULT = 0x8000_4005u32 as i32;
pub const E_UNEXPECTED: HRESULT = 0x8000_FFFFu32 as i32;
pub const E_INVALIDARG: HRESULT = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY: HRESULT = 0x8007_000Eu32 as i32;
pub const HR_INSUFFICIENT_BUFFER: HRESULT = 0x8007_007Au32 as i32;
pub const STG_E_WRITEFAULT: HRESULT = 0x8003_001Du32 as i32;

pub const INVALID_HANDLE_VALUE: HANDLE = usize::MAX as HANDLE;
pub const INFINITE: u32 = 0xFFFF_FFFF;
pub const ERROR_ALREADY_EXISTS: u32 = 0xB7;
pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
pub const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFF_FFFF;
pub const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CONSOLE_SCREEN_BUFFER_INFO {
    pub size: u32,
    pub cursor: u32,
    pub attributes: u16,
    pub window: [u16; 4],
    pub max_window: u32,
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetLastError() -> u32;
    pub fn SetLastError(e: u32);
    pub fn CloseHandle(h: HANDLE) -> BOOL;
    pub fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
    pub fn CreateSemaphoreW(sa: *mut c_void, initial: i32, max: i32, name: *const u16) -> HANDLE;
    pub fn ReleaseSemaphore(h: HANDLE, n: i32, prev: *mut i32) -> BOOL;
    pub fn CreateMutexW(sa: *mut c_void, owner: BOOL, name: *const u16) -> HANDLE;
    pub fn ReleaseMutex(h: HANDLE) -> BOOL;
    pub fn GetCurrentProcessId() -> u32;
    pub fn GetCurrentThreadId() -> u32;
    pub fn OutputDebugStringA(s: *const u8);
    pub fn IsDebuggerPresent() -> BOOL;
    pub fn DebugBreak();
    pub fn GetEnvironmentVariableW(name: *const u16, buf: *mut u16, n: u32) -> u32;
    pub fn GetSystemTimePreciseAsFileTime(ft: *mut u64);
    pub fn GetLocalTime(st: *mut SYSTEMTIME);
    pub fn FileTimeToLocalFileTime(ft: *const u64, local: *mut u64) -> BOOL;
    pub fn FileTimeToSystemTime(ft: *const u64, st: *mut SYSTEMTIME) -> BOOL;
    pub fn GetFullPathNameW(name: *const u16, n: u32, buf: *mut u16, part: *mut *mut u16) -> u32;
    pub fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *mut c_void, disp: u32, flags: u32, tmpl: HANDLE) -> HANDLE;
    pub fn WriteFile(h: HANDLE, buf: *const c_void, n: u32, written: *mut u32, ov: *mut c_void) -> BOOL;
    pub fn SetFilePointer(h: HANDLE, lo: i32, hi: *mut i32, method: u32) -> u32;
    pub fn GetFileAttributesW(name: *const u16) -> u32;
    pub fn CreateDirectoryW(name: *const u16, sa: *mut c_void) -> BOOL;
    pub fn GetStdHandle(n: u32) -> HANDLE;
    pub fn GetConsoleScreenBufferInfo(h: HANDLE, info: *mut CONSOLE_SCREEN_BUFFER_INFO) -> BOOL;
    pub fn SetConsoleTextAttribute(h: HANDLE, attr: u16) -> BOOL;
    pub fn MultiByteToWideChar(cp: u32, flags: u32, s: *const u8, n: i32, w: *mut u16, wn: i32) -> i32;
}

extern "C" {
    pub fn malloc(n: usize) -> *mut c_void;
    pub fn free(p: *mut c_void);
    pub fn atexit(f: unsafe extern "C" fn()) -> i32;
    pub fn _purecall() -> i32;
    pub fn _wsplitpath_s(path: *const u16, drive: *mut u16, dn: usize, dir: *mut u16, dirn: usize, fname: *mut u16, fnn: usize, ext: *mut u16, extn: usize) -> i32;
    pub fn _wcsicmp(a: *const u16, b: *const u16) -> i32;
    pub fn iswalnum(c: u16) -> i32;
    pub fn towupper(c: u16) -> u16;
    pub fn wcscpy_s(dst: *mut u16, n: usize, src: *const u16) -> i32;
    pub fn _wtoi(s: *const u16) -> i32;
    pub fn __stdio_common_vsprintf(options: u64, buf: *mut u8, count: usize, fmt: *const u8, locale: *mut c_void, args: *const c_void) -> i32;
    pub fn __stdio_common_vsprintf_s(options: u64, buf: *mut u8, count: usize, fmt: *const u8, locale: *mut c_void, args: *const c_void) -> i32;
    pub fn __stdio_common_vswprintf_s(options: u64, buf: *mut u16, count: usize, fmt: *const u16, locale: *mut c_void, args: *const c_void) -> i32;
}

pub fn hr_from_last_error() -> HRESULT {
    let e = unsafe { GetLastError() };
    hr_from_win32(e)
}

pub fn hr_from_win32(e: u32) -> HRESULT {
    if (e as i32) <= 0 {
        e as i32
    } else {
        ((e & 0xFFFF) | 0x8007_0000) as i32
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(core::iter::once(0)).collect()
}

pub unsafe fn wcslen(p: *const u16) -> usize {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    n
}

pub unsafe fn strlen(p: *const u8) -> usize {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    n
}

pub unsafe fn wslice<'a>(p: *const u16) -> &'a [u16] {
    if p.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(p, wcslen(p))
    }
}

pub unsafe fn cslice<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(p, strlen(p))
    }
}
