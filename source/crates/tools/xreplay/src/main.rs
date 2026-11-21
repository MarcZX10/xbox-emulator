mod d3d;
mod cap;
mod engine;
mod stats;
#[path = "../../../compat/d3d12_xs/src/pso_serial.rs"]
#[allow(dead_code)]
mod pso_serial;

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage:
  xreplay <capture dir> [--list] [--isolate] [--stats <hex hashes|all>] [--range a-b] [--bmp] [--final]
          [--retranslate] [--hlsl-override dir] [--pso-patch <hash|all>:<field>=<value>]... [--no-predication]
          [--debug] [--out dir] [--verbose]
  xreplay shader <file.dxbc | hash> [--dir shaders_dir] [--out dir] [--rtfmt 0,1,..]

capture dir: %TEMP%\\tombox_replay\\<n> written by d3d12_xs with TOMBOX_RECORD=1.
--isolate     replay every snapshotted call (record_filter) alone from its draw-time inputs, compare to its recorded outputs
--stats       read back the written targets after matching calls (full-frame mode) and print inf/NaN stats
fields for --pso-patch: depthenable depthwrite depthfunc stencilenable fill cull frontccw depthbias depthclip rtcount samplemask or 0x<offset>";

fn arg(a: &[String], k: &str) -> Option<String> {
    a.iter().position(|x| x == k).and_then(|i| a.get(i + 1).cloned())
}

fn hexes(s: &str) -> Vec<u64> {
    s.split(|c: char| c == ',' || c.is_whitespace()).filter_map(|w| u64::from_str_radix(w.trim().trim_start_matches("0x"), 16).ok()).collect()
}

