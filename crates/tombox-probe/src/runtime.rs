use crate::console::ConsoleIndex;
use crate::evidence::{summarize, PcHit, Row};
use crate::image::{Entry, Image, ImportRow, RowKind};
use crate::names::{is_api_set, normalize_module, same_module};
use crate::sites;
use crate::win::{self, HModule};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn pc_hit_from_addr(addr: usize) -> PcHit {
    let owner = win::module_of(addr);
    let host_path = owner.and_then(win::module_path).map(|p| p.to_string_lossy().into_owned());
    let exp = owner.and_then(|h| win::export_at(h, addr));
    PcHit { resolved: true, host_path, ordinal: exp.as_ref().map(|e| e.ordinal), export_name: exp.and_then(|e| e.name), error: None }
}

pub fn pc_lookup(h: HModule, entry: &Entry) -> Result<usize, String> {
    match entry {
        Entry::Name(n) => win::proc_by_name(h, n),
        Entry::Ordinal(o) => win::proc_by_ordinal(h, *o),
    }
    .map_err(win::describe_error)
}

pub fn pc_resolve(cache: &mut HashMap<String, Result<usize, String>>, module: &str, entry: &Entry, flags: u32) -> PcHit {
    let key = normalize_module(module);
    let h = cache.entry(key.clone()).or_insert_with(|| win::load(module, flags).map(|h| h as usize).map_err(win::describe_error)).clone();
    match h {
        Err(e) => PcHit { error: Some(format!("load {module}: {e}")), ..Default::default() },
        Ok(h) => match pc_lookup(h as HModule, entry) {
            Ok(a) => pc_hit_from_addr(a),
            Err(e) => {
                let mut hit = PcHit { error: Some(e), ..Default::default() };
                hit.host_path = win::module_path(h as HModule).map(|p| p.to_string_lossy().into_owned());
                hit
            }
        },
    }
}

pub fn static_hit(img: &Image, entry: &Entry) -> PcHit {
    let ex = img.exports();
    let ordinal = match entry {
        Entry::Name(n) => ex.ordinal_of(n),
        Entry::Ordinal(o) => ex.by_ordinal.contains_key(&(*o as u32)).then_some(*o as u32),
    };
    let export_name = match entry {
        Entry::Name(n) => ordinal.map(|_| n.clone()),
        Entry::Ordinal(o) => ex.name_of(*o as u32).flatten(),
    };
    let forwarded = match entry {
        Entry::Name(n) => ex.forwarders.get(n).cloned(),
        _ => None,
    };
    PcHit {
        resolved: ordinal.is_some(),
        host_path: Some(img.path.to_string_lossy().into_owned()),
        ordinal,
        export_name: match (export_name, forwarded) {
            (Some(n), Some(f)) => Some(format!("{n} -> {f}")),
            (n, _) => n,
        },
        error: if ordinal.is_some() { None } else { Some("not in export table".into()) },
    }
}

pub fn file_index(dirs: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut m = HashMap::new();
    for d in dirs {
        let mut files = Vec::new();
        crate::token::walk(d, &mut files);
        for f in files {
            let ext = f.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if ext == "dll" || ext == "exe" {
                m.entry(normalize_module(&f.file_name().unwrap().to_string_lossy())).or_insert(f);
            }
        }
    }
    m
}

