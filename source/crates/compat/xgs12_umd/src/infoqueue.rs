use crate::{Guid, E_INVALIDARG, E_NOINTERFACE, E_NOTIMPL, E_POINTER, S_OK};
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

pub const S_FALSE: i32 = 1;
pub const IID_IDXGI_INFO_QUEUE: Guid = Guid::new(0xd674_41c7, 0x672a, 0x476f, [0x9e, 0x82, 0xcd, 0x55, 0xb4, 0x49, 0x49, 0xce]);
pub const IID_IUNKNOWN: Guid = Guid::new(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
pub const DXGI_DEBUG_ALL: Guid = Guid::new(0xe48a_e283, 0xda80, 0x490b, [0x87, 0xe6, 0x43, 0xe9, 0xa9, 0xcf, 0xda, 0x08]);
pub const DXGI_DEBUG_DX: Guid = Guid::new(0x35cd_d7fc, 0x13b2, 0x421d, [0xa5, 0xd7, 0x7e, 0x44, 0x51, 0x28, 0x7d, 0x64]);
pub const DXGI_DEBUG_DXGI: Guid = Guid::new(0x25cd_daa4, 0xb1c6, 0x47e1, [0xac, 0x3e, 0x98, 0x87, 0x5b, 0x5a, 0x2e, 0x2a]);
pub const DXGI_DEBUG_APP: Guid = Guid::new(0x06cd_6e01, 0x4219, 0x4ebd, [0x87, 0x09, 0x27, 0xed, 0x23, 0x36, 0x0c, 0x62]);
pub const BREAK_EXCEPTION: u32 = 0x87a;
pub const DEFAULT_LIMIT: u64 = 0x400;
const OUTPUT_MAX: usize = 0xbb8 - 1;
const LIMIT_TEXT: &str = "Can not set message count limit with DXGI_DEBUG_ALL or DXGI_DEBUG_DX as the API.";

extern "system" {
    fn OutputDebugStringA(s: *const u8);
    fn RaiseException(code: u32, flags: u32, n: u32, args: *const usize);
}

fn key(g: &Guid) -> [u8; 16] {
    let mut k = [0u8; 16];
    k[..4].copy_from_slice(&g.d1.to_le_bytes());
    k[4..6].copy_from_slice(&g.d2.to_le_bytes());
    k[6..8].copy_from_slice(&g.d3.to_le_bytes());
    k[8..].copy_from_slice(&g.d4);
    k
}

fn is_special(g: &Guid) -> bool {
    *g == DXGI_DEBUG_ALL || *g == DXGI_DEBUG_DX
}

fn covers(query: &Guid, producer: &Guid) -> bool {
    *query == DXGI_DEBUG_ALL || (*query == DXGI_DEBUG_DX && *producer != DXGI_DEBUG_APP) || query == producer
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Filter {
    pub lists: [Vec<i32>; 6],
    seq: u64,
}

impl Filter {
    fn passes(&self, cat: i32, sev: i32, id: i32) -> bool {
        let v = [cat, sev, id];
        let allow = &self.lists[..3];
        if allow.iter().any(|l| !l.is_empty()) && !(0..3).any(|i| allow[i].contains(&v[i])) {
            return false;
        }
        !(0..3).any(|i| self.lists[3 + i].contains(&v[i]))
    }
}

#[derive(Clone)]
struct Entry {
    producer: Guid,
    limit: u64,
    discarded: u64,
    denied: u64,
    allowed: u64,
    stored: u64,
    retrieved: u64,
    mute: bool,
    break_cat: BTreeSet<i32>,
    break_sev: BTreeSet<i32>,
    break_id: BTreeSet<i32>,
    stacks: [Vec<Filter>; 2],
}

impl Entry {
    fn new(producer: Guid) -> Self {
        Entry {
            producer,
            limit: DEFAULT_LIMIT,
            discarded: 0,
            denied: 0,
            allowed: 0,
            stored: 0,
            retrieved: 0,
            mute: false,
            break_cat: BTreeSet::new(),
            break_sev: BTreeSet::new(),
            break_id: BTreeSet::new(),
            stacks: [Vec::new(), Vec::new()],
        }
    }
}

struct Msg {
    producer: Guid,
    category: i32,
    severity: i32,
    id: i32,
    desc: Vec<u8>,
}

pub struct Queue {
    entries: BTreeMap<[u8; 16], Entry>,
    all: Entry,
    dx: Entry,
    messages: VecDeque<Msg>,
    seq: u64,
    dirty: bool,
}

pub const STORAGE: usize = 0;
pub const RETRIEVAL: usize = 1;

pub struct Effects {
    pub output: Vec<String>,
    pub raise: Option<(i32, i32, Vec<u8>)>,
}

#[repr(C)]
pub struct FilterDesc {
    pub num_categories: u32,
    pub categories: *const i32,
    pub num_severities: u32,
    pub severities: *const i32,
    pub num_ids: u32,
    pub ids: *const i32,
}

#[repr(C)]
pub struct InfoQueueFilter {
    pub allow: FilterDesc,
    pub deny: FilterDesc,
}

#[repr(C)]
pub struct InfoQueueMessage {
    pub producer: Guid,
    pub category: i32,
    pub severity: i32,
    pub id: i32,
    pub description: *const u8,
    pub description_byte_length: usize,
}

pub fn producer_name(p: &Guid) -> &'static str {
    if *p == DXGI_DEBUG_DXGI {
        "DXGI"
    } else if *p == DXGI_DEBUG_APP {
        "Application"
    } else {
        "DX"
    }
}

pub fn category_name(p: &Guid, c: i32) -> &'static str {
    if *p == DXGI_DEBUG_APP {
        return "APPLICATION_DEFINED";
    }
    match c {
        0 => "UNKNOWN",
        1 => "MISCELLANEOUS",
        2 => "INITIALIZATION",
        3 => "CLEANUP",
        4 => "COMPILATION",
        5 => "STATE_CREATION",
        6 => "STATE_SETTING",
        7 => "STATE_GETTING",
        8 => "RESOURCE_MANIPULATION",
        9 => "EXECUTION",
        10 => "SHADER",
        _ => "",
    }
}

