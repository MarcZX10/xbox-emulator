use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

pub type Handle = isize;
pub const INVALID_HANDLE: Handle = -1;

pub const S_OK: i32 = 0;
pub const S_FALSE: i32 = 1;
pub const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
pub const E_OUTOFMEMORY_NT: i32 = 0x8007_0008u32 as i32;
pub const E_NOT_FOUND: i32 = 0x8007_0490u32 as i32;
pub const E_NOTSUPPORTED: i32 = 0x8007_0032u32 as i32;
pub const STATUS_INVALID_PARAMETER: i32 = 0xc000_000du32 as i32;
pub const STATUS_BUFFER_OVERFLOW: i32 = 0x8000_0005u32 as i32;
pub const STATUS_DEVICE_BUSY: i32 = 0x8000_0011u32 as i32;
pub const STATUS_IO_DEVICE_ERROR: i32 = 0xc000_0185u32 as i32;

pub const IOCTL_SCSI_MINIPORT: u32 = 0x4d008;
pub const SRB_HEADER: usize = 0x1c;
pub const XVD_SIGNATURE: [u8; 8] = *b"MSFT_XVD";

pub const GENERIC_READ: u32 = 0x8000_0000;
pub const GENERIC_WRITE: u32 = 0x4000_0000;
pub const SHARE_RW: u32 = 3;
pub const OPEN_EXISTING: u32 = 3;

pub const PATH_PREFIXES: [&str; 37] = [
    "[XTE:]", "[XUC:]", "[XSS:]", "[XSU:]", "", "[XFG:]", "", "[XTF:]", "[XBL:]", "[XOD:]", "[XVE:]", "[XT0:]",
    "[XT1:]", "[XT2:]", "[XT3:]", "[XT4:]", "[XT5:]", "[XT6:]", "[XT7:]", "[XE0:]", "[XE1:]", "[XE2:]", "[XE3:]",
    "[XE4:]", "[XE5:]", "[XE6:]", "[XE7:]", "[XBX:]", "[XSR:]", "", "[XAS:]", "[XRT:]", "[XRU:]", "[XRS:]", "[XRI:]",
    "", "[XCC:]",
];

pub fn prefix_wide(i: usize) -> Vec<u16> {
    PATH_PREFIXES[i].encode_utf16().collect()
}

pub static TRACE_FLAGS: AtomicU32 = AtomicU32::new(0);

pub fn trace(mask: u32, msg: impl FnOnce() -> String) {
    if TRACE_FLAGS.load(Ordering::Relaxed) & mask == 0 {
        return;
    }
    let mut s = msg();
    if s.chars().count() > 0x1ff {
        s = s.chars().take(0x1ff).collect();
    }
    crate::log("trace", &s);
    #[cfg(not(test))]
    println!("{s}");
}

pub fn wtext(w: &[u16]) -> String {
    String::from_utf16_lossy(w)
}

#[link(name = "ntdll")]
extern "system" {
    fn RtlNtStatusToDosErrorNoTeb(status: i32) -> u32;
    fn NtDeviceIoControlFile(
        file: Handle,
        event: Handle,
        apc: *mut c_void,
        apc_ctx: *mut c_void,
        iosb: *mut [usize; 2],
        code: u32,
        input: *mut c_void,
        in_len: u32,
        output: *mut c_void,
        out_len: u32,
    ) -> i32;
    fn NtQueryVolumeInformationFile(h: Handle, iosb: *mut [usize; 2], out: *mut c_void, len: u32, class: u32) -> i32;
}

extern "system" {
    fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *mut c_void, disp: u32, flags: u32, template: Handle) -> Handle;
    fn CloseHandle(h: Handle) -> i32;
    fn DeviceIoControl(h: Handle, code: u32, inp: *mut c_void, in_len: u32, out: *mut c_void, out_len: u32, ret: *mut u32, ov: *mut c_void) -> i32;
    fn GetLastError() -> u32;
    fn SetLastError(e: u32);
    fn Sleep(ms: u32);
    fn GetTickCount64() -> u64;
    fn GetDriveTypeW(root: *const u16) -> u32;
    fn FindFirstFileW(name: *const u16, data: *mut u8) -> Handle;
    fn FindNextFileW(h: Handle, data: *mut u8) -> i32;
    fn FindClose(h: Handle) -> i32;
    fn SetFilePointerEx(h: Handle, dist: i64, newpos: *mut i64, method: u32) -> i32;
    fn ReadFile(h: Handle, buf: *mut u8, n: u32, read: *mut u32, ov: *mut c_void) -> i32;
    fn GetProcessHeap() -> *mut c_void;
    fn HeapAlloc(heap: *mut c_void, flags: u32, size: usize) -> *mut c_void;
    fn HeapFree(heap: *mut c_void, flags: u32, p: *mut c_void) -> i32;
}

