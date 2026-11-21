use core::ffi::c_void;
use std::path::Path;

use crate::heapcheck;

extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

struct Lib(*mut c_void);

impl Lib {
    unsafe fn f<T: Copy>(&self, name: &str) -> T {
        let n = format!("{name}\0");
        let p = GetProcAddress(self.0, n.as_ptr());
        assert!(!p.is_null(), "export {name} missing");
        core::mem::transmute_copy(&p)
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
}

fn pcm(r: &mut Rng, n: usize, t: usize) -> Vec<i16> {
    (0..n).map(|i| (((i + t) as f32 * 0.07).sin() * 8000.0) as i16 ^ (r.next() as i16 & 0x3f)).collect()
}

fn check(stage: &str) -> Result<(), String> {
    heapcheck::validate().map(|_| ()).map_err(|e| format!("after {stage}: {e}"))
}

type Create = unsafe extern "system" fn(*mut *mut c_void) -> i32;
type Destroy = unsafe extern "system" fn(*mut c_void);
type AcqEnc = unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, *mut u32) -> i32;
type AcqDec = unsafe extern "system" fn(*mut c_void, u32, u32, *mut u32) -> i32;
type Rel = unsafe extern "system" fn(*mut c_void, u32) -> i32;
type Submit = unsafe extern "system" fn(*mut c_void, u32, *const u8, u16) -> i32;
type Output = unsafe extern "system" fn(*mut c_void, u32, u8, *mut u32, *mut u8, *mut u16) -> i32;
type SAcq = unsafe extern "system" fn(*mut c_void, *mut u32) -> i32;
type SEnc = unsafe extern "system" fn(*mut c_void, u32, *const i16, i16, *mut u8, *mut i16, *mut [i32; 8], u8) -> i32;
type SEncOut = unsafe extern "system" fn(*mut c_void, u32, *mut u8, *mut i16, *mut [i32; 8]) -> i32;
type SDec = unsafe extern "system" fn(*mut c_void, u32, *const u8, u32, *mut i16, *mut i16, *mut [i32; 6], u8, u8) -> i32;
type SDecOut = unsafe extern "system" fn(*mut c_void, u32, *mut i16, *mut i16, *mut [i32; 6]) -> i32;

#[repr(C)]
struct Guarded<T: Copy> {
    v: T,
    tail: [u8; 64],
}

impl<T: Copy> Guarded<T> {
    fn new(v: T) -> Self {
        Guarded { v, tail: [0xc3; 64] }
    }
    fn ok(&self) -> bool {
        self.tail.iter().all(|&b| b == 0xc3)
    }
}

unsafe fn heap_buf(n: usize) -> (*mut u8, usize) {
    let total = n + 64;
    let p = std::alloc::alloc(std::alloc::Layout::from_size_align(total, 16).unwrap());
    core::ptr::write_bytes(p, 0, n);
    core::ptr::write_bytes(p.add(n), 0xc3, 64);
    (p, n)
}

unsafe fn heap_ok(p: *mut u8, n: usize) -> bool {
    core::slice::from_raw_parts(p.add(n), 64).iter().all(|&b| b == 0xc3)
}

unsafe fn heap_free(p: *mut u8, n: usize) {
    std::alloc::dealloc(p, std::alloc::Layout::from_size_align(n + 64, 16).unwrap());
}

