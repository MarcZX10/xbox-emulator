use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const SYSTEM32: &str = r"EmbeddedXvd\Windows\System32";
pub const DUMPS: &str = r"D:\xbox-pc-port\games";

pub fn file_version(path: &Path) -> Option<[u16; 4]> {
    let b = std::fs::read(path).ok()?;
    let sig = 0xfeef_04bdu32.to_le_bytes();
    let i = b.windows(4).position(|w| w == sig)?;
    let ms = u32::from_le_bytes(b.get(i + 8..i + 12)?.try_into().ok()?);
    let ls = u32::from_le_bytes(b.get(i + 12..i + 16)?.try_into().ok()?);
    Some([(ms >> 16) as u16, ms as u16, (ls >> 16) as u16, ls as u16])
}

pub fn xvd_version(system32: &Path) -> Option<[u16; 4]> {
    ["ntdll.dll", "xpal.dll", "kernelbase.dll"].iter().find_map(|f| file_version(&system32.join(f)))
}

fn identity_of(config: &Path) -> Option<String> {
    let t = std::fs::read_to_string(config).ok()?;
    let s = t.find("<Identity")?;
    let rest = &t[s..];
    let i = rest.find(" Name=\"")? + 7;
    rest[i..].split('"').next().map(|x| x.to_string())
}

fn mount_config(dir: &Path) -> Option<PathBuf> {
    ["MicrosoftGame.config", "MicrosoftGame.Config"].iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

pub fn system32_near(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().take(6).map(|a| a.join(SYSTEM32)).find(|p| p.is_dir())
}

pub fn system32_for_identity(root: &Path, identity: &str) -> Option<PathBuf> {
    let mut roots = Vec::new();
    for t in std::fs::read_dir(root).ok()?.flatten() {
        let ext = t.path().join("extracted");
        roots.push(ext.clone());
        if let Ok(inner) = std::fs::read_dir(&ext) {
            roots.extend(inner.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    roots.into_iter().find_map(|r| {
        let cfg = mount_config(&r.join("Mount"))?;
        let s32 = r.join(SYSTEM32);
        (identity_of(&cfg)?.eq_ignore_ascii_case(identity) && s32.is_dir()).then_some(s32)
    })
}

pub fn title_system32() -> Option<PathBuf> {
    if let Some(p) = crate::gdk::setting("title_xvd").map(PathBuf::from) {
        let s = if p.join(SYSTEM32).is_dir() { p.join(SYSTEM32) } else { p };
        if s.is_dir() {
            return Some(s);
        }
    }
    let t = crate::gdk::title();
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    t.dir.iter().chain(exe_dir.iter()).find_map(|d| system32_near(d)).or_else(|| system32_for_identity(Path::new(DUMPS), &t.identity))
}

pub fn title_os_version() -> Option<[u16; 4]> {
    static V: OnceLock<Option<[u16; 4]>> = OnceLock::new();
    *V.get_or_init(|| title_system32().and_then(|s| xvd_version(&s)))
}

pub fn title_os_build() -> Option<u32> {
    title_os_version().map(|v| v[2] as u32).filter(|&b| b != 0)
}
