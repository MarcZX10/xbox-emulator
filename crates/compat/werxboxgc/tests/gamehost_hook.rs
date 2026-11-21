use gamehost::host::{GameHost, SystemTime};
use gamehost::wire::w;
use gamehost::xcrd::DllXcrd;

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut core::ffi::c_void;
}

#[test]
fn dump_notify_start_takes_flags_and_path_from_game_host() {
    std::env::set_var("TOMBOX_CONSOLE_OSBUILD", "22621");
    let dir = std::env::temp_dir().join(format!("gamehost_hook_{}", std::process::id()));
    werxboxgc::wer().root = Some(dir.clone());
    let mut h = GameHost::new(22621, true, Box::new(DllXcrd::new("tombox_no_such_xcrdapi.dll")));
    let mut cfg = [0u8; 16];
    cfg[8] = (werxboxgc::HOST_HEAP_DUMP | werxboxgc::HOST_HANG_FILE) as u8;
    h.set_launch(&w("Test.Game"), cfg);
    h.record_quiesce(Some(&w("C:\\x\\game.exe")), SystemTime { year: 2026, month: 2, day_of_week: 1, day: 3, hour: 4, minute: 5, second: 6, milliseconds: 7 });
    gamehost::install(h);
    unsafe { werxboxgc::XerDumpNotifyStart(GetCurrentProcess()) };
    {
        let wr = werxboxgc::wer();
        assert_eq!(wr.host_flags, 3);
        assert_eq!(String::from_utf16_lossy(&wr.host_file), "D:\\LocalDumps\\Quiesce-game.exe-2026-2-3-4-5-6-7.etl");
    }
    assert_eq!(werxboxgc::XerProcessHeapDumpRequest(), 0);
    werxboxgc::XerTransportEventFinish();
    assert_eq!(werxboxgc::XerTpUpload(), 0x80004001u32 as i32);
    gamehost::with_host(|h| {
        assert_eq!(h.dumps_in_flight, 1);
        let ids: Vec<u32> = h.controller.as_ref().unwrap().iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![0x0b, 0x10, 0x10, 0x10]);
        assert_eq!(h.debug[1..].to_vec(), vec!["GameHostService: Crash dump Progress -> Collecting Heap Dump...\n", "GameHostService: Crash dump Progress -> Compressing Heap Dump...\n", "GameHostService: Crash dump Progress -> Contacting Watson Server...\n"]);
    })
    .unwrap();
    unsafe { werxboxgc::XerDumpNotifyComplete() };
    gamehost::with_host(|h| {
        assert_eq!(h.dumps_in_flight, 0);
        assert_eq!(h.wnf_crash_dump_state, 0);
    })
    .unwrap();
    gamehost::reset();
    std::env::remove_var("TOMBOX_CONSOLE_OSBUILD");
    let _ = std::fs::remove_dir_all(&dir);
}
