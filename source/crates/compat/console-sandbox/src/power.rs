use core::ffi::c_void;
use std::sync::{Mutex, MutexGuard};

pub const ERROR_SUCCESS: u32 = 0;
pub const ERROR_INVALID_HANDLE: u32 = 6;
pub const ERROR_INVALID_PARAMETER: u32 = 87;
pub const DEVICE_NOTIFY_WINDOW_HANDLE: u32 = 0;
pub const DEVICE_NOTIFY_SERVICE_HANDLE: u32 = 1;
pub const DEVICE_NOTIFY_CALLBACK: u32 = 2;
pub const SERVICE_CONTROL_POWEREVENT: u32 = 0xd;
pub const PBT_APMSUSPEND: u32 = 4;
pub const PBT_APMRESUMESUSPEND: u32 = 7;
pub const PBT_APMRESUMEAUTOMATIC: u32 = 0x12;
pub const PBT_POWERSETTINGCHANGE: u32 = 0x8013;
const HANDLE_BASE: usize = 0x5C50_0000;

pub type DeviceCallback = unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> u32;

#[derive(Clone, Debug)]
pub struct Registration {
    pub setting: [u8; 16],
    pub flags: u32,
    pub recipient: usize,
    pub callback: usize,
    pub context: usize,
    pub live: bool,
}

struct Power {
    regs: Vec<Registration>,
    values: Vec<([u8; 16], Vec<u8>)>,
}

fn power() -> MutexGuard<'static, Power> {
    static P: Mutex<Power> = Mutex::new(Power { regs: Vec::new(), values: Vec::new() });
    P.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn registrations() -> Vec<Registration> {
    power().regs.iter().filter(|r| r.live).cloned().collect()
}

fn setting_blob(guid: &[u8; 16], value: &[u8]) -> Vec<u8> {
    let mut b = guid.to_vec();
    b.extend_from_slice(&(value.len() as u32).to_le_bytes());
    b.extend_from_slice(value);
    b
}

unsafe fn deliver(r: &Registration, kind: u32, data: *mut c_void) {
    match r.flags {
        DEVICE_NOTIFY_SERVICE_HANDLE => {
            crate::host::control_by_handle(r.recipient, SERVICE_CONTROL_POWEREVENT, kind, data);
        }
        DEVICE_NOTIFY_CALLBACK if r.callback != 0 => {
            let f: DeviceCallback = core::mem::transmute(r.callback);
            f(r.context as *mut c_void, kind, data);
        }
        _ => {}
    }
}

pub unsafe fn set_value(guid: [u8; 16], value: &[u8]) {
    let targets: Vec<Registration> = {
        let mut p = power();
        p.values.retain(|(g, _)| g != &guid);
        p.values.push((guid, value.to_vec()));
        p.regs.iter().filter(|r| r.live && r.setting == guid).cloned().collect()
    };
    for r in targets {
        let mut b = setting_blob(&guid, value);
        deliver(&r, PBT_POWERSETTINGCHANGE, b.as_mut_ptr() as *mut c_void);
    }
}

pub unsafe fn broadcast(kind: u32) {
    let targets: Vec<Registration> = power().regs.iter().filter(|r| r.live).cloned().collect();
    let mut seen: Vec<(usize, u32)> = Vec::new();
    for r in targets {
        let key = (if r.flags == DEVICE_NOTIFY_SERVICE_HANDLE { r.recipient } else { r.callback }, r.flags);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        deliver(&r, kind, core::ptr::null_mut());
    }
}

unsafe fn register(guid: *const u8, flags: u32, recipient: *mut c_void, out: *mut usize) -> u32 {
    if guid.is_null() || out.is_null() || recipient.is_null() {
        return ERROR_INVALID_PARAMETER;
    }
    let mut g = [0u8; 16];
    core::ptr::copy_nonoverlapping(guid, g.as_mut_ptr(), 16);
    let (callback, context) = match flags {
        DEVICE_NOTIFY_SERVICE_HANDLE => {
            if !crate::host::is_status_handle(recipient as usize) {
                return ERROR_INVALID_HANDLE;
            }
            (0, 0)
        }
        DEVICE_NOTIFY_CALLBACK => {
            let p = recipient as *const usize;
            let cb = p.read_unaligned();
            if cb == 0 {
                return ERROR_INVALID_PARAMETER;
            }
            (cb, p.add(1).read_unaligned())
        }
        _ => return ERROR_INVALID_PARAMETER,
    };
    let reg = Registration { setting: g, flags, recipient: recipient as usize, callback, context, live: true };
    let current = {
        let mut p = power();
        p.regs.push(reg.clone());
        *out = HANDLE_BASE + p.regs.len() * 8;
        p.values.iter().find(|(x, _)| x == &g).map(|(_, v)| v.clone())
    };
    if let Some(v) = current {
        let mut b = setting_blob(&g, &v);
        deliver(&reg, PBT_POWERSETTINGCHANGE, b.as_mut_ptr() as *mut c_void);
    }
    ERROR_SUCCESS
}

pub unsafe extern "system" fn power_setting_register_notification(guid: *const u8, flags: u32, recipient: *mut c_void, out: *mut usize) -> u32 {
    register(guid, flags, recipient, out)
}

pub unsafe extern "system" fn power_setting_register_notification_ex(guid: *const u8, _scope: u32, flags: u32, recipient: *mut c_void, out: *mut usize) -> u32 {
    register(guid, flags, recipient, out)
}

pub unsafe extern "system" fn power_setting_unregister_notification(h: usize) -> u32 {
    let mut p = power();
    if h < HANDLE_BASE + 8 || (h - HANDLE_BASE) % 8 != 0 {
        return ERROR_INVALID_HANDLE;
    }
    match p.regs.get_mut((h - HANDLE_BASE) / 8 - 1) {
        Some(r) if r.live => {
            r.live = false;
            ERROR_SUCCESS
        }
        _ => ERROR_INVALID_HANDLE,
    }
}
