use core::ffi::c_void;
use dxcompiler_xs::com::*;
use dxcompiler_xs::guids::*;
use dxcompiler_xs::xdxr::CONSOLE;
use std::path::PathBuf;

extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

type QiFn = unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hr;

#[repr(C)]
struct Lib {
    ptr: *const u8,
    size: u32,
    pad: u32,
}

type LinkFn = unsafe extern "system" fn(*mut c_void, *const Lib, u32, *const *const u16, *mut u64, u32, u32, *mut *mut c_void) -> Hr;
type SetU32Fn = unsafe extern "system" fn(*mut c_void, u32) -> Hr;
type SetU8Fn = unsafe extern "system" fn(*mut c_void, u8) -> Hr;
type Create2 = unsafe extern "system" fn(*mut c_void, *const Guid, *const Guid, *mut *mut c_void) -> Hr;

unsafe fn slot<T: Copy>(obj: *mut c_void, i: usize) -> T {
    let f = *(*(obj as *const *const usize)).add(i);
    core::mem::transmute_copy(&f)
}

fn console_range() -> (usize, usize) {
    let h = CONSOLE.module();
    assert!(h != 0, "console dxcompiler_xs.dll not loaded: {:?}", CONSOLE.candidates());
    let size = std::fs::metadata(CONSOLE.path().unwrap()).unwrap().len() as usize;
    (h, h + 2 * size)
}

