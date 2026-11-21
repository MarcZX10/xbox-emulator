use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::msvc::AString;
use crate::ntpath::{last_error_hr, NtPathMap};

pub type Guid = [u8; 16];

pub const fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> Guid {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    [a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]
}

pub const IID_IUNKNOWN: Guid = guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
pub const IID_ILIVEMERGER: Guid = guid(0x6f141e8f, 0xc145, 0x4665, [0x98, 0xdd, 0x69, 0x87, 0xf4, 0xe9, 0x69, 0x27]);
pub const IMAGE_LOAD_GUID: Guid = guid(0x2cb15d1d, 0x5fc1, 0x11d2, [0xab, 0xe1, 0x00, 0xa0, 0xc9, 0x11, 0xf5, 0x18]);
pub const FILE_IO_GUID: Guid = guid(0x90cbdc39, 0x4a3e, 0x11d1, [0x84, 0xf4, 0x00, 0x00, 0xf8, 0x04, 0x64, 0xe3]);
pub const IMAGE_ID_GUID: Guid = guid(0xb3e675d7, 0x2554, 0x4f18, [0x83, 0x0b, 0x27, 0x62, 0x73, 0x25, 0x60, 0xde]);
pub const VOLUME_MAP_GUID: Guid = guid(0x9b79ee91, 0xb5fd, 0x41c0, [0xa2, 0x43, 0x42, 0x48, 0xe2, 0x66, 0xe9, 0xd0]);
pub const MERGER_EVENT_GUID: Guid = guid(0x82c216c0, 0x6341, 0x4aca, [0xb2, 0xe7, 0x75, 0x99, 0x36, 0x64, 0x33, 0x8c]);

pub const S_OK: i32 = 0;
pub const E_POINTER: i32 = 0x8000_4003u32 as i32;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_UNEXPECTED: i32 = 0x8000_ffffu32 as i32;
pub const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
pub const HR_NOT_FOUND: i32 = 0x8007_0490u32 as i32;
pub const HR_INSUFFICIENT_BUFFER: i32 = 0x8007_007au32 as i32;
pub const HR_HANDLE_EOF: i32 = 0x8007_0026u32 as i32;
pub const HR_MOD_NOT_FOUND: i32 = 0x8007_007eu32 as i32;

pub const IMAGE_OPCODE_MASK: u64 = 0x20_0000_041e;
pub const OPCODE_IMAGE_ID: u8 = 0;
pub const OPCODE_DBGID_RSDS: u8 = 0x24;
pub const OPCODE_SYSTEM_PATHS: u8 = 0x21;
pub const OPCODE_UNMOUNTED_VOLUME: u8 = 0x22;
pub const OPCODE_VOLUME_MAPPING: u8 = 0x23;
pub const OPCODE_FIXED_EVENT: u8 = 0x0c;
pub const OPCODE_STRING_EVENT: u8 = 0x0d;
pub const DEVICE_MAP_BUFFER: usize = 0x10000;

const LIVE_MERGER_CPP: &str = "D:\\a\\_work\\1\\s\\shared\\PixLiveMerge\\dll\\LiveMerger.cpp";
const LIVE_IMAGE_MERGER_CPP: &str = "D:\\a\\_work\\1\\s\\shared\\PixLiveMerge\\dll\\LiveImageMerger.cpp";
const PE_FILE_CPP: &str = "D:\\a\\_work\\1\\s\\shared\\PixLiveMerge\\dll\\PEFile.cpp";

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *mut c_void, disp: u32, flags: u32, tmpl: isize) -> isize;
    fn ReadFile(h: isize, buf: *mut u8, n: u32, read: *mut u32, ov: *mut c_void) -> i32;
    fn SetFilePointer(h: isize, lo: i32, hi: *mut i32, method: u32) -> u32;
    fn CloseHandle(h: isize) -> i32;
    fn ExpandEnvironmentStringsW(src: *const u16, dst: *mut u16, n: u32) -> u32;
    fn GetWindowsDirectoryW(buf: *mut u16, n: u32) -> u32;
    fn GetSystemDirectoryW(buf: *mut u16, n: u32) -> u32;
    fn GetModuleHandleExW(flags: u32, name: *const u16, out: *mut isize) -> i32;
    fn GetCurrentProcess() -> isize;
    fn K32GetModuleInformation(process: isize, module: isize, info: *mut u64, cb: u32) -> i32;
    fn FreeLibrary(module: isize) -> i32;
}

