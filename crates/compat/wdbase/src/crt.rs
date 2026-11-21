use core::ffi::c_void;

pub const PRINTF_OPTIONS: u64 = 0x1c;

extern "C" {
    pub fn __acrt_iob_func(index: u32) -> *mut c_void;
    pub fn __stdio_common_vfwprintf(options: u64, stream: *mut c_void, format: *const u16, locale: *mut c_void, args: *const usize) -> i32;
    pub fn __stdio_common_vfprintf(options: u64, stream: *mut c_void, format: *const u8, locale: *mut c_void, args: *const usize) -> i32;
    pub fn __stdio_common_vswprintf_s(options: u64, buffer: *mut u16, count: usize, format: *const u16, locale: *mut c_void, args: *const usize) -> i32;
    pub fn fflush(stream: *mut c_void) -> i32;
    pub fn _wcsicmp(a: *const u16, b: *const u16) -> i32;
    pub fn _wcsnicmp(a: *const u16, b: *const u16, n: usize) -> i32;
    pub fn wcschr(s: *const u16, c: u16) -> *const u16;
    pub fn _kbhit() -> i32;
    pub fn _getwch() -> u16;
}

#[macro_export]
macro_rules! w {
    ($s:literal) => {{
        const S: &str = $s;
        const N: usize = S.len() + 1;
        const A: [u16; N] = {
            let b = S.as_bytes();
            let mut a = [0u16; N];
            let mut i = 0;
            while i < b.len() {
                a[i] = b[i] as u16;
                i += 1;
            }
            a
        };
        &A
    }};
}

pub unsafe fn wprintf(format: &[u16], args: &[usize]) -> i32 {
    __stdio_common_vfwprintf(PRINTF_OPTIONS, __acrt_iob_func(1), format.as_ptr(), core::ptr::null_mut(), args.as_ptr())
}

pub unsafe fn fprintf(stream: u32, format: &[u8], args: &[usize]) -> i32 {
    __stdio_common_vfprintf(PRINTF_OPTIONS, __acrt_iob_func(stream), format.as_ptr(), core::ptr::null_mut(), args.as_ptr())
}

pub unsafe fn swprintf_s(buffer: *mut u16, count: usize, format: &[u16], args: &[usize]) -> i32 {
    __stdio_common_vswprintf_s(PRINTF_OPTIONS, buffer, count, format.as_ptr(), core::ptr::null_mut(), args.as_ptr())
}
