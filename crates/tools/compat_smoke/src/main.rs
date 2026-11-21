mod pe;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::io::Read;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const COVERED: &[&str] = &["implemented", "forwarded", "hooked"];
const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 8;
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
    fn SetErrorMode(m: u32) -> u32;
    fn SetDllDirectoryW(p: *const u16) -> i32;
    fn GetModuleFileNameW(m: *mut c_void, buf: *mut u16, n: u32) -> u32;
}

fn module_path(h: *mut c_void) -> PathBuf {
    let mut b = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(h, b.as_mut_ptr(), b.len() as u32) } as usize;
    PathBuf::from(String::from_utf16_lossy(&b[..n]))
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()),
    }
}

fn load_exact(dll: &Path) -> Result<(*mut c_void, Option<PathBuf>), u32> {
    let h = load(dll, LOAD_WITH_ALTERED_SEARCH_PATH)?;
    if same_file(&module_path(h), dll) {
        return Ok((h, None));
    }
    let dir = std::env::temp_dir().join("compat_smoke").join(std::process::id().to_string());
    let _ = std::fs::create_dir_all(&dir);
    let alias = dir.join(format!("smoke_{}", dll.file_name().unwrap().to_string_lossy()));
    std::fs::copy(dll, &alias).map_err(|_| 0u32)?;
    let h = load(&alias, LOAD_WITH_ALTERED_SEARCH_PATH)?;
    Ok((h, Some(module_path(h))))
}

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

fn load(p: &Path, flags: u32) -> Result<*mut c_void, u32> {
    let w = wide(p.as_os_str());
    let h = unsafe { LoadLibraryExW(w.as_ptr(), core::ptr::null_mut(), flags) };
    if h.is_null() {
        Err(unsafe { GetLastError() })
    } else {
        Ok(h)
    }
}

fn resolve(h: *mut c_void, name: &str) -> bool {
    let p = if let Some(o) = name.strip_prefix('#').and_then(|o| o.parse::<usize>().ok()) {
        unsafe { GetProcAddress(h, o as *const u8) }
    } else {
        let z: Vec<u8> = name.bytes().chain(Some(0)).collect();
        unsafe { GetProcAddress(h, z.as_ptr()) }
    };
    !p.is_null()
}

pub struct Row {
    pub name: String,
    pub status: String,
}

fn rows(table: &Path) -> Vec<Row> {
    std::fs::read_to_string(table)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let body = l.trim().split(" | ").next()?;
            let f: Vec<&str> = body.split_whitespace().collect();
            (f.len() >= 3).then(|| Row { name: f[1].to_string(), status: f[2].to_string() })
        })
        .collect()
}

fn check_imports(img: &pe::Pe, delay: bool) {
    check_imports_tagged(img, delay, if delay { "DELAY" } else { "IMP" }, "");
}

fn check_imports_tagged(img: &pe::Pe, delay: bool, tag: &str, prefix: &str) {
    for imp in img.imports().into_iter().filter(|i| i.delay == delay) {
        match load(Path::new(&imp.dll), 0) {
            Err(e) => println!("{tag}DLL\t{prefix}{}\t{e}", imp.dll),
            Ok(h) => {
                let bound = module_path(h);
                for f in &imp.names {
                    if !resolve(h, f) {
                        println!("{tag}FN\t{prefix}{} (bound to {})\t{f}", imp.dll, bound.display());
                    }
                }
            }
        }
    }
}

fn diagnose_target(dir: &Path, t: &str, seen: &mut Vec<String>) {
    if seen.iter().any(|s| s.eq_ignore_ascii_case(t)) {
        return;
    }
    seen.push(t.to_string());
    let file = dir.join(format!("{t}.dll"));
    if let Some(img) = std::fs::read(&file).ok().and_then(pe::Pe::parse) {
        check_imports_tagged(&img, false, "TGT", &format!("{t}.dll imports "));
    }
}

