//! xpso: check that the host's D3D12 accepts a translated compute shader.
//!
//! Translates an Xbox compute shader container, compiles it (DXC by default, `--fxc` for
//! D3DCompile is not offered here: this tool exists to validate signed DXIL), creates a real
//! D3D12 device, a root signature from the container's own RTS0 chunk, and a compute
//! pipeline state. Exit code 0 only if the pipeline state was created.
//!
//! Usage: xpso <compute-shader.dxbc>
#[cfg(not(windows))]
fn main() {
    eprintln!("xpso needs Windows");
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::ffi::c_void;
    use std::process::ExitCode;
    use tombox_xshader::{dxc, extract, hlsl, Stage};
    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);
    const IID_DEVICE: Guid = Guid(0x189819f1, 0x1db6, 0x4b57, [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7]);
    const IID_ROOTSIG: Guid = Guid(0xc54a6b66, 0x72df, 0x4ee8, [0x8b, 0xe5, 0xa9, 0x46, 0xa1, 0x42, 0x92, 0x14]);
    const IID_PSO: Guid = Guid(0x765a30f3, 0xf624, 0x4c6f, [0xa8, 0x28, 0xac, 0xe9, 0x48, 0x62, 0x24, 0x45]);
    extern "system" {
        fn LoadLibraryA(n: *const u8) -> *mut c_void;
        fn GetProcAddress(h: *mut c_void, n: *const u8) -> *mut c_void;
    }
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: xpso <compute-shader.dxbc>");
        return ExitCode::FAILURE;
    };
    let blob = std::fs::read(&path).expect("read");
    let ex = extract(&blob).expect("extract");
    if ex.stage != Stage::Compute {
        eprintln!("not a compute shader ({:?})", ex.stage);
        return ExitCode::FAILURE;
    }
    let t = hlsl::translate(&ex).expect("translate");
    let profile = dxc::profile_for(t.profile);
    let code = match dxc::compile(&t.hlsl, &profile) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    println!("compiled {} bytes of {profile} bytecode (container magic {:?})", code.len(), std::str::from_utf8(&code[..4]).unwrap_or("?"));
    // RTS0 chunk of the original container.
    let n = u32::from_le_bytes(blob[28..32].try_into().unwrap()) as usize;
    let mut rts: Option<&[u8]> = None;
    for i in 0..n {
        let off = u32::from_le_bytes(blob[32 + i * 4..36 + i * 4].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(blob[off + 4..off + 8].try_into().unwrap()) as usize;
        if &blob[off..off + 4] == b"RTS0" {
            rts = Some(&blob[off + 8..off + 8 + size]);
        }
    }
    let Some(rts) = rts else {
        eprintln!("container has no RTS0 chunk");
        return ExitCode::FAILURE;
    };
    unsafe {
        let d3d12 = LoadLibraryA(b"d3d12.dll\0".as_ptr());
        let create = GetProcAddress(d3d12, b"D3D12CreateDevice\0".as_ptr());
        let serialize = GetProcAddress(d3d12, b"D3D12CreateRootSignatureDeserializer\0".as_ptr());
        let _ = serialize;
        let create: unsafe extern "system" fn(*mut c_void, u32, *const Guid, *mut *mut c_void) -> i32 = std::mem::transmute(create);
        let mut dev: *mut c_void = std::ptr::null_mut();
        let hr = create(std::ptr::null_mut(), 0xb000, &IID_DEVICE, &mut dev);
        if hr < 0 {
            eprintln!("D3D12CreateDevice hr={hr:#010x}");
            return ExitCode::FAILURE;
        }
        let vt = *(dev as *const *const usize);
        // The RTS0 chunk body is a serialized root signature only when wrapped in its own
        // DXBC container; build one (header + one chunk).
        let mut rs = Vec::new();
        rs.extend_from_slice(b"DXBC");
        rs.extend_from_slice(&[0u8; 16]); // hash (not checked for root signatures)
        rs.extend_from_slice(&1u32.to_le_bytes());
        rs.extend_from_slice(&((36 + 8 + rts.len()) as u32).to_le_bytes());
        rs.extend_from_slice(&1u32.to_le_bytes());
        rs.extend_from_slice(&36u32.to_le_bytes());
        rs.extend_from_slice(b"RTS0");
        rs.extend_from_slice(&(rts.len() as u32).to_le_bytes());
        rs.extend_from_slice(rts);
        let create_rs: unsafe extern "system" fn(*mut c_void, u32, *const u8, usize, *const Guid, *mut *mut c_void) -> i32 =
            std::mem::transmute(*vt.add(16));
        let mut rootsig: *mut c_void = std::ptr::null_mut();
        let hr = create_rs(dev, 0, rs.as_ptr(), rs.len(), &IID_ROOTSIG, &mut rootsig);
        if hr < 0 {
            eprintln!("CreateRootSignature hr={hr:#010x}");
            return ExitCode::FAILURE;
        }
        // D3D12_COMPUTE_PIPELINE_STATE_DESC
        let mut desc = [0u64; 7];
        desc[0] = rootsig as u64;
        desc[1] = code.as_ptr() as u64;
        desc[2] = code.len() as u64;
        let create_pso: unsafe extern "system" fn(*mut c_void, *const u64, *const Guid, *mut *mut c_void) -> i32 =
            std::mem::transmute(*vt.add(11));
        let mut pso: *mut c_void = std::ptr::null_mut();
        let hr = create_pso(dev, desc.as_ptr(), &IID_PSO, &mut pso);
        println!("CreateComputePipelineState hr={hr:#010x} pso={pso:p}");
        if hr < 0 || pso.is_null() { ExitCode::FAILURE } else { ExitCode::SUCCESS }
    }
}
