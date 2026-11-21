use core::ffi::c_void;
use super::{oracle, vt, SERIAL};
use crate::bvh::{CompareFn, DecompressFn};

type CreateBvh = unsafe extern "system" fn(*mut *mut c_void) -> i32;

const BASE: usize = 0x8000;
const BUF: usize = 0x18000;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
    fn u32(&mut self) -> u32 {
        self.next() as u32
    }
    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * ((self.next() >> 40) as f32 / (1u64 << 24) as f32)
    }
}

const VFMTS: [(u32, usize); 13] = [(6, 12), (16, 8), (10, 8), (34, 4), (11, 8), (35, 4), (13, 8), (37, 4), (24, 4), (28, 4), (31, 4), (49, 2), (51, 2)];

const SPECIAL: [u32; 11] = [0, 0x8000_0000, 0x7f80_0000, 0xff80_0000, 0x7fc0_0000, 0x7f80_0001, 0xffc1_2345, 0x0000_0123, 0x8000_0456, 0x7149_f2ca, 0xf149_f2ca];

#[derive(Clone, Copy)]
enum Child {
    Empty,
    Leaf(u32),
    Node(usize),
}

struct Node {
    kind: u32,
    off: usize,
    slots: [Child; 4],
}

#[derive(Clone, Copy, PartialEq)]
enum Fault {
    None,
    NotCompressed,
    IndexFormat,
    Geometry,
    VertexFormat,
    LastVertex,
}

struct Case {
    src: Vec<u8>,
    vbs: Vec<Vec<u8>>,
    strides: Vec<u32>,
    vfmts: Vec<u32>,
    ibs: Vec<Vec<u8>>,
    ifmts: Vec<u32>,
    dst_size: usize,
    fill: u64,
    hier_off: usize,
    leaf_off: usize,
    leaves: usize,
    parent_off: isize,
    parent_count: usize,
}

