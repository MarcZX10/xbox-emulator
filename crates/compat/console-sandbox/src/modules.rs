use core::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use compat_common::os_host::patch::patch_image;

#[link(name = "kernel32")]
extern "system" {
    fn K32EnumProcessModules(p: *mut c_void, mods: *mut usize, cb: u32, needed: *mut u32) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetModuleFileNameW(m: usize, buf: *mut u16, n: u32) -> u32;
}

static ATTACHED: Mutex<Vec<(usize, String)>> = Mutex::new(Vec::new());

pub fn module_file(m: usize) -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(m, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 {
        None
    } else {
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
    }
}

pub fn loaded() -> Vec<usize> {
    let mut v = vec![0usize; 1024];
    let mut need = 0u32;
    unsafe { K32EnumProcessModules(GetCurrentProcess(), v.as_mut_ptr(), (v.len() * 8) as u32, &mut need) };
    v.truncate((need as usize / 8).min(1024));
    v
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.parent(), b.parent()) {
        (Some(x), Some(y)) => x.to_string_lossy().eq_ignore_ascii_case(&y.to_string_lossy()),
        _ => false,
    }
}

pub fn attached() -> Vec<(usize, String)> {
    ATTACHED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub unsafe fn attach_tree(module: usize) -> Vec<String> {
    let mut hits = Vec::new();
    let Some(path) = module_file(module) else { return hits };
    let hooks = crate::hooks();
    let mut done = ATTACHED.lock().unwrap_or_else(|e| e.into_inner());
    for m in loaded() {
        let Some(p) = module_file(m) else { continue };
        if m != module && !same_dir(&p, &path) {
            continue;
        }
        patch_image(m as *mut u8, &hooks, &mut hits);
        if !done.iter().any(|(x, _)| *x == m) {
            done.push((m, p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()));
        }
    }
    hits
}