pub fn gamechat2(dir: &Path, frames: usize) -> Result<String, String> {
    heapcheck::enable();
    let path = dir.join("AudioHALWrapper.dll");
    let lib = unsafe { LoadLibraryW(w(&path.to_string_lossy()).as_ptr()) };
    if lib.is_null() {
        return Err(format!("cannot load {}", path.display()));
    }
    let l = Lib(lib);
    let mut rep = String::new();
    let mut r = Rng(0x0bad_cafe_f00d_1234);
    unsafe {
        let (ce, cd): (Create, Create) = (l.f("CreateOpusEncoderHardwareInterface"), l.f("CreateOpusDecoderHardwareInterface"));
        let (de, dd): (Destroy, Destroy) = (l.f("DestroyOpusEncoderHardwareInterface"), l.f("DestroyOpusDecoderHardwareInterface"));
        let acq_e: AcqEnc = l.f("AcquireEncoder");
        let acq_d: AcqDec = l.f("AcquireDecoder");
        let enc: Submit = l.f("EncodeFrame");
        let dec: Submit = l.f("DecodePacket");
        let (eo, dout): (Output, Output) = (l.f("GetEncodeOutput"), l.f("GetDecodeOutput"));
        let infl: unsafe extern "system" fn(*mut c_void, u32, *mut u8) -> i32 = l.f("GetIsDataInFlightForDecoder");
        let infl_e: unsafe extern "system" fn(*mut c_void, u32, *mut u8) -> i32 = l.f("GetIsDataInFlightForEncoder");
        for round in 0..3 {
            let (mut he, mut hd) = (core::ptr::null_mut(), core::ptr::null_mut());
            let hr = (cd(&mut hd), ce(&mut he));
            if hr.0 != 0 || hr.1 != 0 {
                return Err(format!("round {round}: opus create dec={:#x} enc={:#x}", hr.0, hr.1));
            }
            let mut decs = Vec::new();
            for _ in 0..15 {
                let mut g = Guarded::new(0u32);
                let a = acq_d(hd, 0x5dc0, 1, &mut g.v);
                if !g.ok() {
                    return Err("AcquireDecoder wrote past its u32 index".into());
                }
                if a != 0 {
                    break;
                }
                if g.v >= 15 {
                    return Err(format!("AcquireDecoder index {} >= 15 overflows GameChat2's 15-slot table", g.v));
                }
                decs.push(g.v);
            }
            let mut encs = Vec::new();
            for &(rate, ch) in &[(24000u32, 1u32), (48000, 1), (16000, 1)] {
                let mut g = Guarded::new(0u32);
                let a = acq_e(he, rate, ch, 24000, 0, &mut g.v);
                if !g.ok() {
                    return Err("AcquireEncoder wrote past its u32 index".into());
                }
                if a == 0 {
                    if g.v >= 3 {
                        return Err(format!("AcquireEncoder index {} >= 3 overflows GameChat2's 3-slot table", g.v));
                    }
                    encs.push((g.v, rate, ch));
                }
            }
            let (ob, on) = heap_buf(0x780);
            let (db, dn) = heap_buf(0x780);
            let mut packets = 0usize;
            for f in 0..frames {
                for &(ei, rate, ch) in &encs {
                    let n = rate as usize / 50 * ch as usize;
                    let p = pcm(&mut r, n, f * n);
                    let mut fl = Guarded::new(0u8);
                    infl_e(he, ei, &mut fl.v);
                    if !fl.ok() {
                        return Err("GetIsDataInFlightForEncoder wrote past its byte".into());
                    }
                    let e = enc(he, ei, p.as_ptr() as *const u8, (n * 2) as u16);
                    let mut sz = Guarded::new(on as u16);
                    let mut ticks = Guarded::new(0u32);
                    let o = eo(he, ei, 1, &mut ticks.v, ob, &mut sz.v);
                    if !heap_ok(ob, on) || !sz.ok() || !ticks.ok() {
                        return Err(format!("GetEncodeOutput overflow (enc {ei} {rate}/{ch} frame {f}, size {:#x})", sz.v));
                    }
                    if e != 0 || o != 0 || sz.v == 0 {
                        continue;
                    }
                    packets += 1;
                    for (k, &di) in decs.iter().enumerate() {
                        let lost = (f + k) % 9 == 4;
                        let mut fl = Guarded::new(0u8);
                        infl(hd, di, &mut fl.v);
                        let d = if lost { dec(hd, di, core::ptr::null(), 0) } else { dec(hd, di, ob, sz.v) };
                        let mut dsz = Guarded::new(dn as u16);
                        let mut dt = Guarded::new(0u32);
                        let o2 = dout(hd, di, 1, &mut dt.v, db, &mut dsz.v);
                        if !heap_ok(db, dn) || !dsz.ok() || !dt.ok() || !fl.ok() {
                            return Err(format!("GetDecodeOutput overflow (dec {di} frame {f}, size {:#x} ret {o2:#x} submit {d:#x})", dsz.v));
                        }
                    }
                }
                if f % 25 == 0 {
                    check(&format!("gc2 opus round {round} frame {f}"))?;
                }
            }
            heap_free(ob, on);
            heap_free(db, dn);
            dd(hd);
            de(he);
            check(&format!("gc2 opus round {round} destroy without release"))?;
            rep.push_str(&format!("[opus round {round}: {} decoders 24k mono, {} encoders, {packets} packets x{} decoders] ", decs.len(), encs.len(), decs.len()));
        }
        let cs: Create = l.f("CreateSilkHardwareInterface");
        let ds: Destroy = l.f("DestroySilkHardwareInterface");
        let (sae, sad): (SAcq, SAcq) = (l.f("SilkAcquireEncoder"), l.f("SilkAcquireDecoder"));
        let se: SEnc = l.f("SilkEncodeFrame");
        let seo: SEncOut = l.f("SilkGetEncodeOutput");
        let sd: SDec = l.f("SilkDecodeFrame");
        let sdo: SDecOut = l.f("SilkGetDecodeOutput");
        for round in 0..2 {
            let mut hs = core::ptr::null_mut();
            let h = cs(&mut hs);
            if h != 0 {
                rep.push_str(&format!("[silk create {h:#x}] "));
                break;
            }
            let mut decs = Vec::new();
            for _ in 0..7 {
                let mut g = Guarded::new(0u32);
                if sad(hs, &mut g.v) != 0 {
                    break;
                }
                if !g.ok() || g.v >= 7 {
                    return Err(format!("SilkAcquireDecoder index {} (GameChat2 table holds 7)", g.v));
                }
                decs.push(g.v);
            }
            let mut encs = Vec::new();
            for _ in 0..2 {
                let mut g = Guarded::new(0u32);
                if sae(hs, &mut g.v) != 0 {
                    break;
                }
                if !g.ok() || g.v >= 2 {
                    return Err(format!("SilkAcquireEncoder index {} (GameChat2 table holds 2)", g.v));
                }
                encs.push(g.v);
            }
            let (eb, en) = heap_buf(0x8000);
            let (db, dn) = heap_buf(0x8000);
            let mut stats = (0usize, 0usize, 0usize, 0usize);
            for f in 0..frames {
                for &ei in &encs {
                    let n = 24000 / 50;
                    let p = pcm(&mut r, n, f * n);
                    let mut ctl = Guarded::new([24000i32, 24000, n as i32, 20000, 0, 2, 0, 0]);
                    let e = se(hs, ei, p.as_ptr(), n as i16, core::ptr::null_mut(), core::ptr::null_mut(), &mut ctl.v, 0);
                    if !ctl.ok() {
                        return Err("SilkEncodeFrame wrote past the 32-byte encoder control".into());
                    }
                    if e != 0 {
                        stats.0 += 1;
                        continue;
                    }
                    *eb = 2;
                    let mut ob = Guarded::new((en - 1) as i16);
                    let mut o;
                    let mut spins = 0;
                    loop {
                        o = seo(hs, ei, eb.add(1), &mut ob.v, &mut ctl.v);
                        if o != 1 || spins > 100 {
                            break;
                        }
                        spins += 1;
                        ob.v = (en - 1) as i16;
                    }
                    if !heap_ok(eb, en) || !ob.ok() || !ctl.ok() {
                        return Err(format!("SilkGetEncodeOutput overflow (frame {f}, bytes {})", ob.v));
                    }
                    if o != 0 || ob.v <= 0 {
                        stats.1 += 1;
                        continue;
                    }
                    for &di in &decs {
                        let mut dctl = Guarded::new([0x5dc0i32, 0, 0, 0, 0, 0]);
                        let d = sd(hs, di, eb.add(1), ob.v as u32, core::ptr::null_mut(), core::ptr::null_mut(), &mut dctl.v, 0, 1);
                        if !dctl.ok() {
                            return Err("SilkDecodeFrame wrote past the 24-byte decoder control (GameChat2 keeps a flag byte at +0x18)".into());
                        }
                        if d != 0 {
                            stats.2 += 1;
                            continue;
                        }
                        let mut os = Guarded::new(dn as u16 as i16);
                        let mut o2;
                        let mut spins = 0;
                        loop {
                            o2 = sdo(hs, di, db as *mut i16, &mut os.v, &mut dctl.v);
                            if o2 != 1 || spins > 100 {
                                break;
                            }
                            spins += 1;
                            os.v = dn as u16 as i16;
                        }
                        if !heap_ok(db, dn) || !os.ok() || !dctl.ok() {
                            return Err(format!("SilkGetDecodeOutput overflow (frame {f}, samples {})", os.v));
                        }
                        if o2 == 0 {
                            stats.3 += 1;
                        } else {
                            stats.2 += 1;
                        }
                    }
                }
                if f % 25 == 0 {
                    check(&format!("gc2 silk round {round} frame {f}"))?;
                }
            }
            heap_free(eb, en);
            heap_free(db, dn);
            ds(hs);
            check(&format!("gc2 silk round {round} destroy without release"))?;
            rep.push_str(&format!(
                "[silk round {round}: {} dec {} enc; enc fail {} encout fail {} dec fail {} dec ok {}] ",
                decs.len(),
                encs.len(),
                stats.0,
                stats.1,
                stats.2,
                stats.3
            ));
        }
    }
    Ok(rep)
}

