use core::ffi::c_void;

pub trait Xcrd: Send {
    fn open_adapter(&mut self) -> Result<usize, i32>;
    fn close_adapter(&mut self, adapter: usize) -> i32;
    fn mount_content_type(&mut self, adapter: usize, path: &[u16], content_type: u32, flags: u32) -> Result<usize, i32>;
    fn query_device_path(&mut self, device: usize) -> Result<Vec<u16>, i32>;
    fn unmount(&mut self, adapter: usize, device: usize) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
}

pub fn hr_from_win32(e: u32) -> i32 {
    if e as i32 <= 0 {
        e as i32
    } else {
        ((e & 0xffff) | 0x8007_0000) as i32
    }
}

pub fn load(dll: &str) -> Result<usize, i32> {
    let lib = dll.to_ascii_lowercase();
    let lib = lib.trim_end_matches(".dll");
    if compat_common::os_host::routes::console_of(lib).is_some() {
        let h = compat_common::os_host::routes::module(lib);
        if h != 0 {
            return Ok(h);
        }
    }
    let n: Vec<u16> = dll.encode_utf16().chain([0]).collect();
    let m = unsafe { LoadLibraryW(n.as_ptr()) };
    if m.is_null() {
        return Err(hr_from_win32(unsafe { GetLastError() }));
    }
    Ok(m as usize)
}

pub fn proc(module: usize, name: &str) -> Result<usize, i32> {
    let n: Vec<u8> = name.bytes().chain([0]).collect();
    let p = unsafe { GetProcAddress(module as *mut c_void, n.as_ptr()) };
    if p.is_null() {
        return Err(hr_from_win32(unsafe { GetLastError() }));
    }
    Ok(p as usize)
}

type OpenFn = unsafe extern "system" fn(*mut *mut c_void) -> i32;
type CloseFn = unsafe extern "system" fn(*mut c_void) -> i32;
type MountFn = unsafe extern "system" fn(*mut *mut c_void, *mut c_void, *const u16, u32, u32) -> i32;
type QueryFn = unsafe extern "system" fn(*mut *mut u16, *const c_void) -> i32;
type FreeFn = unsafe extern "system" fn(*mut c_void) -> i32;
type UnmountFn = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;

#[derive(Clone, Copy)]
struct Fns {
    open: OpenFn,
    close: CloseFn,
    mount: MountFn,
    query: QueryFn,
    free: FreeFn,
    unmount: UnmountFn,
}

pub struct DllXcrd {
    dll: String,
    fns: Option<Result<Fns, i32>>,
}

impl DllXcrd {
    pub fn new(dll: &str) -> Self {
        DllXcrd { dll: dll.to_string(), fns: None }
    }
    fn fns(&mut self) -> Result<Fns, i32> {
        if self.fns.is_none() {
            let dll = self.dll.clone();
            let r = (|| unsafe {
                let m = load(&dll)?;
                Ok(Fns {
                    open: core::mem::transmute::<usize, OpenFn>(proc(m, "XCrdOpenAdapter")?),
                    close: core::mem::transmute::<usize, CloseFn>(proc(m, "XCrdCloseAdapter")?),
                    mount: core::mem::transmute::<usize, MountFn>(proc(m, "XCrdMountContentType")?),
                    query: core::mem::transmute::<usize, QueryFn>(proc(m, "XCrdQueryDevicePath")?),
                    free: core::mem::transmute::<usize, FreeFn>(proc(m, "XCrdFreeDevicePathBuffer")?),
                    unmount: core::mem::transmute::<usize, UnmountFn>(proc(m, "XCrdUnmount")?),
                })
            })();
            self.fns = Some(r);
        }
        self.fns.unwrap()
    }
}

impl Xcrd for DllXcrd {
    fn open_adapter(&mut self) -> Result<usize, i32> {
        let f = self.fns()?;
        let mut a = core::ptr::null_mut();
        let hr = unsafe { (f.open)(&mut a) };
        if hr < 0 {
            return Err(hr);
        }
        Ok(a as usize)
    }
    fn close_adapter(&mut self, adapter: usize) -> i32 {
        match self.fns() {
            Ok(f) => unsafe { (f.close)(adapter as *mut c_void) },
            Err(e) => e,
        }
    }
    fn mount_content_type(&mut self, adapter: usize, path: &[u16], content_type: u32, flags: u32) -> Result<usize, i32> {
        let f = self.fns()?;
        let p: Vec<u16> = path.iter().copied().chain([0]).collect();
        let mut d = core::ptr::null_mut();
        let hr = unsafe { (f.mount)(&mut d, adapter as *mut c_void, p.as_ptr(), content_type, flags) };
        if hr < 0 {
            return Err(hr);
        }
        Ok(d as usize)
    }
    fn query_device_path(&mut self, device: usize) -> Result<Vec<u16>, i32> {
        let f = self.fns()?;
        let mut p = core::ptr::null_mut();
        let hr = unsafe { (f.query)(&mut p, device as *const c_void) };
        let mut out = Vec::new();
        if !p.is_null() {
            let mut i = 0;
            while unsafe { *p.add(i) } != 0 {
                out.push(unsafe { *p.add(i) });
                i += 1;
            }
            unsafe { (f.free)(p as *mut c_void) };
        }
        if hr < 0 {
            return Err(hr);
        }
        Ok(out)
    }
    fn unmount(&mut self, adapter: usize, device: usize) -> i32 {
        match self.fns() {
            Ok(f) => unsafe { (f.unmount)(adapter as *mut c_void, device as *mut c_void) },
            Err(e) => e,
        }
    }
}
