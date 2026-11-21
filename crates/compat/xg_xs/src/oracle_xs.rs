use crate::oracle::{oracle, SERIAL};
use crate::tile::TileShape;

pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub(crate) fn u32(&mut self) -> u32 {
        (self.next() >> 16) as u32
    }
    pub(crate) fn below(&mut self, n: u32) -> u32 {
        (self.next() % n as u64) as u32
    }
}

#[test]
fn oracle_tile_shape() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    type F = unsafe extern "system" fn(u32, u32, u32, u32, *mut TileShape, *mut TileShape) -> i32;
    let f: F = unsafe { core::mem::transmute(o.f("XGComputeTileShape")) };
    let junk = TileShape { width: 0xaaaa_aaaa, height: 0xbbbb_bbbb, depth: 0xcccc_cccc };
    let mut n = 0;
    let mut bad = Vec::new();
    for dim in 0..7u32 {
        for fmt in 0..=192u32 {
            for bpe in [0u32, 1, 2, 3, 4, 8, 16, 32] {
                for samples in [0u32, 1, 2, 3, 4, 8, 16] {
                    for ptrs in 0..4 {
                        n += 1;
                        let (mut a1, mut a2, mut b1, mut b2) = (junk, junk, junk, junk);
                        let p = |t: &mut TileShape, on: bool| if on { t as *mut TileShape } else { core::ptr::null_mut() };
                        let ha = unsafe { f(dim, fmt, bpe, samples, p(&mut a1, ptrs & 1 != 0), p(&mut a2, ptrs & 2 != 0)) };
                        let hb = unsafe { crate::XGComputeTileShape(dim, fmt, bpe, samples, p(&mut b1, ptrs & 1 != 0), p(&mut b2, ptrs & 2 != 0)) };
                        if ha != hb || a1 != b1 || a2 != b2 {
                            bad.push(format!("dim={dim} fmt={fmt} bpe={bpe} s={samples} ptrs={ptrs}: console {:#x} {a1:?} {a2:?} native {:#x} {b1:?} {b2:?}", ha as u32, hb as u32));
                        }
                    }
                }
            }
        }
    }
    eprintln!("tile shape: {n} cases, {} mismatches", bad.len());
    for b in bad.iter().take(30) {
        eprintln!("  {b}");
    }
    assert!(bad.is_empty());
}

fn random_descriptor(r: &mut Rng) -> [u32; 8] {
    let mut d = [0u32; 8];
    for v in d.iter_mut() {
        *v = r.u32();
    }
    match r.below(6) {
        0 => {}
        1 => {
            d[1] &= !0xff;
            d[3] = (d[3] & 0x0fff_ffff) | (8 + r.below(8)) << 28;
            let sel = [0u32, 1, 4, 5, 6, 7];
            d[3] = (d[3] & !0xfff) | sel[r.below(6) as usize] | sel[r.below(6) as usize] << 3 | sel[r.below(6) as usize] << 6 | sel[r.below(6) as usize] << 9;
            let base = r.below(16);
            let last = base + r.below(16 - base);
            d[3] = (d[3] & !0xf_f000) | base << 12 | last << 16;
            if r.below(2) == 0 {
                d[7] &= 0x00ff_ffff;
            }
            if r.below(2) == 0 {
                d[4] = (d[4] & 0xffff) | (r.below((d[4] & 0xffff) + 1)) << 16;
            }
            if r.below(2) == 0 {
                d[4] = (d[4] & 0xffff_0000) | r.below(0x2100);
                d[4] = (d[4] & 0xffff) | (r.below((d[4] & 0xffff) + 1)) << 16;
            }
            if r.below(2) == 0 {
                d[2] = (d[2] & 0xc000_0000) | r.below(0x1100) | r.below(0x4100) << 14;
            }
        }
        2 => {
            d[3] &= 0x3fff_ffff;
            let sel = [0u32, 1, 4, 5, 6, 7, 2, 3];
            d[3] = (d[3] & !0xfff) | {
                let k = if r.below(4) == 0 { 8 } else { 6 };
                sel[r.below(k) as usize]
            } | sel[r.below(6) as usize] << 3 | sel[r.below(6) as usize] << 6 | sel[r.below(6) as usize] << 9;
            d[3] = (d[3] & !0x7_f000) | r.below(0x50) << 12;
            if r.below(2) == 0 {
                d[1] = (d[1] & 0xffff_0000) | r.below(0x120);
            }
        }
        3 => {
            d[0] = 0;
            d[1] = 0;
            d[2] = 0;
            d[3] = 0;
        }
        4 => {
            d[3] = (d[3] & 0x0fff_ffff) | r.below(8) << 28;
        }
        _ => {
            d[1] = (d[1] & !0x1ff0_00ff) | r.below(0x200) << 20;
            d[3] = (d[3] & 0x0fff_0000) | (8 + r.below(8)) << 28;
            d[7] = 0;
            d[4] = 0;
            d[2] = 0;
        }
    }
    d
}

