#[path = "../../sc_dll/tests/kit/mod.rs"]
mod kit;

use super::*;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

extern "system" {
    fn GetModuleFileNameW(h: *mut c_void, buf: *mut u16, n: u32) -> u32;
}

static SIMPLE: Mutex<Vec<usize>> = Mutex::new(Vec::new());

unsafe extern "system" fn simple(a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize, a8: usize, a9: usize, a10: usize, a11: usize, a12: usize) -> usize {
    let r = SCDxil_SCSimpleCompileShader(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
    SIMPLE.lock().unwrap().push(r);
    r
}

unsafe extern "system" fn gpa_hook(m: *mut c_void, name: *const u8) -> *mut c_void {
    if name as usize > 0xffff {
        match std::ffi::CStr::from_ptr(name as *const core::ffi::c_char).to_bytes() {
            b"SCDxil_SCSimpleCompileShader" => return simple as *const () as *mut c_void,
            b"SCDxil_GetExternalILInterface" => return SCDxil_GetExternalILInterface as *const () as *mut c_void,
            _ => {}
        }
    }
    kit::GetProcAddress(m, name)
}

fn console_dir() -> PathBuf {
    let h = console::module();
    assert!(h != 0, "no console scdxil.dll in the dumps");
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(h as *mut c_void, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    PathBuf::from(String::from_utf16_lossy(&buf[..n])).parent().unwrap().to_path_buf()
}

fn image_range(base: usize) -> core::ops::Range<usize> {
    unsafe {
        let nt = base + (*((base + 0x3c) as *const u32)) as usize;
        base..base + (*((nt + 0x18 + 0x38) as *const u32)) as usize
    }
}

const VS: &str = "[RootSignature(\"RootFlags(ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT)\")]\nfloat4 main(float4 p : POSITION) : SV_Position { return p * 2.0; }\n";

fn compiled() -> &'static Vec<u8> {
    static OUT: OnceLock<Vec<u8>> = OnceLock::new();
    OUT.get_or_init(|| {
        let dir = console_dir();
        let sc = kit::load(&dir.join("sc_dll.dll"));
        assert!(sc != 0, "console sc_dll.dll did not load");
        let mut hooked = false;
        for (name, slot) in unsafe { kit::imports(sc, "api-ms-win-core-libraryloader-l1-2-0.dll") } {
            if name == "GetProcAddress" {
                unsafe { kit::patch(slot, gpa_hook as usize) };
                hooked = true;
            }
        }
        assert!(hooked);
        let dxc = kit::load(&dir.join("dxcompiler_x.dll"));
        assert!(dxc != 0, "console dxcompiler_x.dll did not load");
        kit::dxc_compile(dxc, VS, "vs_6_0").unwrap_or_else(|e| panic!("dxcompiler_x: {e}"))
    })
}

#[test]
fn SCDxil_SCSimpleCompileShader_compiles_real_dxil_through_console_back_end() {
    let out = compiled();
    let r = SIMPLE.lock().unwrap().clone();
    assert!(!r.is_empty(), "console sc_dll never reached SCDxil_SCSimpleCompileShader");
    assert!(r.iter().all(|v| *v as u32 == 0), "{r:?}");
    let progs = kit::dxil_programs(out).unwrap_or_else(|e| panic!("{e}"));
    assert!(progs.iter().any(|p| p.code.iter().any(|c| c.0 == kit::Enc::Exp)), "no export in the vertex program");
    assert!(progs.iter().all(|p| p.code.last().map(|c| c.1) == Some(kit::S_ENDPGM)));
}

#[test]
fn SCDxil_GetExternalILInterface_fills_console_il_entry_points() {
    let h = console::module();
    assert!(h != 0);
    let range = image_range(h);
    let mut ours = [0usize; 3];
    let r = unsafe { SCDxil_GetExternalILInterface(&mut ours) };
    assert_eq!(r, &mut ours as *mut _);
    assert!(ours.iter().all(|p| range.contains(p)), "{ours:x?} outside {range:x?}");
    assert!(ours[0] != ours[1] && ours[1] != ours[2] && ours[0] != ours[2]);
    let direct = console::proc("SCDxil_GetExternalILInterface\0").unwrap();
    let mut theirs = [0usize; 3];
    assert_eq!(unsafe { direct(&mut theirs as *mut _ as usize, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0) }, &mut theirs as *mut _ as usize);
    assert_eq!(ours, theirs);
}
