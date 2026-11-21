use core::ffi::c_void;
use std::path::PathBuf;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetModuleFileNameW(m: *mut c_void, buf: *mut u16, size: u32) -> u32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn built_dll() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    for dir in exe.ancestors() {
        for p in [dir.join("XAudio2_9.dll"), dir.join("debug").join("XAudio2_9.dll")] {
            if p.is_file() {
                return p;
            }
        }
    }
    panic!("built XAudio2_9.dll not found above {}", exe.display());
}

type CreateFn = unsafe extern "system" fn(*mut *mut c_void, u32, u32) -> i32;
type Fp = *const c_void;

unsafe fn slot(obj: *mut c_void, i: usize) -> Fp {
    *(*(obj as *mut *const Fp)).add(i)
}

#[test]
fn shipped_dll_creates_engine_and_master_on_a_thread_without_com_init() {
    let path = built_dll();
    let m = unsafe { LoadLibraryW(wide(path.to_str().unwrap()).as_ptr()) };
    assert!(!m.is_null(), "LoadLibrary {}", path.display());
    let by_name = unsafe { GetProcAddress(m, b"XAudio2Create\0".as_ptr()) };
    let by_ord1 = unsafe { GetProcAddress(m, 1usize as *const u8) };
    let by_ord8 = unsafe { GetProcAddress(m, 8usize as *const u8) };
    let alias = unsafe { GetProcAddress(m, b"XAudio2CreateV2_9\0".as_ptr()) };
    assert!(!by_name.is_null());
    assert_eq!(by_name, by_ord1);
    assert_eq!(by_name, by_ord8);
    assert_eq!(by_name, alias);
    for (ord, name) in [
        (2usize, &b"CreateAudioReverb\0"[..]),
        (3, b"CreateAudioVolumeMeter\0"),
        (4, b"CreateFX\0"),
        (5, b"X3DAudioCalculate\0"),
        (6, b"X3DAudioInitialize\0"),
        (7, b"CreateAudioReverbV2_8\0"),
        (9, b"XAudio2CreateWithVersionInfo\0"),
        (10, b"XAudio2CreateWithSharedContexts\0"),
    ] {
        let a = unsafe { GetProcAddress(m, ord as *const u8) };
        let b = unsafe { GetProcAddress(m, name.as_ptr()) };
        assert!(!a.is_null() && a == b, "ordinal {ord}");
    }
    let create: CreateFn = unsafe { core::mem::transmute(by_name) };
    let addr = create as usize;
    let r = std::thread::spawn(move || unsafe {
        let create: CreateFn = core::mem::transmute(addr);
        let mut engine: *mut c_void = core::ptr::null_mut();
        let hr = create(&mut engine, 0x400, 0);
        assert!(hr >= 0, "XAudio2Create {hr:#x}");
        let cm: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, u32, u32, u32, *const u16, *const c_void, i32) -> i32 = core::mem::transmute(slot(engine, 7));
        let mut master: *mut c_void = core::ptr::null_mut();
        let hr = cm(engine, &mut master, 0, 0, 0, core::ptr::null(), core::ptr::null(), 6);
        assert!(hr >= 0, "CreateMasteringVoice on a thread with no CoInitializeEx {hr:#x}");
        let sv: unsafe extern "system" fn(*mut c_void, f32, u32) -> i32 = core::mem::transmute(slot(master, 12));
        assert!(sv(master, 0.0, 0) >= 0);
        let d: unsafe extern "system" fn(*mut c_void) = core::mem::transmute(slot(master, 18));
        d(master);
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(slot(engine, 2));
        rel(engine);
    })
    .join();
    assert!(r.is_ok());
    let sys = std::env::var("SystemRoot").unwrap() + r"\System32\XAudio2_9.dll";
    let host = unsafe { GetModuleHandleW(wide(&sys).as_ptr()) };
    assert!(!host.is_null(), "host System32 XAudio2_9.dll not loaded");
    assert_ne!(host, m);
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(m, buf.as_mut_ptr(), 1024) } as usize;
    let mine = String::from_utf16_lossy(&buf[..n]);
    assert!(!mine.to_ascii_lowercase().contains("system32"), "{mine}");
}
