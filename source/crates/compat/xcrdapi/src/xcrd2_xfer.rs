use crate::xcrd_core::*;
use core::ffi::c_void;
use core::ptr::{null, null_mut};

pub const HR_NOT_READY: i32 = 0x8007_0015u32 as i32;
pub const HR_BAD_COMMAND: i32 = 0x8007_0016u32 as i32;
pub const HR_IO_PENDING: i32 = 0x8007_03e5u32 as i32;
pub const HR_OPERATION_ABORTED: i32 = 0x8007_03e3u32 as i32;
pub const HR_PIPE_CONNECTED: i32 = 0x8007_0217u32 as i32;
pub const HR_TIMEOUT: i32 = 0x8007_05b4u32 as i32;
pub const HR_UNEXPECTED: i32 = 0x8000_ffffu32 as i32;
pub const HR_INVALID_CONTEXT: i32 = 0x8007_139fu32 as i32;
pub const XFER_MSG: usize = 0x210;
pub const XFER_SLOTS: usize = 16;

#[repr(C)]
pub struct Overlapped {
    pub internal: usize,
    pub internal_high: usize,
    pub offset: u32,
    pub offset_high: u32,
    pub event: isize,
}

#[repr(C)]
pub struct Slot {
    pub busy: u32,
    pub result: *mut Overlapped,
    pub buffer: *mut u8,
    pub len: u32,
}

#[repr(C)]
pub struct XferCtx {
    pub pipe: isize,
    pub session: u32,
    pub transfer: u32,
    pub thread: isize,
    pub stop: isize,
    pub failed: u8,
    pub hr: i32,
    pub cs_ready: u8,
    pub cs: [usize; 5],
    pub slot_event: isize,
    pub slots: [Slot; XFER_SLOTS],
}

const _: () = assert!(core::mem::size_of::<XferCtx>() == 0x260);
const _: () = assert!(core::mem::offset_of!(XferCtx, cs) == 0x30);
const _: () = assert!(core::mem::offset_of!(XferCtx, slots) == 0x60);

#[link(name = "kernel32")]
extern "system" {
    fn CreateEventW(sa: *mut c_void, manual: i32, initial: i32, name: *const u16) -> isize;
    fn SetEvent(h: isize) -> i32;
    fn ResetEvent(h: isize) -> i32;
    fn CloseHandle(h: isize) -> i32;
    fn WaitForSingleObject(h: isize, ms: u32) -> u32;
    fn WaitForMultipleObjects(n: u32, h: *const isize, all: i32, ms: u32) -> u32;
    fn CreateNamedPipeW(name: *const u16, open: u32, mode: u32, max: u32, out_cb: u32, in_cb: u32, timeout: u32, sa: *const c_void) -> isize;
    fn ConnectNamedPipe(h: isize, ov: *mut Overlapped) -> i32;
    fn DisconnectNamedPipe(h: isize) -> i32;
    fn ReadFile(h: isize, buf: *mut u8, n: u32, got: *mut u32, ov: *mut c_void) -> i32;
    fn WriteFile(h: isize, buf: *const u8, n: u32, got: *mut u32, ov: *mut Overlapped) -> i32;
    fn GetOverlappedResult(h: isize, ov: *mut Overlapped, got: *mut u32, wait: i32) -> i32;
    fn CancelIoEx(h: isize, ov: *mut Overlapped) -> i32;
    fn GetLastError() -> u32;
    fn CreateThread(sa: *const c_void, stack: usize, start: unsafe extern "system" fn(*mut c_void) -> u32, param: *mut c_void, flags: u32, id: *mut u32) -> isize;
    fn InitializeCriticalSectionEx(cs: *mut c_void, spin: u32, flags: u32) -> i32;
    fn EnterCriticalSection(cs: *mut c_void);
    fn LeaveCriticalSection(cs: *mut c_void);
    fn DeleteCriticalSection(cs: *mut c_void);
}

#[cfg(test)]
thread_local! {
    pub static PIPE_NAME: std::cell::RefCell<Option<Vec<u16>>> = const { std::cell::RefCell::new(None) };
}

pub fn pipe_name() -> Vec<u16> {
    #[cfg(test)]
    {
        if let Some(n) = PIPE_NAME.with(|p| p.borrow().clone()) {
            return n;
        }
    }
    wz(&pipe_path())
}

pub fn pipe_path() -> String {
    format!("\\\\.\\pipe\\TomBox.HSVPipe.XvdXferPipe.{}", std::process::id())
}

