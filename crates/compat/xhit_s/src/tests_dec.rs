use super::*;
use crate::chunk::{header, ChunkWriter, TAG_SQD2};
use crate::report::*;
use crate::tests::{sink, take_log, LOCK};
use std::sync::Mutex;

static CMDS: Mutex<Vec<(u32, u32, Vec<u8>)>> = Mutex::new(Vec::new());
static SNAP: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static SQ: Mutex<Vec<u8>> = Mutex::new(Vec::new());

unsafe fn responder(data: *mut u8, mailbox: *mut u8) {
    let m = mailbox as usize;
    let d = data as usize;
    let cmd = *((m + kmd::MB_CMD) as *const u32);
    let n = *((m + kmd::MB_PAYLOAD_SIZE) as *const u32);
    let payload = core::slice::from_raw_parts((m + kmd::MB_PAYLOAD) as *const u8, n as usize).to_vec();
    CMDS.lock().unwrap().push((cmd, n, payload));
    let body = if cmd == kmd::CMD_HANG { SNAP.lock().unwrap().clone() } else { SQ.lock().unwrap().clone() };
    core::ptr::copy_nonoverlapping(body.as_ptr(), (d + kmd::D_RESP) as *mut u8, body.len());
    *((d + kmd::D_SIZE) as *mut u32) = body.len() as u32;
    *((d + kmd::D_STATUS) as *mut u32) = 0;
    *((d + kmd::D_ACK) as *mut u32) = *((m + kmd::MB_SEQ) as *const u32);
}

struct Kmd {
    data: Vec<u8>,
    mailbox: Vec<u8>,
}

impl Kmd {
    fn new() -> Self {
        let mut k = Kmd { data: vec![0u8; kmd::DATA_SIZE], mailbox: vec![0u8; kmd::MAILBOX_SIZE] };
        unsafe { kmd::install(k.data.as_mut_ptr(), k.mailbox.as_mut_ptr(), responder) };
        CMDS.lock().unwrap().clear();
        k
    }
    fn regions(&mut self) -> (usize, usize) {
        (self.data.as_mut_ptr() as usize, self.mailbox.as_mut_ptr() as usize)
    }
}

impl Drop for Kmd {
    fn drop(&mut self) {
        kmd::uninstall();
        test_reset(0);
    }
}

