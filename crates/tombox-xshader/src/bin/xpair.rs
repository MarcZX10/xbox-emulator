//! xpair: how well can Xbox shaders be paired with shaders of a PC build of the same game?
//!
//! Usage: xpair <xbox pak or archive> <pc .ushaderbytecode>...
//!
//! Exploratory: reports, for candidate pairing keys built from engine metadata that both
//! platforms carry, how many Xbox shaders have a PC shader with the same key and how
//! ambiguous the key is. It pairs nothing by itself.
use std::collections::HashMap;
use tombox_xshader::uearchive::{find_archives, lz4_block, parse_head, split_entry};

/// Input and output signature of a DXBC container as sorted "NAME<index>" lists
/// (ISGN/OSGN: 24-byte elements; ISG1/OSG1: 32-byte, OSG5: 28-byte, both with a leading
/// stream field). Element order and register packing are compiler-specific, names are not.
fn sig_key(c: &[u8]) -> Vec<u8> {
    let rd = |o: usize| c.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).unwrap_or(0);
    let n = rd(28);
    let mut parts: [Vec<String>; 2] = Default::default();
    for i in 0..n.min(64) {
        let off = rd(32 + 4 * i);
        let Some(tag) = c.get(off..off + 4) else { continue };
        let (which, esz, skip) = match tag {
            b"ISGN" => (0, 24, 0),
            b"OSGN" => (1, 24, 0),
            b"ISG1" => (0, 32, 4),
            b"OSG1" => (1, 32, 4),
            b"OSG5" => (1, 28, 4),
            _ => continue,
        };
        let body = off + 8;
        let count = rd(body).min(64);
        for e in 0..count {
            let eo = body + 8 + e * esz + skip;
            let name_off = body + rd(eo);
            let name: String = c.get(name_off..).unwrap_or(&[]).iter().take_while(|&&b| b != 0).take(48).map(|&b| (b as char).to_ascii_uppercase()).collect();
            // The Xbox compiler appends the interpolation mode to the name ("TEXCOORD10_centroid").
            // and then carries the semantic index inside the name with element index 0.
            match name.strip_suffix("_CENTROID") {
                Some(n) => parts[which].push(n.to_string()),
                None => parts[which].push(format!("{name}{}", rd(eo + 4))),
            }
        }
    }
    parts[0].sort();
    parts[1].sort();
    format!("{}|{}", parts[0].join(","), parts[1].join(",")).into_bytes()
}

struct Rec {
    /// (input file index, offset, compressed size, uncompressed size)
    loc: (usize, usize, usize, usize),
    sig: Vec<u8>,
    freq: u8,
    head: Vec<u8>,
    hashes: Vec<u32>,
    parsed: bool,
}

/// The shader container of a record, re-read from its file.
fn container(files: &[String], r: &Rec) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(&files[r.loc.0]).ok()?;
    f.seek(SeekFrom::Start(r.loc.1 as u64)).ok()?;
    let mut src = vec![0u8; r.loc.2];
    f.read_exact(&mut src).ok()?;
    let d = if r.loc.2 == r.loc.3 { src } else { lz4_block(&src, r.loc.3)? };
    split_entry(&d).map(|p| p.container.to_vec())
}

fn load(path: &str, file_idx: usize) -> Vec<Rec> {
    let data = std::fs::read(path).expect("read");
    let mut out = Vec::new();
    for a in find_archives(&data) {
        for (k, &(off, size, usize_)) in a.entries.iter().enumerate() {
            let src = &data[off..off + size];
            let Some(d) = (if size == usize_ { Some(src.to_vec()) } else { lz4_block(src, usize_) }) else { continue };
            let Some(parts) = split_entry(&d) else { continue };
            let ph = parse_head(parts.head);
            out.push(Rec {
                loc: (file_idx, off, size, usize_),
                sig: sig_key(parts.container),
                freq: a.freq[k],
                head: parts.head.to_vec(),
                hashes: ph.as_ref().map(|p| p.1[3].clone()).unwrap_or_default(),
                parsed: ph.is_some(),
            });
        }
    }
    out
}

