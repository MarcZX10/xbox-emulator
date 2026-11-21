use console_sandbox::*;

#[link(name = "kernel32")]
extern "system" {
    fn AddVectoredExceptionHandler(first: u32, h: unsafe extern "system" fn(*mut *mut u8) -> i32) -> usize;
    fn GetModuleHandleExW(flags: u32, addr: usize, out: *mut usize) -> i32;
}

unsafe extern "system" fn veh(p: *mut *mut u8) -> i32 {
    let rec = *p;
    let ctx = *p.add(1);
    let code = *(rec as *const u32);
    if code != 0x406D1388 && code != 0x40010006 && code != 0x4001000A {
        let addr = *(rec.add(16) as *const usize);
        let mut m = 0usize;
        GetModuleHandleExW(6, addr, &mut m);
        let rsp = *(ctx.add(0x98) as *const usize);
        let mut frames = String::new();
        for i in 0..160 {
            let v = *((rsp + i * 8) as *const usize);
            let mut mm = 0usize;
            if v > 0x10000 && GetModuleHandleExW(6, v, &mut mm) != 0 {
                frames.push_str(&format!(" {}+{:#x}", console_sandbox::modules::module_file(mm).and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned())).unwrap_or_default(), v - mm));
            }
        }
        if code == 0xe06d7363 { let o = *(rec.add(0x28) as *const usize); eprintln!("OBJ {:x?}", core::slice::from_raw_parts(o as *const u32, 12)); }
        eprintln!("EXC {code:#x} at {:?}+{:#x} rcx={:#x} rdx={:#x} info={:#x} stack:{frames}", console_sandbox::modules::module_file(m), addr - m, *(ctx.add(0x80) as *const usize), *(ctx.add(0x88) as *const usize), *(rec.add(0x28) as *const usize));
    }
    0
}
#[test]
fn probe() {
    unsafe { AddVectoredExceptionHandler(1, veh) };
    let dir = std::env::temp_dir().join(format!("tombox-probe-{}", std::process::id()));
    std::env::set_var("TOMBOX_GDK_ROOT", &dir);
    println!("seeded {}", hive::seed_software(r"Microsoft\Windows NT\CurrentVersion\BackgroundModel"));
    let d = r"D:\xbox-pc-port\games\Fortnite\extracted\EmbeddedXvd\Windows\System32";
    let m = console_image::load_path(&std::path::Path::new(d).join("TimeBrokerServer.dll"));
    assert_ne!(m, 0);
    let hits = unsafe { attach(m) };
    println!("hits {}", hits.len());
    host::add_service("TimeBrokerSvc");
    let push: unsafe extern "system" fn(*const u8) = unsafe { core::mem::transmute(console_image::symbol(m, "SvchostPushServiceGlobals").unwrap()) };
    unsafe { push(host::legacy_globals()) };
    let main: unsafe extern "system" fn(u32, *const *const u16) = unsafe { core::mem::transmute(console_image::symbol(m, "ServiceMain").unwrap()) };
    let name = wstr::wide("TimeBrokerSvc");
    let argv = [name.as_ptr()];
    unsafe { main(1, argv.as_ptr()) };
    println!("history {:?}", host::service("TimeBrokerSvc").map(|s| s.history));
    println!("rpc {:?}", rpc::log());
    println!("eps {:?}", rpc::entries());
    println!("power {:?}", power::registrations());
}