fn snap(console: u32, srbm2: u32, srbm: u32, g: u32, g2: u32, s0: u32, s1: u32) -> Vec<u8> {
    let mut s = vec![0u8; 0x400];
    for (o, v) in [(0usize, console), (4, srbm2), (8, srbm), (0xec, g), (0xf0, g2), (0xf4, s0), (0xf8, s1)] {
        s[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    s
}

fn put_q(r: &mut [u8], off: usize, v: u64) {
    r[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

fn put_d(r: &mut [u8], off: usize, v: u32) {
    r[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn with_log<F: FnOnce()>(verbosity: u32, f: F) -> Vec<String> {
    test_reset(verbosity);
    unsafe { XhitInitialize(sink as *const () as usize) };
    take_log();
    f();
    let l = take_log();
    unsafe { XhitTerminate() };
    test_reset(0);
    l
}

#[test]
fn post_requires_regions_and_ack() {
    let _g = LOCK.lock().unwrap();
    assert_eq!(unsafe { kmd::post_hang(None, core::ptr::null()) }, E_FAIL);
    let mut k = Kmd::new();
    let r = k.regions();
    k.mailbox[kmd::MB_SEQ] = 3;
    assert_eq!(unsafe { kmd::post(r, kmd::CMD_SQ, None) }, E_UNEXPECTED);
    assert!(CMDS.lock().unwrap().is_empty());
}

#[test]
fn request_round_trip_uses_console_mailbox_layout() {
    let _g = LOCK.lock().unwrap();
    let mut k = Kmd::new();
    *SQ.lock().unwrap() = vec![0xab; 0x30];
    let r = k.regions();
    let args = [1u32, 2, 3, 4];
    let (p, n, st) = unsafe { kmd::request(r, kmd::CMD_SQ, Some(&args)) }.unwrap();
    assert_eq!((p, n, st), (r.0 + kmd::D_RESP, 0x30, 0));
    assert_eq!(u32::from_le_bytes(k.mailbox[kmd::MB_SEQ..kmd::MB_SEQ + 4].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(k.mailbox[kmd::MB_ARGS + 12..kmd::MB_ARGS + 16].try_into().unwrap()), 4);
    k.mailbox[kmd::MB_CMD] = 5;
    assert_eq!(unsafe { kmd::response(r) }.0, r.0 + kmd::D_RESP5);
    k.mailbox[kmd::MB_CMD] = 7;
    assert_eq!(unsafe { kmd::response(r) }.0, r.0 + kmd::D_RESP7);
}

#[test]
fn chunk_header_packs_like_console() {
    assert_eq!(header(TAG_SQD2, true, 0x30, 2), [0x1_5351_4432, 0x2_0000_0030]);
    assert_eq!(header(TAG_SQD2, false, 0x30, 0), [0x5351_4432 | (0x30u64 << 33), 0x30]);
    let mut out: Vec<u8> = Vec::new();
    let body = [7u8; 5];
    let mut w = ChunkWriter::new(&mut out, true);
    let hr = unsafe { w.write_chunk(TAG_SQD2, &[(body.as_ptr(), 5), (core::ptr::null(), 3)], 0) };
    assert_eq!(hr, 0);
    assert_eq!(w.written, 21);
    assert_eq!(&out[16..], &body);
    assert_eq!(u64::from_le_bytes(out[8..16].try_into().unwrap()), 8);
    let mut none = ChunkWriter { out: None, written: 0, compact: false };
    assert_eq!(unsafe { none.write_chunk(TAG_SQD2, &[], 0) }, E_FAIL);
}

#[test]
fn collect_sq_writes_sqd2_chunk() {
    let _g = LOCK.lock().unwrap();
    let mut k = Kmd::new();
    *SQ.lock().unwrap() = vec![0x5a; 0x10];
    let mut out: Vec<u8> = Vec::new();
    let mut w = ChunkWriter::new(&mut out, true);
    let c = unsafe { sq::collect_sq(k.regions(), Some(&mut w)) }.unwrap();
    assert_eq!(c.size, 0x10);
    assert_eq!(&out[0..4], b"2DQS");
    assert_eq!(&out[16..], &[0x5a; 0x10]);
    assert_eq!(CMDS.lock().unwrap()[0].0, kmd::CMD_SQ);
}

#[test]
fn registers_console_line_only_at_default_verbosity() {
    let _g = LOCK.lock().unwrap();
    let s = snap(2, 0, 0x40, 0xffff_ffff, 0xffff_ffff, 0, 0);
    let mut out = 0u64;
    let l = with_log(0, || assert!(unsafe { regs::decode_registers(s.as_ptr(), &mut out, core::ptr::null(), 0) }));
    assert_eq!(l, vec!["Capture was taken from an Xbox One X console.\n".to_string()]);
    assert_eq!(out, 0);
    assert!(!unsafe { regs::decode_registers(s.as_ptr(), core::ptr::null_mut(), core::ptr::null(), 0) });
}

#[test]
fn registers_list_busy_blocks_with_shader_engines() {
    let _g = LOCK.lock().unwrap();
    let g = (1 << 29) | (1 << 16) | (1 << 17) | (1 << 22) | (1 << 13) | (1 << 12);
    let s0 = (1 << 23) | (1 << 27);
    let s1 = 1 << 23;
    let s = snap(1, (1 << 19) | (1 << 17), (1 << 6) | (1 << 11), g, 1 << 24, s0, s1);
    let mut out = 1u64 << 13;
    let l = with_log(1, || {
        unsafe { regs::decode_registers(s.as_ptr(), &mut out, core::ptr::null(), 0) };
    });
    assert_eq!(
        l,
        vec![
            "Capture was taken from an Xbox One S console.\n",
            "The following blocks are not idle:\nRLC",
            ", CP",
            ", WD(DRAW, not DMA??)",
            ", VGT (SE0&1)",
            ", SPI (SE0)",
            ", SDMA1",
            ", SDMA3",
            ", UVD",
            ", (IH)",
            "\n\n",
        ]
    );
    assert_eq!(out, (1 << 13) | regs::OUT_SDMA1 | regs::OUT_SDMA3);
}

#[test]
fn registers_idle_gpu_sets_idle_bit_and_clean_checks() {
    let _g = LOCK.lock().unwrap();
    let s = snap(0, 0, 0x80, 0, 0, 0, 0x4);
    let mut out = 0u64;
    let l = with_log(1, || {
        unsafe { regs::decode_registers(s.as_ptr(), &mut out, core::ptr::null(), 0) };
    });
    assert_eq!(l[1], "The following blocks are not idle:\nCB!Clean (SE0)");
    assert_eq!(l[2], ", DB!Clean (SE0&1)");
    assert_eq!(l[3], ", VCE(auto-mode, not hung)");
    assert_eq!(out, 0);
    let s = snap(0, 0, 0, (1 << 13) | (1 << 12), 0, 0, 0);
    let mut out = 0u64;
    let l = with_log(1, || {
        unsafe { regs::decode_registers(s.as_ptr(), &mut out, core::ptr::null(), 0) };
    });
    assert_eq!(l.len(), 2);
    assert_eq!(out, regs::OUT_IDLE);
}

#[test]
fn summary_round_trips_through_emulated_kmd() {
    let _g = LOCK.lock().unwrap();
    let k = Kmd::new();
    *SNAP.lock().unwrap() = snap(2, 0, 0, 0, 0, 0, 0);
    *SQ.lock().unwrap() = vec![1u8; 0x20];
    let mut ctx = vec![0u8; 0x60];
    ctx[0x50..0x58].copy_from_slice(&0x1122u64.to_le_bytes());
    ctx[0] = 0x77;
    let mut out = vec![0u64; REPORT_SIZE / 8];
    let mut hr = 0;
    let l = with_log(0, || {
        hr = unsafe { XhitPerformHangSummary(ctx.as_ptr(), out.as_mut_ptr()) };
    });
    assert_eq!(hr, 0);
    let cmds = CMDS.lock().unwrap().clone();
    assert_eq!(cmds.iter().map(|c| c.0).collect::<Vec<_>>(), vec![kmd::CMD_HANG, kmd::CMD_SQ, kmd::CMD_HANG]);
    assert_eq!(cmds[0].1, 0x58);
    assert_eq!(cmds[0].2, ctx[..0x58].to_vec());
    assert_eq!(cmds[1].1, 0);
    assert_eq!(out[0x318 / 8], 0x1122);
    assert!(l.contains(&"Capture was taken from an Xbox One X console.\n".to_string()));
    drop(k);
}

fn report() -> Vec<u8> {
    vec![0u8; REPORT_SIZE]
}

fn summary_log(r: &[u8], file: Option<&str>, dev: Option<&str>) -> String {
    with_log(0, || unsafe { high_level_summary(r.as_ptr(), file, dev) }).concat()
}

#[test]
fn summary_idle_report() {
    let _g = LOCK.lock().unwrap();
    let l = summary_log(&report(), None, None);
    assert!(l.contains("| XHIT GPU Hang High Level Summary"));
    assert!(l.contains("| Next Steps"));
    assert!(l.contains("The GPU appears to be idle.\n"));
    assert!(!l.contains("Autocategorized"));
    assert!(!l.contains("written to file"));
}

#[test]
fn summary_deadlock_and_files() {
    let _g = LOCK.lock().unwrap();
    let mut r = report();
    put_q(&mut r, 0, DEADLOCK_BIT);
    let l = summary_log(&r, Some("t:\\gpuhang-1.xhit"), Some("d:\\gpuhang-latest.xhit"));
    assert!(l.contains("The GPU hang dump data was written to file \"t:\\gpuhang-1.xhit\".\n"));
    assert!(l.contains("also written to development kit file \"d:\\gpuhang-latest.xhit\".\n"));
    assert!(l.contains("A fence deadlock was detected."));
    assert!(l.contains("We hope this information helps"));
}

#[test]
fn summary_bucket_and_ia_classes() {
    let _g = LOCK.lock().unwrap();
    let mut r = report();
    put_d(&mut r, R_BUCKET, 0x300a_0042);
    let l = summary_log(&r, None, None);
    assert!(l.contains("Autocategorized hang type: HangBucketType_WaitRegMem_Pfp0_Sdma0 (0x300a0042)\n"));
    assert!(l.contains("This GPU hang should be looked at by Xbox Developer Support."));
    put_d(&mut r, R_BUCKET, BUCKET_IA_ISSUING);
    let l = summary_log(&r, None, None);
    assert!(!l.contains("Autocategorized"));
    assert!(l.contains("The IA is still successfully issuing new indices / instances."));
    put_d(&mut r, R_BUCKET, BUCKET_NO_SCRATCH);
    put_q(&mut r, 0, F_NO_SCRATCH | F_SHADER_SPIN);
    let l = summary_log(&r, None, None);
    assert!(l.contains("shader scratch memory has been disabled"));
    assert!(l.contains("The sequencer indicates that a shader was spinning"));
}

#[test]
fn summary_memory_wait_with_fence_table() {
    let _g = LOCK.lock().unwrap();
    let mut r = report();
    r[R_WAIT_VALID] = 1;
    let va = 0x12_3456_7890u64;
    put_q(&mut r, 0x18, W_VALID | W_MEMORY | (3 << 43) | va);
    put_d(&mut r, 0x18 + 8, 0xffff);
    put_d(&mut r, 0x18 + 0xc, 5);
    put_q(&mut r, 0x18 + 0x10, 4);
    let mut table = vec![0u8; FENCE_ENTRIES * FENCE_STRIDE];
    let e = 3 * FENCE_STRIDE;
    put_q(&mut table, e, 0xdead_0000);
    put_q(&mut table, e + 8, va);
    for (i, c) in "Frame".encode_utf16().enumerate() {
        table[e + 0x10 + i * 2..e + 0x12 + i * 2].copy_from_slice(&c.to_le_bytes());
    }
    put_q(&mut r, R_FENCE_TABLE, table.as_ptr() as u64);
    put_d(&mut r, R_BUCKET, 0x300a_0000);
    let l = summary_log(&r, None, None);
    assert!(l.contains("The Title command processor ME engine was waiting in an infinite loop for a memory location to\nchange value. The location is 0x1234567890, and the wait was until the 32-bit value\nat that location was == the reference value (0x5)\nAND the mask (0xffff).\nThe current value at that memory location is 0x4.\n"));
    assert!(l.contains("corresponds to the ID3D12Fence object\n(0x00000000DEAD0000 \"Frame\").\n"));
    assert!(l.contains("We hope this information helps"));
}

#[test]
fn summary_wait_variants() {
    let _g = LOCK.lock().unwrap();
    let mut r = report();
    r[R_WAIT_VALID] = 1;
    put_q(&mut r, 0x1b0, W_VALID);
    put_q(&mut r, 0x108, W_VALID | W_64BIT | (5 << 43) | 0x40);
    put_d(&mut r, 0x108 + 8, 1);
    put_d(&mut r, 0x108 + 0xc, 2);
    put_q(&mut r, 0x30, W_VALID | W_MEMORY | W_EXEC_SIGNAL | 0x5000);
    put_q(&mut r, R_WAIT_OBJS + 8, 0xabc);
    put_q(&mut r, 0x48, W_VALID | W_MEMORY | (PRESENT_SYNC_BASE + 8));
    put_q(&mut r, 0x60, W_VALID | W_MEMORY | 0x7000);
    let l = summary_log(&r, None, None);
    assert!(l.contains("The Async compute pipe 0 queue 0 engine was waiting in an infinite loop for a memory location or\nregister to change value."));
    assert!(l.contains("The Async compute pipe 1 queue 0 engine was waiting in an infinite loop for a register to\nchange value. The register offset is 0x0000000040, and the wait was until the value\nin that register was >= the reference value (0x100000002)\nAND the mask (0xffffffffffffffff).\n"));
    assert!(l.contains("The fence memory location 0x0000005000 corresponds to the execution signal writeback\nwithin command list API object 0x0000000000000ABC.\n"));
    assert!(l.contains("corresponds to the present queue sync object 2.\n"));
    assert!(l.contains("The fence location has no additional detail"));
    assert!(l.contains("This GPU hang should be looked at by Xbox Developer Support."));
}

#[test]
fn fence_detail_lookup_cases() {
    let _g = LOCK.lock().unwrap();
    let mut r = report();
    let mut table = vec![0u8; FENCE_ENTRIES * FENCE_STRIDE];
    put_q(&mut table, 8, 0x900);
    put_q(&mut r, R_FENCE_TABLE, table.as_ptr() as u64);
    let l = with_log(0, || unsafe {
        fence_detail(r.as_ptr(), 0x900);
        fence_detail(r.as_ptr(), 0x901);
        fence_detail(r.as_ptr(), 0);
    });
    assert_eq!(l.len(), 2);
    assert_eq!(l[0], "The fence memory location 0x0000000900 corresponds to a custom fence location\nnamed \"<untitled>\".\n");
    assert!(l[1].starts_with("The fence writeback address does not correspond"));
}

#[test]
fn bucket_names_follow_console_table() {
    use crate::bucket::name;
    let n = |b| name(b).unwrap();
    assert_eq!(n(0), "HangBucketType_Idle");
    assert_eq!(n(0x3000), "HangBucketType_Idle_Legacy");
    assert_eq!(n(0x0081), "HangBucketType_Idle_EraInit_UMD_DeviceRemoved");
    assert_eq!(n(0x00c2), "HangBucketType_Idle_EraOther_UnknownSource_0x00c0");
    assert_eq!(n(0x4), "HangBucket_TotallyIdleHung_Unknown0x00000004");
    assert_eq!(n(0x3002_1010), "HangBucketType_CpgBusy_Corruption_Me0");
    assert_eq!(n(0x3002_0009), "HangBucketType_CpgBusy_Unknown0x30020009");
    assert_eq!(n(0x3004_0000), "HangBucketType_CpcBusyGeneric");
    assert_eq!(n(0x3004_1425), "HangBucketType_Cpc_BadPacket_Normal_EOPQ_Pipe2_Queue5");
    assert_eq!(n(0x3100_0801), "HangBucketType_SomethingBeyondCp_Ta_Wd");
    assert_eq!(n(0x301c_0101), "HangBucketType_CafeBabe_pfpIb1(p0)HangBucketType_DeadBeef_pfpIb2(p0)");
    assert_eq!(n(0x4004_1001), "HangBucketType_StillProgressing_ps0_csg");
    assert_eq!(n(0x6002_1100), "HangBucketType_GprWrapBugHS_TitleGraphicsCompute");
    assert_eq!(n(0x6006_0005), "HangBucketType_CbCmaskCorruption_0x00005");
    assert_eq!(n(0x5100_0007), "HangBucketType_LateAllocVs_(internal error 0x51000007)");
    assert_eq!(n(0x3014_0000), "Unknown_0x30140000");
    assert_eq!(n(0x0100_0011), "HangBucketType_SomethingInSrbmBusy_Sdma0_Uvd");
}
