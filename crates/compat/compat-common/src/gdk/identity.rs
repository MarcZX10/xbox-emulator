use super::{log, setting};
use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

pub fn device() -> String {
    setting("device_type").unwrap_or_else(|| super::hook::default_device(super::hook::target_family().as_deref()).into()).to_ascii_lowercase()
}

pub fn device_type_value() -> u32 {
    match super::hook::console_mode(&device()) {
        Some(0x60) => 6,
        Some(0x64) => 7,
        Some(0x68) => 8,
        Some(0x58) => 5,
        Some(0x50) => 4,
        Some(0x40) => 3,
        Some(m) if m == 0x10 || (0x20..=0x21).contains(&m) || (0x30..=0x31).contains(&m) => 2,
        Some(_) => 0,
        None => 1,
    }
}

pub fn device_form() -> &'static str {
    match device_type_value() {
        2 => "Xbox One",
        3 => "Xbox One S",
        4 => "Xbox One X",
        5 => "Xbox One X Devkit",
        6 => "Xbox Series S",
        8 => "Xbox Series X Devkit",
        _ => "Xbox Series X",
    }
}

pub struct Cpu {
    pub count: u32,
    pub mask: u64,
}

pub fn cpu() -> Cpu {
    let t = crate::os_host::affinity::topology();
    Cpu { count: t.full.count_ones(), mask: t.full }
}

static ORIG_SYSINFO: AtomicUsize = AtomicUsize::new(0);
static ORIG_NATIVE_SYSINFO: AtomicUsize = AtomicUsize::new(0);

unsafe fn shape(info: *mut u8) {
    if info.is_null() {
        return;
    }
    let c = cpu();
    core::ptr::write_unaligned(info.add(24) as *mut usize, c.mask as usize);
    core::ptr::write_unaligned(info.add(32) as *mut u32, c.count);
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| log("GetSystemInfo", &format!("title sees {} processors, active mask {:#x} (console cores)", c.count, c.mask)));
}

unsafe extern "system" fn get_system_info_hook(info: *mut u8) {
    let f: unsafe extern "system" fn(*mut u8) = core::mem::transmute(ORIG_SYSINFO.load(Ordering::Acquire));
    f(info);
    shape(info);
}

unsafe extern "system" fn get_native_system_info_hook(info: *mut u8) {
    let f: unsafe extern "system" fn(*mut u8) = core::mem::transmute(ORIG_NATIVE_SYSINFO.load(Ordering::Acquire));
    f(info);
    shape(info);
}

pub fn targets() -> Vec<(&'static str, usize, &'static AtomicUsize)> {
    if setting("console_sysinfo").as_deref() == Some("0") {
        return Vec::new();
    }
    vec![
        ("GetSystemInfo", get_system_info_hook as usize, &ORIG_SYSINFO),
        ("GetNativeSystemInfo", get_native_system_info_hook as usize, &ORIG_NATIVE_SYSINFO),
    ]
}

const MARKER: &[u8] = b"TOMBOX_GDK_OWNER";

pub unsafe fn is_tombox_module(base: *mut c_void) -> bool {
    let b = base as *const u8;
    if b.is_null() || core::ptr::read_unaligned(b as *const u16) != 0x5a4d {
        return false;
    }
    let pe = b.add(core::ptr::read_unaligned(b.add(0x3c) as *const u32) as usize);
    let nsec = core::ptr::read_unaligned(pe.add(6) as *const u16) as usize;
    let opt = core::ptr::read_unaligned(pe.add(20) as *const u16) as usize;
    let sec = pe.add(24 + opt);
    for i in 0..nsec {
        let s = sec.add(i * 40);
        let name = std::slice::from_raw_parts(s, 8);
        if !name.starts_with(b".rdata") {
            continue;
        }
        let va = core::ptr::read_unaligned(s.add(12) as *const u32) as usize;
        let sz = core::ptr::read_unaligned(s.add(8) as *const u32) as usize;
        let data = std::slice::from_raw_parts(b.add(va), sz);
        let mut i = 0;
        while let Some(p) = data[i..].iter().position(|&c| c == b'T') {
            let at = i + p;
            if data[at..].starts_with(MARKER) {
                return true;
            }
            i = at + 1;
        }
    }
    false
}
