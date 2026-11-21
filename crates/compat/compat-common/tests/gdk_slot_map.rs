use compat_common::gdk;

extern "system" {
    fn GetModuleHandleW(name: *const u16) -> usize;
}

#[test]
fn dump_slot_functions() {
    let base = unsafe { GetModuleHandleW(core::ptr::null()) };
    let rows = gdk::slot_functions();
    assert!(!rows.is_empty());
    let mut out = String::from("[\n");
    for (i, (c, iid, name, slot, f)) in rows.iter().enumerate() {
        let rva = f.wrapping_sub(base);
        out.push_str(&format!(
            "{{\"clsid\":\"{}\",\"iid\":\"{}\",\"name\":\"{}\",\"slot\":{},\"rva\":\"0x{:x}\"}}{}\n",
            c.text(),
            iid.text(),
            name,
            slot,
            rva,
            if i + 1 == rows.len() { "" } else { "," }
        ));
    }
    out.push(']');
    if let Ok(p) = std::env::var("GDK_SLOT_MAP_OUT") {
        std::fs::write(&p, &out).unwrap();
        let exe = std::env::current_exe().unwrap();
        std::fs::write(format!("{p}.exe.txt"), exe.to_string_lossy().as_bytes()).unwrap();
    }
}
