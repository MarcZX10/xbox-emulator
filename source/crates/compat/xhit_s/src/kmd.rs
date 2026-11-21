use core::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const IOCTL_DOORBELL: u32 = 0x7900_c19c;
pub const DOORBELL_MAGIC: u32 = 0x4321_4321;
pub const MAILBOX_SIZE: usize = 0x1b58;
pub const DATA_SIZE: usize = 0x9_1118;
pub const MB_SEQ: usize = 0x1430;
pub const MB_CMD: usize = 0x1434;
pub const MB_ARGS: usize = 0x1438;
pub const MB_PAYLOAD_SIZE: usize = 0x1448;
pub const MB_PAYLOAD: usize = 0x144c;
pub const PAYLOAD_LEN: usize = 0x58;
pub const D_ACK: usize = 0xa040;
pub const D_STATUS: usize = 0xa048;
pub const D_SIZE: usize = 0xa04c;
pub const D_RESP: usize = 0xa050;
pub const D_RESP5: usize = 0x4_a050;
pub const D_RESP7: usize = 0x8_a050;
pub const RESP_MAX: u32 = 0x70c8;
pub const CMD_SQ: u32 = 0xd;
pub const CMD_HANG: u32 = 0xe;

pub type Doorbell = unsafe fn(data: *mut u8, mailbox: *mut u8);

static DATA: AtomicUsize = AtomicUsize::new(0);
static MAILBOX: AtomicUsize = AtomicUsize::new(0);
static BELL: AtomicUsize = AtomicUsize::new(0);

#[link(name = "kernel32")]
extern "system" {
    fn DeviceIoControl(h: isize, code: u32, i: *const c_void, il: u32, o: *mut c_void, ol: u32, ret: *mut u32, ov: *mut c_void) -> i32;
    fn Sleep(ms: u32);
}

pub unsafe fn install(data: *mut u8, mailbox: *mut u8, doorbell: Doorbell) {
    DATA.store(data as usize, Ordering::SeqCst);
    MAILBOX.store(mailbox as usize, Ordering::SeqCst);
    BELL.store(doorbell as usize, Ordering::SeqCst);
}

pub fn uninstall() {
    DATA.store(0, Ordering::SeqCst);
    MAILBOX.store(0, Ordering::SeqCst);
    BELL.store(0, Ordering::SeqCst);
}

pub fn installed() -> Option<(usize, usize)> {
    let d = DATA.load(Ordering::SeqCst);
    let m = MAILBOX.load(Ordering::SeqCst);
    if d == 0 || m == 0 {
        None
    } else {
        Some((d, m))
    }
}

unsafe fn rd(p: usize, off: usize) -> u32 {
    ((p + off) as *const u32).read_volatile()
}

unsafe fn wr(p: usize, off: usize, v: u32) {
    ((p + off) as *mut u32).write_volatile(v)
}

pub unsafe fn ring(data: usize, mailbox: usize) -> i32 {
    let b = BELL.load(Ordering::SeqCst);
    if b != 0 {
        let f: Doorbell = core::mem::transmute(b);
        f(data as *mut u8, mailbox as *mut u8);
        return 0;
    }
    let h = crate::open_device().unwrap_or(-1);
    if h == -1 {
        return 0x8007_0057u32 as i32;
    }
    let inb = [DOORBELL_MAGIC, 1u32];
    let mut out = 0u32;
    let mut ret = 0u32;
    let ok = DeviceIoControl(h, IOCTL_DOORBELL, inb.as_ptr() as *const c_void, 8, &mut out as *mut u32 as *mut c_void, 4, &mut ret, core::ptr::null_mut());
    if ok == 0 {
        crate::hr_last()
    } else if out != 0 {
        crate::E_FAIL
    } else {
        0
    }
}

pub fn acked(data: usize, mailbox: usize) -> bool {
    unsafe { rd(data, D_ACK) == rd(mailbox, MB_SEQ) }
}

fn kick(data: usize, mailbox: usize) {
    core::sync::atomic::fence(Ordering::SeqCst);
    unsafe { wr(mailbox, MB_SEQ, rd(mailbox, MB_SEQ).wrapping_add(1)) };
    core::sync::atomic::fence(Ordering::SeqCst);
    unsafe { ring(data, mailbox) };
}

pub unsafe fn post_hang(regions: Option<(usize, usize)>, ctx: *const u8) -> i32 {
    let (data, mailbox) = match regions {
        Some(r) => r,
        None => return crate::E_FAIL,
    };
    if !acked(data, mailbox) {
        return crate::E_UNEXPECTED;
    }
    core::ptr::write_bytes((mailbox + MB_ARGS) as *mut u8, 0, 16);
    let mut n = 0u32;
    if !ctx.is_null() {
        core::ptr::copy_nonoverlapping(ctx, (mailbox + MB_PAYLOAD) as *mut u8, PAYLOAD_LEN);
        n = PAYLOAD_LEN as u32;
    }
    wr(mailbox, MB_PAYLOAD_SIZE, n);
    wr(mailbox, MB_CMD, CMD_HANG);
    kick(data, mailbox);
    0
}

pub unsafe fn post(regions: (usize, usize), cmd: u32, args: Option<&[u32; 4]>) -> i32 {
    let (data, mailbox) = regions;
    if !acked(data, mailbox) {
        return crate::E_UNEXPECTED;
    }
    match args {
        Some(a) => {
            for (i, v) in a.iter().enumerate() {
                wr(mailbox, MB_ARGS + i * 4, *v);
            }
        }
        None => core::ptr::write_bytes((mailbox + MB_ARGS) as *mut u8, 0, 16),
    }
    wr(mailbox, MB_ARGS + 0x10, 0);
    wr(mailbox, MB_CMD, cmd);
    kick(data, mailbox);
    0
}

pub unsafe fn response(regions: (usize, usize)) -> (usize, u32, u32) {
    let (data, mailbox) = regions;
    let p = match rd(mailbox, MB_CMD) {
        5 => data + D_RESP5,
        7 => data + D_RESP7,
        _ => data + D_RESP,
    };
    (p, rd(data, D_SIZE), rd(data, D_STATUS))
}

pub fn spin_ack(regions: (usize, usize)) {
    while !acked(regions.0, regions.1) {
        core::hint::spin_loop();
    }
}

pub fn sleep_ack(regions: (usize, usize)) {
    while !acked(regions.0, regions.1) {
        unsafe { Sleep(5) };
    }
}

pub unsafe fn request(regions: (usize, usize), cmd: u32, args: Option<&[u32; 4]>) -> Result<(usize, u32, u32), i32> {
    let hr = post(regions, cmd, args);
    if hr != 0 {
        return Err(hr);
    }
    spin_ack(regions);
    Ok(response(regions))
}
