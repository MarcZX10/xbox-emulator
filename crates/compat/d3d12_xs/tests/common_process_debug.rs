use core::ffi::c_void;
use std::path::{Path, PathBuf};

type H = *mut c_void;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(p: *const u16) -> H;
    fn GetProcAddress(h: H, n: *const u8) -> *mut c_void;
}

pub const IID_ID3D12DEVICE: [u8; 16] = [0xF1, 0x19, 0x98, 0x18, 0xB6, 0x1D, 0x57, 0x4B, 0xBE, 0x54, 0x18, 0x21, 0x33, 0x9B, 0x85, 0xF7];

pub struct Dll {
    pub h: H,
    pub log: PathBuf,
}

fn w(p: &Path) -> Vec<u16> {
    p.to_string_lossy().encode_utf16().chain(Some(0)).collect()
}

fn built_dll() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap().to_path_buf();
    let mut found: Vec<PathBuf> = ["release", "debug"]
        .iter()
        .flat_map(|p| [target.join(p).join("d3d12_xs.dll"), target.join(p).join("deps").join("d3d12_xs.dll")])
        .filter(|p| p.exists())
        .collect();
    found.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    found.pop().expect("built d3d12_xs.dll not found")
}

pub fn load(tag: &str) -> Dll {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("d3d12_xs_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(built_dll(), dir.join("d3d12_xs.dll")).unwrap();
    let log = dir.join("tombox_compat.log");
    let _ = std::fs::remove_file(&log);
    std::env::set_var("TOMBOX_LOG", &log);
    std::env::remove_var("TOMBOX_D3D12_DEBUG");
    let h = unsafe { LoadLibraryW(w(&dir.join("d3d12_xs.dll")).as_ptr()) };
    assert!(!h.is_null(), "load d3d12_xs.dll");
    Dll { h, log }
}

impl Dll {
    pub fn proc(&self, name: &str) -> usize {
        let n: Vec<u8> = name.bytes().chain(Some(0)).collect();
        let p = unsafe { GetProcAddress(self.h, n.as_ptr()) } as usize;
        assert!(p != 0, "{name} not exported");
        p
    }

    pub fn set(&self, flags: u32) -> i32 {
        let f: extern "system" fn(u32) -> i32 = unsafe { core::mem::transmute(self.proc("D3D12XboxSetProcessDebugFlags")) };
        f(flags)
    }

    pub fn get(&self) -> u32 {
        let f: extern "system" fn() -> u32 = unsafe { core::mem::transmute(self.proc("D3D12XboxGetProcessDebugFlags")) };
        f()
    }

    pub fn create(&self, params_flags: u32) -> (i32, *mut c_void) {
        let f: extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut *mut c_void) -> i32 =
            unsafe { core::mem::transmute(self.proc("D3D12XboxCreateDevice")) };
        let mut params = [0u32; 32];
        params[0] = 0x0002_0002;
        params[1] = params_flags;
        let mut dev: *mut c_void = core::ptr::null_mut();
        let hr = f(core::ptr::null_mut(), params.as_ptr() as *const c_void, IID_ID3D12DEVICE.as_ptr() as *const c_void, &mut dev);
        (hr, dev)
    }

    pub fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

pub fn sdk_layers_present() -> bool {
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    Path::new(&sys).join("System32").join("d3d12SDKLayers.dll").exists()
}
