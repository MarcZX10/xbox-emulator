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

fn main() {
    println!("cargo:rerun-if-changed=exports.txt");
    println!("cargo:rerun-if-changed=forwards.txt");
    let exports = xone_exports::parse(&std::fs::read_to_string("exports.txt").expect("exports.txt"));
    let forwards: Vec<(String, String)> = std::fs::read_to_string("forwards.txt")
        .expect("forwards.txt")
        .lines()
        .filter_map(|l| l.split_once(' ').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())))
        .collect();
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut def = String::from("LIBRARY dab\nEXPORTS\n");
    let mut table = String::from("pub static FORWARDS: &[(u16, &str, &str)] = &[\n");
    for e in &exports {
        let (_, target) = forwards.iter().find(|(n, _)| *n == e.name).unwrap_or_else(|| panic!("{} has no forward target", e.name));
        assert_eq!(e.status, "forwarded", "{}", e.name);
        def.push_str(&format!("    {}={} @{}\n", e.name, target, e.ordinal));
        table.push_str(&format!("    ({}, {:?}, {:?}),\n", e.ordinal, e.name, target));
    }
    table.push_str("];\n");
    std::fs::write(out_dir.join("forwards.rs"), table).unwrap();
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").map(|e| e == "msvc").unwrap_or(false);
    if msvc {
        let def_path = out_dir.join("dab_forward.def");
        let lib_path = out_dir.join("dab_forward.lib");
        let exp_path = out_dir.join("dab_forward.exp");
        std::fs::write(&def_path, def).unwrap();
        let lib = find_lib_exe().expect("lib.exe (MSVC librarian) not found: needed to build the dab forwarder export table");
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
}