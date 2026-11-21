use super::*;
use std::os::windows::io::AsRawHandle;
use std::sync::Once;

static LOCK: Mutex<()> = Mutex::new(());
static TITLE: Once = Once::new();
static TEST_TITLE: Mutex<Option<compat_common::gdk::Title>> = Mutex::new(None);

struct Fx {
    _g: MutexGuard<'static, ()>,
    dir: PathBuf,
}

fn tmp(tag: &str) -> PathBuf {
    let mut t = 0u64;
    unsafe { GetSystemTimePreciseAsFileTime(&mut t) };
    std::env::temp_dir().join(format!("wer-test-{}-{tag}-{t}", std::process::id()))
}

fn fx() -> Fx {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    TITLE.call_once(|| {
        let d = tmp("title");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("MicrosoftGame.config"), "<Game configVersion=\"1\"><Identity Name=\"Test.Game\" Publisher=\"CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US\" Version=\"1.2.3.0\"/></Game>").unwrap();
        *TEST_TITLE.lock().unwrap() = Some(compat_common::gdk::Title::from_dir(&d, "fallback"));
        let _ = std::fs::remove_dir_all(&d);
    });
    let dir = tmp("store");
    let mut w = wer();
    w.root = Some(dir.clone());
    w.transport = None;
    w.mounted = false;
    w.process = 0;
    w.host_flags = 0;
    w.host_file.clear();
    w.enumerated = [false; 2];
    w.title = TEST_TITLE.lock().unwrap().clone();
    drop(w);
    Fx { _g: g, dir }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn transport_events(f: &Fx) -> Vec<String> {
    std::fs::read_to_string(f.dir.join("GameOS-crash0").join("transport.jsonl")).unwrap_or_default().lines().map(|l| l.to_string()).collect()
}

fn current_process() -> *mut c_void {
    -1isize as *mut c_void
}

#[test]
fn publisher_id_matches_windows() {
    assert_eq!(publisher_id("CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US"), "8wekyb3d8bbwe");
}

#[test]
fn max_process_hold() {
    assert_eq!(XerGetMaxProcessHoldMilliseconds(), 30000);
}

#[test]
fn should_wer_manage_root() {
    assert_eq!(XerShouldWerManageRootDirectory(), 0);
}

#[test]
fn encrypt_dump_only_kind_3() {
    assert_eq!(XerEncryptDump(core::ptr::null_mut(), 3), 1);
    assert_eq!(XerEncryptDump(core::ptr::null_mut(), 2), 0);
    assert_eq!(XerEncryptDump(core::ptr::null_mut(), 4), 0);
}

#[test]
fn machine_id_from_hypervisor_leaf() {
    unsafe {
        assert_eq!(XerGetMachineId(core::ptr::null_mut(), 1), E_INVALIDARG);
        let mut g = [0u8; 16];
        assert_eq!(XerGetMachineId(&mut g, 0x1122_3344), S_OK);
        let r = core::arch::x86_64::__cpuid_count(0x4000_000a, 0);
        assert_eq!(&g[0..4], &0x1122_3344u32.to_le_bytes());
        assert_eq!(u16::from_le_bytes([g[4], g[5]]), (r.ecx >> 16) as u16);
        assert_eq!(u16::from_le_bytes([g[6], g[7]]), r.ecx as u16);
        assert_eq!(&g[8..12], &r.ebx.to_be_bytes());
        assert_eq!(&g[12..16], &r.eax.to_be_bytes());
    }
}

#[test]
fn machine_store_path() {
    let f = fx();
    unsafe {
        let mut buf = [0u16; STORE_BYTES / 2];
        assert_eq!(XerGetMachineStorePath(buf.as_mut_ptr()), S_OK);
        let n = buf.iter().position(|c| *c == 0).unwrap();
        assert_eq!(String::from_utf16_lossy(&buf[..n]), f.dir.join("GameOS-crash0").to_string_lossy());
        wer().root = Some(PathBuf::from("x".repeat(300)));
        assert_eq!(XerGetMachineStorePath(buf.as_mut_ptr()), E_MORE_DATA);
    }
}