pub fn run(dir: &Path, frames: usize) -> Result<String, String> {
    heapcheck::enable();
    let path = dir.join("AudioHALWrapper.dll");
    let lib = unsafe { LoadLibraryW(w(&path.to_string_lossy()).as_ptr()) };
    if lib.is_null() {
        return Err(format!("cannot load {}", path.display()));
    }
    let l = Lib(lib);
    check("load")?;
    let mut report = String::new();
    let mut r = Rng(0x1234_5678_9abc_def1);
    unsafe {
        let (ce, cd): (Create, Create) = (l.f("CreateOpusEncoderHardwareInterface"), l.f("CreateOpusDecoderHardwareInterface"));
        let (de, dd): (Destroy, Destroy) = (l.f("DestroyOpusEncoderHardwareInterface"), l.f("DestroyOpusDecoderHardwareInterface"));
        let acq_e: AcqEnc = l.f("AcquireEncoder");
        let acq_d: AcqDec = l.f("AcquireDecoder");
        let (rel_e, rel_d): (Rel, Rel) = (l.f("ReleaseEncoder"), l.f("ReleaseDecoder"));
        let enc: Submit = l.f("EncodeFrame");
        let dec: Submit = l.f("DecodePacket");
        let (eo, dout): (Output, Output) = (l.f("GetEncodeOutput"), l.f("GetDecodeOutput"));
        let (mut he, mut hd) = (core::ptr::null_mut(), core::ptr::null_mut());
        let hr = (ce(&mut he), cd(&mut hd));
        report.push_str(&format!("opus create enc={:#x} dec={:#x}; ", hr.0, hr.1));
        if hr.0 == 0 && hr.1 == 0 {
            for &(rate, ch, ms) in &[(48000u32, 1u32, 20usize), (48000, 2, 20), (24000, 1, 40), (16000, 1, 20), (48000, 1, 10), (48000, 2, 60)] {
                let (mut ei, mut di) = (0u32, 0u32);
                let a = acq_e(he, rate, ch, 24000, 0, &mut ei);
                let b = acq_d(hd, rate, ch, &mut di);
                if a != 0 || b != 0 {
                    report.push_str(&format!("[{rate}/{ch}/{ms}ms acquire {a:#x} {b:#x}] "));
                    continue;
                }
                let n = rate as usize * ms / 1000 * ch as usize;
                let (mut enc_ok, mut dec_ok) = (0, 0);
                for f in 0..frames {
                    let p = pcm(&mut r, n, f * n);
                    let bytes = (n * 2).min(u16::MAX as usize) as u16;
                    let e = enc(he, ei, p.as_ptr() as *const u8, bytes);
                    let mut out = vec![0u8; 0x2f0 + 64];
                    let canary = 0xa5u8;
                    for b in &mut out[0x2f0..] {
                        *b = canary;
                    }
                    let mut size: u16 = 0x2f0;
                    let mut ticks = 0u32;
                    let o = eo(he, ei, 1, &mut ticks, out.as_mut_ptr(), &mut size);
                    if out[0x2f0..].iter().any(|&b| b != canary) {
                        return Err(format!("GetEncodeOutput wrote past the {:#x}-byte buffer at {rate}/{ch}/{ms}ms frame {f}", 0x2f0));
                    }
                    if e != 0 || o != 0 {
                        continue;
                    }
                    enc_ok += 1;
                    let pkt = &out[..size as usize];
                    let d = dec(hd, di, if f % 7 == 3 { core::ptr::null() } else { pkt.as_ptr() }, if f % 7 == 3 { 0 } else { size });
                    let cap = 0x2d00usize * 2;
                    let mut pout = vec![0u8; cap + 64];
                    for b in &mut pout[cap..] {
                        *b = canary;
                    }
                    let mut psize: u16 = cap.min(u16::MAX as usize) as u16;
                    let o2 = dout(hd, di, 1, &mut ticks, pout.as_mut_ptr(), &mut psize);
                    if pout[cap..].iter().any(|&b| b != canary) {
                        return Err(format!("GetDecodeOutput wrote past the {cap:#x}-byte buffer at {rate}/{ch}/{ms}ms frame {f}"));
                    }
                    if d == 0 && o2 == 0 {
                        dec_ok += 1;
                    }
                    if f % 50 == 0 {
                        check(&format!("opus {rate}/{ch}/{ms}ms frame {f}"))?;
                    }
                }
                rel_e(he, ei);
                rel_d(hd, di);
                check(&format!("opus {rate}/{ch}/{ms}ms release"))?;
                report.push_str(&format!("[{rate}/{ch}/{ms}ms enc {enc_ok}/{frames} dec {dec_ok}] "));
            }
            de(he);
            dd(hd);
            check("opus destroy")?;
        }
        let cs: Create = l.f("CreateSilkHardwareInterface");
        let ds: Destroy = l.f("DestroySilkHardwareInterface");
        let (sae, sad): (SAcq, SAcq) = (l.f("SilkAcquireEncoder"), l.f("SilkAcquireDecoder"));
        let (sre, srd): (Rel, Rel) = (l.f("SilkReleaseEncoder"), l.f("SilkReleaseDecoder"));
        let se: SEnc = l.f("SilkEncodeFrame");
        let seo: SEncOut = l.f("SilkGetEncodeOutput");
        let sd: SDec = l.f("SilkDecodeFrame");
        let sdo: SDecOut = l.f("SilkGetDecodeOutput");
        let mut hs = core::ptr::null_mut();
        let h = cs(&mut hs);
        report.push_str(&format!("silk create {h:#x}; "));
        if h == 0 {
            for &(rate, ms, sync) in &[(16000i32, 20usize, 1u8), (24000, 20, 1), (48000, 20, 1), (16000, 40, 1), (16000, 100, 1), (16000, 20, 0)] {
                let (mut ei, mut di) = (0u32, 0u32);
                let (a, b) = (sae(hs, &mut ei), sad(hs, &mut di));
                if a != 0 || b != 0 {
                    report.push_str(&format!("[silk {rate}/{ms}ms acquire {a:#x} {b:#x}] "));
                    continue;
                }
                let n = rate as usize * ms / 1000;
                let (mut eok, mut dok) = (0, 0);
                for f in 0..frames {
                    let p = pcm(&mut r, n, f * n);
                    let mut ctl: [i32; 8] = [rate, 16000, (rate as usize * ms.min(100) / 1000) as i32, 20000, 0, 2, 0, 0];
                    let cap = 0x500usize;
                    let mut out = vec![0xa5u8; cap + 64];
                    let mut ob: i16 = cap as i16;
                    let mut e = se(hs, ei, p.as_ptr(), n as i16, out.as_mut_ptr(), &mut ob, &mut ctl, sync);
                    if sync == 0 {
                        for _ in 0..50 {
                            ob = cap as i16;
                            e = seo(hs, ei, out.as_mut_ptr(), &mut ob, &mut ctl);
                            if e != 1 {
                                break;
                            }
                        }
                    }
                    if out[cap..].iter().any(|&b| b != 0xa5) {
                        return Err(format!("SilkEncodeFrame wrote past the {cap:#x}-byte buffer at {rate}/{ms}ms frame {f}"));
                    }
                    if e != 0 || ob <= 0 {
                        continue;
                    }
                    eok += 1;
                    let mut dctl: [i32; 6] = [rate, 0, 0, 0, 0, 0];
                    let dcap = 0x2580usize / 2;
                    let mut pout = vec![0x5a5ai16; dcap + 32];
                    let mut os: i16 = dcap as i16;
                    let mut d = sd(hs, di, out.as_ptr(), ob as u32, pout.as_mut_ptr(), &mut os, &mut dctl, sync, 1);
                    if sync == 0 {
                        for _ in 0..50 {
                            os = dcap as i16;
                            d = sdo(hs, di, pout.as_mut_ptr(), &mut os, &mut dctl);
                            if d != 1 {
                                break;
                            }
                        }
                    }
                    if pout[dcap..].iter().any(|&v| v != 0x5a5a) {
                        return Err(format!("SilkDecodeFrame wrote past the {dcap}-sample buffer at {rate}/{ms}ms frame {f}"));
                    }
                    if d == 0 {
                        dok += 1;
                    }
                    if f % 50 == 0 {
                        check(&format!("silk {rate}/{ms}ms frame {f}"))?;
                    }
                }
                sre(hs, ei);
                srd(hs, di);
                check(&format!("silk {rate}/{ms}ms release"))?;
                report.push_str(&format!("[silk {rate}/{ms}ms sync={sync} enc {eok}/{frames} dec {dok}] "));
            }
            ds(hs);
            check("silk destroy")?;
        }
    }
    Ok(report)
}
