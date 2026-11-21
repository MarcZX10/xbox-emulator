use core::ffi::c_void;

extern "C" {
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
    fn _callnewh(n: usize) -> i32;
    fn _invalid_parameter_noinfo_noreturn() -> !;
}

pub const BIG_ALLOCATION: usize = 0x1000;

pub unsafe fn try_operator_new(n: usize) -> *mut u8 {
    loop {
        let p = malloc(n) as *mut u8;
        if !p.is_null() {
            return p;
        }
        if _callnewh(n) == 0 {
            return core::ptr::null_mut();
        }
    }
}

pub unsafe fn operator_new(n: usize) -> *mut u8 {
    let p = try_operator_new(n);
    if p.is_null() {
        std::process::abort();
    }
    p
}

pub unsafe fn operator_delete(p: *mut u8) {
    free(p as *mut c_void)
}

pub unsafe fn allocate(bytes: usize) -> *mut u8 {
    if bytes >= BIG_ALLOCATION {
        let total = bytes.checked_add(0x27).unwrap_or_else(|| std::process::abort());
        let raw = operator_new(total);
        let aligned = ((raw as usize + 0x27) & !0x1f) as *mut u8;
        *(aligned as *mut usize).sub(1) = raw as usize;
        aligned
    } else if bytes != 0 {
        operator_new(bytes)
    } else {
        core::ptr::null_mut()
    }
}

pub unsafe fn deallocate(p: *mut u8, bytes: usize) {
    if bytes >= BIG_ALLOCATION {
        let raw = *(p as *const usize).sub(1);
        if (p as usize).wrapping_sub(raw).wrapping_sub(8) > 0x1f {
            _invalid_parameter_noinfo_noreturn();
        }
        operator_delete(raw as *mut u8);
    } else {
        operator_delete(p);
    }
}

pub trait Elem: Copy + Default + PartialEq {
    const BUF: usize;
    const MASK: usize;
    const MAX: usize;
}

impl Elem for u16 {
    const BUF: usize = 8;
    const MASK: usize = 7;
    const MAX: usize = 0x7fff_ffff_ffff_fffe;
}

impl Elem for u8 {
    const BUF: usize = 16;
    const MASK: usize = 15;
    const MAX: usize = 0x7fff_ffff_ffff_ffff;
}

#[repr(C)]
pub struct BasicString<T: Elem> {
    pub bx: [u8; 16],
    pub size: usize,
    pub res: usize,
    _t: core::marker::PhantomData<T>,
}

pub type WString = BasicString<u16>;
pub type AString = BasicString<u8>;

fn calc_growth<T: Elem>(requested: usize, old: usize) -> usize {
    let masked = requested | T::MASK;
    if masked > T::MAX {
        return T::MAX;
    }
    if old > T::MAX - old / 2 {
        return T::MAX;
    }
    masked.max(old + old / 2)
}

impl<T: Elem> BasicString<T> {
    pub fn new() -> Self {
        BasicString { bx: [0; 16], size: 0, res: T::BUF - 1, _t: core::marker::PhantomData }
    }

    pub unsafe fn init(this: *mut Self) {
        core::ptr::write(this, Self::new());
    }

    pub fn large(&self) -> bool {
        self.res >= T::BUF
    }

    pub unsafe fn data(this: *const Self) -> *mut T {
        if (*this).large() {
            *((*this).bx.as_ptr() as *const *mut T)
        } else {
            (*this).bx.as_ptr() as *mut T
        }
    }

    pub unsafe fn as_slice<'a>(this: *const Self) -> &'a [T] {
        core::slice::from_raw_parts(Self::data(this), (*this).size)
    }

    unsafe fn set_ptr(this: *mut Self, p: *mut T) {
        *((*this).bx.as_mut_ptr() as *mut *mut T) = p;
    }

    pub unsafe fn clear(this: *mut Self) {
        let d = Self::data(this);
        (*this).size = 0;
        *d = T::default();
    }

    pub unsafe fn tidy(this: *mut Self) {
        if (*this).large() {
            let p = Self::data(this) as *mut u8;
            deallocate(p, ((*this).res + 1) * core::mem::size_of::<T>());
        }
        (*this).res = T::BUF - 1;
        (*this).size = 0;
        core::ptr::write_bytes(core::ptr::addr_of_mut!((*this).bx) as *mut u8, 0, core::mem::size_of::<T>());
    }

    unsafe fn reallocate_for(this: *mut Self, n: usize, src: *const T) {
        if n > T::MAX {
            std::process::abort();
        }
        let old = (*this).res;
        let cap = calc_growth::<T>(n, old);
        let p = allocate((cap + 1) * core::mem::size_of::<T>()) as *mut T;
        (*this).res = cap;
        (*this).size = n;
        core::ptr::copy_nonoverlapping(src, p, n);
        *p.add(n) = T::default();
        if old >= T::BUF {
            deallocate(Self::data_with_res(this, old) as *mut u8, (old + 1) * core::mem::size_of::<T>());
        }
        Self::set_ptr(this, p);
    }

