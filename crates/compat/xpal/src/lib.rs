#![allow(non_snake_case)]

pub mod host;
pub mod nt;
pub mod platform;
pub mod xconfig;

use nt::*;
use platform::{platform, Platform, Regs, PRODUCT_XBOX_KEYSTONE, SECTION_CONSOLE_ID, SECTION_CONSOLE_ID_TAIL, SECTION_FACTORY, SECTION_SERIAL, SYSTEM_TYPE_DURANGO_X_DEVKIT, SYSTEM_TYPE_SCARLETT_MIN};

pub const FEATURE_MAX_22000: i32 = 10;
pub const FEATURE_MAX_22621: i32 = 15;
pub const OS_BUILD_FEATURES_EXTENDED: u32 = 22621;
pub const TITLE_VM_INFO_SIZE: usize = 0x210;
pub const CONSOLE_ID_FALLBACK: &str = "00000000.00000000.00000000.00000000.00";
pub const SERIAL_FALLBACK: &str = "000000000000";

fn system_cx(p: &Platform) -> u16 {
    p.cpuid(0x4000_000D).ecx as u16
}

fn generation(p: &Platform) -> u16 {
    if system_cx(p) >= SYSTEM_TYPE_SCARLETT_MIN {
        9
    } else {
        8
    }
}

fn keystone(p: &Platform) -> bool {
    p.product == PRODUCT_XBOX_KEYSTONE
}

fn cap_bit(r: Regs, bit: u32, high: bool) -> bool {
    let q = if high { r.ecx as u64 | (r.edx as u64) << 32 } else { r.eax as u64 | (r.ebx as u64) << 32 };
    (q >> (bit & 63)) & 1 == 1
}

pub fn capability(p: &Platform, n: i32) -> bool {
    if n > 0x7f {
        return false;
    }
    let b = if n < 0x40 { n } else { n - 0x40 } as u8;
    cap_bit(p.cpuid(0x4000_0007), b as u32, n >= 0x40)
}

pub fn capability_by_vm(p: &Platform, n: i32, vm: i32) -> bool {
    if n > 0x7f {
        return false;
    }
    let leaf = match vm {
        2 => 0x4000_0008,
        3 => 0x4000_0009,
        _ => 0x4000_0007,
    };
    let b = if n < 0x40 { n } else { n - 0x40 } as u32;
    cap_bit(p.cpuid(leaf), b, n >= 0x40)
}

pub fn factory_setting(p: &Platform, n: i32) -> bool {
    if n >= 3 {
        return false;
    }
    let r = p.cpuid(0x4000_000E);
    let q = r.eax as u64 | (r.ebx as u64) << 32;
    if (q >> ((n as u32) & 63)) & 1 == 1 {
        return true;
    }
    n == 0 && capability(p, 0x36)
}

pub fn feature(p: &Platform, f: i32) -> bool {
    let max = if p.os_build >= OS_BUILD_FEATURES_EXTENDED { FEATURE_MAX_22621 } else { FEATURE_MAX_22000 };
    if f < 0 || f > max {
        return false;
    }
    let sys = system_cx(p);
    let gen9 = generation(p) == 9;
    match f {
        0 => p.xbox_hv,
        1 | 8 | 15 => !keystone(p),
        2 => factory_setting(p, 1),
        3 => sys == SYSTEM_TYPE_DURANGO_X_DEVKIT,
        4 => factory_setting(p, 0),
        5 => factory_setting(p, 2),
        6 | 7 => !gen9 && !factory_setting(p, 0) && !keystone(p),
        9 => gen9,
        10 => gen9 && !factory_setting(p, 0) && !p.internal_devkit && !p.external_devkit,
        11 => (sys >= 0x31 && sys != 0xffff) || keystone(p),
        12 => !gen9 && !keystone(p),
        13 => sys != 0x10 && !keystone(p),
        14 => gen9 && !keystone(p),
        _ => false,
    }
}

pub fn host_os_information(p: &Platform) -> u64 {
    let r = p.cpuid(0x4000_000D);
    let b = [r.ebx as u8, (r.eax >> 24) as u8, r.eax as u8, (r.eax >> 8) as u8, r.edx as u8, (r.edx >> 8) as u8, (r.eax >> 16) as u8, (r.ebx >> 8) as u8];
    u64::from_le_bytes(b)
}

