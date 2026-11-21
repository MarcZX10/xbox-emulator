use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

extern "system" {
    fn GetProcessHeaps(n: u32, heaps: *mut *mut c_void) -> u32;
    fn HeapValidate(h: *mut c_void, flags: u32, mem: *const c_void) -> i32;
    fn HeapSetInformation(h: *mut c_void, class: u32, info: *mut c_void, len: usize) -> i32;
    fn GetProcessHeap() -> *mut c_void;
}

static ON: AtomicBool = AtomicBool::new(false);
static BROKEN: AtomicBool = AtomicBool::new(false);

pub fn enable() {
    ON.store(true, Ordering::Relaxed);
    unsafe {
        HeapSetInformation(core::ptr::null_mut(), 1, core::ptr::null_mut(), 0);
    }
}

pub fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn heaps() -> Vec<usize> {
    unsafe {
        let n = GetProcessHeaps(0, core::ptr::null_mut());
        let mut v = vec![core::ptr::null_mut(); n as usize + 16];
        let m = GetProcessHeaps(v.len() as u32, v.as_mut_ptr());
        v.truncate(m as usize);
        v.into_iter().map(|h| h as usize).collect()
    }
}

pub fn validate() -> Result<usize, String> {
    let hs = heaps();
    let main = unsafe { GetProcessHeap() } as usize;
    let mut bad = Vec::new();
    for &h in &hs {
        if unsafe { HeapValidate(h as *mut c_void, 0, core::ptr::null()) } == 0 {
            bad.push(if h == main { format!("{h:#x} (process heap)") } else { format!("{h:#x}") });
        }
    }
    if bad.is_empty() { Ok(hs.len()) } else { Err(format!("HeapValidate failed on {} of {} heaps: {}", bad.len(), hs.len(), bad.join(", "))) }
}

pub static PHASE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
static WATCH: AtomicBool = AtomicBool::new(false);
pub static FIRST_BAD: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn phase(p: &str) {
    *PHASE.lock().unwrap_or_else(|e| e.into_inner()) = p.to_string();
}

pub fn watch_start(period_us: u64) {
    if WATCH.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let t0 = std::time::Instant::now();
        let mut last_ok = String::new();
        while WATCH.load(Ordering::SeqCst) {
            let before = PHASE.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Err(e) = validate() {
                let msg = format!("{e} at +{} ms; phase now '{before}', last clean check during '{last_ok}'", t0.elapsed().as_millis());
                println!("HEAP  {msg}");
                *FIRST_BAD.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg);
                BROKEN.store(true, Ordering::Relaxed);
                break;
            }
            last_ok = before;
            std::thread::sleep(std::time::Duration::from_micros(period_us));
        }
    });
}

pub fn watch_stop() {
    WATCH.store(false, Ordering::SeqCst);
}

pub fn after_step(name: &str) {
    if !enabled() || BROKEN.load(Ordering::Relaxed) {
        return;
    }
    match validate() {
        Ok(_) => {}
        Err(e) => {
            BROKEN.store(true, Ordering::Relaxed);
            println!("HEAP  corrupted after step '{name}': {e}");
        }
    }
}