pub fn scan(title: &str, bin_dir: &Path, xvd: Option<&Path>, stage_dirs: &[PathBuf]) -> Value {
    win::quiet_errors();
    let mut console = ConsoleIndex::new(xvd);
    let title_files = file_index(&[bin_dir.to_path_buf()]);
    let stage_files = file_index(stage_dirs);
    let mut binaries: Vec<(String, PathBuf)> = Vec::new();
    let mut all = Vec::new();
    crate::token::walk(bin_dir, &mut all);
    for f in all {
        let ext = f.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ext == "dll" || ext == "exe" {
            let rel = f.strip_prefix(bin_dir).unwrap_or(&f).to_string_lossy().replace('\\', "/");
            binaries.push((rel, f));
        }
    }
    let mut images: HashMap<String, Option<Image>> = HashMap::new();
    let mut cache = HashMap::new();
    let mut rows = Vec::new();
    let mut skipped = Vec::new();
    for (rel, path) in &binaries {
        let img = match Image::open(path) {
            Ok(i) => i,
            Err(e) => {
                skipped.push(json!({"binary": rel, "error": e}));
                continue;
            }
        };
        for r in img.import_rows() {
            let key = normalize_module(&r.module);
            let (pc, check, note) = if let Some(p) = title_files.get(&key).filter(|_| !is_api_set(&key)) {
                let dep = images.entry(key.clone()).or_insert_with(|| Image::open(p).ok());
                match dep {
                    Some(d) => (static_hit(d, &r.entry), "title-shipped static export check", Some(format!("title ships {}", p.strip_prefix(bin_dir).unwrap_or(p).display()))),
                    None => (PcHit { error: Some("title module unparsable".into()), ..Default::default() }, "title-shipped static export check", None),
                }
            } else if let Some(p) = stage_files.get(&key) {
                let dep = images.entry(format!("stage:{key}")).or_insert_with(|| Image::open(p).ok());
                match dep {
                    Some(d) => (static_hit(d, &r.entry), "emulator-staged static export check", None),
                    None => (PcHit { error: Some("staged module unparsable".into()), ..Default::default() }, "emulator-staged static export check", None),
                }
            } else {
                (pc_resolve(&mut cache, &r.module, &r.entry, win::LOAD_LIBRARY_SEARCH_SYSTEM32), "host LoadLibraryExW(System32)+GetProcAddress", None)
            };
            let con = console.lookup(&r.module, &r.entry);
            rows.push(Row {
                title: title.into(),
                binary: rel.clone(),
                module: r.module.clone(),
                entry: r.entry.clone(),
                kind: r.kind.as_str().into(),
                check: check.into(),
                pc,
                console: con,
                iat_match: None,
                null_checked: None,
                note,
            });
        }
    }
    json!({
        "probe": "tombox-probe scan",
        "title": title,
        "bin_dir": bin_dir.to_string_lossy(),
        "console_system32": console.dir.as_ref().map(|d| d.to_string_lossy().into_owned()),
        "summary": summarize(&rows),
        "skipped": skipped,
        "rows": rows.iter().map(|r| r.to_json()).collect::<Vec<_>>(),
    })
}

pub struct ChildPlan {
    pub title: String,
    pub binary: String,
    pub target: PathBuf,
    pub scratch: PathBuf,
    pub xvd: Option<PathBuf>,
    pub expect: Vec<(String, ImportRow)>,
    pub pinvoke: Vec<(String, Vec<String>)>,
}