pub const CONSOLE_ID_FORMAT: &str = "%02x%02x%02x%02x.%02x%02x%02x%02x.%02x%02x%02x%02x.%02x%02x%02x%02x.%02x";
pub const SERIAL_FORMAT: &str = "%c%c%c%c%c%c%c%c%c%c%c%c";

unsafe fn print_fallback(buf: *mut u16, cch: usize, s: &str) -> bool {
    let w: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
    crt_swprintf_s(buf, cch, "%s", &[w.as_ptr() as u64]) >= 0
}

pub unsafe fn console_id_cch(sec: Option<&[u8]>, buf: *mut u16, cch: usize) -> bool {
    let Some(s) = sec else { return print_fallback(buf, cch, CONSOLE_ID_FALLBACK) };
    let mut a: Vec<u64> = (0..16).rev().map(|i| s[SECTION_CONSOLE_ID + i] as u64).collect();
    a.push(u16::from_le_bytes([s[SECTION_CONSOLE_ID_TAIL], s[SECTION_CONSOLE_ID_TAIL + 1]]) as u64);
    crt_swprintf_s(buf, cch, CONSOLE_ID_FORMAT, &a) >= 0
}

pub unsafe fn serial_cch(sec: Option<&[u8]>, buf: *mut u16, cch: usize) -> bool {
    let Some(s) = sec else { return print_fallback(buf, cch, SERIAL_FALLBACK) };
    let a: Vec<u64> = s[SECTION_SERIAL..SECTION_SERIAL + 12].iter().map(|&c| c as u64).collect();
    crt_swprintf_s(buf, cch, SERIAL_FORMAT, &a) >= 0
}

fn vmctrl_open(p: &Platform) -> bool {
    if !p.vmctrl_present {
        unsafe { SetLastError(ERROR_FILE_NOT_FOUND) };
    }
    p.vmctrl_present
}

fn vm_ioctl(code: u32, input: &[u8], out: &mut [u8]) -> Option<Result<usize, i32>> {
    let p = platform();
    if !vmctrl_open(&p) {
        return None;
    }
    Some(p.host.ioctl(code, input, out))
}

fn vm_bool(code: u32, input: &[u8]) -> bool {
    match vm_ioctl(code, input, &mut []) {
        None => false,
        Some(r) => device_io_result(r, None),
    }
}

fn vm_query(code: u32, n: usize) -> Option<Vec<u8>> {
    let mut out = vec![0u8; n];
    match vm_ioctl(code, &[], &mut out) {
        Some(r) if device_io_result(r, None) => Some(out),
        _ => None,
    }
}

fn nt_vm_ioctl(code: u32, input: &[u8]) -> i32 {
    let p = platform();
    if !p.vmctrl_present {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    }
    match p.host.ioctl(code, input, &mut []) {
        Ok(_) => STATUS_SUCCESS,
        Err(s) => s,
    }
}

#[no_mangle]
pub extern "system" fn XGetConsoleMode() -> u16 {
    (platform().cpuid(0x4000_000D).ecx >> 16) as u16
}

#[no_mangle]
pub extern "system" fn XpalGetConsoleMode() -> u16 {
    XGetConsoleMode()
}

#[no_mangle]
pub extern "system" fn XGetSystemType() -> u16 {
    system_cx(&platform())
}

#[no_mangle]
pub extern "system" fn XpalGetSystemType() -> u16 {
    XGetSystemType()
}

#[no_mangle]
pub extern "system" fn XGetGenerationType() -> u16 {
    generation(&platform())
}

#[no_mangle]
pub extern "system" fn XpalGetGenerationType() -> u16 {
    XGetGenerationType()
}

#[no_mangle]
pub extern "system" fn XpalGetIsGen8() -> bool {
    generation(&platform()) == 8
}

#[no_mangle]
pub extern "system" fn XGetIsScarlettGeneration() -> bool {
    generation(&platform()) == 9
}

#[no_mangle]
pub extern "system" fn XpalGetIsGen9() -> bool {
    XGetIsScarlettGeneration()
}

#[no_mangle]
pub extern "system" fn XGetVmType() -> u8 {
    platform().cpuid(0x4000_0007).ecx as u8
}

#[no_mangle]
pub extern "system" fn XpalGetVmType() -> u8 {
    XGetVmType()
}