fn stage_sys(img: &pe::Pe) {
    let mut done = Vec::new();
    for e in img.exports() {
        let Some((t, _)) = e.forwarder.as_deref().and_then(|f| f.rsplit_once('.')) else { continue };
        let Some(base) = t.strip_suffix("_sys") else { continue };
        if done.iter().any(|d: &String| d.eq_ignore_ascii_case(t)) {
            continue;
        }
        done.push(t.to_string());
        if load(Path::new(&format!("{t}.dll")), 0).is_ok() {
            continue;
        }
        let src = PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join("System32").join(format!("{base}.dll"));
        let dir = std::env::temp_dir().join("compat_smoke").join(std::process::id().to_string());
        let _ = std::fs::create_dir_all(&dir);
        let dst = dir.join(format!("{t}.dll"));
        let r = std::fs::copy(&src, &dst).map_err(|_| 2u32).and_then(|_| load(&dst, LOAD_WITH_ALTERED_SEARCH_PATH));
        match r {
            Ok(_) => println!("STAGED\t{t}.dll\tok"),
            Err(e) => println!("STAGED\t{t}.dll\terror {e}"),
        }
    }
}

fn probe(dll: &Path, table: &Path) -> i32 {
    unsafe { SetErrorMode(0x8007) };
    if let Some(d) = dll.parent() {
        let w = wide(d.as_os_str());
        unsafe { SetDllDirectoryW(w.as_ptr()) };
    }
    let img = std::fs::read(dll).ok().and_then(pe::Pe::parse);
    match load_exact(dll) {
        Err(e) => {
            println!("LOAD\tfail\t{e}");
            if let Some(img) = &img {
                check_imports(img, false);
            }
        }
        Ok((h, alias)) => {
            println!("LOAD\tok");
            if let Some(a) = alias {
                println!("ALIAS\t{}", a.display());
            }
            if let Some(img) = &img {
                stage_sys(img);
            }
            for r in rows(table).iter().filter(|r| COVERED.contains(&r.status.as_str())) {
                println!("SYM\t{}\t{}\t{}", r.name, r.status, if resolve(h, &r.name) { "ok" } else { "missing" });
            }
            if let Some(img) = &img {
                let mut seen = Vec::new();
                for e in img.exports() {
                    if let Some(f) = &e.forwarder {
                        let key = e.name.clone().unwrap_or_else(|| format!("#{}", e.ordinal));
                        let target = match f.rsplit_once('.') {
                            None => "malformed".to_string(),
                            Some((t, _)) => match load(Path::new(&format!("{t}.dll")), 0) {
                                Ok(_) => "target-ok".to_string(),
                                Err(e) => {
                                    if let Some(d) = dll.parent() {
                                        diagnose_target(d, t, &mut seen);
                                    }
                                    format!("target {t}.dll error {e}")
                                }
                            },
                        };
                        println!("FWD\t{key}\t{f}\t{}\t{target}", if resolve(h, &key) { "ok" } else { "fail" });
                    }
                }
                check_imports(img, true);
            }
        }
    }
    0
}

struct Crate {
    dir: String,
    package: String,
    lib: String,
    cdylib: bool,
    member: bool,
}

fn field(toml: &str, section: &str, key: &str) -> Option<String> {
    let s = toml.find(&format!("[{section}]"))?;
    let rest = &toml[s + section.len() + 2..];
    let end = rest.find("\n[").unwrap_or(rest.len());
    rest[..end].lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

fn crates(root: &Path) -> Vec<Crate> {
    let ws = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir(root.join("crates").join("compat")) else { return v };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("exports.txt").is_file() && p.join("Cargo.toml").is_file()).collect();
    dirs.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()));
    for d in dirs {
        let t = std::fs::read_to_string(d.join("Cargo.toml")).unwrap_or_default();
        let dir = d.file_name().unwrap().to_string_lossy().into_owned();
        let package = field(&t, "package", "name").unwrap_or_else(|| dir.clone());
        let lib = field(&t, "lib", "name").unwrap_or_else(|| package.replace('-', "_"));
        let cdylib = field(&t, "lib", "crate-type").map(|c| c.contains("cdylib")).unwrap_or(false);
        let member = ws.contains(&format!("\"crates/compat/{dir}\""));
        v.push(Crate { dir, package, lib, cdylib, member });
    }
    v
}

