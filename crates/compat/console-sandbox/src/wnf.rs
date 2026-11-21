use core::ffi::c_void;
use std::collections::{HashMap, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

pub const STATUS_SUCCESS: i32 = 0;
pub const STATUS_TIMEOUT: i32 = 0x102;
pub const STATUS_UNSUCCESSFUL: i32 = 0xC000_0001u32 as i32;
pub const STATUS_INVALID_PARAMETER: i32 = 0xC000_000Du32 as i32;
pub const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023u32 as i32;
pub const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xC000_0034u32 as i32;
pub const STATUS_ACCESS_VIOLATION: i32 = 0xC000_0005u32 as i32;
pub const STATUS_INVALID_INFO_CLASS: i32 = 0xC000_0003u32 as i32;
pub const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004u32 as i32;
pub const STATUS_INVALID_HANDLE: i32 = 0xC000_0008u32 as i32;

pub const NAME_XOR: u64 = 0x41C6_4E6D_A3BC_0074;
pub const LIFETIME_WELL_KNOWN: u32 = 0;
pub const LIFETIME_PERMANENT: u32 = 1;
pub const LIFETIME_PERSISTENT: u32 = 2;
pub const LIFETIME_TEMPORARY: u32 = 3;
pub const DEFAULT_MAX_SIZE: u32 = 0x1000;
pub const MAX_STATE_SIZE: u32 = 0x1000;
pub const META_QUIESCENT: u32 = 8;

pub type UserCallback = unsafe extern "system" fn(u64, u32, *const c_void, *mut c_void, *const c_void, u32) -> i32;

#[derive(Default, Clone)]
pub struct State {
    pub data: Vec<u8>,
    pub stamp: u32,
    pub max: u32,
    pub type_id: Option<[u8; 16]>,
}

struct Sub {
    name: u64,
    seen: u32,
    callback: usize,
    context: usize,
    type_id: usize,
    group: u32,
    active: bool,
    running: bool,
}

struct Delivery {
    sub: usize,
    name: u64,
    stamp: u32,
    data: Vec<u8>,
}

#[derive(Default)]
struct Store {
    states: HashMap<u64, State>,
    deleted: Vec<u64>,
    subs: HashMap<usize, Sub>,
    queue: VecDeque<Delivery>,
    next_sub: usize,
    next_seq: u64,
    next_group: u32,
    worker: bool,
}

struct Shared {
    store: Mutex<Store>,
    wake: Condvar,
    idle: Condvar,
}

fn shared() -> &'static Shared {
    static S: OnceLock<Shared> = OnceLock::new();
    S.get_or_init(|| Shared { store: Mutex::new(Store { next_seq: 1, next_group: 1, ..Store::default() }), wake: Condvar::new(), idle: Condvar::new() })
}

fn lock() -> MutexGuard<'static, Store> {
    shared().store.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn lifetime(name: u64) -> u32 {
    (((name ^ NAME_XOR) >> 4) & 3) as u32
}

pub fn scope(name: u64) -> u32 {
    (((name ^ NAME_XOR) >> 6) & 0xf) as u32
}

pub fn make_name(lifetime: u32, scope: u32, permanent: bool, seq: u64) -> u64 {
    (1 | ((lifetime as u64 & 3) << 4) | ((scope as u64 & 0xf) << 6) | ((permanent as u64) << 10) | (seq << 11)) ^ NAME_XOR
}

fn exists(s: &Store, name: u64) -> bool {
    s.states.contains_key(&name) || (lifetime(name) != LIFETIME_TEMPORARY && !s.deleted.contains(&name))
}

pub fn state(name: u64) -> Option<State> {
    lock().states.get(&name).cloned()
}

pub fn subscribers(name: u64) -> usize {
    lock().subs.values().filter(|s| s.active && s.name == name).count()
}

pub fn quiescent(name: u64) -> bool {
    let s = lock();
    !s.queue.iter().any(|d| d.name == name) && !s.subs.values().any(|x| x.name == name && x.running)
}

