#![allow(non_snake_case, clippy::missing_safety_doc)]

#[cfg(test)]
mod tests;

use core::ffi::c_void;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

pub const S_OK: i32 = 0;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
pub const E_ABORT: i32 = 0x8000_4004u32 as i32;
pub const E_FAIL: i32 = 0x8000_4005u32 as i32;
pub const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;
pub const E_ACCESSDENIED: i32 = 0x8007_0005u32 as i32;
pub const E_NOT_FOUND: i32 = 0x8007_0002u32 as i32;
pub const E_WRITE_FAULT: i32 = 0x8007_001Du32 as i32;
pub const E_BUFFER_OVERFLOW: i32 = 0x8007_006Fu32 as i32;
pub const E_MORE_DATA: i32 = 0x8007_00EAu32 as i32;
pub const E_ARITHMETIC_OVERFLOW: i32 = 0x8007_0216u32 as i32;
pub const APPMODEL_E_NO_PACKAGE: i32 = 0x8007_3D54u32 as i32;
pub const MAX_PROCESS_HOLD_MS: u32 = 30000;
pub const STORE_BYTES: usize = 0x208;
pub const XML_HEADER: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-16\"?>";

pub const E_NO_MORE_FILES: i32 = 0x8007_0012u32 as i32;
pub const STRSAFE_E_INSUFFICIENT_BUFFER: i32 = 0x8007_007Au32 as i32;
pub const S_FALSE: i32 = 1;
pub const HOST_HEAP_DUMP: u64 = 1;
pub const HOST_HANG_FILE: u64 = 2;
pub const ENUM_METADATA: usize = 0;
pub const ENUM_FILES: usize = 1;
pub const HANG_EVENT: &str = "AppHangB1";
pub const OS_VERSION_KEY: &str = "SRAOSVersion";

pub const TRANSPORT_UPLOAD: u64 = 0;
pub const TRANSPORT_HEAP_DUMP: u64 = 1;
pub const TRANSPORT_FINISH: u64 = 2;
pub const TRANSPORT_MOUNTED: u64 = 3;

const M: &str = "werxboxgc";

fn trace(f: &str, d: &str) {
    #[cfg(not(test))]
    compat_common::record(M, f, d);
    #[cfg(test)]
    compat_common::record_raw(M, f, d);
}

pub struct Wer {
    pub root: Option<PathBuf>,
    pub transport: Option<u64>,
    pub mounted: bool,
    pub process: usize,
    pub host_flags: u64,
    pub host_file: Vec<u16>,
    pub enumerated: [bool; 2],
    pub title: Option<compat_common::gdk::Title>,
}

pub static WER: Mutex<Wer> = Mutex::new(Wer { root: None, transport: None, mounted: false, process: 0, host_flags: 0, host_file: Vec::new(), enumerated: [false; 2], title: None });

pub fn wer() -> MutexGuard<'static, Wer> {
    WER.lock().unwrap_or_else(|e| e.into_inner())
}

