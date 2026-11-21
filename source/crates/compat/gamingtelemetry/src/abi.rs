use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicI32, Ordering};

extern "C" {
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
}

pub unsafe fn crt_malloc(n: usize) -> *mut u8 {
    malloc(n) as *mut u8
}

pub unsafe fn crt_alloc(n: usize) -> *mut u8 {
    let p = malloc(n) as *mut u8;
    if p.is_null() {
        std::process::abort();
    }
    p
}

pub unsafe fn crt_free(p: *mut u8) {
    if !p.is_null() {
        free(p as *mut c_void)
    }
}

pub unsafe fn std_alloc(n: usize) -> *mut u8 {
    if n == 0 {
        return null_mut();
    }
    if n >= 0x1000 {
        let raw = crt_alloc(n + 0x27) as usize;
        let p = (raw + 0x27) & !0x1f;
        *((p - 8) as *mut usize) = raw;
        p as *mut u8
    } else {
        crt_alloc(n)
    }
}

pub unsafe fn std_free(p: *mut u8, n: usize) {
    if p.is_null() {
        return;
    }
    if n >= 0x1000 {
        crt_free(*((p as usize - 8) as *const usize) as *mut u8)
    } else {
        crt_free(p)
    }
}

#[repr(C)]
pub struct CtrlVtbl {
    pub destroy: unsafe extern "C" fn(*mut Ctrl),
    pub delete_this: unsafe extern "C" fn(*mut Ctrl),
    pub get_deleter: unsafe extern "C" fn(*const Ctrl, *const c_void) -> *mut c_void,
}

#[repr(C)]
pub struct Ctrl {
    pub vtbl: *const CtrlVtbl,
    pub uses: AtomicI32,
    pub weaks: AtomicI32,
    pub res: *mut u8,
}

unsafe extern "C" fn ctrl_destroy(c: *mut Ctrl) {
    crt_free((*c).res)
}

unsafe extern "C" fn ctrl_delete(c: *mut Ctrl) {
    crt_free(c as *mut u8)
}

unsafe extern "C" fn ctrl_deleter(_: *const Ctrl, _: *const c_void) -> *mut c_void {
    null_mut()
}

pub static CTRL_VTBL: CtrlVtbl = CtrlVtbl { destroy: ctrl_destroy, delete_this: ctrl_delete, get_deleter: ctrl_deleter };

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Sp {
    pub ptr: *mut u8,
    pub ctrl: *mut Ctrl,
}

impl Sp {
    pub const NULL: Sp = Sp { ptr: null_mut(), ctrl: null_mut() };

    pub unsafe fn from_bytes(b: &[u8]) -> Sp {
        let p = crt_alloc(b.len().max(1));
        std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len());
        let c = crt_alloc(std::mem::size_of::<Ctrl>()) as *mut Ctrl;
        c.write(Ctrl { vtbl: &CTRL_VTBL, uses: AtomicI32::new(1), weaks: AtomicI32::new(1), res: p });
        Sp { ptr: p, ctrl: c }
    }

    pub unsafe fn wide(s: &[u16]) -> Sp {
        let mut b = Vec::with_capacity(s.len() * 2 + 2);
        for c in s {
            b.extend_from_slice(&c.to_le_bytes());
        }
        b.extend_from_slice(&[0, 0]);
        Sp::from_bytes(&b)
    }

    pub unsafe fn narrow(s: &[u8]) -> Sp {
        let mut b = s.to_vec();
        b.push(0);
        Sp::from_bytes(&b)
    }

    pub unsafe fn name(p: *const u16) -> Sp {
        if p.is_null() {
            Sp::NULL
        } else {
            Sp::wide(wstr(p))
        }
    }

    pub unsafe fn clone_ref(&self) -> Sp {
        if !self.ctrl.is_null() {
            (*self.ctrl).uses.fetch_add(1, Ordering::Relaxed);
        }
        *self
    }

    pub unsafe fn release(&self) {
        let c = self.ctrl;
        if c.is_null() {
            return;
        }
        if (*c).uses.fetch_sub(1, Ordering::AcqRel) == 1 {
            ((*(*c).vtbl).destroy)(c);
            if (*c).weaks.fetch_sub(1, Ordering::AcqRel) == 1 {
                ((*(*c).vtbl).delete_this)(c);
            }
        }
    }

    pub unsafe fn wtext<'a>(&self) -> &'a [u16] {
        wstr(self.ptr as *const u16)
    }
}

