//! HLSL → DXIL through Microsoft's `dxcompiler.dll` (Windows only).
//!
//! Why DXC rather than FXC: the HLSL this crate generates (flag-based forward branches,
//! guarded writes, loops) is pathological for FXC — minutes for some compute shaders — and
//! compiles in about a second with DXC. DXC output is DXIL (shader model 6.0), which D3D12
//! accepts when it has been validated and signed; `dxil.dll` next to `dxcompiler.dll` does
//! that as part of the compile.
//!
//! The two DLLs are looked for in: `%TOMBOX_DXC_DIR%`, the directory of the running
//! executable, then the newest `Windows Kits\10\bin\<version>\x64`.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

extern "system" {
    fn LoadLibraryW(n: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
}

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);
const CLSID_DXC_COMPILER: Guid = Guid(0x73e22d93, 0xe6ce, 0x47f3, [0xb5, 0xbf, 0xf0, 0x66, 0x4f, 0x39, 0xc1, 0xb0]);
const IID_IDXC_COMPILER3: Guid = Guid(0x228b4687, 0x5a6a, 0x4730, [0x90, 0x0c, 0x97, 0x02, 0xb2, 0x20, 0x3f, 0x54]);
const IID_IDXC_RESULT: Guid = Guid(0x58346cda, 0xdde7, 0x4497, [0x94, 0x61, 0x6f, 0x87, 0xaf, 0x5e, 0x06, 0x59]);

#[repr(C)]
struct DxcBuffer {
    ptr: *const c_void,
    size: usize,
    encoding: u32,
}

type CreateInstance = unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(d) = std::env::var("TOMBOX_DXC_DIR") {
        v.push(PathBuf::from(d));
    }
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        v.push(d);
    }
    let kits = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if let Ok(rd) = std::fs::read_dir(&kits) {
        let mut vers: Vec<PathBuf> = rd.flatten().map(|e| e.path().join("x64")).filter(|p| p.join("dxcompiler.dll").exists()).collect();
        vers.sort();
        vers.reverse();
        v.extend(vers);
    }
    v
}

/// `DxcCreateInstance`, or why it is unavailable.
fn create_instance() -> Result<CreateInstance, String> {
    static F: OnceLock<Result<usize, String>> = OnceLock::new();
    F.get_or_init(|| {
        for dir in candidate_dirs() {
            let (comp, dxil) = (dir.join("dxcompiler.dll"), dir.join("dxil.dll"));
            if !comp.exists() || !dxil.exists() {
                continue;
            }
            unsafe {
                // dxil.dll first, by full path, so the compiler finds the validator/signer.
                let _ = LoadLibraryW(wide(&dxil.to_string_lossy()).as_ptr());
                let h = LoadLibraryW(wide(&comp.to_string_lossy()).as_ptr());
                if h.is_null() {
                    continue;
                }
                let p = GetProcAddress(h, b"DxcCreateInstance\0".as_ptr());
                if !p.is_null() {
                    return Ok(p as usize);
                }
            }
        }
        Err("dxcompiler.dll + dxil.dll not found (set TOMBOX_DXC_DIR or install the Windows SDK)".to_string())
    })
    .clone()
    .map(|p| unsafe { std::mem::transmute::<usize, CreateInstance>(p) })
}

pub fn available() -> bool {
    create_instance().is_ok()
}

unsafe fn vt(obj: *mut c_void) -> *const usize {
    *(obj as *const *const usize)
}
unsafe fn release(obj: *mut c_void) {
    if !obj.is_null() {
        let f: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vt(obj).add(2));
        f(obj);
    }
}
unsafe fn blob_bytes(blob: *mut c_void) -> Vec<u8> {
    if blob.is_null() {
        return Vec::new();
    }
    let ptr: unsafe extern "system" fn(*mut c_void) -> *const u8 = std::mem::transmute(*vt(blob).add(3));
    let len: unsafe extern "system" fn(*mut c_void) -> usize = std::mem::transmute(*vt(blob).add(4));
    let (p, n) = (ptr(blob), len(blob));
    if p.is_null() || n == 0 { Vec::new() } else { std::slice::from_raw_parts(p, n).to_vec() }
}