/// `--decls`: for every uniquely keyed pair, compare the resource declarations the PC
/// compiler recorded with the ones the translator inferred for the Xbox shader.
#[derive(Default)]
struct DeclStats {
    pairs: usize,
    no_pc_decls: usize,
    translate_failed: usize,
    clean: usize,
    /// Same kinds per register class, at different slots.
    permuted: usize,
    /// Slot-independent difference (pc-only kinds / xbox-only kinds) -> (count, example).
    sets: std::collections::BTreeMap<String, (usize, String)>,
    /// Structured buffers declared at the same register on both sides: (pc stride, xbox stride) -> count.
    strides: std::collections::BTreeMap<(u32, u32), usize>,
    /// difference description -> (shaders showing it, first example)
    kinds: std::collections::BTreeMap<String, (usize, String)>,
}

impl DeclStats {
    fn add(&mut self, xc: &[u8], pc: &[u8], freq: u8) {
        use tombox_xshader::decls::{compare, from_dxbc, from_generated_hlsl, Diff, Kind};
        let ordinal = self.pairs;
        self.pairs += 1;
        let Some(r) = from_dxbc(pc) else {
            self.no_pc_decls += 1;
            return;
        };
        let Some(t) = tombox_xshader::extract(xc).ok().and_then(|ex| tombox_xshader::hlsl::translate(&ex).ok()) else {
            self.translate_failed += 1;
            return;
        };
        let x = from_generated_hlsl(&t.hlsl);
        for (k, v) in &r.res {
            if let (Kind::Structured(a), Some(Kind::Structured(b))) = (v, x.res.get(k)) {
                *self.strides.entry((*a, *b)).or_insert(0) += 1;
            }
        }
        let d = compare(&r, &x);
        if d.is_empty() {
            self.clean += 1;
            return;
        }
        let show = |k: &Kind| match k {
            Kind::ConstantBuffer(_) => "cbuffer".to_string(),
            Kind::Sampler { comparison } => if *comparison { "sampler_cmp" } else { "sampler" }.to_string(),
            Kind::Typed { dim, ty } => format!("{dim}<{}>", if *ty == "float" { "float" } else { "int" }),
            Kind::Structured(n) => format!("structured[{n}]"),
            Kind::Raw => "raw".to_string(),
        };
        // Slot-independent view: the two compilers assign registers independently, so a
        // pure slot permutation is not a translation fault.
        let bag = |d: &tombox_xshader::decls::ShaderDecls| {
            let mut v: Vec<String> = d.res.iter().map(|(k, v)| format!("{}:{}", k.0, show(v))).collect();
            v.sort();
            v
        };
        let (mut pb, mut xb) = (bag(&r), bag(&x));
        let mut i = 0;
        while i < pb.len() {
            match xb.iter().position(|e| *e == pb[i]) {
                Some(j) => {
                    xb.remove(j);
                    pb.remove(i);
                }
                None => i += 1,
            }
        }
        if pb.is_empty() && xb.is_empty() {
            self.permuted += 1;
        } else {
            let e = self.sets.entry(format!("pc only [{}]  xbox only [{}]", pb.join(" "), xb.join(" "))).or_insert((0, format!("stage {freq} pair #{ordinal}")));
            e.0 += 1;
        }
        let mut seen = std::collections::BTreeSet::new();
        for x in d {
            let s = match &x {
                Diff::MissingInTranslation(c, k) => format!("{c}: pc {} / xbox none", show(k)),
                Diff::ExtraInTranslation(c, k) => format!("{c}: pc none / xbox {}", show(k)),
                Diff::Mismatch(c, a, b) if matches!(a, Kind::ConstantBuffer(_)) => format!("{c}: xbox {} larger than pc", show(b)),
                Diff::Mismatch(c, a, b) => format!("{c}: pc {} / xbox {}", show(a), show(b)),
                Diff::Threads => "thread group size".to_string(),
            };
            if seen.insert(s.clone()) {
                let e = self.kinds.entry(s).or_insert((0, format!("stage {freq} pair #{ordinal}")));
                e.0 += 1;
            }
        }
    }
    fn report(&self) {
        println!("unique pairs: {}  pc declarations unreadable: {}  xbox translate failed: {}", self.pairs, self.no_pc_decls, self.translate_failed);
        println!("declarations agree: {}  differ: {}", self.clean, self.pairs - self.no_pc_decls - self.translate_failed - self.clean);
        let mut v: Vec<_> = self.kinds.iter().collect();
        v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        for (k, (n, ex)) in v {
            println!("{n:>7}  {k}   (e.g. {ex})");
        }
        println!("
structured buffers at the same register (pc stride, xbox inferred stride): count");
        for ((a, b), n) in &self.strides {
            println!("  pc {a:>3}  xbox {b:>3}  x{n}");
        }
        println!("
slot-independent: {} differ only in slot assignment; {} differ in kind:", self.permuted, self.sets.values().map(|v| v.0).sum::<usize>());
        let mut v: Vec<_> = self.sets.iter().collect();
        v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        for (k, (n, ex)) in v.iter().take(60) {
            println!("{n:>7}  {k}   (e.g. {ex})");
        }
        // Integer-vs-float disagreements are translator faults whatever their count (a float
        // declaration over an integer view reads garbage), so they are always listed.
        let ints: Vec<_> = v.iter().skip(60).filter(|(k, _)| k.contains("<int>")).collect();
        if !ints.is_empty() {
            println!("
further kinds involving integer element types:");
            for (k, (n, ex)) in ints {
                println!("{n:>7}  {k}   (e.g. {ex})");
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: xpair <xbox pak> <pc archive>...");
        std::process::exit(2);
    }
    // Options: --pairs <dir> [--stage n] [--limit n] writes pairs whose key (stage + layout
    // hashes in order + signatures) is unique on both sides as x_<k>.dxbc / p_<k>.dxbc.
    let mut files: Vec<String> = Vec::new();
    let (mut pairs_dir, mut stage, mut limit) = (None, None, 50usize);
    let mut decls = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--pairs" => pairs_dir = it.next().cloned(),
            "--decls" => {
                decls = true;
                pairs_dir = Some(String::new());
                limit = usize::MAX;
            }
            "--stage" => stage = it.next().and_then(|v| v.parse::<u8>().ok()),
            "--limit" => limit = it.next().and_then(|v| v.parse().ok()).unwrap_or(50),
            _ => files.push(a.clone()),
        }
    }
    let xb = load(&files[0], 0);
    let mut pc = Vec::new();
    for (i, p) in files.iter().enumerate().skip(1) {
        pc.extend(load(p, i));
    }
    if let Some(dir) = pairs_dir {
        let key = |r: &Rec| {
            let mut v = vec![r.freq];
            r.hashes.iter().for_each(|h| v.extend_from_slice(&h.to_le_bytes()));
            v.extend_from_slice(&r.sig);
            v
        };
        let mut mx: HashMap<Vec<u8>, Vec<&Rec>> = HashMap::new();
        let mut mp: HashMap<Vec<u8>, Vec<&Rec>> = HashMap::new();
        xb.iter().for_each(|r| mx.entry(key(r)).or_default().push(r));
        pc.iter().for_each(|r| mp.entry(key(r)).or_default().push(r));
        if !decls {
            std::fs::create_dir_all(&dir).expect("mkdir");
        }
        let mut n = 0;
        let mut stats = DeclStats::default();
        let mut keys: Vec<&Vec<u8>> = mx.keys().collect();
        keys.sort();
        for k in keys {
            let (x, p) = (&mx[k], mp.get(k));
            let Some(p) = p else { continue };
            // Records with no layout hashes at all carry no pairing information.
            if x.len() != 1 || p.len() != 1 || stage.map(|s| s != x[0].freq).unwrap_or(false) || x[0].hashes.iter().all(|&h| h == 0) {
                continue;
            }
            let (Some(xc), Some(pcc)) = (container(&files, x[0]), container(&files, p[0])) else { continue };
            if decls {
                stats.add(&xc, &pcc, x[0].freq);
                continue;
            }
            std::fs::write(format!("{dir}/x_{n}.dxbc"), xc).expect("write");
            std::fs::write(format!("{dir}/p_{n}.dxbc"), pcc).expect("write");
            n += 1;
            if n >= limit {
                break;
            }
        }
        if decls {
            stats.report();
            return;
        }
        println!("wrote {n} unique pairs to {dir}");
        return;
    }
    println!("xbox entries: {}  (head parsed: {})", xb.len(), xb.iter().filter(|r| r.parsed).count());
    println!("pc entries:   {}  (head parsed: {})", pc.len(), pc.iter().filter(|r| r.parsed).count());
    if std::env::var("XPAIR_DEBUG").is_ok() {
        // Show how signatures differ within a layout-hash match.
        let hk = |r: &Rec| {
            let mut v = vec![r.freq];
            r.hashes.iter().for_each(|h| v.extend_from_slice(&h.to_le_bytes()));
            v
        };
        let mut m: HashMap<Vec<u8>, Vec<&Rec>> = HashMap::new();
        for r in &pc {
            m.entry(hk(r)).or_default().push(r);
        }
        let mut shown = 0;
        for r in xb.iter().step_by(997) {
            if let Some(c) = m.get(&hk(r)) {
                if !c.iter().any(|p| p.sig == r.sig) && shown < 8 {
                    shown += 1;
                    println!("xbox freq {} : {}", r.freq, String::from_utf8_lossy(&r.sig));
                    let mut sigs: Vec<&[u8]> = c.iter().map(|p| &p.sig[..]).collect();
                    sigs.sort();
                    sigs.dedup();
                    for s in sigs.iter().take(4) {
                        println!("   pc ({} candidates, {} distinct sigs): {}", c.len(), sigs.len(), String::from_utf8_lossy(s));
                    }
                }
            }
        }
    }
    type KeyFn = fn(&Rec) -> Vec<u8>;
    let keys: [(&str, KeyFn); 5] = [
        ("stage + layout hashes (in order) + signatures", |r| {
            let mut v = vec![r.freq];
            r.hashes.iter().for_each(|h| v.extend_from_slice(&h.to_le_bytes()));
            v.extend_from_slice(&r.sig);
            v
        }),
        ("stage + layout hashes (sorted, non-zero) + signatures", |r| {
            let mut h: Vec<u32> = r.hashes.iter().copied().filter(|&x| x != 0).collect();
            h.sort();
            let mut v = vec![r.freq];
            h.iter().for_each(|h| v.extend_from_slice(&h.to_le_bytes()));
            v.extend_from_slice(&r.sig);
            v
        }),
        ("stage + whole header", |r| [&[r.freq][..], &r.head[..]].concat()),
        ("stage + layout hashes (in order)", |r| {
            let mut v = vec![r.freq];
            r.hashes.iter().for_each(|h| v.extend_from_slice(&h.to_le_bytes()));
            v
        }),
        ("stage + layout hashes (sorted, non-zero)", |r| {
            let mut h: Vec<u32> = r.hashes.iter().copied().filter(|&x| x != 0).collect();
            h.sort();
            let mut v = vec![r.freq];
            h.iter().for_each(|h| v.extend_from_slice(&h.to_le_bytes()));
            v
        }),
    ];
    for (name, kf) in keys {
        let mut m: HashMap<Vec<u8>, usize> = HashMap::new();
        for r in &pc {
            *m.entry(kf(r)).or_insert(0) += 1;
        }
        let mut hit = 0usize;
        let mut buckets = [0usize; 5]; // 1, 2-10, 11-100, 101-1000, >1000 candidates
        for r in &xb {
            if let Some(&n) = m.get(&kf(r)) {
                hit += 1;
                buckets[match n { 1 => 0, 2..=10 => 1, 11..=100 => 2, 101..=1000 => 3, _ => 4 }] += 1;
            }
        }
        println!("\nkey: {name}\n  xbox shaders with a PC candidate: {hit} of {} ({:.1}%)", xb.len(), 100.0 * hit as f64 / xb.len() as f64);
        println!("  candidates per hit: 1: {}  2-10: {}  11-100: {}  101-1000: {}  >1000: {}", buckets[0], buckets[1], buckets[2], buckets[3], buckets[4]);
    }
}
