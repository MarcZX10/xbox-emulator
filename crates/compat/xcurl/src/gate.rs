use crate::net::Signal;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

struct State {
    active: u32,
    limit: u32,
    suspended: bool,
    suspends: u64,
    waiters: Vec<Weak<Signal>>,
}

static STATE: Mutex<State> = Mutex::new(State { active: 0, limit: 0, suspended: false, suspends: 0, waiters: Vec::new() });
static CV: Condvar = Condvar::new();

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn wake(g: &mut State) {
    g.waiters.retain(|w| match w.upgrade() {
        Some(s) => {
            s.notify();
            true
        }
        None => false,
    });
    CV.notify_all();
}

pub struct Slot(());

impl Drop for Slot {
    fn drop(&mut self) {
        let mut g = lock();
        g.active = g.active.saturating_sub(1);
        wake(&mut g);
    }
}

pub fn watch(sig: &Arc<Signal>) {
    let mut g = lock();
    if !g.waiters.iter().any(|w| w.as_ptr() == Arc::as_ptr(sig)) {
        g.waiters.push(Arc::downgrade(sig));
    }
}

pub fn try_acquire() -> Option<Slot> {
    let mut g = lock();
    if g.suspended || (g.limit != 0 && g.active >= g.limit) {
        return None;
    }
    g.active += 1;
    Some(Slot(()))
}

pub fn acquire_until(deadline: Option<Instant>) -> Option<Slot> {
    let mut g = lock();
    loop {
        if !g.suspended && (g.limit == 0 || g.active < g.limit) {
            g.active += 1;
            return Some(Slot(()));
        }
        let wait = match deadline {
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    return None;
                }
                (d - now).min(Duration::from_millis(500))
            }
            None => Duration::from_millis(500),
        };
        g = CV.wait_timeout(g, wait).unwrap_or_else(|e| e.into_inner()).0;
    }
}

pub fn wait_resumed(deadline: Option<Instant>) -> bool {
    let mut g = lock();
    while g.suspended {
        let wait = match deadline {
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    return false;
                }
                (d - now).min(Duration::from_millis(500))
            }
            None => Duration::from_millis(500),
        };
        g = CV.wait_timeout(g, wait).unwrap_or_else(|e| e.into_inner()).0;
    }
    true
}

pub fn suspended() -> bool {
    lock().suspended
}

pub fn set_limit(limit: u32) {
    let mut g = lock();
    g.limit = limit;
    wake(&mut g);
}

pub fn limit() -> u32 {
    lock().limit
}

pub fn active() -> u32 {
    lock().active
}

pub fn suspend() -> bool {
    let mut g = lock();
    if g.suspended {
        return false;
    }
    g.suspended = true;
    g.suspends += 1;
    wake(&mut g);
    true
}

pub fn resume() -> bool {
    let mut g = lock();
    if !g.suspended {
        return false;
    }
    g.suspended = false;
    wake(&mut g);
    true
}
