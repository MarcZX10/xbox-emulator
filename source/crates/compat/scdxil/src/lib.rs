pub mod console;
pub mod dxil;

use core::ffi::c_void;

pub const SCDXIL_COMPILE_FAILED: usize = 1;

#[no_mangle]
pub unsafe extern "system" fn SCDxil_SCSimpleCompileShader(a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize, a8: usize, a9: usize, a10: usize, a11: usize, a12: usize) -> usize {
    match console::proc("SCDxil_SCSimpleCompileShader\0") {
        Some(f) => f(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12),
        None => SCDXIL_COMPILE_FAILED,
    }
}

#[no_mangle]
pub unsafe extern "system" fn SCDxil_GetExternalILInterface(out: *mut [usize; 3]) -> *mut [usize; 3] {
    match console::proc("SCDxil_GetExternalILInterface\0") {
        Some(f) => f(out as usize, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0) as *mut [usize; 3],
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "system" fn SCDxil_IsDxilShader(program: *const c_void, size: u32) -> bool {
    dxil::is_dxil_program(program as *const u8, size)
}

#[no_mangle]
pub unsafe extern "system" fn SCDxil_GetOriginalShader(out: *mut dxil::ShaderRef, program: *const c_void) -> *mut dxil::ShaderRef {
    *out = dxil::original_shader(program as *const u8);
    out
}

#[no_mangle]
pub unsafe extern "system" fn SCDxil_GetDriverShaderCode(container: *const c_void, out: *mut *const c_void) -> u32 {
    dxil::driver_shader_code(container as *const u8, out as *mut *const u8)
}

#[no_mangle]
pub unsafe extern "system" fn SCDxil_IsStageSupportedByNewBE(desc: *const c_void) -> u32 {
    dxil::stage_supported_by_new_be(desc as *const u8)
}

#[no_mangle]
pub unsafe extern "system" fn SCDxil_DxcCreateInstance(clsid: *const c_void, riid: *const c_void, ppv: *mut *mut c_void) -> i32 {
    dxcompiler_xs::DxcCreateInstance2(core::ptr::null_mut(), clsid, riid, ppv)
}

#[cfg(test)]
#[allow(non_snake_case)]
mod console_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use dxil::tests::{container, program};

    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);

    const CLSID_DXC_UTILS: Guid = Guid(0x6245_d6af, 0x66e0, 0x48fd, [0x80, 0xb4, 0x4d, 0x27, 0x17, 0x96, 0x74, 0x8c]);
    const IID_IDXC_UTILS: Guid = Guid(0x4605_c4cb, 0x2019, 0x492a, [0xad, 0xa4, 0x65, 0xf2, 0x0b, 0xb7, 0xd6, 0x7f]);
    const CLSID_UNKNOWN: Guid = Guid(0x1111_2222, 0x3333, 0x4444, [5; 8]);

    #[test]
    fn exported_queries() {
        unsafe {
            let p = program(16, &[]);
            assert!(SCDxil_IsDxilShader(p.as_ptr() as *const c_void, p.len() as u32));
            assert!(!SCDxil_IsDxilShader(p.as_ptr() as *const c_void, 8));
            let mut r = dxil::ShaderRef { data: 1 as *const u8, size: 9, pad: 9 };
            assert_eq!(SCDxil_GetOriginalShader(&mut r, p.as_ptr() as *const c_void), &mut r as *mut _);
            assert_eq!((r.data, r.size), (p.as_ptr(), p.len() as u32));
            let c = container(&[(dxil::FOURCC_DXIL, p.clone())]);
            let mut out: *const c_void = core::ptr::null();
            assert_eq!(SCDxil_GetDriverShaderCode(c.as_ptr() as *const c_void, &mut out), 0);
            assert_eq!(out as usize, c.as_ptr() as usize + 0x24 + 8);
            let mut d = [0u8; 0x20];
            d[0xf] = 1 << 3;
            d[0x1a] = 0x60;
            assert_eq!(SCDxil_IsStageSupportedByNewBE(d.as_ptr() as *const c_void), 1);
            d[0xf] = 2 << 3;
            assert_eq!(SCDxil_IsStageSupportedByNewBE(d.as_ptr() as *const c_void), 0);
        }
    }

    #[test]
    fn dxc_factory_maps_to_emulator_dxc() {
        unsafe {
            let mut o: *mut c_void = core::ptr::null_mut();
            let hr = SCDxil_DxcCreateInstance(&CLSID_DXC_UTILS as *const _ as *const c_void, &IID_IDXC_UTILS as *const _ as *const c_void, &mut o);
            assert_eq!(hr, 0, "{hr:#x}");
            assert!(!o.is_null());
            let vt = *(o as *const *const usize);
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(*vt.add(2));
            rel(o);
            let mut o: *mut c_void = 1 as *mut c_void;
            let hr = SCDxil_DxcCreateInstance(&CLSID_UNKNOWN as *const _ as *const c_void, &IID_IDXC_UTILS as *const _ as *const c_void, &mut o);
            assert!(hr < 0);
            assert!(o.is_null());
        }
    }
}
