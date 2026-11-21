use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::OnceLock;

static HOST_PERIOD: AtomicI64 = AtomicI64::new(0);
static HOST_MHZ: AtomicU32 = AtomicU32::new(0);
static LOGGED: AtomicI64 = AtomicI64::new(0);
static LOGGED_OFF: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Console,
    Host,
    Hz(u32),
}

fn mode() -> Mode {
    static CFG: OnceLock<Option<String>> = OnceLock::new();
    static LIVE: crate::live_sw::Text = crate::live_sw::Text::new("TOMBOX_FRAME_HZ", "frame_hz");
    let v = LIVE.get().or_else(|| CFG.get_or_init(|| compat_common::cfg("frame_hz")).clone()).unwrap_or_else(|| "host".into());
    let m = match v.trim() {
        "console" | "0" => Mode::Console,
        "host" | "" => Mode::Host,
        n => n.parse::<u32>().ok().filter(|&h| (30..=1000).contains(&h)).map(Mode::Hz).unwrap_or(Mode::Host),
    };
    static LAST: std::sync::Mutex<Option<Mode>> = std::sync::Mutex::new(None);
    let mut l = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if l.is_some_and(|p| p != m) {
        LOGGED.store(0, Ordering::Relaxed);
        LOGGED_OFF.store(false, Ordering::Relaxed);
    }
    *l = Some(m);
    m
}

pub fn host_period(period_qpc: i64, refresh_mhz: u32) {
    if period_qpc > 0 {
        HOST_PERIOD.store(period_qpc, Ordering::Relaxed);
        HOST_MHZ.store(refresh_mhz, Ordering::Relaxed);
    }
}

fn target(console: i64) -> Option<i64> {
    let t = match mode() {
        Mode::Console => return None,
        Mode::Host => HOST_PERIOD.load(Ordering::Relaxed),
        Mode::Hz(h) => crate::frame::qpf() / h as i64,
    };
    if t <= 0 || t * 100 >= console * 95 {
        return None;
    }
    Some(t)
}

pub fn retime<T>(console: i64, events: Vec<(usize, i64, isize, T)>) -> (i64, Vec<(usize, i64, isize, T)>) {
    let Some(t) = target(console) else {
        if !LOGGED_OFF.swap(true, Ordering::Relaxed) {
            compat_common::record("d3d12_xs", "frame.clock", "frame events at the title's interval (TOMBOX_FRAME_HZ=console, or the host refresh is not faster)");
        }
        return (console, events);
    };
    if LOGGED.swap(t, Ordering::Relaxed) != t {
        let f = crate::frame::qpf() as f64;
        compat_common::record("d3d12_xs", "frame.clock", &format!(
            "frame events follow the host: interval {:.0}us -> {:.0}us ({:.1} Hz, host refresh {}.{:03} Hz); TOMBOX_FRAME_HZ=console restores the title's interval",
            console as f64 * 1e6 / f, t as f64 * 1e6 / f, f / t as f64,
            HOST_MHZ.load(Ordering::Relaxed) / 1000, HOST_MHZ.load(Ordering::Relaxed) % 1000));
    }
    let events = events.into_iter().map(|(ty, off, h, d)| (ty, (off as i128 * t as i128 / console as i128) as i64, h, d)).collect();
    (t, events)
}
