use crate::sys::*;
use core::ffi::c_void;

pub const PRINTF_OPTIONS: u64 = 0;

pub unsafe fn vscprintf(fmt: *const u8, va: *const c_void) -> i32 {
    let r = __stdio_common_vsprintf(PRINTF_OPTIONS | 2, core::ptr::null_mut(), 0, fmt, core::ptr::null_mut(), va);
    if r < 0 {
        -1
    } else {
        r
    }
}

pub unsafe fn vsprintf_s(buf: *mut u8, n: usize, fmt: *const u8, va: *const c_void) -> i32 {
    let r = __stdio_common_vsprintf_s(PRINTF_OPTIONS, buf, n, fmt, core::ptr::null_mut(), va);
    if r < 0 {
        -1
    } else {
        r
    }
}

pub unsafe fn sprintf_s(buf: *mut u8, n: usize, fmt: *const u8, args: &[u64]) -> i32 {
    vsprintf_s(buf, n, fmt, args.as_ptr() as *const c_void)
}

pub unsafe fn vswprintf_s(buf: *mut u16, n: usize, fmt: *const u16, va: *const c_void) -> i32 {
    let r = __stdio_common_vswprintf_s(PRINTF_OPTIONS, buf, n, fmt, core::ptr::null_mut(), va);
    if r < 0 {
        -1
    } else {
        r
    }
}

pub unsafe fn format_alloc(out: *mut *mut u8, fmt: *const u8, va: *const c_void, fix_crlf: bool) -> i32 {
    *out = core::ptr::null_mut();
    if fmt.is_null() {
        return 0;
    }
    let mut owned: Vec<u8>;
    let mut f = fmt;
    if fix_crlf {
        owned = cslice(fmt).to_vec();
        let n = owned.len();
        let ends_crlf = n >= 2 && owned[n - 2] == b'\r' && owned[n - 1] == b'\n';
        if !ends_crlf {
            if n >= 1 && owned[n - 1] == b'\n' {
                owned[n - 1] = b'\r';
                owned.push(b'\n');
            } else {
                owned.extend_from_slice(b"\r\n");
            }
            owned.push(0);
            f = owned.as_ptr();
        }
    } else {
        owned = Vec::new();
    }
    let zero = [0u64; 16];
    let (n, va) = if va.is_null() {
        let l = strlen(f) as i32;
        (l, zero.as_ptr() as *const c_void)
    } else {
        (vscprintf(f, va), va)
    };
    if n < 0 {
        drop(owned);
        return 0;
    }
    let size = n as usize + 1;
    let buf = malloc(size) as *mut u8;
    if buf.is_null() {
        return 0;
    }
    let r = vsprintf_s(buf, size, f, va);
    drop(owned);
    if r < 0 {
        free(buf as *mut c_void);
        return 0;
    }
    *out = buf;
    r
}