impl Wer {
    pub fn store(&self) -> PathBuf {
        self.root.clone().unwrap_or_else(|| compat_common::gdk::data_root().join("wer")).join("GameOS-crash0")
    }
    pub fn send(&mut self, kind: u64) -> i32 {
        self.transport = Some(kind);
        let dir = self.store();
        if std::fs::create_dir_all(&dir).is_err() {
            return E_FAIL;
        }
        let mut t = 0u64;
        unsafe { GetSystemTimePreciseAsFileTime(&mut t) };
        let line = format!("{{\"time\":{t},\"event\":{kind}}}\n");
        match std::fs::OpenOptions::new().create(true).append(true).open(dir.join("transport.jsonl")).and_then(|mut f| f.write_all(line.as_bytes())) {
            Ok(()) => S_OK,
            Err(_) => E_FAIL,
        }
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemTimePreciseAsFileTime(ft: *mut u64);
    fn GetProcessId(h: *mut c_void) -> u32;
    fn GetCurrentProcessId() -> u32;
    fn WriteFile(h: *mut c_void, buf: *const u8, n: u32, written: *mut u32, ov: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

#[link(name = "bcrypt")]
extern "system" {
    fn BCryptHash(alg: usize, secret: *const u8, secret_len: u32, input: *const u8, input_len: u32, out: *mut u8, out_len: u32) -> i32;
}

const BCRYPT_SHA256_ALG_HANDLE: usize = 0x41;

fn hr_win32(e: u32) -> i32 {
    if e as i32 <= 0 {
        e as i32
    } else {
        ((e & 0xffff) | 0x8007_0000) as i32
    }
}

pub fn publisher_id(publisher: &str) -> String {
    let w: Vec<u8> = publisher.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let mut h = [0u8; 32];
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, core::ptr::null(), 0, w.as_ptr(), w.len() as u32, h.as_mut_ptr(), 32) };
    const ALPHA: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let v = (u64::from_be_bytes(h[..8].try_into().unwrap()) as u128) << 1;
    (0..13).map(|i| ALPHA[((v >> (60 - 5 * i)) & 31) as usize] as char).collect()
}

pub fn package_identity() -> Result<(String, String), i32> {
    let t = wer().title.clone().unwrap_or_else(|| compat_common::gdk::title().clone());
    if t.dir.is_none() || t.identity.is_empty() {
        return Err(E_NOT_FOUND);
    }
    let v = t.version;
    let pid = publisher_id(&t.publisher);
    Ok((format!("{}_{}.{}.{}.{}_x64__{}", t.identity, v[0], v[1], v[2], v[3], pid), format!("{}_{}", t.identity, pid)))
}

#[no_mangle]
pub extern "system" fn XerGetMaxProcessHoldMilliseconds() -> u32 {
    MAX_PROCESS_HOLD_MS
}

#[no_mangle]
pub extern "system" fn XerShouldWerManageRootDirectory() -> u8 {
    0
}

#[no_mangle]
pub extern "system" fn XerEncryptDump(_dump: *mut c_void, kind: u32) -> u8 {
    (kind == 3) as u8
}

#[no_mangle]
pub unsafe extern "system" fn XerGetMachineId(out: *mut [u8; 16], data1: u32) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    let r = core::arch::x86_64::__cpuid_count(0x4000_000a, 0);
    let mut g = [0u8; 16];
    g[0..4].copy_from_slice(&data1.to_le_bytes());
    g[4..6].copy_from_slice(&((r.ecx >> 16) as u16).to_le_bytes());
    g[6..8].copy_from_slice(&(r.ecx as u16).to_le_bytes());
    g[8..12].copy_from_slice(&r.ebx.to_be_bytes());
    g[12..16].copy_from_slice(&r.eax.to_be_bytes());
    *out = g;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XerGetMachineStorePath(buf: *mut u16) -> i32 {
    let p = wer().store().to_string_lossy().into_owned();
    let w: Vec<u16> = p.encode_utf16().chain([0]).collect();
    if w.len() * 2 > STORE_BYTES {
        return E_MORE_DATA;
    }
    if buf.is_null() {
        return E_INVALIDARG;
    }
    core::ptr::copy_nonoverlapping(w.as_ptr(), buf, w.len());
    if w.len() == 1 {
        return E_ACCESSDENIED;
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XerGetPackageIdentity(process: *mut c_void, full_len: *mut u32, full: *mut u16, family_len: *mut u32, family: *mut u16) -> i32 {
    let pid = GetProcessId(process);
    if pid == 0 || pid != GetCurrentProcessId() {
        return APPMODEL_E_NO_PACKAGE;
    }
    let (f, fam) = match package_identity() {
        Ok(x) => x,
        Err(hr) => return hr,
    };
    let fw: Vec<u16> = f.encode_utf16().collect();
    if !full_len.is_null() {
        let old = *full_len;
        let need = fw.len() as u32 + 1;
        *full_len = need;
        if !full.is_null() {
            if old <= need {
                return E_BUFFER_OVERFLOW;
            }
            core::ptr::copy_nonoverlapping(fw.as_ptr(), full, fw.len());
            *full.add(fw.len()) = 0;
            *full.add(need as usize) = 0;
        }
    }
    if !family_len.is_null() {
        let mw: Vec<u16> = fam.encode_utf16().chain([0]).collect();
        let need = mw.len() as u32;
        let old = *family_len;
        *family_len = need;
        if old < need {
            return E_BUFFER_OVERFLOW;
        }
        if family.is_null() {
            return E_INVALIDARG;
        }
        core::ptr::copy_nonoverlapping(mw.as_ptr(), family, mw.len());
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XerDumpNotifyStart(process: *mut c_void) {
    trace("XerDumpNotifyStart", &format!("{:#x}", process as usize));
    match gamehost::wer::dump_notify_start() {
        Ok(Some((flags, file))) => set_host_response(flags, &file),
        Ok(None) => {}
        Err(hr) => trace("XerDumpNotifyStart", &format!("game host {hr:#x}")),
    }
    let mut w = wer();
    w.process = process as usize;
    let store = w.store();
    for d in ["Temp", "ReportQueue", "ReportArchive"] {
        if std::fs::create_dir_all(store.join(d)).is_err() {
            return;
        }
    }
    w.mounted = true;
    w.send(TRANSPORT_MOUNTED);
    let _ = std::fs::write(store.join("WerXvdCreated"), b"1");
}

#[no_mangle]
pub unsafe extern "system" fn XerDumpNotifyComplete() {
    trace("XerDumpNotifyComplete", "");
    gamehost::wer::dump_notify_complete();
    let mut w = wer();
    w.mounted = false;
    w.transport = None;
    w.process = 0;
}

#[no_mangle]
pub extern "system" fn XerTpUpload() -> i32 {
    trace("XerTpUpload", "");
    gamehost::wer::transport_event(TRANSPORT_UPLOAD);
    let mut w = wer();
    if w.transport.is_some() {
        w.send(TRANSPORT_UPLOAD);
    }
    E_NOTIMPL
}

#[no_mangle]
pub extern "system" fn XerTransportEventFinish() {
    let mut w = wer();
    if w.transport == Some(TRANSPORT_HEAP_DUMP) {
        gamehost::wer::transport_event(TRANSPORT_FINISH);
        w.send(TRANSPORT_FINISH);
    }
}

#[no_mangle]
pub extern "system" fn XerProcessHeapDumpRequest() -> i32 {
    let mut w = wer();
    if w.host_flags & HOST_HEAP_DUMP == 0 {
        return E_ABORT;
    }
    gamehost::wer::transport_event(TRANSPORT_HEAP_DUMP);
    w.send(TRANSPORT_HEAP_DUMP);
    S_OK
}

#[no_mangle]
pub extern "system" fn XerTpCompleteUpload() {}

#[no_mangle]
pub extern "system" fn XerTransportEventBegin() {}

#[no_mangle]
pub extern "system" fn XerTransportEventUploadComplete() {}

#[no_mangle]
pub extern "system" fn XerTransportEventUploadProgress() {}

#[no_mangle]
pub extern "system" fn XerTransportEventUploadStart() {}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '>' => o.push_str("&gt;"),
            '<' => o.push_str("&lt;"),
            '\'' => o.push_str("&apos;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

fn indent(depth: usize) -> String {
    let mut s = String::from("\r\n");
    for _ in 0..depth {
        s.push('\t');
    }
    s
}

pub fn element(depth: usize, name: &str, value: Result<String, i32>) -> String {
    match value {
        Ok(v) => format!("{}<{name}>{}</{name}>", indent(depth), escape(&v)),
        Err(hr) => format!("{}<{name} hr=\"0x{:08X}\"/>", indent(depth), hr as u32),
    }
}

unsafe fn write_w(h: *mut c_void, s: &str) -> i32 {
    let b: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    if b.len() > u32::MAX as usize {
        return E_ARITHMETIC_OVERFLOW;
    }
    let mut n = 0u32;
    if WriteFile(h, b.as_ptr(), b.len() as u32, &mut n, core::ptr::null_mut()) == 0 {
        let hr = hr_win32(GetLastError());
        return if hr >= 0 { E_FAIL } else { hr };
    }
    if n as usize != b.len() {
        E_WRITE_FAULT
    } else {
        S_OK
    }
}

pub fn metadata_lines() -> Vec<String> {
    let content = compat_common::gdk::setting("content_id").ok_or(E_NOT_FOUND);
    vec![
        format!("{}<XboxInformation>", indent(0)),
        element(1, "ConsoleID", Ok(compat_common::gdk::system::console_id())),
        element(1, "SandboxID", Ok(compat_common::gdk::system::sandbox_id())),
        element(1, "ContentId", content),
        element(1, "PackageIdentity", package_identity().map(|x| x.0)),
        format!("{}</XboxInformation>", indent(0)),
    ]
}

#[no_mangle]
pub unsafe extern "system" fn XerWriteMetadata(_report: *mut c_void, file: *mut c_void) -> i32 {
    if file as isize == -1 {
        return E_INVALIDARG;
    }
    let hr = write_w(file, XML_HEADER);
    if hr < 0 {
        return hr;
    }
    for l in metadata_lines() {
        let hr = write_w(file, &l);
        if hr < 0 {
            return hr;
        }
    }
    S_OK
}

pub fn set_host_response(flags: u64, file: &str) {
    let mut w = wer();
    w.host_flags = flags;
    w.host_file = file.encode_utf16().collect();
}

pub unsafe fn cch_copy(buf: *mut u16, cch: usize, s: &[u16]) -> i32 {
    if cch.wrapping_sub(1) > 0x7fff_fffe {
        if cch != 0 && !buf.is_null() {
            *buf = 0;
        }
        return E_INVALIDARG;
    }
    if buf.is_null() {
        return E_INVALIDARG;
    }
    let room = cch - 1;
    let n = s.len().min(room);
    core::ptr::copy_nonoverlapping(s.as_ptr(), buf, n);
    *buf.add(n) = 0;
    if s.len() > room {
        STRSAFE_E_INSUFFICIENT_BUFFER
    } else {
        S_OK
    }
}

fn enumerate_once(slot: usize, f: impl FnOnce(&Wer) -> i32) -> i32 {
    let mut w = wer();
    if w.enumerated[slot] {
        return E_NO_MORE_FILES;
    }
    let hr = f(&w);
    w.enumerated[slot] = true;
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XerEnumerateReportFiles(event: *const u16, name: *mut u16, cch: usize) -> i32 {
    enumerate_once(ENUM_FILES, |w| {
        if event.is_null() {
            return E_INVALIDARG;
        }
        let ev = core::slice::from_raw_parts(event, (0..HANG_EVENT.len()).take_while(|i| *event.add(*i) != 0).count());
        let hang: Vec<u16> = HANG_EVENT.encode_utf16().collect();
        if ev == &hang[..] && w.host_flags & HOST_HANG_FILE != 0 {
            cch_copy(name, cch, &w.host_file)
        } else {
            S_FALSE
        }
    })
}

pub fn os_version_text() -> Result<String, i32> {
    let v = compat_common::title_os::title_os_version().ok_or(E_NOT_FOUND)?;
    let q = (v[0] as u64) << 48 | (v[1] as u64) << 32 | (v[2] as u64) << 16 | v[3] as u64;
    Ok(format!("{}.{}.{}.{}", q >> 48, (q >> 32) & 0xffff, (q >> 16) & 0xffff, q & 0xffff))
}

#[no_mangle]
pub unsafe extern "system" fn XerEnumerateReportMetadata(_event: *const u16, key: *mut u16, key_cch: usize, value: *mut u16, value_cch: usize) -> i32 {
    enumerate_once(ENUM_METADATA, |_| {
        let text = match os_version_text() {
            Ok(t) => t,
            Err(hr) => return hr,
        };
        let w: Vec<u16> = text.encode_utf16().collect();
        let hr = cch_copy(value, value_cch, &w);
        if hr < 0 {
            return hr;
        }
        let k: Vec<u16> = OS_VERSION_KEY.encode_utf16().collect();
        let hr = cch_copy(key, key_cch, &k);
        if hr < 0 {
            return hr;
        }
        S_OK
    })
}
