use core::ffi::c_void;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::heapcheck;

extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn K32EnumProcessModules(p: *mut c_void, mods: *mut *mut c_void, cb: u32, needed: *mut u32) -> i32;
    fn GetModuleFileNameW(m: *mut c_void, buf: *mut u16, n: u32) -> u32;
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub unsafe fn modules() -> Vec<String> {
    let mut v = vec![core::ptr::null_mut(); 1024];
    let mut need = 0u32;
    K32EnumProcessModules(GetCurrentProcess(), v.as_mut_ptr(), (v.len() * 8) as u32, &mut need);
    v.truncate((need as usize / 8).min(1024));
    v.into_iter()
        .map(|m| {
            let mut b = [0u16; 520];
            let n = GetModuleFileNameW(m, b.as_mut_ptr(), b.len() as u32) as usize;
            format!("{} @{:#x}", String::from_utf16_lossy(&b[..n]), m as usize)
        })
        .collect()
}

struct G(*mut c_void);

impl G {
    unsafe fn f<T: Copy>(&self, name: &str) -> T {
        let n = format!("{name}\0");
        let p = GetProcAddress(self.0, n.as_ptr());
        assert!(!p.is_null(), "GameChat2 export {name} missing");
        core::mem::transmute_copy(&p)
    }
}

#[repr(C)]
struct DataFrame {
    target_count: u32,
    targets: *const u64,
    transport: u32,
    size: u32,
    buffer: *const u8,
}

type Init = unsafe extern "system" fn(u32, f32, u32, u32, u32, u32) -> i32;
type AddLocal = unsafe extern "system" fn(*const u16, *mut *mut c_void) -> i32;
type AddRemote = unsafe extern "system" fn(*const u16, u64, *mut *mut c_void) -> i32;
type StartList = unsafe extern "system" fn(*mut u32, *mut *const *const c_void) -> i32;
type FinishList = unsafe extern "system" fn(*const *const c_void) -> i32;
type Incoming = unsafe extern "system" fn(u64, u32, *const u8) -> i32;
type Cleanup = unsafe extern "system" fn() -> i32;
type Relation = unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> i32;
type Mute = unsafe extern "system" fn(*mut c_void, u8) -> i32;

extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut u8;
    fn VirtualAlloc(addr: *mut c_void, size: usize, kind: u32, prot: u32) -> *mut u8;
    fn VirtualProtect(addr: *mut c_void, size: usize, prot: u32, old: *mut u32) -> i32;
}

unsafe fn rd32(p: *const u8) -> u32 {
    core::ptr::read_unaligned(p as *const u32)
}

pub struct Counters {
    names: Vec<String>,
    page: *mut u8,
}

impl Counters {
    pub unsafe fn get(&self) -> Vec<(String, u64)> {
        self.names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), core::ptr::read_volatile(self.page.add(0x800 + i * 8) as *const u64)))
            .filter(|(_, c)| *c > 0)
            .collect()
    }
}

