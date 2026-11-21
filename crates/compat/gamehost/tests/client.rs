use gamehost::host::*;
use gamehost::wire::*;
use gamehost::xcrd::{DllXcrd, Xcrd};
use std::process::Command;

struct NoXcrd;

impl Xcrd for NoXcrd {
    fn open_adapter(&mut self) -> Result<usize, i32> {
        Err(0x80070002u32 as i32)
    }
    fn close_adapter(&mut self, _: usize) -> i32 {
        0
    }
    fn mount_content_type(&mut self, _: usize, _: &[u16], _: u32, _: u32) -> Result<usize, i32> {
        Err(0x80070002u32 as i32)
    }
    fn query_device_path(&mut self, _: usize) -> Result<Vec<u16>, i32> {
        Err(0x80070002u32 as i32)
    }
    fn unmount(&mut self, _: usize, _: usize) -> i32 {
        0
    }
}

#[test]
fn wer_client_flow_against_installed_host() {
    std::env::set_var("TOMBOX_CONSOLE_OSBUILD", "22621");
    gamehost::reset();
    assert_eq!(gamehost::wer::transport_event(0), Some(HR_NO_CONNECTION));
    let mut h = GameHost::new(22621, true, Box::new(NoXcrd));
    let mut cfg = [0u8; 16];
    cfg[8] = 3;
    h.set_launch(&w("Ident"), cfg);
    h.record_quiesce(Some(&w("D:\\t\\title.exe")), SystemTime { year: 2026, month: 1, day_of_week: 0, day: 2, hour: 3, minute: 4, second: 5, milliseconds: 6 });
    gamehost::install(h);
    let r = gamehost::wer::dump_notify_start().unwrap().unwrap();
    assert_eq!(r, (3, "D:\\LocalDumps\\Quiesce-title.exe-2026-1-2-3-4-5-6.etl".to_string()));
    assert_eq!(gamehost::wer::transport_event(1), Some(0));
    gamehost::with_host(|h| {
        assert_eq!(h.dumps_in_flight, 1);
        assert_eq!(h.wnf_crash_dump_state, 1);
        assert!(h.debug.iter().any(|d| d == "GameHostService: Crash dump Progress -> Collecting Heap Dump...\n"));
    })
    .unwrap();
    gamehost::wer::dump_notify_complete();
    gamehost::with_host(|h| {
        assert_eq!(h.dumps_in_flight, 0);
        assert_eq!(h.wnf_crash_dump_state, 0);
        assert_eq!(h.debug.last().unwrap(), "GameHostService: Crash dump completed.\n");
    })
    .unwrap();
    assert_eq!(gamehost::wer::transport_event(2), Some(HR_NO_CONNECTION));

    std::env::set_var("TOMBOX_CONSOLE_OSBUILD", "22000");
    gamehost::install(GameHost::new(22000, false, Box::new(NoXcrd)));
    assert_eq!(gamehost::wer::dump_notify_start().unwrap(), Some((0, String::new())));
    assert_eq!(gamehost::wer::transport_event(0), None);
    gamehost::wer::dump_notify_complete();

    std::env::set_var("TOMBOX_CONSOLE_OSBUILD", "19041");
    assert_eq!(gamehost::wer::dump_notify_start(), Ok(None));
    assert_eq!(gamehost::wer::transport_event(0), None);
    std::env::remove_var("TOMBOX_CONSOLE_OSBUILD");
    gamehost::reset();
}

#[test]
fn reply_validation_matches_werxboxgc_checks() {
    let ok = CrashDumpInfo { config: [0; 16], file: Vec::new() }.encode();
    assert!(gamehost::validate_crash_dump_reply(&ok).is_ok());
    assert_eq!(gamehost::validate_crash_dump_reply(&Message::new(0xc9, vec![0; 0x218])), Err(Fault::FailFast { file: "gamehostserviceproxy.cpp", line: 0x30 }));
    assert_eq!(gamehost::validate_crash_dump_reply(&Message::new(0xc8, vec![0; 0x210])), Err(Fault::FailFast { file: "gamehostserviceproxy.cpp", line: 0x27 }));
}

#[test]
fn capability_and_xcrd_bind_by_dll_export() {
    assert_eq!(gamehost::capability_from("tombox_no_such_xpal.dll", 0x7f), Err(0x8007007eu32 as i32));
    assert_eq!(gamehost::capability_from("kernel32.dll", 0x7f), Err(0x8007007fu32 as i32));
    let mut x = DllXcrd::new("tombox_no_such_xcrdapi.dll");
    assert_eq!(x.open_adapter(), Err(0x8007007eu32 as i32));
    assert_eq!(x.unmount(0, 0), 0x8007007eu32 as i32);
}

#[test]
fn host_start_without_xpal_means_no_connection() {
    if std::env::var("GAMEHOST_CHILD").is_ok() {
        assert_eq!(gamehost::start_host(22000).map(|h| h.local_dumps_capability), Ok(false));
        if gamehost::xcrd::load("xpal.dll").is_err() {
            assert_eq!(gamehost::start_host(22621).err(), Some(0x8007007eu32 as i32));
        }
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let st = Command::new(exe).args(["--exact", "host_start_without_xpal_means_no_connection", "--nocapture"]).env("GAMEHOST_CHILD", "1").status().unwrap();
    assert!(st.success());
}

#[test]
fn fail_fast_terminates_with_fast_fail_status() {
    if std::env::var("GAMEHOST_FF").is_ok() {
        gamehost::fail_fast();
    }
    let exe = std::env::current_exe().unwrap();
    let st = Command::new(exe).args(["--exact", "fail_fast_terminates_with_fast_fail_status"]).env("GAMEHOST_FF", "1").status().unwrap();
    assert_eq!(st.code().map(|c| c as u32), Some(0xC0000409));
}
