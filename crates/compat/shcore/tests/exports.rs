mod common;

use common::*;
use core::ffi::c_void;

#[link(name = "kernel32")]
extern "system" {
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

#[link(name = "user32")]
extern "system" {
    fn SystemParametersInfoW(action: u32, ui: u32, pv: *mut c_void, flags: u32) -> i32;
}

fn ord(h: *mut c_void, n: usize) -> *mut c_void {
    unsafe { GetProcAddress(h, n as *const u8) }
}

fn dll() -> *mut c_void {
    load_built("shcore.dll", "tbx_shcore.dll")
}

#[test]
fn named_table_matches_and_ordinals_are_noname() {
    let named = xone_exports::pe_exports(&std::fs::read(built("shcore.dll")).unwrap()).unwrap_or_default();
    assert_eq!(named, vec![(37u16, "RevokeScaleChangeNotifications".to_string())]);
    let h = dll();
    for n in [225, 226, 227] {
        assert!(!ord(h, n).is_null(), "#{n}");
    }
    assert_eq!(ord(h, 37), proc(h, "RevokeScaleChangeNotifications"));
    assert!(proc(h, "Ordinal225").is_null());
    assert!(proc(h, "Ordinal227").is_null());
}

#[test]
fn ordinal_227_registers_and_revoke_releases_through_the_dll() {
    let reg: unsafe extern "system" fn(i32, *mut c_void, *mut u32) -> i32 = unsafe { core::mem::transmute(ord(dll(), 227)) };
    let rev: unsafe extern "system" fn(i32, u32) -> i32 = unsafe { core::mem::transmute(ord(dll(), 37)) };
    let mut cookie = 5u32;
    assert_eq!(unsafe { reg(0, core::ptr::null_mut(), &mut cookie) }, 0x8007_0057u32 as i32);
    assert_eq!(cookie, 0);
    assert_eq!(unsafe { rev(0, 0x7fff_fff0) }, 0x8007_0057u32 as i32);
}

#[test]
fn ordinal_225_matches_spi_a7() {
    let f: extern "system" fn() -> u8 = unsafe { core::mem::transmute(ord(dll(), 225)) };
    let mut v = [0u32; 3];
    unsafe { SystemParametersInfoW(0xa7, 0xc, v.as_mut_ptr() as *mut c_void, 0) };
    assert_eq!(f(), (v[2] as i32 > 0) as u8);
}

#[test]
fn ordinal_226_matches_spi_logical_dpi_override() {
    let f: unsafe extern "system" fn(*mut u8) -> i32 = unsafe { core::mem::transmute(ord(dll(), 226)) };
    let mut v: i32 = 0;
    let ok = unsafe { SystemParametersInfoW(0x9e, 0, &mut v as *mut i32 as *mut c_void, 0) };
    let mut out = 0xccu8;
    let hr = unsafe { f(&mut out) };
    if ok != 0 {
        assert_eq!(hr, 0);
        assert_eq!(out, (v > 0) as u8);
    } else {
        assert!(hr < 0);
        assert_eq!(out, 0);
    }
}