unsafe fn compiler() -> *mut c_void {
    let mut p = core::ptr::null_mut();
    let hr = dxcompiler_xs::CreateXdxrCompiler(&CLSID_XDXR_COMPILER as *const _ as *const c_void, &IID_IXDXR_COMPILER as *const _ as *const c_void, &mut p);
    assert_eq!(hr, S_OK);
    assert!(!p.is_null());
    p
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[test]
fn forwarded_object_is_the_console_xdxr_compiler() {
    let (lo, hi) = console_range();
    unsafe {
        let x = compiler();
        let vt = *(x as *const usize);
        assert!(vt > lo && vt < hi, "vtable {vt:#x} outside console module {lo:#x}..{hi:#x}");
        for i in 0..13 {
            let f = *((vt + 8 * i) as *const usize);
            assert!(f > lo && f < hi, "slot {i} {f:#x} outside the console module");
        }
        let mut q = core::ptr::null_mut();
        assert_eq!(slot::<QiFn>(x, 0)(x, &IID_IXDXR_COMPILER, &mut q), S_OK);
        assert_eq!(q, x);
        release(q);
        assert_eq!(release(x), 0);
    }
}

#[test]
fn built_cdylib_forwards_both_factories_to_the_console_dll() {
    let exe = std::env::current_exe().unwrap();
    let dll: PathBuf = exe.ancestors().skip(1).map(|d| d.join("dxcompiler_xs.dll")).find(|p| p.is_file()).unwrap_or_else(|| panic!("built dxcompiler_xs.dll not near {}", exe.display()));
    let w: Vec<u16> = dll.to_string_lossy().encode_utf16().chain(Some(0)).collect();
    let h = unsafe { LoadLibraryExW(w.as_ptr(), core::ptr::null_mut(), 8) } as usize;
    assert!(h != 0);
    let (lo, hi) = console_range();
    assert!(h < lo || h >= hi, "the compat cdylib and the console dxcompiler_xs.dll must be separate modules");
    unsafe {
        let c2: Create2 = core::mem::transmute(GetProcAddress(h as *mut c_void, b"CreateXdxrCompiler2\0".as_ptr()));
        let c1: unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> Hr = core::mem::transmute(GetProcAddress(h as *mut c_void, b"CreateXdxrCompiler\0".as_ptr()));
        for hr_obj in [
            {
                let mut p = core::ptr::null_mut();
                (c2(core::ptr::null_mut(), &CLSID_XDXR_COMPILER, &IID_IXDXR_COMPILER, &mut p), p)
            },
            {
                let mut p = core::ptr::null_mut();
                (c1(&CLSID_XDXR_COMPILER, &IID_IXDXR_COMPILER, &mut p), p)
            },
        ] {
            assert_eq!(hr_obj.0, S_OK);
            let vt = *(hr_obj.1 as *const usize);
            let console_vt = {
                let x = compiler();
                let v = *(x as *const usize);
                release(x);
                v
            };
            assert_eq!(vt, console_vt);
            release(hr_obj.1);
        }
        let mut p = 1usize as *mut c_void;
        assert_eq!(c1(&CLSID_DXC_COMPILER, &IID_IXDXR_COMPILER, &mut p), REGDB_E_CLASSNOTREG);
        assert!(p.is_null());
        assert_eq!(c1(&CLSID_XDXR_COMPILER, &IID_IXDXR_COMPILER, core::ptr::null_mut()), E_POINTER);
    }
}

#[test]
fn link_validates_its_arguments_like_the_console() {
    unsafe {
        let x = compiler();
        let link: LinkFn = slot(x, 8);
        let data = [0u8; 4];
        let libs = [Lib { ptr: data.as_ptr(), size: 4, pad: 0 }];
        let name = wide("RayGen");
        let names = [name.as_ptr()];
        let mut outs = [0u64; 1];
        let mut res = 7usize as *mut c_void;
        assert_eq!(link(x, core::ptr::null(), 1, names.as_ptr(), outs.as_mut_ptr(), 1, 0, &mut res), E_POINTER);
        assert_eq!(link(x, libs.as_ptr(), 1, core::ptr::null(), outs.as_mut_ptr(), 1, 0, &mut res), E_POINTER);
        assert_eq!(link(x, libs.as_ptr(), 1, names.as_ptr(), outs.as_mut_ptr(), 1, 0, core::ptr::null_mut()), E_POINTER);
        assert_eq!(res as usize, 7);
        assert_eq!(link(x, libs.as_ptr(), 0, names.as_ptr(), outs.as_mut_ptr(), 1, 0, &mut res), E_INVALIDARG);
        assert_eq!(link(x, libs.as_ptr(), 1, names.as_ptr(), outs.as_mut_ptr(), 0, 0, &mut res), E_INVALIDARG);
        assert_eq!(res as usize, 7);
        release(x);
    }
}

#[test]
fn option_setters_store_into_the_console_object() {
    unsafe {
        let x = compiler();
        let b = x as *const u8;
        assert_eq!(slot::<SetU8Fn>(x, 3)(x, 1), S_OK);
        assert_eq!(*b.add(0x20), 1);
        assert_eq!(slot::<SetU8Fn>(x, 4)(x, 1), S_OK);
        assert_eq!(*b.add(0x21), 1);
        assert_eq!(slot::<SetU8Fn>(x, 3)(x, 0), S_OK);
        assert_eq!((*b.add(0x20), *b.add(0x21)), (0, 1));
        let id = |o: *const u8| *(o.add(0x2c) as *const u32);
        assert_eq!(slot::<SetU32Fn>(x, 5)(x, 1), S_OK);
        assert_eq!(*(b.add(0x24) as *const u32), 1);
        let first = id(b);
        assert_eq!(slot::<SetU32Fn>(x, 5)(x, 2), S_OK);
        assert!(id(b) > first, "repro serial must advance");
        let second = id(b);
        assert_eq!(slot::<SetU32Fn>(x, 5)(x, 0), S_OK);
        assert_eq!((*(b.add(0x24) as *const u32), id(b)), (0, second));
        release(x);
    }
}

#[test]
fn console_dll_is_the_newest_title_build() {
    let p = { console_range(); CONSOLE.path().unwrap().to_path_buf() };
    let all = console_dll::dumps(std::path::Path::new(console_dll::GAMES), "dxcompiler_xs.dll");
    assert_eq!(all.first(), Some(&p));
}