#[test]
fn package_identity_lengths() {
    let _f = fx();
    unsafe {
        assert_eq!(XerGetPackageIdentity(core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()), APPMODEL_E_NO_PACKAGE);
        let full = "Test.Game_1.2.3.0_x64__8wekyb3d8bbwe";
        let fam = "Test.Game_8wekyb3d8bbwe";
        let mut fl = 0u32;
        let mut ml = 0u32;
        assert_eq!(XerGetPackageIdentity(current_process(), &mut fl, core::ptr::null_mut(), &mut ml, core::ptr::null_mut()), E_BUFFER_OVERFLOW);
        assert_eq!(fl as usize, full.len() + 1);
        assert_eq!(ml as usize, fam.len() + 1);
        let mut fb = [0u16; 128];
        let mut mb = [0u16; 128];
        fl = full.len() as u32 + 1;
        assert_eq!(XerGetPackageIdentity(current_process(), &mut fl, fb.as_mut_ptr(), &mut ml, mb.as_mut_ptr()), E_BUFFER_OVERFLOW);
        fl = 128;
        ml = fam.len() as u32 + 1;
        assert_eq!(XerGetPackageIdentity(current_process(), &mut fl, fb.as_mut_ptr(), &mut ml, mb.as_mut_ptr()), S_OK);
        assert_eq!(String::from_utf16_lossy(&fb[..full.len()]), full);
        assert_eq!(fb[full.len()], 0);
        assert_eq!(String::from_utf16_lossy(&mb[..fam.len()]), fam);
        assert_eq!(mb[fam.len()], 0);
    }
}

#[test]
fn dump_notify_start_creates_store() {
    let f = fx();
    unsafe {
        XerDumpNotifyStart(current_process());
        let s = f.dir.join("GameOS-crash0");
        for d in ["Temp", "ReportQueue", "ReportArchive"] {
            assert!(s.join(d).is_dir());
        }
        assert!(s.join("WerXvdCreated").is_file());
        let w = wer();
        assert!(w.mounted);
        assert_eq!(w.transport, Some(TRANSPORT_MOUNTED));
    }
}

#[test]
fn dump_notify_complete_releases() {
    let _f = fx();
    unsafe {
        XerDumpNotifyStart(current_process());
        XerDumpNotifyComplete();
        let w = wer();
        assert!(!w.mounted);
        assert_eq!(w.transport, None);
    }
}

#[test]
fn tp_upload_sends_and_returns_not_impl() {
    let f = fx();
    unsafe { XerDumpNotifyStart(current_process()) };
    assert_eq!(XerTpUpload(), E_NOTIMPL);
    assert_eq!(wer().transport, Some(TRANSPORT_UPLOAD));
    let ev = transport_events(&f);
    assert_eq!(ev.len(), 2);
    assert!(ev[1].ends_with("\"event\":0}"));
}

#[test]
fn transport_finish_only_after_heap_dump() {
    let f = fx();
    unsafe { XerDumpNotifyStart(current_process()) };
    XerTransportEventFinish();
    assert_eq!(transport_events(&f).len(), 1);
    wer().host_flags = HOST_HEAP_DUMP;
    assert_eq!(XerProcessHeapDumpRequest(), S_OK);
    XerTransportEventFinish();
    assert_eq!(wer().transport, Some(TRANSPORT_FINISH));
    assert_eq!(transport_events(&f).len(), 3);
}

#[test]
fn heap_dump_request_aborts_without_request() {
    let _f = fx();
    assert_eq!(XerProcessHeapDumpRequest(), E_ABORT);
    assert_eq!(wer().transport, None);
}

#[test]
fn tp_complete_upload_is_inert() {
    let _f = fx();
    XerTpCompleteUpload();
    assert_eq!(wer().transport, None);
}

#[test]
fn transport_event_begin_is_inert() {
    let _f = fx();
    XerTransportEventBegin();
    assert_eq!(wer().transport, None);
}

#[test]
fn transport_event_upload_complete_is_inert() {
    let _f = fx();
    XerTransportEventUploadComplete();
    assert_eq!(wer().transport, None);
}

#[test]
fn transport_event_upload_progress_is_inert() {
    let _f = fx();
    XerTransportEventUploadProgress();
    assert_eq!(wer().transport, None);
}

#[test]
fn transport_event_upload_start_is_inert() {
    let _f = fx();
    XerTransportEventUploadStart();
    assert_eq!(wer().transport, None);
}

#[test]
fn escape_and_element() {
    assert_eq!(escape("a&b<c>'\""), "a&amp;b&lt;c&gt;&apos;&quot;");
    assert_eq!(element(1, "X", Ok("v".into())), "\r\n\t<X>v</X>");
    assert_eq!(element(2, "X", Err(E_NOT_FOUND)), "\r\n\t\t<X hr=\"0x80070002\"/>");
}

