use core::ffi::c_void;
#[link(name = "kernel32")]
extern "system" {
    
    
    fn AddVectoredExceptionHandler(first: u32, h: unsafe extern "system" fn(*mut *mut usize) -> i32) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, a: *const u16, out: *mut usize) -> i32;
    fn GetModuleFileNameW(m: usize, b: *mut u16, n: u32) -> u32;
    fn VirtualQuery(a: *const c_void, info: *mut [u8; 48], len: usize) -> usize;
    fn ExitProcess(c: u32);
}
unsafe fn modname(a: usize) -> String {
    let mut h = 0usize;
    if GetModuleHandleExW(6, a as *const u16, &mut h) == 0 {
        return format!("{a:#x} (no module)");
    }
    let mut b = [0u16; 512];
    let n = GetModuleFileNameW(h, b.as_mut_ptr(), 512) as usize;
    format!("{a:#x} = {}+{:#x}", String::from_utf16_lossy(&b[..n]), a - h)
}
unsafe extern "system" fn veh(p: *mut *mut usize) -> i32 {
    let rec = *p;
    let code = *(rec as *const u32);
    if code != 0xc0000005 {
        return 0;
    }
    let addr = *(rec.add(2));
    let kind = *(rec.add(4));
    let target = *(rec.add(5));
    let mut info = [0u8; 48];
    VirtualQuery(target as *const c_void, &mut info, 48);
    let prot = u32::from_le_bytes([info[36], info[37], info[38], info[39]]);
    { use std::io::Write; let mut f = std::fs::OpenOptions::new().create(true).append(true).open(r"D:\TomBox\target-xmemrace\veh.txt").unwrap(); let _ = writeln!(f, "AV at {} kind={kind} target={} prot={prot:#x}", modname(addr), modname(target)); let _ = writeln!(f, "{}", std::backtrace::Backtrace::force_capture()); }
    ExitProcess(77);
    0
}
pub(crate) fn install() { static O: std::sync::Once = std::sync::Once::new(); O.call_once(|| unsafe { AddVectoredExceptionHandler(1, veh); }); }