#[no_mangle]
pub extern "system" fn XGetServerType() -> u8 {
    let p = platform();
    if factory_setting(&p, 0) {
        (p.cpuid(0x4000_000D).edx >> 16) as u8
    } else {
        0
    }
}

#[no_mangle]
pub extern "system" fn XpalGetServerType() -> u8 {
    XGetServerType()
}

#[no_mangle]
pub extern "system" fn XGetHostOsInformation() -> u64 {
    host_os_information(&platform())
}

#[no_mangle]
pub extern "system" fn XpalGetHostOsInformation() -> u64 {
    XGetHostOsInformation()
}

#[no_mangle]
pub unsafe extern "system" fn XpalGetCapabilities(out: *mut u32) {
    if out.is_null() {
        return;
    }
    let r = platform().cpuid(0x4000_0007);
    *out = r.eax;
    *out.add(1) = r.ebx;
    *out.add(2) = r.ecx;
    *out.add(3) = r.edx;
}

#[no_mangle]
pub extern "system" fn XIsCapabilityEnabled(n: i32) -> bool {
    capability(&platform(), n)
}

#[no_mangle]
pub extern "system" fn XpalIsCapabilityEnabled(n: i32) -> bool {
    XIsCapabilityEnabled(n)
}

#[no_mangle]
pub extern "system" fn XIsCapabilityEnabledByVm(n: i32, vm: i32) -> bool {
    capability_by_vm(&platform(), n, vm)
}

#[no_mangle]
pub extern "system" fn XpalIsCapabilityEnabledByVm(n: i32, vm: i32) -> bool {
    XIsCapabilityEnabledByVm(n, vm)
}

#[no_mangle]
pub extern "system" fn XIsFactorySettingEnabled(n: i32) -> bool {
    factory_setting(&platform(), n)
}

#[no_mangle]
pub extern "system" fn XpalIsFactorySettingEnabled(n: i32) -> bool {
    XIsFactorySettingEnabled(n)
}

#[no_mangle]
pub extern "system" fn XpalIsFeatureEnabled(f: i32) -> bool {
    feature(&platform(), f)
}

#[no_mangle]
pub extern "system" fn XpalIsHyperV() -> bool {
    false
}

#[no_mangle]
pub extern "system" fn XpalIsInternalDevkit() -> bool {
    platform().internal_devkit
}

#[no_mangle]
pub extern "system" fn XpalIsExternalDevkit() -> bool {
    platform().external_devkit
}

#[no_mangle]
pub extern "system" fn XpalIsKeystone() -> bool {
    keystone(&platform())
}

#[no_mangle]
pub extern "system" fn XpalGetConsoleCertificate() -> *const u8 {
    platform().section.as_ref().map(|s| s.as_ptr()).unwrap_or(core::ptr::null())
}

#[no_mangle]
pub extern "system" fn XpalGetFactorySettings() -> *const u8 {
    platform().section.as_ref().map(|s| unsafe { s.as_ptr().add(SECTION_FACTORY) }).unwrap_or(core::ptr::null())
}

#[no_mangle]
pub unsafe extern "system" fn XpalGetConsoleIdCch(buf: *mut u16, cch: usize) -> bool {
    let p = platform();
    console_id_cch(p.section.as_deref(), buf, cch)
}

#[no_mangle]
pub unsafe extern "system" fn XpalGetConsoleSerialNumberCch(buf: *mut u16, cch: usize) -> bool {
    let p = platform();
    serial_cch(p.section.as_deref(), buf, cch)
}

#[no_mangle]
pub unsafe extern "system" fn XpalGetConfiguration(id: u32, buf: *mut u8, size: u32, returned: *mut u32) -> i32 {
    let p = platform();
    if !p.xconfig_present {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    }
    if buf.is_null() && size != 0 {
        return STATUS_INVALID_PARAMETER;
    }
    let out: &mut [u8] = if size == 0 { &mut [] } else { core::slice::from_raw_parts_mut(buf, size as usize) };
    match p.config.ioctl(xconfig::IOCTL_XCONFIG_GET, &id.to_le_bytes(), out) {
        Ok(n) => {
            if !returned.is_null() {
                *returned = n as u32;
            }
            STATUS_SUCCESS
        }
        Err(s) => s,
    }
}

