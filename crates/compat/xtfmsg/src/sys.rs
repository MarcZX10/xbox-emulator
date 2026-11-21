use core::ffi::c_void;
pub use xtfcommon::sys::{hr_from_win32, wide, wcslen, strlen, GetLastError, HANDLE, HRESULT, BOOL, S_OK, E_FAIL, E_INVALIDARG, E_OUTOFMEMORY, E_UNEXPECTED};

pub type BSTR = *mut u16;
pub const E_PENDING: HRESULT = 0x8000_000Au32 as i32;
pub const E_NOINTERFACE: HRESULT = 0x8000_4002u32 as i32;
pub const E_NOTIMPL: HRESULT = 0x8000_4001u32 as i32;
pub const E_JSON_NOT_FOUND: HRESULT = 0x8375_0009u32 as i32;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl GUID {
    pub const fn new(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> GUID {
        GUID { data1: d1, data2: d2, data3: d3, data4: d4 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct EventDescriptor {
    pub id: u16,
    pub version: u8,
    pub channel: u8,
    pub level: u8,
    pub opcode: u8,
    pub task: u16,
    pub keyword: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct EventDataDescriptor {
    pub ptr: u64,
    pub size: u32,
    pub reserved: u32,
}

pub type EnableCallback = unsafe extern "system" fn(*const GUID, u32, u8, u64, u64, *const c_void, *mut c_void);

#[link(name = "kernel32")]
extern "system" {
    pub fn LoadLibraryW(name: *const u16) -> HANDLE;
    pub fn GetModuleHandleW(name: *const u16) -> HANDLE;
    pub fn GetProcAddress(m: HANDLE, name: *const u8) -> *mut c_void;
}

#[link(name = "oleaut32")]
extern "system" {
    pub fn SysAllocString(s: *const u16) -> BSTR;
    pub fn SysAllocStringLen(s: *const u16, n: u32) -> BSTR;
    pub fn SysFreeString(s: BSTR);
    pub fn SysStringLen(s: BSTR) -> u32;
}

#[link(name = "advapi32")]
extern "system" {
    pub fn EventRegister(guid: *const GUID, cb: Option<EnableCallback>, ctx: *mut c_void, handle: *mut u64) -> u32;
    pub fn EventUnregister(handle: u64) -> u32;
    pub fn EventSetInformation(handle: u64, class: u32, info: *const c_void, len: u32) -> u32;
    pub fn EventWriteTransfer(handle: u64, desc: *const EventDescriptor, activity: *const GUID, related: *const GUID, n: u32, data: *const EventDataDescriptor) -> u32;
    pub fn EventActivityIdControl(code: u32, id: *mut GUID) -> u32;
}

#[link(name = "rpcrt4")]
extern "system" {
    pub fn UuidCreate(id: *mut GUID) -> i32;
}

pub fn hr_last_error_or_fail() -> HRESULT {
    let e = unsafe { GetLastError() };
    if e == 0 {
        E_FAIL
    } else {
        hr_from_win32(e)
    }
}

pub unsafe fn wslice<'a>(p: *const u16) -> &'a [u16] {
    xtfcommon::sys::wslice(p)
}

pub fn from_wide(p: *const u16) -> String {
    String::from_utf16_lossy(unsafe { wslice(p) })
}
