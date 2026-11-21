use std::ffi::c_void;
use std::path::PathBuf;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(r: *mut c_void, flags: u32) -> i32;
    fn CoCreateInstance(clsid: *const [u8; 16], outer: *mut c_void, ctx: u32, iid: *const [u8; 16], out: *mut *mut c_void) -> i32;
}

fn guid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> [u8; 16] {
    let mut g = [0u8; 16];
    g[..4].copy_from_slice(&d1.to_le_bytes());
    g[4..6].copy_from_slice(&d2.to_le_bytes());
    g[6..8].copy_from_slice(&d3.to_le_bytes());
    g[8..].copy_from_slice(&d4);
    g
}

fn wide(p: &std::path::Path) -> Vec<u16> {
    p.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect()
}

fn built_mfplat() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).take(7).map(|d| d.join("mfplat.dll")).find(|p| p.exists())
}

#[test]
fn system_decoders_bind_to_compat_mfplat() {
    let Some(ours) = built_mfplat() else {
        panic!("built mfplat.dll not found next to the test exe");
    };
    let dir = std::env::temp_dir().join(format!("mfplat_bind_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(&ours, dir.join("mfplat.dll")).unwrap();
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    std::fs::copy(PathBuf::from(sysroot).join("System32").join("mfplat.dll"), dir.join("mfplat_sys.dll")).unwrap();
    let h = unsafe { LoadLibraryW(wide(&dir.join("mfplat.dll")).as_ptr()) };
    assert!(!h.is_null(), "LoadLibraryW compat mfplat.dll failed");
    let startup = unsafe { GetProcAddress(h, b"MFStartup\0".as_ptr()) };
    assert!(!startup.is_null());
    let startup: unsafe extern "system" fn(u32, u32) -> i32 = unsafe { std::mem::transmute(startup) };
    assert_eq!(unsafe { CoInitializeEx(std::ptr::null_mut(), 0) } & !1, 0);
    assert_eq!(unsafe { startup(0x0002_0070, 0) }, 0);
    let iid_mft = guid(0xbf94c121, 0x5b05, 0x4e6f, [0x80, 0x00, 0xba, 0x59, 0x89, 0x61, 0x41, 0x4d]);
    let decoders = [
        ("CMSH264DecoderMFT", guid(0x62ce7e72, 0x4c71, 0x4d20, [0xb1, 0x5d, 0x45, 0x28, 0x31, 0xa8, 0x7d, 0x9d])),
        ("CMSAACDecMFT", guid(0x32d186a7, 0x218f, 0x4c75, [0x88, 0x76, 0xdd, 0x77, 0x27, 0x3a, 0x89, 0x99])),
    ];
    for (name, clsid) in decoders {
        let mut obj: *mut c_void = std::ptr::null_mut();
        let hr = unsafe { CoCreateInstance(&clsid, std::ptr::null_mut(), 1, &iid_mft, &mut obj) };
        assert_eq!(hr, 0, "{name} CoCreateInstance hr={:#010x} with compat mfplat loaded as MFPlat.DLL", hr as u32);
        assert!(!obj.is_null());
        unsafe {
            let vt = *(obj as *const *const usize);
            let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vt.add(2));
            release(obj);
        }
    }
}
