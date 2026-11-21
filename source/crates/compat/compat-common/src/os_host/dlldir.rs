use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn is_relative(p: &str) -> bool {
    let b = p.as_bytes();
    if b.is_empty() || b[0] == b'\\' || b[0] == b'/' {
        return false;
    }
    !(b.len() >= 2 && b[1] == b':')
}

fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
    let s = s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string();
    s.trim_end_matches('\\').to_string()
}

pub fn resolve(path: &str, cwd: &Path, root: &Path) -> Option<String> {
    if !is_relative(path) || norm(cwd) == norm(root) {
        return None;
    }
    let mut out = PathBuf::from(root);
    for part in path.split(['\\', '/']) {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            x => out.push(x),
        }
    }
    Some(out.to_string_lossy().replace('/', "\\"))
}

pub fn package_root_from(exe_dir: &Path) -> PathBuf {
    let mut d = Some(exe_dir);
    let mut n = 0;
    while let Some(p) = d {
        if p.join("MicrosoftGame.Config").is_file() || p.join("MicrosoftGame.config").is_file() {
            return p.to_path_buf();
        }
        n += 1;
        if n > 6 {
            break;
        }
        d = p.parent();
    }
    exe_dir.to_path_buf()
}

pub fn package_root() -> &'static PathBuf {
    static R: OnceLock<PathBuf> = OnceLock::new();
    R.get_or_init(|| {
        if let Ok(p) = std::env::var("TOMBOX_PACKAGE_ROOT") {
            return PathBuf::from(p);
        }
        let exe = std::env::current_exe().unwrap_or_default();
        package_root_from(exe.parent().unwrap_or(Path::new(".")))
    })
}

type SetDirFn = unsafe extern "system" fn(*const u16) -> i32;

fn real() -> SetDirFn {
    static R: OnceLock<usize> = OnceLock::new();
    let p = *R.get_or_init(|| unsafe { super::resolve("kernelbase.dll", "SetDllDirectoryW") as usize });
    unsafe { core::mem::transmute(p) }
}

pub unsafe extern "system" fn set_dll_directory_w(path: *const u16) -> i32 {
    if path.is_null() {
        return real()(path);
    }
    let mut n = 0usize;
    while n < 32767 && *path.add(n) != 0 {
        n += 1;
    }
    let s = String::from_utf16_lossy(core::slice::from_raw_parts(path, n));
    let cwd = std::env::current_dir().unwrap_or_default();
    match resolve(&s, &cwd, package_root()) {
        Some(full) => {
            crate::record_raw("os_host", "SetDllDirectoryW", &format!("{s:?} is relative to the console package root; cwd {} differs, using {full:?}", cwd.display()));
            let w: Vec<u16> = full.encode_utf16().chain(Some(0)).collect();
            real()(w.as_ptr())
        }
        None => real()(path),
    }
}

pub fn check() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let root = package_root();
    if norm(&cwd) == norm(root) {
        format!("cwd is the package root {} (relative SetDllDirectoryW paths resolve as on console)", root.display())
    } else {
        format!("cwd {} differs from package root {}: relative SetDllDirectoryW paths are rebased onto the root", cwd.display(), root.display())
    }
}
