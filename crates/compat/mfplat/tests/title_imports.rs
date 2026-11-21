use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn u16_at(b: &[u8], o: usize) -> usize {
    u16::from_le_bytes([b[o], b[o + 1]]) as usize
}

fn u32_at(b: &[u8], o: usize) -> usize {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize
}

struct Pe<'a> {
    b: &'a [u8],
    secs: Vec<(usize, usize, usize)>,
    dd: usize,
}

impl<'a> Pe<'a> {
    fn new(b: &'a [u8]) -> Self {
        let pe = u32_at(b, 0x3c);
        assert_eq!(&b[pe..pe + 4], b"PE\0\0");
        let nsec = u16_at(b, pe + 6);
        let opt = pe + 24;
        let optsz = u16_at(b, pe + 20);
        let dd = opt + if u16_at(b, opt) == 0x20b { 112 } else { 96 };
        let secs = (0..nsec)
            .map(|i| {
                let s = opt + optsz + i * 40;
                (u32_at(b, s + 12), u32_at(b, s + 8).max(u32_at(b, s + 16)), u32_at(b, s + 20))
            })
            .collect();
        Pe { b, secs, dd }
    }

    fn off(&self, rva: usize) -> Option<usize> {
        self.secs.iter().find(|(va, vs, _)| rva >= *va && rva < va + vs).map(|(va, _, raw)| rva - va + raw)
    }

    fn cstr(&self, rva: usize) -> String {
        let o = self.off(rva).unwrap();
        let end = self.b[o..].iter().position(|&c| c == 0).unwrap();
        String::from_utf8_lossy(&self.b[o..o + end]).into_owned()
    }

    fn exports(&self) -> BTreeSet<String> {
        let erva = u32_at(self.b, self.dd);
        let Some(ed) = self.off(erva) else { return BTreeSet::new() };
        let n = u32_at(self.b, ed + 24);
        let names = self.off(u32_at(self.b, ed + 32)).unwrap();
        (0..n).map(|i| self.cstr(u32_at(self.b, names + i * 4))).collect()
    }

    fn imports_from(&self, dll: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for (dir, desc_size, name_off, thunk_off, delay) in [(1usize, 20usize, 12usize, 0usize, false), (13, 32, 4, 16, true)] {
            let rva = u32_at(self.b, self.dd + dir * 8);
            let Some(mut d) = self.off(rva) else { continue };
            loop {
                let name_rva = u32_at(self.b, d + name_off);
                if name_rva == 0 {
                    break;
                }
                if self.cstr(name_rva).eq_ignore_ascii_case(dll) {
                    let mut t = u32_at(self.b, d + thunk_off);
                    if !delay && t == 0 {
                        t = u32_at(self.b, d + 16);
                    }
                    let mut o = self.off(t).unwrap();
                    loop {
                        let v = u64::from_le_bytes(self.b[o..o + 8].try_into().unwrap());
                        if v == 0 {
                            break;
                        }
                        if v & (1 << 63) == 0 {
                            out.insert(self.cstr(v as usize + 2));
                        }
                        o += 8;
                    }
                }
                d += desc_size;
            }
        }
        out
    }
}

fn built_mfplat() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors().skip(1).take(7).map(|d| d.join("mfplat.dll")).find(|p| p.exists()).expect("built mfplat.dll")
}

fn titles() -> Vec<PathBuf> {
    let root = std::env::var("TOMBOX_GAMES").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("D:\\xbox-pc-port\\games"));
    let mut v = vec![root.join("Fortnite\\extracted\\Mount\\FortniteGame\\Binaries\\XSX\\FortniteClient-XSX-Shipping.exe")];
    for g in ["Terraria", "Trackmania", "XDefiant"] {
        let Ok(rd) = std::fs::read_dir(root.join(g).join("extracted")) else { continue };
        for pkg in rd.flatten() {
            let mount = pkg.path().join("Mount");
            let Ok(files) = std::fs::read_dir(&mount) else { continue };
            v.extend(files.flatten().map(|f| f.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))));
        }
    }
    v.into_iter().filter(|p| p.exists()).collect()
}

fn check(exports: &BTreeSet<String>, path: &Path) -> (usize, Vec<String>) {
    let bytes = std::fs::read(path).unwrap();
    let need = Pe::new(&bytes).imports_from("mfplat.dll");
    let missing = need.iter().filter(|n| !exports.contains(*n)).cloned().collect();
    (need.len(), missing)
}

#[test]
fn every_mfplat_import_is_exported() {
    let ours = std::fs::read(built_mfplat()).unwrap();
    let exports = Pe::new(&ours).exports();
    assert!(exports.len() > 200, "proxy exports only {}", exports.len());
    let sys = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32");
    let mut checked = 0;
    let mut failures = Vec::new();
    let consumers = ["mfreadwrite.dll", "mf.dll", "msmpeg2vdec.dll", "msauddecmft.dll", "msvproc.dll", "mfcore.dll", "mfsrcsnk.dll", "mfmp4srcsnk.dll"];
    for p in titles().into_iter().chain(consumers.iter().map(|d| sys.join(d)).filter(|p| p.exists())) {
        let (n, missing) = check(&exports, &p);
        eprintln!("{}: {n} MFPlat imports, missing {missing:?}", p.display());
        checked += 1;
        if !missing.is_empty() {
            failures.push(format!("{}: {missing:?}", p.display()));
        }
    }
    assert!(checked >= 3);
    assert!(failures.is_empty(), "mfplat proxy lacks exports: {failures:#?}");
}
