use core::ffi::c_void;
use std::path::{Path, PathBuf};

const CONSOLE_DLL: &str = r"D:\xbox-pc-port\games\Fortnite\extracted\EmbeddedXvd\Windows\System32\XAudio2_9.dll";

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn built_dll() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    for dir in exe.ancestors() {
        for p in [dir.join("XAudio2_9.dll"), dir.join("debug").join("XAudio2_9.dll")] {
            if p.is_file() {
                return p;
            }
        }
    }
    panic!("built XAudio2_9.dll not found above {}", exe.display());
}

fn exports(path: &Path) -> Vec<(String, u32)> {
    let b = std::fs::read(path).unwrap();
    let u16_ = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]) as usize;
    let u32_ = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize;
    let pe = u32_(0x3c);
    let nsec = u16_(pe + 6);
    let opt = pe + 24;
    let opt_size = u16_(pe + 20);
    let dd = if u16_(opt) == 0x20b { opt + 112 } else { opt + 96 };
    let sec = opt + opt_size;
    let rva = |r: usize| -> usize {
        for i in 0..nsec {
            let s = sec + i * 40;
            let va = u32_(s + 12);
            let sz = u32_(s + 16).max(u32_(s + 8));
            if r >= va && r < va + sz {
                return r - va + u32_(s + 20);
            }
        }
        panic!("rva {r:#x} outside sections");
    };
    let ed = rva(u32_(dd));
    let base = u32_(ed + 16);
    let nnames = u32_(ed + 24);
    let names = rva(u32_(ed + 32));
    let ords = rva(u32_(ed + 36));
    let mut out = Vec::new();
    for i in 0..nnames {
        let n = rva(u32_(names + i * 4));
        let end = b[n..].iter().position(|c| *c == 0).unwrap();
        out.push((String::from_utf8_lossy(&b[n..n + end]).into_owned(), (u16_(ords + i * 2) + base) as u32));
    }
    out.sort_by_key(|e| e.1);
    out
}

#[test]
fn built_dll_exports_console_names_at_console_ordinals() {
    let console = exports(Path::new(CONSOLE_DLL));
    let ours = exports(&built_dll());
    assert_eq!(ours, console);
    let table: Vec<(String, u32)> = XAudio2_9::exports::CONSOLE_ORDINALS.iter().map(|(n, o)| (n.to_string(), *o)).collect();
    assert_eq!(ours, table);
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct V3 {
    x: f32,
    y: f32,
    z: f32,
}

#[repr(C, packed)]
struct Listener {
    front: V3,
    top: V3,
    pos: V3,
    vel: V3,
    cone: *const c_void,
}

#[repr(C, packed)]
struct Emitter {
    cone: *const c_void,
    front: V3,
    top: V3,
    pos: V3,
    vel: V3,
    inner_radius: f32,
    inner_radius_angle: f32,
    channel_count: u32,
    channel_radius: f32,
    azimuths: *const f32,
    volume_curve: *const c_void,
    lfe_curve: *const c_void,
    lpf_direct_curve: *const c_void,
    lpf_reverb_curve: *const c_void,
    reverb_curve: *const c_void,
    curve_distance_scaler: f32,
    doppler_scaler: f32,
}

#[repr(C, packed)]
struct Dsp {
    matrix: *mut f32,
    delay: *mut f32,
    src: u32,
    dst: u32,
    lpf_direct: f32,
    lpf_reverb: f32,
    reverb: f32,
    doppler: f32,
    angle: f32,
    distance: f32,
    emitter_vel: f32,
    listener_vel: f32,
}

type InitFn = unsafe extern "system" fn(u32, f32, *mut u8) -> i32;
type CalcFn = unsafe extern "system" fn(*const u8, *const Listener, *const Emitter, u32, *mut Dsp);

fn console_fns() -> (InitFn, CalcFn) {
    unsafe {
        let m = LoadLibraryW(wide(CONSOLE_DLL).as_ptr());
        assert!(!m.is_null(), "console XAudio2_9.dll did not load");
        let i = GetProcAddress(m, b"X3DAudioInitialize\0".as_ptr());
        let c = GetProcAddress(m, b"X3DAudioCalculate\0".as_ptr());
        assert!(!i.is_null() && !c.is_null());
        (core::mem::transmute(i), core::mem::transmute(c))
    }
}

struct Rng(u64);
impl Rng {
    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        lo + (hi - lo) * ((self.0 >> 40) as f32 / (1u64 << 24) as f32)
    }
    fn v(&mut self, r: f32) -> V3 {
        V3 { x: self.f(-r, r), y: self.f(-r, r), z: self.f(-r, r) }
    }
}