#[test]
fn oracle_validate_descriptor() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    type F = unsafe extern "system" fn(u32, *const u32, u32) -> i32;
    let f: F = unsafe { core::mem::transmute(o.f("XGValidateDescriptor")) };
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let mut n = 0;
    let mut ok = 0;
    let mut bad = Vec::new();
    for i in 0..200_000u32 {
        let d = random_descriptor(&mut r);
        let kind = match i % 16 {
            0 => r.below(12),
            1..=7 => 4,
            _ => 8,
        };
        let flags = if i % 97 == 0 { 1 + r.below(4) } else { 0 };
        let ptr = if i % 211 == 0 { core::ptr::null() } else { d.as_ptr() };
        n += 1;
        let ha = unsafe { f(kind, ptr, flags) };
        let hb = unsafe { crate::XGValidateDescriptor(kind, ptr, flags) };
        if ha == 0 {
            ok += 1;
        }
        if ha != hb {
            bad.push(format!("kind={kind} flags={flags} d={d:08x?}: console {:#x} native {:#x}", ha as u32, hb as u32));
        }
    }
    eprintln!("validate descriptor: {n} cases ({ok} valid), {} mismatches", bad.len());
    for b in bad.iter().take(30) {
        eprintln!("  {b}");
    }
    assert!(bad.is_empty());
}

pub(crate) fn srd(ty: u32, hw: u32, sw: u32, w: u32, h: u32, depth: u32, mips: u32, last_level: u32) -> [u32; 8] {
    let mut d = [0u32; 8];
    d[1] = hw << 20 | ((w - 1) & 3) << 30;
    d[2] = ((w - 1) >> 2) & 0x3fff | (h - 1) << 14;
    let ll = if ty >= 14 { last_level } else { mips - 1 };
    d[3] = sw << 20 | ty << 28 | ll << 16;
    d[4] = depth - 1;
    d[5] = (mips - 1) << 4;
    d
}

#[derive(Default)]
pub(crate) struct SrdTally {
    pub(crate) cases: usize,
    pub(crate) both_ok: usize,
    pub(crate) both_fail: usize,
    pub(crate) native_notimpl: usize,
    pub(crate) elements: usize,
    pub(crate) failures: Vec<String>,
    pub(crate) gaps: std::collections::BTreeMap<String, usize>,
}

pub(crate) fn mask_cookie_mip(lo: &[u8], ln: &mut [u8]) {
    use crate::layout::{MIP_BASE, MIP_BYTES, PLANE_BASE};
    let g = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    if g(lo, 0x10) != 16 || g(ln, 0x10) != 16 {
        return;
    }
    let (ft_o, ft_n) = (g(lo, PLANE_BASE + 0x2c), g(ln, PLANE_BASE + 0x2c));
    if ft_o == ft_n + 1 && g(lo, PLANE_BASE + 0x30) + 1 == g(ln, PLANE_BASE + 0x30) {
        ln[PLANE_BASE + 0x2c..PLANE_BASE + 0x34].copy_from_slice(&lo[PLANE_BASE + 0x2c..PLANE_BASE + 0x34]);
    }
    let r = PLANE_BASE + MIP_BASE + 15 * MIP_BYTES;
    ln[r..r + MIP_BYTES].copy_from_slice(&lo[r..r + MIP_BYTES]);
}

