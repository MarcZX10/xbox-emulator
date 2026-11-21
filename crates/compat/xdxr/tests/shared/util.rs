use core::ffi::c_void;
use std::path::PathBuf;
use x::CONSOLE;

extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn module_path(h: usize) -> String {
    console_dll::module_path(h).map(|p| p.display().to_string().to_lowercase()).unwrap_or_default()
}

fn near_exe(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors().skip(1).map(|d| d.join(name)).find(|p| p.is_file()).unwrap_or_else(|| panic!("{name} not built near {} (cargo build -p {})", exe.display(), name.trim_end_matches(".dll")))
}

fn emulator_xmem() -> String {
    let dll = near_exe("xmem.dll");
    let loaded = unsafe { GetModuleHandleW(wide("xmem.dll").as_ptr()) } as usize;
    let h = if loaded != 0 { loaded } else { unsafe { LoadLibraryExW(wide(&dll.to_string_lossy()).as_ptr(), core::ptr::null_mut(), 8) as usize } };
    assert!(h != 0);
    module_path(h)
}

#[allow(dead_code)]
fn console_module() -> usize {
    emulator_xmem();
    let h = CONSOLE.module();
    assert!(h != 0, "console {} did not load: {:?}", CONSOLE.spec.name, CONSOLE.candidates());
    h
}
