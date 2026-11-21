use acphal::*;
use core::ffi::c_void;
use std::path::PathBuf;

type ConnectFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> i32;
type SubmitFn = unsafe extern "system" fn(*mut c_void, u32, u64, u32, *const u8, u32) -> i32;
type PopFn = unsafe extern "system" fn(*mut c_void, *mut u8) -> i32;
type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;

const RING: u32 = 31;
const DMA_BLOCKS: u32 = 9;

fn packets() -> (Vec<u8>, usize) {
    if let Some(dir) = std::env::var_os("TOMBOX_XMA_REF") {
        let mut v: Vec<_> = std::fs::read_dir(PathBuf::from(dir)).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "xma")).collect();
        v.sort();
        if let Some(p) = v.iter().find(|p| p.to_string_lossy().contains("_2ch_")) {
            let mut d = std::fs::read(p).unwrap();
            d.truncate(2048 * 24);
            return (d, 2);
        }
    }
    let mut d = vec![0u8; 2048 * 8];
    let mut x = 0x9e37_79b9u32;
    for (i, b) in d.iter_mut().enumerate() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = if i % 2048 < 4 { [0x04, 0x00, 0x00, 0x00][i % 2048] } else { x as u8 };
    }
    (d, 2)
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
fn wwise_style_flowgraph_recording_replays_to_identical_dma_output() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("acphal_flowgraph");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rec = dir.join("run.tbrec");
    audiorec::record_to(&rec).unwrap();
    assert_eq!(ApuCreateHeap(0x0400_0000, 0x0011_4000), 0);
    let mut ctx = [0u8; 0x60];
    for (k, n) in [(0usize, 1u32), (3, 2), (4, 1)] {
        ctx[k * 4..k * 4 + 4].copy_from_slice(&n.to_le_bytes());
    }
    assert_eq!(AcpHalAllocateShapeContexts(ctx.as_mut_ptr()), 0);
    let ptr = |k: usize| usize::from_le_bytes(ctx[0x18 + k * 8..0x20 + k * 8].try_into().unwrap());
    let (src, dma, xma) = (ptr(0), ptr(3), ptr(4));
    let mut hal: *mut c_void = core::ptr::null_mut();
    assert_eq!(AcpHalCreate(&mut hal), 0);
    let vt = unsafe { *(hal as *const *const usize) };
    let slot = |i: usize| unsafe { *vt.add(i) };
    let connect: ConnectFn = unsafe { core::mem::transmute(slot(0)) };
    let submit: SubmitFn = unsafe { core::mem::transmute(slot(2)) };
    let pop: PopFn = unsafe { core::mem::transmute(slot(3)) };
    let release: ReleaseFn = unsafe { core::mem::transmute(slot(5)) };
    assert_eq!(unsafe { connect(hal, 0x100, 0x100) }, 0);
    let (data, ch) = packets();
    let (inva, inapu) = alloc(data.len() as u32, 0x800);
    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), inva as *mut u8, data.len()) };
    let (_, outapu) = alloc(RING * 256, 0x100);
    let (ringva, ringapu) = alloc(DMA_BLOCKS * 2 * 128 * 4, 0x200);
    let (listva, listapu) = alloc(0x100, 0x100);
    let cmds = [shape::encode_clear(0), shape::encode_clear(1), shape::encode_src(0, 0, 1), shape::encode_dma(0, 0), shape::encode_dma(1, 1)];
    for (i, c) in cmds.iter().enumerate() {
        unsafe { core::ptr::write_unaligned((listva + i * 8) as *mut u64, *c) };
    }
    unsafe {
        w32(xma, 0, (data.len() / 2048) as u32 | (1 << 20) | (RING << 22));
        w32(xma, 4, (1 << 20) | (3 << 27) | (((ch == 2) as u32) << 29) | (1 << 31));
        w32(xma, 8, 32);
        w32(xma, 0x14, inapu);
        w32(xma, 0x1c, outapu);
        w32(src, 4, 240_845);
        for d in 0..2usize {
            let p = dma + d * 0x10;
            w32(p, 4, 0);
            w32(p, 8, ringapu);
            w32(p, 0xc, (DMA_BLOCKS - 1) | (1 << 16) | ((d as u32) << 24));
        }
    }
    let mut mask = [0u8; 64];
    mask[0] = 1;
    assert_eq!(unsafe { submit(hal, 6, 0x33, u32::MAX, mask.as_ptr(), 0) }, 0);
    let mut live = [Vec::new(), Vec::new()];
    let mut m = [0u8; 28];
    let mut pl = [0u8; 8];
    pl[0..4].copy_from_slice(&(cmds.len() as u32).to_le_bytes());
    pl[4..8].copy_from_slice(&listapu.to_le_bytes());
    let mut blocked = 0;
    for frame in 0..400 {
        let w0: Vec<u32> = (0..2).map(|d| unsafe { (r32(dma + d * 0x10, 4) >> 8) & 31 }).collect();
        assert_eq!(unsafe { submit(hal, 1, 0x34, frame, pl.as_ptr(), 0) }, 0);
        while unsafe { pop(hal, m.as_mut_ptr()) } != 0 {
            if m[0] == shape::MSG_SRC_BLOCKED as u8 {
                blocked += 1;
            }
        }
        for d in 0..2usize {
            let w1 = unsafe { (r32(dma + d * 0x10, 4) >> 8) & 31 };
            if w1 == w0[d] {
                continue;
            }
            let blk = w0[d] as usize;
            let off = (blk * 2 + d) * 128 * 4;
            live[d].extend_from_slice(unsafe { core::slice::from_raw_parts((ringva + off) as *const u8, 512) });
            let mut c = [0u8; 8];
            c[0..4].copy_from_slice(&(d as u32).to_le_bytes());
            c[4..8].copy_from_slice(&1u32.to_le_bytes());
            assert_eq!(unsafe { submit(hal, 0x10, 0, u32::MAX, c.as_ptr(), 0) }, 0);
        }
        if frame % 16 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    }
    let mut off = [0u8; 64];
    off[0] = 1;
    assert_eq!(unsafe { submit(hal, 8, 0x35, u32::MAX, off.as_ptr(), 0) }, 0);
    unsafe { release(hal) };
    assert_eq!(AcpHalReleaseShapeContexts(), 0);
    audiorec::stop_recording();
    assert!(live[0].len() >= 512 * 100, "{} bytes, {blocked} SRC_BLOCKED", live[0].len());
    let out = dir.join("replay");
    let st = std::process::Command::new(env!("CARGO_BIN_EXE_audio-replay")).arg(&rec).arg(&out).output().unwrap();
    let text = String::from_utf8_lossy(&st.stdout).to_string();
    assert!(st.status.success(), "{text}\n{}", String::from_utf8_lossy(&st.stderr));
    eprintln!("{text}");
    assert!(text.contains("every return value matched the recording"), "{text}");
    for d in 0..2usize {
        let name = format!("dma{d:04}_ch{d}_48000.wav");
        let bytes = std::fs::read(out.join(&name)).unwrap_or_else(|_| panic!("{name} missing"));
        assert_eq!(&bytes[44..], &live[d][..], "{name} differs from the live DMA ring");
    }
}
