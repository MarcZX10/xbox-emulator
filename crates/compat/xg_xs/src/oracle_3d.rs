use core::ffi::c_void;
use crate::layout::{LAYOUT_BYTES, MIP_BASE, MIP_BYTES, PLANE_BASE};
use crate::oracle::{field, first_diff, oracle, vt, Create, Obj, Oracle, SERIAL};

extern "system" {
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

fn try_f(o: &Oracle, name: &str) -> Option<*mut c_void> {
    let n = format!("{name}\0");
    let f = unsafe { GetProcAddress(o.h as *mut c_void, n.as_ptr()) };
    if f.is_null() {
        None
    } else {
        Some(f)
    }
}

fn d3d12(dim: u32, w: u32, h: u32, depth: u16, mips: u16, fmt: u32, layout: u32, flags: u32) -> [u8; 0x38] {
    let mut d = [0u8; 0x38];
    d[0..4].copy_from_slice(&dim.to_le_bytes());
    d[0x10..0x18].copy_from_slice(&(w as u64).to_le_bytes());
    d[0x18..0x1c].copy_from_slice(&h.to_le_bytes());
    d[0x1c..0x1e].copy_from_slice(&depth.to_le_bytes());
    d[0x1e..0x20].copy_from_slice(&mips.to_le_bytes());
    d[0x20..0x24].copy_from_slice(&fmt.to_le_bytes());
    d[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
    d[0x2c..0x30].copy_from_slice(&layout.to_le_bytes());
    d[0x30..0x34].copy_from_slice(&flags.to_le_bytes());
    d
}

#[derive(Default)]
struct Tally {
    cases: usize,
    matched: usize,
    both_fail: usize,
    native_notimpl: usize,
    elements: usize,
    failures: Vec<String>,
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn points(n: u32, pad: u32, step: u32) -> Vec<u32> {
    let mut v: Vec<u32> = (0..n.min(3)).chain((0..n).step_by(step.max(1) as usize)).chain([n - 1, pad, pad + 3, n + pad]).collect();
    v.sort();
    v.dedup();
    v
}

unsafe fn compare_objs(o: &Obj, n: &Obj, label: &str, t: &mut Tally, sample: u32) -> bool {
    let (lo, ln) = (o.layout(), n.layout());
    if let Some((off, x, y)) = first_diff(&lo, &ln) {
        t.failures.push(format!("{label}: layout {} console {x:#x} native {y:#x}", field(off)));
        return false;
    }
    if o.size() != n.size() || o.align() != n.align() {
        t.failures.push(format!("{label}: size/align {:#x}/{:#x} native {:#x}/{:#x}", o.size(), o.align(), n.size(), n.align()));
        return false;
    }
    let mips = rd32(&lo, 0x10);
    for mip in 0..mips + 1 {
        if o.mip_offset(mip) != n.mip_offset(mip) {
            t.failures.push(format!("{label}: mip offset {mip} console {:#x} native {:#x}", o.mip_offset(mip), n.mip_offset(mip)));
            return false;
        }
        if mip == mips {
            break;
        }
        let r = PLANE_BASE + MIP_BASE + mip as usize * MIP_BYTES;
        let (pw, ph, pd) = (rd32(&lo, r + 0x24), rd32(&lo, r + 0x28), rd32(&lo, r + 0x2c));
        let (we, he, dp) = (rd32(&lo, r + 0x30), rd32(&lo, r + 0x34), rd32(&lo, r + 0x40));
        let zs = points(dp, pd, (dp / 3).max(1));
        for &z in &zs {
            for y in points(he, ph, (he / sample).max(1)) {
                for x in points(we, pw, (we / sample).max(1)) {
                    let (a, b) = (o.elem(mip, x as u64, y, z), n.elem(mip, x as u64, y, z));
                    t.elements += 1;
                    if a != b {
                        t.failures.push(format!("{label}: elem mip{mip} ({x},{y},{z}) console {a:#x} native {b:#x}"));
                        return false;
                    }
                }
            }
        }
    }
    let (a, b) = (o.elem(mips, 0, 0, 0), n.elem(mips, 0, 0, 0));
    if a != b {
        t.failures.push(format!("{label}: elem past last mip console {a:#x} native {b:#x}"));
        return false;
    }
    true
}

unsafe fn compare_create(o_create: Create, n_create: Create, d: &[u8], label: &str, t: &mut Tally, sample: u32) {
    t.cases += 1;
    let mut pn: *mut c_void = core::ptr::null_mut();
    let hr_n = n_create(d.as_ptr(), &mut pn);
    if hr_n == compat_common::E_NOTIMPL {
        t.native_notimpl += 1;
        return;
    }
    let mut po: *mut c_void = core::ptr::null_mut();
    let hr_o = o_create(d.as_ptr(), &mut po);
    if hr_o != hr_n || (hr_o < 0) != po.is_null() || (hr_n < 0) != pn.is_null() {
        t.failures.push(format!("{label}: hr console {:#010x} native {:#010x}", hr_o as u32, hr_n as u32));
        if !po.is_null() {
            drop(Obj(po));
        }
        if !pn.is_null() {
            drop(Obj(pn));
        }
        return;
    }
    if hr_o < 0 {
        t.both_fail += 1;
        return;
    }
    let (o, n) = (Obj(po), Obj(pn));
    if compare_objs(&o, &n, label, t, sample) {
        t.matched += 1;
    }
}

type Lay = unsafe extern "system" fn(*const u8, *mut u8) -> i32;

unsafe fn compare_layout(o_lay: Lay, n_lay: Lay, d: &[u8], label: &str, t: &mut Tally) {
    t.cases += 1;
    let mut a = vec![0u8; 0x2000];
    let mut b = vec![0u8; 0x2000];
    let hb = n_lay(d.as_ptr(), b.as_mut_ptr());
    if hb == compat_common::E_NOTIMPL {
        t.native_notimpl += 1;
        return;
    }
    let ha = o_lay(d.as_ptr(), a.as_mut_ptr());
    if ha != hb {
        t.failures.push(format!("{label}: hr console {:#x} native {:#x}", ha as u32, hb as u32));
        return;
    }
    if ha < 0 {
        t.both_fail += 1;
        return;
    }
    if let Some((off, x, y)) = first_diff(&a[..LAYOUT_BYTES], &b[..LAYOUT_BYTES]) {
        t.failures.push(format!("{label}: {} console {x:#x} native {y:#x}", field(off)));
        return;
    }
    t.matched += 1;
}

fn report(name: &str, t: &Tally) {
    eprintln!("{name}: {} cases, {} matched, {} both fail, {} native E_NOTIMPL, {} element offsets checked, {} mismatches", t.cases, t.matched, t.both_fail, t.native_notimpl, t.elements, t.failures.len());
    for f in t.failures.iter().take(std::env::var("XG3D_REPORT").ok().and_then(|v| v.parse().ok()).unwrap_or(60)) {
        eprintln!("  {f}");
    }
}

const FMTS: [u32; 9] = [61, 56, 28, 10, 2, 71, 77, 83, 24];
const SIZES_3D: [(u32, u32, u32); 22] = [
    (1, 1, 1),
    (3, 3, 3),
    (64, 32, 16),
    (100, 60, 7),
    (256, 256, 256),
    (1, 1, 65),
    (33, 1, 1),
    (1, 33, 1),
    (17, 9, 5),
    (300, 7, 33),
    (1000, 33, 3),
    (16, 16, 1),
    (9, 9, 9),
    (513, 2, 2),
    (2, 513, 2),
    (2, 2, 513),
    (128, 128, 64),
    (1024, 1024, 8),
    (4096, 16, 16),
    (1, 1, 2048),
    (77, 1023, 31),
    (16384, 4, 2),
];

fn layouts() -> Vec<u32> {
    let mut v = vec![0u32, 1, 2, 3];
    for m in 0..32 {
        v.push(0x100 + m);
    }
    v
}

unsafe fn native_create() -> Create {
    crate::XGCreateTextureComputer
}

fn sweep_3d(o: &Oracle, name: &str, sizes: &[(u32, u32, u32)], fmts: &[u32], mips_list: &[u16], sample: u32) -> Tally {
    let create: Create = unsafe { core::mem::transmute(o.f("XGCreateTextureComputer")) };
    let mut t = Tally::default();
    for lay in layouts() {
        for &fmt in fmts {
            for &(w, h, d) in sizes {
                for &mips in mips_list {
                    let desc = d3d12(4, w, h, d as u16, mips, fmt, lay, 0);
                    unsafe { compare_create(create, native_create(), &desc, &format!("{name} 3d lay={lay:#x} fmt={fmt} {w}x{h}x{d} mips={mips}"), &mut t, sample) };
                }
            }
        }
    }
    t
}

fn sweep_1d(o: &Oracle, name: &str, sample: u32) -> Tally {
    let create: Create = unsafe { core::mem::transmute(o.f("XGCreateTextureComputer")) };
    let mut t = Tally::default();
    for lay in layouts() {
        for fmt in FMTS {
            for w in [1u32, 3, 64, 65, 300, 1000, 4096, 16384] {
                for arr in [1u16, 5, 64] {
                    for mips in [0u16, 1, 3] {
                        for h in [1u32, 7] {
                            let desc = d3d12(2, w, h, arr, mips, fmt, lay, 0);
                            unsafe { compare_create(create, native_create(), &desc, &format!("{name} 1d lay={lay:#x} fmt={fmt} w={w} h={h} arr={arr} mips={mips}"), &mut t, sample) };
                        }
                    }
                }
            }
        }
    }
    t
}

#[test]
fn oracle_3d_d3d12_layouts() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let t = sweep_3d(&o, "", &SIZES_3D, &FMTS, &[0, 1, 2, 5], 6);
    report("3d d3d12 layouts", &t);
    assert!(t.failures.is_empty());
}

#[test]
fn oracle_1d_d3d12_layouts() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let t = sweep_1d(&o, "", 8);
    report("1d d3d12 layouts", &t);
    assert!(t.failures.is_empty());
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u32) -> u32 {
        (self.next() % n as u64) as u32
    }
    fn dim(&mut self, max: u32) -> u32 {
        match self.below(4) {
            0 => 1 + self.below(40),
            1 => 1 << self.below(max.trailing_zeros() + 1),
            2 => 1 + self.below(700),
            _ => 1 + self.below(max),
        }
    }
}

