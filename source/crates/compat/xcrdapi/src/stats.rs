use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Instant;

pub const READ: usize = 0;
pub const INFLATE: usize = 1;
pub const COPY: usize = 2;
const SLOTS: usize = 3;

static NS: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
static BYTES: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
static OPS: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
static MAX_NS: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
static ENQUEUED: AtomicU64 = AtomicU64::new(0);
static DONE: AtomicU64 = AtomicU64::new(0);
static REQUESTS: AtomicU64 = AtomicU64::new(0);
static BUSY_NS: AtomicU64 = AtomicU64::new(0);
static WINDOW_START: AtomicI64 = AtomicI64::new(0);
static LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn clock() -> &'static Instant {
    static T: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T.get_or_init(Instant::now)
}

fn now_ns() -> i64 {
    clock().elapsed().as_nanos() as i64
}

pub fn timed<R>(slot: usize, bytes: usize, f: impl FnOnce() -> R) -> R {
    let t0 = now_ns();
    let r = f();
    let dt = (now_ns() - t0).max(0) as u64;
    NS[slot].fetch_add(dt, Ordering::Relaxed);
    BYTES[slot].fetch_add(bytes as u64, Ordering::Relaxed);
    OPS[slot].fetch_add(1, Ordering::Relaxed);
    MAX_NS[slot].fetch_max(dt, Ordering::Relaxed);
    r
}

static PENDING_SINCE: std::sync::Mutex<Vec<(usize, i64)>> = std::sync::Mutex::new(Vec::new());
static FLUSH_SUBMIT: AtomicU64 = AtomicU64::new(0);
static FLUSH_HALF: AtomicU64 = AtomicU64::new(0);
static FLUSH_AGE_MAX_NS: AtomicU64 = AtomicU64::new(0);
static SLOW_FLUSH_LOGS: AtomicU64 = AtomicU64::new(0);

pub fn on_enqueue_q(queue: usize, was_empty: bool) {
    ENQUEUED.fetch_add(1, Ordering::Relaxed);
    if was_empty {
        let now = now_ns();
        let mut v = PENDING_SINCE.lock().unwrap_or_else(|p| p.into_inner());
        match v.iter_mut().find(|e| e.0 == queue) {
            Some(e) => e.1 = now,
            None => v.push((queue, now)),
        }
    }
}

pub fn on_flush(queue: usize, entries: usize, by_submit: bool, priority: i8) {
    if entries == 0 {
        return;
    }
    if by_submit {
        FLUSH_SUBMIT.fetch_add(1, Ordering::Relaxed);
    } else {
        FLUSH_HALF.fetch_add(1, Ordering::Relaxed);
    }
    let since = PENDING_SINCE.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|e| e.0 == queue).map(|e| e.1);
    let Some(since) = since else { return };
    let age = (now_ns() - since).max(0) as u64;
    FLUSH_AGE_MAX_NS.fetch_max(age, Ordering::Relaxed);
    if age > 200_000_000 && SLOW_FLUSH_LOGS.fetch_add(1, Ordering::Relaxed) < 200 {
        crate::log("[io]", &format!("queue priority={priority} held {entries} entries for {:.0} ms before {} handed them to the worker",
            age as f64 / 1e6, if by_submit { "Submit" } else { "the half-capacity flush" }));
    }
}

pub fn on_entry_done(busy_ns: u64, was_request: bool) {
    DONE.fetch_add(1, Ordering::Relaxed);
    BUSY_NS.fetch_add(busy_ns, Ordering::Relaxed);
    if was_request {
        REQUESTS.fetch_add(1, Ordering::Relaxed);
    }
    maybe_log();
}

pub fn measure_entry<R>(was_request: bool, f: impl FnOnce() -> R) -> R {
    let t0 = now_ns();
    let r = f();
    on_entry_done((now_ns() - t0).max(0) as u64, was_request);
    r
}

fn maybe_log() {
    let now = now_ns();
    let start = WINDOW_START.load(Ordering::Relaxed);
    if start == 0 {
        let _ = WINDOW_START.compare_exchange(0, now, Ordering::Relaxed, Ordering::Relaxed);
        return;
    }
    if now - start < 5_000_000_000 {
        return;
    }
    let Ok(_g) = LOG_LOCK.try_lock() else { return };
    if WINDOW_START.compare_exchange(start, now, Ordering::Relaxed, Ordering::Relaxed).is_err() {
        return;
    }
    let secs = (now - start) as f64 / 1e9;
    let reqs = REQUESTS.swap(0, Ordering::Relaxed);
    let busy = BUSY_NS.swap(0, Ordering::Relaxed) as f64 / 1e6;
    let backlog = ENQUEUED.load(Ordering::Relaxed).saturating_sub(DONE.load(Ordering::Relaxed));
    let mut parts = String::new();
    for (s, name) in ["read", "inflate", "copy"].iter().enumerate() {
        let n = OPS[s].swap(0, Ordering::Relaxed);
        let b = BYTES[s].swap(0, Ordering::Relaxed) as f64 / (1 << 20) as f64;
        let t = NS[s].swap(0, Ordering::Relaxed) as f64 / 1e6;
        let mx = MAX_NS[s].swap(0, Ordering::Relaxed) as f64 / 1e6;
        if n == 0 {
            continue;
        }
        let rate = if t > 0.0 { b / (t / 1000.0) } else { 0.0 };
        parts.push_str(&format!(" {name}={n}x/{b:.1}MB/{t:.0}ms({rate:.0}MB/s max {mx:.1}ms)"));
    }
    if reqs == 0 && parts.is_empty() {
        return;
    }
    parts.push_str(&format!(" flushes={}submit/{}half pending_age_max={:.1}ms",
        FLUSH_SUBMIT.swap(0, Ordering::Relaxed), FLUSH_HALF.swap(0, Ordering::Relaxed), FLUSH_AGE_MAX_NS.swap(0, Ordering::Relaxed) as f64 / 1e6));
    parts.push_str(&format!(" parallel_groups={} parallel_requests={}",
        crate::pool::PARALLEL_GROUPS.swap(0, Ordering::Relaxed), crate::pool::PARALLEL_REQUESTS.swap(0, Ordering::Relaxed)));
    let hits = crate::access_cache::HITS.swap(0, Ordering::Relaxed);
    let misses = crate::access_cache::MISSES.swap(0, Ordering::Relaxed);
    crate::log("[io]", &format!("window={secs:.1}s requests={reqs} ({:.0}/s) worker_busy={busy:.0}ms ({:.0}% of one thread) backlog={backlog} access_checks={hits} cached/{misses} queried{parts}",
        reqs as f64 / secs, busy / (secs * 10.0)));
}