pub fn fail(hr: i32, expr: &str, file: &str, line: u32) {
    println!("Failed ({:#x}) {} {} ({})", hr as u32, expr, file, line);
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
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
#[derive(Clone, Copy, Default, Debug)]
pub struct EventHeader {
    pub size: u16,
    pub header_type: u16,
    pub flags: u16,
    pub event_property: u16,
    pub thread_id: u32,
    pub process_id: u32,
    pub time_stamp: i64,
    pub provider_id: Guid,
    pub descriptor: EventDescriptor,
    pub kernel_time: u32,
    pub user_time: u32,
    pub activity_id: Guid,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EventRecord {
    pub header: EventHeader,
    pub buffer_context: u32,
    pub extended_data_count: u16,
    pub user_data_length: u16,
    pub extended_data: *mut c_void,
    pub user_data: *mut c_void,
    pub user_context: *mut c_void,
}

impl EventRecord {
    pub fn zeroed() -> EventRecord {
        EventRecord {
            header: EventHeader::default(),
            buffer_context: 0,
            extended_data_count: 0,
            user_data_length: 0,
            extended_data: core::ptr::null_mut(),
            user_data: core::ptr::null_mut(),
            user_context: core::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LiveMergerConfig {
    pub include_kernel: u8,
    pub all_processes: u8,
    pub pad: [u8; 2],
    pub process_id: u32,
}

#[repr(C)]
pub struct ICallbackVtbl {
    pub query_interface: unsafe extern "system" fn(*mut ICallback, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut ICallback) -> u32,
    pub release: unsafe extern "system" fn(*mut ICallback) -> u32,
    pub on_event: unsafe extern "system" fn(*mut ICallback, *const EventRecord) -> i32,
    pub on_image_lookup_failed: unsafe extern "system" fn(*mut ICallback, u64, u64, u32, *const u16, i32) -> i32,
}

#[repr(C)]
pub struct ICallback {
    pub vtbl: *const ICallbackVtbl,
}

unsafe fn cb_add_ref(c: *mut ICallback) {
    ((*(*c).vtbl).add_ref)(c);
}

unsafe fn cb_release(c: *mut ICallback) {
    ((*(*c).vtbl).release)(c);
}

unsafe fn cb_event(c: *mut ICallback, r: *const EventRecord) -> i32 {
    ((*(*c).vtbl).on_event)(c, r)
}

unsafe fn wcs(p: *const u16) -> Vec<u16> {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    core::slice::from_raw_parts(p, n).to_vec()
}

fn nul(s: &[u16]) -> Vec<u16> {
    s.iter().copied().chain(std::iter::once(0)).collect()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn fold(c: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&c) {
        c + 32
    } else {
        c
    }
}

pub fn wcsnicmp_prefix(a: &[u16], p: &[u16]) -> bool {
    a.len() >= p.len() && a.iter().zip(p).all(|(x, y)| fold(*x) == fold(*y))
}

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub trait DataSource {
    fn read(&mut self, buf: &mut [u8]) -> i32;
    fn set_pos(&mut self, pos: u32) -> i32;
    fn pos(&self) -> u32;
    fn file_backed(&self) -> bool;
}

pub struct MemorySource {
    pub module: isize,
    pub base: usize,
    pub pos: u32,
}

impl Drop for MemorySource {
    fn drop(&mut self) {
        if self.module != 0 {
            unsafe { FreeLibrary(self.module) };
        }
    }
}

impl DataSource for MemorySource {
    fn read(&mut self, buf: &mut [u8]) -> i32 {
        unsafe { core::ptr::copy_nonoverlapping((self.base.wrapping_add(self.pos as usize)) as *const u8, buf.as_mut_ptr(), buf.len()) };
        self.pos = self.pos.wrapping_add(buf.len() as u32);
        S_OK
    }
    fn set_pos(&mut self, pos: u32) -> i32 {
        self.pos = pos;
        S_OK
    }
    fn pos(&self) -> u32 {
        self.pos
    }
    fn file_backed(&self) -> bool {
        false
    }
}

pub struct FileSource {
    pub handle: isize,
    pub pos: u32,
}

impl Drop for FileSource {
    fn drop(&mut self) {
        if self.handle != -1 {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

impl DataSource for FileSource {
    fn read(&mut self, buf: &mut [u8]) -> i32 {
        if buf.is_empty() {
            return S_OK;
        }
        let mut got = 0u32;
        if unsafe { ReadFile(self.handle, buf.as_mut_ptr(), buf.len() as u32, &mut got, core::ptr::null_mut()) } == 0 {
            return last_error_hr();
        }
        self.pos = self.pos.wrapping_add(got);
        if got != buf.len() as u32 && got == 0 {
            return HR_HANDLE_EOF;
        }
        S_OK
    }
    fn set_pos(&mut self, pos: u32) -> i32 {
        let r = unsafe { SetFilePointer(self.handle, pos as i32, core::ptr::null_mut(), 0) };
        if r == u32::MAX {
            let hr = last_error_hr();
            if hr < 0 {
                fail(hr, "HRESULT_FROM_WIN32(GetLastError())", PE_FILE_CPP, 0x6a);
                return hr;
            }
        }
        if r != pos {
            fail(E_UNEXPECTED, "E_UNEXPECTED", PE_FILE_CPP, 0x6f);
            return E_UNEXPECTED;
        }
        self.pos = pos;
        S_OK
    }
    fn pos(&self) -> u32 {
        self.pos
    }
    fn file_backed(&self) -> bool {
        true
    }
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct DebugInfo {
    pub guid: Guid,
    pub age: u32,
    pub pdb: Vec<u8>,
}

pub struct PeFile {
    pub src: Box<dyn DataSource>,
    pub dos: [u8; 0x40],
    pub file_header: [u8; 0x14],
    pub optional: [u8; 0xf0],
    pub sections: Vec<u8>,
    pub section_count: u32,
}

impl PeFile {
    pub fn new(src: Box<dyn DataSource>) -> PeFile {
        PeFile { src, dos: [0; 0x40], file_header: [0; 0x14], optional: [0; 0xf0], sections: Vec::new(), section_count: 0 }
    }

    pub fn parse(&mut self) -> i32 {
        let hr = self.src.set_pos(0);
        if hr < 0 {
            fail(hr, "m_pDataSource->SetCurrentReadPosition(0)", PE_FILE_CPP, 0x150);
            return hr;
        }
        let hr = self.src.read(&mut self.dos);
        if hr < 0 {
            return hr;
        }
        let hr = self.src.set_pos(rd32(&self.dos, 0x3c));
        if hr < 0 {
            return hr;
        }
        let mut sig = [0u8; 4];
        let hr = self.src.read(&mut sig);
        if hr < 0 {
            return hr;
        }
        if u32::from_le_bytes(sig) != 0x4550 {
            fail(E_INVALIDARG, "E_INVALIDARG", PE_FILE_CPP, 0x16d);
            return E_INVALIDARG;
        }
        let hr = self.src.read(&mut self.file_header);
        if hr < 0 {
            return hr;
        }
        self.section_count = rd16(&self.file_header, 2) as u32;
        if rd16(&self.file_header, 0x10) == 0xf0 {
            let hr = self.src.read(&mut self.optional);
            if hr < 0 {
                return hr;
            }
        }
        self.sections = vec![0u8; self.section_count as usize * 40];
        let hr = self.src.read(&mut self.sections);
        if hr < 0 {
            return hr;
        }
        S_OK
    }

    pub fn time_date_stamp(&self) -> u32 {
        rd32(&self.file_header, 4)
    }

    pub fn size_of_image(&self) -> u32 {
        rd32(&self.optional, 0x38)
    }

    fn read_debug_directories(&mut self, rva: u32, size: u32, count: u32, out: &mut [u8]) -> i32 {
        if self.src.file_backed() {
            let mut found = None;
            for i in 0..self.section_count as usize {
                let s = &self.sections[i * 40..i * 40 + 40];
                let va = rd32(s, 0xc);
                if va > rva {
                    continue;
                }
                if rd32(s, 0x10).wrapping_add(va) >= size.wrapping_add(rva) {
                    found = Some(rd32(s, 0x14).wrapping_sub(va).wrapping_add(rva));
                    break;
                }
            }
            let Some(off) = found else { return 1 };
            let hr = self.src.set_pos(off);
            if hr < 0 {
                fail(hr, "m_pDataSource->SetCurrentReadPosition(dwDebugDirectoryFileOffset)", PE_FILE_CPP, 0x1b7);
                return hr;
            }
        } else {
            let hr = self.src.set_pos(rva);
            if hr < 0 {
                fail(hr, "m_pDataSource->SetCurrentReadPosition(pDataDirectory->VirtualAddress)", PE_FILE_CPP, 0x1c8);
                return hr;
            }
        }
        let hr = self.src.read(&mut out[..count as usize * 0x1c]);
        if hr < 0 {
            return hr;
        }
        S_OK
    }

    pub fn debug_info(&mut self, out: &mut DebugInfo) -> i32 {
        let (rva, size) = if rd32(&self.optional, 0x6c) > 6 { (rd32(&self.optional, 0xa0), rd32(&self.optional, 0xa4)) } else { (0, 0) };
        let count = size / 0x1c;
        if count.wrapping_sub(1) > 9 {
            return 1;
        }
        let mut dirs = vec![0u8; count as usize * 0x1c];
        if self.read_debug_directories(rva, size, count, &mut dirs) != 0 {
            return 1;
        }
        let mut data: Vec<u8> = Vec::new();
        for i in 0..count as usize {
            let e = &dirs[i * 0x1c..i * 0x1c + 0x1c];
            if rd32(e, 0xc) != 2 {
                continue;
            }
            if self.src.file_backed() {
                let hr = self.src.set_pos(rd32(e, 0x18));
                if hr < 0 {
                    fail(hr, "m_pDataSource->SetCurrentReadPosition(pCurrentDebugDirectory->PointerToRawData)", PE_FILE_CPP, 0x201);
                    return hr;
                }
            } else {
                let hr = self.src.set_pos(rd32(e, 0x14));
                if hr < 0 {
                    fail(hr, "m_pDataSource->SetCurrentReadPosition(pCurrentDebugDirectory->AddressOfRawData)", PE_FILE_CPP, 0x205);
                    return hr;
                }
            }
            let n = rd32(e, 0x10) as usize;
            data.clear();
            data.resize(n, 0);
            if self.src.read(&mut data) != 0 {
                continue;
            }
            if n < 0x1c || rd32(&data, 0) != 0x5344_5352 || data[4..20].iter().all(|b| *b == 0) || data[0x18] == 0 {
                continue;
            }
            let room = n - 0x18;
            let len = data[0x18..].iter().position(|b| *b == 0).unwrap_or(room);
            if len == room || len > 0x104 {
                return E_UNEXPECTED;
            }
            out.guid.copy_from_slice(&data[4..20]);
            out.age = rd32(&data, 0x14);
            out.pdb = data[0x18..0x18 + len].to_vec();
            return S_OK;
        }
        1
    }
}

pub fn split_file_name(path: &[u16]) -> Vec<u16> {
    let mut s = path;
    if s.len() >= 2 && s[1] == b':' as u16 {
        s = &s[2..];
    }
    let rest = match s.iter().rposition(|c| *c == b'\\' as u16 || *c == b'/' as u16) {
        Some(i) => &s[i + 1..],
        None => s,
    };
    let (f, e) = match rest.iter().rposition(|c| *c == b'.' as u16) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, &rest[rest.len()..]),
    };
    if f.len() + 1 > 0x104 || e.len() + 1 > 0x100 {
        std::process::abort();
    }
    let mut o = f.to_vec();
    o.extend_from_slice(e);
    o
}

fn cch_cat(buf: &mut Vec<u16>, src: &[u16], cch: usize) {
    if buf.len() >= cch {
        return;
    }
    let room = cch - 1 - buf.len();
    buf.extend(src.iter().take(room).copied());
}

fn cch_printf(parts: &[&[u16]], cch: usize) -> Vec<u16> {
    let mut o: Vec<u16> = parts.iter().flat_map(|p| p.iter().copied()).collect();
    o.truncate(cch - 1);
    o
}

fn windows_dir() -> Vec<u16> {
    let mut b = [0u16; 0x105];
    unsafe { GetWindowsDirectoryW(b.as_mut_ptr(), 0x104) };
    b.iter().take_while(|c| **c != 0).copied().collect()
}

fn system_dir() -> Vec<u16> {
    let mut b = [0u16; 0x105];
    unsafe { GetSystemDirectoryW(b.as_mut_ptr(), 0x104) };
    b.iter().take_while(|c| **c != 0).copied().collect()
}

pub fn resolve_image_path(path: &[u16]) -> Result<Vec<u16>, i32> {
    let mut buf = [0u16; 0x105];
    let src = nul(path);
    if unsafe { ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), 0x105) } == 0 {
        return Err(last_error_hr());
    }
    let b: Vec<u16> = buf.iter().take_while(|c| **c != 0).copied().collect();
    let bs = b'\\' as u16;
    let tail = |n: usize| path.get(n..).unwrap_or(&[]);
    if b.contains(&(b':' as u16)) || (b.len() >= 2 && b[0] == bs && b[1] == bs) {
        return Ok(b);
    }
    if wcsnicmp_prefix(&b, &wide("\\SystemRoot\\")) {
        let mut o = windows_dir();
        cch_cat(&mut o, tail(11), 0x104);
        return Ok(o);
    }
    if wcsnicmp_prefix(&b, &wide("\\Device\\")) {
        return Ok(cch_printf(&[&wide("\\\\?\\GLOBALROOT"), path], 0x105));
    }
    if wcsnicmp_prefix(path, &wide("\\GLOBAL??\\")) {
        return Ok(cch_printf(&[&[bs], tail(7)], 0x105));
    }
    if wcsnicmp_prefix(&b, &[bs]) {
        let mut o = windows_dir();
        o.truncate(2);
        cch_cat(&mut o, path, 0x104);
        return Ok(o);
    }
    let mut o = system_dir();
    cch_cat(&mut o, &[bs], 0x104);
    cch_cat(&mut o, path, 0x104);
    Ok(o)
}

pub fn open_loaded_module(path: &[u16], base: u64, size: u64) -> Result<PeFile, i32> {
    let short = match path.iter().rposition(|c| *c == b'\\' as u16) {
        Some(i) => &path[i + 1..],
        None => path,
    };
    let name = nul(short);
    let mut h: isize = 0;
    if unsafe { GetModuleHandleExW(0, name.as_ptr(), &mut h) } == 0 {
        let hr = last_error_hr();
        if hr < 0 {
            fail(hr, "HRESULT_FROM_WIN32(GetLastError())", PE_FILE_CPP, 0xc5);
            return Err(hr);
        }
    }
    let release = |h: isize| {
        if h != 0 {
            unsafe { FreeLibrary(h) };
        }
    };
    let mut info = [0u64; 3];
    if unsafe { K32GetModuleInformation(GetCurrentProcess(), h, info.as_mut_ptr(), 0x18) } == 0 {
        let hr = last_error_hr();
        if hr < 0 {
            fail(hr, "HRESULT_FROM_WIN32(GetLastError())", PE_FILE_CPP, 0xd1);
            release(h);
            return Err(hr);
        }
    }
    if info[0] != base || (info[1] as u32) as u64 != size {
        release(h);
        return Err(HR_MOD_NOT_FOUND);
    }
    let mut pe = PeFile::new(Box::new(MemorySource { module: h, base: info[0] as usize, pos: 0 }));
    let hr = pe.parse();
    if hr < 0 {
        fail(hr, "tmpPeFile->ReadImageHeaders()", PE_FILE_CPP, 0xe9);
        return Err(hr);
    }
    Ok(pe)
}

pub fn open_image_file(path: &[u16]) -> Result<PeFile, i32> {
    let target = resolve_image_path(path)?;
    let n = nul(&target);
    let h = unsafe { CreateFileW(n.as_ptr(), 0x8000_0000, 1, core::ptr::null_mut(), 3, 0x80, 0) };
    if h == -1 {
        return Err(last_error_hr());
    }
    let mut pe = PeFile::new(Box::new(FileSource { handle: h, pos: 0 }));
    let hr = pe.parse();
    if hr < 0 {
        return Err(hr);
    }
    Ok(pe)
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct ModuleInfo {
    pub image_size: u64,
    pub time_date_stamp: u32,
    pub debug: DebugInfo,
    pub pe_file_name: Vec<u16>,
}

#[repr(C)]
struct VbiOut {
    guid: Guid,
    age: u32,
    pad: u32,
    pdb: AString,
}

unsafe fn vbi_lookup(name: *const u16, out: &mut DebugInfo) -> i32 {
    let mut o = VbiOut { guid: [0; 16], age: 0, pad: 0, pdb: AString::new() };
    let hr = crate::PdbInfoFromVBI(name, &mut o as *mut VbiOut as *mut u8);
    if hr >= 0 {
        out.guid = o.guid;
        out.age = o.age;
        out.pdb = o.pdb.as_slice().iter().take_while(|b| **b != 0).copied().collect();
    }
    o.pdb.drop_in_place();
    hr
}

pub struct ImageMerger {
    pub config: LiveMergerConfig,
    pub callback: *mut ICallback,
    pub cache: HashMap<Vec<u16>, ModuleInfo>,
    pub cache_hits: u32,
    pub memory_lookups: u32,
    pub file_lookups: u32,
}

impl Drop for ImageMerger {
    fn drop(&mut self) {
        if !self.callback.is_null() {
            unsafe { cb_release(self.callback) };
        }
    }
}

impl ImageMerger {
    unsafe fn new(config: LiveMergerConfig, callback: *mut ICallback) -> ImageMerger {
        cb_add_ref(callback);
        ImageMerger { config, callback, cache: HashMap::new(), cache_hits: 0, memory_lookups: 0, file_lookups: 0 }
    }

    unsafe fn lookup(&mut self, info: *const u8, name_ptr: *const u16, name: &[u16]) -> Result<ModuleInfo, i32> {
        let file = split_file_name(name);
        if file.len() >= 0x104 {
            fail(HR_INSUFFICIENT_BUFFER, "StringCchPrintf( szfileName, ARRAYSIZE(szfileName), L\"%s%s\", szFullFileName, szExt)", LIVE_IMAGE_MERGER_CPP, 0x69);
            return Err(HR_INSUFFICIENT_BUFFER);
        }
        let base = (info as *const u64).read_unaligned();
        let event_size = (info.add(8) as *const u64).read_unaligned();
        let pid = (info.add(0x10) as *const u32).read_unaligned();
        let mut tds = (info.add(0x18) as *const u32).read_unaligned();
        let mut size32 = event_size as u32;
        let mut mi = ModuleInfo::default();
        let mut pe: Option<PeFile> = None;
        let mut resolved = false;
        if self.config.include_kernel != 0 && pid == 0 && vbi_lookup(name_ptr, &mut mi.debug) >= 0 {
            resolved = true;
        }
        if !resolved {
            if pid == GetCurrentProcessId() {
                if let Ok(mut p) = open_loaded_module(name, base, event_size) {
                    let mut d = DebugInfo::default();
                    if p.debug_info(&mut d) == 0 {
                        mi.debug = d;
                        self.memory_lookups += 1;
                        pe = Some(p);
                        resolved = true;
                    }
                }
            }
            if !resolved {
                let mut p = open_image_file(name)?;
                let mut d = DebugInfo::default();
                match p.debug_info(&mut d) {
                    0 => {}
                    1 => return Err(HR_NOT_FOUND),
                    hr => return Err(hr),
                }
                mi.debug = d;
                self.file_lookups += 1;
                pe = Some(p);
            }
        }
        if let Some(p) = &pe {
            tds = p.time_date_stamp();
            if event_size == 0 {
                size32 = p.size_of_image();
            }
        }
        mi.image_size = size32 as u64;
        mi.time_date_stamp = tds;
        mi.pe_file_name = file;
        Ok(mi)
    }

    unsafe fn send(&self, src: &EventRecord, opcode: u8, payload: *const u8, size: u32) -> i32 {
        let mut r = EventRecord::zeroed();
        r.header.size = (size as u16).wrapping_add(0x70);
        r.user_data_length = size as u16;
        r.header.time_stamp = src.header.time_stamp;
        r.header.kernel_time = src.header.kernel_time;
        r.header.user_time = src.header.user_time;
        r.header.process_id = src.header.process_id;
        r.header.thread_id = src.header.thread_id;
        r.user_data = payload as *mut c_void;
        r.header.provider_id = IMAGE_ID_GUID;
        r.header.descriptor.level = 0;
        r.header.descriptor.version = 2;
        r.header.descriptor.opcode = opcode;
        cb_event(self.callback, &r)
    }

    pub unsafe fn handle(&mut self, rec: *const EventRecord) -> i32 {
        let r = &*rec;
        if r.header.descriptor.version.wrapping_sub(2) > 1 {
            fail(E_UNEXPECTED, "E_UNEXPECTED", LIVE_IMAGE_MERGER_CPP, 0x31);
            fail(E_UNEXPECTED, "ParseImageEvent(EventRecord, &pImageLoadInfo)", LIVE_IMAGE_MERGER_CPP, 0xd5);
            return E_UNEXPECTED;
        }
        let info = r.user_data as *const u8;
        let pid = (info.add(0x10) as *const u32).read_unaligned();
        if pid == 0 {
            if self.config.include_kernel == 0 {
                return S_OK;
            }
        } else if pid != self.config.process_id && self.config.all_processes == 0 {
            return S_OK;
        }
        let name_ptr = info.add(0x38) as *const u16;
        let key = wcs(name_ptr);
        let cached = if crate::image_name_cache_enabled() { self.cache.get(&key).cloned() } else { None };
        let mi = if let Some(m) = cached {
            self.cache_hits += 1;
            m
        } else {
            match self.lookup(info, name_ptr, &key) {
                Ok(m) => {
                    self.cache.entry(key).or_insert_with(|| m.clone());
                    m
                }
                Err(hr) => {
                    let base = (info as *const u64).read_unaligned();
                    let size = (info.add(8) as *const u64).read_unaligned();
                    let h = ((*(*self.callback).vtbl).on_image_lookup_failed)(self.callback, base, size, pid, name_ptr, hr);
                    if h < 0 {
                        fail(h, "_pDecoder->OnImageLookupFailed(pImageLoadInfo->ImageBase64, pImageLoadInfo->ImageSize64, pImageLoadInfo->ProcessId, pImageLoadInfo->FileName, hr)", LIVE_IMAGE_MERGER_CPP, 0xfa);
                        return h;
                    }
                    return S_OK;
                }
            }
        };
        let base = (info as *const u64).read_unaligned();
        let mut image = vec![0u8; 0x18 + 0x208];
        image[0..8].copy_from_slice(&base.to_le_bytes());
        image[8..16].copy_from_slice(&mi.image_size.to_le_bytes());
        image[0x10..0x14].copy_from_slice(&pid.to_le_bytes());
        image[0x14..0x18].copy_from_slice(&mi.time_date_stamp.to_le_bytes());
        let ilen = mi.pe_file_name.len().min(0x103);
        for (i, c) in mi.pe_file_name[..ilen].iter().enumerate() {
            image[0x18 + i * 2..0x1a + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        if mi.pe_file_name.len() >= 0x104 {
            fail(HR_INSUFFICIENT_BUFFER, "StringCchCopy(imageInfo.OriginalFileName, MAX_PATH, moduleInfo.peFileName.c_str())", LIVE_IMAGE_MERGER_CPP, 0x10d);
            return HR_INSUFFICIENT_BUFFER;
        }
        let mut dbg = vec![0u8; 0x128];
        dbg[0..8].copy_from_slice(&base.to_le_bytes());
        dbg[8..0xc].copy_from_slice(&pid.to_le_bytes());
        dbg[0xc..0x1c].copy_from_slice(&mi.debug.guid);
        dbg[0x1c..0x20].copy_from_slice(&mi.debug.age.to_le_bytes());
        let plen = mi.debug.pdb.len().min(0x103);
        dbg[0x20..0x20 + plen].copy_from_slice(&mi.debug.pdb[..plen]);
        if mi.debug.pdb.len() >= 0x104 {
            fail(HR_INSUFFICIENT_BUFFER, "StringCchCopyA(dbgInfo.szPdb, MAX_PATH, moduleInfo.PdbInfo.m_PdbName.c_str())", LIVE_IMAGE_MERGER_CPP, 0x113);
            return HR_INSUFFICIENT_BUFFER;
        }
        let hr = self.send(r, OPCODE_IMAGE_ID, image.as_ptr(), ilen as u32 * 2 + 0x1a);
        if hr < 0 {
            fail(hr, "SendImageIdEvent(EventRecord->EventHeader, reinterpret_cast<IMAGEID_INFORMATION64*>(&imageInfo))", LIVE_IMAGE_MERGER_CPP, 0x115);
            return hr;
        }
        let hr = self.send(r, OPCODE_DBGID_RSDS, dbg.as_ptr(), plen as u32 + 0x21);
        if hr < 0 {
            fail(hr, "SendDbgIdEvent(EventRecord->EventHeader, reinterpret_cast<DBGID_RSDS_INFORMATION64*>(&dbgInfo))", LIVE_IMAGE_MERGER_CPP, 0x116);
            return hr;
        }
        S_OK
    }
}

pub struct DeviceMapper {
    pub callback: *mut ICallback,
    pub buffer: Vec<u64>,
}

impl Drop for DeviceMapper {
    fn drop(&mut self) {
        if !self.callback.is_null() {
            unsafe { cb_release(self.callback) };
        }
    }
}

impl DeviceMapper {
    unsafe fn new(callback: *mut ICallback) -> DeviceMapper {
        cb_add_ref(callback);
        DeviceMapper { callback, buffer: vec![0u64; DEVICE_MAP_BUFFER / 8] }
    }

    unsafe fn emit(&mut self, time_stamp: i64, opcode: u8, strings: &[&[u16]]) {
        let base = self.buffer.as_mut_ptr() as *mut u8;
        core::ptr::write_bytes(base, 0, 0x70);
        let mut avail = (DEVICE_MAP_BUFFER - 0x70) as u32;
        let mut at = base.add(0x70) as *mut u16;
        for s in strings {
            let cch = (avail / 2) as usize;
            let copied = s.len().min(cch.saturating_sub(1));
            core::ptr::copy_nonoverlapping(s.as_ptr(), at, copied);
            *at.add(copied) = 0;
            at = at.add(copied + 1);
            avail -= (copied as u32 + 1) * 2;
        }
        let rec = &mut *(base as *mut EventRecord);
        rec.user_data = base.add(0x70) as *mut c_void;
        rec.header.time_stamp = time_stamp;
        rec.header.provider_id = VOLUME_MAP_GUID;
        rec.header.descriptor.opcode = opcode;
        let size = (DEVICE_MAP_BUFFER as u32 - avail) as u16;
        rec.header.size = size;
        rec.user_data_length = size.wrapping_sub(0x70);
        cb_event(self.callback, rec);
    }

    pub unsafe fn send(&mut self, time_stamp: i64) -> i32 {
        let map = match NtPathMap::build() {
            Ok(m) => m,
            Err(hr) => return hr,
        };
        self.emit(time_stamp, OPCODE_SYSTEM_PATHS, &[&map.sysdir, &map.windir]);
        for u in &map.unmounted {
            self.emit(time_stamp, OPCODE_UNMOUNTED_VOLUME, &[u]);
        }
        for (p, r) in &map.entries {
            self.emit(time_stamp, OPCODE_VOLUME_MAPPING, &[p, r]);
        }
        S_OK
    }
}

#[repr(C)]
pub struct LiveMergerVtbl {
    pub query_interface: unsafe extern "system" fn(*mut LiveMerger, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut LiveMerger) -> u32,
    pub release: unsafe extern "system" fn(*mut LiveMerger) -> u32,
    pub process_event: unsafe extern "system" fn(*mut LiveMerger, *const EventRecord) -> i32,
    pub send_fixed_event: unsafe extern "system" fn(*mut LiveMerger, *const EventHeader, *const c_void) -> i32,
    pub get_stats: unsafe extern "system" fn(*mut LiveMerger, *mut u32) -> i32,
    pub send_string_event: unsafe extern "system" fn(*mut LiveMerger, *const EventHeader, *const u16) -> i32,
}

#[repr(C)]
pub struct LiveMerger {
    pub vtbl: *const LiveMergerVtbl,
    pub refs: AtomicU32,
    pub callback: *mut ICallback,
    pub image: Option<Box<ImageMerger>>,
    pub device: Option<Box<DeviceMapper>>,
    pub device_map_sent: bool,
}

impl Drop for LiveMerger {
    fn drop(&mut self) {
        self.device = None;
        self.image = None;
        if !self.callback.is_null() {
            unsafe { cb_release(self.callback) };
        }
    }
}

pub static LIVE_MERGER_VTBL: LiveMergerVtbl = LiveMergerVtbl {
    query_interface: lm_query_interface,
    add_ref: lm_add_ref,
    release: lm_release,
    process_event: lm_process_event,
    send_fixed_event: lm_send_fixed_event,
    get_stats: lm_get_stats,
    send_string_event: lm_send_string_event,
};

pub unsafe extern "system" fn lm_query_interface(this: *mut LiveMerger, riid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    *ppv = core::ptr::null_mut();
    if *riid == IID_IUNKNOWN || *riid == IID_ILIVEMERGER {
        *ppv = this as *mut c_void;
        lm_add_ref(this);
        return S_OK;
    }
    E_NOINTERFACE
}

pub unsafe extern "system" fn lm_add_ref(this: *mut LiveMerger) -> u32 {
    (*this).refs.fetch_add(1, Ordering::SeqCst) + 1
}

pub unsafe extern "system" fn lm_release(this: *mut LiveMerger) -> u32 {
    let n = (*this).refs.fetch_sub(1, Ordering::SeqCst) - 1;
    if n == 0 {
        drop(Box::from_raw(this));
    }
    n
}

pub unsafe extern "system" fn lm_process_event(this: *mut LiveMerger, rec: *const EventRecord) -> i32 {
    let m = &mut *this;
    let h = &(*rec).header;
    if h.provider_id == IMAGE_LOAD_GUID {
        let op = h.descriptor.opcode;
        if op <= 0x25 && (IMAGE_OPCODE_MASK >> op) & 1 == 1 {
            let hr = m.image.as_mut().unwrap().handle(rec);
            if hr < 0 {
                fail(hr, "_imageMerger->HandleLiveImageEvent(EventRecord)", LIVE_MERGER_CPP, 0x24);
                return hr;
            }
        }
        return S_OK;
    }
    if !m.device_map_sent && h.provider_id == FILE_IO_GUID {
        m.device.as_mut().unwrap().send(h.time_stamp);
        m.device_map_sent = true;
    }
    S_OK
}

unsafe fn send_custom(m: &LiveMerger, hdr: *const EventHeader, data: *const c_void, size: u16, opcode: u8) -> i32 {
    if hdr.is_null() {
        fail(E_POINTER, "E_POINTER", LIVE_MERGER_CPP, 0x4b);
        return E_POINTER;
    }
    let h = &*hdr;
    let mut r = EventRecord::zeroed();
    r.header.size = 0x90;
    r.user_data = data as *mut c_void;
    r.header.time_stamp = h.time_stamp;
    r.header.process_id = h.process_id;
    r.header.thread_id = h.thread_id;
    r.header.kernel_time = h.kernel_time;
    r.header.user_time = h.user_time;
    r.header.descriptor.opcode = opcode;
    r.user_data_length = size;
    r.header.provider_id = MERGER_EVENT_GUID;
    cb_event(m.callback, &r)
}

pub unsafe extern "system" fn lm_send_fixed_event(this: *mut LiveMerger, hdr: *const EventHeader, data: *const c_void) -> i32 {
    send_custom(&*this, hdr, data, 0x20, OPCODE_FIXED_EVENT)
}

pub unsafe extern "system" fn lm_send_string_event(this: *mut LiveMerger, hdr: *const EventHeader, s: *const u16) -> i32 {
    if s.is_null() {
        fail(E_POINTER, "E_POINTER", LIVE_MERGER_CPP, 0x39);
        return E_POINTER;
    }
    let n = (wcs(s).len() as u16).wrapping_add(1);
    send_custom(&*this, hdr, s as *const c_void, n.wrapping_mul(2), OPCODE_STRING_EVENT)
}

pub unsafe extern "system" fn lm_get_stats(this: *mut LiveMerger, out: *mut u32) -> i32 {
    if out.is_null() {
        fail(E_POINTER, "E_POINTER", LIVE_MERGER_CPP, 0x42);
        return E_POINTER;
    }
    let im = (*this).image.as_ref().unwrap();
    *out = im.cache_hits;
    *out.add(1) = im.memory_lookups;
    *out.add(2) = im.file_lookups;
    S_OK
}

unsafe fn make_and_initialize(config: &LiveMergerConfig, callback: *mut ICallback) -> Result<*mut LiveMerger, i32> {
    let obj = Box::into_raw(Box::new(LiveMerger {
        vtbl: &LIVE_MERGER_VTBL,
        refs: AtomicU32::new(1),
        callback: core::ptr::null_mut(),
        image: None,
        device: None,
        device_map_sent: false,
    }));
    if callback.is_null() {
        fail(E_POINTER, "E_POINTER", LIVE_MERGER_CPP, 0xf);
        lm_release(obj);
        return Err(E_POINTER);
    }
    (*obj).callback = callback;
    (*obj).image = Some(Box::new(ImageMerger::new(*config, callback)));
    (*obj).device = Some(Box::new(DeviceMapper::new(callback)));
    Ok(obj)
}

#[no_mangle]
pub unsafe extern "system" fn CreateLiveMerger(callback: *mut ICallback, config: *const LiveMergerConfig, riid: *const Guid, ppv: *mut *mut c_void) -> i32 {
    if ppv.is_null() || config.is_null() {
        fail(E_POINTER, "E_POINTER", LIVE_MERGER_CPP, 0x6a);
        return E_POINTER;
    }
    if (*config).all_processes == 0 && (*config).process_id == u32::MAX {
        fail(E_INVALIDARG, "E_INVALIDARG", LIVE_MERGER_CPP, 0x71);
        return E_INVALIDARG;
    }
    let obj = match make_and_initialize(&*config, callback) {
        Ok(o) => o,
        Err(hr) => {
            fail(hr, "Microsoft::WRL::MakeAndInitialize<LiveMergerImpl>(&impl, *config, pCallback)", LIVE_MERGER_CPP, 0x75);
            return hr;
        }
    };
    let hr = lm_query_interface(obj, riid, ppv);
    if hr < 0 {
        fail(hr, "impl->QueryInterface(riid, ppvObject)", LIVE_MERGER_CPP, 0x76);
    }
    lm_release(obj);
    if hr < 0 {
        hr
    } else {
        S_OK
    }
}