#[test]
fn oracle_3d_random() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let create: Create = unsafe { core::mem::transmute(o.f("XGCreateTextureComputer")) };
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let mut t = Tally::default();
    let fmts: Vec<u32> = crate::formats::FORMATS.iter().map(|f| f.dxgi).filter(|&f| f < 194 || !crate::layout::legacy_19041()).collect();
    for _ in 0..12000 {
        let w = r.dim(4096);
        let h = r.dim(4096);
        let d = if r.below(3) == 0 { r.dim(512) } else { 1 + r.below(70) };
        let lay = [0x100u32, 0x105, 0x109, 0x111, 0x115, 0x118, 0x119, 0x11a, 0x11b, 0x11c, 0x11f, 0, 1, 2][r.below(14) as usize];
        let fmt = fmts[r.below(fmts.len() as u32) as usize];
        let chain = crate::layout3d::full_chain_3d(w, h, d);
        let mips = if r.below(3) == 0 { 0 } else { 1 + r.below(chain) } as u16;
        let desc = d3d12(4, w, h, d as u16, mips, fmt, lay, 0);
        unsafe { compare_create(create, native_create(), &desc, &format!("random 3d lay={lay:#x} fmt={fmt} {w}x{h}x{d} mips={mips}"), &mut t, 3) };
    }
    report("3d random", &t);
    assert!(t.failures.is_empty());
}

