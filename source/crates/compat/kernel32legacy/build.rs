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

fn main() {
    let pkg = std::env::var("CARGO_PKG_NAME").unwrap();
    let lib_name = pkg.replace('-', "_");
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("..").join("kernel32legacy");
    let (table, refs) = if lib_name.ends_with("_19041") { ("exports_19041.txt", "extracted") } else { ("exports.txt", "trackmania,xdefiant,terraria") };
    let table_path = dir.join(table);
    let fwd_path = dir.join("forwards.txt");
    println!("cargo:rustc-env=K32L_EXPORTS={}", table_path.display());
    println!("cargo:rustc-env=K32L_FORWARDS={}", fwd_path.display());
    println!("cargo:rustc-env=K32L_REFS={refs}");
    let exports = xone_exports::parse(&read(&table_path.display().to_string()));
    let forwards: BTreeMap<String, String> = read(&fwd_path.display().to_string())
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (n, t) = l.split_once(' ').unwrap_or_else(|| panic!("bad forwards.txt line: {l}"));
            (n.to_string(), t.trim().to_string())
        })
        .collect();
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").map(|e| e == "msvc").unwrap_or(false);
    if !msvc {
        return;
    }
    let mut def = format!("LIBRARY {lib_name}\nEXPORTS\n");
    for e in &exports {
        match e.status.as_str() {
            "forwarded" => {
                let t = forwards.get(&e.name).unwrap_or_else(|| panic!("{} is forwarded but has no forwards.txt target", e.name));
                def.push_str(&format!("    {}={} @{}\n", e.name, t, e.ordinal));
            }
            "implemented" | "hooked" => def.push_str(&format!("    {}=k32l_{} @{}\n", e.name, e.name, e.ordinal)),
            s => panic!("{}: status {s} not allowed", e.name),
        }
    }
    for n in forwards.keys() {
        if let Some(e) = exports.iter().find(|e| &e.name == n) {
            assert_eq!(e.status, "forwarded", "forwards.txt names {n}, which is not a forwarded row");
        }
    }
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let def_path = out_dir.join("kernel32legacy.def");
    let lib_path = out_dir.join("kernel32legacy_exports.lib");
    let exp_path = out_dir.join("kernel32legacy_exports.exp");
    std::fs::write(&def_path, def).unwrap();
    let lib = find_lib_exe().expect("lib.exe (MSVC librarian) not found: needed to build the kernel32legacy export table");
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
