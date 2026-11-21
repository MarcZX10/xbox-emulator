use core::ffi::c_void;

pub type HRESULT = i32;
pub type HANDLE = *mut c_void;
pub type BSTR = *mut u16;

pub const S_OK: HRESULT = 0;
pub const S_FALSE: HRESULT = 1;
pub const E_NOTIMPL: HRESULT = 0x8000_4001u32 as i32;
pub const E_POINTER: HRESULT = 0x8000_4003u32 as i32;
pub const E_ABORT: HRESULT = 0x8000_4004u32 as i32;
pub const E_FAIL: HRESULT = 0x8000_4005u32 as i32;
pub const E_UNEXPECTED: HRESULT = 0x8000_FFFFu32 as i32;
pub const E_PENDING: HRESULT = 0x8000_000Au32 as i32;
pub const E_INVALIDARG: HRESULT = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY: HRESULT = 0x8007_000Eu32 as i32;
pub const E_NOINTERFACE: HRESULT = 0x8000_4002u32 as i32;
pub const HR_NOT_SUPPORTED: HRESULT = 0x8007_0032u32 as i32;
pub const HR_SHARING_VIOLATION: HRESULT = 0x8007_0020u32 as i32;
pub const HR_INSUFFICIENT_BUFFER: HRESULT = 0x8007_007Au32 as i32;

pub const INVALID_HANDLE_VALUE: HANDLE = usize::MAX as HANDLE;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl GUID {
    pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        GUID { data1, data2, data3, data4 }
    }
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&self.data1.to_le_bytes());
        b[4..6].copy_from_slice(&self.data2.to_le_bytes());
        b[6..8].copy_from_slice(&self.data3.to_le_bytes());
        b[8..16].copy_from_slice(&self.data4);
        b
    }
    pub fn from_bytes(b: &[u8]) -> Self {
        let mut d4 = [0u8; 8];
        d4.copy_from_slice(&b[8..16]);
        GUID {
            data1: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            data2: u16::from_le_bytes([b[4], b[5]]),
            data3: u16::from_le_bytes([b[6], b[7]]),
            data4: d4,
        }
    }
    pub fn is_null(&self) -> bool {
        *self == GUID::default()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct OVERLAPPED {
    pub internal: usize,
    pub internal_high: usize,
    pub offset: u32,
    pub offset_high: u32,
    pub event: HANDLE,
}

impl Default for OVERLAPPED {
    fn default() -> Self {
        OVERLAPPED { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: core::ptr::null_mut() }
    }
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetLastError() -> u32;
    pub fn SetLastError(e: u32);
    pub fn CreateEventW(attr: *mut c_void, manual: i32, initial: i32, name: *const u16) -> HANDLE;
    pub fn SetEvent(h: HANDLE) -> i32;
    pub fn ResetEvent(h: HANDLE) -> i32;
    pub fn CloseHandle(h: HANDLE) -> i32;
    pub fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
    pub fn WaitForMultipleObjects(n: u32, h: *const HANDLE, all: i32, ms: u32) -> u32;
    pub fn Sleep(ms: u32);
    pub fn GetTickCount64() -> u64;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn GetCurrentProcessId() -> u32;
    pub fn GetCurrentThreadId() -> u32;
    pub fn OutputDebugStringA(s: *const u8);
    pub fn OutputDebugStringW(s: *const u16);
    pub fn LoadLibraryExW(name: *const u16, file: HANDLE, flags: u32) -> HANDLE;
    pub fn GetProcAddress(m: HANDLE, name: *const u8) -> *mut c_void;
    pub fn GetModuleHandleW(name: *const u16) -> HANDLE;
    pub fn GetModuleFileNameW(m: HANDLE, buf: *mut u16, n: u32) -> u32;
    pub fn GetEnvironmentVariableW(name: *const u16, buf: *mut u16, n: u32) -> u32;
    pub fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *mut c_void, disp: u32, flags: u32, tmpl: HANDLE) -> HANDLE;
    pub fn DeviceIoControl(h: HANDLE, code: u32, inb: *const c_void, inl: u32, outb: *mut c_void, outl: u32, ret: *mut u32, ov: *mut OVERLAPPED) -> i32;
    pub fn CancelIoEx(h: HANDLE, ov: *mut OVERLAPPED) -> i32;
    pub fn GetOverlappedResult(h: HANDLE, ov: *mut OVERLAPPED, n: *mut u32, wait: i32) -> i32;
    pub fn CreateIoCompletionPort(h: HANDLE, port: HANDLE, key: usize, threads: u32) -> HANDLE;
    pub fn CreateThread(sa: *mut c_void, stack: usize, start: unsafe extern "system" fn(*mut c_void) -> u32, param: *mut c_void, flags: u32, id: *mut u32) -> HANDLE;
    pub fn QueueUserWorkItem(f: unsafe extern "system" fn(*mut c_void) -> u32, ctx: *mut c_void, flags: u32) -> i32;
    pub fn ReadFile(h: HANDLE, buf: *mut c_void, n: u32, read: *mut u32, ov: *mut OVERLAPPED) -> i32;
    pub fn WriteFile(h: HANDLE, buf: *const c_void, n: u32, written: *mut u32, ov: *mut OVERLAPPED) -> i32;
    pub fn GetProcessHeap() -> HANDLE;
    pub fn HeapAlloc(h: HANDLE, flags: u32, n: usize) -> *mut c_void;
    pub fn HeapFree(h: HANDLE, flags: u32, p: *mut c_void) -> i32;
    pub fn MultiByteToWideChar(cp: u32, flags: u32, s: *const u8, n: i32, w: *mut u16, wn: i32) -> i32;
    pub fn WideCharToMultiByte(cp: u32, flags: u32, w: *const u16, wn: i32, s: *mut u8, n: i32, def: *const u8, used: *mut i32) -> i32;
    pub fn FormatMessageW(flags: u32, src: *const c_void, id: u32, lang: u32, buf: *mut u16, n: u32, args: *mut c_void) -> u32;
    pub fn IsDebuggerPresent() -> i32;
    pub fn TlsAlloc() -> u32;
    pub fn TlsGetValue(i: u32) -> *mut c_void;
    pub fn TlsSetValue(i: u32, v: *mut c_void) -> i32;
}