#[test]
fn oracle_1d_3d_flags_and_auto() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let create: Create = unsafe { core::mem::transmute(o.f("XGCreateTextureComputer")) };
    let mut t = Tally::default();
    for dim in [2u32, 4, 5, 6] {
        for lay in [0u32, 1, 2, 0x105, 0x10a, 0x111, 0x112, 0x118, 0x119, 0x11b, 0x11f] {
            for flags in [0u32, 1, 2, 4, 5, 8, 0x20, 0x40_0000, 0x80_0000, 0x10_0001] {
                for fmt in [28u32, 2, 61, 71, 45] {
                    for (w, h, d, mips) in [(256u32, 256u32, 4u16, 0u16), (64, 64, 64, 1), (100, 60, 3, 0), (2048, 4, 2, 0)] {
                        let desc = d3d12(dim, w, h, d, mips, fmt, lay, flags);
                        unsafe { compare_create(create, native_create(), &desc, &format!("flags dim={dim} lay={lay:#x} flags={flags:#x} fmt={fmt} {w}x{h}x{d} mips={mips}"), &mut t, 8) };
                    }
                }
            }
        }
    }
    report("1d/3d flags and auto", &t);
    assert!(t.failures.is_empty());
}

#[test]
fn oracle_1d_3d_all_formats() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let create: Create = unsafe { core::mem::transmute(o.f("XGCreateTextureComputer")) };
    let mut t = Tally::default();
    for fmt in 0..194u32 {
        for lay in [0x100u32, 0x105, 0x109, 0x111, 0x118, 0x11a, 0x11b, 0x11c] {
            for (dim, w, h, d) in [(4u32, 130u32, 70u32, 9u16), (2, 130, 1, 3)] {
                let desc = d3d12(dim, w, h, d, 0, fmt, lay, 0);
                unsafe { compare_create(create, native_create(), &desc, &format!("fmt={fmt} dim={dim} lay={lay:#x}"), &mut t, 8) };
            }
        }
    }
    report("1d/3d all formats", &t);
    assert!(t.failures.is_empty());
}