fn shader_cmd(a: &[String]) -> ExitCode {
    let Some(src) = a.get(2) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let path = if std::path::Path::new(src).exists() {
        PathBuf::from(src)
    } else {
        let dir = arg(a, "--dir").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"D:\xbox-pc-port\emulator\runs\run33\capture\shaders"));
        dir.join(format!("{}.dxbc", src.trim_start_matches("0x")))
    };
    let out = arg(a, "--out").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("xreplay_shader"));
    let _ = std::fs::create_dir_all(&out);
    let blob = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return ExitCode::from(2);
        }
    };
    let stem = path.file_stem().unwrap().to_string_lossy().to_string();
    let t0 = std::time::Instant::now();
    let ex = match tombox_xshader::extract(&blob) {
        Ok(e) => e,
        Err(e) => {
            println!("FAIL extract: {e}");
            return ExitCode::from(1);
        }
    };
    let mut t = match tombox_xshader::hlsl::translate(&ex) {
        Ok(t) => t,
        Err(e) => {
            println!("FAIL translate: {e}");
            return ExitCode::from(1);
        }
    };
    let ms_t = t0.elapsed().as_secs_f64() * 1000.0;
    if let Some(r) = arg(a, "--rtfmt") {
        let defs: String = r.split(',').enumerate().filter(|(_, k)| *k != "0").map(|(i, k)| format!("#define TB_RTFMT{i} {k}u\n")).collect();
        t.hlsl = defs + &t.hlsl;
    }
    let hp = out.join(format!("{stem}.hlsl"));
    let _ = std::fs::write(&hp, &t.hlsl);
    let profile = tombox_xshader::dxc::profile_for(t.profile);
    let t1 = std::time::Instant::now();
    let r = tombox_xshader::dxc::compile(&t.hlsl, &profile);
    let ms_c = t1.elapsed().as_secs_f64() * 1000.0;
    let regs: Vec<&str> = t.hlsl.lines().filter(|l| l.contains("register(")).collect();
    println!("{stem}: {} bytes in, profile {profile}, {} HLSL lines (translate {ms_t:.1} ms) -> {}", blob.len(), t.hlsl.lines().count(), hp.display());
    println!("bindings ({}):", regs.len());
    for l in regs.iter().take(64) {
        println!("  {}", l.trim());
    }
    match r {
        Ok(v) => {
            let cp = out.join(format!("{stem}.dxil"));
            let _ = std::fs::write(&cp, &v);
            println!("PASS compile {} bytes DXIL in {ms_c:.0} ms -> {}", v.len(), cp.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("FAIL compile ({ms_c:.0} ms): {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 2 || a.iter().any(|x| x == "-h" || x == "--help") {
        println!("{USAGE}");
        return ExitCode::from(2);
    }
    if a[1] == "shader" {
        return shader_cmd(&a);
    }
    std::env::remove_var("TOMBOX_RECORD");
    std::env::remove_var("TOMBOX_RTCAP");
    let dir = PathBuf::from(&a[1]);
    let cap = match cap::Capture::load(&dir) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot load capture: {e}");
            return ExitCode::from(2);
        }
    };
    if a.iter().any(|x| x == "--list") {
        println!("{} executes, {} psos, {} root sigs, {} heaps, {} views, {} buffers, {} resources", cap.executes.len(), cap.psos.len(), cap.roots.len(), cap.heaps.len(), cap.views.len(), cap.buffers.len(), cap.resources.len());
        for ex in &cap.executes {
            println!("execute #{} list={:#x} lines={} complete={}", ex.n, ex.list, ex.lines.len(), ex.complete);
            let mut pso = 0u64;
            for l in &ex.lines {
                let w: Vec<&str> = l.text.split_whitespace().collect();
                match w.first() {
                    Some(&"pso") => pso = cap::num(w[1]),
                    Some(&"reset") => pso = cap::kv(&w, "pso").map(cap::num).unwrap_or(0),
                    Some(&"draw") | Some(&"drawidx") | Some(&"dispatch") | Some(&"indirect") => {
                        let hs: Vec<String> = cap.psos.get(&pso).map(|p| p.stages.iter().map(|(s, h)| format!("{s}={}", h.map(|h| format!("{h:016x}")).unwrap_or("?".into()))).collect()).unwrap_or_default();
                        println!("  {:6} {}  {}", l.index, l.text, hs.join(" "));
                    }
                    Some(&"snapcall") | Some(&"beginquery") | Some(&"endquery") | Some(&"resolvequery") | Some(&"predication") => println!("  {:6} {}", l.index, l.text),
                    _ => {}
                }
            }
        }
        for n in &cap.notes {
            println!("note: {n}");
        }
        return ExitCode::SUCCESS;
    }
    let out = arg(&a, "--out").map(PathBuf::from).unwrap_or_else(|| dir.join("xreplay"));
    let _ = std::fs::create_dir_all(&out);
    let stats_arg = arg(&a, "--stats");
    let mut patches = Vec::new();
    for (i, x) in a.iter().enumerate() {
        if x == "--pso-patch" {
            if let Some(p) = a.get(i + 1) {
                let (h, rest) = p.split_once(':').unwrap_or(("all", p));
                let (f, v) = rest.split_once('=').unwrap_or((rest, "0"));
                let Some(off) = engine::pso_field(f) else {
                    eprintln!("unknown --pso-patch field {f}");
                    return ExitCode::from(2);
                };
                let v = if let Some(x) = v.strip_prefix("0x") { u32::from_str_radix(x, 16).unwrap_or(0) } else { v.parse::<i64>().unwrap_or(0) as u32 };
                patches.push((if h == "all" { None } else { u64::from_str_radix(h, 16).ok() }, off, v));
            }
        }
    }
    let opts = engine::Opts {
        stats: stats_arg.as_deref().filter(|s| *s != "all").map(hexes).unwrap_or_default(),
        stats_all: stats_arg.as_deref() == Some("all"),
        range: arg(&a, "--range").and_then(|r| r.split_once('-').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)))),
        bmp: a.iter().any(|x| x == "--bmp"),
        out: out.clone(),
        retranslate: a.iter().any(|x| x == "--retranslate"),
        hlsl_override: arg(&a, "--hlsl-override").map(PathBuf::from),
        pso_patch: patches,
        no_predication: a.iter().any(|x| x == "--no-predication"),
        verbose: a.iter().any(|x| x == "--verbose"),
    };
    let gpu = match unsafe { d3d::Gpu::new(a.iter().any(|x| x == "--debug")) } {
        Ok(g) => g,
        Err(e) => {
            eprintln!("GPU init: {e}");
            return ExitCode::from(2);
        }
    };
    let mut e = engine::Engine::new(gpu, cap, opts);
    let r = unsafe {
        let r = if a.iter().any(|x| x == "--isolate") {
            e.gpu.begin();
            let r = e.run_isolate();
            let _ = e.gpu.submit();
            r
        } else {
            e.run_full()
        };
        if r.is_ok() && a.iter().any(|x| x == "--final") {
            e.dump_targets();
        }
        r
    };
    print!("{}", e.report);
    let _ = std::fs::write(out.join("report.txt"), &e.report);
    println!("report: {}", out.join("report.txt").display());
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("replay stopped: {err}");
            ExitCode::from(1)
        }
    }
}