impl ChildPlan {
    pub fn to_json(&self) -> Value {
        json!({
            "title": self.title,
            "binary": self.binary,
            "target": self.target.to_string_lossy(),
            "scratch": self.scratch.to_string_lossy(),
            "xvd": self.xvd.as_ref().map(|x| x.to_string_lossy().into_owned()),
            "expect": self.expect.iter().map(|(b, r)| json!({"binary": b, "module": r.module, "function": r.entry.label(), "kind": r.kind.as_str()})).collect::<Vec<_>>(),
            "pinvoke": self.pinvoke.iter().map(|(b, n)| json!({"binary": b, "names": n})).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(v: &Value) -> Option<ChildPlan> {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
        let entry = |f: &str| match f.strip_prefix('#').and_then(|o| o.parse::<u16>().ok()) {
            Some(o) => Entry::Ordinal(o),
            None => Entry::Name(f.to_string()),
        };
        let expect = v
            .get("expect")?
            .as_array()?
            .iter()
            .filter_map(|e| {
                let kind = if e.get("kind")?.as_str()? == "delay" { RowKind::Delay } else { RowKind::Import };
                Some((e.get("binary")?.as_str()?.to_string(), ImportRow { module: e.get("module")?.as_str()?.to_string(), entry: entry(e.get("function")?.as_str()?), kind, desc_rva: 0, iat_rva: 0 }))
            })
            .collect();
        let pinvoke = v
            .get("pinvoke")?
            .as_array()?
            .iter()
            .filter_map(|e| Some((e.get("binary")?.as_str()?.to_string(), e.get("names")?.as_array()?.iter().filter_map(|n| n.as_str().map(|x| x.to_string())).collect())))
            .collect();
        Some(ChildPlan { title: s("title")?, binary: s("binary")?, target: PathBuf::from(s("target")?), scratch: PathBuf::from(s("scratch")?), xvd: s("xvd").map(PathBuf::from), expect, pinvoke })
    }
}

pub fn child(plan: &ChildPlan) -> Value {
    win::quiet_errors();
    let prefer = win::prefer_dir(&plan.scratch);
    let mut console = ConsoleIndex::new(plan.xvd.as_deref());
    let img = match Image::open(&plan.target) {
        Ok(i) => i,
        Err(e) => return json!({"loaded": false, "error": e}),
    };
    let mut preloaded = serde_json::Map::new();
    if let Ok(rd) = std::fs::read_dir(&plan.scratch) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().ends_with(".dll") {
                if let Some(h) = win::loaded(&name) {
                    preloaded.insert(normalize_module(&name), json!(win::module_path(h).map(|p| p.to_string_lossy().into_owned())));
                }
            }
        }
    }
    let load = win::load_path(&plan.target, win::LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | win::LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    let mut rows: Vec<Row> = Vec::new();
    let mut deps = serde_json::Map::new();
    let mut delay_addrs: HashMap<String, usize> = HashMap::new();
    let mk = |module: &str, entry: &Entry, kind: &str, check: &str, pc: PcHit, con, iat: Option<bool>, nc: Option<bool>, note: Option<String>, binary: &str| Row {
        title: plan.title.clone(),
        binary: binary.to_string(),
        module: module.to_string(),
        entry: entry.clone(),
        kind: kind.to_string(),
        check: check.to_string(),
        pc,
        console: con,
        iat_match: iat,
        null_checked: nc,
        note,
    };
    let base = match load {
        Ok(h) => h as usize,
        Err(code) => {
            let err = win::describe_error(code);
            for r in img.import_rows() {
                let con = console.lookup(&r.module, &r.entry);
                rows.push(mk(&r.module, &r.entry, r.kind.as_str(), "image load failed", PcHit { error: Some(err.clone()), ..Default::default() }, con, None, None, None, &plan.binary));
            }
            return json!({"loaded": false, "error": err, "summary": summarize(&rows), "rows": rows.iter().map(|r| r.to_json()).collect::<Vec<_>>()});
        }
    };
    let mut cache: HashMap<String, Result<usize, String>> = HashMap::new();
    for r in img.import_rows() {
        let con = console.lookup(&r.module, &r.entry);
        match r.kind {
            RowKind::Import => {
                let iat = unsafe { win::read_u64(base + r.iat_rva as usize) } as usize;
                let pc = pc_resolve(&mut cache, &r.module, &r.entry, 0);
                let addr_ok = pc.resolved;
                let indep = if addr_ok { pc_hit_addr(&cache, &r.module, &r.entry) } else { None };
                let iat_match = indep.map(|a| a == iat);
                deps.entry(normalize_module(&r.module)).or_insert_with(|| json!(pc.host_path));
                rows.push(mk(&r.module, &r.entry, "import", "loader-bound IAT slot vs LoadLibraryExW+GetProcAddress", pc, con, iat_match, None, None, &plan.binary));
            }
            RowKind::Delay => {
                let res = win::resolve_delay(base, r.desc_rva, r.iat_rva);
                let (pc, iat_match) = match res {
                    Ok(a) => {
                        if let Entry::Name(n) = &r.entry {
                            delay_addrs.insert(n.clone(), a);
                        }
                        let indep = win::loaded(&r.module).and_then(|h| pc_lookup(h, &r.entry).ok());
                        let slot = unsafe { win::read_u64(base + r.iat_rva as usize) } as usize;
                        (pc_hit_from_addr(a), Some(indep == Some(a) && slot == a))
                    }
                    Err(e) => (PcHit { error: Some(format!("ResolveDelayLoadedAPI: {e}")), ..Default::default() }, None),
                };
                deps.entry(normalize_module(&r.module)).or_insert_with(|| json!(win::loaded(&r.module).and_then(win::module_path).map(|p| p.to_string_lossy().into_owned())));
                rows.push(mk(&r.module, &r.entry, "delay", "ResolveDelayLoadedAPI on the image's own delay descriptor, patched IAT slot vs GetProcAddress", pc, con, iat_match, None, None, &plan.binary));
            }
        }
    }
    let own = img.exports();
    for (binary, r) in &plan.expect {
        let pc = match pc_lookup(base as HModule, &r.entry) {
            Ok(a) => pc_hit_from_addr(a),
            Err(e) => PcHit { error: Some(e), ..Default::default() },
        };
        let con = crate::console::ConsoleHit {
            module_present: false,
            host: own.has(&r.entry).then(|| normalize_module(&plan.binary)),
            ordinal: match &r.entry {
                Entry::Name(n) => own.ordinal_of(n),
                Entry::Ordinal(o) => Some(*o as u32),
            },
            name_at_ordinal: match &r.entry {
                Entry::Name(n) => Some(n.clone()),
                Entry::Ordinal(o) => own.name_of(*o as u32).flatten(),
            },
            other_hosts: 0,
        };
        rows.push(mk(&r.module, &r.entry, r.kind.as_str(), "importer row: GetProcAddress on the loaded title module", pc, con, None, None, Some("console column is the title module's own export table (same file on console)".into()), binary));
    }
    let mut site_json = Vec::new();
    for s in sites::analyze(&img) {
        site_json.push(json!({
            "dll": s.dll, "wide": s.wide, "string_rva": format!("{:#x}", s.string_rva), "lea_rva": format!("{:#x}", s.lea_rva),
            "loader": s.loader, "loader_call_rva": s.loader_call_rva.map(|r| format!("{r:#x}")), "load_failure_checked": s.null_checked,
            "check_branch_target_rva": s.failure_branch_rva.map(|r| format!("{r:#x}")),
            "procs": s.procs.iter().map(|p| json!({"name": p.name, "call_rva": format!("{:#x}", p.call_rva), "null_checked": p.null_checked})).collect::<Vec<_>>(),
            "listing": sites::listing(&img, s.lea_rva, 16),
        }));
        for p in &s.procs {
            let Some(name) = &p.name else { continue };
            let entry = Entry::Name(name.clone());
            let pc = pc_resolve(&mut cache, &s.dll, &entry, 0);
            let con = console.lookup(&s.dll, &entry);
            let note = Some(format!("{} at {:#x}; module load result {}checked", s.loader.clone().unwrap_or_default(), s.loader_call_rva.unwrap_or(0), if s.null_checked { "" } else { "not " }));
            rows.push(mk(&s.dll, &entry, "dynamic", "code-site GetProcAddress name, resolved on PC with LoadLibraryExW+GetProcAddress", pc, con, None, Some(p.null_checked), note, &plan.binary));
        }
    }
    for (binary, names) in &plan.pinvoke {
        for n in names {
            let entry = Entry::Name(n.clone());
            let pc = match pc_lookup(base as HModule, &entry) {
                Ok(a) => pc_hit_from_addr(a),
                Err(e) => PcHit { error: Some(e), ..Default::default() },
            };
            let con = crate::console::ConsoleHit { module_present: false, host: own.has(&entry).then(|| normalize_module(&plan.binary)), ordinal: own.ordinal_of(n), name_at_ordinal: Some(n.clone()), other_hosts: 0 };
            rows.push(mk(&plan.binary, &entry, "pinvoke", "GetProcAddress on the loaded title module for a P/Invoke entry point named in metadata", pc, con, None, None, None, binary));
        }
    }
    let concrt = if crate::concrt::ALL.iter().all(|n| delay_addrs.contains_key(*n)) { Some(crate::concrt::selftest(&delay_addrs)) } else { None };
    let mut loaded_paths = serde_json::Map::new();
    for (k, v) in deps {
        loaded_paths.insert(k, v);
    }
    json!({
        "loaded": true,
        "module_path": win::module_path(base as HModule).map(|p| p.to_string_lossy().into_owned()),
        "default_dll_dirs_set": prefer,
        "preloaded_by_probe_host": Value::Object(preloaded),
        "dependencies": Value::Object(loaded_paths),
        "summary": summarize(&rows),
        "dynamic_load_sites": site_json,
        "concrt_selftest": concrt,
        "rows": rows.iter().map(|r| r.to_json()).collect::<Vec<_>>(),
    })
}

fn pc_hit_addr(cache: &HashMap<String, Result<usize, String>>, module: &str, entry: &Entry) -> Option<usize> {
    let h = cache.get(&normalize_module(module))?.clone().ok()?;
    pc_lookup(h as HModule, entry).ok()
}

pub fn expected_rows(importer: &Path, target_name: &str) -> Result<Vec<ImportRow>, String> {
    let img = Image::open(importer)?;
    Ok(img.import_rows().into_iter().filter(|r| same_module(&r.module, target_name)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_resolution_records_owner_and_ordinal() {
        let mut cache = HashMap::new();
        let hit = pc_resolve(&mut cache, "api-ms-win-core-sysinfo-l1-1-0.dll", &Entry::Name("GetTickCount64".into()), win::LOAD_LIBRARY_SEARCH_SYSTEM32);
        assert!(hit.resolved, "{hit:?}");
        assert!(hit.ordinal.is_some());
        assert_eq!(hit.export_name.as_deref(), Some("GetTickCount64"));
        let miss = pc_resolve(&mut cache, "kernel32.dll", &Entry::Name("TomboxMissing".into()), win::LOAD_LIBRARY_SEARCH_SYSTEM32);
        assert!(!miss.resolved);
        assert_eq!(miss.error.as_deref(), Some("ERROR_PROC_NOT_FOUND"));
        let nomod = pc_resolve(&mut cache, "tombox-absent.dll", &Entry::Name("X".into()), win::LOAD_LIBRARY_SEARCH_SYSTEM32);
        assert!(nomod.error.unwrap().contains("ERROR_MOD_NOT_FOUND"));
    }

    #[test]
    fn plan_round_trips() {
        let plan = ChildPlan {
            title: "T".into(),
            binary: "GameChat2.dll".into(),
            target: PathBuf::from(r"C:\s\GameChat2.dll"),
            scratch: PathBuf::from(r"C:\s"),
            xvd: None,
            expect: vec![("a.exe".into(), ImportRow { module: "GameChat2.dll".into(), entry: Entry::Name("ChatManagerInitialize".into()), kind: RowKind::Delay, desc_rva: 0, iat_rva: 0 })],
            pinvoke: vec![("GameAssembly.dll".into(), vec!["PartyXblInitialize".into()])],
        };
        let back = ChildPlan::from_json(&plan.to_json()).unwrap();
        assert_eq!(back.binary, "GameChat2.dll");
        assert_eq!(back.expect[0].1.entry, Entry::Name("ChatManagerInitialize".into()));
        assert_eq!(back.expect[0].1.kind, RowKind::Delay);
        assert_eq!(back.pinvoke[0].1, vec!["PartyXblInitialize".to_string()]);
        assert!(back.xvd.is_none());
    }

    #[test]
    fn child_probe_on_staged_system_dll() {
        let src = PathBuf::from(r"C:\Windows\System32\ws2_32.dll");
        if !src.is_file() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tombox-probe-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("ws2_copy.dll");
        std::fs::copy(&src, &target).unwrap();
        let plan = ChildPlan { title: "host".into(), binary: "ws2_copy.dll".into(), target: target.clone(), scratch: dir.clone(), xvd: None, expect: vec![], pinvoke: vec![("x".into(), vec!["htonl".into()])] };
        let v = child(&plan);
        assert_eq!(v["loaded"], true, "{v}");
        let s = &v["summary"];
        assert!(s["rows"].as_u64().unwrap() > 10);
        assert_eq!(s["unresolved"], 0, "{s}");
        let rows = v["rows"].as_array().unwrap();
        let imports: Vec<_> = rows.iter().filter(|r| r["kind"] == "import").collect();
        assert!(!imports.is_empty());
        assert!(imports.iter().filter(|r| r["iat_match"] == true).count() * 10 >= imports.len() * 9, "{s}");
        assert!(rows.iter().any(|r| r["kind"] == "pinvoke" && r["function"] == "htonl" && r["resolved"] == true));
    }
}
