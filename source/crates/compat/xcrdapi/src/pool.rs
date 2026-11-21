use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

extern "system" {
    fn GetActiveProcessorCount(group: u16) -> u32;
}

struct Job {
    f: &'static (dyn Fn(usize) + Sync),
    n: usize,
    next: AtomicUsize,
    done: Mutex<usize>,
    cv: Condvar,
}

struct Pool {
    jobs: Mutex<Vec<Arc<Job>>>,
    cv: Condvar,
    threads: usize,
}

pub static PARALLEL_GROUPS: AtomicU64 = AtomicU64::new(0);
pub static PARALLEL_REQUESTS: AtomicU64 = AtomicU64::new(0);

fn threads() -> usize {
    if let Some(n) = std::env::var("TOMBOX_DSTORAGE_WORKERS").ok().and_then(|v| v.parse::<usize>().ok()) {
        return n.min(32);
    }
    let cpus = unsafe { GetActiveProcessorCount(0xffff) } as usize;
    let cpus = if cpus == 0 { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) } else { cpus };
    cpus.clamp(1, 32)
}

fn pool() -> &'static Pool {
    static P: OnceLock<&'static Pool> = OnceLock::new();
    P.get_or_init(|| {
        let n = threads();
        let p: &'static Pool = Box::leak(Box::new(Pool { jobs: Mutex::new(Vec::new()), cv: Condvar::new(), threads: n }));
        for i in 0..n {
            let _ = std::thread::Builder::new().name(format!("xcrdapi-io{i}")).spawn(move || worker(p));
        }
        crate::log("[io]", &format!("DirectStorage request pool: {n} worker threads on all host cores (TOMBOX_DSTORAGE_WORKERS overrides)"));
        p
    })
}

fn worker(p: &'static Pool) {
    loop {
        let job = {
            let mut g = p.jobs.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                g.retain(|j| j.next.load(Ordering::Relaxed) < j.n);
                if let Some(j) = g.first() {
                    break j.clone();
                }
                g = p.cv.wait(g).unwrap_or_else(|e| e.into_inner());
            }
        };
        run(&job);
    }
}

fn run(job: &Job) {
    loop {
        let i = job.next.fetch_add(1, Ordering::AcqRel);
        if i >= job.n {
            return;
        }
        (job.f)(i);
        let mut d = job.done.lock().unwrap_or_else(|e| e.into_inner());
        *d += 1;
        if *d == job.n {
            job.cv.notify_all();
        }
    }
}

pub fn for_each(n: usize, f: &(dyn Fn(usize) + Sync)) {
    if n == 0 {
        return;
    }
    let p = pool();
    if n == 1 || p.threads == 0 {
        for i in 0..n {
            f(i);
        }
        return;
    }
    PARALLEL_GROUPS.fetch_add(1, Ordering::Relaxed);
    PARALLEL_REQUESTS.fetch_add(n as u64, Ordering::Relaxed);
    let f: &'static (dyn Fn(usize) + Sync) = unsafe { core::mem::transmute(f) };
    let job = Arc::new(Job { f, n, next: AtomicUsize::new(0), done: Mutex::new(0), cv: Condvar::new() });
    p.jobs.lock().unwrap_or_else(|e| e.into_inner()).push(job.clone());
    if n > 2 {
        p.cv.notify_all();
    } else {
        p.cv.notify_one();
    }
    run(&job);
    let mut d = job.done.lock().unwrap_or_else(|e| e.into_inner());
    while *d < n {
        d = job.cv.wait(d).unwrap_or_else(|e| e.into_inner());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_every_index_once() {
        let hits: Vec<AtomicUsize> = (0..1000).map(|_| AtomicUsize::new(0)).collect();
        for_each(hits.len(), &|i| {
            hits[i].fetch_add(1, Ordering::Relaxed);
        });
        assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 1));
    }
}
