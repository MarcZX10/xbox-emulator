use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const REQUIRED: &[&str] = &["mfplat.dll", "XAudio2_9.dll", "xmem.dll", "d3d12_xs.dll", "xcrdapi.dll", "dxcompiler_xs.dll", "acphal.dll"];

fn dll_dir() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("XHOST_DLL_DIR") {
        cands.push(PathBuf::from(d));
    }
    cands.push(PathBuf::from(r"D:\xbox-pc-port\emulator\target-qa\release"));
    cands.push(PathBuf::from(r"D:\xbox-pc-port\emulator\target-qa\debug"));
    cands.into_iter().find(|d| REQUIRED.iter().all(|n| d.join(n).exists()))
}

fn run(args: &[&str]) -> (Option<i32>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xhost")).args(args).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().expect("spawn xhost");
    let mut out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let code = loop {
        if let Some(st) = child.try_wait().expect("wait xhost") {
            break st.code();
        }
        if start.elapsed() > Duration::from_secs(90) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let text = reader.join().unwrap_or_default();
    println!("{text}");
    (code, text)
}

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn harness(extra: &[&str]) {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(dir) = dll_dir() else {
        eprintln!("SKIP: compat DLLs not built in target-qa (cargo build --release -p d3d12_xs -p xmem -p acphal -p dxcompiler_xs -p mfplat -p xcrdapi -p audiohalwrapper -p xaudio2_9 with CARGO_TARGET_DIR=target-qa)");
        return;
    };
    let d = dir.to_string_lossy().into_owned();
    let mut args = vec!["--dir", d.as_str()];
    args.extend_from_slice(extra);
    let (code, text) = run(&args);
    assert!(code.is_some(), "xhost did not finish within 90 s");
    if code == Some(3) {
        eprintln!("SKIP: xhost could not stage the DLL set");
        return;
    }
    let fails: Vec<&str> = text.lines().filter(|l| l.starts_with("FAIL")).collect();
    assert!(fails.is_empty() && code == Some(0), "xhost exit {code:?}; failures:\n{}", fails.join("\n"));
    assert!(text.contains("RESULT:"), "no RESULT line");
}

#[test]
fn game_load_order() {
    harness(&[]);
}

#[test]
fn xmem_loaded_first() {
    harness(&["--order", "xmem.dll,mfplat.dll,XAudio2_9.dll,d3d12_xs.dll,xcrdapi.dll,dxcompiler_xs.dll,acphal.dll,AudioHALWrapper.dll"]);
}
