use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

extern "system" {
    fn GetModuleHandleA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

type EpochFn = unsafe extern "system" fn() -> u64;
type ChangedFn = unsafe extern "system" fn(u64, usize, usize) -> i32;

const MAX_RANGES: usize = 512;

struct Cache {
    ranges: Vec<(usize, usize, bool, u64)>,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { ranges: Vec::new() });
static EPOCH_FN: AtomicUsize = AtomicUsize::new(0);
static CHANGED_FN: AtomicUsize = AtomicUsize::new(0);
pub static HITS: AtomicU64 = AtomicU64::new(0);
pub static MISSES: AtomicU64 = AtomicU64::new(0);

fn resolve(slot: &AtomicUsize, name: &[u8]) -> usize {
    let mut f = slot.load(Ordering::Acquire);
    if f == 0 {
        unsafe {
            let m = GetModuleHandleA(b"xmem.dll\0".as_ptr());
            if !m.is_null() {
                f = GetProcAddress(m, name.as_ptr()) as usize;
            }
        }
        if f != 0 {
            slot.store(f, Ordering::Release);
        }
    }
    f
}

fn epoch_fn() -> Option<EpochFn> {
    let f = resolve(&EPOCH_FN, b"TomboxAddressSpaceEpoch\0");
    (f != 0).then(|| unsafe { core::mem::transmute::<usize, EpochFn>(f) })
}

fn changed(ep: EpochFn, seq: u64, lo: usize, hi: usize) -> bool {
    let f = resolve(&CHANGED_FN, b"TomboxAddressSpaceChangedSince\0");
    if f == 0 {
        return unsafe { ep() } != seq;
    }
    unsafe { core::mem::transmute::<usize, ChangedFn>(f)(seq, lo, hi) != 0 }
}

impl Cache {
    fn find(&self, lo: usize, hi: usize, write: bool) -> Option<usize> {
        self.ranges.iter().position(|&(a, b, w, _)| a <= lo && hi <= b && (w || !write))
    }

    fn insert(&mut self, mut lo: usize, mut hi: usize, w: bool, mut seq: u64) {
        let mut i = 0;
        while i < self.ranges.len() {
            let (a, b, rw, s) = self.ranges[i];
            if rw == w && a <= hi && lo <= b {
                lo = lo.min(a);
                hi = hi.max(b);
                seq = seq.min(s);
                self.ranges.swap_remove(i);
            } else {
                i += 1;
            }
        }
        if self.ranges.len() >= MAX_RANGES {
            self.ranges.remove(0);
        }
        self.ranges.push((lo, hi, w, seq));
    }
}

#[cfg(test)]
mod tests {
    use super::Cache;

    #[test]
    fn merges_adjacent_and_respects_write() {
        let mut c = Cache { ranges: Vec::new() };
        c.insert(0x1000, 0x2000, true, 5);
        c.insert(0x2000, 0x3000, true, 3);
        assert_eq!(c.ranges.len(), 1);
        assert_eq!(c.ranges[0].3, 3);
        assert!(c.find(0x1800, 0x2800, true).is_some());
        assert!(c.find(0x800, 0x1800, false).is_none());
        c.insert(0x5000, 0x6000, false, 7);
        assert!(c.find(0x5000, 0x6000, false).is_some());
        assert!(c.find(0x5000, 0x6000, true).is_none());
        assert!(c.find(0x1000, 0x3000, false).is_some());
    }
}

pub fn check(lo: usize, hi: usize, write: bool, slow: impl FnOnce() -> Option<(usize, usize)>) -> bool {
    let Some(ep) = epoch_fn() else { return slow().is_some() };
    let e0 = unsafe { ep() };
    {
        let mut c = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(i) = c.find(lo, hi, write) {
            let (a, b, _, seq) = c.ranges[i];
            if !changed(ep, seq, a, b) {
                c.ranges[i].3 = e0;
                HITS.fetch_add(1, Ordering::Relaxed);
                return true;
            }
            c.ranges.swap_remove(i);
        }
    }
    MISSES.fetch_add(1, Ordering::Relaxed);
    match slow() {
        Some((a, b)) => {
            CACHE.lock().unwrap_or_else(|p| p.into_inner()).insert(a.min(lo), b.max(hi), write, e0);
            true
        }
        None => false,
    }
}
