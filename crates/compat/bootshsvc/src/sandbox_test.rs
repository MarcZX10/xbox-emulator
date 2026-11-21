use crate::{console_module, sandbox_hits, ServiceMain, SERVICE_NAME};
use console_sandbox::{host, registry, system};

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(h: usize, sub: *const u16, opt: u32, sam: u32, out: *mut usize) -> u32;
    fn RegCloseKey(h: usize) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetComputerNameExW(kind: u32, buf: *mut u16, size: *mut u32) -> i32;
}

fn pc_key_exists(path: &str) -> bool {
    let w: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let mut h = 0usize;
    let ok = unsafe { RegOpenKeyExW(registry::HKEY_LOCAL_MACHINE, w.as_ptr(), 0, 0x20019, &mut h) == 0 };
    if ok {
        unsafe { RegCloseKey(h) };
    }
    ok
}

fn pc_hostname() -> String {
    let mut b = [0u16; 64];
    let mut n = 64u32;
    unsafe { GetComputerNameExW(0, b.as_mut_ptr(), &mut n) };
    String::from_utf16_lossy(&b[..n as usize])
}

pub fn run_console_bootsh(console: &str) {
    let dir = std::env::temp_dir().join(format!("tombox-bootsh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("TOMBOX_GDK_ROOT", &dir);
    std::env::set_var("TOMBOX_BOOTSHSVC_CONSOLE", console);
    assert!(std::path::Path::new(console).is_file(), "{console}");
    let m = console_module();
    assert_ne!(m, 0, "console bootshsvc.dll loads");
    let hits = sandbox_hits();
    for f in ["NtCreateKey", "NtSetValueKey", "RegOpenKeyExW", "CreateFileW", "CreateProcessW", "GetComputerNameExW", "SetServiceStatus", "RegisterServiceCtrlHandlerExW"] {
        assert!(hits.iter().any(|h| h.ends_with(&format!("!{f}"))), "{f} routed to the sandbox: {hits:?}");
    }
    assert_eq!(unsafe { console_sandbox::unrouted(m) }, Vec::<String>::new(), "every console import is sandboxed or a pure in-process call");
    let fs = dir.join("console").join("fs").join("C").join("Windows");
    let boot = fs.join("System32").join("boot");
    std::fs::create_dir_all(&boot).unwrap();
    let script = "\u{feff}sethostname TESTBOX\r\ncopy c:\\windows\\system32\\boot\\a.txt c:\\windows\\b.txt\r\n";
    std::fs::write(boot.join("startup.bsc"), script.encode_utf16().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
    std::fs::write(boot.join("a.txt"), b"hello").unwrap();
    let pc_had_bootsh = pc_key_exists(r"SYSTEM\CurrentControlSet\Services\Bootsh");
    let pc_host = pc_hostname();
    let name: Vec<u16> = SERVICE_NAME.encode_utf16().chain(Some(0)).collect();
    let argv = [name.as_ptr()];
    unsafe { ServiceMain(1, argv.as_ptr()) };
    let t = std::time::Instant::now();
    while t.elapsed() < std::time::Duration::from_secs(10) {
        if host::service(SERVICE_NAME).map(|s| s.status[1] == host::SERVICE_STOPPED && s.stop_waits == 0).unwrap_or(false) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let states: Vec<u32> = host::service(SERVICE_NAME).unwrap().history.iter().map(|s| s[1]).collect();
    assert_eq!(states, vec![2, 4, 3, 1], "START_PENDING, RUNNING, STOP_PENDING, STOPPED through the in-process host");
    for (k, v) in [(system::ACTIVE_COMPUTER_NAME, "ComputerName"), (system::COMPUTER_NAME, "ComputerName"), (system::TCPIP_PARAMETERS, "Hostname"), (system::TCPIP_PARAMETERS, "NV Hostname")] {
        assert_eq!(registry::get_string(k, v).as_deref(), Some("TESTBOX"), "{k}\\{v}");
    }
    assert_eq!(system::hostname(0).as_deref(), Some("TESTBOX"));
    assert_eq!(system::env("COMPUTERNAME").as_deref(), Some("TESTBOX"));
    assert_eq!(std::fs::read(fs.join("b.txt")).unwrap(), b"hello");
    assert!(std::fs::read_to_string(dir.join("console").join("registry.txt")).unwrap().contains("ActiveComputerName"), "sandbox registry persisted under the data dir");
    assert_eq!(pc_key_exists(r"SYSTEM\CurrentControlSet\Services\Bootsh"), pc_had_bootsh, "PC registry untouched");
    assert_eq!(pc_hostname(), pc_host, "PC hostname untouched");
    assert_ne!(std::env::var("COMPUTERNAME").ok().as_deref(), Some("TESTBOX"), "emulator process environment untouched");
}
