use std::sync::Mutex;

pub const CURLE_OK: i32 = 0;
pub const CURLE_FAILED_INIT: i32 = 2;
pub const CURLE_BAD_FUNCTION_ARGUMENT: i32 = 43;

static COUNT: Mutex<u32> = Mutex::new(0);

fn count() -> std::sync::MutexGuard<'static, u32> {
    COUNT.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn init_count() -> u32 {
    *count()
}

fn setup() {
    crate::guard();
    compat_common::record("xcurl", "curl_global_init", "first reference: global state set up");
}

fn teardown() {
    crate::net::Pool::global().close_idle();
    compat_common::record("xcurl", "curl_global_cleanup", "last reference released: pooled connections closed");
}

pub fn init(_flags: i32) -> i32 {
    let mut c = count();
    *c += 1;
    if *c == 1 {
        setup();
    }
    CURLE_OK
}

pub fn init_mem(callbacks: [usize; 5]) -> i32 {
    let mut c = count();
    *c += 1;
    if *c != 1 {
        return CURLE_FAILED_INIT;
    }
    if callbacks.contains(&0) {
        *c -= 1;
        return CURLE_BAD_FUNCTION_ARGUMENT;
    }
    setup();
    compat_common::record("xcurl", "curl_global_init_mem", "memory callbacks accepted; the compat layer allocates through the Rust allocator");
    CURLE_OK
}

pub fn cleanup() {
    let mut c = count();
    if *c == 0 {
        return;
    }
    *c -= 1;
    if *c == 0 {
        teardown();
    }
}

pub fn ensure_for_easy() -> bool {
    if init_count() == 0 {
        return init(3) == CURLE_OK;
    }
    true
}
