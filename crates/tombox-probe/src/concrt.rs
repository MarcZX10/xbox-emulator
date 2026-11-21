use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const CS_CTOR: &str = "??0critical_section@Concurrency@@QEAA@XZ";
pub const CS_DTOR: &str = "??1critical_section@Concurrency@@QEAA@XZ";
pub const CS_LOCK: &str = "?lock@critical_section@Concurrency@@QEAAXXZ";
pub const CS_TRY_LOCK: &str = "?try_lock@critical_section@Concurrency@@QEAA_NXZ";
pub const CS_TRY_LOCK_FOR: &str = "?try_lock_for@critical_section@Concurrency@@QEAA_NI@Z";
pub const CS_UNLOCK: &str = "?unlock@critical_section@Concurrency@@QEAAXXZ";
pub const CV_CTOR: &str = "??0_Condition_variable@details@Concurrency@@QEAA@XZ";
pub const CV_DTOR: &str = "??1_Condition_variable@details@Concurrency@@QEAA@XZ";
pub const CV_WAIT: &str = "?wait@_Condition_variable@details@Concurrency@@QEAAXAEAVcritical_section@3@@Z";
pub const CV_WAIT_FOR: &str = "?wait_for@_Condition_variable@details@Concurrency@@QEAA_NAEAVcritical_section@3@I@Z";
pub const CV_NOTIFY_ONE: &str = "?notify_one@_Condition_variable@details@Concurrency@@QEAAXXZ";
pub const CV_NOTIFY_ALL: &str = "?notify_all@_Condition_variable@details@Concurrency@@QEAAXXZ";

pub const ALL: [&str; 12] = [CS_CTOR, CS_DTOR, CS_LOCK, CS_TRY_LOCK, CS_TRY_LOCK_FOR, CS_UNLOCK, CV_CTOR, CV_DTOR, CV_WAIT, CV_WAIT_FOR, CV_NOTIFY_ONE, CV_NOTIFY_ALL];

type This = extern "system" fn(usize) -> usize;
type ThisBool = extern "system" fn(usize) -> u8;
type ThisMs = extern "system" fn(usize, u32) -> u8;
type Wait = extern "system" fn(usize, usize);
type WaitFor = extern "system" fn(usize, usize, u32) -> u8;

#[derive(Clone, Copy)]
struct Fns {
    cs_ctor: This,
    cs_dtor: This,
    lock: This,
    try_lock: ThisBool,
    try_lock_for: ThisMs,
    unlock: This,
    cv_ctor: This,
    cv_dtor: This,
    wait: Wait,
    wait_for: WaitFor,
    notify_one: This,
    notify_all: This,
}

unsafe fn bind(addrs: &HashMap<String, usize>) -> Result<Fns, String> {
    let get = |n: &str| addrs.get(n).copied().filter(|a| *a != 0).ok_or_else(|| format!("unresolved {n}"));
    Ok(Fns {
        cs_ctor: std::mem::transmute::<usize, This>(get(CS_CTOR)?),
        cs_dtor: std::mem::transmute::<usize, This>(get(CS_DTOR)?),
        lock: std::mem::transmute::<usize, This>(get(CS_LOCK)?),
        try_lock: std::mem::transmute::<usize, ThisBool>(get(CS_TRY_LOCK)?),
        try_lock_for: std::mem::transmute::<usize, ThisMs>(get(CS_TRY_LOCK_FOR)?),
        unlock: std::mem::transmute::<usize, This>(get(CS_UNLOCK)?),
        cv_ctor: std::mem::transmute::<usize, This>(get(CV_CTOR)?),
        cv_dtor: std::mem::transmute::<usize, This>(get(CV_DTOR)?),
        wait: std::mem::transmute::<usize, Wait>(get(CV_WAIT)?),
        wait_for: std::mem::transmute::<usize, WaitFor>(get(CV_WAIT_FOR)?),
        notify_one: std::mem::transmute::<usize, This>(get(CV_NOTIFY_ONE)?),
        notify_all: std::mem::transmute::<usize, This>(get(CV_NOTIFY_ALL)?),
    })
}

#[repr(C, align(64))]
struct Storage([u8; 1024]);

pub fn selftest(addrs: &HashMap<String, usize>) -> Value {
    let f = match unsafe { bind(addrs) } {
        Ok(f) => f,
        Err(e) => return json!({"ran": false, "error": e}),
    };
    let mut steps: Vec<Value> = Vec::new();
    let mut ok_all = true;
    let mut step = |name: &str, ok: bool| {
        ok_all &= ok;
        steps.push(json!({"step": name, "ok": ok}));
    };
    let cs_box = Box::new(Storage([0u8; 1024]));
    let cv_box = Box::new(Storage([0u8; 1024]));
    let cs = &*cs_box as *const Storage as usize;
    let cv = &*cv_box as *const Storage as usize;
    let r = (f.cs_ctor)(cs);
    step("critical_section::critical_section returns this", r == cs);
    step("try_lock on free lock", (f.try_lock)(cs) != 0);
    let contended = std::thread::spawn(move || {
        let a = (f.try_lock)(cs) != 0;
        let b = (f.try_lock_for)(cs, 25) != 0;
        (a, b)
    })
    .join()
    .unwrap_or((true, true));
    step("try_lock from other thread while held fails", !contended.0);
    step("try_lock_for(25ms) from other thread while held times out", !contended.1);
    (f.unlock)(cs);
    let freed = std::thread::spawn(move || {
        let a = (f.try_lock_for)(cs, 1000) != 0;
        if a {
            (f.unlock)(cs);
        }
        a
    })
    .join()
    .unwrap_or(false);
    step("unlock then try_lock_for from other thread succeeds", freed);
    let r = (f.cv_ctor)(cv);
    step("_Condition_variable::_Condition_variable returns this", r == cv);
    (f.lock)(cs);
    step("lock", true);
    let t0 = std::time::Instant::now();
    let signalled = (f.wait_for)(cv, cs, 15) != 0;
    step("wait_for(15ms) without notify times out", !signalled && t0.elapsed().as_millis() >= 10);
    let flag = Arc::new(AtomicBool::new(false));
    let flag2 = flag.clone();
    let notifier = std::thread::spawn(move || {
        (f.lock)(cs);
        flag2.store(true, Ordering::SeqCst);
        (f.notify_one)(cv);
        (f.unlock)(cs);
    });
    let mut rounds = 0;
    while !flag.load(Ordering::SeqCst) && rounds < 1000 {
        (f.wait)(cv, cs);
        rounds += 1;
    }
    step("wait released the lock and woke on notify_one", flag.load(Ordering::SeqCst));
    (f.unlock)(cs);
    step("notifier thread finished", notifier.join().is_ok());
    (f.notify_all)(cv);
    step("notify_all with no waiters", true);
    (f.cv_dtor)(cv);
    (f.cs_dtor)(cs);
    step("destructors", true);
    drop(cs_box);
    drop(cv_box);
    json!({"ran": true, "ok": ok_all, "steps": steps})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win;

    #[test]
    fn host_concrt_semantics() {
        let Ok(h) = win::load("concrt140.dll", win::LOAD_LIBRARY_SEARCH_SYSTEM32) else { return };
        let mut addrs = HashMap::new();
        for n in ALL {
            addrs.insert(n.to_string(), win::proc_by_name(h, n).unwrap());
        }
        let v = selftest(&addrs);
        assert_eq!(v["ran"], true);
        assert_eq!(v["ok"], true, "{v}");
    }

    #[test]
    fn missing_entry_is_reported() {
        let v = selftest(&HashMap::new());
        assert_eq!(v["ran"], false);
    }
}