fn desc3(w: u32, h: u32, d: u32, mips: u32, fmt: u32, usage: u32, bind: u32, cpu: u32, misc: u32, mode: u32, pitch: u32) -> [u8; 0x2c] {
    let mut b = [0u8; 0x2c];
    for (i, v) in [w, h, d, mips, fmt, usage, bind, cpu, misc, mode, pitch].iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    b
}

fn desc1(w: u32, mips: u32, arr: u32, fmt: u32, usage: u32, bind: u32, cpu: u32, misc: u32, mode: u32, pitch: u32) -> [u8; 0x28] {
    let mut b = [0u8; 0x28];
    for (i, v) in [w, mips, arr, fmt, usage, bind, cpu, misc, mode, pitch].iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    b
}

#[test]
fn oracle_1d_3d_desc_path() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let lay3: Lay = unsafe { core::mem::transmute(o.f("XGComputeTexture3DLayout")) };
    let cre3: Create = unsafe { core::mem::transmute(o.f("XGCreateTexture3DComputer")) };
    let lay1: Lay = unsafe { core::mem::transmute(o.f("XGComputeTexture1DLayout")) };
    let cre1: Create = unsafe { core::mem::transmute(o.f("XGCreateTexture1DComputer")) };
    let mut t = Tally::default();
    let mut c = Tally::default();
    let usages = [0u32, 1, 2, 3, 4];
    let binds = [0u32, 8, 0x20, 0x40, 0x80, 0xa8, 0x100, 0x400, 0x600, 0x1000];
    let miscs = [0u32, 1, 2, 4, 8, 0x10, 0x20, 0x8000, 0x8_0000, 0x10_0000, 0x40_0000, 0x400_0000, 0x800_0000, 0x1000_0000];
    for mode in [0u32, 1, 5, 9, 10, 17, 21, 24, 25, 26, 27, 28, 31, 32, 33, 37, 59, 64, 0x8000_0005] {
        for (i, &usage) in usages.iter().enumerate() {
            for (j, &bind) in binds.iter().enumerate() {
                for (k, &misc) in miscs.iter().enumerate() {
                    if (i + j + k) % 3 != 0 && !(usage == 0 && bind == 8) && misc != 0 {
                        continue;
                    }
                    for (w, h, d, mips, fmt, cpu, pitch) in [(100u32, 60u32, 7u32, 0u32, 28u32, 0u32, 0u32), (64, 64, 64, 1, 71, 0x1_0000, 64), (33, 17, 5, 3, 61, 0, 32), (16, 16, 16, 16, 28, 0, 0), (0, 4, 4, 1, 28, 0, 0), (4, 4, 0, 1, 28, 0, 0), (40, 30, 20, 2, 6, 0, 0), (40, 30, 20, 2, 0, 0, 0), (513, 9, 2, 0, 10, 3, 600)] {
                        let label = format!("xg3d mode={mode} usage={usage} bind={bind:#x} misc={misc:#x} {w}x{h}x{d} mips={mips} fmt={fmt} cpu={cpu:#x} pitch={pitch}");
                        let b = desc3(w, h, d, mips, fmt, usage, bind, cpu, misc, mode, pitch);
                        unsafe {
                            compare_layout(lay3, crate::XGComputeTexture3DLayout, &b, &label, &mut t);
                            compare_create(cre3, crate::XGCreateTexture3DComputer, &b, &label, &mut c, 4);
                        }
                    }
                    for (w, mips, arr, fmt, cpu, pitch) in [(300u32, 0u32, 4u32, 28u32, 0u32, 0u32), (64, 1, 1, 71, 0, 0), (33, 3, 2, 61, 0x2_0000, 33), (1000, 16, 1, 28, 0, 0), (0, 1, 1, 28, 0, 0), (4, 1, 0, 28, 0, 0), (40, 2, 2, 7, 0, 0), (40, 2, 2, 0, 0, 0), (513, 0, 3, 10, 1, 1000), (513, 0, 3, 10, 0, 512)] {
                        let label = format!("xg1d mode={mode} usage={usage} bind={bind:#x} misc={misc:#x} w={w} mips={mips} arr={arr} fmt={fmt} cpu={cpu:#x} pitch={pitch}");
                        let b = desc1(w, mips, arr, fmt, usage, bind, cpu, misc, mode, pitch);
                        unsafe {
                            compare_layout(lay1, crate::XGComputeTexture1DLayout, &b, &label, &mut t);
                            compare_create(cre1, crate::XGCreateTexture1DComputer, &b, &label, &mut c, 4);
                        }
                    }
                }
            }
        }
    }
    report("1d/3d desc layouts", &t);
    report("1d/3d desc computers", &c);
    assert!(t.failures.is_empty() && c.failures.is_empty());
}