pub fn severity_name(s: i32) -> &'static str {
    match s {
        0 => "CORRUPTION",
        1 => "ERROR",
        2 => "WARNING",
        3 => "INFO",
        4 => "MESSAGE",
        _ => "",
    }
}

fn clip(mut s: String) -> String {
    if s.len() > OUTPUT_MAX {
        let mut n = OUTPUT_MAX;
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        s.truncate(n);
    }
    s
}

unsafe fn read_list(n: u32, p: *const i32) -> Vec<i32> {
    if n == 0 {
        Vec::new()
    } else {
        core::slice::from_raw_parts(p, n as usize).to_vec()
    }
}

pub unsafe fn read_filter(f: *const InfoQueueFilter) -> Result<[Vec<i32>; 6], i32> {
    if f.is_null() {
        return Err(E_INVALIDARG);
    }
    let f = &*f;
    let d = [
        (f.allow.num_categories, f.allow.categories),
        (f.allow.num_severities, f.allow.severities),
        (f.allow.num_ids, f.allow.ids),
        (f.deny.num_categories, f.deny.categories),
        (f.deny.num_severities, f.deny.severities),
        (f.deny.num_ids, f.deny.ids),
    ];
    if d.iter().any(|(n, p)| *n != 0 && p.is_null()) {
        return Err(E_INVALIDARG);
    }
    Ok([read_list(d[0].0, d[0].1), read_list(d[1].0, d[1].1), read_list(d[2].0, d[2].1), read_list(d[3].0, d[3].1), read_list(d[4].0, d[4].1), read_list(d[5].0, d[5].1)])
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

impl Queue {
    pub fn new() -> Self {
        Queue { entries: BTreeMap::new(), all: Entry::new(DXGI_DEBUG_ALL), dx: Entry::new(DXGI_DEBUG_DX), messages: VecDeque::new(), seq: 1, dirty: true }
    }

    fn entry(&mut self, p: &Guid) -> &mut Entry {
        if *p == DXGI_DEBUG_ALL {
            return &mut self.all;
        }
        if *p == DXGI_DEBUG_DX {
            return &mut self.dx;
        }
        self.entries.entry(key(p)).or_insert_with(|| Entry::new(*p))
    }

    fn next_seq(&mut self) -> u64 {
        let s = self.seq;
        self.seq += 1;
        s
    }

    fn effective(&self, p: &Guid, kind: usize) -> Option<&Filter> {
        let mut best: Option<&Filter> = self.entries.get(&key(p)).and_then(|e| e.stacks[kind].last());
        let mut candidates = Vec::with_capacity(2);
        if *p != DXGI_DEBUG_APP {
            candidates.push(self.dx.stacks[kind].last());
        }
        candidates.push(self.all.stacks[kind].last());
        for c in candidates.into_iter().flatten() {
            if best.map(|b| b.seq).unwrap_or(0) < c.seq {
                best = Some(c);
            }
        }
        best
    }

    fn passes(&self, m: &Msg, kind: usize) -> bool {
        self.effective(&m.producer, kind).map(|f| f.passes(m.category, m.severity, m.id)).unwrap_or(true)
    }

    fn refresh(&mut self) {
        if !self.dirty {
            return;
        }
        let pass: Vec<bool> = self.messages.iter().map(|m| self.passes(m, RETRIEVAL)).collect();
        let count = |q: &Guid, msgs: &VecDeque<Msg>| msgs.iter().zip(&pass).filter(|(m, ok)| **ok && covers(q, &m.producer)).count() as u64;
        let all = count(&DXGI_DEBUG_ALL, &self.messages);
        let dx = count(&DXGI_DEBUG_DX, &self.messages);
        let counts: Vec<([u8; 16], u64)> = self.entries.iter().map(|(k, e)| (*k, count(&e.producer, &self.messages))).collect();
        self.all.retrieved = all;
        self.dx.retrieved = dx;
        for (k, c) in counts {
            if let Some(e) = self.entries.get_mut(&k) {
                e.retrieved = c;
            }
        }
        self.dirty = false;
    }

    fn sum(&mut self, p: &Guid, f: impl Fn(&Entry) -> u64) -> u64 {
        if is_special(p) {
            self.entries.values().filter(|e| covers(p, &e.producer)).map(f).sum()
        } else {
            f(self.entry(p))
        }
    }

    fn each(&mut self, p: &Guid, f: impl Fn(&mut Entry)) {
        if is_special(p) {
            for e in self.entries.values_mut().filter(|e| covers(p, &e.producer)) {
                f(e);
            }
        }
        f(self.entry(p));
    }

    pub fn set_message_count_limit(&mut self, p: &Guid, limit: u64, fx: &mut Effects) -> i32 {
        if limit == 0 {
            return E_INVALIDARG;
        }
        if is_special(p) {
            self.add_message(&DXGI_DEBUG_DXGI, 1, 2, 0, LIMIT_TEXT.as_bytes(), fx);
            return E_INVALIDARG;
        }
        let e = self.entry(p);
        if e.limit == limit {
            return S_OK;
        }
        if limit != u64::MAX && e.stored > limit {
            let mut remove = e.stored - limit;
            e.discarded += remove;
            e.stored = limit;
            self.dirty = true;
            self.messages.retain(|m| {
                if remove > 0 && m.producer == *p {
                    remove -= 1;
                    false
                } else {
                    true
                }
            });
        }
        self.entry(p).limit = limit;
        S_OK
    }

    pub fn clear_stored_messages(&mut self, p: &Guid) {
        self.messages.retain(|m| !covers(p, &m.producer));
        self.each(p, |e| e.stored = 0);
        self.dirty = true;
    }

    pub fn add_message(&mut self, p: &Guid, cat: i32, sev: i32, id: i32, desc: &[u8], fx: &mut Effects) -> i32 {
        if is_special(p) {
            return E_INVALIDARG;
        }
        let m = Msg { producer: *p, category: cat, severity: sev, id, desc: desc.iter().copied().chain(core::iter::once(0)).collect() };
        self.entry(p);
        if !self.passes(&m, STORAGE) {
            self.entry(p).denied += 1;
            return S_FALSE;
        }
        self.dirty = true;
        let e = self.entry(p);
        e.allowed += 1;
        let full = e.stored == e.limit;
        if full {
            if self.messages.pop_front().is_some() {
                self.entry(p).discarded += 1;
            }
        } else {
            e.stored += 1;
        }
        self.messages.push_back(m);
        let e = self.entry(p);
        let brk = e.break_cat.contains(&cat) || e.break_sev.contains(&sev) || e.break_id.contains(&id);
        let text = String::from_utf8_lossy(desc).into_owned();
        let (pn, cn, sn) = (producer_name(p), category_name(p, cat), severity_name(sev));
        if !e.mute {
            fx.output.push(clip(format!("{pn} {sn}: {text} [ {cn} {sn} #{id}: ]\n")));
            if brk {
                fx.output.push(clip(format!("{pn}: **BREAK** enabled for the previous message, which was: [ {sn} {cn} #{id}:  ]\n")));
            }
        }
        if brk {
            fx.raise = Some((sev, id, desc.iter().copied().chain(core::iter::once(0)).collect()));
        }
        S_OK
    }

    pub unsafe fn get_message(&mut self, p: &Guid, index: u64, out: *mut InfoQueueMessage, len: *mut usize) -> i32 {
        self.refresh();
        let count = self.entry(p).retrieved;
        if index >= count || len.is_null() {
            return E_INVALIDARG;
        }
        let mut seen = 0u64;
        let mut m = None;
        for c in self.messages.iter() {
            if self.passes(c, RETRIEVAL) && covers(p, &c.producer) {
                if seen == index {
                    m = Some(c);
                    break;
                }
                seen += 1;
            }
        }
        let Some(m) = m else {
            return E_INVALIDARG;
        };
        let size = core::mem::size_of::<InfoQueueMessage>() + m.desc.len();
        if out.is_null() {
            *len = size;
            return S_FALSE;
        }
        let have = *len;
        *len = size;
        if have < size {
            return E_INVALIDARG;
        }
        let text = (out as *mut u8).add(core::mem::size_of::<InfoQueueMessage>());
        core::ptr::copy_nonoverlapping(m.desc.as_ptr(), text, m.desc.len());
        *out = InfoQueueMessage { producer: m.producer, category: m.category, severity: m.severity, id: m.id, description: text, description_byte_length: m.desc.len() };
        S_OK
    }

    pub fn num_allowed_by_retrieval(&mut self, p: &Guid) -> u64 {
        self.refresh();
        self.entry(p).retrieved
    }

    pub fn num_stored(&mut self, p: &Guid) -> u64 {
        self.sum(p, |e| e.stored)
    }

    pub fn num_discarded(&mut self, p: &Guid) -> u64 {
        self.sum(p, |e| e.discarded)
    }

    pub fn num_allowed_by_storage(&mut self, p: &Guid) -> u64 {
        self.sum(p, |e| e.allowed)
    }

    pub fn num_denied_by_storage(&mut self, p: &Guid) -> u64 {
        self.sum(p, |e| e.denied)
    }

    pub fn message_count_limit(&mut self, p: &Guid) -> u64 {
        self.entry(p).limit
    }

    fn touched(&mut self, kind: usize) {
        if kind == RETRIEVAL {
            self.dirty = true;
        }
    }

    pub fn add_filter_entries(&mut self, p: &Guid, kind: usize, lists: [Vec<i32>; 6]) -> i32 {
        if self.entry(p).stacks[kind].is_empty() {
            self.push_empty(p, kind);
        }
        let top = self.entry(p).stacks[kind].last_mut().unwrap();
        for (d, s) in top.lists.iter_mut().zip(lists) {
            d.extend(s);
        }
        self.touched(kind);
        S_OK
    }

    pub unsafe fn get_filter(&mut self, p: &Guid, kind: usize, out: *mut InfoQueueFilter, len: *mut usize) -> i32 {
        if len.is_null() {
            return E_INVALIDARG;
        }
        let f = self.entry(p).stacks[kind].last().cloned().unwrap_or_default();
        let total: usize = f.lists.iter().map(|l| l.len()).sum();
        let size = core::mem::size_of::<InfoQueueFilter>() + 4 * total;
        if out.is_null() {
            *len = size;
            return S_FALSE;
        }
        let have = *len;
        *len = size;
        if have < size {
            return E_INVALIDARG;
        }
        let mut cur = (out as *mut u8).add(core::mem::size_of::<InfoQueueFilter>()) as *mut i32;
        let mut ptrs = [core::ptr::null::<i32>(); 6];
        for (i, l) in f.lists.iter().enumerate() {
            if !l.is_empty() {
                core::ptr::copy_nonoverlapping(l.as_ptr(), cur, l.len());
                ptrs[i] = cur;
                cur = cur.add(l.len());
            }
        }
        let n = |i: usize| f.lists[i].len() as u32;
        *out = InfoQueueFilter {
            allow: FilterDesc { num_categories: n(0), categories: ptrs[0], num_severities: n(1), severities: ptrs[1], num_ids: n(2), ids: ptrs[2] },
            deny: FilterDesc { num_categories: n(3), categories: ptrs[3], num_severities: n(4), severities: ptrs[4], num_ids: n(5), ids: ptrs[5] },
        };
        S_OK
    }

    pub fn clear_filter(&mut self, p: &Guid, kind: usize) {
        if self.entry(p).stacks[kind].is_empty() {
            return;
        }
        let seq = self.next_seq();
        *self.entry(p).stacks[kind].last_mut().unwrap() = Filter { lists: Default::default(), seq };
        self.touched(kind);
    }

    pub fn push_empty(&mut self, p: &Guid, kind: usize) -> i32 {
        let seq = self.next_seq();
        self.entry(p).stacks[kind].push(Filter { lists: Default::default(), seq });
        self.touched(kind);
        S_OK
    }

    pub fn push_deny_all(&mut self, p: &Guid, kind: usize) -> i32 {
        self.push_empty(p, kind);
        let mut l: [Vec<i32>; 6] = Default::default();
        l[4] = vec![0, 1, 2, 3, 4];
        self.add_filter_entries(p, kind, l)
    }

    pub fn push_copy(&mut self, p: &Guid, kind: usize) -> i32 {
        let top = self.entry(p).stacks[kind].last().map(|f| f.lists.clone());
        self.push_empty(p, kind);
        match top {
            Some(l) => self.add_filter_entries(p, kind, l),
            None => S_OK,
        }
    }

    pub unsafe fn push_filter(&mut self, p: &Guid, kind: usize, f: *const InfoQueueFilter) -> i32 {
        self.push_empty(p, kind);
        match read_filter(f) {
            Ok(l) => self.add_filter_entries(p, kind, l),
            Err(hr) => {
                self.pop_filter(p, kind);
                hr
            }
        }
    }

    pub fn pop_filter(&mut self, p: &Guid, kind: usize) {
        if self.entry(p).stacks[kind].pop().is_some() {
            self.touched(kind);
        }
    }

    pub fn filter_stack_size(&mut self, p: &Guid, kind: usize) -> u32 {
        self.entry(p).stacks[kind].len() as u32
    }

    pub fn set_break(&mut self, p: &Guid, which: usize, v: i32, on: bool) -> i32 {
        self.each(p, |e| {
            let s = match which {
                0 => &mut e.break_cat,
                1 => &mut e.break_sev,
                _ => &mut e.break_id,
            };
            if on {
                s.insert(v);
            } else {
                s.remove(&v);
            }
        });
        S_OK
    }

    pub fn get_break(&mut self, p: &Guid, which: usize, v: i32) -> bool {
        let e = self.entry(p);
        match which {
            0 => e.break_cat.contains(&v),
            1 => e.break_sev.contains(&v),
            _ => e.break_id.contains(&v),
        }
    }

    pub fn set_mute(&mut self, p: &Guid, mute: bool) {
        self.each(p, |e| e.mute = mute);
    }

    pub fn get_mute(&mut self, p: &Guid) -> bool {
        self.entry(p).mute
    }
}

#[repr(C)]
pub struct InfoQueueObject {
    vtbl: *const Vtbl,
    refs: AtomicU32,
}

static QUEUE: Mutex<Option<Queue>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Queue, &mut Effects) -> R) -> R {
    let mut fx = Effects { output: Vec::new(), raise: None };
    let r = {
        let mut g = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        f(g.get_or_insert_with(Queue::new), &mut fx)
    };
    for s in fx.output {
        let c: Vec<u8> = s.into_bytes().into_iter().chain(core::iter::once(0)).collect();
        unsafe { OutputDebugStringA(c.as_ptr()) };
    }
    if let Some((sev, id, desc)) = fx.raise {
        let idv = id;
        let args = [sev as isize as usize, &idv as *const i32 as usize, desc.as_ptr() as usize];
        unsafe { RaiseException(BREAK_EXCEPTION, 0, 3, args.as_ptr()) };
    }
    r
}

