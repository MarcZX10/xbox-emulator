use crate::easy::Easy;
use crate::net::{Pool, Signal};
use core::ffi::c_void;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const CURLM_OK: i32 = 0;
pub const CURLM_BAD_HANDLE: i32 = 1;
pub const CURLM_BAD_EASY_HANDLE: i32 = 2;
pub const CURLM_UNKNOWN_OPTION: i32 = 6;
pub const CURLM_ADDED_ALREADY: i32 = 7;
pub const CURLM_BAD_FUNCTION_ARGUMENT: i32 = 10;
const CURLMSG_DONE: i32 = 1;
const MAGIC: u32 = 0x000b_ab1e;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Pending,
    Running,
    Done,
}

#[repr(C)]
pub struct CurlMsg {
    msg: i32,
    easy: *mut c_void,
    data: usize,
}

pub struct Multi {
    magic: u32,
    entries: Vec<(*mut Easy, State)>,
    sig: Arc<Signal>,
    msgs: VecDeque<(*mut Easy, i32)>,
    cur: CurlMsg,
    max_total: i64,
    max_host: i64,
    max_connects: i64,
    pipelining: i64,
    max_streams: i64,
    pool: Arc<Pool>,
}

unsafe fn multi<'a>(m: *mut c_void) -> Option<&'a mut Multi> {
    let p = (m as *mut Multi).as_mut()?;
    if p.magic == MAGIC {
        Some(p)
    } else {
        None
    }
}

impl Multi {
    fn running(&self) -> i32 {
        self.entries.iter().filter(|(_, s)| *s != State::Done).count() as i32
    }

    unsafe fn admissible(&self, e: *mut Easy) -> bool {
        if crate::gate::suspended() {
            return false;
        }
        let lim = crate::gate::limit();
        if lim != 0 && crate::gate::active() >= lim {
            return false;
        }
        if self.max_total <= 0 && self.max_host <= 0 {
            return true;
        }
        let mut per_host: Vec<(String, bool, i64)> = Vec::new();
        for (x, s) in &self.entries {
            if *s != State::Running {
                continue;
            }
            let k = (**x).host_key();
            let mux = self.multiplexed(*x);
            match per_host.iter_mut().find(|(h, m, _)| *h == k && *m == mux) {
                Some(v) => v.2 += 1,
                None => per_host.push((k, mux, 1)),
            }
        }
        let host = (*e).host_key();
        let mux = self.multiplexed(e);
        match per_host.iter_mut().find(|(h, m, _)| *h == host && *m == mux) {
            Some(v) => v.2 += 1,
            None => per_host.push((host.clone(), mux, 1)),
        }
        let conns = |mux: bool, n: i64| if mux { (n + self.max_streams - 1) / self.max_streams } else { n };
        let total: i64 = per_host.iter().map(|(_, m, n)| conns(*m, *n)).sum();
        if self.max_total > 0 && total > self.max_total {
            return false;
        }
        if self.max_host > 0 {
            let on_host: i64 = per_host.iter().filter(|(h, _, _)| *h == host).map(|(_, m, n)| conns(*m, *n)).sum();
            if on_host > self.max_host {
                return false;
            }
        }
        true
    }

    unsafe fn multiplexed(&self, e: *mut Easy) -> bool {
        self.pipelining & 2 != 0 && (*e).wants_http2()
    }

    fn pool_cap(&self) -> usize {
        if self.max_connects > 0 { self.max_connects as usize } else { (self.entries.len() * 4).max(4) }
    }

    unsafe fn ready(&self) -> bool {
        self.entries.iter().any(|(e, s)| match s {
            State::Pending => self.admissible(*e),
            State::Running => (**e).pending(),
            State::Done => false,
        })
    }

    unsafe fn next_deadline(&self) -> Option<Instant> {
        self.entries.iter().filter(|(_, s)| *s == State::Running).filter_map(|(e, _)| (**e).next_deadline()).min()
    }