#[test]
fn write_metadata_file() {
    let f = fx();
    unsafe {
        assert_eq!(XerWriteMetadata(core::ptr::null_mut(), -1isize as *mut c_void), E_INVALIDARG);
        std::fs::create_dir_all(&f.dir).unwrap();
        let p = f.dir.join("meta.xml");
        {
            let file = std::fs::File::create(&p).unwrap();
            assert_eq!(XerWriteMetadata(core::ptr::null_mut(), file.as_raw_handle() as *mut c_void), S_OK);
        }
        let b = std::fs::read(&p).unwrap();
        assert_eq!(&b[..2], &[0xff, 0xfe]);
        let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let s = String::from_utf16_lossy(&u);
        assert_eq!(XML_HEADER.encode_utf16().count() * 2, 0x50);
        assert!(s.starts_with("\u{feff}<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n<XboxInformation>\r\n\t<ConsoleID>"));
        assert!(s.contains(&format!("\r\n\t<SandboxID>{}</SandboxID>", compat_common::gdk::system::sandbox_id())));
        assert!(s.contains("\r\n\t<PackageIdentity>Test.Game_1.2.3.0_x64__8wekyb3d8bbwe</PackageIdentity>"));
        assert!(s.ends_with("\r\n</XboxInformation>"));
        let ro = std::fs::OpenOptions::new().read(true).open(&p).unwrap();
        assert_eq!(XerWriteMetadata(core::ptr::null_mut(), ro.as_raw_handle() as *mut c_void), E_ACCESSDENIED);
    }
}

#[test]
fn enumerate_report_files_once() {
    let _f = fx();
    unsafe {
        let ev: Vec<u16> = "AppHangB1\0".encode_utf16().collect();
        let mut buf = [0u16; 64];
        assert_eq!(XerEnumerateReportFiles(ev.as_ptr(), buf.as_mut_ptr(), 64), S_FALSE);
        assert_eq!(XerEnumerateReportFiles(ev.as_ptr(), buf.as_mut_ptr(), 64), E_NO_MORE_FILES);
        wer().enumerated = [false; 2];
        set_host_response(HOST_HANG_FILE, "D:\\hang.dmp");
        assert_eq!(XerEnumerateReportFiles(ev.as_ptr(), buf.as_mut_ptr(), 64), S_OK);
        assert_eq!(String::from_utf16_lossy(&buf[..11]), "D:\\hang.dmp");
        assert_eq!(buf[11], 0);
        wer().enumerated = [false; 2];
        assert_eq!(XerEnumerateReportFiles(ev.as_ptr(), buf.as_mut_ptr(), 4), STRSAFE_E_INSUFFICIENT_BUFFER);
        assert_eq!(buf[3], 0);
        wer().enumerated = [false; 2];
        let other: Vec<u16> = "APPCRASH\0".encode_utf16().collect();
        assert_eq!(XerEnumerateReportFiles(other.as_ptr(), buf.as_mut_ptr(), 64), S_FALSE);
        wer().enumerated = [false; 2];
        assert_eq!(XerEnumerateReportFiles(ev.as_ptr(), buf.as_mut_ptr(), 0), E_INVALIDARG);
    }
}

#[test]
fn enumerate_report_metadata_once() {
    let _f = fx();
    unsafe {
        let mut k = [0u16; 32];
        let mut v = [0u16; 64];
        let hr = XerEnumerateReportMetadata(core::ptr::null(), k.as_mut_ptr(), 32, v.as_mut_ptr(), 64);
        match os_version_text() {
            Ok(t) => {
                assert_eq!(hr, S_OK);
                assert_eq!(String::from_utf16_lossy(&k[..12]), "SRAOSVersion");
                assert_eq!(String::from_utf16_lossy(&v[..t.len()]), t);
            }
            Err(e) => assert_eq!(hr, e),
        }
        assert_eq!(XerEnumerateReportMetadata(core::ptr::null(), k.as_mut_ptr(), 32, v.as_mut_ptr(), 64), E_NO_MORE_FILES);
    }
}

#[test]
fn cch_copy_strsafe_rules() {
    unsafe {
        let mut b = [7u16; 4];
        assert_eq!(cch_copy(b.as_mut_ptr(), 0, &[1]), E_INVALIDARG);
        assert_eq!(b[0], 7);
        assert_eq!(cch_copy(b.as_mut_ptr(), 4, &[1, 2, 3]), S_OK);
        assert_eq!(b, [1, 2, 3, 0]);
        assert_eq!(cch_copy(b.as_mut_ptr(), 2, &[5, 6]), STRSAFE_E_INSUFFICIENT_BUFFER);
        assert_eq!(&b[..2], &[5, 0]);
    }
}
