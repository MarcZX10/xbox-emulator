use core::ffi::c_void;
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use crate::wstr::{from_ptr, put_wide};

pub const ACTIVE_COMPUTER_NAME: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\ComputerName\\ActiveComputerName";
pub const COMPUTER_NAME: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\ComputerName\\ComputerName";
pub const TCPIP_PARAMETERS: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Tcpip\\Parameters";
pub const ERROR_MORE_DATA: u32 = 234;
pub const ERROR_INVALID_PARAMETER: u32 = 87;
pub const ERROR_FILE_NOT_FOUND: u32 = 2;
pub const ERROR_NOT_SUPPORTED: u32 = 50;
pub const ERROR_INVALID_HANDLE: u32 = 6;

#[link(name = "kernel32")]
extern "system" {
    fn SetLastError(e: u32);
}

#[derive(Default)]
pub struct Process {
    pub env: BTreeMap<String, (String, String)>,
    pub priority_class: u32,
    pub spawn_requests: Vec<String>,
}

pub fn process() -> MutexGuard<'static, Process> {
    static P: Mutex<Process> = Mutex::new(Process { env: BTreeMap::new(), priority_class: 0x20, spawn_requests: Vec::new() });
    P.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn env(name: &str) -> Option<String> {
    process().env.get(&name.to_uppercase()).map(|v| v.1.clone())
}

pub fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => {
                let name = &after[..j];
                let v = if name.eq_ignore_ascii_case("SystemRoot") || name.eq_ignore_ascii_case("windir") { Some(crate::fs::WINDOWS_DIR.to_string()) } else { env(name) };
                match v {
                    Some(v) if !name.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn hostname(kind: u32) -> Option<String> {
    let host = || crate::registry::get_string(TCPIP_PARAMETERS, "Hostname");
    let domain = || crate::registry::get_string(TCPIP_PARAMETERS, "Domain").unwrap_or_default();
    match kind {
        0 => crate::registry::get_string(ACTIVE_COMPUTER_NAME, "ComputerName").or_else(|| crate::registry::get_string(COMPUTER_NAME, "ComputerName")),
        4 => crate::registry::get_string(COMPUTER_NAME, "ComputerName"),
        1 | 5 => host(),
        2 | 6 => Some(domain()),
        3 | 7 => host().map(|h| {
            let d = domain();
            if d.is_empty() {
                h
            } else {
                format!("{h}.{d}")
            }
        }),
        _ => None,
    }
}

pub unsafe extern "system" fn get_computer_name_ex_w(kind: u32, buf: *mut u16, size: *mut u32) -> i32 {
    if kind > 7 || size.is_null() {
        SetLastError(ERROR_INVALID_PARAMETER);
        return 0;
    }
    let Some(name) = hostname(kind) else {
        SetLastError(ERROR_FILE_NOT_FOUND);
        return 0;
    };
    let w: Vec<u16> = name.encode_utf16().collect();
    if buf.is_null() || (*size as usize) < w.len() + 1 {
        *size = w.len() as u32 + 1;
        SetLastError(ERROR_MORE_DATA);
        return 0;
    }
    put_wide(buf, &w);
    *size = w.len() as u32;
    1
}

pub unsafe extern "system" fn set_environment_variable_w(name: *const u16, value: *const u16) -> i32 {
    let n = from_ptr(name);
    if n.is_empty() || n.contains('=') {
        SetLastError(ERROR_INVALID_PARAMETER);
        return 0;
    }
    let mut p = process();
    if value.is_null() {
        p.env.remove(&n.to_uppercase());
    } else {
        p.env.insert(n.to_uppercase(), (n, from_ptr(value)));
    }
    1
}

fn image_of(app: &str, cmd: &str) -> String {
    if !app.is_empty() {
        return app.to_string();
    }
    let c = cmd.trim_start();
    if let Some(r) = c.strip_prefix('"') {
        return r.split('"').next().unwrap_or("").to_string();
    }
    c.split_whitespace().next().unwrap_or("").to_string()
}

pub unsafe extern "system" fn create_process_w(app: *const u16, cmd: *mut u16, _psa: *const c_void, _tsa: *const c_void, _inherit: i32, _flags: u32, _env: *const c_void, _cwd: *const u16, _si: *const c_void, _pi: *mut c_void) -> i32 {
    let a = from_ptr(app);
    let c = from_ptr(cmd);
    let image = image_of(&a, &c);
    let mut cands = vec![image.clone()];
    if !image.to_lowercase().ends_with(".exe") {
        cands.push(format!("{image}.exe"));
    }
    let found = cands.iter().filter_map(|i| crate::fs::resolve(i)).find(|(_, h)| h.is_file());
    process().spawn_requests.push(if c.is_empty() { a.clone() } else { c.clone() });
    match found {
        None => {
            SetLastError(ERROR_FILE_NOT_FOUND);
            0
        }
        Some((console, _)) => {
            compat_common::record_raw("console-sandbox", "CreateProcessW", &format!("console image {console} exists in the sandbox but a second console process cannot be hosted in-process; returning ERROR_NOT_SUPPORTED"));
            SetLastError(ERROR_NOT_SUPPORTED);
            0
        }
    }
}

pub unsafe extern "system" fn set_priority_class(h: isize, class: u32) -> i32 {
    if h != -1 {
        SetLastError(ERROR_INVALID_HANDLE);
        return 0;
    }
    if ![0x20, 0x40, 0x80, 0x100, 0x4000, 0x8000, 0x10_0000, 0x20_0000].contains(&class) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return 0;
    }
    process().priority_class = class;
    1
}
