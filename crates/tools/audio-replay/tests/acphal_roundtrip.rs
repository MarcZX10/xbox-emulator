use acphal::*;
use core::ffi::c_void;
use std::path::PathBuf;

type ConnectFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> i32;
type SubmitFn = unsafe extern "system" fn(*mut c_void, u32, u64, u32, *const u8, u32) -> i32;
type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;

const RING: u32 = 31;

fn packets() -> (Vec<u8>, usize) {
    if let Some(dir) = std::env::var_os("TOMBOX_XMA_REF") {
        for e in std::fs::read_dir(PathBuf::from(dir)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().and_then(|x| x.to_str()) == Some("xma") {
                let ch = if p.to_string_lossy().contains("_2ch_") { 2 } else { 1 };
                let mut d = std::fs::read(&p).unwrap();
                d.truncate(2048 * 48);
                return (d, ch);
            }
        }
    }
    let mut d = vec![0u8; 2048 * 24];
    let mut x = 0x1234_5678u32;
    for (i, b) in d.iter_mut().enumerate() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = if i % 2048 < 4 { [0x04, 0x00, 0x00, 0x00][i % 2048] } else { x as u8 };
    }
    (d, 1)
}

fn alloc(size: u32, align: u32) -> (usize, u32) {
    let mut p: *mut c_void = core::ptr::null_mut();
    let mut a = 0u32;
    assert_eq!(ApuAlloc(&mut p, &mut a, size, align, 1), 0);
    (p as usize, a)
}

unsafe fn r32(p: usize, o: usize) -> u32 {
    core::ptr::read_unaligned((p + o) as *const u32)
}

unsafe fn w32(p: usize, o: usize, v: u32) {
    core::ptr::write_unaligned((p + o) as *mut u32, v)
}

#[test]
fn fortnite_style_recording_replays_to_identical_output() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("acphal_roundtrip");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rec = dir.join("run.tbrec");
    audiorec::record_to(&rec).unwrap();
    assert_eq!(ApuCreateHeap(0x0400_0000, 0x0011_4000), 0);
    let mut ctx = [0u8; 0x60];
    ctx[0..4].copy_from_slice(&1u32.to_le_bytes());
    ctx[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(AcpHalAllocateShapeContexts(ctx.as_mut_ptr()), 0);
    let xma = usize::from_le_bytes(ctx[0x18 + 4 * 8..0x20 + 4 * 8].try_into().unwrap());
    let mut hal: *mut c_void = core::ptr::null_mut();
    assert_eq!(AcpHalCreate(&mut hal), 0);
    let vt = unsafe { *(hal as *const *const usize) };
    let slot = |i: usize| unsafe { *vt.add(i) };
    let connect: ConnectFn = unsafe { core::mem::transmute(slot(0)) };
    let submit: SubmitFn = unsafe { core::mem::transmute(slot(2)) };
    let release: ReleaseFn = unsafe { core::mem::transmute(slot(5)) };
    assert_eq!(unsafe { connect(hal, 0x1000, 0x1000) }, 0);
    let (data, ch) = packets();
    let npk = (data.len() / 2048) as u32;
    let (inva, inapu) = alloc(data.len() as u32, 0x800);
    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), inva as *mut u8, data.len()) };
    let (outva, outapu) = alloc(RING * 256, 0x100);
    let (fva, fapu) = alloc(0x100, 0x100);
    unsafe {
        w32(xma, 0, npk | (1 << 20) | (RING << 22));
        w32(xma, 4, (4 << 20) | (3 << 27) | (((ch == 2) as u32) << 29) | (1 << 31));
        w32(xma, 8, 32);
        w32(xma, 0x14, inapu);
        w32(xma, 0x1c, outapu);
        w32(xma, 0x24, 0);
    }
    let idx = 0u32.to_le_bytes();
    let mut live = Vec::new();
    let mut idle = 0;
    while idle < 3 {
        for kind in [5u32, 7] {
            unsafe { core::ptr::write_volatile(fva as *mut u32, 0) };
            assert_eq!(unsafe { submit(hal, kind, 0xff, u32::MAX, idx.as_ptr(), fapu) }, 0);
            assert_eq!(unsafe { core::ptr::read_volatile(fva as *const u32) }, 1);
        }
        let (w, r) = unsafe { ((r32(xma, 0) >> 27) % RING, r32(xma, 0x24) & 31) };
        if w == r {
            idle += 1;
            continue;
        }
        idle = 0;
        let mut b = r;
        while b != w {
            let s = unsafe { core::slice::from_raw_parts((outva + b as usize * 256) as *const u8, 256) };
            live.extend_from_slice(s);
            b = (b + 1) % RING;
        }
        unsafe { w32(xma, 0x24, (r32(xma, 0x24) & !31) | w) };
    }
    unsafe { release(hal) };
    assert_eq!(AcpHalReleaseShapeContexts(), 0);
    audiorec::stop_recording();
    assert!(!live.is_empty());
    let out = dir.join("replay");
    let st = std::process::Command::new(env!("CARGO_BIN_EXE_audio-replay")).arg(&rec).arg(&out).output().unwrap();
    let text = String::from_utf8_lossy(&st.stdout).to_string();
    assert!(st.status.success(), "{text}\n{}", String::from_utf8_lossy(&st.stderr));
    eprintln!("{text}");
    assert!(text.contains("every return value matched the recording"), "{text}");
    let wav = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().path()).find(|p| p.file_name().unwrap().to_string_lossy().starts_with("xma000_0_")).expect("xma wav");
    let bytes = std::fs::read(wav).unwrap();
    assert_eq!(&bytes[44..], &live[..], "replayed XMA ring output differs from the live run");
    assert!(std::fs::read_to_string(out.join("depth.csv")).unwrap().lines().count() > 2);
}