pub fn nt_to_hr(status: i32) -> i32 {
    if status == 0 {
        return 0;
    }
    let mut s = status as u32;
    if s & (1 << 29) != 0 {
        return (s & !(1 << 29)) as i32;
    }
    if s.wrapping_add(0x3fa2_0000) <= 0xff {
        return ((s & 0xffff) | 0x8924_0000) as i32;
    }
    let e = unsafe { RtlNtStatusToDosErrorNoTeb(status) };
    if e == 0x13d {
        s |= 1 << 28;
        return s as i32;
    }
    hr_from_win32(e)
}

pub fn hr_from_win32(e: u32) -> i32 {
    if (e as i32) <= 0 {
        e as i32
    } else {
        ((e & 0xffff) | 0x8007_0000) as i32
    }
}

pub trait Host {
    fn create_file(&mut self, name: &[u16], access: u32, share: u32, disp: u32, flags: u32) -> Handle;
    fn close_handle(&mut self, h: Handle) -> bool;
    fn nt_ioctl(&mut self, h: Handle, code: u32, buf: &mut [u8]) -> i32;
    fn ioctl(&mut self, h: Handle, code: u32, input: &[u8], output: &mut [u8], returned: &mut u32) -> bool;
    fn last_error(&mut self) -> u32;
    fn set_last_error(&mut self, e: u32);
    fn sleep(&mut self, ms: u32);
    fn tick64(&mut self) -> u64;
    fn query_fs_size(&mut self, h: Handle, out: &mut [u8; 0x18]) -> i32;
    fn drive_type(&mut self, root: &[u16]) -> u32;
    fn find_first(&mut self, pattern: &[u16], data: &mut [u8; FIND_DATA]) -> Handle;
    fn find_next(&mut self, h: Handle, data: &mut [u8; FIND_DATA]) -> bool;
    fn find_close(&mut self, h: Handle) -> bool;
    fn seek(&mut self, h: Handle, pos: i64) -> bool;
    fn read(&mut self, h: Handle, buf: &mut [u8], read: &mut u32) -> bool;
}

pub const FIND_DATA: usize = 0x250;

fn nul(s: &[u16]) -> Vec<u16> {
    let mut z = s.to_vec();
    z.push(0);
    z
}

pub struct RealHost;

