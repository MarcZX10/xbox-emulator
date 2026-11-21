use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::SystemTime;

pub const REFRESH_MS: u64 = 250;

struct Cfg {
    path: PathBuf,
    stamp: Option<(SystemTime, u64)>,
    map: HashMap<String, String>,
}

static CFG: RwLock<Option<Cfg>> = RwLock::new(None);
static NEXT_CHECK: AtomicU64 = AtomicU64::new(0);
static GEN: AtomicU64 = AtomicU64::new(0);

extern "system" {
    fn GetTickCount64() -> u64;
}

fn now_ms() -> u64 {
    unsafe { GetTickCount64() }
}

pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("TOMBOX_LIVE_CFG") {
        return PathBuf::from(p);
    }
    std::env::temp_dir().join("tombox").join("live.cfg")
}

pub fn normalize(key: &str) -> String {
    let k = key.trim().to_ascii_lowercase();
    match k.strip_prefix("tombox_") {
        Some(rest) => rest.to_string(),
        None => k,
    }
}

pub fn parse(text: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            let k = normalize(k);
            if !k.is_empty() {
                m.insert(k, v.trim().to_string());
            }
        }
    }
    m
}

fn stamp_of(p: &PathBuf) -> Option<(SystemTime, u64)> {
    let md = std::fs::metadata(p).ok()?;
    Some((md.modified().ok()?, md.len()))
}

pub fn refresh_now() {
    let p = path();
    let stamp = stamp_of(&p);
    {
        let g = CFG.read().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = g.as_ref() {
            if c.path == p && c.stamp == stamp {
                return;
            }
        }
    }
    let map = if stamp.is_some() { std::fs::read_to_string(&p).map(|t| parse(&t)).unwrap_or_default() } else { HashMap::new() };
    let mut g = CFG.write().unwrap_or_else(|e| e.into_inner());
    let changed = g.as_ref().map(|c| c.map != map || c.path != p).unwrap_or(true);
    *g = Some(Cfg { path: p, stamp, map });
    if changed {
        GEN.fetch_add(1, Ordering::Relaxed);
    }
}

fn refresh() {
    let now = now_ms();
    let next = NEXT_CHECK.load(Ordering::Relaxed);
    if now < next {
        return;
    }
    if NEXT_CHECK.compare_exchange(next, now + REFRESH_MS, Ordering::AcqRel, Ordering::Relaxed).is_err() {
        return;
    }
    refresh_now();
}

pub fn generation() -> u64 {
    refresh();
    GEN.load(Ordering::Relaxed)
}

pub fn get(key: &str) -> Option<String> {
    refresh();
    let k = normalize(key);
    let g = CFG.read().unwrap_or_else(|e| e.into_inner());
    g.as_ref().and_then(|c| c.map.get(&k).cloned())
}

pub fn snapshot() -> Vec<(String, String)> {
    refresh();
    let g = CFG.read().unwrap_or_else(|e| e.into_inner());
    let mut v: Vec<(String, String)> = g.as_ref().map(|c| c.map.iter().map(|(a, b)| (a.clone(), b.clone())).collect()).unwrap_or_default();
    v.sort();
    v
}

pub fn value(env: &str, key: &str) -> Option<String> {
    get(key).or_else(|| std::env::var(env).ok())
}

pub fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

pub fn flag(env: &str, key: &str, default: bool) -> bool {
    value(env, key).and_then(|v| parse_bool(&v)).unwrap_or(default)
}

pub fn env_flag(env: &str, default: bool) -> bool {
    flag(env, env, default)
}

pub fn number(env: &str, key: &str, default: u64) -> u64 {
    value(env, key).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

pub fn hashes(env: &str, key: &str) -> Vec<u64> {
    value(env, key)
        .map(|v| v.split(|c: char| c == ',' || c.is_whitespace()).filter_map(|w| u64::from_str_radix(w.trim().trim_start_matches("0x"), 16).ok()).collect())
        .unwrap_or_default()
}

pub struct Edge {
    last: Mutex<Option<String>>,
}

impl Edge {
    pub const fn new() -> Edge {
        Edge { last: Mutex::new(None) }
    }

    pub fn poll(&self, key: &str) -> Option<String> {
        let v = get(key);
        let seq = get(&format!("{key}_seq"));
        let cur = format!("{}#{}", v.as_deref().unwrap_or(""), seq.as_deref().unwrap_or(""));
        let mut g = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let first = g.is_none();
        if g.as_deref() == Some(cur.as_str()) {
            return None;
        }
        *g = Some(cur);
        if first {
            return None;
        }
        v.filter(|s| !s.is_empty() && s != "0")
    }
}

impl Default for Edge {
    fn default() -> Self {
        Edge::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_normalizes_keys_and_skips_noise() {
        let m = parse("TOMBOX_QUERY_FORCE_VISIBLE = 1\n# x=2\n; y=3\n\nblend_clamp=0\nbad line\nrecord_filter = f89dddcf70078d07, 251ed6ba5631cd6d\n");
        assert_eq!(m.get("query_force_visible").map(String::as_str), Some("1"));
        assert_eq!(m.get("blend_clamp").map(String::as_str), Some("0"));
        assert!(!m.contains_key("x") && !m.contains_key("# x"));
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn file_overrides_env_and_edges_fire_on_change() {
        let dir = std::env::temp_dir().join(format!("tombox_live_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("live.cfg");
        std::env::set_var("TOMBOX_LIVE_CFG", &f);
        std::env::set_var("TOMBOX_LIVETEST_A", "1");
        let _ = std::fs::remove_file(&f);
        refresh_now();
        assert!(flag("TOMBOX_LIVETEST_A", "livetest_a", false));
        std::fs::write(&f, "livetest_a=0\nrecord_filter=0xf89dddcf70078d07 251ed6ba5631cd6d\n").unwrap();
        refresh_now();
        assert!(!flag("TOMBOX_LIVETEST_A", "livetest_a", true));
        assert!(!env_flag("TOMBOX_LIVETEST_A", true));
        assert_eq!(hashes("X", "record_filter"), vec![0xf89dddcf70078d07, 0x251ed6ba5631cd6d]);
        let e = Edge::new();
        assert_eq!(e.poll("livetest_rec"), None);
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(&f, "livetest_rec=2\n").unwrap();
        refresh_now();
        assert_eq!(e.poll("livetest_rec").as_deref(), Some("2"));
        assert_eq!(e.poll("livetest_rec"), None);
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(&f, "livetest_rec=2\nlivetest_rec_seq=1\n").unwrap();
        refresh_now();
        assert_eq!(e.poll("livetest_rec").as_deref(), Some("2"));
        std::env::remove_var("TOMBOX_LIVE_CFG");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