unsafe fn g(p: *const Guid) -> Guid {
    *p
}

unsafe extern "system" fn qi(this: *mut c_void, riid: *const Guid, pp: *mut *mut c_void) -> i32 {
    if pp.is_null() {
        return E_POINTER;
    }
    if !riid.is_null() && (*riid == IID_IUNKNOWN || *riid == IID_IDXGI_INFO_QUEUE) {
        addref(this);
        *pp = this;
        return S_OK;
    }
    *pp = core::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn addref(this: *mut c_void) -> u32 {
    (*(this as *const InfoQueueObject)).refs.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    let r = &(*(this as *const InfoQueueObject)).refs;
    let mut cur = r.load(Ordering::Acquire);
    loop {
        if cur <= 1 {
            return cur;
        }
        match r.compare_exchange(cur, cur - 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return cur - 1,
            Err(v) => cur = v,
        }
    }
}

unsafe extern "system" fn set_message_count_limit(_t: *mut c_void, p: *const Guid, limit: u64) -> i32 {
    let p = g(p);
    with(|q, fx| q.set_message_count_limit(&p, limit, fx))
}

unsafe extern "system" fn clear_stored_messages(_t: *mut c_void, p: *const Guid) {
    let p = g(p);
    with(|q, _| q.clear_stored_messages(&p))
}

unsafe extern "system" fn get_message(_t: *mut c_void, p: *const Guid, index: u64, m: *mut InfoQueueMessage, len: *mut usize) -> i32 {
    let p = g(p);
    with(|q, _| q.get_message(&p, index, m, len))
}

macro_rules! counter {
    ($name:ident, $m:ident) => {
        unsafe extern "system" fn $name(_t: *mut c_void, p: *const Guid) -> u64 {
            let p = g(p);
            with(|q, _| q.$m(&p))
        }
    };
}

counter!(num_allowed_by_retrieval, num_allowed_by_retrieval);
counter!(num_stored, num_stored);
counter!(num_discarded, num_discarded);
counter!(message_count_limit, message_count_limit);
counter!(num_allowed_by_storage, num_allowed_by_storage);
counter!(num_denied_by_storage, num_denied_by_storage);

macro_rules! filter_methods {
    ($kind:expr, $add:ident, $get:ident, $clear:ident, $pe:ident, $pd:ident, $pc:ident, $push:ident, $pop:ident, $size:ident) => {
        unsafe extern "system" fn $add(_t: *mut c_void, p: *const Guid, f: *const InfoQueueFilter) -> i32 {
            let p = g(p);
            match read_filter(f) {
                Ok(l) => with(|q, _| q.add_filter_entries(&p, $kind, l)),
                Err(hr) => hr,
            }
        }
        unsafe extern "system" fn $get(_t: *mut c_void, p: *const Guid, f: *mut InfoQueueFilter, len: *mut usize) -> i32 {
            let p = g(p);
            with(|q, _| q.get_filter(&p, $kind, f, len))
        }
        unsafe extern "system" fn $clear(_t: *mut c_void, p: *const Guid) {
            let p = g(p);
            with(|q, _| q.clear_filter(&p, $kind))
        }
        unsafe extern "system" fn $pe(_t: *mut c_void, p: *const Guid) -> i32 {
            let p = g(p);
            with(|q, _| q.push_empty(&p, $kind))
        }
        unsafe extern "system" fn $pd(_t: *mut c_void, p: *const Guid) -> i32 {
            let p = g(p);
            with(|q, _| q.push_deny_all(&p, $kind))
        }
        unsafe extern "system" fn $pc(_t: *mut c_void, p: *const Guid) -> i32 {
            let p = g(p);
            with(|q, _| q.push_copy(&p, $kind))
        }
        unsafe extern "system" fn $push(_t: *mut c_void, p: *const Guid, f: *const InfoQueueFilter) -> i32 {
            let p = g(p);
            with(|q, _| q.push_filter(&p, $kind, f))
        }
        unsafe extern "system" fn $pop(_t: *mut c_void, p: *const Guid) {
            let p = g(p);
            with(|q, _| q.pop_filter(&p, $kind))
        }
        unsafe extern "system" fn $size(_t: *mut c_void, p: *const Guid) -> u32 {
            let p = g(p);
            with(|q, _| q.filter_stack_size(&p, $kind))
        }
    };
}

filter_methods!(STORAGE, add_storage, get_storage, clear_storage, push_empty_storage, push_deny_storage, push_copy_storage, push_storage, pop_storage, storage_size);
filter_methods!(RETRIEVAL, add_retrieval, get_retrieval, clear_retrieval, push_empty_retrieval, push_deny_retrieval, push_copy_retrieval, push_retrieval, pop_retrieval, retrieval_size);

unsafe extern "system" fn add_message(_t: *mut c_void, p: *const Guid, cat: i32, sev: i32, id: i32, desc: *const u8) -> i32 {
    let p = g(p);
    if desc.is_null() {
        return E_INVALIDARG;
    }
    let d = core::ffi::CStr::from_ptr(desc as *const core::ffi::c_char).to_bytes().to_vec();
    with(|q, fx| q.add_message(&p, cat, sev, id, &d, fx))
}

unsafe extern "system" fn add_application_message(t: *mut c_void, sev: i32, desc: *const u8) -> i32 {
    add_message(t, &DXGI_DEBUG_APP, 0, sev, 0, desc)
}

macro_rules! break_methods {
    ($which:expr, $set:ident, $get:ident) => {
        unsafe extern "system" fn $set(_t: *mut c_void, p: *const Guid, v: i32, on: i32) -> i32 {
            let p = g(p);
            with(|q, _| q.set_break(&p, $which, v, on != 0))
        }
        unsafe extern "system" fn $get(_t: *mut c_void, p: *const Guid, v: i32) -> i32 {
            let p = g(p);
            with(|q, _| q.get_break(&p, $which, v)) as i32
        }
    };
}

break_methods!(0, set_break_category, get_break_category);
break_methods!(1, set_break_severity, get_break_severity);
break_methods!(2, set_break_id, get_break_id);

unsafe extern "system" fn set_mute(_t: *mut c_void, p: *const Guid, mute: i32) {
    let p = g(p);
    with(|q, _| q.set_mute(&p, mute != 0))
}

unsafe extern "system" fn get_mute(_t: *mut c_void, p: *const Guid) -> i32 {
    let p = g(p);
    with(|q, _| q.get_mute(&p)) as i32
}

pub struct Vtbl(pub [*const (); 40]);

unsafe impl Sync for Vtbl {}

pub static VTBL: Vtbl = Vtbl([
    qi as *const (),
    addref as *const (),
    release as *const (),
    set_message_count_limit as *const (),
    clear_stored_messages as *const (),
    get_message as *const (),
    num_allowed_by_retrieval as *const (),
    num_stored as *const (),
    num_discarded as *const (),
    message_count_limit as *const (),
    num_allowed_by_storage as *const (),
    num_denied_by_storage as *const (),
    add_storage as *const (),
    get_storage as *const (),
    clear_storage as *const (),
    push_empty_storage as *const (),
    push_deny_storage as *const (),
    push_copy_storage as *const (),
    push_storage as *const (),
    pop_storage as *const (),
    storage_size as *const (),
    add_retrieval as *const (),
    get_retrieval as *const (),
    clear_retrieval as *const (),
    push_empty_retrieval as *const (),
    push_deny_retrieval as *const (),
    push_copy_retrieval as *const (),
    push_retrieval as *const (),
    pop_retrieval as *const (),
    retrieval_size as *const (),
    add_message as *const (),
    add_application_message as *const (),
    set_break_category as *const (),
    set_break_severity as *const (),
    set_break_id as *const (),
    get_break_category as *const (),
    get_break_severity as *const (),
    get_break_id as *const (),
    set_mute as *const (),
    get_mute as *const (),

]);

static OBJECT: Mutex<usize> = Mutex::new(0);

pub fn singleton() -> *mut c_void {
    let mut o = OBJECT.lock().unwrap_or_else(|e| e.into_inner());
    if *o == 0 {
        *o = Box::into_raw(Box::new(InfoQueueObject { vtbl: &VTBL, refs: AtomicU32::new(1) })) as usize;
    }
    *o as *mut c_void
}

pub unsafe fn get_debug_interface(riid: *const Guid, pp: *mut *mut c_void) -> i32 {
    if *riid != IID_IDXGI_INFO_QUEUE {
        return E_NOTIMPL;
    }
    let o = singleton();
    addref(o);
    *pp = o;
    S_OK
}

#[macro_export]
macro_rules! umd_debug_info_queue_export {
    () => {
        #[no_mangle]
        pub unsafe extern "system" fn UMDGetDebugInterface(riid: *const core::ffi::c_void, pp: *mut *mut core::ffi::c_void) -> i32 {
            $crate::infoqueue::get_debug_interface(riid as *const $crate::Guid, pp)
        }

        #[cfg(test)]
        mod umd_debug_info_queue_tests {
            #[test]
            fn debug_interface_is_info_queue_singleton() {
                let mut a: *mut core::ffi::c_void = core::ptr::null_mut();
                let mut b: *mut core::ffi::c_void = 5 as *mut core::ffi::c_void;
                let iid = $crate::infoqueue::IID_IDXGI_INFO_QUEUE;
                let other = $crate::infoqueue::IID_IUNKNOWN;
                unsafe {
                    assert_eq!(super::UMDGetDebugInterface(&other as *const _ as *const core::ffi::c_void, &mut b), $crate::E_NOTIMPL);
                    assert_eq!(b as usize, 5);
                    assert_eq!(super::UMDGetDebugInterface(&iid as *const _ as *const core::ffi::c_void, &mut a), $crate::S_OK);
                    assert_eq!(super::UMDGetDebugInterface(&iid as *const _ as *const core::ffi::c_void, &mut b), $crate::S_OK);
                    assert_eq!(a, b);
                    let v = *(a as *const *const usize);
                    let limit: unsafe extern "system" fn(*mut core::ffi::c_void, *const $crate::Guid) -> u64 = core::mem::transmute(*v.add(9));
                    assert_eq!(limit(a, &$crate::infoqueue::DXGI_DEBUG_DXGI), $crate::infoqueue::DEFAULT_LIMIT);
                    let rel: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 = core::mem::transmute(*v.add(2));
                    assert!(rel(a) >= 2);
                    rel(b);
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: Guid = DXGI_DEBUG_DXGI;
    const A: Guid = DXGI_DEBUG_APP;
    const P: Guid = Guid::new(0x1111_2222, 0x3333, 0x4444, [1, 2, 3, 4, 5, 6, 7, 8]);

    fn fx() -> Effects {
        Effects { output: Vec::new(), raise: None }
    }

    unsafe fn get(q: &mut Queue, p: &Guid, i: u64) -> Result<(i32, i32, i32, Guid, String), i32> {
        let mut len = 0usize;
        let hr = q.get_message(p, i, core::ptr::null_mut(), &mut len);
        if hr != S_FALSE {
            return Err(hr);
        }
        let mut buf = vec![0u64; len / 8 + 1];
        let m = buf.as_mut_ptr() as *mut InfoQueueMessage;
        let mut small = len - 1;
        assert_eq!(q.get_message(p, i, m, &mut small), E_INVALIDARG);
        assert_eq!(small, len);
        assert_eq!(q.get_message(p, i, m, &mut len), S_OK);
        let m = &*m;
        let s = core::ffi::CStr::from_ptr(m.description as *const core::ffi::c_char).to_string_lossy().into_owned();
        assert_eq!(m.description_byte_length, s.len() + 1);
        Ok((m.category, m.severity, m.id, m.producer, s))
    }

    #[test]
    fn add_get_and_output_text() {
        let mut q = Queue::new();
        let mut f = fx();
        assert_eq!(q.add_message(&D, 1, 2, 7, b"hello", &mut f), S_OK);
        assert_eq!(f.output, vec!["DXGI WARNING: hello [ MISCELLANEOUS WARNING #7: ]\n".to_string()]);
        let mut f = fx();
        assert_eq!(q.add_message(&A, 0, 4, 0, b"app", &mut f), S_OK);
        assert_eq!(f.output, vec!["Application MESSAGE: app [ APPLICATION_DEFINED MESSAGE #0: ]\n".to_string()]);
        let mut f = fx();
        q.add_message(&P, 10, 1, 3, b"x", &mut f);
        assert_eq!(f.output[0], "DX ERROR: x [ SHADER ERROR #3: ]\n");
        assert_eq!(q.add_message(&DXGI_DEBUG_ALL, 0, 0, 0, b"x", &mut f), E_INVALIDARG);
        assert_eq!(q.add_message(&DXGI_DEBUG_DX, 0, 0, 0, b"x", &mut f), E_INVALIDARG);
        unsafe {
            assert_eq!(get(&mut q, &D, 0).unwrap(), (1, 2, 7, D, "hello".into()));
            assert_eq!(get(&mut q, &DXGI_DEBUG_ALL, 1).unwrap().4, "app");
            assert_eq!(get(&mut q, &DXGI_DEBUG_DX, 1).unwrap().4, "x");
            assert_eq!(get(&mut q, &D, 1), Err(E_INVALIDARG));
            let mut len = 0usize;
            assert_eq!(q.get_message(&D, 0, core::ptr::null_mut(), core::ptr::null_mut()), E_INVALIDARG);
            assert_eq!(q.get_message(&A, 0, core::ptr::null_mut(), &mut len), S_FALSE);
            assert_eq!(len, 0x30 + 4);
        }
        assert_eq!(q.num_stored(&D), 1);
        assert_eq!(q.num_stored(&DXGI_DEBUG_ALL), 3);
        assert_eq!(q.num_stored(&DXGI_DEBUG_DX), 2);
        assert_eq!(q.num_allowed_by_retrieval(&DXGI_DEBUG_ALL), 3);
        assert_eq!(q.num_allowed_by_storage(&D), 1);
        q.clear_stored_messages(&DXGI_DEBUG_DX);
        assert_eq!(q.num_stored(&DXGI_DEBUG_ALL), 1);
        assert_eq!(q.num_allowed_by_retrieval(&A), 1);
    }

    #[test]
    fn count_limit_rules() {
        let mut q = Queue::new();
        let mut f = fx();
        assert_eq!(q.message_count_limit(&D), 0x400);
        assert_eq!(q.set_message_count_limit(&D, 0, &mut f), E_INVALIDARG);
        assert_eq!(q.set_message_count_limit(&DXGI_DEBUG_ALL, 5, &mut f), E_INVALIDARG);
        assert_eq!(f.output, vec![format!("DXGI WARNING: {LIMIT_TEXT} [ MISCELLANEOUS WARNING #0: ]\n")]);
        assert_eq!(q.num_stored(&D), 1);
        assert_eq!(q.set_message_count_limit(&DXGI_DEBUG_DX, 5, &mut f), E_INVALIDARG);
        assert_eq!(q.num_stored(&D), 2);
        for i in 0..3 {
            q.add_message(&D, 0, 3, i, b"m", &mut f);
        }
        assert_eq!(q.num_stored(&D), 5);
        assert_eq!(q.set_message_count_limit(&D, 2, &mut f), S_OK);
        assert_eq!(q.num_stored(&D), 2);
        assert_eq!(q.num_discarded(&D), 3);
        unsafe {
            assert_eq!(get(&mut q, &D, 0).unwrap().2, 1);
        }
        q.add_message(&D, 0, 3, 9, b"m", &mut f);
        assert_eq!(q.num_stored(&D), 2);
        assert_eq!(q.num_discarded(&D), 4);
        unsafe {
            assert_eq!(get(&mut q, &D, 0).unwrap().2, 2);
            assert_eq!(get(&mut q, &D, 1).unwrap().2, 9);
        }
        assert_eq!(q.set_message_count_limit(&D, u64::MAX, &mut f), S_OK);
        assert_eq!(q.message_count_limit(&D), u64::MAX);
    }

    #[test]
    fn storage_and_retrieval_filters() {
        let mut q = Queue::new();
        let mut f = fx();
        assert_eq!(q.filter_stack_size(&D, STORAGE), 0);
        let mut deny: [Vec<i32>; 6] = Default::default();
        deny[4] = vec![3];
        assert_eq!(q.add_filter_entries(&D, STORAGE, deny), S_OK);
        assert_eq!(q.filter_stack_size(&D, STORAGE), 1);
        assert_eq!(q.add_message(&D, 0, 3, 1, b"info", &mut f), S_FALSE);
        assert!(f.output.is_empty());
        assert_eq!(q.add_message(&D, 0, 2, 2, b"warn", &mut f), S_OK);
        assert_eq!(q.num_denied_by_storage(&D), 1);
        assert_eq!(q.num_allowed_by_storage(&D), 1);
        q.push_deny_all(&DXGI_DEBUG_DX, STORAGE);
        assert_eq!(q.add_message(&D, 0, 2, 3, b"w", &mut f), S_FALSE);
        assert_eq!(q.add_message(&A, 0, 2, 3, b"w", &mut f), S_OK);
        q.push_empty(&D, STORAGE);
        assert_eq!(q.add_message(&D, 0, 3, 4, b"i", &mut f), S_OK);
        q.pop_filter(&D, STORAGE);
        q.pop_filter(&DXGI_DEBUG_DX, STORAGE);
        assert_eq!(q.add_message(&D, 0, 3, 5, b"i", &mut f), S_FALSE);
        q.clear_filter(&D, STORAGE);
        assert_eq!(q.add_message(&D, 0, 3, 6, b"i", &mut f), S_OK);
        let mut allow: [Vec<i32>; 6] = Default::default();
        allow[0] = vec![9];
        allow[2] = vec![4];
        q.add_filter_entries(&D, RETRIEVAL, allow);
        assert_eq!(q.num_allowed_by_retrieval(&D), 1);
        unsafe {
            assert_eq!(get(&mut q, &D, 0).unwrap().2, 4);
            assert_eq!(q.num_allowed_by_retrieval(&DXGI_DEBUG_ALL), 2);
            assert_eq!(get(&mut q, &DXGI_DEBUG_ALL, 0).unwrap().2, 3);
            assert_eq!(get(&mut q, &DXGI_DEBUG_ALL, 1).unwrap().2, 4);
            let mut len = 0usize;
            assert_eq!(q.get_filter(&D, RETRIEVAL, core::ptr::null_mut(), &mut len), S_FALSE);
            assert_eq!(len, 0x60 + 8);
            let mut buf = vec![0u64; len / 8 + 1];
            let out = buf.as_mut_ptr() as *mut InfoQueueFilter;
            let mut small = len - 4;
            assert_eq!(q.get_filter(&D, RETRIEVAL, out, &mut small), E_INVALIDARG);
            assert_eq!(q.get_filter(&D, RETRIEVAL, out, &mut len), S_OK);
            assert_eq!(read_filter(out).unwrap(), [vec![9], vec![], vec![4], vec![], vec![], vec![]]);
            assert_eq!(q.get_filter(&D, RETRIEVAL, out, core::ptr::null_mut()), E_INVALIDARG);
            assert_eq!(q.push_copy(&D, RETRIEVAL), S_OK);
            assert_eq!(q.filter_stack_size(&D, RETRIEVAL), 2);
            assert_eq!(q.push_filter(&D, RETRIEVAL, core::ptr::null()), E_INVALIDARG);
            assert_eq!(q.filter_stack_size(&D, RETRIEVAL), 2);
            let bad = InfoQueueFilter {
                allow: FilterDesc { num_categories: 1, categories: core::ptr::null(), num_severities: 0, severities: core::ptr::null(), num_ids: 0, ids: core::ptr::null() },
                deny: FilterDesc { num_categories: 0, categories: core::ptr::null(), num_severities: 0, severities: core::ptr::null(), num_ids: 0, ids: core::ptr::null() },
            };
            assert!(read_filter(&bad).is_err());
            let sev = [1i32];
            let ok = InfoQueueFilter {
                allow: FilterDesc { num_categories: 0, categories: core::ptr::null(), num_severities: 1, severities: sev.as_ptr(), num_ids: 0, ids: core::ptr::null() },
                deny: FilterDesc { num_categories: 0, categories: core::ptr::null(), num_severities: 0, severities: core::ptr::null(), num_ids: 0, ids: core::ptr::null() },
            };
            assert_eq!(q.push_filter(&D, RETRIEVAL, &ok), S_OK);
            assert_eq!(q.num_allowed_by_retrieval(&D), 0);
        }
    }

    #[test]
    fn break_and_mute_settings() {
        let mut q = Queue::new();
        let mut f = fx();
        q.add_message(&D, 0, 3, 0, b"seed", &mut f);
        q.add_message(&A, 0, 3, 0, b"seed", &mut f);
        assert_eq!(q.set_break(&DXGI_DEBUG_DX, 1, 1, true), S_OK);
        assert!(q.get_break(&D, 1, 1));
        assert!(!q.get_break(&A, 1, 1));
        assert!(q.get_break(&DXGI_DEBUG_DX, 1, 1));
        let mut f = fx();
        q.add_message(&D, 5, 1, 42, b"bad", &mut f);
        assert_eq!(f.output[1], "DXGI: **BREAK** enabled for the previous message, which was: [ ERROR STATE_CREATION #42:  ]\n");
        assert_eq!(f.raise, Some((1, 42, b"bad\0".to_vec())));
        q.set_mute(&D, true);
        assert!(q.get_mute(&D));
        let mut f = fx();
        q.add_message(&D, 0, 1, 1, b"quiet", &mut f);
        assert!(f.output.is_empty());
        assert!(f.raise.is_some());
        q.set_break(&D, 1, 1, false);
        q.set_break(&D, 2, 77, true);
        assert!(q.get_break(&D, 2, 77));
        q.set_break(&D, 0, 9, true);
        assert!(q.get_break(&D, 0, 9));
        let mut f = fx();
        q.add_message(&D, 0, 1, 1, b"x", &mut f);
        assert!(f.raise.is_none());
        q.set_mute(&DXGI_DEBUG_ALL, false);
        assert!(!q.get_mute(&D));
        assert!(!q.get_mute(&A));
    }

    extern "system" {
        fn AddVectoredExceptionHandler(first: u32, h: unsafe extern "system" fn(*mut *mut u32) -> i32) -> *mut c_void;
        fn RemoveVectoredExceptionHandler(h: *mut c_void) -> u32;
    }

    static RAISED: AtomicU32 = AtomicU32::new(0);

    unsafe extern "system" fn handler(info: *mut *mut u32) -> i32 {
        let rec = *info;
        if *rec == BREAK_EXCEPTION {
            RAISED.fetch_add(1, Ordering::SeqCst);
            return -1;
        }
        0
    }

    #[test]
    fn com_object_raises_break_exception() {
        let _g = crate::serial();
        let o = singleton();
        let v = unsafe { *(o as *const *const usize) };
        unsafe {
            let h = AddVectoredExceptionHandler(1, handler);
            let set_sev: unsafe extern "system" fn(*mut c_void, *const Guid, i32, i32) -> i32 = core::mem::transmute(*v.add(33));
            let app: unsafe extern "system" fn(*mut c_void, i32, *const u8) -> i32 = core::mem::transmute(*v.add(31));
            let get_sev: unsafe extern "system" fn(*mut c_void, *const Guid, i32) -> i32 = core::mem::transmute(*v.add(36));
            let mute: unsafe extern "system" fn(*mut c_void, *const Guid, i32) = core::mem::transmute(*v.add(38));
            let stored: unsafe extern "system" fn(*mut c_void, *const Guid) -> u64 = core::mem::transmute(*v.add(7));
            let before = stored(o, &A);
            mute(o, &A, 1);
            assert_eq!(set_sev(o, &A, 0, 1), S_OK);
            assert_eq!(get_sev(o, &A, 0), 1);
            assert_eq!(app(o, 0, b"boom\0".as_ptr()), S_OK);
            assert_eq!(RAISED.load(Ordering::SeqCst), 1);
            assert_eq!(app(o, 4, core::ptr::null()), E_INVALIDARG);
            assert_eq!(stored(o, &A), before + 1);
            set_sev(o, &A, 0, 0);
            RemoveVectoredExceptionHandler(h);
            let mut p: *mut c_void = core::ptr::null_mut();
            let qi: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 = core::mem::transmute(*v);
            assert_eq!(qi(o, &IID_IDXGI_INFO_QUEUE, &mut p), S_OK);
            assert_eq!(p, o);
            assert_eq!(qi(o, &P, &mut p), E_NOINTERFACE);
            assert!(p.is_null());
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(*v.add(2));
            rel(o);
        }
    }
}
