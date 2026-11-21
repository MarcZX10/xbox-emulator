use super::super::*;

const FC: [&str; 16] = ["f", "lt", "eq", "le", "gt", "lg", "ge", "o", "u", "nge", "nlg", "ngt", "nle", "neq", "nlt", "tru"];
const IC: [&str; 8] = ["f", "lt", "eq", "le", "gt", "ne", "ge", "t"];
const SENT: u32 = 0x5a5a_5a5a;

#[derive(Clone, Copy, PartialEq)]
enum Ty {
    F16,
    F32,
    F64,
    I32,
    U32,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::F16 => "f16",
            Ty::F32 => "f32",
            Ty::F64 => "f64",
            Ty::I32 => "i32",
            Ty::U32 => "u32",
        }
    }
    fn conds(self) -> usize {
        if matches!(self, Ty::I32 | Ty::U32) { 8 } else { 16 }
    }
    fn op(self, i: usize, x: bool) -> u32 {
        let i = i as u32;
        let x = if x { 0x10 } else { 0 };
        match self {
            Ty::F32 => x | i,
            Ty::F64 => 0x20 | x | i,
            Ty::I32 => 0x80 | x | i,
            Ty::U32 => 0xc0 | x | i,
            Ty::F16 => (if i < 8 { 0xc8 + i } else { 0xe8 + i - 8 }) | x,
        }
    }
    fn mnem(self, i: usize, x: bool) -> &'static str {
        let cc = if self.conds() == 8 { IC[i] } else { FC[i] };
        Box::leak(format!("v_cmp{}_{cc}_{}", if x { "x" } else { "" }, self.name()).into_boxed_str())
    }
    fn val(self, b: u64) -> f64 {
        match self {
            Ty::F16 => h2f(b as u32 & 0xffff) as f64,
            Ty::F32 => f32::from_bits(b as u32) as f64,
            Ty::F64 => f64::from_bits(b),
            Ty::I32 => b as u32 as i32 as f64,
            Ty::U32 => b as u32 as f64,
        }
    }
    fn sign(self) -> u64 {
        match self {
            Ty::F16 => 0x8000,
            Ty::F32 => 0x8000_0000,
            _ => 1 << 63,
        }
    }
    fn wide(self) -> bool {
        self == Ty::F64
    }
}

fn want(t: Ty, i: usize, a: u64, b: u64) -> bool {
    let (x, y) = (t.val(a), t.val(b));
    if t.conds() == 8 {
        return match i {
            0 => false,
            1 => x < y,
            2 => x == y,
            3 => x <= y,
            4 => x > y,
            5 => x != y,
            6 => x >= y,
            _ => true,
        };
    }
    match i {
        0 => false,
        1 => x < y,
        2 => x == y,
        3 => x <= y,
        4 => x > y,
        5 => x < y || x > y,
        6 => x >= y,
        7 => !x.is_nan() && !y.is_nan(),
        8 => x.is_nan() || y.is_nan(),
        9 => !(x >= y),
        10 => !(x < y || x > y),
        11 => !(x > y),
        12 => !(x <= y),
        13 => !(x == y),
        14 => !(x < y),
        _ => true,
    }
}

fn inputs(t: Ty, a: u64, b: u64) -> Prog {
    let mut p = pr();
    if t.wide() {
        p.vv(2, a as u32).vv(3, (a >> 32) as u32).vv(4, b as u32).vv(5, (b >> 32) as u32);
    } else if t == Ty::F16 {
        p.vv(1, 0xdead_0000 | a as u32).vv(2, 0xbeef_0000 | b as u32);
    } else {
        p.vv(1, a as u32).vv(2, b as u32);
    }
    p
}

fn srcs(t: Ty) -> (u32, u32) {
    if t.wide() { (v(2), 4) } else { (v(1), 2) }
}

fn post_cmpx() -> Prog {
    let mut p = pr();
    p.raw(&[sop1(3, 30, 126), sop1(3, 126, ic(1)), vop1(1, 0, v(10))]).s2v(1, 30).s2v(2, 106);
    p
}