impl Host for RealHost {
    fn create_file(&mut self, name: &[u16], access: u32, share: u32, disp: u32, flags: u32) -> Handle {
        let mut z: Vec<u16> = name.to_vec();
        z.push(0);
        unsafe { CreateFileW(z.as_ptr(), access, share, core::ptr::null_mut(), disp, flags, 0) }
    }
    fn close_handle(&mut self, h: Handle) -> bool {
        unsafe { CloseHandle(h) != 0 }
    }
    fn nt_ioctl(&mut self, h: Handle, code: u32, buf: &mut [u8]) -> i32 {
        let mut iosb = [0usize; 2];
        let p = buf.as_mut_ptr() as *mut c_void;
        let n = buf.len() as u32;
        unsafe { NtDeviceIoControlFile(h, 0, core::ptr::null_mut(), core::ptr::null_mut(), &mut iosb, code, p, n, p, n) }
    }
    fn ioctl(&mut self, h: Handle, code: u32, input: &[u8], output: &mut [u8], returned: &mut u32) -> bool {
        let ip = if input.is_empty() { core::ptr::null_mut() } else { input.as_ptr() as *mut c_void };
        let op = if output.is_empty() { core::ptr::null_mut() } else { output.as_mut_ptr() as *mut c_void };
        unsafe { DeviceIoControl(h, code, ip, input.len() as u32, op, output.len() as u32, returned, core::ptr::null_mut()) != 0 }
    }
    fn last_error(&mut self) -> u32 {
        unsafe { GetLastError() }
    }
    fn set_last_error(&mut self, e: u32) {
        unsafe { SetLastError(e) }
    }
    fn sleep(&mut self, ms: u32) {
        unsafe { Sleep(ms) }
    }
    fn tick64(&mut self) -> u64 {
        unsafe { GetTickCount64() }
    }
    fn query_fs_size(&mut self, h: Handle, out: &mut [u8; 0x18]) -> i32 {
        let mut iosb = [0usize; 2];
        unsafe { NtQueryVolumeInformationFile(h, &mut iosb, out.as_mut_ptr() as *mut c_void, 0x18, 3) }
    }
    fn drive_type(&mut self, root: &[u16]) -> u32 {
        unsafe { GetDriveTypeW(nul(root).as_ptr()) }
    }
    fn find_first(&mut self, pattern: &[u16], data: &mut [u8; FIND_DATA]) -> Handle {
        unsafe { FindFirstFileW(nul(pattern).as_ptr(), data.as_mut_ptr()) }
    }
    fn find_next(&mut self, h: Handle, data: &mut [u8; FIND_DATA]) -> bool {
        unsafe { FindNextFileW(h, data.as_mut_ptr()) != 0 }
    }
    fn find_close(&mut self, h: Handle) -> bool {
        unsafe { FindClose(h) != 0 }
    }
    fn seek(&mut self, h: Handle, pos: i64) -> bool {
        unsafe { SetFilePointerEx(h, pos, core::ptr::null_mut(), 0) != 0 }
    }
    fn read(&mut self, h: Handle, buf: &mut [u8], read: &mut u32) -> bool {
        unsafe { ReadFile(h, buf.as_mut_ptr(), buf.len() as u32, read, core::ptr::null_mut()) != 0 }
    }
}

#[cfg(test)]
thread_local! {
    pub static HOST_OVERRIDE: std::cell::RefCell<Option<Box<dyn Host>>> = std::cell::RefCell::new(None);
}

pub fn host<R>(f: impl FnOnce(&mut dyn Host) -> R) -> R {
    #[cfg(test)]
    {
        let mut f = Some(f);
        let r = HOST_OVERRIDE.with(|o| {
            let mut b = o.borrow_mut();
            match b.as_mut() {
                Some(h) => Some((f.take().unwrap())(h.as_mut())),
                None => None,
            }
        });
        if let Some(r) = r {
            return r;
        }
        return (f.take().unwrap())(&mut crate::xcrd_driver::EmuHost { drv: crate::xcrd_driver::active() });
    }
    #[cfg(not(test))]
    f(&mut crate::xcrd_driver::EmuHost::global())
}

pub fn last_error_hr() -> i32 {
    hr_from_win32(host(|h| h.last_error()))
}

pub unsafe fn heap_alloc(n: usize) -> *mut u8 {
    unsafe { HeapAlloc(GetProcessHeap(), 0, n) as *mut u8 }
}

pub unsafe fn heap_alloc_zeroed(n: usize) -> *mut u8 {
    unsafe { HeapAlloc(GetProcessHeap(), 8, n) as *mut u8 }
}

pub unsafe fn heap_free(p: *mut c_void) -> bool {
    unsafe { HeapFree(GetProcessHeap(), 0, p) != 0 }
}

pub unsafe fn wlen(p: *const u16) -> usize {
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    n
}

pub unsafe fn wslice<'a>(p: *const u16) -> &'a [u16] {
    unsafe { core::slice::from_raw_parts(p, wlen(p)) }
}

pub unsafe fn cch_copy(dst: *mut u16, cb: usize, src: *const u16) -> i32 {
    let mut cch = cb >> 1;
    if cch.wrapping_sub(1) > 0x7fff_fffe {
        if cch != 0 {
            unsafe { *dst = 0 };
        }
        return STATUS_INVALID_PARAMETER;
    }
    let mut d = dst;
    let mut s = src;
    let mut budget = 0x7fff_fffeusize;
    while budget != 0 {
        let c = unsafe { *s };
        if c == 0 {
            break;
        }
        unsafe { *d = c };
        d = unsafe { d.add(1) };
        s = unsafe { s.add(1) };
        cch -= 1;
        budget -= 1;
        if cch == 0 {
            break;
        }
    }
    if cch == 0 {
        unsafe { *d.sub(1) = 0 };
        STATUS_BUFFER_OVERFLOW
    } else {
        unsafe { *d = 0 };
        0
    }
}