fn worker() {
    let sh = shared();
    loop {
        let (cb, ctx, ty, d) = {
            let mut s = lock();
            loop {
                let mut taken = None;
                while let Some(d) = s.queue.pop_front() {
                    match s.subs.get_mut(&d.sub) {
                        Some(sub) if sub.active => {
                            sub.running = true;
                            sub.seen = d.stamp;
                            taken = Some((sub.callback, sub.context, sub.type_id, d));
                            break;
                        }
                        _ => {}
                    }
                }
                sh.idle.notify_all();
                if let Some(t) = taken {
                    break t;
                }
                s = sh.wake.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        };
        let f: UserCallback = unsafe { core::mem::transmute(cb) };
        unsafe { f(d.name, d.stamp, ty as *const c_void, ctx as *mut c_void, if d.data.is_empty() { core::ptr::null() } else { d.data.as_ptr() as *const c_void }, d.data.len() as u32) };
        let mut s = lock();
        if let Some(sub) = s.subs.get_mut(&d.sub) {
            sub.running = false;
        }
        drop(s);
        sh.idle.notify_all();
    }
}

fn enqueue(s: &mut Store, d: Delivery) {
    s.queue.push_back(d);
    if !s.worker {
        s.worker = true;
        std::thread::Builder::new().name("console-wnf".into()).spawn(worker).expect("wnf delivery thread");
    }
    shared().wake.notify_all();
}

pub fn create(lifetime: u32, scope: u32, permanent: bool, type_id: Option<[u8; 16]>, max: u32) -> u64 {
    let mut s = lock();
    let seq = s.next_seq;
    s.next_seq += 1;
    let n = make_name(lifetime, scope, permanent, seq);
    s.states.insert(n, State { data: Vec::new(), stamp: 0, max: if max == 0 { DEFAULT_MAX_SIZE } else { max }, type_id });
    n
}

pub fn delete(name: u64) -> i32 {
    let mut s = lock();
    if !exists(&s, name) {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    }
    s.states.remove(&name);
    if !s.deleted.contains(&name) {
        s.deleted.push(name);
    }
    for sub in s.subs.values_mut().filter(|x| x.name == name) {
        sub.active = false;
    }
    STATUS_SUCCESS
}

pub fn update(name: u64, data: &[u8], type_id: Option<[u8; 16]>, check: Option<u32>) -> i32 {
    let mut s = lock();
    if !exists(&s, name) {
        return STATUS_OBJECT_NAME_NOT_FOUND;
    }
    let st = s.states.entry(name).or_insert_with(|| State { max: MAX_STATE_SIZE, ..State::default() });
    if st.type_id.is_some() && st.type_id != type_id {
        return STATUS_INVALID_PARAMETER;
    }
    if data.len() as u32 > st.max {
        return STATUS_INVALID_PARAMETER;
    }
    if let Some(c) = check {
        if c != st.stamp {
            return STATUS_UNSUCCESSFUL;
        }
    }
    st.data = data.to_vec();
    st.stamp = st.stamp.wrapping_add(1);
    let stamp = st.stamp;
    let targets: Vec<usize> = s.subs.iter().filter(|(_, x)| x.active && x.name == name).map(|(k, _)| *k).collect();
    for t in targets {
        enqueue(&mut s, Delivery { sub: t, name, stamp, data: data.to_vec() });
    }
    STATUS_SUCCESS
}

pub fn query(name: u64, type_id: Option<[u8; 16]>) -> Result<(u32, Vec<u8>), i32> {
    let s = lock();
    if !exists(&s, name) {
        return Err(STATUS_OBJECT_NAME_NOT_FOUND);
    }
    match s.states.get(&name) {
        Some(st) => {
            if st.type_id.is_some() && st.type_id != type_id {
                return Err(STATUS_INVALID_PARAMETER);
            }
            Ok((st.stamp, st.data.clone()))
        }
        None => Ok((0, Vec::new())),
    }
}

pub fn subscribe(name: u64, seen: u32, callback: usize, context: usize, type_id: usize, group: u32) -> Result<usize, i32> {
    let mut s = lock();
    if !exists(&s, name) {
        return Err(STATUS_OBJECT_NAME_NOT_FOUND);
    }
    s.next_sub += 1;
    let id = s.next_sub;
    s.subs.insert(id, Sub { name, seen, callback, context, type_id, group, active: true, running: false });
    if let Some(st) = s.states.get(&name).cloned() {
        if st.stamp > seen {
            enqueue(&mut s, Delivery { sub: id, name, stamp: st.stamp, data: st.data });
        }
    }
    Ok(id)
}

pub fn unsubscribe(id: usize, wait: bool) -> i32 {
    let mut s = lock();
    match s.subs.get_mut(&id) {
        Some(sub) if sub.active => sub.active = false,
        _ => return STATUS_INVALID_PARAMETER,
    }
    if wait && std::thread::current().name() != Some("console-wnf") {
        while s.subs.get(&id).map(|x| x.running).unwrap_or(false) {
            s = shared().idle.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }
    s.queue.retain(|d| d.sub != id);
    if !s.subs.get(&id).map(|x| x.running).unwrap_or(false) {
        s.subs.remove(&id);
    }
    STATUS_SUCCESS
}

pub fn wait_quiescent(name: u64, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    let mut s = lock();
    loop {
        let busy = s.queue.iter().any(|d| d.name == name) || s.subs.values().any(|x| x.name == name && x.running);
        if !busy {
            return true;
        }
        let now = Instant::now();
        if now >= end {
            return false;
        }
        s = shared().idle.wait_timeout(s, end - now).unwrap_or_else(|e| e.into_inner()).0;
    }
}

pub fn group_of(id: usize) -> Option<u32> {
    lock().subs.get(&id).map(|s| s.group)
}

unsafe fn type_of(p: *const u8) -> Option<[u8; 16]> {
    if p.is_null() {
        None
    } else {
        let mut g = [0u8; 16];
        core::ptr::copy_nonoverlapping(p, g.as_mut_ptr(), 16);
        Some(g)
    }
}

pub unsafe extern "system" fn nt_create_wnf_state_name(out: *mut u64, lifetime: u32, scope: u32, persist: u8, type_id: *const u8, max: u32, _sd: *const c_void) -> i32 {
    if out.is_null() {
        return STATUS_ACCESS_VIOLATION;
    }
    if lifetime > LIFETIME_TEMPORARY || scope > 5 || max > MAX_STATE_SIZE {
        return STATUS_INVALID_PARAMETER;
    }
    *out = create(lifetime, scope, persist != 0, type_of(type_id), max);
    STATUS_SUCCESS
}

pub unsafe extern "system" fn nt_delete_wnf_state_name(name: *const u64) -> i32 {
    if name.is_null() {
        return STATUS_ACCESS_VIOLATION;
    }
    delete(*name)
}

pub unsafe extern "system" fn nt_update_wnf_state_data(name: *const u64, buf: *const u8, len: u32, type_id: *const u8, _scope: *const c_void, stamp: u32, check: u32) -> i32 {
    if name.is_null() || (buf.is_null() && len != 0) {
        return STATUS_INVALID_PARAMETER;
    }
    let d = if len == 0 { &[][..] } else { std::slice::from_raw_parts(buf, len as usize) };
    update(*name, d, type_of(type_id), if check != 0 { Some(stamp) } else { None })
}

pub unsafe extern "system" fn rtl_publish_wnf_state_data(name: u64, type_id: *const u8, buf: *const u8, len: u32, scope: *const c_void) -> i32 {
    nt_update_wnf_state_data(&name, buf, len, type_id, scope, 0, 0)
}

pub unsafe extern "system" fn nt_query_wnf_state_data(name: *const u64, type_id: *const u8, _scope: *const c_void, stamp: *mut u32, buf: *mut u8, size: *mut u32) -> i32 {
    if name.is_null() || size.is_null() || stamp.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    match query(*name, type_of(type_id)) {
        Err(e) => e,
        Ok((st, data)) => {
            *stamp = st;
            let cap = *size as usize;
            *size = data.len() as u32;
            if data.len() > cap {
                return STATUS_BUFFER_TOO_SMALL;
            }
            if !data.is_empty() {
                if buf.is_null() {
                    return STATUS_BUFFER_TOO_SMALL;
                }
                core::ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len());
            }
            STATUS_SUCCESS
        }
    }
}

pub unsafe extern "system" fn nt_query_wnf_state_name_information(name: *const u64, class: u32, _scope: *const c_void, buf: *mut u32, len: u32) -> i32 {
    if name.is_null() || buf.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    if class > 2 {
        return STATUS_INVALID_INFO_CLASS;
    }
    if len != 4 {
        return STATUS_INFO_LENGTH_MISMATCH;
    }
    let n = *name;
    let v = {
        let s = lock();
        if !exists(&s, n) {
            if class == 0 {
                *buf = 0;
                return STATUS_SUCCESS;
            }
            return STATUS_OBJECT_NAME_NOT_FOUND;
        }
        match class {
            0 => s.states.get(&n).map(|x| x.stamp > 0 || lifetime(n) == LIFETIME_TEMPORARY).unwrap_or(false) as u32,
            1 => s.subs.values().any(|x| x.active && x.name == n) as u32,
            _ => (!s.queue.iter().any(|d| d.name == n) && !s.subs.values().any(|x| x.name == n && x.running)) as u32,
        }
    };
    *buf = v;
    STATUS_SUCCESS
}

pub unsafe extern "system" fn rtl_subscribe_wnf_state_change_notification(out: *mut usize, name: u64, stamp: u32, callback: *const c_void, context: *mut c_void, type_id: *const u8, group: u32, flags: u32) -> i32 {
    if out.is_null() || callback.is_null() || flags != 0 {
        return STATUS_INVALID_PARAMETER;
    }
    match subscribe(name, stamp, callback as usize, context as usize, type_id as usize, group) {
        Ok(id) => {
            *out = id;
            STATUS_SUCCESS
        }
        Err(e) => e,
    }
}

pub unsafe extern "system" fn rtl_unsubscribe_wnf_state_change_notification(sub: usize) -> i32 {
    unsubscribe(sub, false)
}

pub unsafe extern "system" fn rtl_unsubscribe_wnf_notification_wait_for_completion(sub: usize) -> i32 {
    unsubscribe(sub, true)
}

pub unsafe extern "system" fn rtl_allocate_wnf_serialization_group() -> u32 {
    let mut s = lock();
    s.next_group += 1;
    s.next_group
}

pub unsafe extern "system" fn rtl_wait_for_wnf_meta_notification(name: u64, flags: u32, timeout: u32, _reserved: *const c_void, result: *mut u32) -> i32 {
    if result.is_null() {
        return STATUS_ACCESS_VIOLATION;
    }
    *result = 0;
    if timeout > 0x7fff_ffff || flags & 0x11 != 0 || flags & !0xe != 0 {
        return STATUS_INVALID_PARAMETER;
    }
    {
        let s = lock();
        if !exists(&s, name) {
            return STATUS_OBJECT_NAME_NOT_FOUND;
        }
    }
    let end = Instant::now() + Duration::from_millis(timeout as u64);
    loop {
        let mut hit = 0;
        if flags & META_QUIESCENT != 0 && quiescent(name) {
            hit |= META_QUIESCENT;
        }
        if flags & 2 != 0 && subscribers(name) > 0 {
            hit |= 2;
        }
        if flags & 4 != 0 && subscribers(name) == 0 {
            hit |= 4;
        }
        if hit != 0 {
            *result = hit;
            return STATUS_SUCCESS;
        }
        let now = Instant::now();
        if now >= end {
            return STATUS_TIMEOUT;
        }
        if flags & META_QUIESCENT != 0 {
            wait_quiescent(name, (end - now).min(Duration::from_millis(20)));
        } else {
            std::thread::sleep((end - now).min(Duration::from_millis(5)));
        }
    }
}