#[no_mangle]
pub unsafe extern "system" fn XpalSetConfiguration(id: u32, buf: *const u8, size: u32) -> i32 {
    let p = platform();
    if !p.xconfig_present {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    }
    if buf.is_null() && size != 0 {
        return STATUS_INVALID_PARAMETER;
    }
    let mut data: Vec<u8> = if size == 0 { Vec::new() } else { core::slice::from_raw_parts(buf, size as usize).to_vec() };
    match p.config.ioctl(xconfig::IOCTL_XCONFIG_SET, &id.to_le_bytes(), &mut data) {
        Ok(_) => STATUS_SUCCESS,
        Err(s) => s,
    }
}

#[no_mangle]
pub extern "system" fn XpalRequestCrashDump(kind: i32, flags: u32, a3: u64, a4: u64, a5: u64) -> i32 {
    if kind >= 9 || flags >= 8 {
        return STATUS_INVALID_PARAMETER;
    }
    let mut b = [0u8; 0x28];
    b[0..8].copy_from_slice(&(flags as u64).to_le_bytes());
    b[8..16].copy_from_slice(&(kind as i64).to_le_bytes());
    b[16..24].copy_from_slice(&a3.to_le_bytes());
    b[24..32].copy_from_slice(&a4.to_le_bytes());
    b[32..40].copy_from_slice(&a5.to_le_bytes());
    nt_vm_ioctl(host::IOCTL_REQUEST_CRASH_DUMP, &b)
}

#[no_mangle]
pub unsafe extern "system" fn XpalSetInformation(class: i32, buf: *const u8, size: u32) -> i32 {
    if class != 0 {
        return STATUS_INVALID_PARAMETER;
    }
    if buf.is_null() && size != 0 {
        return STATUS_INVALID_PARAMETER;
    }
    let data: &[u8] = if size == 0 { &[] } else { core::slice::from_raw_parts(buf, size as usize) };
    nt_vm_ioctl(host::IOCTL_SET_INFORMATION, data)
}

#[no_mangle]
pub unsafe extern "system" fn XpalQueryTitleVmInfo(out: *mut u8) -> bool {
    if out.is_null() {
        SetLastError(ERROR_INVALID_PARAMETER);
        return false;
    }
    core::ptr::write_bytes(out, 0, TITLE_VM_INFO_SIZE);
    let raw = match vm_query(host::IOCTL_QUERY_TITLE_VM_INFO, host::TITLE_VM_INFO_RAW) {
        Some(r) => r,
        None => return false,
    };
    let o = core::slice::from_raw_parts_mut(out, TITLE_VM_INFO_SIZE);
    let running = u32::from_le_bytes(raw[0..4].try_into().unwrap());
    let flags = u32::from_le_bytes(raw[8..12].try_into().unwrap());
    o[0..4].copy_from_slice(&raw[4..8]);
    o[4] = (running != 0) as u8;
    o[5] = (flags & 1) as u8;
    o[6] = ((flags >> 5) & 1) as u8;
    o[8..0x10E].copy_from_slice(&raw[0x10C..0x212]);
    o[0x10E..0x20E].copy_from_slice(&raw[0xC..0x10C]);
    true
}

#[no_mangle]
pub unsafe extern "system" fn XpalWaitForTitleVmTermination(name: *const u16, timeout: u32) -> bool {
    if name.is_null() {
        SetLastError(ERROR_INVALID_PARAMETER);
        return false;
    }
    let mut n = 0usize;
    while *name.add(n) != 0 {
        n += 1;
    }
    let bytes = (n + 1) * 2;
    let p = platform();
    if !vmctrl_open(&p) {
        return false;
    }
    let mut b = Vec::new();
    if b.try_reserve_exact(bytes + 0x18).is_err() {
        SetLastError(ERROR_OUTOFMEMORY);
        return false;
    }
    b.resize(bytes + 0x18, 0u8);
    b[0..4].copy_from_slice(&(bytes as u32).to_le_bytes());
    let ptr = b.as_ptr() as u64 + 0x18;
    b[8..16].copy_from_slice(&ptr.to_le_bytes());
    b[0x10..0x14].copy_from_slice(&timeout.to_le_bytes());
    core::ptr::copy_nonoverlapping(name as *const u8, b.as_mut_ptr().add(0x18), bytes);
    device_io_result(p.host.ioctl(host::IOCTL_WAIT_TITLE_VM_TERMINATION, &b, &mut []), None)
}