fn unit(v: V3) -> V3 {
    let l = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt().max(1e-6);
    V3 { x: v.x / l, y: v.y / l, z: v.z / l }
}

fn ortho(f: V3, t: V3) -> V3 {
    let d = f.x * t.x + f.y * t.y + f.z * t.z;
    unit(V3 { x: t.x - d * f.x, y: t.y - d * f.y, z: t.z - d * f.z })
}

#[test]
fn x3daudio_matches_console_dll() {
    let (cinit, ccalc) = console_fns();
    let mut nonzero = 0usize;
    for (mask, flags) in [(0x3u32, 0x7Du32), (0x3F, 0x7D), (0x3F, 0x1007D), (0x3F, 0x2007D), (0x63F, 0x7D), (0x4, 0x5D), (0x3F, 0x3B)] {
        let mut ih = [0u8; 20];
        let mut ch = [0u8; 20];
        assert_eq!(unsafe { XAudio2_9::X3DAudioInitialize(mask, 343.5, ih.as_mut_ptr()) }, unsafe { cinit(mask, 343.5, ch.as_mut_ptr()) });
        assert_eq!(ih, ch, "X3DAUDIO_HANDLE for mask {mask:#x}");
        let dst = mask.count_ones();
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ mask as u64 ^ (flags as u64) << 20);
        for _ in 0..200 {
            let src = if flags & 2 != 0 { 1 } else { 1 + (rng.f(0.0, 3.99) as u32) };
            let az: Vec<f32> = (0..src).map(|_| rng.f(0.0, 6.28)).collect();
            let lf = unit(rng.v(1.0));
            let ef = unit(rng.v(1.0));
            let l = Listener { front: lf, top: ortho(lf, rng.v(1.0)), pos: rng.v(20.0), vel: rng.v(5.0), cone: core::ptr::null() };
            let e = Emitter {
                cone: core::ptr::null(),
                front: ef,
                top: ortho(ef, rng.v(1.0)),
                pos: rng.v(20.0),
                vel: rng.v(5.0),
                inner_radius: rng.f(0.0, 2.0),
                inner_radius_angle: rng.f(0.0, 0.78),
                channel_count: src,
                channel_radius: rng.f(0.0, 1.0),
                azimuths: az.as_ptr(),
                volume_curve: core::ptr::null(),
                lfe_curve: core::ptr::null(),
                lpf_direct_curve: core::ptr::null(),
                lpf_reverb_curve: core::ptr::null(),
                reverb_curve: core::ptr::null(),
                curve_distance_scaler: rng.f(0.5, 4.0),
                doppler_scaler: rng.f(0.0, 2.0),
            };
            let run = |calc: &dyn Fn(*const u8, *const Listener, *const Emitter, u32, *mut Dsp), h: &[u8; 20]| {
                let mut m = vec![0f32; (src * dst) as usize];
                let mut d = vec![0f32; dst as usize];
                let mut s = Dsp { matrix: m.as_mut_ptr(), delay: d.as_mut_ptr(), src, dst, lpf_direct: 0.0, lpf_reverb: 0.0, reverb: 0.0, doppler: 0.0, angle: 0.0, distance: 0.0, emitter_vel: 0.0, listener_vel: 0.0 };
                calc(h.as_ptr(), &l, &e, flags, &mut s);
                let mut v = m.clone();
                if flags & 2 != 0 {
                    v.extend_from_slice(&d);
                }
                v.extend_from_slice(&[s.lpf_direct, s.lpf_reverb, s.reverb, s.doppler, s.angle, s.distance, s.emitter_vel, s.listener_vel]);
                v
            };
            let ours = run(&|h, l, e, f, d| unsafe { XAudio2_9::X3DAudioCalculate(h, l as *const c_void, e as *const c_void, f, d as *mut c_void) }, &ih);
            let theirs = run(&|h, l, e, f, d| unsafe { ccalc(h, l, e, f, d) }, &ch);
            nonzero += theirs.iter().filter(|v| **v != 0.0).count();
            for (i, (a, b)) in ours.iter().zip(theirs.iter()).enumerate() {
                assert!((a - b).abs() <= 1e-5 * b.abs().max(1.0), "mask {mask:#x} flags {flags:#x} field {i}: ours {a} console {b}");
            }
        }
    }
    assert!(nonzero > 10_000, "console X3DAudioCalculate produced only {nonzero} nonzero outputs");
}