pub unsafe fn wlen(p: *const u16) -> usize {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    n
}

pub unsafe fn wstr<'a>(p: *const u16) -> &'a [u16] {
    if p.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(p, wlen(p))
    }
}

pub unsafe fn cstr<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    std::slice::from_raw_parts(p, n)
}

pub fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn wz(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub unsafe fn sp_less(a: &Sp, b: &Sp) -> bool {
    if b.ptr.is_null() {
        return true;
    }
    if a.ptr.is_null() {
        return false;
    }
    a.wtext() < b.wtext()
}

#[repr(C)]
pub struct Node {
    pub left: *mut Node,
    pub parent: *mut Node,
    pub right: *mut Node,
    pub color: u8,
    pub isnil: u8,
}

pub const PRIM_NODE: usize = 0x48;
pub const BVAR_NODE: usize = 0x58;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RawMap {
    pub head: *mut Node,
    pub size: usize,
}

pub unsafe fn nkey(n: *mut Node) -> *mut Sp {
    (n as *mut u8).add(0x20) as *mut Sp
}

pub unsafe fn nval<T>(n: *mut Node) -> *mut T {
    (n as *mut u8).add(0x30) as *mut T
}

impl RawMap {
    pub unsafe fn new(ns: usize) -> RawMap {
        let h = crt_alloc(ns) as *mut Node;
        std::ptr::write_bytes(h as *mut u8, 0, ns);
        (*h).left = h;
        (*h).parent = h;
        (*h).right = h;
        (*h).color = 1;
        (*h).isnil = 1;
        RawMap { head: h, size: 0 }
    }

    pub unsafe fn nodes(&self) -> Vec<*mut Node> {
        let mut out = Vec::new();
        let head = self.head;
        let mut n = (*head).left;
        while n != head && (*n).isnil == 0 {
            out.push(n);
            if (*(*n).right).isnil == 0 {
                n = (*n).right;
                while (*(*n).left).isnil == 0 {
                    n = (*n).left;
                }
            } else {
                let mut p = (*n).parent;
                while (*p).isnil == 0 && n == (*p).right {
                    n = p;
                    p = (*p).parent;
                }
                n = p;
            }
        }
        out
    }

    unsafe fn lower(&self, k: &Sp) -> (*mut Node, bool, *mut Node) {
        let head = self.head;
        let mut par = (*head).parent;
        let mut right = true;
        let mut bound = head;
        let mut t = par;
        while (*t).isnil == 0 {
            par = t;
            if sp_less(&*nkey(t), k) {
                right = true;
                t = (*t).right;
            } else {
                right = false;
                bound = t;
                t = (*t).left;
            }
        }
        (par, right, bound)
    }

    pub unsafe fn find(&self, k: &Sp) -> Option<*mut Node> {
        let (_, _, b) = self.lower(k);
        if (*b).isnil == 0 && !sp_less(k, &*nkey(b)) {
            Some(b)
        } else {
            None
        }
    }

    pub unsafe fn insert(&mut self, ns: usize, key: Sp, val: &[u8]) -> (*mut Node, bool) {
        let (par, right, b) = self.lower(&key);
        if (*b).isnil == 0 && !sp_less(&key, &*nkey(b)) {
            return (b, false);
        }
        let n = crt_alloc(ns) as *mut Node;
        std::ptr::write_bytes(n as *mut u8, 0, ns);
        nkey(n).write(key);
        std::ptr::copy_nonoverlapping(val.as_ptr(), nval::<u8>(n), val.len().min(ns - 0x30));
        self.link(par, right, n);
        (n, true)
    }

    unsafe fn link(&mut self, par: *mut Node, right: bool, node: *mut Node) {
        let head = self.head;
        (*node).parent = par;
        (*node).left = head;
        (*node).right = head;
        (*node).color = 0;
        (*node).isnil = 0;
        self.size += 1;
        if par == head {
            (*head).parent = node;
            (*head).left = node;
            (*head).right = node;
        } else if right {
            (*par).right = node;
            if par == (*head).right {
                (*head).right = node;
            }
        } else {
            (*par).left = node;
            if par == (*head).left {
                (*head).left = node;
            }
        }
        let mut p = node;
        while (*(*p).parent).color == 0 {
            let pp = (*p).parent;
            let g = (*pp).parent;
            if pp == (*g).left {
                let u = (*g).right;
                if (*u).color == 0 {
                    (*pp).color = 1;
                    (*u).color = 1;
                    (*g).color = 0;
                    p = g;
                } else {
                    if p == (*pp).right {
                        p = pp;
                        self.lrotate(p);
                    }
                    (*(*p).parent).color = 1;
                    (*(*(*p).parent).parent).color = 0;
                    self.rrotate((*(*p).parent).parent);
                }
            } else {
                let u = (*g).left;
                if (*u).color == 0 {
                    (*pp).color = 1;
                    (*u).color = 1;
                    (*g).color = 0;
                    p = g;
                } else {
                    if p == (*pp).left {
                        p = pp;
                        self.rrotate(p);
                    }
                    (*(*p).parent).color = 1;
                    (*(*(*p).parent).parent).color = 0;
                    self.lrotate((*(*p).parent).parent);
                }
            }
        }
        (*(*head).parent).color = 1;
    }

    unsafe fn lrotate(&mut self, w: *mut Node) {
        let head = self.head;
        let p = (*w).right;
        (*w).right = (*p).left;
        if (*(*p).left).isnil == 0 {
            (*(*p).left).parent = w;
        }
        (*p).parent = (*w).parent;
        if w == (*head).parent {
            (*head).parent = p;
        } else if w == (*(*w).parent).left {
            (*(*w).parent).left = p;
        } else {
            (*(*w).parent).right = p;
        }
        (*p).left = w;
        (*w).parent = p;
    }

    unsafe fn rrotate(&mut self, w: *mut Node) {
        let head = self.head;
        let p = (*w).left;
        (*w).left = (*p).right;
        if (*(*p).right).isnil == 0 {
            (*(*p).right).parent = w;
        }
        (*p).parent = (*w).parent;
        if w == (*head).parent {
            (*head).parent = p;
        } else if w == (*(*w).parent).right {
            (*(*w).parent).right = p;
        } else {
            (*(*w).parent).left = p;
        }
        (*p).right = w;
        (*w).parent = p;
    }

    pub unsafe fn destroy(&self, drop_val: unsafe fn(*mut Node)) {
        if self.head.is_null() {
            return;
        }
        unsafe fn erase(n: *mut Node, d: unsafe fn(*mut Node)) {
            let mut n = n;
            while (*n).isnil == 0 {
                erase((*n).right, d);
                let l = (*n).left;
                (*nkey(n)).release();
                d(n);
                crt_free(n as *mut u8);
                n = l;
            }
        }
        erase((*self.head).parent, drop_val);
        crt_free(self.head as *mut u8);
    }
}

pub unsafe fn bytes_of<T>(v: &T) -> &[u8] {
    std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>())
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Prim {
    pub d: [u64; 2],
    pub idx: i8,
    pub pad: [u8; 7],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Coll {
    pub d: [u64; 3],
    pub idx: i8,
    pub pad: [u8; 7],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BVar {
    pub d: [u64; 4],
    pub idx: i8,
    pub pad: [u8; 7],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Value {
    pub name: Sp,
    pub a: [u64; 5],
    pub aidx: i8,
    pub pad: [u8; 7],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MapValue {
    pub name: Sp,
    pub b: BVar,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PrimMapValue {
    pub name: Sp,
    pub p: Prim,
}

#[repr(C)]
pub struct Span<T> {
    pub first: *const T,
    pub last: *const T,
}

pub unsafe fn span_items<'a, T>(s: *const Span<T>) -> &'a [T] {
    if s.is_null() {
        return &[];
    }
    let s = &*s;
    if s.first.is_null() || s.last <= s.first {
        return &[];
    }
    std::slice::from_raw_parts(s.first, (s.last as usize - s.first as usize) / std::mem::size_of::<T>())
}

pub static EMPTY_W: [u16; 1] = [0];

impl Prim {
    pub fn new(idx: i8, d0: u64, d1: u64) -> Prim {
        Prim { d: [d0, d1], idx, pad: [0; 7] }
    }
    pub fn sp(idx: i8, s: Sp) -> Prim {
        Prim::new(idx, s.ptr as u64, s.ctrl as u64)
    }
    pub fn wref(p: *const u16) -> Prim {
        Prim::new(1, p as u64, 0)
    }
    pub fn empty_w() -> Prim {
        Prim::wref(EMPTY_W.as_ptr())
    }
    pub fn nref(p: *const u8) -> Prim {
        Prim::new(0, p as u64, 0)
    }
    pub fn u8v(v: u8) -> Prim {
        Prim::new(5, v as u64, 0)
    }
    pub fn i32v(v: i32) -> Prim {
        Prim::new(8, v as u32 as u64, 0)
    }
    pub fn u32v(v: u32) -> Prim {
        Prim::new(9, v as u64, 0)
    }
    pub fn u64v(v: u64) -> Prim {
        Prim::new(10, v, 0)
    }
    pub fn i64v(v: i64) -> Prim {
        Prim::new(11, v as u64, 0)
    }
    pub fn f64v(v: f64) -> Prim {
        Prim::new(13, v.to_bits(), 0)
    }
    pub fn boolv(v: bool) -> Prim {
        Prim::new(14, v as u64, 0)
    }
    pub fn guid(g: &crate::sys::Guid) -> Prim {
        let b = g.bytes();
        Prim::new(15, u64::from_le_bytes(b[0..8].try_into().unwrap()), u64::from_le_bytes(b[8..16].try_into().unwrap()))
    }
    pub fn as_sp(&self) -> Sp {
        Sp { ptr: self.d[0] as *mut u8, ctrl: self.d[1] as *mut Ctrl }
    }
    pub unsafe fn clone_ref(&self) -> Prim {
        if self.idx == 2 || self.idx == 3 {
            self.as_sp().clone_ref();
        }
        *self
    }
    pub unsafe fn drop_ref(&self) {
        if self.idx == 2 || self.idx == 3 {
            self.as_sp().release();
        }
    }
}

unsafe fn drop_prim_node(n: *mut Node) {
    (*nval::<Prim>(n)).drop_ref()
}

unsafe fn drop_bvar_node(n: *mut Node) {
    bvar_drop(&*nval::<BVar>(n))
}

pub unsafe fn prim_map_destroy(m: &RawMap) {
    m.destroy(drop_prim_node)
}

pub unsafe fn bvar_map_destroy(m: &RawMap) {
    m.destroy(drop_bvar_node)
}

pub unsafe fn prim_map_put(m: &mut RawMap, key: Sp, p: Prim, assign: bool) {
    let (n, inserted) = m.insert(PRIM_NODE, key, bytes_of(&p));
    if !inserted {
        key.release();
        if assign {
            (*nval::<Prim>(n)).drop_ref();
            nval::<Prim>(n).write(p);
        } else {
            p.drop_ref();
        }
    }
}

pub unsafe fn bvar_map_put(m: &mut RawMap, key: Sp, b: BVar) {
    let (_, inserted) = m.insert(BVAR_NODE, key, bytes_of(&b));
    if !inserted {
        key.release();
        bvar_drop(&b);
    }
}

pub unsafe fn prim_map_clone(m: &RawMap) -> RawMap {
    let mut out = RawMap::new(PRIM_NODE);
    for n in m.nodes() {
        prim_map_put(&mut out, (*nkey(n)).clone_ref(), (*nval::<Prim>(n)).clone_ref(), false);
    }
    out
}

pub unsafe fn bvar_map_clone(m: &RawMap) -> RawMap {
    let mut out = RawMap::new(BVAR_NODE);
    for n in m.nodes() {
        bvar_map_put(&mut out, (*nkey(n)).clone_ref(), bvar_clone(&*nval::<BVar>(n)));
    }
    out
}

pub unsafe fn raw_vec<T: Copy>(items: &[T]) -> [u64; 3] {
    let bytes = std::mem::size_of_val(items);
    let p = std_alloc(bytes);
    if bytes > 0 {
        std::ptr::copy_nonoverlapping(items.as_ptr() as *const u8, p, bytes);
    }
    [p as u64, p as u64 + bytes as u64, p as u64 + bytes as u64]
}

pub unsafe fn vec_items<'a, T>(d: &[u64; 3]) -> &'a [T] {
    if d[0] == 0 || d[1] <= d[0] {
        return &[];
    }
    std::slice::from_raw_parts(d[0] as *const T, (d[1] - d[0]) as usize / std::mem::size_of::<T>())
}

pub unsafe fn vec_free(d: &[u64; 3]) {
    std_free(d[0] as *mut u8, (d[2] - d[0]) as usize)
}

impl Coll {
    pub fn new(idx: i8, d: [u64; 3]) -> Coll {
        Coll { d, idx, pad: [0; 7] }
    }
    pub fn map(m: RawMap) -> Coll {
        Coll::new(0, [m.head as u64, m.size as u64, 0])
    }
    pub fn as_map(&self) -> RawMap {
        RawMap { head: self.d[0] as *mut Node, size: self.d[1] as usize }
    }
}

pub unsafe fn coll_clone(c: &Coll) -> Coll {
    match c.idx {
        0 => Coll::map(prim_map_clone(&c.as_map())),
        1 => {
            let v: Vec<Prim> = vec_items::<Prim>(&c.d).iter().map(|p| p.clone_ref()).collect();
            Coll::new(1, raw_vec(&v))
        }
        2 => {
            let v: Vec<RawMap> = vec_items::<RawMap>(&c.d).iter().map(|m| prim_map_clone(m)).collect();
            Coll::new(2, raw_vec(&v))
        }
        _ => *c,
    }
}

pub unsafe fn coll_drop(c: &Coll) {
    match c.idx {
        0 => prim_map_destroy(&c.as_map()),
        1 => {
            for p in vec_items::<Prim>(&c.d) {
                p.drop_ref();
            }
            vec_free(&c.d);
        }
        2 => {
            for m in vec_items::<RawMap>(&c.d) {
                prim_map_destroy(m);
            }
            vec_free(&c.d);
        }
        _ => {}
    }
}

impl BVar {
    pub fn from_prim(p: Prim) -> BVar {
        let mut b = BVar { d: [0; 4], idx: 0, pad: [0; 7] };
        unsafe { (&mut b as *mut BVar as *mut Prim).write(p) };
        b
    }
    pub fn from_coll(c: Coll) -> BVar {
        let mut b = BVar { d: [0; 4], idx: 1, pad: [0; 7] };
        unsafe { (&mut b as *mut BVar as *mut Coll).write(c) };
        b
    }
    pub fn prim(&self) -> &Prim {
        unsafe { &*(self as *const BVar as *const Prim) }
    }
    pub fn coll(&self) -> &Coll {
        unsafe { &*(self as *const BVar as *const Coll) }
    }
}

pub unsafe fn bvar_clone(b: &BVar) -> BVar {
    match b.idx {
        0 => BVar::from_prim(b.prim().clone_ref()),
        1 => BVar::from_coll(coll_clone(b.coll())),
        _ => *b,
    }
}

pub unsafe fn bvar_drop(b: &BVar) {
    match b.idx {
        0 => b.prim().drop_ref(),
        1 => coll_drop(b.coll()),
        _ => {}
    }
}

impl Value {
    pub fn with_bvar(name: Sp, b: BVar) -> Value {
        let mut v = Value { name, a: [0; 5], aidx: 0, pad: [0; 7] };
        unsafe { (v.a.as_mut_ptr() as *mut BVar).write(b) };
        v
    }
    pub fn with_map(name: Sp, m: RawMap) -> Value {
        let mut v = Value { name, a: [0; 5], aidx: 1, pad: [0; 7] };
        v.a[0] = m.head as u64;
        v.a[1] = m.size as u64;
        v
    }
    pub fn bvar(&self) -> &BVar {
        unsafe { &*(self.a.as_ptr() as *const BVar) }
    }
    pub fn map(&self) -> RawMap {
        RawMap { head: self.a[0] as *mut Node, size: self.a[1] as usize }
    }
}

pub unsafe fn value_clone(v: &Value) -> Value {
    let name = v.name.clone_ref();
    match v.aidx {
        0 => Value::with_bvar(name, bvar_clone(v.bvar())),
        1 => Value::with_map(name, bvar_map_clone(&v.map())),
        _ => Value { name, ..*v },
    }
}

pub unsafe fn value_drop(v: &Value) {
    v.name.release();
    match v.aidx {
        0 => bvar_drop(v.bvar()),
        1 => bvar_map_destroy(&v.map()),
        _ => {}
    }
}

pub struct Vals(pub Vec<Value>);

impl Drop for Vals {
    fn drop(&mut self) {
        for v in &self.0 {
            unsafe { value_drop(v) }
        }
    }
}

pub unsafe fn utf8_to_wide(s: &[u8]) -> Option<Vec<u16>> {
    let mut z = s.to_vec();
    z.push(0);
    let n = crate::sys::MultiByteToWideChar(65001, 8, z.as_ptr(), -1, null_mut(), 0);
    if n <= 0 {
        return None;
    }
    let mut out = vec![0u16; n as usize];
    let m = crate::sys::MultiByteToWideChar(65001, 8, z.as_ptr(), -1, out.as_mut_ptr(), n);
    if m <= 0 {
        return None;
    }
    out.truncate(m as usize);
    Some(out)
}

pub unsafe fn normalize(p: &Prim) -> Prim {
    match p.idx {
        1 if p.d[0] != 0 => Prim::sp(3, Sp::wide(wstr(p.d[0] as *const u16))),
        0 if p.d[0] != 0 => {
            let s = cstr(p.d[0] as *const u8);
            match utf8_to_wide(s) {
                Some(wv) if wv.len() != s.len() + 1 => Prim::sp(3, Sp::from_bytes(&wv.iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>())),
                _ => Prim::sp(2, Sp::narrow(s)),
            }
        }
        2 if p.d[0] != 0 => {
            let s = cstr(p.d[0] as *const u8);
            if s.iter().all(|c| *c < 0x80) {
                return p.clone_ref();
            }
            match utf8_to_wide(s) {
                Some(wv) => Prim::sp(3, Sp::from_bytes(&wv.iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>())),
                None => p.clone_ref(),
            }
        }
        _ => p.clone_ref(),
    }
}

pub unsafe fn bvar_norm(b: &BVar) -> BVar {
    match b.idx {
        0 => BVar::from_prim(normalize(b.prim())),
        1 => BVar::from_coll(coll_clone(b.coll())),
        _ => *b,
    }
}

pub unsafe fn v_prim(name: &str, p: Prim) -> Value {
    Value::with_bvar(Sp::wide(&w(name)), BVar::from_prim(p))
}

pub unsafe fn v_coll(name: &str, c: Coll) -> Value {
    Value::with_bvar(Sp::wide(&w(name)), BVar::from_coll(c))
}

pub unsafe fn string_prim(v: *const u16) -> Prim {
    if v.is_null() {
        Prim::empty_w()
    } else {
        Prim::sp(3, Sp::wide(wstr(v)))
    }
}

pub unsafe fn v_string(name: &str, v: *const u16) -> Value {
    v_prim(name, string_prim(v))
}

pub unsafe fn v_tag(t: u32) -> Value {
    v_prim("GTL__TlgEventTag", Prim::u32v(t))
}

pub unsafe fn v_bvar_map(name: &str, entries: Vec<(&str, BVar)>) -> Value {
    let mut m = RawMap::new(BVAR_NODE);
    for (k, b) in entries {
        bvar_map_put(&mut m, Sp::wide(&w(k)), b);
    }
    Value::with_map(Sp::wide(&w(name)), m)
}

pub unsafe fn prim_map_of(entries: Vec<(&str, Prim)>) -> RawMap {
    let mut m = RawMap::new(PRIM_NODE);
    for (k, p) in entries {
        prim_map_put(&mut m, Sp::wide(&w(k)), p, false);
    }
    m
}

pub unsafe fn v_base_type(t: &str, keep: &mut Vec<Vec<u16>>) -> Value {
    keep.push(wz(t));
    let p = keep.last().unwrap().as_ptr();
    Value::with_bvar(Sp::wide(&w("baseType")), BVar::from_prim(normalize(&Prim::wref(p))))
}