    unsafe fn remove(&mut self, e: *mut Easy) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(x, _)| *x != e);
        self.msgs.retain(|(x, _)| *x != e);
        if before != self.entries.len() {
            (*e).abort();
            (*e).multi = 0;
            (*e).pool = None;
            self.pool.set_cap(self.pool_cap());
            true
        } else {
            false
        }
    }
}

pub unsafe fn detach(m: usize, e: *mut Easy) {
    if let Some(mm) = multi(m as *mut c_void) {
        mm.remove(e);
    }
    (*e).multi = 0;
}

#[no_mangle]
pub extern "C" fn curl_multi_init() -> *mut c_void {
    crate::guard();
    let sig = Signal::new();
    crate::gate::watch(&sig);
    Box::into_raw(Box::new(Multi {
        magic: MAGIC,
        entries: Vec::new(),
        sig,
        msgs: VecDeque::new(),
        cur: CurlMsg { msg: 0, easy: core::ptr::null_mut(), data: 0 },
        max_total: 0,
        max_host: 0,
        max_connects: 0,
        pipelining: 2,
        max_streams: 100,
        pool: Pool::new(0),
    })) as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn curl_multi_cleanup(m: *mut c_void) -> i32 {
    let Some(mm) = multi(m) else { return CURLM_BAD_HANDLE };
    for (e, _) in mm.entries.drain(..) {
        (*e).abort();
        (*e).multi = 0;
        (*e).pool = None;
    }
    mm.magic = 0;
    drop(Box::from_raw(m as *mut Multi));
    CURLM_OK
}

#[no_mangle]
pub unsafe extern "C" fn curl_multi_add_handle(m: *mut c_void, e: *mut c_void) -> i32 {
    let Some(mm) = multi(m) else { return CURLM_BAD_HANDLE };
    let Some(ee) = (e as *mut Easy).as_mut() else { return CURLM_BAD_EASY_HANDLE };
    if ee.multi != 0 {
        return CURLM_ADDED_ALREADY;
    }
    ee.abort();
    ee.result = None;
    ee.multi = m as usize;
    ee.pool = Some(mm.pool.clone());
    mm.entries.push((e as *mut Easy, State::Pending));
    mm.pool.set_cap(mm.pool_cap());
    mm.sig.notify();
    CURLM_OK
}

#[no_mangle]
pub unsafe extern "C" fn curl_multi_remove_handle(m: *mut c_void, e: *mut c_void) -> i32 {
    let Some(mm) = multi(m) else { return CURLM_BAD_HANDLE };
    if e.is_null() {
        return CURLM_BAD_EASY_HANDLE;
    }
    mm.remove(e as *mut Easy);
    CURLM_OK
}

#[no_mangle]
pub unsafe extern "C" fn curl_multi_perform(m: *mut c_void, running: *mut i32) -> i32 {
    let Some(mm) = multi(m) else { return CURLM_BAD_HANDLE };
    let list: Vec<*mut Easy> = mm.entries.iter().map(|(e, _)| *e).collect();
    for e in list {
        let Some(idx) = mm.entries.iter().position(|(x, _)| *x == e) else { continue };
        let st = mm.entries[idx].1;
        let outcome = match st {
            State::Pending => {
                if !mm.admissible(e) {
                    continue;
                }
                let Some(slot) = crate::gate::try_acquire() else { continue };
                match (*e).begin(mm.sig.clone(), Some(slot)) {
                    Ok(()) => {
                        mm.entries[idx].1 = State::Running;
                        (*e).pump()
                    }
                    Err(code) => Some(code),
                }
            }
            State::Running => (*e).pump(),
            State::Done => None,
        };
        if let Some(code) = outcome {
            if let Some(i) = mm.entries.iter().position(|(x, _)| *x == e) {
                mm.entries[i].1 = State::Done;
                mm.msgs.push_back((e, code));
            }
        }
    }
    if !running.is_null() {
        *running = mm.running();
    }
    CURLM_OK
}

unsafe fn wait_common(m: *mut c_void, timeout_ms: i32, numfds: *mut i32, wakeable: bool) -> i32 {
    let Some(mm) = multi(m) else { return CURLM_BAD_HANDLE };
    if timeout_ms < 0 {
        return CURLM_BAD_FUNCTION_ARGUMENT;
    }
    let sig = mm.sig.clone();
    let s0 = sig.seq();
    let set = |n: i32| {
        if !numfds.is_null() {
            *numfds = n;
        }
    };
    if wakeable && sig.take_woken() {
        set(0);
        return CURLM_OK;
    }
    if mm.ready() {
        set(1);
        return CURLM_OK;
    }
    let end = Instant::now() + Duration::from_millis(timeout_ms as u64);
    let mut seq = s0;
    loop {
        let now = Instant::now();
        let mut until = end;
        if !crate::gate::suspended() {
            if let Some(d) = mm.next_deadline() {
                until = until.min(d.max(now + Duration::from_millis(1)));
            }
        }
        if now >= until {
            set(if mm.ready() { 1 } else { 0 });
            return CURLM_OK;
        }
        let changed = sig.wait_change(seq, until - now);
        if wakeable && sig.take_woken() {
            set(0);
            return CURLM_OK;
        }
        if mm.ready() {
            set(1);
            return CURLM_OK;
        }
        if changed {
            seq = sig.seq();
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn curl_multi_poll(m: *mut c_void, _extra: *mut c_void, _n: u32, timeout_ms: i32, numfds: *mut i32) -> i32 {
    wait_common(m, timeout_ms, numfds, true)
}

#[no_mangle]
pub unsafe extern "C" fn curl_multi_wait(m: *mut c_void, _extra: *mut c_void, _n: u32, timeout_ms: i32, numfds: *mut i32) -> i32 {
    wait_common(m, timeout_ms, numfds, false)
}

#[no_mangle]
pub unsafe extern "C" fn curl_multi_wakeup(m: *mut c_void) -> i32 {
    let Some(mm) = multi(m) else { return CURLM_BAD_HANDLE };
    mm.sig.wake();
    CURLM_OK
}

#[no_mangle]
pub unsafe extern "C" fn curl_multi_info_read(m: *mut c_void, left: *mut i32) -> *mut c_void {
    let Some(mm) = multi(m) else {
        if !left.is_null() {
            *left = 0;
        }
        return core::ptr::null_mut();
    };
    let next = mm.msgs.pop_front();
    if !left.is_null() {
        *left = mm.msgs.len() as i32;
    }
    match next {
        Some((e, code)) => {
            mm.cur = CurlMsg { msg: CURLMSG_DONE, easy: e as *mut c_void, data: code as u32 as usize };
            &mut mm.cur as *mut CurlMsg as *mut c_void
        }
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn curl_multi_setopt(m: *mut c_void, opt: u32, arg: usize) -> i32 {
    let Some(mm) = multi(m) else { return CURLM_BAD_HANDLE };
    let long = arg as u32 as i32 as i64;
    match opt {
        3 => mm.pipelining = long,
        6 => mm.max_connects = long,
        7 => mm.max_host = long,
        13 => mm.max_total = long,
        16 => mm.max_streams = if long < 1 { 100 } else { long.min(2_147_483_647) },
        _ => {
            compat_common::record("xcurl", "unimplemented", &format!("curl_multi_setopt option {opt} (arg 0x{arg:x}) is not implemented; returned CURLM_UNKNOWN_OPTION"));
            return CURLM_UNKNOWN_OPTION;
        }
    }
    mm.pool.set_cap(mm.pool_cap());
    compat_common::record(
        "xcurl",
        "multi",
        &format!("limits: total {} per-host {} cache {} pipelining {} streams {}", mm.max_total, mm.max_host, mm.max_connects, mm.pipelining, mm.max_streams),
    );
    mm.sig.notify();
    CURLM_OK
}