fn add_e32(c: &mut Cases, t: Ty, i: usize, a: u64, b: u64) {
    let (s0, s1) = srcs(t);
    let r = want(t, i, a, b) as u32;
    let mut pre = inputs(t, a, b);
    pre.sv(106, SENT);
    let mut post = pr();
    post.vcc_to_v(0);
    c.add(t.mnem(i, false), &pre, &[vopc(t.op(i, false), s0, s1)], &post, &[r]);
    let mut pre = inputs(t, a, b);
    pre.sv(106, SENT).vv(10, 0x1234);
    c.add(t.mnem(i, true), &pre, &[vopc(t.op(i, true), s0, s1), vop1(1, 10, ic(7))], &post_cmpx(), &[if r != 0 { 7 } else { 0x1234 }, r, SENT]);
}

fn add_e64(c: &mut Cases, t: Ty, i: usize, a: u64, b: u64, abs: u32, neg: u32) {
    let (s0, _) = srcs(t);
    let s1 = if t.wide() { v(4) } else { v(2) };
    let md = |x: u64, k: u32| {
        let x = if (abs >> k) & 1 != 0 { x & !t.sign() } else { x };
        if (neg >> k) & 1 != 0 { x ^ t.sign() } else { x }
    };
    let r = want(t, i, md(a, 0), md(b, 1)) as u32;
    let mut pre = inputs(t, a, b);
    pre.sv(20, 0xdead_beef).sv(106, SENT);
    let mut post = pr();
    post.s2v(0, 20).s2v(1, 106);
    c.add(t.mnem(i, false), &pre, &vop3m(t.op(i, false), 20, [s0, s1, 0], abs, neg, 0, false), &post, &[r, SENT]);
    let mut pre = inputs(t, a, b);
    pre.sv(106, SENT).vv(10, 0x1234);
    let mut op = vop3m(t.op(i, true), 126, [s0, s1, 0], abs, neg, 0, false).to_vec();
    op.push(vop1(1, 10, ic(7)));
    c.add(t.mnem(i, true), &pre, &op, &post_cmpx(), &[if r != 0 { 7 } else { 0x1234 }, r, SENT]);
}

fn add_dst(c: &mut Cases, t: Ty, i: usize, a: u64, b: u64) {
    let (s0, _) = srcs(t);
    let s1 = if t.wide() { v(4) } else { v(2) };
    let r = want(t, i, a, b) as u32;
    let mut pre = inputs(t, a, b);
    pre.sv(106, SENT);
    let mut post = pr();
    post.vcc_to_v(0);
    c.add(t.mnem(i, false), &pre, &vop3m(t.op(i, false), 106, [s0, s1, 0], 0, 0, 0, false), &post, &[r]);
    let mut pre = inputs(t, a, b);
    pre.sv(20, 0xdead_beef).sv(106, SENT);
    let mut post = pr();
    post.s2v(0, 20).s2v(1, 106);
    c.add(t.mnem(i, false), &pre, &vop3m(t.op(i, false), 125, [s0, s1, 0], 0, 0, 0, false), &post, &[0xdead_beef, SENT]);
}

fn add_masked(c: &mut Cases, t: Ty, i: usize, a: u64, b: u64) {
    let (s0, s1) = srcs(t);
    let mut pre = inputs(t, a, b);
    pre.sv(106, SENT).vv(10, 0x1234).raw(&[sop1(3, 126, ic(0))]);
    let mut post = pr();
    post.raw(&[sop1(3, 126, ic(1))]).vcc_to_v(0);
    c.add(t.mnem(i, false), &pre, &[vopc(t.op(i, false), s0, s1)], &post, &[0]);
    c.add(t.mnem(i, true), &pre, &[vopc(t.op(i, true), s0, s1), vop1(1, 10, ic(7))], &post_cmpx(), &[0x1234, 0, SENT]);
}

fn add_inline(c: &mut Cases, t: Ty, i: usize, b: u64) {
    let one = match t {
        Ty::F16 => 0x3c00,
        Ty::F32 => fb(1.0) as u64,
        _ => 1f64.to_bits(),
    };
    let s1 = srcs(t).1;
    let r = want(t, i, one, b) as u32;
    let pre = inputs(t, 0, b);
    let mut post = pr();
    post.vcc_to_v(0);
    c.add(t.mnem(i, false), &pre, &[vopc(t.op(i, false), 242, s1)], &post, &[r]);
}