pub fn pipe_text() -> String {
    String::from_utf16_lossy(zstr(&pipe_name()))
}

fn win_hr() -> i32 {
    hr_from_win32(unsafe { GetLastError() })
}

fn xlog(level: u32, msg: impl FnOnce() -> String) {
    trace(level, || format!("XCRD-XFER: {}\n", msg()));
}

unsafe fn cs(c: *mut XferCtx) -> *mut c_void {
    unsafe { core::ptr::addr_of_mut!((*c).cs) as *mut c_void }
}

unsafe fn set_failed(c: *mut XferCtx, hr: i32) {
    unsafe {
        if (*c).failed != 1 {
            (*c).failed = 1;
            (*c).hr = hr;
        }
    }
}

unsafe fn pipe_io(c: *mut XferCtx, write: bool, mut buf: *mut u8, mut len: u32) -> i32 {
    let ev = unsafe { CreateEventW(null_mut(), 0, 0, null()) };
    if ev == 0 {
        return win_hr();
    }
    let mut ov = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: ev };
    let handles = [ev, unsafe { (*c).stop }];
    let (verb, dir) = if write { ("Writing", "to") } else { ("Reading", "from") };
    let mut hr = 0;
    while len != 0 {
        xlog(4, || format!("{verb} {len} bytes {dir} pipe"));
        let pipe = unsafe { (*c).pipe };
        let ok = unsafe { if write { WriteFile(pipe, buf, len, null_mut(), &mut ov) } else { ReadFile(pipe, buf, len, null_mut(), &mut ov as *mut Overlapped as *mut c_void) } };
        if ok == 0 {
            let e = unsafe { GetLastError() };
            if e != 0x3e5 {
                hr = hr_from_win32(e);
                xlog(1, || format!("{} failed 0x{:08X}", if write { "WriteFile" } else { "ReadFile" }, hr as u32));
                break;
            }
            let w = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, u32::MAX) };
            if w > 1 {
                hr = win_hr();
                xlog(1, || format!("Pipe wait error 0x{:08X} ({w})", hr as u32));
                break;
            }
            if handles[w as usize] == unsafe { (*c).stop } {
                let mut got = 0u32;
                unsafe {
                    CancelIoEx(pipe, &mut ov);
                    GetOverlappedResult(pipe, &mut ov, &mut got, 1);
                }
                hr = HR_OPERATION_ABORTED;
                xlog(2, || format!("Pipe {} aborted due to teardown", if write { "write" } else { "read" }));
                unsafe { set_failed(c, hr) };
                break;
            }
        }
        let mut got = 0u32;
        if unsafe { GetOverlappedResult(pipe, &mut ov, &mut got, 0) } == 0 {
            hr = win_hr();
            xlog(1, || format!("Error getting pipe {} result 0x{:08X}", if write { "write" } else { "read" }, hr as u32));
            break;
        }
        xlog(4, || format!("{} {got} bytes {dir} pipe", if write { "Wrote" } else { "Read" }));
        buf = unsafe { buf.add(got as usize) };
        len -= got;
    }
    unsafe { CloseHandle(ev) };
    hr
}

unsafe fn release_slot(c: *mut XferCtx, s: *mut Slot) {
    unsafe {
        EnterCriticalSection(cs(c));
        core::ptr::write_bytes(s as *mut u8, 0, 0x20);
        LeaveCriticalSection(cs(c));
        SetEvent((*c).slot_event);
    }
}

fn rd32(m: &[u8; XFER_MSG], o: usize) -> u32 {
    u32::from_le_bytes(m[o..o + 4].try_into().unwrap())
}

