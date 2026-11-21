#![allow(non_snake_case)]

use newbe_xs as nb;

const S_OK: usize = 0;
const E_POINTER: usize = 0x8000_4003;
const E_INVALIDARG: usize = 0x8007_0057;
const E_FAIL: usize = 0x8000_4005;

fn console() -> bool {
    nb::CONSOLE.module(nb::own()) != 0
}

type Count = unsafe extern "system" fn(usize, *mut u32) -> i32;
type Pass = unsafe extern "system" fn(usize, u32, *mut usize) -> i32;
type Name = unsafe extern "system" fn(usize, *mut *const u16) -> i32;
type Release = unsafe extern "system" fn(usize) -> u32;

unsafe fn slot(obj: usize, i: usize) -> usize {
    *(*(obj as *const *const usize)).add(i)
}

unsafe fn wide(p: *const u16) -> String {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(core::slice::from_raw_parts(p, n))
}

#[test]
fn every_export_is_forwarded() {
    let names: Vec<&str> = nb::FORWARDED.iter().map(|f| f.0).collect();
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/exports.txt")).unwrap();
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        let n = l.split_whitespace().nth(1).unwrap();
        assert!(names.contains(&n), "{n}");
        if console() {
            assert!(nb::CONSOLE.proc(nb::own(), n).is_some(), "{n}");
        }
    }
}

#[test]
fn NB_CreateOptimizer_enumerates_console_passes() {
    if !console() {
        return;
    }
    unsafe {
        assert_eq!(nb::NB_CreateOptimizer(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0), E_POINTER);
        let mut o = 0usize;
        assert_eq!(nb::NB_CreateOptimizer(0, &mut o as *mut usize as usize, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0), S_OK);
        assert_ne!(o, 0);
        let count: Count = core::mem::transmute(slot(o, 3));
        let mut n = 0u32;
        assert_eq!(count(o, &mut n), 0);
        assert_eq!(n, 498);
        let pass: Pass = core::mem::transmute(slot(o, 4));
        let mut p = 0usize;
        let mut names = Vec::new();
        for i in 0..n {
            assert_eq!(pass(o, i, &mut p), 0);
            let name: Name = core::mem::transmute(slot(p, 3));
            let mut s: *const u16 = core::ptr::null();
            assert_eq!(name(p, &mut s), 0);
            names.push(wide(s));
        }
        for want in ["localizer", "xbox-delete-redundant-debug-values", "xbox-lower-uniform-rootdescriptor", "indvars", "scev-aa"] {
            assert!(names.iter().any(|x| x == want), "{want}");
        }
        assert_eq!(pass(o, n, &mut p) as u32 as usize, E_INVALIDARG);
        let release: Release = core::mem::transmute(slot(o, 2));
        release(o);
    }
}

#[test]
fn NB_CompileShader_rejects_null_arguments() {
    if !console() {
        return;
    }
    unsafe { assert_eq!(nb::NB_CompileShader(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0), E_INVALIDARG) }
}

#[test]
fn NB_CompilePipeline_rejects_missing_input() {
    if !console() {
        return;
    }
    unsafe { assert_eq!(nb::NB_CompilePipeline(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0), E_FAIL) }
}

#[test]
fn NB_CreatePixDwarfApi_rejects_null_arguments() {
    if !console() {
        return;
    }
    unsafe { assert_eq!(nb::NB_CreatePixDwarfApi(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0), E_INVALIDARG) }
}

#[test]
fn NB_GetShaderStats_fails_without_a_shader() {
    if !console() {
        return;
    }
    unsafe { assert_eq!(nb::NB_GetShaderStats(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0), E_FAIL) }
}

#[test]
fn NB_GetRegPressure_fails_without_a_shader() {
    if !console() {
        return;
    }
    unsafe { assert_eq!(nb::NB_GetRegPressure(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0), E_FAIL) }
}