#[test]
fn oracle_1d_3d_hardware_version_4() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let hwf = match try_f(&o, "XGSetHardwareVersion") {
        Some(f) => f,
        None => {
            eprintln!("hardware version 4: oracle has no XGSetHardwareVersion");
            return;
        }
    };
    type Hw = unsafe extern "system" fn(u32) -> i32;
    let hw: Hw = unsafe { core::mem::transmute(hwf) };
    unsafe {
        hw(4);
        crate::XGSetHardwareVersion(4);
    }
    let t3 = sweep_3d(&o, "hw4", &SIZES_3D, &[61, 56, 28, 10, 2, 71], &[0, 1, 3], 6);
    let t1 = sweep_1d(&o, "hw4", 8);
    unsafe {
        hw(0);
        crate::XGSetHardwareVersion(0);
    }
    report("hw4 3d", &t3);
    report("hw4 1d", &t1);
    assert!(t3.failures.is_empty() && t1.failures.is_empty());
}

type Copy = unsafe extern "system" fn(*mut c_void, *mut u8, u32, u32, *const u8, u32, u32) -> i32;

#[test]
fn oracle_1d_3d_copy() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let create: Create = unsafe { core::mem::transmute(o.f("XGCreateTextureComputer")) };
    let mut bad = Vec::new();
    let mut subs = 0;
    for (dim, lay, fmt, w, h, d) in [(4u32, 0x105u32, 28u32, 70u32, 45u32, 9u16), (4, 0x109, 71, 64, 64, 20), (4, 0x100, 61, 33, 17, 3), (4, 0x11b, 10, 100, 40, 5), (4, 0x11a, 56, 40, 40, 40), (4, 0x11a, 10, 40, 40, 4), (4, 0x115, 2, 30, 20, 10), (2, 0x100, 28, 300, 1, 3), (2, 0x118, 61, 500, 1, 2), (2, 0x11f, 2, 77, 1, 4)] {
        let desc = d3d12(dim, w, h, d, 0, fmt, lay, 0);
        let mut po: *mut c_void = core::ptr::null_mut();
        let mut pn: *mut c_void = core::ptr::null_mut();
        unsafe {
            let (a, b) = (create(desc.as_ptr(), &mut po), crate::XGCreateTextureComputer(desc.as_ptr(), &mut pn));
            if a != b {
                bad.push(format!("dim={dim} lay={lay:#x} fmt={fmt}: create hr {:#x}/{:#x}", a as u32, b as u32));
            }
            if a < 0 || b < 0 {
                if !po.is_null() {
                    drop(Obj(po));
                }
                if !pn.is_null() {
                    drop(Obj(pn));
                }
                continue;
            }
        }
        let (oo, nn) = (Obj(po), Obj(pn));
        let total = unsafe { oo.size() } as usize;
        let lo = unsafe { oo.layout() };
        let mips = rd32(&lo, 0x10);
        let bpe = rd32(&lo, PLANE_BASE + 0x28) as usize;
        let nsub = mips * d as u32;
        let mut ra = vec![0u8; total];
        let mut rb = vec![0u8; total];
        for sub in 0..nsub + 1 {
            let r = PLANE_BASE + MIP_BASE + (sub % mips) as usize * MIP_BYTES;
            let we = rd32(&lo, r + 0x30) as usize;
            let he = rd32(&lo, r + 0x34) as usize;
            let dp = if dim == 4 { rd32(&lo, r + 0x40) as usize } else { 1 };
            let pitch = we * bpe + 12;
            let slice = pitch * he + 20;
            let src: Vec<u8> = (0..slice * dp).map(|i| (i as u32).wrapping_mul(2654435761).wrapping_add(sub) as u8 | 1).collect();
            let mut back_a = vec![0u8; src.len()];
            let mut back_b = vec![0u8; src.len()];
            unsafe {
                let fa: Copy = core::mem::transmute(vt(oo.0, 8));
                let fb: Copy = core::mem::transmute(vt(nn.0, 8));
                let ha = fa(oo.0, ra.as_mut_ptr(), 0, sub, src.as_ptr(), pitch as u32, slice as u32);
                let hb = fb(nn.0, rb.as_mut_ptr(), 0, sub, src.as_ptr(), pitch as u32, slice as u32);
                subs += 1;
                if ha != hb {
                    bad.push(format!("dim={dim} lay={lay:#x} fmt={fmt} sub={sub}: copy into hr {:#x}/{:#x}", ha as u32, hb as u32));
                }
                let ga: Copy = core::mem::transmute(vt(oo.0, 9));
                let gb: Copy = core::mem::transmute(vt(nn.0, 9));
                let ha = ga(oo.0, back_a.as_mut_ptr(), 0, sub, ra.as_ptr(), pitch as u32, slice as u32);
                let hb = gb(nn.0, back_b.as_mut_ptr(), 0, sub, ra.as_ptr(), pitch as u32, slice as u32);
                if ha != hb || back_a != back_b {
                    bad.push(format!("dim={dim} lay={lay:#x} fmt={fmt} sub={sub}: copy from hr {:#x}/{:#x} same={}", ha as u32, hb as u32, back_a == back_b));
                }
            }
        }
        if ra != rb {
            let i = ra.iter().zip(&rb).position(|(a, b)| a != b).unwrap();
            bad.push(format!("dim={dim} lay={lay:#x} fmt={fmt}: resource bytes differ first at {i:#x}"));
        }
    }
    eprintln!("1d/3d copy: {subs} subresources, {} mismatches", bad.len());
    for b in &bad {
        eprintln!("  {b}");
    }
    assert!(bad.is_empty());
}

