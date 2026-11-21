include!("util.rs");

fn rd(b: &[u8], o: usize) -> usize {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize
}

fn file_exports(bytes: &[u8]) -> Vec<(u16, String)> {
    let pe = rd(bytes, 0x3c);
    let nsec = u16::from_le_bytes(bytes[pe + 6..pe + 8].try_into().unwrap()) as usize;
    let opt = pe + 24;
    let sec = opt + u16::from_le_bytes(bytes[pe + 20..pe + 22].try_into().unwrap()) as usize;
    let off = |rva: usize| {
        (0..nsec)
            .map(|i| sec + 40 * i)
            .find(|s| rva >= rd(bytes, s + 12) && rva < rd(bytes, s + 12) + rd(bytes, s + 16).max(rd(bytes, s + 8)))
            .map(|s| rva - rd(bytes, s + 12) + rd(bytes, s + 20))
            .unwrap()
    };
    let ed = off(rd(bytes, opt + 112));
    let base = rd(bytes, ed + 16);
    let (names, nameptr, ords) = (rd(bytes, ed + 24), off(rd(bytes, ed + 32)), off(rd(bytes, ed + 36)));
    (0..names)
        .map(|i| {
            let n = off(rd(bytes, nameptr + 4 * i));
            let end = bytes[n..].iter().position(|c| *c == 0).unwrap();
            let o = u16::from_le_bytes(bytes[ords + 2 * i..ords + 2 * i + 2].try_into().unwrap()) as usize + base;
            (o as u16, String::from_utf8_lossy(&bytes[n..n + end]).into_owned())
        })
        .collect()
}

unsafe fn import_target(base: usize, dll: &str, func: &str) -> usize {
    let rd = |p: usize| (p as *const u32).read_unaligned() as usize;
    let cs = |p: usize| std::ffi::CStr::from_ptr(p as *const core::ffi::c_char).to_string_lossy().into_owned();
    let nt = base + rd(base + 0x3c);
    let mut d = base + rd(nt + 0x18 + 0x70 + 8);
    while rd(d + 12) != 0 {
        if cs(base + rd(d + 12)).eq_ignore_ascii_case(dll) {
            let (int, iat) = (base + rd(d), base + rd(d + 16));
            let mut i = 0;
            loop {
                let e = (int as *const u64).add(i).read_unaligned();
                if e == 0 {
                    break;
                }
                if e >> 63 == 0 && cs(base + e as usize + 2) == func {
                    return (iat as *const usize).add(i).read();
                }
                i += 1;
            }
        }
        d += 20;
    }
    0
}

#[test]
fn console_dll_loads_with_the_emulator_xmem() {
    let xm = emulator_xmem();
    let h = CONSOLE.module();
    assert!(h != 0, "console {} did not load: {:?}", CONSOLE.spec.name, CONSOLE.candidates());
    let p = module_path(h);
    assert!(p.ends_with(&CONSOLE.spec.name.to_lowercase()) && p.contains("embeddedxvd"), "{p}");
    for f in ["XMemAlloc", "XMemFree"] {
        let target = unsafe { import_target(h, "xmem.dll", f) };
        assert!(target != 0, "{f} not imported");
        assert_eq!(module_path(console_dll::own_module(target)), xm, "{f} resolved outside the emulator xmem.dll");
    }
    let fwd = CONSOLE.proc("XDXRCreateDevice").unwrap();
    assert_eq!(console_dll::own_module(fwd), h);
}

#[test]
fn console_dll_pairs_with_the_same_title_dxcompiler_xs() {
    emulator_xmem();
    assert!(CONSOLE.module() != 0);
    let dir = CONSOLE.path().unwrap().parent().unwrap().to_path_buf();
    let dxc = console_dll::dumps(std::path::Path::new(console_dll::GAMES), "dxcompiler_xs.dll");
    assert_eq!(dxc.first().and_then(|p| p.parent()), Some(dir.as_path()), "xdxr and dxcompiler_xs resolve to different titles");
}

#[test]
fn built_dll_exports_the_console_table() {
    let ours = file_exports(&std::fs::read(near_exe(CONSOLE.spec.name)).unwrap());
    emulator_xmem();
    assert!(CONSOLE.module() != 0);
    let console = file_exports(&std::fs::read(CONSOLE.path().unwrap()).unwrap());
    assert_eq!(console, vec![(1, "XDXRCreateDevice".to_string())]);
    assert_eq!(ours, console);
}
