use super::*;

fn sys(name: &str) -> PathBuf {
    PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join("System32").join(name)
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("compat_smoke_test_{tag}_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d
}

#[test]
fn rows_parse_status_and_ordinal_names() {
    let d = scratch("rows");
    let t = d.join("exports.txt");
    std::fs::write(&t, "1 Foo implemented | note with | pipes\n2 #225 forwarded | x\n3 Bar missing\n\n4 Baz hooked\n").unwrap();
    let r = rows(&t);
    assert_eq!(r.len(), 4);
    assert_eq!((r[0].name.as_str(), r[0].status.as_str()), ("Foo", "implemented"));
    assert_eq!((r[1].name.as_str(), r[1].status.as_str()), ("#225", "forwarded"));
    assert_eq!(r[2].status, "missing");
    assert_eq!(r.iter().filter(|x| COVERED.contains(&x.status.as_str())).count(), 3);
}

#[test]
fn field_reads_section_keys_only() {
    let t = "[package]\nname = \"pkg-a\"\n\n[lib]\nname = \"liba\"\ncrate-type = [\"cdylib\", \"rlib\"]\n[dependencies]\nname = \"wrong\"\n";
    assert_eq!(field(t, "package", "name").as_deref(), Some("pkg-a"));
    assert_eq!(field(t, "lib", "name").as_deref(), Some("liba"));
    assert!(field(t, "lib", "crate-type").unwrap().contains("cdylib"));
    assert_eq!(field(t, "lib", "path"), None);
    assert_eq!(field("[package]\nname=\"x\"\n", "lib", "name"), None);
}

#[test]
fn pe_exports_include_forwarders() {
    let img = pe::Pe::parse(std::fs::read(sys("kernel32.dll")).unwrap()).unwrap();
    assert!(img.pe64);
    let ex = img.exports();
    assert!(ex.iter().any(|e| e.name.as_deref() == Some("GetProcAddress")));
    let fwd: Vec<_> = ex.iter().filter_map(|e| e.forwarder.as_deref()).collect();
    assert!(!fwd.is_empty());
    assert!(fwd.iter().all(|f| f.contains('.')));
}

#[test]
fn pe_imports_list_static_and_delay() {
    let img = pe::Pe::parse(std::fs::read(std::env::current_exe().unwrap()).unwrap()).unwrap();
    let im = img.imports();
    let k = im.iter().find(|i| i.dll.eq_ignore_ascii_case("kernel32.dll") && !i.delay).unwrap();
    assert!(k.names.iter().any(|n| n == "LoadLibraryExW"));
    let sh = pe::Pe::parse(std::fs::read(sys("shell32.dll")).unwrap()).unwrap();
    assert!(sh.imports().iter().any(|i| i.delay));
}

#[test]
fn pe_rejects_non_pe() {
    assert!(pe::Pe::parse(b"not a pe file at all".to_vec()).is_none());
    assert!(pe::Pe::parse(Vec::new()).is_none());
}

#[test]
fn resolve_by_name_and_ordinal() {
    let h = load(&sys("kernel32.dll"), 0).unwrap();
    assert!(resolve(h, "GetProcAddress"));
    assert!(!resolve(h, "NoSuchExportXyz"));
    let img = pe::Pe::parse(std::fs::read(sys("kernel32.dll")).unwrap()).unwrap();
    let e = img.exports().into_iter().find(|e| e.name.as_deref() == Some("GetProcAddress")).unwrap();
    assert!(resolve(h, &format!("#{}", e.ordinal)));
}

#[test]
fn load_reports_win32_error() {
    let d = scratch("missing");
    assert_eq!(load(&d.join("does_not_exist.dll"), LOAD_WITH_ALTERED_SEARCH_PATH).unwrap_err(), 126);
}

#[test]
fn load_exact_returns_the_requested_file() {
    let d = scratch("exact");
    let f = d.join("smoke_version_copy.dll");
    std::fs::copy(sys("version.dll"), &f).unwrap();
    let (h, alias) = load_exact(&f).unwrap();
    assert!(alias.is_none());
    assert!(same_file(&module_path(h), &f));
}

#[test]
fn same_file_compares_canonical_paths() {
    let k = sys("kernel32.dll");
    assert!(same_file(&k, &PathBuf::from(k.to_string_lossy().to_ascii_uppercase())));
    assert!(!same_file(&k, &sys("ntdll.dll")));
}

#[test]
fn load_error_names_known_codes() {
    assert!(load_error(126).contains("ERROR_MOD_NOT_FOUND"));
    assert!(load_error(127).contains("ERROR_PROC_NOT_FOUND"));
    assert!(load_error(1114).contains("ERROR_DLL_INIT_FAILED"));
    assert_eq!(load_error(5), "5");
}

#[test]
fn clip_truncates_with_count() {
    let v: Vec<String> = (0..5).map(|i| i.to_string()).collect();
    assert_eq!(clip(&v, 10), "0, 1, 2, 3, 4");
    assert_eq!(clip(&v, 2), "0, 1, … (+3)");
}

#[test]
fn crates_discovers_compat_members_with_exports() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("..");
    let list = crates(&root);
    let x = list.iter().find(|c| c.dir == "xpal").unwrap();
    assert_eq!(x.lib, "xpal");
    assert!(x.cdylib && x.member);
    assert!(list.iter().all(|c| root.join("crates").join("compat").join(&c.dir).join("exports.txt").is_file()));
}

#[test]
fn probe_child_reports_symbols_and_missing() {
    let d = scratch("probe");
    let t = d.join("exports.txt");
    std::fs::write(&t, "1 GetFileVersionInfoW implemented | x\n2 NoSuchExportXyz implemented | x\n3 VerQueryValueW missing | x\n").unwrap();
    let me = std::env::current_exe().unwrap();
    let exe = me.ancestors().skip(1).map(|a| a.join("compat_smoke.exe")).find(|p| p.is_file() && *p != me).expect("compat_smoke.exe built beside the test binary");
    let p = run_probe_with(&exe, &sys("version.dll"), &t);
    assert_eq!(p.load, Some(Ok(())), "{:?} via {}", p.crash, exe.display());
    assert_eq!(p.covered, 2);
    assert_eq!(p.missing, vec!["NoSuchExportXyz".to_string()]);
    assert!(p.crash.is_none());
}