pub fn cch_printf(dst: &mut [u16], text: &str) -> i32 {
    let cch = dst.len();
    if cch.wrapping_sub(1) > 0x7fff_fffe {
        if cch != 0 {
            dst[0] = 0;
        }
        return STATUS_INVALID_PARAMETER;
    }
    let w: Vec<u16> = text.encode_utf16().collect();
    let max = cch - 1;
    if w.len() > max {
        dst[..max].copy_from_slice(&w[..max]);
        dst[max] = 0;
        return STATUS_BUFFER_OVERFLOW;
    }
    dst[..w.len()].copy_from_slice(&w);
    if w.len() == max {
        dst[max] = 0;
    } else {
        dst[w.len()] = 0;
    }
    0
}

pub fn zstr(buf: &[u16]) -> &[u16] {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    &buf[..n]
}

#[repr(C)]
pub struct Adapter {
    pub handle: Handle,
    pub index: u32,
}

pub unsafe fn adapter_handle(a: *const c_void) -> Handle {
    unsafe { (*(a as *const Adapter)).handle }
}

pub unsafe fn adapter_index(a: *const c_void) -> u32 {
    unsafe { (*(a as *const Adapter)).index }
}

pub struct XvdMsg {
    pub buf: Vec<u8>,
}

impl XvdMsg {
    pub fn new(code: u32, payload: usize) -> XvdMsg {
        let mut buf = vec![0u8; payload + SRB_HEADER];
        buf[0..4].copy_from_slice(&(SRB_HEADER as u32).to_le_bytes());
        buf[4..12].copy_from_slice(&XVD_SIGNATURE);
        buf[0x10..0x14].copy_from_slice(&code.to_le_bytes());
        buf[0x18..0x1c].copy_from_slice(&(payload as u32).to_le_bytes());
        XvdMsg { buf }
    }
    pub fn total_len(&self) -> u32 {
        self.buf.len() as u32
    }
    pub fn p(&mut self) -> &mut [u8] {
        &mut self.buf[SRB_HEADER..]
    }
    pub fn pr(&self) -> &[u8] {
        &self.buf[SRB_HEADER..]
    }
    pub fn pp(&mut self) -> *mut u8 {
        self.buf[SRB_HEADER..].as_mut_ptr()
    }
    pub fn u8(&self, off: usize) -> u8 {
        self.pr()[off]
    }
    pub fn u16(&self, off: usize) -> u16 {
        u16::from_le_bytes(self.pr()[off..off + 2].try_into().unwrap())
    }
    pub fn u32(&self, off: usize) -> u32 {
        u32::from_le_bytes(self.pr()[off..off + 4].try_into().unwrap())
    }
    pub fn u64(&self, off: usize) -> u64 {
        u64::from_le_bytes(self.pr()[off..off + 8].try_into().unwrap())
    }
    pub fn set_u8(&mut self, off: usize, v: u8) {
        self.p()[off] = v;
    }
    pub fn set_u16(&mut self, off: usize, v: u16) {
        self.p()[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
    pub fn set_u32(&mut self, off: usize, v: u32) {
        self.p()[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn or_u32(&mut self, off: usize, v: u32) {
        let x = self.u32(off) | v;
        self.set_u32(off, x);
    }
    pub fn set_u64(&mut self, off: usize, v: u64) {
        self.p()[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }
    pub fn set_bytes(&mut self, off: usize, b: &[u8]) {
        self.p()[off..off + b.len()].copy_from_slice(b);
    }
    pub fn bytes(&self, off: usize, n: usize) -> &[u8] {
        &self.pr()[off..off + n]
    }
    pub unsafe fn set_raw(&mut self, off: usize, src: *const u8, n: usize) {
        if n != 0 {
            let d = &mut self.p()[off..off + n];
            unsafe { core::ptr::copy_nonoverlapping(src, d.as_mut_ptr(), n) };
        }
    }
    pub unsafe fn get_raw(&self, off: usize, dst: *mut u8, n: usize) {
        if n != 0 {
            let s = &self.pr()[off..off + n];
            unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), dst, n) };
        }
    }
    pub unsafe fn copy_wstr(&mut self, off: usize, cb: usize, src: *const u16) -> i32 {
        let p = unsafe { self.pp().add(off) } as *mut u16;
        unsafe { cch_copy(p, cb, src) }
    }
    pub fn set_wide(&mut self, off: usize, w: &[u16]) {
        for (i, c) in w.iter().enumerate() {
            self.set_u16(off + i * 2, *c);
        }
    }
    pub fn return_code(&self) -> i32 {
        i32::from_le_bytes(self.buf[0x14..0x18].try_into().unwrap())
    }
    pub fn send(&mut self, h: Handle) -> i32 {
        let mut tries = 0u32;
        loop {
            let st = host(|x| x.nt_ioctl(h, IOCTL_SCSI_MINIPORT, &mut self.buf));
            let rc = self.return_code();
            let s = if st >= 0 { rc } else if rc < 0 { rc } else { st };
            if s == STATUS_DEVICE_BUSY {
                tries += 1;
                if tries < 0x64 {
                    host(|x| x.sleep(0x64));
                    continue;
                }
            }
            crate::xcrd_driver::flush_active();
            return nt_to_hr(s);
        }
    }
    pub unsafe fn send_adapter(&mut self, adapter: *const c_void) -> i32 {
        self.send(unsafe { adapter_handle(adapter) })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct ScsiAddress {
    pub length: u32,
    pub port: u8,
    pub path: u8,
    pub target: u8,
    pub lun: u8,
}

impl ScsiAddress {
    pub fn to_bytes(&self) -> [u8; 8] {
        let l = self.length.to_le_bytes();
        [l[0], l[1], l[2], l[3], self.port, self.path, self.target, self.lun]
    }
    pub fn from_bytes(b: &[u8]) -> ScsiAddress {
        ScsiAddress { length: u32::from_le_bytes(b[0..4].try_into().unwrap()), port: b[4], path: b[5], target: b[6], lun: b[7] }
    }
}

#[repr(C)]
pub struct DevicePath {
    pub path: *mut u16,
    pub address: ScsiAddress,
    pub disk: u32,
    pub volume_number: u32,
}

pub const DEVICE_PATH_HEADER: usize = 0x18;

pub unsafe fn scsi_address_for_path(adapter: *const c_void, path: *const u16, out: &mut ScsiAddress) -> i32 {
    let n = unsafe { wlen(path) };
    let len = n * 2 + 0x22;
    let mut m = XvdMsg::new(0x7800_0078, len);
    m.set_u32(0, len as u32);
    m.set_u32(4, 0x20);
    m.set_u32(8, (n * 2) as u32);
    let hr = unsafe { m.copy_wstr(0x20, (n + 1) * 2, path) };
    if hr < 0 {
        return hr;
    }
    let hr = unsafe { m.send_adapter(adapter) };
    if hr < 0 {
        return hr;
    }
    out.lun = m.u8(0x18);
    out.target = m.u8(0x19);
    out.path = m.u8(0x1a);
    out.port = unsafe { adapter_index(adapter) } as u8;
    out.length = 8;
    hr
}

pub unsafe fn remove_lun(adapter: *const c_void, addr: &ScsiAddress) -> i32 {
    let mut m = XvdMsg::new(0x7800_0008, 0x10);
    m.set_u8(0, addr.lun);
    m.set_u8(2, addr.path);
    m.set_u8(1, addr.target);
    unsafe { m.send_adapter(adapter) }
}

pub fn disk_number_for_address(out: &mut u32, addr: &ScsiAddress) -> i32 {
    for i in 0..0x80u32 {
        let mut name = [0u16; 0x20];
        if nt_to_hr(cch_printf(&mut name, &format!("\\\\.\\PhysicalDrive{i}"))) < 0 {
            continue;
        }
        let h = host(|x| x.create_file(zstr(&name), GENERIC_READ | GENERIC_WRITE, SHARE_RW, OPEN_EXISTING, 0));
        if h == INVALID_HANDLE {
            host(|x| x.last_error());
            continue;
        }
        let q = [0u8; 0xc];
        let mut desc = [0u8; 0x28];
        let mut ret = 0u32;
        let ok = host(|x| x.ioctl(h, 0x2d_1400, &q, &mut desc, &mut ret));
        let mut skip = false;
        if ok {
            let bus = u32::from_le_bytes(desc[0x1c..0x20].try_into().unwrap());
            if bus == 0xc || bus == 7 {
                skip = true;
            }
        } else {
            host(|x| x.last_error());
        }
        if !skip {
            let mut a = [0u8; 8];
            let ok = host(|x| x.ioctl(h, 0x4_1018, &[], &mut a, &mut ret));
            if ok && a[7] == addr.lun && a[6] == addr.target && a[5] == addr.path && a[4] == addr.port {
                host(|x| x.close_handle(h));
                *out = i;
                return 0;
            }
        }
        host(|x| x.close_handle(h));
    }
    E_NOT_FOUND
}

pub unsafe fn device_path_for_disk(out: &mut *mut DevicePath, disk: u32, addr: &ScsiAddress) -> i32 {
    if disk > 0x270f {
        return E_INVALIDARG;
    }
    let mut name = [0u16; 0x2f];
    let mut hr = nt_to_hr(cch_printf(&mut name, &format!("\\\\?\\GLOBALROOT\\Device\\Harddisk{disk}\\Partition1")));
    if hr < 0 {
        return hr;
    }
    let h = host(|x| x.create_file(zstr(&name), 0x80, SHARE_RW, OPEN_EXISTING, 0));
    if h == INVALID_HANDLE {
        return last_error_hr();
    }
    let mut ret = 0u32;
    let ok = host(|x| x.ioctl(h, 0x7_0024, &[], &mut [], &mut ret));
    if !ok && host(|x| x.last_error()) != 0x13 {
        let r = last_error_hr();
        host(|x| x.close_handle(h));
        return r;
    }
    let mut vol = [0u8; 0x14];
    if !host(|x| x.ioctl(h, 0x56_001c, &[], &mut vol, &mut ret)) {
        let r = last_error_hr();
        host(|x| x.close_handle(h));
        return r;
    }
    let n = zstr(&name).len();
    let cb = n * 2 + 0x1a;
    let dp = unsafe { heap_alloc_zeroed(cb) };
    if dp.is_null() {
        host(|x| x.close_handle(h));
        return E_OUTOFMEMORY_NT;
    }
    let s = unsafe { dp.add(DEVICE_PATH_HEADER) } as *mut u16;
    hr = unsafe { cch_copy(s, cb - DEVICE_PATH_HEADER, name.as_ptr()) };
    if hr >= 0 {
        let d = dp as *mut DevicePath;
        unsafe {
            (*d).address = *addr;
            (*d).disk = disk;
            (*d).path = s;
            (*d).volume_number = u32::from_le_bytes(vol[0..4].try_into().unwrap());
        }
        hr = nt_to_hr(cch_printf(&mut name, &format!("\\\\?\\GLOBALROOT\\Device\\Harddisk{disk}\\Partition1\\")));
        if hr >= 0 {
            let r = host(|x| x.create_file(zstr(&name), GENERIC_READ, SHARE_RW, OPEN_EXISTING, 0x0200_0000));
            if r == INVALID_HANDLE {
                hr = last_error_hr();
            } else {
                host(|x| x.close_handle(r));
            }
            if hr >= 0 {
                *out = d;
            }
        }
    }
    if hr < 0 {
        unsafe { heap_free(dp as *mut c_void) };
    }
    host(|x| x.close_handle(h));
    hr
}

pub fn volume_on_disk(h: Handle, disk: u32, found: &mut bool) -> i32 {
    *found = false;
    let mut ext = [0u8; 0x20];
    let mut ret = 0u32;
    if host(|x| x.ioctl(h, 0x56_0000, &[], &mut ext, &mut ret)) {
        if u32::from_le_bytes(ext[8..12].try_into().unwrap()) == disk {
            *found = true;
            return 0;
        }
        return E_INVALIDARG;
    }
    if host(|x| x.last_error()) != 0xea {
        return E_INVALIDARG;
    }
    let count = u32::from_le_bytes(ext[0..4].try_into().unwrap()) as usize;
    let size = (count * 3 * 8 + 8) as u32 as usize;
    let mut big = vec![0u8; size];
    if !host(|x| x.ioctl(h, 0x56_0000, &[], &mut big, &mut ret)) {
        let hr = last_error_hr();
        trace(1, || format!("Failed to query volume extents - {:x}", hr as u32));
        return hr;
    }
    let n = u32::from_le_bytes(big[0..4].try_into().unwrap()) as usize;
    for i in 0..n {
        let o = 8 + i * 0x18;
        if o + 4 > big.len() {
            break;
        }
        if u32::from_le_bytes(big[o..o + 4].try_into().unwrap()) == disk {
            *found = true;
            return 0;
        }
    }
    E_INVALIDARG
}

pub fn lock_and_dismount(h: Handle) -> i32 {
    let mut ret = 0u32;
    let mut hr = 0;
    let mut locked = false;
    for _ in 0..5 {
        if host(|x| x.ioctl(h, 0x9_0018, &[], &mut [], &mut ret)) {
            locked = true;
            break;
        }
        hr = last_error_hr();
        host(|x| x.sleep(0x32));
    }
    if !locked && hr < 0 {
        trace(2, || format!("XiDismountVolume - Failed to lock volume - {:x}. Proceeding.", hr as u32));
    }
    if host(|x| x.ioctl(h, 0x9_0020, &[], &mut [], &mut ret)) {
        0
    } else {
        last_error_hr()
    }
}

pub unsafe fn dismount_device_path(dp: *const DevicePath) -> i32 {
    let path = unsafe { wslice((*dp).path) };
    let disk = unsafe { (*dp).disk };
    let mut h = host(|x| x.create_file(path, GENERIC_READ | GENERIC_WRITE, SHARE_RW, OPEN_EXISTING, 0x80));
    if h == INVALID_HANDLE {
        h = host(|x| x.create_file(path, GENERIC_READ, SHARE_RW, OPEN_EXISTING, 0x80));
    }
    if h != INVALID_HANDLE {
        let mut found = false;
        let r = volume_on_disk(h, disk, &mut found);
        if r < 0 {
            host(|x| x.close_handle(h));
            return r;
        }
        if !found {
            trace(1, || format!("Failed to dismount {} on disk {:x}", wtext(path), disk));
        }
    }
    let r = lock_and_dismount(h);
    if h != INVALID_HANDLE {
        host(|x| x.close_handle(h));
    }
    r
}

pub struct MountPoints {
    pub buf: Vec<u8>,
}

impl MountPoints {
    fn u32(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.buf[o..o + 4].try_into().unwrap())
    }
    fn u16(&self, o: usize) -> u16 {
        u16::from_le_bytes(self.buf[o..o + 2].try_into().unwrap())
    }
    fn wch(&self, base: usize, i: usize) -> u16 {
        let o = base + i * 2;
        if o + 2 > self.buf.len() {
            return 0;
        }
        self.u16(o)
    }
    fn clear(&mut self, entry: usize) {
        self.buf[entry..entry + 4].copy_from_slice(&0u32.to_le_bytes());
    }
    fn volume_link(&self, entry: usize) -> Option<(usize, u16)> {
        let off = self.u32(entry) as usize;
        let len = self.u16(entry + 4);
        if off == 0 {
            return None;
        }
        let ok_len = len == 0x60 || (len == 0x62 && self.wch(off, 0x30) == 0x5c);
        let c = |i: usize| self.wch(off, i);
        let pattern = ok_len
            && c(0) == 0x5c
            && (c(1) == 0x3f || c(1) == 0x5c)
            && c(2) == 0x3f
            && c(3) == 0x5c
            && c(4) == 0x56
            && c(5) == 0x6f
            && c(6) == 0x6c
            && c(7) == 0x75
            && c(8) == 0x6d
            && c(9) == 0x65
            && c(10) == 0x7b
            && c(0x13) == 0x2d
            && c(0x18) == 0x2d
            && c(0x1d) == 0x2d
            && c(0x22) == 0x2d
            && c(0x2f) == 0x7d;
        if pattern && len == 0x60 && c(1) == 0x3f {
            Some((off, len))
        } else {
            None
        }
    }
    fn device_name(&self, entry: usize) -> Vec<u16> {
        let off = self.u32(entry + 0x10) as usize;
        let len = self.u16(entry + 0x14) as usize;
        (0..len / 2).map(|i| self.wch(off, i)).collect()
    }
    pub fn next_volume(&mut self, out: &mut [u16; 0x104]) -> bool {
        let count = self.u32(4) as usize;
        let mut hit = None;
        for i in 0..count {
            let e = 8 + i * 0x18;
            if self.u32(e) == 0 {
                continue;
            }
            if self.volume_link(e).is_some() {
                hit = Some(i);
                break;
            }
            self.clear(e);
        }
        let i = match hit {
            Some(i) => i,
            None => {
                host(|x| x.set_last_error(0x12));
                return false;
            }
        };
        let e = 8 + i * 0x18;
        let dev = self.device_name(e);
        for j in i + 1..count {
            let f = 8 + j * 0x18;
            if self.u32(f) == 0 {
                continue;
            }
            if self.volume_link(f).is_some() {
                let other = self.device_name(f);
                let eq = other.len() == dev.len()
                    && other.iter().zip(dev.iter()).all(|(a, b)| upcase(*a) == upcase(*b));
                if !eq {
                    continue;
                }
            }
            self.clear(f);
        }
        let off = self.u32(e) as usize;
        let len = self.u16(e + 4) as usize;
        if len + 4 > 0x208 {
            host(|x| x.set_last_error(0xce));
            return false;
        }
        for k in 0..len / 2 {
            out[k] = self.wch(off, k);
        }
        out[1] = 0x5c;
        out[len / 2] = 0x5c;
        out[len / 2 + 1] = 0;
        self.clear(e);
        true
    }
}

fn upcase(c: u16) -> u16 {
    if (0x61..=0x7a).contains(&c) {
        c - 0x20
    } else if c < 0x80 {
        c
    } else {
        char::from_u32(c as u32).and_then(|ch| ch.to_uppercase().next()).map(|u| u as u32).filter(|u| *u <= 0xffff).unwrap_or(c as u32) as u16
    }
}

pub fn first_volume(out: &mut [u16; 0x104]) -> Option<MountPoints> {
    let mgr: Vec<u16> = "\\\\.\\MountPointManager".encode_utf16().collect();
    let h = host(|x| x.create_file(&mgr, 0, SHARE_RW, OPEN_EXISTING, 0x80));
    if h == INVALID_HANDLE {
        return None;
    }
    let input = [0u8; 0x18];
    let mut buf = vec![0u8; 0x20];
    let mut ret = 0u32;
    let mut ok = host(|x| x.ioctl(h, 0x6d_0008, &input, &mut buf, &mut ret));
    while !ok {
        if host(|x| x.last_error()) != 0xea {
            break;
        }
        let size = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
        buf = vec![0u8; size];
        ok = host(|x| x.ioctl(h, 0x6d_0008, &input, &mut buf, &mut ret));
    }
    host(|x| x.close_handle(h));
    if !ok {
        return None;
    }
    let mut mp = MountPoints { buf };
    if mp.next_volume(out) {
        Some(mp)
    } else {
        None
    }
}

pub fn dismount_disk_volumes(disk: u32) -> i32 {
    let mut name = [0u16; 0x104];
    let mut mp = match first_volume(&mut name) {
        Some(m) => m,
        None => return last_error_hr(),
    };
    let mut result = 0;
    loop {
        let n = zstr(&name).len();
        if n > 0 && name[n - 1] == 0x5c {
            name[n - 1] = 0;
        }
        let path = zstr(&name).to_vec();
        let h = host(|x| x.create_file(&path, GENERIC_READ | GENERIC_WRITE, SHARE_RW, OPEN_EXISTING, 0x80));
        if h == INVALID_HANDLE {
            let r = host(|x| x.create_file(&path, GENERIC_READ, SHARE_RW, OPEN_EXISTING, 0x80));
            if r != INVALID_HANDLE {
                let mut found = false;
                if volume_on_disk(r, disk, &mut found) >= 0 {
                    trace(1, || format!("Failed to dismount {} on disk {:x}", wtext(&path), disk));
                    host(|x| x.close_handle(r));
                    return 1;
                }
                host(|x| x.close_handle(r));
            }
            let e = last_error_hr();
            trace(1, || format!("Failed to open volume {} - {:x}", wtext(&path), e as u32));
        } else {
            let mut found = false;
            if volume_on_disk(h, disk, &mut found) >= 0 && found {
                let r = lock_and_dismount(h);
                trace(1, || format!("Dismounting disk {disk} - {:x}", r as u32));
                if r < 0 {
                    trace(1, || format!("Failed to dismount {} - {:x}", wtext(&path), r as u32));
                    result = r;
                } else {
                    trace(1, || format!("Successfully dismounted volume {}", wtext(&path)));
                }
            }
            host(|x| x.close_handle(h));
        }
        if !mp.next_volume(&mut name) {
            break;
        }
    }
    result
}

pub fn wz(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}