fn put32(b: &mut Vec<u8>, o: usize, v: u32) {
    if b.len() < o + 4 {
        b.resize(o + 4, 0);
    }
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn float_bits(r: &mut Rng) -> u32 {
    if r.chance(12) {
        SPECIAL[r.below(SPECIAL.len() as u64) as usize]
    } else {
        r.f(-50.0, 50.0).to_bits()
    }
}

fn half_bits(r: &mut Rng) -> u16 {
    if r.chance(30) {
        r.u32() as u16
    } else {
        let s = (r.below(2) as u16) << 15;
        let e = (r.below(22) as u16) << 10;
        s | e | (r.u32() as u16 & 0x3ff)
    }
}

fn gen_tree(r: &mut Rng, items: &[Child], nodes: &mut Vec<Node>, next_off: &mut usize) -> Child {
    if items.len() <= 4 && (items.len() <= 1 || r.chance(60)) {
        let kind = if r.chance(50) { 4 } else { 5 };
        let mut slots = [Child::Empty; 4];
        let mut pos: Vec<usize> = (0..4).collect();
        for i in (1..4).rev() {
            let j = r.below(i as u64 + 1) as usize;
            pos.swap(i, j);
        }
        for (k, it) in items.iter().enumerate() {
            slots[pos[k]] = *it;
        }
        let off = *next_off;
        *next_off += if kind == 4 { 64 } else { 128 };
        nodes.push(Node { kind, off, slots });
        return Child::Node(nodes.len() - 1);
    }
    let parts = (2 + r.below(3) as usize).min(items.len());
    let mut cuts: Vec<usize> = (1..items.len()).collect();
    for i in (1..cuts.len()).rev() {
        let j = r.below(i as u64 + 1) as usize;
        cuts.swap(i, j);
    }
    let mut cuts: Vec<usize> = cuts.into_iter().take(parts - 1).collect();
    cuts.sort();
    let mut kids = Vec::new();
    let mut s = 0;
    for c in cuts.into_iter().chain(Some(items.len())) {
        kids.push(gen_tree(r, &items[s..c], nodes, next_off));
        s = c;
    }
    gen_tree(r, &kids, nodes, next_off)
}

fn ptr_of(c: Child, nodes: &[Node], leaf_off: usize) -> u32 {
    match c {
        Child::Empty => u32::MAX,
        Child::Leaf(j) => ((leaf_off + j as usize * 64) >> 3) as u32,
        Child::Node(i) => ((nodes[i].off >> 3) as u32) | nodes[i].kind,
    }
}

fn preorder(c: Child, nodes: &[Node], leaf_off: usize, out: &mut Vec<u32>) {
    if let Child::Node(i) = c {
        for s in nodes[i].slots {
            out.push(ptr_of(s, nodes, leaf_off));
        }
        for s in nodes[i].slots {
            preorder(s, nodes, leaf_off, out);
        }
    }
}

fn postorder(c: Child, nodes: &[Node], out: &mut Vec<(usize, usize)>) {
    if let Child::Node(i) = c {
        for s in nodes[i].slots {
            postorder(s, nodes, out);
        }
        for (k, s) in nodes[i].slots.iter().enumerate() {
            if !matches!(s, Child::Empty) {
                out.push((i, k));
            }
        }
    }
}

fn gen_case(seed: u64, fault: Fault) -> Case {
    let mut r = Rng(seed);
    let geoms = 1 + r.below(4) as usize;
    let mut vbs = Vec::new();
    let mut strides = Vec::new();
    let mut vfmts = Vec::new();
    let mut ibs = Vec::new();
    let mut ifmts = Vec::new();
    let mut tris = Vec::new();
    let verts = 48usize;
    for _ in 0..geoms {
        let (fmt, size) = VFMTS[r.below(VFMTS.len() as u64) as usize];
        let stride = size + 4 * r.below(3) as usize;
        let mut vb = vec![0u8; verts * stride + 16];
        for b in vb.iter_mut() {
            *b = r.u32() as u8;
        }
        for v in 0..verts {
            let p = v * stride;
            match fmt {
                6 | 16 => {
                    for c in 0..size / 4 {
                        let o = p + c * 4;
                        vb[o..o + 4].copy_from_slice(&float_bits(&mut r).to_le_bytes());
                    }
                }
                10 | 34 => {
                    for c in 0..size / 2 {
                        let o = p + c * 2;
                        vb[o..o + 2].copy_from_slice(&half_bits(&mut r).to_le_bytes());
                    }
                }
                _ => {}
            }
        }
        let t = 8 + r.below(40) as usize;
        let r32 = r.chance(50);
        let isz = if r32 { 4 } else { 2 };
        let mut ib = vec![0u8; t * 3 * isz + 8];
        for k in 0..t * 3 {
            let v = r.below(verts as u64) as u32;
            if r32 {
                ib[k * 4..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            } else {
                ib[k * 2..k * 2 + 2].copy_from_slice(&(v as u16).to_le_bytes());
            }
        }
        for b in ib[t * 3 * isz..].iter_mut() {
            *b = r.u32() as u8;
        }
        vbs.push(vb);
        strides.push(stride as u32);
        vfmts.push(fmt);
        ibs.push(ib);
        ifmts.push(if r32 { 0x2a } else { 0x39 });
        tris.push(t as u32);
    }
    let prims = 1 + r.below(24) as usize;
    let pad_pct = if r.chance(40) { 0 } else { 25 };
    let mut recs: Vec<[u32; 3]> = Vec::new();
    let mut valid = 0;
    let sel = [(0u32, 1u32), (1, 0), (0, 2), (2, 0), (1, 1), (1, 2), (2, 1)];
    let make = |r: &mut Rng, pad: bool| -> [u32; 3] {
        let g = r.below(geoms as u64) as usize;
        let d0 = ((g as u32) << 8) | (r.u32() & 0xff);
        let d1 = if pad { u32::MAX } else { r.below(tris[g] as u64) as u32 };
        let (a0, a1) = sel[r.below(sel.len() as u64) as usize];
        let mut d2 = (a0 << 24) | (a1 << 26);
        if r.chance(55) {
            d2 |= r.below(tris[g] as u64) as u32;
            let (b28, b30) = loop {
                let b28 = r.below(3) as u32;
                let b30 = r.below(3) as u32;
                if (b28, b30) != (0, 0) && (b28, b30) != (1, 1) {
                    break (b28, b30);
                }
            };
            d2 |= (b28 << 28) | (b30 << 30);
        } else {
            d2 |= 0x00ff_ffff | ((r.below(16) as u32) << 28);
        }
        [d0, d1, d2]
    };
    while valid < prims {
        let pad = r.chance(pad_pct);
        recs.push(make(&mut r, pad));
        if !pad {
            valid += 1;
        }
    }
    if recs.len() > prims && recs.len() % 2 == 1 {
        let pad = r.chance(50);
        recs.push(make(&mut r, pad));
    }
    let n = recs.len();
    let fi = r.below(n as u64) as usize;
    match fault {
        Fault::Geometry => recs[fi][0] = ((geoms as u32 + r.below(3) as u32) << 8) | (recs[fi][0] & 0xff),
        Fault::LastVertex => {
            let g = (recs[fi][0] >> 8) as usize;
            recs[fi][2] = (recs[fi][2] & 0x0f00_0000) | (1 << 28) | (1 << 30) | r.below(tris[g] as u64) as u32;
        }
        _ => {}
    }
    let leaf_off = 0x40 * (1 + r.below(3) as usize);
    let hier_off = (n * 64 + leaf_off + 0x7f) & !0x7f;
    let mut nodes = Vec::new();
    let mut next_off = hier_off;
    let root = if r.chance(6) {
        Child::Leaf(r.below(n as u64) as u32)
    } else {
        let mut items: Vec<Child> = (0..n as u32).filter(|_| !r.chance(8)).map(Child::Leaf).collect();
        if items.is_empty() {
            items.push(Child::Leaf(0));
        }
        for i in (1..items.len()).rev() {
            let j = r.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
        gen_tree(&mut r, &items, &mut nodes, &mut next_off)
    };
    let hier_end = next_off;
    let mut stream = Vec::new();
    preorder(root, &nodes, leaf_off, &mut stream);
    let mut slots = Vec::new();
    postorder(root, &nodes, &mut slots);
    let mut exc = Vec::new();
    for (ni, k) in slots {
        if !r.chance(20) {
            continue;
        }
        let mask = r.below(64) as u8;
        exc.extend_from_slice(&((nodes[ni].off >> 6) as u32).to_le_bytes());
        exc.push(mask | ((k as u8) << 6));
        for b in 0..6 {
            if mask & (1 << b) != 0 {
                if nodes[ni].kind == 4 {
                    exc.extend_from_slice(&half_bits(&mut r).to_le_bytes());
                } else {
                    exc.extend_from_slice(&float_bits(&mut r).to_le_bytes());
                }
            }
        }
    }
    exc.extend_from_slice(&[0xff; 8]);
    let refs_off = ((0x40 + exc.len() + 3) & !3) + r.below(4) as usize;
    let mut src = vec![0u8; refs_off];
    src[0x40..0x40 + exc.len()].copy_from_slice(&exc);
    for rec in &recs {
        for d in rec {
            src.extend_from_slice(&d.to_le_bytes());
        }
    }
    while src.len() % 4 != 0 {
        src.push(r.u32() as u8);
    }
    for p in &stream {
        src.extend_from_slice(&p.to_le_bytes());
    }
    src.extend_from_slice(&[0xffu8; 32]);
    let flags = (r.u32() & !0x1_0000) | if fault == Fault::NotCompressed { 0 } else { 0x1_0000 };
    let size_units = (hier_end / 64).max(hier_off / 64 + 1) - 1 + r.below(3) as usize;
    let total = (size_units + 1) * 64;
    let parent_count = (hier_end.max(leaf_off + n * 64) - leaf_off) / 64 + 1;
    let parent_off: isize = match r.below(10) {
        0 => 0x40,
        1..=5 => -((parent_count * 4 + 4 * r.below(16) as usize) as isize),
        _ => (total + 4 * r.below(16) as usize) as isize,
    };
    let mut bounds = [0f32; 6];
    for a in 0..3 {
        let (lo, hi) = match r.below(4) {
            0 => (r.f(1.0, 20.0), r.f(25.0, 60.0)),
            1 => (r.f(-60.0, -25.0), r.f(-20.0, -1.0)),
            _ => (r.f(-60.0, -1.0), r.f(1.0, 60.0)),
        };
        bounds[a] = lo;
        bounds[a + 3] = hi;
    }
    let mode = match r.below(6) {
        0 => 0x007f_7f7f,
        1 => 0,
        2 => 1 << 30,
        3 => 0x8000_0000 | (0x70 + r.below(24) as u32) | ((0x70 + r.below(24) as u32) << 8) | ((0x70 + r.below(24) as u32) << 16),
        4 => 0xc000_0000 | (0x70 + r.below(24) as u32) | ((0x70 + r.below(24) as u32) << 8) | ((0x70 + r.below(24) as u32) << 16),
        _ => r.u32(),
    };
    put32(&mut src, 0, flags);
    put32(&mut src, 4, size_units as u32);
    put32(&mut src, 8, r.u32());
    put32(&mut src, 0xc, prims as u32);
    for (k, b) in bounds.iter().enumerate() {
        put32(&mut src, 0x10 + k * 4, b.to_bits());
    }
    put32(&mut src, 0x28, refs_off as u32);
    put32(&mut src, 0x2c, r.u32());
    put32(&mut src, 0x30, ptr_of(root, &nodes, leaf_off));
    put32(&mut src, 0x34, leaf_off as u32);
    put32(&mut src, 0x38, parent_off as i32 as u32);
    put32(&mut src, 0x3c, mode);
    match fault {
        Fault::IndexFormat => {
            let g = r.below(geoms as u64) as usize;
            ifmts[g] = [0x29u32, 0x3a, 0, 0x2b, 57 + 1][r.below(5) as usize];
        }
        Fault::VertexFormat => {
            for v in vfmts.iter_mut() {
                *v = [2u32, 7, 12, 20, 29, 52, 0][r.below(7) as usize];
            }
        }
        _ => {}
    }
    Case { src, vbs, strides, vfmts, ibs, ifmts, dst_size: total, fill: r.next(), hier_off, leaf_off, leaves: n, parent_off, parent_count }
}

unsafe fn run(case: &Case, obj: *mut c_void) -> (i32, Vec<u8>) {
    let mut buf = vec![0u8; BUF];
    let mut f = Rng(case.fill);
    for b in buf.iter_mut() {
        *b = f.next() as u8;
    }
    let vbs: Vec<*const u8> = case.vbs.iter().map(|v| v.as_ptr()).collect();
    let ibs: Vec<*const u8> = case.ibs.iter().map(|v| v.as_ptr()).collect();
    let d: DecompressFn = core::mem::transmute(vt(obj, 0));
    let hr = d(obj as _, case.src.as_ptr(), buf.as_mut_ptr().add(BASE), case.dst_size, vbs.as_ptr(), case.strides.as_ptr(), case.vfmts.as_ptr(), ibs.as_ptr(), case.ifmts.as_ptr(), case.vbs.len() as u32);
    (hr, buf)
}

unsafe fn cmp(obj: *mut c_void, a: &[u8], b: &[u8]) -> i32 {
    let c: CompareFn = core::mem::transmute(vt(obj, 1));
    c(obj as _, a.as_ptr().add(BASE), b.as_ptr().add(BASE))
}

fn first_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| x != y)
}

#[test]
fn oracle_bvh_computer() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let o = match oracle() {
        Some(o) => o,
        None => return,
    };
    unsafe {
        let sym = super::GetProcAddress(o.h as *mut c_void, b"XGCreateBVHComputer\0".as_ptr());
        if sym.is_null() {
            eprintln!("oracle skipped: XGCreateBVHComputer not exported by this console xg_xs.dll");
            return;
        }
        let oc: CreateBvh = core::mem::transmute(sym);
        let mut op: *mut c_void = core::ptr::null_mut();
        let mut np: *mut c_void = core::ptr::null_mut();
        assert_eq!(oc(&mut op), 0);
        assert_eq!(crate::XGCreateBVHComputer(&mut np), 0);
        assert!(!op.is_null() && !np.is_null());
        let faults = [Fault::None, Fault::NotCompressed, Fault::IndexFormat, Fault::Geometry, Fault::VertexFormat, Fault::LastVertex];
        let mut cases = 0;
        let mut ok_cases = 0;
        let mut hrs = std::collections::BTreeMap::new();
        let mut compares = 0;
        let mut compare_hits = [0u32; 2];
        let mut mism = 0;
        for seed in 0..6000u64 {
            let fault = if seed % 4 == 0 { faults[(seed / 4) as usize % faults.len()] } else { Fault::None };
            let case = gen_case(seed.wrapping_mul(0x1234_5678_9abc_def1) ^ 0x5eed, fault);
            let trace = std::env::var("BVH_TRACE").is_ok();
            if trace {
                eprintln!("seed {seed} oracle");
            }
            let (ohr, obuf) = run(&case, op);
            if trace {
                eprintln!("seed {seed} native");
            }
            let (nhr, nbuf) = run(&case, np);
            if trace {
                eprintln!("seed {seed} ran {ohr:#x} {nhr:#x} same={}", obuf == nbuf);
            }
            cases += 1;
            *hrs.entry(ohr as u32).or_insert(0u32) += 1;
            if ohr != nhr || obuf != nbuf {
                mism += 1;
                if mism <= 10 {
                    let d = first_diff(&obuf, &nbuf).map(|x| x as isize - BASE as isize);
                    eprintln!("seed {seed} fault {:?}: oracle {ohr:#x} native {nhr:#x} first diff at dst{:+?} hier_off {:#x} leaf_off {:#x} leaves {}", fault as u32, d, case.hier_off, case.leaf_off, case.leaves);
                }
                continue;
            }
            if ohr != 0 {
                continue;
            }
            ok_cases += 1;
            let root_kind = u32::from_le_bytes(case.src[0x30..0x34].try_into().unwrap()) & 7;
            if (root_kind != 4 && root_kind != 5) || case.parent_off == 0x40 {
                continue;
            }
            let oc0 = cmp(op, &obuf, &nbuf);
            let nc0 = cmp(np, &obuf, &nbuf);
            compares += 1;
            if oc0 != nc0 || oc0 != 0 {
                mism += 1;
                eprintln!("seed {seed}: compare identical oracle {oc0} native {nc0}");
            }
            let mut r = Rng(seed ^ 0xc0ffee);
            for t in 0..6 {
                let mut b = nbuf.clone();
                let off: isize = match t {
                    0 => r.below(0x40) as isize,
                    1 => (case.leaf_off + r.below(case.leaves as u64 * 64) as usize) as isize,
                    2 => (case.hier_off + r.below((case.dst_size - 64 - case.hier_off).max(1) as u64) as usize) as isize,
                    3 => case.parent_off + 4 * r.below(case.parent_count as u64) as isize,
                    4 => case.parent_off + 4 * r.below(case.parent_count as u64) as isize + r.below(4) as isize,
                    _ => r.below(case.dst_size as u64 - 64) as isize,
                };
                if t == 2 && case.dst_size - 64 <= case.hier_off {
                    continue;
                }
                let at = (BASE as isize + off) as usize;
                b[at] ^= 1 << r.below(8);
                if trace {
                    eprintln!("seed {seed} flip {t} at {off:#x}");
                }
                let x = cmp(op, &obuf, &b);
                let y = cmp(np, &obuf, &b);
                compares += 1;
                compare_hits[(x != 0) as usize] += 1;
                if x != y {
                    mism += 1;
                    eprintln!("seed {seed} flip {t} at {off:#x}: compare oracle {x} native {y}");
                }
            }
        }
        eprintln!("bvh decompress cases {cases} ({ok_cases} S_OK) hr histogram {hrs:x?}; compare calls {compares} (equal {} differ {}); mismatches {mism}", compare_hits[0], compare_hits[1]);
        assert_eq!(mism, 0);
    }
}