pub(crate) unsafe fn compare_srd(create: crate::oracle::Create, d: &[u32; 8], label: &str, t: &mut SrdTally) {
    use crate::layout::{MIP_BASE, MIP_BYTES, PLANE_BASE};
    use crate::oracle::{field, first_diff, Obj};
    use core::ffi::c_void;
    t.cases += 1;
    let mut po: *mut c_void = core::ptr::null_mut();
    let mut pn: *mut c_void = core::ptr::null_mut();
    let hr_o = create(d.as_ptr() as *const u8, &mut po);
    let hr_n = crate::XGCreateTextureComputerFromDescriptor(d.as_ptr(), &mut pn);
    if hr_n == compat_common::E_NOTIMPL {
        t.native_notimpl += 1;
        let why = match crate::layout3d::from_srd(d) {
            Some(Err(e)) => format!("1D/3D: {}", e.why()),
            _ => crate::descriptor::layout_from_srd(d).err().map(|e| e.why()).unwrap_or("?").to_string(),
        };
        let why = if why.ends_with("descriptor format") {
            let hw = d[1] >> 20 & 0x1ff;
            let row = crate::descriptor::HW_FORMAT_INDEX.get(hw as usize).map(|&i| crate::descriptor::FORMAT_INFO[i as usize]);
            format!("{why} {row:?}")
        } else {
            why
        };
        *t.gaps.entry(format!("{why} (console {})", if hr_o >= 0 { "ok".to_string() } else { format!("{:#x}", hr_o as u32) })).or_default() += 1;
        if !po.is_null() {
            drop(Obj(po));
        }
        return;
    }
    if hr_o != hr_n {
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
    let (lo, mut ln) = (o.layout(), n.layout());
    mask_cookie_mip(&lo, &mut ln);
    if let Some((off, x, y)) = first_diff(&lo, &ln) {
        let q = |b: &Vec<u8>, o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        t.failures.push(format!("{label}: layout {} console {x:#x} native {y:#x} (total {:#x}/{:#x} slice {:#x}/{:#x})", field(off), q(&lo, 0), q(&ln, 0), q(&lo, PLANE_BASE + 0x10), q(&ln, PLANE_BASE + 0x10)));
        return;
    }
    if o.size() != n.size() || o.align() != n.align() {
        t.failures.push(format!("{label}: size/align {:#x}/{:#x} vs {:#x}/{:#x}", o.size(), o.align(), n.size(), n.align()));
        return;
    }
    let mips = u32::from_le_bytes(lo[0x10..0x14].try_into().unwrap()).min(15);
    for mip in 0..mips {
        if o.mip_offset(mip) != n.mip_offset(mip) {
            t.failures.push(format!("{label}: mip offset {mip}"));
            return;
        }
        let r = PLANE_BASE + MIP_BASE + mip as usize * MIP_BYTES;
        let we = u32::from_le_bytes(lo[r + 0x30..r + 0x34].try_into().unwrap());
        let he = u32::from_le_bytes(lo[r + 0x34..r + 0x38].try_into().unwrap());
        let arr = u32::from_le_bytes(lo[r + 0x2c..r + 0x30].try_into().unwrap()).max(1);
        for z in [0, arr - 1] {
            for (x, y) in [(0, 0), (1, 0), (0, 1), (we / 2, he / 3), (we - 1, he - 1), (we / 3, he - 1)] {
                if x >= we || y >= he {
                    continue;
                }
                t.elements += 1;
                let (a, b) = (o.elem(mip, x as u64, y, z), n.elem(mip, x as u64, y, z));
                if a != b {
                    t.failures.push(format!("{label}: elem mip{mip} ({x},{y},{z}) console {a:#x} native {b:#x}"));
                    return;
                }
            }
        }
    }
    t.both_ok += 1;
}

#[test]
fn oracle_texture_from_descriptor() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let create: crate::oracle::Create = unsafe { core::mem::transmute(o.f("XGCreateTextureComputerFromDescriptor")) };
    let mut t = SrdTally::default();
    let hws = [0x1u32, 0x7, 0xe, 0x14, 0x17, 0x1e, 0x25, 0x2c, 0x32, 0x38, 0x3e, 0x41, 0x48, 0x4b, 0x4e, 0x80, 0x85, 0x88, 0x89, 0x8a, 0x8d, 0x91, 0x93, 0x97, 0x9c, 0xa9, 0xab, 0xad, 0xb3, 0xb7, 0x109, 0x115, 0x11d, 0x11e];
    for ty in 8..16u32 {
        for &hw in &hws {
            for sw in 0..32u32 {
                for (w, h, depth) in [(1u32, 1u32, 1u32), (64, 64, 1), (100, 37, 3), (1024, 512, 6), (4096, 8, 1)] {
                    for mips in [1u32, 4, 15, 16] {
                        let ll = if ty >= 14 { [0u32, 2][(w as usize) % 2] } else { 0 };
                        let d = srd(ty, hw, sw, w, h, depth, mips, ll);
                        unsafe { compare_srd(create, &d, &format!("ty={ty} hw={hw:#x} sw={sw} {w}x{h}x{depth} mips={mips} ll={ll}"), &mut t) };
                    }
                }
            }
        }
    }
    let mut r = Rng(0x1234_5678_9abc_def1);
    for _ in 0..20000 {
        let mut d = [0u32; 8];
        for v in d.iter_mut() {
            *v = r.u32();
        }
        if r.below(4) != 0 {
            d[3] |= 0x8000_0000;
        }
        if r.below(2) == 0 {
            d[2] &= 0x00ff_00ff;
        }
        unsafe { compare_srd(create, &d, &format!("random {d:08x?}"), &mut t) };
    }
    for _ in 0..40000 {
        let ty = [9u32, 11, 13, 14, 15][r.below(5) as usize];
        let hw = r.below(0x120);
        let sw = r.below(32);
        let w = 1 + if r.below(2) == 0 { r.below(300) } else { r.below(16384) };
        let h = 1 + if r.below(2) == 0 { r.below(300) } else { r.below(16384) };
        let depth = 1 + if r.below(3) == 0 { r.below(64) } else { r.below(3) };
        let chain = crate::layout::full_chain(w, h);
        let mips = 1 + r.below(chain);
        let ll = if ty >= 14 { r.below(4) } else { 0 };
        let mips = if ty >= 14 { if ll != 0 { 1 } else { mips.min(1 + r.below(2)) } } else { mips };
        let mut d = srd(ty, hw, sw, w, h, depth, mips, ll);
        if r.below(4) == 0 {
            d[0] = r.u32();
            d[1] |= r.u32() & 0xff;
            d[3] |= r.u32() & 0xf000;
            d[6] = r.u32();
        }
        unsafe { compare_srd(create, &d, &format!("structured ty={ty} hw={hw:#x} sw={sw} {w}x{h}x{depth} mips={mips} ll={ll} {d:08x?}"), &mut t) };
    }
    eprintln!("from descriptor: {} cases, {} both ok, {} both fail, {} native E_NOTIMPL, {} elements, {} mismatches", t.cases, t.both_ok, t.both_fail, t.native_notimpl, t.elements, t.failures.len());
    for (k, v) in &t.gaps {
        eprintln!(" native E_NOTIMPL: {k}: {v}");
    }
    let mut buckets: std::collections::BTreeMap<String, Vec<&String>> = Default::default();
    for f in &t.failures {
        let key: String = f.split(": ").nth(1).unwrap_or("").split(' ').take(2).collect::<Vec<_>>().join(" ");
        let key = format!("{} {}", f.split(' ').next().unwrap_or(""), key);
        buckets.entry(key).or_default().push(f);
    }
    for (k, v) in &buckets {
        eprintln!(" bucket [{k}] {}", v.len());
        for f in v.iter().take(4) {
            eprintln!("  {f}");
        }
    }
    assert!(t.failures.is_empty());
}

