use std::path::PathBuf;
use xapu::*;

const FRAME: usize = 512;

fn wait_result(h: usize) {
    let t0 = std::time::Instant::now();
    let mut dq = 0u32;
    while XApuGetResultCount(h, &mut dq) == 0 {
        assert!(t0.elapsed().as_secs() < 10, "result never arrived");
        std::thread::yield_now();
    }
}

#[test]
fn opus_recording_replays_to_identical_output() {
    let refs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../third_party/opus-refs/hybrid.opus");
    let Ok(file) = std::fs::read(&refs) else {
        eprintln!("skipping: {} not present", refs.display());
        return;
    };
    let f = opusdec::ogg::parse_opus_file(&file).unwrap();
    let ch = f.channels;
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("xapu_roundtrip");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rec = dir.join("run.tbrec");
    rec::record_to(&rec).unwrap();
    let p = [2u32, 1, 4, 0x10000, FRAME as u32, 0];
    let mut region = [0u8; 16];
    let mut h = 0usize;
    assert_eq!(XApuConnect(p.as_ptr(), region.as_mut_ptr(), &mut h), 0);
    let mut seq = 0u32;
    let mut c = [0u8; 0x40];
    c[0..4].copy_from_slice(&CMD_START.to_le_bytes());
    c[0x18..0x1c].copy_from_slice(&(ch as u32).to_le_bytes());
    c[0x28..0x2c].copy_from_slice(&1.0f32.to_le_bytes());
    c[0x2c..0x30].copy_from_slice(&(ch as u32 * (FRAME as u32 * 4 + 0x1e00)).to_le_bytes());
    assert_eq!(XApuEnqueueCommand(h, c.as_ptr(), &mut seq), 0);
    wait_result(h);
    let mut r = [0u8; 0x50];
    assert_eq!(XApuDequeueResult(h, r.as_mut_ptr()), 0);
    let mut out = vec![0f32; FRAME * ch];
    let mut live: Vec<f32> = Vec::new();
    for (k, pkt) in f.packets.iter().take(150).enumerate() {
        let mut d = [0u8; 0x40];
        d[0..4].copy_from_slice(&CMD_DATA.to_le_bytes());
        d[0x1c..0x20].copy_from_slice(&1.0f32.to_le_bytes());
        d[0x20..0x24].copy_from_slice(&(if k == 0 { f.pre_skip as u32 } else { 0 }).to_le_bytes());
        d[0x24..0x28].copy_from_slice(&u32::MAX.to_le_bytes());
        d[0x28..0x2c].copy_from_slice(&(pkt.len() as u32).to_le_bytes());
        d[0x2c..0x34].copy_from_slice(&(pkt.as_ptr() as u64).to_le_bytes());
        d[0x34..0x38].copy_from_slice(&((out.len() * 4) as u32).to_le_bytes());
        d[0x38..0x40].copy_from_slice(&(out.as_mut_ptr() as u64).to_le_bytes());
        assert_eq!(XApuEnqueueCommand(h, d.as_ptr(), &mut seq), 0);
        wait_result(h);
        assert_eq!(XApuDequeueResult(h, r.as_mut_ptr()), 0);
        let n = u32::from_le_bytes(r[0x30..0x34].try_into().unwrap()) as usize;
        for i in 0..n {
            for c in 0..ch {
                live.push(out[c * FRAME + i]);
            }
        }
    }
    assert_eq!(XApuDisconnect(h), 0);
    rec::stop_recording();
    assert!(live.len() > FRAME * ch * 100);
    let outdir = dir.join("replay");
    let st = std::process::Command::new(env!("CARGO_BIN_EXE_audio-replay")).arg(&rec).arg(&outdir).output().unwrap();
    let text = String::from_utf8_lossy(&st.stdout).to_string();
    assert!(st.status.success(), "{text}\n{}", String::from_utf8_lossy(&st.stderr));
    eprintln!("{text}");
    assert!(text.contains("every return value and result matched the recording"), "{text}");
    let wav = std::fs::read_dir(&outdir).unwrap().map(|e| e.unwrap().path()).find(|p| p.extension().is_some_and(|x| x == "wav")).expect("wav");
    let got: Vec<f32> = std::fs::read(wav).unwrap()[44..].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    assert_eq!(got, live);
}
