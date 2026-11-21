const EXPORTS: &str = include_str!("../exports.txt");

#[test]
fn table_matches_console_dll() {
    let ex = xone_exports::parse(EXPORTS);
    assert_eq!(ex.len(), 133);
    let want = xone_exports::table(&ex);
    let refs = xone_exports::reference_dlls("xbtp.dll");
    for r in &refs {
        let got = xone_exports::pe_exports(&std::fs::read(r).unwrap()).expect("reference PE");
        assert_eq!(got, want, "exports.txt differs from {}", r.display());
    }
    for e in &ex {
        assert!(xone_exports::STATUSES.contains(&e.status.as_str()), "{}: bad status {}", e.name, e.status);
    }
}

#[test]
fn every_export_has_a_definition() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut text = String::new();
    for e in std::fs::read_dir(&src).unwrap().flatten() {
        if e.path().extension().map(|x| x == "rs").unwrap_or(false) {
            text.push_str(&std::fs::read_to_string(e.path()).unwrap());
        }
    }
    for e in xone_exports::parse(EXPORTS) {
        if e.status == "missing" {
            continue;
        }
        let a = format!("export_name = \"{}\"", e.name);
        let b = format!("fn {}(", e.name);
        let c = format!("\"{}\"", e.name);
        assert!(text.contains(&a) || text.contains(&b) || text.contains(&c), "{} has no definition", e.name);
    }
}