#[no_mangle]
pub extern "system" fn XpalInitiateSystemShutdown(reason: u32, reboot: u8) -> bool {
    let mut b = [0u8; 8];
    b[0..4].copy_from_slice(&reason.to_le_bytes());
    b[4..8].copy_from_slice(&((reboot != 0) as u32).to_le_bytes());
    vm_bool(host::IOCTL_INITIATE_SYSTEM_SHUTDOWN, &b)
}

#[no_mangle]
pub extern "system" fn XpalInitiatePowerStateToggle(source: u32) -> bool {
    vm_bool(host::IOCTL_INITIATE_POWER_STATE_TOGGLE, &source.to_le_bytes())
}

#[no_mangle]
pub unsafe extern "system" fn XpalQueryPowerToggleSource(out: *mut u32) -> bool {
    if out.is_null() {
        SetLastError(ERROR_INVALID_PARAMETER);
        return false;
    }
    match vm_query(host::IOCTL_QUERY_POWER_TOGGLE_SOURCE, 4) {
        Some(v) => {
            *out = u32::from_le_bytes(v[..4].try_into().unwrap());
            true
        }
        None => false,
    }
}

#[no_mangle]
pub extern "system" fn XpalBackgroundActivityRequest(enable: u8) -> bool {
    vm_bool(host::IOCTL_BACKGROUND_ACTIVITY_REQUEST, &((enable != 0) as u32).to_le_bytes())
}

#[no_mangle]
pub extern "system" fn XpalRefreshPowerChimeMode() -> i32 {
    vm_bool(host::IOCTL_REFRESH_POWER_CHIME_MODE, &[]) as i32
}

#[no_mangle]
pub extern "system" fn XpalIsRebootRequested() -> bool {
    vm_query(host::IOCTL_IS_REBOOT_REQUESTED, 1).map(|v| v[0] != 0).unwrap_or(false)
}

#[no_mangle]
pub extern "system" fn XpalIsSilentBootMode() -> bool {
    vm_query(host::IOCTL_IS_SILENT_BOOT_MODE, 4).map(|v| v != [0, 0, 0, 0]).unwrap_or(false)
}

#[no_mangle]
pub extern "system" fn XpalIsTransitioningToConnectedStandby() -> bool {
    vm_query(host::IOCTL_IS_TRANSITIONING_TO_CS, 4).map(|v| v != [0, 0, 0, 0]).unwrap_or(false)
}

#[no_mangle]
pub extern "system" fn XpalExitForcedCs() -> bool {
    vm_bool(host::IOCTL_EXIT_FORCED_CS, &[])
}

#[no_mangle]
pub extern "system" fn XpalIsInForcedCs() -> u32 {
    vm_query(host::IOCTL_IS_IN_FORCED_CS, 4).map(|v| u32::from_le_bytes(v[..4].try_into().unwrap())).unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn XpalNotifyForcedCsHeartbeat(value: u32) -> i32 {
    vm_bool(host::IOCTL_NOTIFY_FORCED_CS_HEARTBEAT, &value.to_le_bytes()) as i32
}

#[no_mangle]
pub extern "system" fn XpalReportUem(a: u32, b: u32, c: u32) -> bool {
    let mut v = [0u8; 12];
    v[0..4].copy_from_slice(&a.to_le_bytes());
    v[4..8].copy_from_slice(&b.to_le_bytes());
    v[8..12].copy_from_slice(&c.to_le_bytes());
    vm_bool(host::IOCTL_REPORT_UEM, &v)
}

#[no_mangle]
pub extern "system" fn XpalInitiateDump(kind: u64) -> i32 {
    vm_bool(host::IOCTL_INITIATE_DUMP, &kind.to_le_bytes()) as i32
}

#[no_mangle]
pub extern "system" fn XpalSetWakeupTimer(value: u32) -> bool {
    vm_bool(host::IOCTL_SET_WAKEUP_TIMER, &value.to_le_bytes())
}

#[no_mangle]
pub unsafe extern "system" fn XpalSetSystemTime(ft: *const u64) -> bool {
    if ft.is_null() {
        SetLastError(ERROR_INVALID_PARAMETER);
        return false;
    }
    vm_bool(host::IOCTL_SET_SYSTEM_TIME, &core::ptr::read_unaligned(ft).to_le_bytes())
}

#[cfg(test)]
mod tests;