#[test]
fn oracle_texture_from_descriptor_copy_and_hw4() {
    use crate::layout::{MIP_BASE, MIP_BYTES, PLANE_BASE};
    use crate::oracle::{vt, Obj};
    use core::ffi::c_void;
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    let create: crate::oracle::Create = unsafe { core::mem::transmute(o.f("XGCreateTextureComputerFromDescriptor")) };
    type Copy = unsafe extern "system" fn(*mut c_void, *mut u8, u32, u32, *const u8, u32, u32) -> i32;
    let mut bad = Vec::new();
    let mut subs = 0;
    for (ty, hw, sw, w, h, depth, mips) in [(9u32, 0x38u32, 0u32, 70u32, 45u32, 1u32, 3u32), (13, 0x38, 5, 70, 45, 2, 3), (9, 0x4b, 27, 256, 128, 1, 4), (13, 0x1, 9, 33, 17, 3, 2), (9, 0xa9, 26, 300, 40, 1, 5), (11, 0x7, 24, 90, 90, 6, 1), (13, 0xad, 10, 64, 64, 2, 7), (9, 0x41, 21, 513, 9, 1, 1)] {
        let d = srd(ty, hw, sw, w, h, depth, mips, 0);
        let mut po: *mut c_void = core::ptr::null_mut();
        let mut pn: *mut c_void = core::ptr::null_mut();
        let (a, b) = unsafe { (create(d.as_ptr() as *const u8, &mut po), crate::XGCreateTextureComputerFromDescriptor(d.as_ptr(), &mut pn)) };
        if a != b || a < 0 {
            bad.push(format!("ty={ty} hw={hw:#x} sw={sw}: create hr {:#x}/{:#x}", a as u32, b as u32));
            continue;
        }
        let (oo, nn) = (Obj(po), Obj(pn));
        let total = unsafe { oo.size() } as usize;
        let lo = unsafe { oo.layout() };
        let lm = u32::from_le_bytes(lo[0x10..0x14].try_into().unwrap());
        let bpe = u32::from_le_bytes(lo[PLANE_BASE + 0x28..PLANE_BASE + 0x2c].try_into().unwrap()) as usize;
        let mut ra = vec![0u8; total];
        let mut rb = vec![0u8; total];
        for sub in 0..lm * depth {
            let r = PLANE_BASE + MIP_BASE + (sub % lm) as usize * MIP_BYTES;
            let we = u32::from_le_bytes(lo[r + 0x30..r + 0x34].try_into().unwrap()) as usize;
            let he = u32::from_le_bytes(lo[r + 0x34..r + 0x38].try_into().unwrap()) as usize;
            let pitch = we * bpe + 12;
            let src: Vec<u8> = (0..pitch * he).map(|i| (i as u32).wrapping_mul(2654435761).wrapping_add(sub) as u8 | 1).collect();
            unsafe {
                let fa: Copy = core::mem::transmute(vt(oo.0, 8));
                let fb: Copy = core::mem::transmute(vt(nn.0, 8));
                let ha = fa(oo.0, ra.as_mut_ptr(), 0, sub, src.as_ptr(), pitch as u32, (pitch * he) as u32);
                let hb = fb(nn.0, rb.as_mut_ptr(), 0, sub, src.as_ptr(), pitch as u32, (pitch * he) as u32);
                subs += 1;
                if ha != hb {
                    bad.push(format!("ty={ty} hw={hw:#x} sw={sw} sub={sub}: copy hr {:#x}/{:#x}", ha as u32, hb as u32));
                }
            }
        }
        if ra != rb {
            let i = ra.iter().zip(&rb).position(|(a, b)| a != b).unwrap();
            bad.push(format!("ty={ty} hw={hw:#x} sw={sw}: resource bytes differ first at {i:#x}"));
        }
    }
    eprintln!("from descriptor copy into subresource: {subs} subresources, {} mismatches", bad.len());
    type Hw = unsafe extern "system" fn(u32) -> i32;
    let hwv: Hw = unsafe { core::mem::transmute(o.f("XGSetHardwareVersion")) };
    unsafe {
        hwv(4);
        crate::XGSetHardwareVersion(4);
    }
    let mut t = SrdTally::default();
    let mut r = Rng(0x5555_aaaa_1234_8765);
    for _ in 0..20000 {
        let ty = [9u32, 11, 13][r.below(3) as usize];
        let hw = [0x1u32, 0x7, 0xe, 0x14, 0x38, 0x41, 0x4b, 0xa9, 0xab, 0xad, 0x32][r.below(11) as usize];
        let sw = r.below(32);
        let w = 1 + r.below(2048);
        let h = 1 + r.below(2048);
        let depth = 1 + r.below(8);
        let mips = 1 + r.below(crate::layout::full_chain(w, h));
        let d = srd(ty, hw, sw, w, h, depth, mips, 0);
        unsafe { compare_srd(create, &d, &format!("hw4 ty={ty} hw={hw:#x} sw={sw} {w}x{h}x{depth} mips={mips}"), &mut t) };
    }
    unsafe {
        hwv(0);
        crate::XGSetHardwareVersion(0);
    }
    eprintln!("from descriptor hw4: {} cases, {} both ok, {} both fail, {} native E_NOTIMPL, {} elements, {} mismatches", t.cases, t.both_ok, t.both_fail, t.native_notimpl, t.elements, t.failures.len());
    for f in t.failures.iter().take(20) {
        eprintln!("  {f}");
    }
    for b in bad.iter().take(20) {
        eprintln!("  {b}");
    }
    assert!(bad.is_empty() && t.failures.is_empty());
}
