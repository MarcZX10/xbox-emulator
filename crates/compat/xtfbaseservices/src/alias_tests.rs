use crate::targets::*;

const ALIASES: [(u16, &str, &str); 4] = [
    (7, "??1LoggingTargetBase@xtfbaseservices@@UEAA@XZ", "??1LoggingManager@xtfbaseservices@@UEAA@XZ"),
    (9, "??1LoggingTargetDebuggerAndConsole@xtfbaseservices@@UEAA@XZ", "??1LoggingManager@xtfbaseservices@@UEAA@XZ"),
    (10, "??1NonCopyableDLLBase@xtfbaseservices@@UEAA@XZ", "??1LoggingManager@xtfbaseservices@@UEAA@XZ"),
    (15, "??_7NonCopyableDLLBase@xtfbaseservices@@6B@", "??_7LoggingManager@xtfbaseservices@@6B@"),
];

fn rd16(b: &[u8], o: usize) -> usize {
    u16::from_le_bytes([b[o], b[o + 1]]) as usize
}

fn rd32(b: &[u8], o: usize) -> usize {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize
}

fn file_off(b: &[u8], rva: usize) -> usize {
    let pe = rd32(b, 0x3c);
    let n = rd16(b, pe + 6);
    let sh = pe + 0x18 + rd16(b, pe + 0x14);
    for i in 0..n {
        let s = sh + i * 0x28;
        let (va, vs, raw) = (rd32(b, s + 12), rd32(b, s + 8).max(rd32(b, s + 16)), rd32(b, s + 20));
        if rva >= va && rva < va + vs {
            return rva - va + raw;
        }
    }
    panic!("rva {rva:#x} outside sections");
}

fn export_rvas(b: &[u8]) -> std::collections::HashMap<String, usize> {
    let pe = rd32(b, 0x3c);
    let ed = file_off(b, rd32(b, pe + 0x18 + 0x70));
    let (nfun, nname) = (rd32(b, ed + 0x14), rd32(b, ed + 0x18));
    let (funs, names, ords) = (file_off(b, rd32(b, ed + 0x1c)), file_off(b, rd32(b, ed + 0x20)), file_off(b, rd32(b, ed + 0x24)));
    let mut m = std::collections::HashMap::new();
    for i in 0..nname {
        let p = file_off(b, rd32(b, names + i * 4));
        let end = b[p..].iter().position(|&c| c == 0).unwrap();
        let o = rd16(b, ords + i * 2);
        assert!(o < nfun);
        m.insert(String::from_utf8_lossy(&b[p..p + end]).into_owned(), rd32(b, funs + o * 4));
    }
    m
}

#[test]
fn alias_rows_link_to_their_target_symbol() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/exports.txt")).unwrap();
    let ex = xtfcommon::exports::parse(&text);
    let link = xtfcommon::exports::link_args(&text);
    for (ord, name, target) in ALIASES {
        let e = ex.iter().find(|e| e.ordinal == ord).unwrap();
        assert_eq!(e.name, name);
        assert_eq!(xtfcommon::exports::internal_symbol(e), target);
        let data = if e.data { ",DATA" } else { "" };
        assert_eq!(e.data, name.starts_with("??_7"), "{name} data flag");
        let want = format!("/EXPORT:{name}={target},@{ord}{data}");
        assert!(link.contains(&want), "{want} not in link args");
    }
}

#[test]
fn console_dll_exports_each_alias_at_its_target_address() {
    let refs = xtfcommon::pe::title_dlls("xtfbaseservices.dll");
    assert!(!refs.is_empty(), "no console xtfbaseservices.dll in the title dumps");
    for (title, p) in refs {
        let m = export_rvas(&std::fs::read(&p).unwrap());
        for (_, name, target) in ALIASES {
            assert_eq!(m.get(name), m.get(target), "{title}: {name} vs {target}");
            assert!(m.contains_key(name), "{title}: {name} not exported");
        }
    }
}

#[test]
fn shared_destructor_restores_the_shared_vtable() {
    unsafe {
        for start in [core::ptr::null(), &BASE_VTBL as *const TargetVtbl as *const NcVtbl] {
            let mut o = NcObject { vtbl: start };
            nc_dtor(&mut o);
            assert_eq!(o.vtbl, &NC_VTBL_LM as *const NcVtbl);
            nc_dtor(&mut o);
            assert_eq!(o.vtbl, &NC_VTBL_LM as *const NcVtbl);
        }
    }
}