pub unsafe fn count_exports(dll: &str) -> Option<Counters> {
    let base = GetModuleHandleW(w(dll).as_ptr());
    if base.is_null() {
        return None;
    }
    let pe = base.add(rd32(base.add(0x3c)) as usize);
    let size_of_image = rd32(pe.add(0x50)) as usize;
    let dir_rva = rd32(pe.add(0x88)) as usize;
    if dir_rva == 0 {
        return None;
    }
    let dir = base.add(dir_rva);
    let nnames = rd32(dir.add(0x18)) as usize;
    let funcs = base.add(rd32(dir.add(0x1c)) as usize) as *mut u32;
    let names = base.add(rd32(dir.add(0x20)) as usize) as *const u32;
    let ords = base.add(rd32(dir.add(0x24)) as usize) as *const u16;
    let mut page = core::ptr::null_mut();
    let mut at = (base as usize + size_of_image + 0xffff) & !0xffff;
    for _ in 0..4096 {
        page = VirtualAlloc(at as *mut c_void, 0x1000, 0x3000, 0x40);
        if !page.is_null() {
            break;
        }
        at += 0x10000;
    }
    if page.is_null() || (page as usize - base as usize) > u32::MAX as usize || nnames > 60 {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..nnames {
        let np = base.add(*names.add(i) as usize);
        let mut n = 0;
        while *np.add(n) != 0 {
            n += 1;
        }
        let name = String::from_utf8_lossy(core::slice::from_raw_parts(np, n)).into_owned();
        let slot = funcs.add(*ords.add(i) as usize);
        let target = base as usize + *slot as usize;
        let th = page.add(i * 32);
        let ctr = page.add(0x800 + i * 8);
        let rel = (ctr as isize - (th as isize + 8)) as i32;
        let mut code = vec![0xF0u8, 0x48, 0xFF, 0x05];
        code.extend_from_slice(&rel.to_le_bytes());
        code.extend_from_slice(&[0x48, 0xB8]);
        code.extend_from_slice(&(target as u64).to_le_bytes());
        code.extend_from_slice(&[0xFF, 0xE0]);
        core::ptr::copy_nonoverlapping(code.as_ptr(), th, code.len());
        let mut old = 0;
        VirtualProtect(slot as *mut c_void, 4, 0x04, &mut old);
        *slot = (th as usize - base as usize) as u32;
        VirtualProtect(slot as *mut c_void, 4, old, &mut old);
        out.push(name);
    }
    Some(Counters { names: out, page })
}

pub unsafe fn hook_count(base: *mut u8, rva: usize, steal: usize, slot: usize, page: *mut u8) -> *const u64 {
    let f = base.add(rva);
    let th = page.add(slot * 64);
    let ctr = page.add(0xc00 + slot * 8);
    let rel = (ctr as isize - (th as isize + 8)) as i32;
    let mut code = vec![0xF0u8, 0x48, 0xFF, 0x05];
    code.extend_from_slice(&rel.to_le_bytes());
    code.extend_from_slice(core::slice::from_raw_parts(f, steal));
    code.extend_from_slice(&[0x48, 0xB8]);
    code.extend_from_slice(&(f.add(steal) as u64).to_le_bytes());
    code.extend_from_slice(&[0xFF, 0xE0]);
    core::ptr::copy_nonoverlapping(code.as_ptr(), th, code.len());
    let mut patch = vec![0x48u8, 0xB8];
    patch.extend_from_slice(&(th as u64).to_le_bytes());
    patch.extend_from_slice(&[0xFF, 0xE0]);
    let mut old = 0;
    VirtualProtect(f as *mut c_void, patch.len(), 0x40, &mut old);
    core::ptr::copy_nonoverlapping(patch.as_ptr(), f, patch.len());
    VirtualProtect(f as *mut c_void, patch.len(), old, &mut old);
    ctr as *const u64
}

pub fn run(gamechat: &Path, xuid: u64, seconds: u64, report: &mut Vec<String>) -> Result<String, String> {
    heapcheck::enable();
    if let Ok(d) = std::env::var("XHOST_DLL_SEARCH") {
        extern "system" {
            fn SetDllDirectoryW(p: *const u16) -> i32;
        }
        unsafe { SetDllDirectoryW(w(&d).as_ptr()) };
    }
    let hal =unsafe { count_exports("AudioHALWrapper.dll") };
    let xa = unsafe { count_exports("XAudio2_9.dll") };
    heapcheck::phase("load GameChat2");
    heapcheck::watch_start(500);
    let before: Vec<String> = unsafe { modules() };
    let h = unsafe { LoadLibraryW(w(&gamechat.to_string_lossy()).as_ptr()) };
    if h.is_null() {
        return Err(format!("cannot load {}", gamechat.display()));
    }
    let g = G(h);
    let mut log = Vec::new();
    let gb = h as *mut u8;
    let probes: Vec<(&str, *const u64)> = unsafe {
        let mut page = core::ptr::null_mut();
        let mut at = (gb as usize + 0x200000) & !0xffff;
        for _ in 0..4096 {
            page = VirtualAlloc(at as *mut c_void, 0x1000, 0x3000, 0x40);
            if !page.is_null() {
                break;
            }
            at += 0x10000;
        }
        let ok = |rva: usize, want: &[u8]| core::slice::from_raw_parts(gb.add(rva), want.len()) == want;
        let mut v = Vec::new();
        if !page.is_null() && ok(0x4561c, &[0x4c, 0x8b, 0xdc]) && ok(0x47168, &[0x48, 0x89, 0x5c, 0x24, 0x08]) {
            v.push(("codec factory (opus)", hook_count(gb, 0x4561c, 15, 0, page)));
            v.push(("AudioHALWrapper loader", hook_count(gb, 0x47168, 15, 1, page)));
        } else {
            log.push("GameChat2 build differs from the analysed one: no code probes".into());
        }
        v
    };
    unsafe {
        let init: Init = g.f("ChatManagerInitialize");
        let add_l: AddLocal = g.f("ChatManagerAddLocalUser");
        let add_r: AddRemote = g.f("ChatManagerAddRemoteUser");
        let sdf: StartList = g.f("ChatManagerStartProcessingDataFrames");
        let fdf: FinishList = g.f("ChatManagerFinishProcessingDataFrames");
        let ssc: StartList = g.f("ChatManagerStartProcessingStateChanges");
        let fsc: FinishList = g.f("ChatManagerFinishProcessingStateChanges");
        let sss: StartList = g.f("ChatManagerStartProcessingStreamStateChanges");
        let fss: FinishList = g.f("ChatManagerFinishProcessingStreamStateChanges");
        let inc: Incoming = g.f("ChatManagerProcessIncomingPacket");
        let cleanup: Cleanup = g.f("ChatManagerCleanup");
        let rel: Relation = g.f("ChatUserSetCommunicationRelationship");
        let mute: Mute = g.f("ChatUserSetMicrophoneMuted");
        heapcheck::phase("ChatManagerInitialize");
        let hr = init(8, 1.0, 0x1f, 0, 0, 0);
        log.push(format!("ChatManagerInitialize(8, 1.0, 0x1f, 0, 0, 0) hr={hr:#010x}"));
        if hr < 0 {
            heapcheck::watch_stop();
            return Err(log.join("; "));
        }
        heapcheck::phase("AddLocalUser");
        let lx = w(&xuid.to_string());
        let mut lu = core::ptr::null_mut();
        let hr = add_l(lx.as_ptr(), &mut lu);
        log.push(format!("AddLocalUser({xuid}) hr={hr:#010x} user={lu:p}"));
        heapcheck::phase("AddRemoteUser");
        let rx = w("2535400000000002");
        let mut ru = core::ptr::null_mut();
        let hr = add_r(rx.as_ptr(), 1, &mut ru);
        log.push(format!("AddRemoteUser(endpoint 1) hr={hr:#010x} user={ru:p}"));
        if !lu.is_null() && !ru.is_null() {
            let hr = rel(lu, ru, 0x1f);
            log.push(format!("SetCommunicationRelationship hr={hr:#010x}"));
            let hr = mute(lu, 0);
            log.push(format!("SetMicrophoneMuted(0) hr={hr:#010x}"));
        }
        let t0 = Instant::now();
        let (mut frames, mut bytes, mut changes, mut streams, mut fed, mut iters) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
        let mut first_frame = String::new();
        while t0.elapsed() < Duration::from_secs(seconds) && heapcheck::FIRST_BAD.lock().unwrap().is_none() {
            iters += 1;
            heapcheck::phase(&format!("state changes iter {iters}"));
            let (mut n, mut arr) = (0u32, core::ptr::null());
            if ssc(&mut n, &mut arr) >= 0 {
                changes += n as usize;
                fsc(arr);
            }
            heapcheck::phase(&format!("stream state changes iter {iters}"));
            let (mut n, mut arr) = (0u32, core::ptr::null());
            if sss(&mut n, &mut arr) >= 0 {
                streams += n as usize;
                fss(arr);
            }
            heapcheck::phase(&format!("data frames iter {iters}"));
            let (mut n, mut arr) = (0u32, core::ptr::null());
            if sdf(&mut n, &mut arr) >= 0 {
                let mut pkts = Vec::new();
                for i in 0..n as usize {
                    let f = *arr.add(i) as *const DataFrame;
                    if f.is_null() {
                        continue;
                    }
                    let f = &*f;
                    if first_frame.is_empty() {
                        first_frame = format!("targets={} transport={} size={}", f.target_count, f.transport, f.size);
                    }
                    frames += 1;
                    bytes += f.size as usize;
                    if !f.buffer.is_null() && f.size > 0 && f.size < 0x10000 {
                        pkts.push(core::slice::from_raw_parts(f.buffer, f.size as usize).to_vec());
                    }
                }
                fdf(arr);
                for p in pkts {
                    heapcheck::phase(&format!("ProcessIncomingPacket loopback iter {iters} ({} bytes)", p.len()));
                    inc(1, p.len() as u32, p.as_ptr());
                    fed += 1;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        log.push(format!("pumped {iters} iterations in {:.1}s: {changes} state changes, {streams} stream changes, {frames} data frames ({bytes} bytes, first: {first_frame}), {fed} looped back", t0.elapsed().as_secs_f64()));
        if heapcheck::FIRST_BAD.lock().unwrap().is_none() {
            heapcheck::phase("ChatManagerCleanup");
            let hr = cleanup();
            log.push(format!("ChatManagerCleanup hr={hr:#010x}"));
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    heapcheck::watch_stop();
    std::thread::sleep(Duration::from_millis(5));
    let after: Vec<String> = unsafe { modules() };
    let strip = |s: &String| s.split(" @").next().unwrap_or("").to_ascii_lowercase();
    let old: Vec<String> = before.iter().map(strip).collect();
    for m in &after {
        if !old.contains(&strip(m)) {
            report.push(m.clone());
        }
    }
    for (n, c) in &probes {
        log.push(format!("{n} calls={}", unsafe { core::ptr::read_volatile(*c) }));
    }
    if !probes.is_empty() {
        log.push(format!("codec mode global={} (2 = Windows.Xbox hardware codecs, 1 = desktop, 0 = not decided yet)", unsafe { core::ptr::read_volatile(gb.add(0x111d9c) as *const u32) }));
    }
    for (dll, c) in [("AudioHALWrapper", &hal), ("XAudio2_9", &xa)] {
        match c {
            Some(c) => {
                let v = unsafe { c.get() };
                log.push(format!("{dll} export calls: {}", if v.is_empty() { "none".into() } else { v.iter().map(|(n, k)| format!("{n}={k}")).collect::<Vec<_>>().join(" ") }));
            }
            None => log.push(format!("{dll} export counting unavailable")),
        }
    }
    let bad = heapcheck::FIRST_BAD.lock().unwrap().clone();
    match bad.or_else(|| heapcheck::validate().err()) {
        Some(e) => Err(format!("{}; HEAP CORRUPTED: {e}", log.join("; "))),
        None => Ok(log.join("; ")),
    }
}