fn cargo_build(root: &Path, target: &Path, pkgs: &[&str]) -> Result<(), String> {
    let mut r = cargo_build_once(root, target, pkgs);
    for _ in 0..6 {
        match &r {
            Err(e) if e.contains("failed to load manifest") || e.contains("failed to parse manifest") => {
                eprintln!("workspace manifest is being edited by another agent, retrying in 20s");
                std::thread::sleep(Duration::from_secs(20));
                r = cargo_build_once(root, target, pkgs);
            }
            _ => break,
        }
    }
    r
}

fn cargo_build_once(root: &Path, target: &Path, pkgs: &[&str]) -> Result<(), String> {
    let mut c = Command::new("cargo");
    c.current_dir(root).env("CARGO_TARGET_DIR", target).args(["build", "--offline"]);
    for p in pkgs {
        c.args(["-p", p]);
    }
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = err.lines().filter(|l| l.starts_with("error")).take(3).collect();
    Err(if lines.is_empty() { err.lines().last().unwrap_or("build failed").to_string() } else { lines.join(" / ") })
}

#[derive(Default)]
struct Probe {
    load: Option<Result<(), u32>>,
    crash: Option<String>,
    alias: Option<String>,
    staged: Vec<String>,
    tgt: Vec<String>,
    covered: usize,
    missing: Vec<String>,
    fwd_total: usize,
    fwd_fail: Vec<String>,
    imp: Vec<String>,
    delay: Vec<String>,
}

fn run_probe(dll: &Path, table: &Path) -> Probe {
    run_probe_with(&std::env::current_exe().unwrap(), dll, table)
}

fn run_probe_with(exe: &Path, dll: &Path, table: &Path) -> Probe {
    let mut p = Probe::default();
    let log = std::env::temp_dir().join("compat_smoke_child.log");
    let mut child = match Command::new(exe)
        .arg("--probe")
        .arg(dll)
        .arg(table)
        .env("TOMBOX_LOG", &log)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            p.crash = Some(format!("spawn failed: {e}"));
            return p;
        }
    };
    let mut so = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() > PROBE_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };
    let text = reader.join().unwrap_or_default();
    match status {
        None => p.crash = Some(format!("hung > {}s", PROBE_TIMEOUT.as_secs())),
        Some(s) if !s.success() => p.crash = Some(format!("child exit {:#010x}", s.code().unwrap_or(-1) as u32)),
        _ => {}
    }
    for l in text.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        match f.as_slice() {
            ["LOAD", "ok"] => p.load = Some(Ok(())),
            ["LOAD", "fail", e] => p.load = Some(Err(e.parse().unwrap_or(0))),
            ["SYM", n, _, r] => {
                p.covered += 1;
                if *r != "ok" {
                    p.missing.push(n.to_string());
                }
            }
            ["FWD", n, t, r, tgt] => {
                p.fwd_total += 1;
                if *r != "ok" {
                    p.fwd_fail.push(if *tgt == "target-ok" { format!("{n} -> {t} (target loads, symbol missing)") } else { format!("{n} -> {t} ({tgt})") });
                }
            }
            ["ALIAS", a] => p.alias = Some(a.to_string()),
            ["TGTDLL", d, e] => p.tgt.push(format!("{d} (error {e})")),
            ["TGTFN", d, n] => p.tgt.push(format!("{d}!{n}")),
            ["STAGED", d, r] => p.staged.push(format!("{d} ({r})")),
            ["IMPDLL", d, e] => p.imp.push(format!("{d} (error {e})")),
            ["IMPFN", d, n] => p.imp.push(format!("{d}!{n}")),
            ["DELAYDLL", d, e] => p.delay.push(format!("{d} (error {e})")),
            ["DELAYFN", d, n] => p.delay.push(format!("{d}!{n}")),
            _ => {}
        }
    }
    p
}

