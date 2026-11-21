extern "C" {
    fn malloc(n: usize) -> *mut u8;
    fn free(p: *mut u8);
}

pub unsafe fn alloc(bytes: usize) -> *mut u8 {
    if bytes == 0 {
        return core::ptr::null_mut();
    }
    if bytes >= 0x1000 {
        let raw = malloc(bytes + 0x27);
        if raw.is_null() {
            std::process::abort();
        }
        let p = ((raw as usize + 0x27) & !0x1f) as *mut u8;
        *(p as *mut usize).sub(1) = raw as usize;
        return p;
    }
    let p = malloc(bytes);
    if p.is_null() {
        std::process::abort();
    }
    p
}

pub unsafe fn dealloc(p: *mut u8, bytes: usize) {
    if p.is_null() {
        return;
    }
    if bytes >= 0x1000 {
        free(*(p as *mut *mut u8).sub(1));
    } else {
        free(p);
    }
}

#[repr(C)]
pub struct WString {
    pub buf: [u16; 8],
    pub size: usize,
    pub cap: usize,
}

#[repr(C)]
pub struct AString {
    pub buf: [u8; 16],
    pub size: usize,
    pub cap: usize,
}

impl WString {
    pub fn new() -> WString {
        WString { buf: [0; 8], size: 0, cap: 7 }
    }
    pub unsafe fn data(&self) -> *const u16 {
        if self.cap >= 8 {
            *(self.buf.as_ptr() as *const *const u16)
        } else {
            self.buf.as_ptr()
        }
    }
    pub unsafe fn as_slice(&self) -> &[u16] {
        core::slice::from_raw_parts(self.data(), self.size)
    }
    pub unsafe fn from_slice(s: &[u16]) -> WString {
        let mut w = WString::new();
        w.size = s.len();
        if s.len() < 8 {
            w.buf[..s.len()].copy_from_slice(s);
            w.buf[s.len()] = 0;
            return w;
        }
        w.cap = (s.len() | 7).min(0x7fff_ffff_ffff_fffe);
        let p = alloc((w.cap + 1) * 2) as *mut u16;
        core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        *p.add(s.len()) = 0;
        *(w.buf.as_mut_ptr() as *mut *mut u16) = p;
        w
    }
    pub unsafe fn drop_in_place(&mut self) {
        if self.cap >= 8 {
            dealloc(*(self.buf.as_ptr() as *const *mut u8), (self.cap + 1) * 2);
        }
        *self = WString::new();
    }
}

impl AString {
    pub fn new() -> AString {
        AString { buf: [0; 16], size: 0, cap: 15 }
    }
    pub unsafe fn data(&self) -> *const u8 {
        if self.cap >= 16 {
            *(self.buf.as_ptr() as *const *const u8)
        } else {
            self.buf.as_ptr()
        }
    }
    pub unsafe fn data_mut(&mut self) -> *mut u8 {
        self.data() as *mut u8
    }
    pub unsafe fn as_slice(&self) -> &[u8] {
        core::slice::from_raw_parts(self.data(), self.size)
    }
    pub unsafe fn from_slice(s: &[u8]) -> AString {
        let mut a = AString::new();
        a.assign(s);
        a
    }
    pub unsafe fn copy_of(s: &[u8]) -> AString {
        let mut a = AString::new();
        a.size = s.len();
        if s.len() < 16 {
            a.buf[..s.len()].copy_from_slice(s);
            a.buf[s.len()] = 0;
            return a;
        }
        a.cap = (s.len() | 15).min(0x7fff_ffff_ffff_fffe);
        let p = alloc(a.cap + 1);
        core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        *p.add(s.len()) = 0;
        *(a.buf.as_mut_ptr() as *mut *mut u8) = p;
        a
    }
    pub unsafe fn assign(&mut self, s: &[u8]) {
        if s.len() <= self.cap {
            let d = self.data_mut();
            core::ptr::copy(s.as_ptr(), d, s.len());
            *d.add(s.len()) = 0;
            self.size = s.len();
            return;
        }
        let max = 0x7fff_ffff_ffff_ffffusize;
        let grown = if self.cap > max - self.cap / 2 { max } else { self.cap + self.cap / 2 };
        let cap = (s.len() | 15).max(grown).min(max);
        let p = alloc(cap + 1);
        core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        *p.add(s.len()) = 0;
        if self.cap >= 16 {
            dealloc(*(self.buf.as_ptr() as *const *mut u8), self.cap + 1);
        }
        *(self.buf.as_mut_ptr() as *mut *mut u8) = p;
        self.cap = cap;
        self.size = s.len();
    }
    pub unsafe fn drop_in_place(&mut self) {
        if self.cap >= 16 {
            dealloc(*(self.buf.as_ptr() as *const *mut u8), self.cap + 1);
        }
        *self = AString::new();
    }
}

#[repr(C)]
pub struct Record {
    pub name: WString,
    pub guid: [u8; 16],
    pub age: u32,
    pub pad: u32,
    pub pdb: AString,
}

#[repr(C)]
pub struct RecordVec {
    pub first: *mut Record,
    pub last: *mut Record,
    pub end: *mut Record,
}

pub const RECORD_SIZE: usize = 0x58;

impl RecordVec {
    pub const fn new() -> RecordVec {
        RecordVec { first: core::ptr::null_mut(), last: core::ptr::null_mut(), end: core::ptr::null_mut() }
    }
    pub unsafe fn len(&self) -> usize {
        if self.first.is_null() {
            0
        } else {
            self.last.offset_from(self.first) as usize
        }
    }
    pub unsafe fn cap(&self) -> usize {
        if self.first.is_null() {
            0
        } else {
            self.end.offset_from(self.first) as usize
        }
    }
    pub unsafe fn as_slice(&self) -> &[Record] {
        if self.first.is_null() {
            &[]
        } else {
            core::slice::from_raw_parts(self.first, self.len())
        }
    }
    pub unsafe fn as_mut_slice(&mut self) -> &mut [Record] {
        if self.first.is_null() {
            &mut []
        } else {
            core::slice::from_raw_parts_mut(self.first, self.len())
        }
    }
    pub unsafe fn push(&mut self, r: Record) {
        if self.last == self.end {
            let n = self.len();
            let c = self.cap();
            let max = 0x2e8_ba2e_8ba2_e8bausize;
            let grown = if c > max - c / 2 { max } else { c + c / 2 };
            let nc = grown.max(n + 1);
            let p = alloc(nc * RECORD_SIZE) as *mut Record;
            if n > 0 {
                core::ptr::copy_nonoverlapping(self.first, p, n);
            }
            dealloc(self.first as *mut u8, c * RECORD_SIZE);
            self.first = p;
            self.last = p.add(n);
            self.end = p.add(nc);
        }
        core::ptr::write(self.last, r);
        self.last = self.last.add(1);
    }
    pub unsafe fn clear_and_free(&mut self) {
        for r in self.as_mut_slice() {
            r.name.drop_in_place();
            r.pdb.drop_in_place();
        }
        dealloc(self.first as *mut u8, self.cap() * RECORD_SIZE);
        *self = RecordVec::new();
    }
}

const _: () = assert!(core::mem::size_of::<Record>() == RECORD_SIZE);
const _: () = assert!(core::mem::size_of::<WString>() == 0x20);
const _: () = assert!(core::mem::size_of::<AString>() == 0x20);
