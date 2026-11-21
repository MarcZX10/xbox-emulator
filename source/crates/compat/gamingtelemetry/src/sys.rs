#![allow(non_snake_case)]
use std::ffi::c_void;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Guid {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: [u8; 8],
}

impl Guid {
    pub fn bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&self.d1.to_le_bytes());
        b[4..6].copy_from_slice(&self.d2.to_le_bytes());
        b[6..8].copy_from_slice(&self.d3.to_le_bytes());
        b[8..16].copy_from_slice(&self.d4);
        b
    }
    pub fn from_bytes(b: &[u8; 16]) -> Guid {
        Guid {
            d1: u32::from_le_bytes(b[0..4].try_into().unwrap()),
            d2: u16::from_le_bytes(b[4..6].try_into().unwrap()),
            d3: u16::from_le_bytes(b[6..8].try_into().unwrap()),
            d4: b[8..16].try_into().unwrap(),
        }
    }
    pub const fn new(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Guid {
        Guid { d1, d2, d3, d4 }
    }
    pub fn is_zero(&self) -> bool {
        self.bytes() == [0; 16]
    }
}

#[repr(C)]
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
pub struct EventData {
    pub ptr: u64,
    pub size: u32,
    pub kind: u32,
}

pub type EnableCb = unsafe extern "system" fn(*const Guid, u32, u8, u64, u64, *const c_void, *mut c_void);

#[link(name = "advapi32")]
extern "system" {
    pub fn EventRegister(id: *const Guid, cb: Option<EnableCb>, ctx: *mut c_void, handle: *mut u64) -> u32;
    pub fn EventUnregister(h: u64) -> u32;
    pub fn EventSetInformation(h: u64, class: u32, info: *const c_void, len: u32) -> u32;
    pub fn EventWriteTransfer(h: u64, d: *const EventDescriptor, a: *const Guid, r: *const Guid, n: u32, data: *const EventData) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetTickCount() -> u32;
    pub fn GetTickCount64() -> u64;
    pub fn MultiByteToWideChar(cp: u32, flags: u32, s: *const u8, n: i32, d: *mut u16, dn: i32) -> i32;
    pub fn LCMapStringEx(locale: *const u16, flags: u32, src: *const u16, n: i32, dst: *mut u16, dn: i32, v: *mut c_void, r: *mut c_void, lp: isize) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    pub fn CoCreateGuid(g: *mut Guid) -> i32;
}

#[link(name = "rpcrt4")]
extern "system" {
    pub fn UuidCreate(g: *mut Guid) -> i32;
}

#[link(name = "runtimeobject")]
extern "system" {
    pub fn WindowsGetStringRawBuffer(h: *mut c_void, len: *mut u32) -> *const u16;
    pub fn WindowsDeleteString(h: *mut c_void) -> i32;
}

extern "C" {
    fn __stdio_common_vswprintf(opts: u64, buf: *mut u16, n: usize, fmt: *const u16, loc: *mut c_void, args: *const u64) -> i32;
}

pub const STRSAFE_E_INSUFFICIENT_BUFFER: i32 = 0x8007007Au32 as i32;

pub fn printf(cch: usize, fmt: &[u16], args: &[u64]) -> Result<Vec<u16>, i32> {
    let mut f = fmt.to_vec();
    if f.last() != Some(&0) {
        f.push(0);
    }
    let mut va = vec![0u64; 256];
    va[..args.len()].copy_from_slice(args);
    let mut buf = vec![0u16; cch];
    let r = unsafe { __stdio_common_vswprintf(5, buf.as_mut_ptr(), cch - 1, f.as_ptr(), std::ptr::null_mut(), va.as_ptr()) };
    if r < 0 || r as usize > cch - 1 {
        return Err(STRSAFE_E_INSUFFICIENT_BUFFER);
    }
    buf.truncate(r as usize);
    Ok(buf)
}

pub fn hr_from_win32(e: u32) -> i32 {
    if (e as i32) > 0 {
        ((e & 0xffff) | 0x80070000) as i32
    } else {
        e as i32
    }
}

pub fn co_guid() -> Guid {
    let mut g = Guid::default();
    unsafe { CoCreateGuid(&mut g) };
    g
}

pub fn uuid() -> Guid {
    let mut g = Guid::default();
    unsafe { UuidCreate(&mut g) };
    g
}

pub fn tick64() -> u64 {
    unsafe { GetTickCount64() }
}

pub fn tick32() -> u64 {
    unsafe { GetTickCount() as u64 }
}