pub fn profile_for(fxc_profile: &str) -> String {
    let mut it = fxc_profile.split('_');
    let stage = it.next().unwrap_or("ps");
    let minor = if it.next() == Some("6") { it.next().and_then(|m| m.parse::<u32>().ok()).unwrap_or(0) } else { 0 };
    format!("{stage}_6_{}", minor.max(2))
}

/// Compile `hlsl` (entry `main`) to signed DXIL. `profile` is e.g. `cs_6_0`.
pub fn compile(hlsl: &str, profile: &str) -> Result<Vec<u8>, String> {
    compile_with(hlsl, profile, &[])
}

pub fn compile_with(hlsl: &str, profile: &str, extra: &[&str]) -> Result<Vec<u8>, String> {
    let create = create_instance()?;
    unsafe {
        let mut compiler: *mut c_void = std::ptr::null_mut();
        let hr = create(&CLSID_DXC_COMPILER, &IID_IDXC_COMPILER3, &mut compiler);
        if hr < 0 || compiler.is_null() {
            return Err(format!("dxc: DxcCreateInstance(IDxcCompiler3) hr={hr:#010x}"));
        }
        let minor = profile.rsplit('_').next().and_then(|m| m.parse::<u32>().ok()).unwrap_or(0);
        let denorm: &[&str] = if minor >= 2 && !extra.contains(&"-denorm") { &["-denorm", "preserve"] } else { &[] };
        let args_w: Vec<Vec<u16>> = ["-T", profile, "-E", "main"].iter().chain(denorm.iter()).chain(extra.iter()).map(|a| wide(a)).collect();
        let args: Vec<*const u16> = args_w.iter().map(|a| a.as_ptr()).collect();
        let src = DxcBuffer { ptr: hlsl.as_ptr() as *const c_void, size: hlsl.len(), encoding: 65001 };
        let mut result: *mut c_void = std::ptr::null_mut();
        // IDxcCompiler3::Compile (slot 3)
        let f: unsafe extern "system" fn(*mut c_void, *const DxcBuffer, *const *const u16, u32, *mut c_void, *const Guid, *mut *mut c_void) -> i32 =
            std::mem::transmute(*vt(compiler).add(3));
        let hr = f(compiler, &src, args.as_ptr(), args.len() as u32, std::ptr::null_mut(), &IID_IDXC_RESULT, &mut result);
        release(compiler);
        if hr < 0 || result.is_null() {
            return Err(format!("dxc: Compile hr={hr:#010x}"));
        }
        // IDxcOperationResult: 3 GetStatus, 4 GetResult, 5 GetErrorBuffer.
        let mut status = 0i32;
        let get_status: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32 = std::mem::transmute(*vt(result).add(3));
        get_status(result, &mut status);
        let get_blob = |slot: usize| -> *mut c_void {
            let g: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 = std::mem::transmute(*vt(result).add(slot));
            let mut b: *mut c_void = std::ptr::null_mut();
            g(result, &mut b);
            b
        };
        let out = if status >= 0 {
            let b = get_blob(4);
            let v = blob_bytes(b);
            release(b);
            if v.is_empty() { Err("dxc: empty result".to_string()) } else { Ok(v) }
        } else {
            let b = get_blob(5);
            let text = String::from_utf8_lossy(&blob_bytes(b)).into_owned();
            release(b);
            let line = text.lines().find(|l| l.contains("error")).unwrap_or(text.lines().next().unwrap_or("")).to_string();
            // "hlsl.hlsl:12:5: error: message" -> "message"
            Err(format!("dxc: {}", line.split("error: ").nth(1).unwrap_or(&line)))
        };
        release(result);
        out
    }
}

/// Location of a compiled shader in the on-disk cache shared by the offline sweep
/// (`xscan --compile`) and the runtime (`d3d12_xs`). Keyed by the generated HLSL and the
/// profile, so any change to a shader's translation misses the cache.
pub fn cache_path(hlsl: &str, profile: &str) -> PathBuf {
    fn fnv1a(b: &[u8]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for &x in b {
            h ^= x as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
    let key = fnv1a(hlsl.as_bytes()) ^ fnv1a(profile.as_bytes()).rotate_left(17);
    std::env::temp_dir().join("tombox_shader_cache").join("by_hlsl").join(format!("{key:016x}.cso"))
}
