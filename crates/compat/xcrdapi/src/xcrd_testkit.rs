use crate::xcrd_core::*;
use core::ffi::c_void;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Xvd { h: Handle, code: u32, payload: Vec<u8> },
    Create { path: String, access: u32, share: u32, disp: u32, flags: u32 },
    Ioctl { h: Handle, code: u32, input: Vec<u8>, out_len: usize },
    Nt { h: Handle, code: u32, len: usize },
    Close(Handle),
    Sleep(u32),
}

pub type XvdFn = Box<dyn FnMut(Handle, u32, &mut [u8]) -> i32>;
pub type IoctlFn = Box<dyn FnMut(Handle, u32, &[u8], &mut [u8]) -> Result<u32, u32>>;
pub type RawFn = Box<dyn FnMut(Handle, &mut [u8]) -> i32>;

pub struct State {
    pub calls: Vec<Call>,
    pub files: Vec<(String, Result<Handle, u32>)>,
    pub xvd: XvdFn,
    pub ioctl: IoctlFn,
    pub nt_status: i32,
    pub last_error: u32,
    pub tick: u64,
    pub fs_size: [u8; 0x18],
    pub raw: Option<RawFn>,
    pub drive: u32,
    pub dir: Vec<[u8; FIND_DATA]>,
    pub dir_pos: usize,
    pub open: Vec<(Handle, Vec<u8>, usize)>,
}

#[derive(Clone)]
pub struct Mock(pub Rc<RefCell<State>>);

impl Mock {
    pub fn new() -> Mock {
        Mock(Rc::new(RefCell::new(State {
            calls: Vec::new(),
            files: Vec::new(),
            xvd: Box::new(|_, _, _| 0),
            ioctl: Box::new(|_, _, _, _| Err(1)),
            nt_status: 0,
            last_error: 0,
            tick: 0,
            fs_size: [0; 0x18],
            raw: None,
            drive: 1,
            dir: Vec::new(),
            dir_pos: 0,
            open: Vec::new(),
        })))
    }
    pub fn on_xvd(&self, f: impl FnMut(Handle, u32, &mut [u8]) -> i32 + 'static) -> &Self {
        self.0.borrow_mut().xvd = Box::new(f);
        self
    }
    pub fn on_ioctl(&self, f: impl FnMut(Handle, u32, &[u8], &mut [u8]) -> Result<u32, u32> + 'static) -> &Self {
        self.0.borrow_mut().ioctl = Box::new(f);
        self
    }
    pub fn on_raw(&self, f: impl FnMut(Handle, &mut [u8]) -> i32 + 'static) -> &Self {
        self.0.borrow_mut().raw = Some(Box::new(f));
        self
    }
    pub fn file(&self, path: &str, r: Result<Handle, u32>) -> &Self {
        self.0.borrow_mut().files.push((path.to_string(), r));
        self
    }
    pub fn calls(&self) -> Vec<Call> {
        self.0.borrow().calls.clone()
    }
    pub fn xvd_calls(&self) -> Vec<(u32, Vec<u8>)> {
        self.calls().into_iter().filter_map(|c| if let Call::Xvd { code, payload, .. } = c { Some((code, payload)) } else { None }).collect()
    }
    pub fn install(&self) -> Guard {
        let m = self.clone();
        HOST_OVERRIDE.with(|o| *o.borrow_mut() = Some(Box::new(m)));
        Guard
    }
}

pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        HOST_OVERRIDE.with(|o| *o.borrow_mut() = None);
    }
}