fn srd(ty: u32, hw: u32, sw: u32, w: u32, h: u32, depth: u32, mips: u32) -> [u32; 8] {
    let mut d = [0u32; 8];
    d[1] = hw << 20 | ((w - 1) & 3) << 30;
    d[2] = ((w - 1) >> 2) & 0x3fff | (h - 1) << 14;
    d[3] = sw << 20 | ty << 28 | (mips - 1) << 16;
    d[4] = depth - 1;
    d[5] = (mips - 1) << 4;
    d
}

#[test]
fn oracle_1d_3d_from_descriptor() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let create: Create = unsafe { core::mem::transmute(o.f("XGCreateTextureComputerFromDescriptor")) };
    let native: Create = unsafe { core::mem::transmute(crate::XGCreateTextureComputerFromDescriptor as unsafe extern "system" fn(*const u32, *mut *mut c_void) -> i32) };
    let mut t = Tally::default();
    let hws = [0x1u32, 0x7, 0xe, 0x14, 0x38, 0x41, 0x48, 0x4b, 0x80, 0x88, 0x91, 0xa9, 0xad, 0x109];
    for ty in [8u32, 10, 12] {
        for &hw in &hws {
            for sw in 0..32u32 {
                for (w, h, depth, mips) in [(1u32, 1u32, 1u32, 1u32), (100, 60, 7, 3), (64, 64, 64, 7), (33, 1, 5, 1), (300, 20, 40, 2), (1000, 3, 2, 10), (16, 16, 300, 9)] {
                    let d = srd(ty, hw, sw, w, h, depth, mips);
                    unsafe { compare_create(create, native, bytes(&d), &format!("srd ty={ty} hw={hw:#x} sw={sw} {w}x{h}x{depth} mips={mips}"), &mut t, 4) };
                }
            }
        }
    }
    let mut r = Rng(0x0123_4567_89ab_cdef);
    for _ in 0..20000 {
        let ty = [8u32, 10, 12][r.below(3) as usize];
        let hw = r.below(0x120);
        let sw = r.below(32);
        let w = r.dim(4096);
        let h = r.dim(4096);
        let depth = if r.below(2) == 0 { 1 + r.below(8) } else { r.dim(1024) };
        let chain = if ty == 10 { crate::layout3d::full_chain_3d(w, h, depth) } else { crate::layout::full_chain(w, 1) };
        let mips = 1 + r.below(chain + 1);
        let mut d = srd(ty, hw, sw, w, h, depth, mips);
        if r.below(4) == 0 {
            d[0] = r.next() as u32;
            d[1] |= r.next() as u32 & 0xff;
            d[4] |= r.next() as u32 & 0xffff_0000;
            d[6] = r.next() as u32;
        }
        unsafe { compare_create(create, native, bytes(&d), &format!("srd random {d:08x?}"), &mut t, 3) };
    }
    report("1d/3d from descriptor", &t);
    assert!(t.failures.is_empty());
}

fn bytes(d: &[u32; 8]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(d.as_ptr() as *const u8, 32) }
}
