use std::path::PathBuf;

fn exports(crate_dir: &str) -> Vec<String> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(crate_dir).join("exports.txt");
    std::fs::read_to_string(p).unwrap().lines().filter_map(|l| l.split_whitespace().nth(1).map(str::to_string)).collect()
}

#[test]
fn bound_names_exist_in_compat_exports() {
    let x = exports("xpal");
    assert!(x.iter().any(|n| n == "XIsCapabilityEnabled"));
    let c = exports("xcrdapi");
    for n in ["XCrdOpenAdapter", "XCrdCloseAdapter", "XCrdMountContentType", "XCrdQueryDevicePath", "XCrdFreeDevicePathBuffer", "XCrdUnmount"] {
        assert!(c.iter().any(|e| e == n), "{n}");
    }
}

#[test]
fn capability_0x7f_from_built_xpal_matches_xpal_export() {
    let exe = std::env::current_exe().unwrap();
    let Some(dll) = exe.ancestors().map(|a| a.join("xpal.dll")).find(|p| p.is_file()) else {
        eprintln!("SKIP: xpal.dll not built in this target dir");
        return;
    };
    let got = gamehost::capability_from(&dll.display().to_string(), 0x7f).unwrap();
    let direct = gamehost::capability_from(&dll.display().to_string(), 0x80).unwrap();
    assert!(!direct);
    eprintln!("xpal XIsCapabilityEnabled(0x7f) = {got}");
}
