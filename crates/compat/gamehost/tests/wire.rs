use gamehost::wire::*;

fn console_two_string_size(n1: usize, n2: usize) -> usize {
    ((n1 * 2 + 7) & !3) + (n2 + 2) * 2
}

#[test]
fn launch_new_game_size_matches_xgameruntime_generator() {
    for (a, b) in [("", ""), ("g", "xy"), ("game.exe", "-a -b"), ("abc", "")] {
        let m = LaunchNewGame { exe: w(a), args: w(b) }.encode();
        assert_eq!(m.id, 0x67);
        assert_eq!(m.payload.len(), console_two_string_size(a.len(), b.len()));
        let (d, used) = LaunchNewGame::decode(&m.payload).unwrap();
        assert_eq!((d.exe, d.args, used), (w(a), w(b), m.payload.len()));
    }
}

#[test]
fn restart_on_crash_size_matches_xgameruntime_generator() {
    for a in ["", "x", "-restart now"] {
        let m = LaunchRestartOnCrash { args: w(a), value: 0x55 }.encode();
        assert_eq!(m.id, 0x68);
        assert_eq!(m.payload.len(), ((a.len() * 2 + 7) & !3) + 4);
        assert_eq!(LaunchRestartOnCrash::decode(&m.payload).unwrap().0, LaunchRestartOnCrash { args: w(a), value: 0x55 });
    }
}

#[test]
fn string_layout_is_count_then_utf16_without_nul() {
    let m = PackageUnmount { name: w("AB") }.encode();
    assert_eq!(m.payload, vec![2, 0, 0, 0, b'A', 0, b'B', 0]);
}

#[test]
fn trailing_nul_is_stripped_on_parse() {
    let p = vec![3, 0, 0, 0, b'A', 0, b'B', 0, 0, 0];
    assert_eq!(PackageUnmount::decode(&p).unwrap().0.name, w("AB"));
}

#[test]
fn short_payload_is_invalid_data() {
    assert_eq!(PackageUnmount::decode(&[5, 0, 0, 0, 1, 0]), Err(HR_INVALID_DATA));
    assert_eq!(PackageUnmount::decode(&[5, 0]), Err(HR_INVALID_DATA));
}

#[test]
fn mount_reply_size_matches_host_generator() {
    for (a, b) in [("", ""), ("n", "\\Device\\X"), ("name", "")] {
        let m = PackageMountResponse { hr: -1, name: w(a), device_path: w(b) }.encode();
        assert_eq!(m.id, 0x12c);
        assert_eq!(m.payload.len(), ((a.len() * 2 + 0xb) & !3) + (b.len() + 2) * 2);
        assert_eq!(&m.payload[..4], &[0xff; 4]);
        assert_eq!(PackageMountResponse::decode(&m.payload).unwrap().device_path, w(b));
    }
}

#[test]
fn unmount_reply_size_matches_host_generator() {
    let m = PackageUnmountResponse { hr: 0x80070490u32 as i32, name: w("abc") }.encode();
    assert_eq!(m.id, 0x12d);
    assert_eq!(m.payload.len(), 3 * 2 + 8);
    assert_eq!(&m.payload[..4], &0x80070490u32.to_le_bytes());
}

#[test]
fn mount_request_round_trip() {
    let r = PackageMount { name: w("dlc"), path: w("[XUC:]\\x.xvd") };
    let m = r.encode();
    assert_eq!(m.payload.len(), console_two_string_size(3, 12));
    assert_eq!(PackageMount::decode(&m.payload).unwrap().0, r);
}

#[test]
fn crash_dump_info_layout() {
    let mut cfg = [0u8; 16];
    cfg[8] = 2;
    cfg[0] = 0xaa;
    let m = CrashDumpInfo { config: cfg, file: w("D:\\x.etl") }.encode();
    assert_eq!((m.id, m.size()), (0xc8, 0x218));
    assert_eq!(m.payload[0], 0xaa);
    assert_eq!(&m.payload[8..16], &2u64.to_le_bytes());
    assert_eq!(&m.payload[0x10..0x12], &(b'D' as u16).to_le_bytes());
    assert_eq!(&m.payload[0x10 + 16..0x12 + 16], &[0, 0]);
    let d = CrashDumpInfo::decode(&m.payload);
    assert_eq!((d.flags(), d.reserved(), d.file), (2, 0xaa, w("D:\\x.etl")));
}

#[test]
fn crash_dump_path_never_overruns_its_field() {
    let m = CrashDumpInfo { config: [0; 16], file: vec![b'a' as u16; 0x200] }.encode();
    assert_eq!(m.payload.len(), 0x218);
}