fn wr32(m: &mut [u8; XFER_MSG], o: usize, v: u32) {
    m[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

unsafe extern "system" fn completion(p: *mut c_void) -> u32 {
    let c = p as *mut XferCtx;
    let mut m = [0u8; XFER_MSG];
    let mut hr = unsafe { pipe_io(c, false, m.as_mut_ptr(), XFER_MSG as u32) };
    while hr >= 0 {
        if rd32(&m, 0) != 4 || rd32(&m, 0xc) != unsafe { (*c).transfer } || rd32(&m, 0x10) >= XFER_SLOTS as u32 {
            hr = HR_BAD_COMMAND;
            break;
        }
        let status = nt_to_hr(rd32(&m, 8) as i32);
        let bytes = rd32(&m, 0x14);
        xlog(4, || format!("Read ack status 0x{:08X}, bytes read {bytes}", status as u32));
        let s = unsafe { core::ptr::addr_of_mut!((*c).slots[rd32(&m, 0x10) as usize]) };
        let r = unsafe { (*s).result };
        unsafe {
            (*r).internal = status as i64 as usize;
            (*r).internal_high = bytes as usize;
        }
        let mut rc = 0;
        if status >= 0 && bytes != 0 {
            rc = unsafe { pipe_io(c, false, (*s).buffer, bytes) };
            if rc < 0 {
                unsafe {
                    set_failed(c, rc);
                    (*r).internal = rc as i64 as usize;
                    (*r).internal_high = 0;
                }
            }
        }
        unsafe {
            SetEvent((*r).event);
            release_slot(c, s);
        }
        if rc < 0 {
            hr = rc;
            break;
        }
        m = [0u8; XFER_MSG];
        hr = unsafe { pipe_io(c, false, m.as_mut_ptr(), XFER_MSG as u32) };
    }
    if hr < 0 {
        unsafe { set_failed(c, hr) };
    }
    unsafe {
        EnterCriticalSection(cs(c));
        for i in 0..XFER_SLOTS {
            let s = core::ptr::addr_of_mut!((*c).slots[i]);
            if (*s).busy != 0 {
                let r = (*s).result;
                (*r).internal = (*c).hr as i64 as usize;
                (*r).internal_high = 0;
                SetEvent((*r).event);
                release_slot(c, s);
            }
        }
        LeaveCriticalSection(cs(c));
    }
    xlog(4, || format!("Completion thread exiting 0x{:08X}", hr as u32));
    0
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXvdXferInitialize(adapter: *mut c_void, out: *mut *mut XferCtx) -> i32 {
    if adapter.is_null() {
        xlog(1, || "Invalid adapter handle".into());
        return E_INVALIDARG;
    }
    if out.is_null() {
        xlog(1, || "Invalid transfer handle".into());
        return E_INVALIDARG;
    }
    unsafe { *out = null_mut() };
    xlog(4, || "Initializing".into());
    let c = unsafe { heap_alloc_zeroed(0x260) } as *mut XferCtx;
    if c.is_null() {
        let hr = E_OUTOFMEMORY_NT;
        xlog(4, || format!("Initialize result 0x{:X}", hr as u32));
        return hr;
    }
    unsafe {
        (*c).pipe = -1;
        (*c).session = u32::MAX;
        (*c).transfer = u32::MAX;
        set_failed(c, HR_NOT_READY);
    }
    let mut hr;
    let mut keep = c;
    'run: {
        let stop = unsafe { CreateEventW(null_mut(), 1, 0, null()) };
        unsafe { (*c).stop = stop };
        if stop == 0 {
            hr = win_hr();
            break 'run;
        }
        let name = pipe_name();
        let pipe = unsafe { CreateNamedPipeW(name.as_ptr(), 0x4000_0003, 8, 1, 0, 0, 0, null()) };
        unsafe { (*c).pipe = pipe };
        if pipe == -1 {
            hr = win_hr();
            break 'run;
        }
        let mut m = XvdMsg::new(0x7800_00c0, 0x20);
        m.set_u8(4, 1);
        hr = unsafe { m.send_adapter(adapter) };
        if hr < 0 {
            break 'run;
        }
        let session = m.u32(0);
        unsafe { (*c).session = session };
        xlog(4, || format!("Session ID is {session}"));
        let ev = unsafe { CreateEventW(null_mut(), 1, 0, null()) };
        if ev == 0 {
            hr = win_hr();
            break 'run;
        }
        let mut ov = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: ev };
        let mut pending = false;
        if unsafe { ConnectNamedPipe(pipe, &mut ov) } != 0 {
            hr = HR_UNEXPECTED;
        } else {
            hr = win_hr();
            pending = hr == HR_IO_PENDING;
            if hr == HR_PIPE_CONNECTED {
                unsafe { SetEvent(ev) };
            }
            if hr == HR_PIPE_CONNECTED || hr == HR_IO_PENDING {
                match unsafe { WaitForSingleObject(ev, 5000) } {
                    0 => {
                        unsafe { *out = c };
                        keep = null_mut();
                        pending = false;
                        hr = 0;
                    }
                    0x102 => hr = HR_TIMEOUT,
                    _ => hr = win_hr(),
                }
            }
        }
        if pending {
            let mut got = 0u32;
            unsafe {
                CancelIoEx(pipe, &mut ov);
                GetOverlappedResult(pipe, &mut ov, &mut got, 1);
            }
        }
        unsafe { CloseHandle(ev) };
    }
    if !keep.is_null() {
        unsafe { XCrdXvdXferUninitialize(adapter, &mut keep) };
    }
    xlog(4, || format!("Initialize result 0x{:X}", hr as u32));
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXvdXferStart(c: *mut XferCtx, path: *const u16) -> i32 {
    if c.is_null() {
        xlog(1, || "Invalid transfer handle".into());
        return E_INVALIDARG;
    }
    if path.is_null() {
        xlog(1, || "Invalid CRD path".into());
        return E_INVALIDARG;
    }
    unsafe {
        if (*c).pipe == -1 || (*c).session == u32::MAX || (*c).transfer != u32::MAX {
            xlog(1, || "Invalid transfer context".into());
            return HR_INVALID_CONTEXT;
        }
    }
    let text = unsafe { wslice(path) };
    xlog(4, || format!("Starting transfer for {}", String::from_utf16_lossy(text)));
    let mut m = [0u8; XFER_MSG];
    wr32(&mut m, 0, 1);
    let hr = 'run: {
        if text.len() >= 0x104 {
            break 'run hr_from_win32(122);
        }
        for (i, ch) in text.iter().enumerate() {
            m[8 + i * 2..10 + i * 2].copy_from_slice(&ch.to_le_bytes());
        }
        let hr = unsafe { pipe_io(c, true, m.as_mut_ptr(), XFER_MSG as u32) };
        if hr < 0 {
            break 'run hr;
        }
        m = [0u8; XFER_MSG];
        let hr = unsafe { pipe_io(c, false, m.as_mut_ptr(), XFER_MSG as u32) };
        if hr < 0 {
            break 'run hr;
        }
        if rd32(&m, 0) != 2 {
            break 'run HR_BAD_COMMAND;
        }
        let hr = nt_to_hr(rd32(&m, 8) as i32);
        if hr < 0 {
            break 'run hr;
        }
        let id = rd32(&m, 0xc);
        unsafe { (*c).transfer = id };
        xlog(4, || format!("Transfer ID is {id}"));
        unsafe {
            InitializeCriticalSectionEx(cs(c), 0x1f40, 0);
            (*c).cs_ready = 1;
            (*c).slot_event = CreateEventW(null_mut(), 0, 0, null());
            if (*c).slot_event == 0 {
                break 'run win_hr();
            }
            (*c).thread = CreateThread(null(), 0, completion, c as *mut c_void, 0, null_mut());
            if (*c).thread == 0 {
                break 'run win_hr();
            }
            if (*c).failed != 0 {
                (*c).hr = 0;
                (*c).failed = 0;
            }
        }
        0
    };
    if hr < 0 {
        unsafe { XCrdXvdXferStop(c) };
    }
    xlog(4, || format!("Start result 0x{:X}", hr as u32));
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXvdXferRead(c: *mut XferCtx, offset: u64, len: u32, buffer: *mut u8, result: *mut Overlapped) -> i32 {
    if c.is_null() {
        xlog(1, || "Invalid transfer handle".into());
        return E_INVALIDARG;
    }
    if buffer.is_null() {
        xlog(1, || "Invalid buffer".into());
        return E_INVALIDARG;
    }
    if result.is_null() {
        xlog(1, || "Invalid overlapped".into());
        return E_INVALIDARG;
    }
    xlog(4, || format!("Reading 0x{len:X} bytes from offset 0x{offset:X}"));
    unsafe {
        if (*c).failed != 0 {
            return (*c).hr;
        }
    }
    let mut m = [0u8; XFER_MSG];
    wr32(&mut m, 0, 3);
    wr32(&mut m, 8, unsafe { (*c).transfer });
    wr32(&mut m, 0x10, len);
    m[0x18..0x20].copy_from_slice(&offset.to_le_bytes());
    let slot = loop {
        let mut found: *mut Slot = null_mut();
        unsafe {
            EnterCriticalSection(cs(c));
            for i in 0..XFER_SLOTS {
                let s = core::ptr::addr_of_mut!((*c).slots[i]);
                if (*s).busy == 0 {
                    wr32(&mut m, 0xc, i as u32);
                    (*s).busy = 1;
                    found = s;
                    break;
                }
            }
            LeaveCriticalSection(cs(c));
        }
        if !found.is_null() {
            break found;
        }
        unsafe { WaitForSingleObject((*c).slot_event, u32::MAX) };
    };
    unsafe {
        if (*c).failed != 0 {
            release_slot(c, slot);
            return (*c).hr;
        }
        (*slot).result = result;
        (*slot).buffer = buffer;
        (*slot).len = len;
    }
    let hr = unsafe { pipe_io(c, true, m.as_mut_ptr(), XFER_MSG as u32) };
    if hr >= 0 {
        HR_IO_PENDING
    } else {
        unsafe { release_slot(c, slot) };
        hr
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXvdXferGetResult(_c: *mut XferCtx, result: *mut Overlapped, bytes: *mut u32) -> i32 {
    unsafe {
        WaitForSingleObject((*result).event, u32::MAX);
        *bytes = (*result).internal_high as u32;
        (*result).internal as u32 as i32
    }
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXvdXferStop(c: *mut XferCtx) -> i32 {
    if c.is_null() {
        xlog(1, || "Invalid transfer handle".into());
        return E_INVALIDARG;
    }
    xlog(4, || format!("Stopping transfer {}", unsafe { (*c).transfer }));
    unsafe {
        set_failed(c, HR_NOT_READY);
        if (*c).thread != 0 {
            SetEvent((*c).stop);
            WaitForSingleObject((*c).thread, u32::MAX);
            CloseHandle((*c).thread);
            (*c).thread = 0;
            ResetEvent((*c).stop);
        }
        if (*c).slot_event != 0 {
            CloseHandle((*c).slot_event);
            (*c).slot_event = 0;
        }
        if (*c).cs_ready != 0 {
            DeleteCriticalSection(cs(c));
            (*c).cs_ready = 0;
        }
    }
    let id = unsafe { (*c).transfer };
    let hr = 'run: {
        if id == u32::MAX {
            break 'run 0;
        }
        let mut m = [0u8; XFER_MSG];
        wr32(&mut m, 0, 5);
        wr32(&mut m, 8, id);
        let hr = unsafe { pipe_io(c, true, m.as_mut_ptr(), XFER_MSG as u32) };
        if hr < 0 {
            break 'run hr;
        }
        loop {
            m = [0u8; XFER_MSG];
            let hr = unsafe { pipe_io(c, false, m.as_mut_ptr(), XFER_MSG as u32) };
            if hr < 0 {
                break 'run hr;
            }
            if rd32(&m, 0) != 4 {
                break;
            }
        }
        if rd32(&m, 0) != 6 {
            break 'run HR_BAD_COMMAND;
        }
        let hr = nt_to_hr(rd32(&m, 8) as i32);
        if hr < 0 {
            break 'run hr;
        }
        unsafe { (*c).transfer = u32::MAX };
        0
    };
    xlog(4, || format!("Stop result 0x{:X}", hr as u32));
    hr
}

#[no_mangle]
pub unsafe extern "system" fn XCrdXvdXferUninitialize(adapter: *mut c_void, pc: *mut *mut XferCtx) -> i32 {
    if adapter.is_null() {
        xlog(1, || "Invalid adapter handle".into());
        return E_INVALIDARG;
    }
    if pc.is_null() || unsafe { (*pc).is_null() } {
        xlog(1, || "Invalid transfer handle".into());
        return E_INVALIDARG;
    }
    let c = unsafe { *pc };
    xlog(4, || format!("Uninitializing transfer session {}", unsafe { (*c).session }));
    let hr = 'run: {
        let hr = unsafe { XCrdXvdXferStop(c) };
        if hr < 0 {
            break 'run hr;
        }
        if unsafe { (*c).session } != u32::MAX {
            let mut m = XvdMsg::new(0x7800_00c0, 0x20);
            m.set_u8(4, 0);
            m.set_u32(0, unsafe { (*c).session });
            let hr = unsafe { m.send_adapter(adapter) };
            if hr < 0 {
                break 'run hr;
            }
        }
        unsafe {
            if (*c).pipe != -1 {
                DisconnectNamedPipe((*c).pipe);
                CloseHandle((*c).pipe);
                (*c).pipe = -1;
            }
            if (*c).stop != 0 {
                CloseHandle((*c).stop);
                (*c).stop = 0;
            }
            heap_free(c as *mut c_void);
            *pc = null_mut();
        }
        0
    };
    xlog(4, || format!("Uninitialization result 0x{:X}", hr as u32));
    hr
}
