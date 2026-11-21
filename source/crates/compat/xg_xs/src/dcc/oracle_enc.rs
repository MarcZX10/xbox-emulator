use super::{px, Dcc};
use crate::dcc_oracle::configs;

#[test]
fn probe_enc_bijective() {
    let mut bad = std::collections::BTreeMap::new();
    let mut n = 0;
    for (f, sw, dim, p0, _) in configs() {
        let mut d = Dcc::new();
        if d.init(f, sw, dim, p0, 0).is_err() {
            continue;
        }
        n += 1;
        let mut e = Dcc::new();
        e.m.copy_from_slice(&d.m);
        if e.r32(4) == 16 {
            e.w32(4, 8);
        }
        for (a, b, dd) in [(0u32, 1u32, &d), (1, 2, &d), (2, 3, &d), (3, 4, &e)] {
            for k in 0..4u32 {
                let mut seen = [false; 64];
                for x in k * 64..k * 64 + 64 {
                    let c = px::get(dd, x, a).unwrap();
                    let y = px::set(dd, &c, b).unwrap();
                    if y >> 6 != k {
                        *bad.entry(format!("{}->{} crosses seg", a, b)).or_insert(0) += 1;
                    }
                    seen[(y & 63) as usize] = true;
                }
                if seen.iter().any(|&s| !s) {
                    *bad.entry(format!("{}->{} f={} bpe={} sw={} dim={}", a, b, f, d.r32(4), d.r32(0), d.r32(0x14))).or_insert(0) += 1;
                }
            }
        }
    }
    println!("{} configs", n);
    for (k, v) in &bad {
        println!("{} {}", k, v);
    }
}
