use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

type H = *mut c_void;

extern "system" {
    fn GetModuleHandleW(name: *const u16) -> H;
    fn GetModuleHandleExW(flags: u32, addr: *const u16, out: *mut H) -> i32;
    fn GetModuleFileNameW(h: H, buf: *mut u16, n: u32) -> u32;
    fn VirtualProtect(addr: *mut c_void, size: usize, prot: u32, old: *mut u32) -> i32;
    fn CreateThread(sa: *mut c_void, stack: usize, start: unsafe extern "system" fn(*mut c_void) -> u32, param: *mut c_void, flags: u32, id: *mut u32) -> H;
    fn CloseHandle(h: H) -> i32;
    fn Sleep(ms: u32);
}

const PAGE_READWRITE: u32 = 4;

fn log(msg: &str) {
    compat_common::record_raw("mfplat", "trace", msg);
}

fn module_of(addr: usize) -> String {
    let mut h: H = core::ptr::null_mut();
    if addr == 0 || unsafe { GetModuleHandleExW(4 | 2, addr as *const u16, &mut h) } == 0 {
        return "<none>".into();
    }
    let mut buf = [0u16; 520];
    let n = unsafe { GetModuleFileNameW(h, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let full = String::from_utf16_lossy(&buf[..n]);
    let base = full.rsplit(['\\', '/']).next().unwrap_or(&full).to_string();
    format!("{base}+{:#x}", addr - h as usize)
}

fn should_log(count: u64) -> bool {
    count <= 8 || count.is_power_of_two()
}

unsafe fn wstr(p: *const u16) -> String {
    if p.is_null() {
        return "<null>".into();
    }
    let mut n = 0;
    while n < 1024 && unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    String::from_utf16_lossy(unsafe { core::slice::from_raw_parts(p, n) })
}

unsafe fn out_ptr(p: *mut *mut c_void) -> usize {
    if p.is_null() { 0 } else { unsafe { *p as usize } }
}

macro_rules! traced {
    ($($idx:literal $dll:literal $name:ident ($($a:ident : $t:ty),*) -> $r:ty, |$hr:ident| $fmt:expr;)*) => {
        pub static TRACED: &[(&str, &str)] = &[$(($dll, stringify!($name))),*];
        static ORIG: [AtomicUsize; TRACED_N] = [const { AtomicUsize::new(0) }; TRACED_N];
        static CALLS: [AtomicU64; TRACED_N] = [const { AtomicU64::new(0) }; TRACED_N];
        pub mod thunks {
            #![allow(non_snake_case)]
            use super::*;
            $(
                pub unsafe extern "system" fn $name($($a: $t),*) -> $r {
                    let f: unsafe extern "system" fn($($t),*) -> $r = unsafe { core::mem::transmute(ORIG[$idx].load(Ordering::Relaxed)) };
                    let $hr = unsafe { f($($a),*) };
                    let n = CALLS[$idx].fetch_add(1, Ordering::Relaxed) + 1;
                    if should_log(n) {
                        #[allow(unused_unsafe)]
                        let body: String = unsafe { $fmt };
                        log(&format!("{}!{}#{} {}", $dll, stringify!($name), n, body));
                    }
                    $hr
                }
            )*
            pub fn addrs() -> [usize; TRACED_N] { [$($name as usize),*] }
        }
    };
}

const TRACED_N: usize = 18;

traced! {
    0 "MFPlat.DLL" MFStartup(version: u32, flags: u32) -> i32, |hr| format!("version={version:#x} flags={flags:#x} hr={:#010x}", hr as u32);
    1 "MFPlat.DLL" MFShutdown() -> i32, |hr| format!("hr={:#010x}", hr as u32);
    2 "MFPlat.DLL" MFCreateAttributes(out: *mut *mut c_void, size: u32) -> i32, |hr| format!("initial_size={size} hr={:#010x} attrs={:#x}", hr as u32, out_ptr(out));
    3 "MFPlat.DLL" MFCreateMediaType(out: *mut *mut c_void) -> i32, |hr| format!("hr={:#010x} type={:#x}", hr as u32, out_ptr(out));
    4 "MFPlat.DLL" MFCreateSample(out: *mut *mut c_void) -> i32, |hr| format!("hr={:#010x} sample={:#x}", hr as u32, out_ptr(out));
    5 "MFPlat.DLL" MFCreateMemoryBuffer(len: u32, out: *mut *mut c_void) -> i32, |hr| format!("max_length={len} hr={:#010x} buffer={:#x}", hr as u32, out_ptr(out));
    6 "MFPlat.DLL" MFCreateAlignedMemoryBuffer(len: u32, align: u32, out: *mut *mut c_void) -> i32, |hr| format!("max_length={len} align_mask={align:#x} hr={:#010x} buffer={:#x}", hr as u32, out_ptr(out));
    7 "MFPlat.DLL" MFCreateAsyncResult(obj: *mut c_void, cb: *mut c_void, state: *mut c_void, out: *mut *mut c_void) -> i32, |hr| format!("object={:#x} callback={:#x} state={:#x} hr={:#010x} result={:#x}", obj as usize, cb as usize, state as usize, hr as u32, out_ptr(out));
    8 "MFPlat.DLL" MFInvokeCallback(result: *mut c_void) -> i32, |hr| format!("result={:#x} hr={:#010x}", result as usize, hr as u32);
    9 "MFPlat.DLL" MFPutWorkItem2(queue: u32, prio: i32, cb: *mut c_void, state: *mut c_void) -> i32, |hr| format!("queue={queue:#x} priority={prio} callback={:#x} state={:#x} hr={:#010x}", cb as usize, state as usize, hr as u32);
    10 "MFPlat.DLL" MFCreateFile(access: u32, open: u32, fflags: u32, url: *const u16, out: *mut *mut c_void) -> i32, |hr| format!("access={access} open={open} flags={fflags:#x} url={:?} hr={:#010x} stream={:#x}", wstr(url), hr as u32, out_ptr(out));
    11 "MFPlat.DLL" MFCreateSourceResolver(out: *mut *mut c_void) -> i32, |hr| format!("hr={:#010x} resolver={:#x}", hr as u32, out_ptr(out));
    12 "MFPlat.DLL" MFCreateWaveFormatExFromMFMediaType(mt: *mut c_void, wfx: *mut *mut u8, size: *mut u32, flags: u32) -> i32, |hr| format!("type={:#x} flags={flags} hr={:#010x} size={}", mt as usize, hr as u32, if size.is_null() { 0 } else { *size });
    13 "MFPlat.DLL" MFllMulDiv(a: i64, b: i64, c: i64, d: i64) -> i64, |r| format!("({a},{b},{c},{d}) = {r}");
    14 "MFPlat.DLL" MFCreateDXGIDeviceManager(token: *mut u32, out: *mut *mut c_void) -> i32, |hr| format!("hr={:#010x} token={} manager={:#x}", hr as u32, if token.is_null() { 0 } else { *token }, out_ptr(out));
    15 "MFPlat.DLL" MFResetDXGIDeviceManagerX(mgr: *mut c_void, dev: *mut c_void, token: u32) -> i32, |hr| format!("manager={:#x} device={:#x} token={token} hr={:#010x}", mgr as usize, dev as usize, hr as u32);
    16 "MFReadWrite.dll" MFCreateSourceReaderFromByteStream(bs: *mut c_void, attrs: *mut c_void, out: *mut *mut c_void) -> i32, |hr| format!("stream={:#x} attrs={:#x} hr={:#010x} reader={:#x}", bs as usize, attrs as usize, hr as u32, out_ptr(out));
    17 "MFReadWrite.dll" MFCreateSourceReaderFromMediaSource(src: *mut c_void, attrs: *mut c_void, out: *mut *mut c_void) -> i32, |hr| format!("source={:#x} attrs={:#x} hr={:#010x} reader={:#x}", src as usize, attrs as usize, hr as u32, out_ptr(out));
}

pub struct Slot {
    pub dll: String,
    pub name: String,
    pub slot: *mut usize,
}

pub unsafe fn exe_slots(base: usize, wanted: &[(&str, &str)]) -> Vec<Slot> {
    let b = base as *const u8;
    let rd32 = |o: usize| unsafe { (b.add(o) as *const u32).read_unaligned() } as usize;
    let rd64 = |o: usize| unsafe { (b.add(o) as *const u64).read_unaligned() };
    let mut out = Vec::new();
    if base == 0 || unsafe { (b as *const u16).read_unaligned() } != 0x5a4d {
        return out;
    }
    let pe = rd32(0x3c);
    let opt = pe + 24;
    if unsafe { (b.add(opt) as *const u16).read_unaligned() } != 0x20b {
        return out;
    }
    let imp = rd32(opt + 112 + 8);
    if imp == 0 {
        return out;
    }
    let mut d = imp;
    loop {
        let oft = rd32(d);
        let name = rd32(d + 12);
        let ft = rd32(d + 16);
        if name == 0 {
            break;
        }
        let dll = unsafe { std::ffi::CStr::from_ptr(b.add(name) as *const i8) }.to_string_lossy().to_string();
        if wanted.iter().any(|(w, _)| w.eq_ignore_ascii_case(&dll)) {
            let lookup = if oft != 0 { oft } else { ft };
            let mut i = 0;
            loop {
                let thunk = rd64(lookup + i * 8);
                if thunk == 0 {
                    break;
                }
                if thunk & (1 << 63) == 0 {
                    let f = unsafe { std::ffi::CStr::from_ptr(b.add(thunk as usize + 2) as *const i8) }.to_string_lossy().to_string();
                    if wanted.iter().any(|(w, n)| w.eq_ignore_ascii_case(&dll) && *n == f) {
                        out.push(Slot { dll: dll.clone(), name: f, slot: unsafe { b.add(ft + i * 8) } as *mut usize });
                    }
                }
                i += 1;
            }
        }
        d += 20;
    }
    out
}

pub unsafe fn install(base: usize) -> usize {
    let slots = unsafe { exe_slots(base, TRACED) };
    if slots.is_empty() {
        return 0;
    }
    let mut patched = 0;
    for s in &slots {
        let Some(idx) = TRACED.iter().position(|(d, n)| d.eq_ignore_ascii_case(&s.dll) && *n == s.name) else { continue };
        let cur = unsafe { core::ptr::read_volatile(s.slot) };
        if cur == thunks::addrs()[idx] {
            continue;
        }
        log(&format!("bind {}!{} slot={:#x} -> {} ({cur:#x})", s.dll, s.name, s.slot as usize - base, module_of(cur)));
        if cur == 0 {
            continue;
        }
        ORIG[idx].store(cur, Ordering::SeqCst);
        let mut old = 0u32;
        if unsafe { VirtualProtect(s.slot as *mut c_void, 8, PAGE_READWRITE, &mut old) } == 0 {
            log(&format!("VirtualProtect failed on {}!{}; calls not traced", s.dll, s.name));
            continue;
        }
        unsafe { core::ptr::write_volatile(s.slot, thunks::addrs()[idx]) };
        unsafe { VirtualProtect(s.slot as *mut c_void, 8, old, &mut old) };
        patched += 1;
    }
    log(&format!("{} of {} traced imports found in the host EXE, {patched} wrapped", slots.len(), TRACED.len()));
    patched
}

unsafe extern "system" fn worker(_p: *mut c_void) -> u32 {
    let base = unsafe { GetModuleHandleW(core::ptr::null()) } as usize;
    for _ in 0..3000 {
        let slots = unsafe { exe_slots(base, TRACED) };
        if slots.is_empty() {
            return 0;
        }
        if slots.iter().all(|s| unsafe { core::ptr::read_volatile(s.slot) } != 0) {
            unsafe { install(base) };
            return 0;
        }
        unsafe { Sleep(1) };
    }
    log("host EXE import slots for MFPlat/MFReadWrite never bound; not traced");
    0
}

pub fn enabled() -> bool {
    std::env::var("TOMBOX_MF_TRACE").map(|v| v != "0").unwrap_or(true)
}

#[no_mangle]
pub extern "system" fn DllMain(_h: H, reason: u32, _r: *mut c_void) -> i32 {
    if reason == 1 && enabled() {
        let t = unsafe { CreateThread(core::ptr::null_mut(), 0, worker, core::ptr::null_mut(), 0, core::ptr::null_mut()) };
        if !t.is_null() {
            unsafe { CloseHandle(t) };
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "system" fn fake_startup(version: u32, flags: u32) -> i32 {
        (version ^ flags) as i32 & 0x7fff_ffff
    }

    unsafe extern "system" fn fake_reader(bs: *mut c_void, _a: *mut c_void, out: *mut *mut c_void) -> i32 {
        unsafe { *out = bs };
        0
    }

    fn put32(v: &mut [u8], o: usize, x: u32) {
        v[o..o + 4].copy_from_slice(&x.to_le_bytes());
    }

    fn put64(v: &mut [u8], o: usize, x: u64) {
        v[o..o + 8].copy_from_slice(&x.to_le_bytes());
    }

    fn image() -> Vec<u8> {
        let mut v = vec![0u8; 0x1000];
        v[0] = b'M';
        v[1] = b'Z';
        put32(&mut v, 0x3c, 0x40);
        v[0x40..0x44].copy_from_slice(b"PE\0\0");
        let opt = 0x40 + 24;
        v[opt] = 0x0b;
        v[opt + 1] = 0x02;
        put32(&mut v, opt + 120, 0x200);
        let mods: [(&str, &[&str], usize); 2] = [
            ("MFPlat.DLL", &["MFllMulDiv", "MFStartup"], 0x400),
            ("MFReadWrite.dll", &["MFCreateSourceReaderFromByteStream"], 0x600),
        ];
        for (i, (dll, funcs, at)) in mods.iter().enumerate() {
            let d = 0x200 + i * 20;
            let name = at + 0x180;
            v[name..name + dll.len()].copy_from_slice(dll.as_bytes());
            let ilt = *at;
            let iat = at + 0x80;
            for (j, f) in funcs.iter().enumerate() {
                let hint = at + 0x100 + j * 0x30;
                v[hint + 2..hint + 2 + f.len()].copy_from_slice(f.as_bytes());
                put64(&mut v, ilt + j * 8, hint as u64);
            }
            put32(&mut v, d, ilt as u32);
            put32(&mut v, d + 12, name as u32);
            put32(&mut v, d + 16, iat as u32);
        }
        v
    }

    #[test]
    fn install_wraps_named_slots_and_thunks_call_through() {
        std::env::set_var("TOMBOX_LOG", std::env::temp_dir().join("mfplat_fortnite_trace_test.log"));
        let mut img = image();
        let base = img.as_mut_ptr() as usize;
        let s_startup = base + 0x480 + 8;
        let s_muldiv = base + 0x480;
        let s_reader = base + 0x680;
        unsafe {
            *(s_startup as *mut usize) = fake_startup as usize;
            *(s_muldiv as *mut usize) = 0;
            *(s_reader as *mut usize) = fake_reader as usize;
        }
        let slots = unsafe { exe_slots(base, TRACED) };
        let names: Vec<String> = slots.iter().map(|s| format!("{}!{}", s.dll, s.name)).collect();
        assert_eq!(names, ["MFPlat.DLL!MFllMulDiv", "MFPlat.DLL!MFStartup", "MFReadWrite.dll!MFCreateSourceReaderFromByteStream"]);
        assert_eq!(unsafe { install(base) }, 2);
        let startup_idx = TRACED.iter().position(|(_, n)| *n == "MFStartup").unwrap();
        let reader_idx = TRACED.iter().position(|(_, n)| *n == "MFCreateSourceReaderFromByteStream").unwrap();
        assert_eq!(unsafe { *(s_startup as *const usize) }, thunks::addrs()[startup_idx]);
        assert_eq!(unsafe { *(s_reader as *const usize) }, thunks::addrs()[reader_idx]);
        assert_eq!(unsafe { *(s_muldiv as *const usize) }, 0, "unbound slot left alone");
        let f: unsafe extern "system" fn(u32, u32) -> i32 = unsafe { core::mem::transmute(*(s_startup as *const usize)) };
        assert_eq!(unsafe { f(0x20070, 0) }, 0x20070);
        let r: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> i32 = unsafe { core::mem::transmute(*(s_reader as *const usize)) };
        let mut out: *mut c_void = core::ptr::null_mut();
        assert_eq!(unsafe { r(0x1234 as *mut c_void, core::ptr::null_mut(), &mut out) }, 0);
        assert_eq!(out as usize, 0x1234);
        assert_eq!(CALLS[startup_idx].load(Ordering::SeqCst), 1);
        assert_eq!(unsafe { install(base) }, 0, "second install is idempotent");
        assert_eq!(TRACED.len(), TRACED_N);
        assert_eq!(thunks::addrs().len(), TRACED_N);
    }

    #[test]
    fn non_pe_and_empty_images_are_rejected() {
        let v = vec![0u8; 0x200];
        assert!(unsafe { exe_slots(v.as_ptr() as usize, TRACED) }.is_empty());
        assert!(unsafe { exe_slots(0, TRACED) }.is_empty());
    }

    #[test]
    fn log_throttle() {
        let logged: Vec<u64> = (1..=300).filter(|n| should_log(*n)).collect();
        assert_eq!(logged, [1, 2, 3, 4, 5, 6, 7, 8, 16, 32, 64, 128, 256]);
    }
}