impl Host for Mock {
    fn create_file(&mut self, name: &[u16], access: u32, share: u32, disp: u32, flags: u32) -> Handle {
        let path = String::from_utf16_lossy(name);
        let mut s = self.0.borrow_mut();
        s.calls.push(Call::Create { path: path.clone(), access, share, disp, flags });
        let r = s.files.iter().find(|(p, _)| p.eq_ignore_ascii_case(&path)).map(|(_, r)| *r).unwrap_or(Err(2));
        match r {
            Ok(h) => h,
            Err(e) => {
                s.last_error = e;
                INVALID_HANDLE
            }
        }
    }
    fn close_handle(&mut self, h: Handle) -> bool {
        self.0.borrow_mut().calls.push(Call::Close(h));
        h != INVALID_HANDLE
    }
    fn nt_ioctl(&mut self, h: Handle, code: u32, buf: &mut [u8]) -> i32 {
        let mut s = self.0.borrow_mut();
        if code == IOCTL_SCSI_MINIPORT && buf.len() >= SRB_HEADER && buf[4..12] == XVD_SIGNATURE {
            let xc = u32::from_le_bytes(buf[0x10..0x14].try_into().unwrap());
            s.calls.push(Call::Xvd { h, code: xc, payload: buf[SRB_HEADER..].to_vec() });
            let st = s.nt_status;
            let rc = (s.xvd)(h, xc, &mut buf[SRB_HEADER..]);
            buf[0x14..0x18].copy_from_slice(&rc.to_le_bytes());
            return st;
        }
        s.calls.push(Call::Nt { h, code, len: buf.len() });
        if let Some(r) = s.raw.as_mut() {
            return r(h, buf);
        }
        s.nt_status
    }
    fn ioctl(&mut self, h: Handle, code: u32, input: &[u8], output: &mut [u8], returned: &mut u32) -> bool {
        let mut s = self.0.borrow_mut();
        s.calls.push(Call::Ioctl { h, code, input: input.to_vec(), out_len: output.len() });
        match (s.ioctl)(h, code, input, output) {
            Ok(n) => {
                *returned = n;
                true
            }
            Err(e) => {
                s.last_error = e;
                false
            }
        }
    }
    fn last_error(&mut self) -> u32 {
        self.0.borrow().last_error
    }
    fn set_last_error(&mut self, e: u32) {
        self.0.borrow_mut().last_error = e;
    }
    fn sleep(&mut self, ms: u32) {
        let mut s = self.0.borrow_mut();
        s.calls.push(Call::Sleep(ms));
        s.tick += ms as u64;
    }
    fn tick64(&mut self) -> u64 {
        self.0.borrow().tick
    }
    fn query_fs_size(&mut self, h: Handle, out: &mut [u8; 0x18]) -> i32 {
        let mut s = self.0.borrow_mut();
        s.calls.push(Call::Nt { h, code: 3, len: 0x18 });
        out.copy_from_slice(&s.fs_size);
        s.nt_status
    }
    fn drive_type(&mut self, _root: &[u16]) -> u32 {
        self.0.borrow().drive
    }
    fn find_first(&mut self, pattern: &[u16], data: &mut [u8; FIND_DATA]) -> Handle {
        let mut s = self.0.borrow_mut();
        s.calls.push(Call::Create { path: String::from_utf16_lossy(pattern), access: 0, share: 0, disp: 0, flags: 0xf1 });
        if s.dir.is_empty() {
            s.last_error = 2;
            return INVALID_HANDLE;
        }
        *data = s.dir[0];
        s.dir_pos = 1;
        0xf1f1
    }
    fn find_next(&mut self, _h: Handle, data: &mut [u8; FIND_DATA]) -> bool {
        let mut s = self.0.borrow_mut();
        if s.dir_pos >= s.dir.len() {
            s.last_error = 0x12;
            return false;
        }
        *data = s.dir[s.dir_pos];
        s.dir_pos += 1;
        true
    }
    fn find_close(&mut self, h: Handle) -> bool {
        self.0.borrow_mut().calls.push(Call::Close(h));
        true
    }
    fn seek(&mut self, h: Handle, pos: i64) -> bool {
        let mut s = self.0.borrow_mut();
        match s.open.iter_mut().find(|o| o.0 == h) {
            Some(o) => {
                o.2 = pos as usize;
                true
            }
            None => false,
        }
    }
    fn read(&mut self, h: Handle, buf: &mut [u8], read: &mut u32) -> bool {
        let mut s = self.0.borrow_mut();
        let o = s.open.iter_mut().find(|o| o.0 == h).unwrap();
        let n = buf.len().min(o.1.len().saturating_sub(o.2));
        buf[..n].copy_from_slice(&o.1[o.2..o.2 + n]);
        o.2 += n;
        *read = n as u32;
        true
    }
}

impl Mock {
    pub fn find_entry(&self, name: &str, attrs: u32, size: u64, ctime: u64) -> &Self {
        let mut d = [0u8; FIND_DATA];
        d[0..4].copy_from_slice(&attrs.to_le_bytes());
        d[4..12].copy_from_slice(&ctime.to_le_bytes());
        d[0x1c..0x20].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
        d[0x20..0x24].copy_from_slice(&(size as u32).to_le_bytes());
        for (i, c) in name.encode_utf16().enumerate() {
            d[0x2c + i * 2..0x2e + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        self.0.borrow_mut().dir.push(d);
        self
    }
    pub fn file_content(&self, path: &str, h: Handle, data: Vec<u8>) -> &Self {
        self.file(path, Ok(h));
        self.0.borrow_mut().open.push((h, data, 0));
        self
    }
}

pub const TEST_HANDLE: Handle = 0x5a5a;

pub fn adapter(index: u32) -> *mut c_void {
    unsafe {
        let p = heap_alloc(core::mem::size_of::<Adapter>()) as *mut Adapter;
        p.write(Adapter { handle: TEST_HANDLE, index });
        p as *mut c_void
    }
}

pub fn w(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

pub fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

pub fn le64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

pub fn wide_at(b: &[u8], o: usize) -> String {
    let mut v = Vec::new();
    let mut i = o;
    while i + 2 <= b.len() {
        let c = u16::from_le_bytes([b[i], b[i + 1]]);
        if c == 0 {
            break;
        }
        v.push(c);
        i += 2;
    }
    String::from_utf16_lossy(&v)
}
