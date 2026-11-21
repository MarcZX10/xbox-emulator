const VERSION: [u16; 4] = [10, 0, 22621, 1852];

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(Some(0)).flat_map(|c| c.to_le_bytes()).collect()
}

fn pad4(v: &mut Vec<u8>) {
    while v.len() % 4 != 0 {
        v.push(0);
    }
}

fn block(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0u8; 6];
    b[2..4].copy_from_slice(&value_len.to_le_bytes());
    b[4..6].copy_from_slice(&(text as u16).to_le_bytes());
    b.extend_from_slice(&utf16z(key));
    pad4(&mut b);
    b.extend_from_slice(value);
    for c in children {
        pad4(&mut b);
        b.extend_from_slice(c);
    }
    let len = b.len() as u16;
    b[0..2].copy_from_slice(&len.to_le_bytes());
    b
}

fn string_entry(key: &str, value: &str) -> Vec<u8> {
    let v = utf16z(value);
    block(key, &v, (v.len() / 2) as u16, true, &[])
}

fn version_info() -> Vec<u8> {
    let text = format!("{}.{}.{}.{}", VERSION[0], VERSION[1], VERSION[2], VERSION[3]);
    let ms = ((VERSION[0] as u32) << 16) | VERSION[1] as u32;
    let ls = ((VERSION[2] as u32) << 16) | VERSION[3] as u32;
    let mut fixed = Vec::new();
    for v in [0xfeef_04bdu32, 0x0001_0000, ms, ls, ms, ls, 0x3f, 0, 0x0004_0004, 2, 0, 0, 0] {
        fixed.extend_from_slice(&v.to_le_bytes());
    }
    let strings = [
        string_entry("CompanyName", "TomBox"),
        string_entry("FileDescription", "wdbase.dll compatibility library"),
        string_entry("FileVersion", &text),
        string_entry("InternalName", "wdbase.dll"),
        string_entry("OriginalFilename", "wdbase.dll"),
        string_entry("ProductName", "TomBox"),
        string_entry("ProductVersion", &text),
    ];
    let table = block("040904B0", &[], 0, true, &strings);
    let sfi = block("StringFileInfo", &[], 0, true, &[table]);
    let translation = block("Translation", &[0x09, 0x04, 0xb0, 0x04], 4, false, &[]);
    let vfi = block("VarFileInfo", &[], 0, true, &[translation]);
    block("VS_VERSION_INFO", &fixed, fixed.len() as u16, false, &[sfi, vfi])
}

fn version_res(out_dir: &str) -> String {
    let data = version_info();
    let mut res = Vec::new();
    let header = |res: &mut Vec<u8>, size: u32, kind: u16, name: u16, flags: u16, lang: u16| {
        res.extend_from_slice(&size.to_le_bytes());
        res.extend_from_slice(&0x20u32.to_le_bytes());
        res.extend_from_slice(&0xffffu16.to_le_bytes());
        res.extend_from_slice(&kind.to_le_bytes());
        res.extend_from_slice(&0xffffu16.to_le_bytes());
        res.extend_from_slice(&name.to_le_bytes());
        res.extend_from_slice(&0u32.to_le_bytes());
        res.extend_from_slice(&flags.to_le_bytes());
        res.extend_from_slice(&lang.to_le_bytes());
        res.extend_from_slice(&0u32.to_le_bytes());
        res.extend_from_slice(&0u32.to_le_bytes());
    };
    header(&mut res, 0, 0, 0, 0, 0);
    header(&mut res, data.len() as u32, 16, 1, 0x30, 0x0409);
    res.extend_from_slice(&data);
    pad4(&mut res);
    let path = format!("{}\\wdbase_version.res", out_dir);
    std::fs::write(&path, res).expect("write res");
    path
}

fn main() {
    println!("cargo:rerun-if-changed=exports.txt");
    println!("cargo:rerun-if-changed=build.rs");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").map(|e| e == "msvc").unwrap_or(false);
    if !msvc {
        return;
    }
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    println!("cargo:rustc-link-arg={}", version_res(&out_dir));
    let text = std::fs::read_to_string("exports.txt").expect("exports.txt");
    for e in xone_exports::parse(&text) {
        if e.status == "missing" {
            continue;
        }
        match e.note.strip_prefix("PE forwarder to ").and_then(|r| r.split(';').next()) {
            Some(target) => println!("cargo:rustc-cdylib-link-arg=/EXPORT:{}={},@{}", e.name, target.trim(), e.ordinal),
            None => println!("cargo:rustc-cdylib-link-arg=/EXPORT:{},@{}", e.name, e.ordinal),
        }
    }
}