fn load_error(e: u32) -> String {
    match e {
        126 => "126 ERROR_MOD_NOT_FOUND (an import DLL is missing)".into(),
        127 => "127 ERROR_PROC_NOT_FOUND (an imported function is missing)".into(),
        193 => "193 ERROR_BAD_EXE_FORMAT".into(),
        1114 => "1114 ERROR_DLL_INIT_FAILED (DllMain/TLS returned FALSE)".into(),
        x => format!("{x}"),
    }
}

fn clip(v: &[String], n: usize) -> String {
    let mut s = v.iter().take(n).cloned().collect::<Vec<_>>().join(", ");
    if v.len() > n {
        s.push_str(&format!(", … (+{})", v.len() - n));
    }
    s
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 4 && args[1] == "--probe" {
        std::process::exit(probe(Path::new(&args[2]), Path::new(&args[3])));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("..").canonicalize().map(|p| PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\").to_string())).unwrap();
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    let no_build = args.iter().any(|a| a == "--no-build");
    let only: Option<Vec<String>> = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).map(|s| s.split(',').map(|x| x.to_string()).collect());
    let report = args.iter().position(|a| a == "--report").and_then(|i| args.get(i + 1)).map(PathBuf::from).unwrap_or_else(|| root.join("tools").join("dev").join("smoke_report.md"));
    let mut list = crates(&root);
    if let Some(o) = &only {
        list.retain(|c| o.contains(&c.dir));
    }
    let mut build: BTreeMap<String, Result<(), String>> = BTreeMap::new();
    let buildable: Vec<&Crate> = list.iter().filter(|c| c.member && c.cdylib).collect();
    if !no_build {
        let pk: Vec<&str> = buildable.iter().map(|c| c.package.as_str()).collect();
        eprintln!("building {} packages", pk.len());
        if cargo_build(&root, &target, &pk).is_ok() {
            for c in &buildable {
                build.insert(c.dir.clone(), Ok(()));
            }
        } else {
            for c in &buildable {
                eprintln!("building {}", c.package);
                build.insert(c.dir.clone(), cargo_build(&root, &target, &[c.package.as_str()]));
            }
        }
    }
    let mut md = String::new();
    md.push_str("# Compat DLL smoke report\n\n");
    md.push_str(&format!("Generated by `crates/tools/compat_smoke` (debug build, target `{}`). Each DLL is loaded with `LoadLibraryExW` in its own child process; every exports.txt row marked implemented/forwarded/hooked is resolved with `GetProcAddress` (name or `#ordinal`), every PE forwarder is resolved, and on load failure each static import is bound individually. Forwarder targets are loaded by name with the DLL's own directory on the search path. \"ok (alias copy)\" means a module with the same base name (a system DLL such as kernel32) was already mapped, so the probe loaded a renamed copy to test this file and not the system one.\n\n", target.display()));
    md.push_str("| crate | DLL | build | load | covered rows resolved | forwarders | problems |\n|---|---|---|---|---|---|---|\n");
    let mut details = String::new();
    let (mut ok_dlls, mut bad_dlls) = (0, 0);
    for c in &list {
        let dll = target.join("debug").join(format!("{}.dll", c.lib));
        let mut problems = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        let b = if !c.member {
            "not a workspace member".to_string()
        } else if !c.cdylib {
            "no cdylib".to_string()
        } else {
            match build.get(&c.dir) {
                Some(Ok(())) => "ok".into(),
                Some(Err(e)) => {
                    problems.push(format!("build: {e}"));
                    "FAIL".into()
                }
                None => "skipped".into(),
            }
        };
        let (load, res, fwd) = if !dll.is_file() || (b != "ok" && b != "skipped") {
            ("-".to_string(), "-".to_string(), "-".to_string())
        } else {
            eprintln!("probing {}", dll.display());
            let p = run_probe(&dll, &root.join("crates").join("compat").join(&c.dir).join("exports.txt"));
            let load = match &p.load {
                Some(Ok(())) if p.alias.is_some() => "ok (alias copy)".to_string(),
                Some(Ok(())) => "ok".to_string(),
                Some(Err(e)) => {
                    problems.push(format!("LoadLibraryEx error {}", load_error(*e)));
                    "FAIL".into()
                }
                None => "FAIL".into(),
            };
            if let Some(cr) = &p.crash {
                problems.push(format!("probe {cr} (crash/hang during load: DllMain, TLS or CRT initializers)"));
            }
            if !p.imp.is_empty() {
                problems.push(format!("imports that do not bind on this PC: {}", clip(&p.imp, 12)));
            }
            if !p.missing.is_empty() {
                problems.push(format!("{} covered rows not exported", p.missing.len()));
                details.push_str(&format!("### {} ({}.dll): covered rows that do not resolve\n\n{}\n\n", c.dir, c.lib, p.missing.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ")));
            }
            if !p.fwd_fail.is_empty() {
                problems.push(format!("{} forwarders do not resolve", p.fwd_fail.len()));
                details.push_str(&format!("### {} ({}.dll): forwarders that do not resolve\n\n{}\n\n", c.dir, c.lib, p.fwd_fail.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ")));
            }
            if !p.delay.is_empty() {
                problems.push(format!("delay imports unavailable: {}", clip(&p.delay, 8)));
            }
            if !p.tgt.is_empty() {
                problems.push(format!("{} forwarder-target imports do not bind in this process", p.tgt.len()));
                details.push_str(&format!("### {} ({}.dll): why the forwarder targets fail to load here\n\n{}\n\n", c.dir, c.lib, p.tgt.iter().map(|m| format!("- `{m}`")).collect::<Vec<_>>().join("\n")));
            }
            for s in &p.staged {
                if s.ends_with("(ok)") {
                    notes.push(format!("forwarder target {} staged by the probe from System32, as stage_compat.ps1 and xhost stage every forwarded *_sys.dll", s.trim_end_matches(" (ok)")));
                } else {
                    problems.push(format!("forwarder target {s} could not be staged from System32"));
                }
            }
            (load, format!("{}/{}", p.covered - p.missing.len(), p.covered), if p.fwd_total == 0 { "-".into() } else { format!("{}/{}", p.fwd_total - p.fwd_fail.len(), p.fwd_total) })
        };
        if !dll.is_file() && c.member && c.cdylib {
            problems.push(format!("{} not found after build", dll.display()));
        }
        if problems.is_empty() {
            ok_dlls += 1;
        } else {
            bad_dlls += 1;
        }
        md.push_str(&format!("| {} | {}.dll | {} | {} | {} | {} | {} |\n", c.dir, c.lib, b, load, res, fwd, {
            let mut all = if problems.is_empty() { vec!["none".to_string()] } else { problems.clone() };
            all.extend(notes.iter().map(|n| format!("note: {n}")));
            all.join("; ").replace('|', "\\|")
        }));
    }
    md.push_str(&format!("\n{} crates checked: {} clean, {} with problems.\n\n", list.len(), ok_dlls, bad_dlls));
    if !details.is_empty() {
        md.push_str("## Details\n\n");
        md.push_str(&details);
    }
    if let Some(d) = report.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    std::fs::write(&report, md).unwrap();
    eprintln!("wrote {} ({} clean, {} with problems)", report.display(), ok_dlls, bad_dlls);
}