    unsafe fn data_with_res(this: *const Self, res: usize) -> *mut T {
        if res >= T::BUF {
            *((*this).bx.as_ptr() as *const *mut T)
        } else {
            (*this).bx.as_ptr() as *mut T
        }
    }

    pub unsafe fn assign(this: *mut Self, src: *const T, n: usize) {
        if n <= (*this).res {
            let d = Self::data(this);
            (*this).size = n;
            core::ptr::copy(src, d, n);
            *d.add(n) = T::default();
        } else {
            Self::reallocate_for(this, n, src);
        }
    }

    pub unsafe fn assign_cstr(this: *mut Self, src: *const T) {
        let mut n = 0;
        while *src.add(n) != T::default() {
            n += 1;
        }
        Self::assign(this, src, n);
    }

    pub unsafe fn assign_str(this: *mut Self, other: *const Self) {
        if this as *const Self == other {
            return;
        }
        Self::assign(this, Self::data(other), (*other).size);
    }

    pub unsafe fn copy_construct(this: *mut Self, src: *const Self) {
        core::ptr::write(this, Self::new());
        let n = (*src).size;
        let s = Self::data(src);
        if n < T::BUF {
            core::ptr::copy_nonoverlapping(s as *const u8, (*this).bx.as_mut_ptr(), 16);
        } else {
            let cap = (n | T::MASK).min(T::MAX);
            let p = allocate((cap + 1) * core::mem::size_of::<T>()) as *mut T;
            core::ptr::copy_nonoverlapping(s, p, n + 1);
            Self::set_ptr(this, p);
            (*this).res = cap;
        }
        (*this).size = n;
    }

    pub unsafe fn append(this: *mut Self, src: *const T, n: usize) {
        let old_size = (*this).size;
        let old_cap = (*this).res;
        if n <= old_cap - old_size {
            let d = Self::data(this);
            (*this).size = old_size + n;
            core::ptr::copy(src, d.add(old_size), n);
            *d.add(old_size + n) = T::default();
            return;
        }
        if n > T::MAX - old_size {
            std::process::abort();
        }
        let new_size = old_size + n;
        let cap = calc_growth::<T>(new_size, old_cap);
        let p = allocate((cap + 1) * core::mem::size_of::<T>()) as *mut T;
        (*this).size = new_size;
        (*this).res = cap;
        let old = Self::data_with_res(this, old_cap);
        core::ptr::copy_nonoverlapping(old, p, old_size);
        core::ptr::copy_nonoverlapping(src, p.add(old_size), n);
        *p.add(new_size) = T::default();
        if old_cap >= T::BUF {
            deallocate(old as *mut u8, (old_cap + 1) * core::mem::size_of::<T>());
        }
        Self::set_ptr(this, p);
    }

    pub unsafe fn append_cstr(this: *mut Self, src: *const T) {
        let mut n = 0;
        while *src.add(n) != T::default() {
            n += 1;
        }
        Self::append(this, src, n);
    }
}

#[repr(C)]
pub struct RawVec {
    pub first: *mut u8,
    pub last: *mut u8,
    pub end: *mut u8,
}

impl RawVec {
    pub unsafe fn len(this: *const Self, elem: usize) -> usize {
        ((*this).last as usize - (*this).first as usize) / elem
    }

    pub unsafe fn cap(this: *const Self, elem: usize) -> usize {
        ((*this).end as usize - (*this).first as usize) / elem
    }

    pub unsafe fn at(this: *const Self, elem: usize, i: usize) -> *mut u8 {
        (*this).first.add(i * elem)
    }

    pub unsafe fn resize(this: *mut Self, elem: usize, max: usize, n: usize, init: unsafe fn(*mut u8), drop: unsafe fn(*mut u8)) {
        let len = Self::len(this, elem);
        if n < len {
            let mut p = (*this).first.add(n * elem);
            let new_last = p;
            while p != (*this).last {
                drop(p);
                p = p.add(elem);
            }
            (*this).last = new_last;
            return;
        }
        if n == len {
            return;
        }
        let cap = Self::cap(this, elem);
        if n > cap {
            if n > max {
                std::process::abort();
            }
            let new_cap = if cap > max - cap / 2 { max } else { (cap + cap / 2).max(n) };
            let p = allocate(new_cap * elem);
            for i in len..n {
                init(p.add(i * elem));
            }
            if len != 0 {
                core::ptr::copy_nonoverlapping((*this).first, p, len * elem);
            }
            if !(*this).first.is_null() {
                deallocate((*this).first, cap * elem);
            }
            (*this).first = p;
            (*this).last = p.add(n * elem);
            (*this).end = p.add(new_cap * elem);
            return;
        }
        let mut p = (*this).last;
        for _ in len..n {
            init(p);
            p = p.add(elem);
        }
        (*this).last = p;
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub unsafe fn wcslen(p: *const u16) -> usize {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    n
}