#[link(name = "oleaut32")]
extern "system" {
    pub fn SysAllocString(s: *const u16) -> BSTR;
    pub fn SysAllocStringLen(s: *const u16, n: u32) -> BSTR;
    pub fn SysFreeString(s: BSTR);
    pub fn SysStringLen(s: BSTR) -> u32;
}

#[link(name = "ntdll")]
extern "system" {
    pub fn NtSetInformationProcess(h: HANDLE, class: u32, info: *const c_void, len: u32) -> i32;
}

extern "C" {
    pub fn malloc(n: usize) -> *mut c_void;
    pub fn free(p: *mut c_void);
}

pub fn hr_from_win32(e: u32) -> HRESULT {
    if (e as i32) <= 0 {
        e as i32
    } else {
        ((e & 0xFFFF) | 0x8007_0000) as i32
    }
}

pub fn hr_last_error() -> HRESULT {
    hr_from_win32(unsafe { GetLastError() })
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(core::iter::once(0)).collect()
}

pub unsafe fn wcslen(p: *const u16) -> usize {
    if p.is_null() {
        return 0;
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    n
}

pub unsafe fn from_wide(p: *const u16) -> String {
    String::from_utf16_lossy(core::slice::from_raw_parts(p, wcslen(p)))
}

pub unsafe fn strlen(p: *const u8) -> usize {
    if p.is_null() {
        return 0;
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    n
}

pub unsafe fn bstr_from(s: &str) -> BSTR {
    let w = wide(s);
    SysAllocString(w.as_ptr())
}
