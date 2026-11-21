use core::sync::atomic::{AtomicU64, Ordering};
use compat_common::live;

pub struct Sw {
    env: &'static str,
    key: &'static str,
    default: bool,
    cache: AtomicU64,
}

impl Sw {
    pub const fn new(env: &'static str, key: &'static str, default: bool) -> Sw {
        Sw { env, key, default, cache: AtomicU64::new(0) }
    }

    pub fn on(&self) -> bool {
        self.on_or(self.default)
    }

    pub fn on_or(&self, default: bool) -> bool {
        let g = live::generation() + 1;
        let c = self.cache.load(Ordering::Relaxed);
        if c >> 2 == g {
            return c & 1 == 1;
        }
        let v = live::value(self.env, self.key)
            .map(|s| live::parse_bool(&s).unwrap_or(s.trim() != "0"))
            .unwrap_or(default);
        let prev = if c & 2 != 0 { Some(c & 1 == 1) } else { None };
        self.cache.store((g << 2) | 2 | v as u64, Ordering::Relaxed);
        if prev.is_some_and(|p| p != v) {
            compat_common::record("d3d12_xs", "live", &format!("{} = {} (live.cfg)", self.key, if v { "on" } else { "off" }));
        }
        v
    }

    pub fn present_in_cfg(&self) -> bool {
        live::get(self.key).is_some()
    }
}

pub struct Armed {
    sw: Sw,
    start: std::sync::OnceLock<(bool, bool)>,
}

impl Armed {
    pub const fn new(env: &'static str, key: &'static str, default: bool) -> Armed {
        Armed { sw: Sw::new(env, key, default), start: std::sync::OnceLock::new() }
    }

    fn start(&self, default: impl FnOnce() -> bool) -> (bool, bool) {
        *self.start.get_or_init(|| {
            let d = default();
            let on = self.sw.on_or(d);
            (on || self.sw.present_in_cfg(), d)
        })
    }

    pub fn armed_or(&self, default: impl FnOnce() -> bool) -> bool {
        self.start(default).0
    }

    pub fn on_or(&self, default: impl FnOnce() -> bool) -> bool {
        let (armed, d) = self.start(default);
        armed && self.sw.on_or(d)
    }

    pub fn armed(&self) -> bool {
        let d = self.sw.default;
        self.armed_or(|| d)
    }

    pub fn on(&self) -> bool {
        let d = self.sw.default;
        self.on_or(|| d)
    }
}

pub struct Text {
    env: &'static str,
    key: &'static str,
    gen: AtomicU64,
    value: std::sync::Mutex<Option<String>>,
}

impl Text {
    pub const fn new(env: &'static str, key: &'static str) -> Text {
        Text { env, key, gen: AtomicU64::new(0), value: std::sync::Mutex::new(None) }
    }

    pub fn get(&self) -> Option<String> {
        let g = live::generation() + 1;
        let mut v = self.value.lock().unwrap_or_else(|e| e.into_inner());
        if self.gen.load(Ordering::Relaxed) != g {
            let n = live::value(self.env, self.key);
            if self.gen.load(Ordering::Relaxed) != 0 && n != *v {
                compat_common::record("d3d12_xs", "live", &format!("{} = {:?} (live.cfg)", self.key, n));
            }
            *v = n;
            self.gen.store(g, Ordering::Relaxed);
        }
        v.clone()
    }
}