fn fpairs(t: Ty) -> Vec<(u64, u64)> {
    let (one, two, m1, z, nz, inf, ninf, qn, sn, nn, max, m25) = match t {
        Ty::F16 => (0x3c00, 0x4000, 0xbc00, 0, 0x8000, 0x7c00, 0xfc00, 0x7e00, 0x7c01, 0xfe00, 0x7bff, 0xc100),
        Ty::F32 => (0x3f80_0000, 0x4000_0000, 0xbf80_0000, 0, 0x8000_0000, 0x7f80_0000, 0xff80_0000, 0x7fc0_0000, 0x7f80_0001, 0xffc0_0000, 0x7f7f_ffff, 0xc020_0000),
        _ => (
            1f64.to_bits(),
            2f64.to_bits(),
            (-1f64).to_bits(),
            0,
            1 << 63,
            f64::INFINITY.to_bits(),
            f64::NEG_INFINITY.to_bits(),
            0x7ff8_0000_0000_0000,
            0x7ff0_0000_0000_0001,
            0xfff8_0000_0000_0000,
            f64::MAX.to_bits(),
            (-2.5f64).to_bits(),
        ),
    };
    let mut v = vec![
        (one, two),
        (two, one),
        (one, one),
        (z, nz),
        (nz, z),
        (m1, m25),
        (inf, inf),
        (ninf, inf),
        (inf, max),
        (qn, one),
        (one, sn),
        (nn, nn),
        (ninf, nn),
    ];
    match t {
        Ty::F16 => v.extend([(1, 0), (1, 2), (0x8001, 0)]),
        Ty::F64 => v.extend([((1e-310f64).to_bits(), 0), (0x3ff0_0000_0000_0001, 0x3ff0_0000_0000_0000), (0x7ff0_0000_0000_0001, 0x7ff0_0000_0000_0001)]),
        _ => v.push((0x3f80_0001, 0x3f80_0000)),
    }
    v
}

fn ipairs() -> Vec<(u64, u64)> {
    [(i32::MIN as u32, i32::MAX as u32), (i32::MAX as u32, i32::MIN as u32), (u32::MAX, 0), (0, u32::MAX), (5, 5), ((-7i32) as u32, (-7i32) as u32), (0x8000_0000, 0x7fff_ffff), (1, 2), (2, 1), (0, 0)]
        .iter()
        .map(|&(a, b)| (a as u64, b as u64))
        .collect()
}

fn group(t: Ty) -> Cases {
    let mut c = Cases::new();
    let pairs = if t.conds() == 8 { ipairs() } else { fpairs(t) };
    for i in 0..t.conds() {
        for &(a, b) in &pairs {
            add_e32(&mut c, t, i, a, b);
        }
        for &(a, b) in pairs.iter().step_by(3) {
            add_e64(&mut c, t, i, a, b, 0, 0);
        }
        add_dst(&mut c, t, i, pairs[0].0, pairs[0].1);
        add_masked(&mut c, t, i, pairs[2].0, pairs[2].1);
        if t.conds() == 16 {
            let sg = t.sign();
            let (nn, one, z) = (pairs[11].0, pairs[2].0, 0u64);
            add_e64(&mut c, t, i, nn, nn, 3, 0);
            add_e64(&mut c, t, i, z, z, 0, 2);
            add_e64(&mut c, t, i, one | sg, one, 1, 0);
            add_e64(&mut c, t, i, one, one | sg, 0, 2);
            add_e64(&mut c, t, i, one, one, 1, 1);
            add_inline(&mut c, t, i, one);
            add_inline(&mut c, t, i, pairs[0].1);
        }
    }
    c
}

#[test]
fn vopc_f32() {
    group(Ty::F32).run_both("vopc_f32");
}

#[test]
fn vopc_f16() {
    group(Ty::F16).run_both("vopc_f16");
}

#[test]
fn vopc_f64() {
    group(Ty::F64).run_both("vopc_f64");
}

#[test]
fn vopc_i32() {
    group(Ty::I32).run_both("vopc_i32");
}

#[test]
fn vopc_u32() {
    group(Ty::U32).run_both("vopc_u32");
}
