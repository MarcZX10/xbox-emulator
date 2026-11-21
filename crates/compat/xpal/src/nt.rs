use core::ffi::c_void;

pub const STATUS_SUCCESS: i32 = 0;
pub const STATUS_UNSUCCESSFUL: i32 = 0xC000_0001u32 as i32;
pub const STATUS_INVALID_PARAMETER: i32 = 0xC000_000Du32 as i32;
pub const STATUS_INVALID_DEVICE_REQUEST: i32 = 0xC000_0010u32 as i32;
pub const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023u32 as i32;
pub const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xC000_0034u32 as i32;
pub const STATUS_IO_TIMEOUT: i32 = 0xC000_00B5u32 as i32;
pub const STATUS_INVALID_DEVICE_STATE: i32 = 0xC000_0184u32 as i32;
pub const STATUS_NOT_FOUND: i32 = 0xC000_0225u32 as i32;

pub const ERROR_FILE_NOT_FOUND: u32 = 2;
pub const ERROR_OUTOFMEMORY: u32 = 14;
pub const ERROR_INVALID_PARAMETER: u32 = 87;

pub fn nt_error(s: i32) -> bool {
    (s as u32) & 0xC000_0000 == 0xC000_0000
}

#[link(name = "kernel32")]
extern "system" {
    pub fn SetLastError(e: u32);
    pub fn GetLastError() -> u32;
    pub fn GetCurrentProcess() -> *mut c_void;
    pub fn GetSystemTimeAsFileTime(ft: *mut u64);
}

#[link(name = "ntdll")]
extern "system" {
    pub fn RtlNtStatusToDosError(s: i32) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetSystemDirectoryW(buf: *mut u16, n: u32) -> u32;
}

pub fn system32_path(dll: &str) -> Option<Vec<u16>> {
    let mut b = vec![0u16; 520];
    let n = unsafe { GetSystemDirectoryW(b.as_mut_ptr(), b.len() as u32) } as usize;
    if n == 0 || n >= b.len() {
        return None;
    }
    b.truncate(n);
    b.push(b'\\' as u16);
    b.extend(dll.encode_utf16());
    b.push(0);
    Some(b)
}

type MiniDumpWriteDumpFn = unsafe extern "system" fn(*mut c_void, u32, *mut c_void, u32, *const c_void, *const c_void, *const c_void) -> i32;

pub fn minidump_writer() -> Option<MiniDumpWriteDumpFn> {
    static F: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let f = *F.get_or_init(|| system32_proc("dbghelp.dll", b"MiniDumpWriteDump\0"));
    (f != 0).then(|| unsafe { core::mem::transmute::<usize, MiniDumpWriteDumpFn>(f) })
}

pub const BCRYPT_SHA256_ALG_HANDLE: usize = 0x41;

type BCryptHashFn = unsafe extern "system" fn(usize, *const u8, u32, *const u8, u32, *mut u8, u32) -> i32;

pub(crate) fn system32_proc(dll: &str, name: &[u8]) -> usize {
    unsafe {
        let Some(w) = system32_path(dll) else { return 0 };
        let m = LoadLibraryExW(w.as_ptr(), core::ptr::null_mut(), 0);
        if m.is_null() {
            0
        } else {
            GetProcAddress(m, name.as_ptr()) as usize
        }
    }
}

pub fn sha256(data: &[u8]) -> Option<[u8; 32]> {
    static F: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let f = *F.get_or_init(|| system32_proc("bcrypt.dll", b"BCryptHash\0"));
    if f == 0 {
        return None;
    }
    let hash = unsafe { core::mem::transmute::<usize, BCryptHashFn>(f) };
    let mut o = [0u8; 32];
    let s = unsafe { hash(BCRYPT_SHA256_ALG_HANDLE, core::ptr::null(), 0, data.as_ptr(), data.len() as u32, o.as_mut_ptr(), 32) };
    (s >= 0).then_some(o)
}

pub const CRT_PRINTF_LEGACY_WIDE_SPECIFIERS: u64 = 4;

type VswprintfS = unsafe extern "C" fn(u64, *mut u16, usize, *const u16, *mut c_void, *const u64) -> i32;
type SetIph = unsafe extern "C" fn(usize) -> usize;

pub fn ucrt_proc(name: &[u8]) -> usize {
    system32_proc("ucrtbase.dll", name)
}

pub unsafe fn crt_swprintf_s(buf: *mut u16, cch: usize, fmt: &str, args: &[u64]) -> i32 {
    static F: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let f = *F.get_or_init(|| ucrt_proc(b"__stdio_common_vswprintf_s\0"));
    if f == 0 {
        crate::nt::fast_fail(FAST_FAIL_INVALID_ARG);
    }
    let w: Vec<u16> = fmt.encode_utf16().chain(Some(0)).collect();
    let call = core::mem::transmute::<usize, VswprintfS>(f);
    call(CRT_PRINTF_LEGACY_WIDE_SPECIFIERS, buf, cch, w.as_ptr(), core::ptr::null_mut(), args.as_ptr())
}

pub unsafe fn set_thread_invalid_parameter_handler(h: usize) -> usize {
    let f = ucrt_proc(b"_set_thread_local_invalid_parameter_handler\0");
    if f == 0 {
        return 0;
    }
    core::mem::transmute::<usize, SetIph>(f)(h)
}

pub const FAST_FAIL_INVALID_ARG: u32 = 5;

pub fn fast_fail(code: u32) -> ! {
    unsafe { core::arch::asm!("int 0x29", in("ecx") code, options(noreturn)) }
}

pub fn now_filetime() -> u64 {
    let mut t = 0u64;
    unsafe { GetSystemTimeAsFileTime(&mut t) };
    t
}

pub fn device_io_result(r: Result<usize, i32>, returned: Option<&mut u32>) -> bool {
    match r {
        Ok(n) => {
            if let Some(p) = returned {
                *p = n as u32;
            }
            true
        }
        Err(s) => {
            unsafe { SetLastError(RtlNtStatusToDosError(s)) };
            false
        }
    }
}
