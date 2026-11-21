use std::collections::BTreeMap;
use std::path::PathBuf;

fn find_lib_exe() -> Option<PathBuf> {
    let host = |dir: PathBuf| -> Option<PathBuf> {
        let p = dir.join("bin").join("HostX64").join("x64").join("lib.exe");
        p.exists().then_some(p)
    };
    if let Ok(d) = std::env::var("VCToolsInstallDir") {
        if let Some(p) = host(PathBuf::from(d)) {
            return Some(p);
        }
    }
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
    let vswhere = PathBuf::from(&pf86).join("Microsoft Visual Studio").join("Installer").join("vswhere.exe");
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(o) = std::process::Command::new(&vswhere)
        .args(["-all", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"])
        .output()
    {
        roots.extend(String::from_utf8_lossy(&o.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from));
    }
    for root in roots {
        let msvc = root.join("VC").join("Tools").join("MSVC");
        let Ok(rd) = std::fs::read_dir(&msvc) else { continue };
        let mut vers: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        vers.sort();
        if let Some(p) = vers.into_iter().rev().find_map(host) {
            return Some(p);
        }
    }
    std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|d| d.join("lib.exe")).find(|p| p.exists()))
}

fn read(path: &str) -> String {
    println!("cargo:rerun-if-changed={path}");
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn symbol(name: &str) -> String {
    match name.strip_prefix('#') {
        Some(o) => format!("mux_ord{o}"),
        None => format!("mux_{name}"),
    }
}

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let table = dir.join("exports.txt");
    println!("cargo:rustc-env=MUX_EXPORTS={}", table.display());
    let text = read(&table.display().to_string());
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").map(|e| e == "msvc").unwrap_or(false);
    if !msvc {
        return;
    }
    let mut def = String::from("LIBRARY minuserx\nEXPORTS\n");
    let mut seen: BTreeMap<u16, String> = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (head, note) = line.split_once(" | ").unwrap_or((line, ""));
        let f: Vec<&str> = head.split_whitespace().collect();
        assert_eq!(f.len(), 3, "bad exports.txt line: {line}");
        let ord: u16 = f[0].parse().unwrap_or_else(|_| panic!("bad ordinal: {line}"));
        assert!(seen.insert(ord, f[1].to_string()).is_none(), "duplicate ordinal {ord}");
        match f[2] {
            "forwarded" => {
                let t = note.strip_prefix("PE forwarder to ").and_then(|r| r.split(';').next()).map(str::trim).unwrap_or_else(|| panic!("{} is forwarded without a 'PE forwarder to' target", f[1]));
                assert!(t.starts_with("user32."), "{}: forward target {t} is not user32", f[1]);
                def.push_str(&format!("    {}={} @{}\n", f[1], t, ord));
            }
            "implemented" => {
                if f[1].starts_with('#') {
                    def.push_str(&format!("    {} @{} NONAME\n", symbol(f[1]), ord));
                } else {
                    def.push_str(&format!("    {}={} @{}\n", f[1], symbol(f[1]), ord));
                }
            }
            s => panic!("{}: status {s} not allowed", f[1]),
        }
    }
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let def_path = out_dir.join("minuserx.def");
    let lib_path = out_dir.join("minuserx_exports.lib");
    let exp_path = out_dir.join("minuserx_exports.exp");
    std::fs::write(&def_path, def).unwrap();
    let lib = find_lib_exe().expect("lib.exe (MSVC librarian) not found: needed to build the minuserx export table");
    let st = std::process::Command::new(&lib)
        .arg("/NOLOGO")
        .arg("/MACHINE:X64")
        .arg(format!("/DEF:{}", def_path.display()))
        .arg(format!("/OUT:{}", lib_path.display()))
        .status()
        .unwrap_or_else(|e| panic!("{}: {e}", lib.display()));
    assert!(st.success() && exp_path.exists(), "lib.exe failed to produce {}", exp_path.display());
    println!("cargo:rustc-cdylib-link-arg={}", exp_path.display());
}
