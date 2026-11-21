use crate::image::{Entry, Exports, Image};
use crate::names::{is_api_set, normalize_module};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsoleHit {
    pub module_present: bool,
    pub host: Option<String>,
    pub ordinal: Option<u32>,
    pub name_at_ordinal: Option<String>,
    pub other_hosts: usize,
}

pub struct ConsoleIndex {
    pub dir: Option<PathBuf>,
    files: HashMap<String, PathBuf>,
    cache: HashMap<String, Option<Exports>>,
    by_name: Option<HashMap<String, Vec<(String, u32)>>>,
}

const HOST_PREFERENCE: [&str; 10] = [
    "kernelbase.dll",
    "ucrtbase.dll",
    "msvcp_win.dll",
    "combase.dll",
    "ntdll.dll",
    "kernel32.dll",
    "sechost.dll",
    "advapi32.dll",
    "twinapi.appcore.dll",
    "ws2_32.dll",
];

pub fn system32_of(xvd: &Path) -> Option<PathBuf> {
    let cands = [xvd.join("Windows").join("System32"), xvd.join("System32"), xvd.to_path_buf()];
    cands
        .iter()
        .find(|c| c.is_dir() && c.join("kernelbase.dll").is_file())
        .or_else(|| cands.iter().find(|c| c.is_dir() && c.ends_with("System32")))
        .cloned()
}

impl ConsoleIndex {
    pub fn new(xvd: Option<&Path>) -> ConsoleIndex {
        let dir = xvd.and_then(system32_of);
        let mut files = HashMap::new();
        if let Some(d) = &dir {
            if let Ok(rd) = std::fs::read_dir(d) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_file() && p.extension().map(|x| x.eq_ignore_ascii_case("dll")).unwrap_or(false) {
                        files.insert(normalize_module(&e.file_name().to_string_lossy()), p);
                    }
                }
            }
        }
        ConsoleIndex { dir, files, cache: HashMap::new(), by_name: None }
    }

    pub fn has_module(&self, module: &str) -> bool {
        self.files.contains_key(&normalize_module(module))
    }

    pub fn module_path(&self, module: &str) -> Option<PathBuf> {
        self.files.get(&normalize_module(module)).cloned()
    }

    pub fn exports(&mut self, module: &str) -> Option<&Exports> {
        let key = normalize_module(module);
        if !self.cache.contains_key(&key) {
            let ex = self.files.get(&key).and_then(|p| Image::open(p).ok()).map(|i| i.exports());
            self.cache.insert(key.clone(), ex);
        }
        self.cache.get(&key).and_then(|o| o.as_ref())
    }

    fn name_index(&mut self) -> &HashMap<String, Vec<(String, u32)>> {
        if self.by_name.is_none() {
            let mut idx: HashMap<String, Vec<(String, u32)>> = HashMap::new();
            let mut keys: Vec<String> = self.files.keys().cloned().collect();
            keys.sort();
            for k in keys {
                let Some(ex) = self.exports(&k).cloned() else { continue };
                for (n, o) in ex.by_name {
                    if ex.forwarders.contains_key(&n) {
                        continue;
                    }
                    idx.entry(n).or_default().push((k.clone(), o));
                }
            }
            self.by_name = Some(idx);
        }
        self.by_name.as_ref().unwrap()
    }

    pub fn lookup(&mut self, module: &str, entry: &Entry) -> ConsoleHit {
        let key = normalize_module(module);
        let mut hit = ConsoleHit { module_present: self.has_module(&key), ..Default::default() };
        if hit.module_present {
            if let Some(ex) = self.exports(&key) {
                match entry {
                    Entry::Name(n) => {
                        if let Some(o) = ex.ordinal_of(n) {
                            hit.host = Some(key.clone());
                            hit.ordinal = Some(o);
                            hit.name_at_ordinal = Some(n.clone());
                        }
                    }
                    Entry::Ordinal(o) => {
                        if let Some(n) = ex.name_of(*o as u32) {
                            hit.host = Some(key.clone());
                            hit.ordinal = Some(*o as u32);
                            hit.name_at_ordinal = n;
                        }
                    }
                }
            }
            return hit;
        }
        if let Entry::Name(n) = entry {
            if is_api_set(&key) || self.dir.is_some() {
                let hosts = self.name_index().get(n).cloned().unwrap_or_default();
                if let Some(best) = pick_host(&hosts) {
                    hit.host = Some(best.0.clone());
                    hit.ordinal = Some(best.1);
                    hit.name_at_ordinal = Some(n.clone());
                    hit.other_hosts = hosts.len() - 1;
                }
            }
        }
        hit
    }
}

pub fn pick_host(hosts: &[(String, u32)]) -> Option<&(String, u32)> {
    for pref in HOST_PREFERENCE {
        if let Some(h) = hosts.iter().find(|h| h.0 == pref) {
            return Some(h);
        }
    }
    hosts.first()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_preference_order() {
        let hosts = vec![("zzz.dll".to_string(), 5), ("ucrtbase.dll".to_string(), 9), ("kernelbase.dll".to_string(), 3)];
        assert_eq!(pick_host(&hosts).unwrap().0, "kernelbase.dll");
        let hosts = vec![("a.dll".to_string(), 1)];
        assert_eq!(pick_host(&hosts).unwrap().1, 1);
        assert!(pick_host(&[]).is_none());
    }

    #[test]
    fn empty_index_without_dump() {
        let mut ci = ConsoleIndex::new(None);
        let h = ci.lookup("kernel32.dll", &Entry::Name("GetTickCount64".into()));
        assert!(!h.module_present);
        assert!(h.host.is_none());
    }

    #[test]
    fn fortnite_console_lookups_when_dump_present() {
        let xvd = Path::new(r"D:\xbox-pc-port\games\Fortnite\extracted\EmbeddedXvd");
        if !xvd.is_dir() {
            return;
        }
        let mut ci = ConsoleIndex::new(Some(xvd));
        let h = ci.lookup("AudioHALWrapper.dll", &Entry::Name("CreateOpusDecoderHardwareInterface".into()));
        assert!(h.module_present);
        assert_eq!(h.host.as_deref(), Some("audiohalwrapper.dll"));
        assert!(h.ordinal.is_some());
        let h = ci.lookup("api-ms-win-core-sysinfo-l1-1-0.dll", &Entry::Name("GetTickCount64".into()));
        assert!(!h.module_present);
        assert!(h.host.is_some());
    }
}
