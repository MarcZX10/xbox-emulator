use super::*;
use std::path::PathBuf;
use std::sync::Mutex as M;

static SERIAL: M<()> = M::new(());

pub(crate) fn guard() -> std::sync::MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    reset_state();
    g
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("symsrv-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn wz(s: &str) -> Vec<u16> {
    z(&w(s))
}

fn ps(p: &std::path::Path) -> String {
    p.to_string_lossy().to_string()
}

fn out_str(v: &[u16]) -> String {
    String::from_utf16_lossy(unsafe { wstr(v.as_ptr()) })
}

fn makecab(src: &std::path::Path, dst: &std::path::Path) {
    let st = std::process::Command::new("makecab.exe").arg(src).arg(dst).output().unwrap();
    assert!(st.status.success());
}

fn put(p: &std::path::Path, data: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, data).unwrap();
}

#[test]
fn set_options_tracks_bits_and_side_state() {
    let _g = guard();
    unsafe {
        assert_eq!(SymbolServerSetOptionsW(SSRVOPT_UNATTENDED as usize, 1), 1);
        assert_eq!(SymbolServerGetOptions() as u64 & SSRVOPT_UNATTENDED, SSRVOPT_UNATTENDED);
        assert_eq!(SymbolServerSetOptionsW(SSRVOPT_UNATTENDED as usize, 0), 1);
        assert_eq!(SymbolServerGetOptions() as u64 & SSRVOPT_UNATTENDED, 0);
        SetLastError(0);
        assert_eq!(SymbolServerSetOptionsW(SSRVOPT_PARAMTYPE as usize, 3), 0);
        assert_eq!(GetLastError(), 87);
        assert_eq!(SymbolServerSetOptionsW(SSRVOPT_PARAMTYPE as usize, SSRVOPT_GUIDPTR), 1);
        assert_eq!(param_type(), SSRVOPT_GUIDPTR);
        assert_eq!(options() & TYPE_BITS, SSRVOPT_GUIDPTR);
        assert_eq!(SymbolServerSetOptionsW(SSRVOPT_GUIDPTR as usize, 0), 1);
        assert_eq!(param_type(), SSRVOPT_DWORD);
        assert_eq!(options() & TYPE_BITS, SSRVOPT_DWORD);
        assert_eq!(SymbolServerSetOptionsW(SSRVOPT_STRING as usize, 1), 1);
        assert_eq!(param_type(), SSRVOPT_STRING);
        let d = b"c:\\down\0";
        assert_eq!(SymbolServerSetOptions(SSRVOPT_DOWNSTREAM_STORE as usize, d.as_ptr() as u64), 1);
        assert_eq!(String::from_utf16_lossy(&downstream()), "c:\\down");
        let p = wz("proxy:8080");
        assert_eq!(SymbolServerSetOptionsW(SSRVOPT_PROXYW as usize, p.as_ptr() as u64), 1);
        assert_eq!(String::from_utf16_lossy(&STATE.lock().unwrap().proxy), "proxy:8080");
        assert_eq!(SymbolServerSetOptionsW(SSRVOPT_DISABLE_PING_HOST as usize, 0), 1);
        assert!(STATE.lock().unwrap().ping_disabled);
        assert_eq!(SymbolServerSetOptionsW(SSRVOPT_SETCONTEXT as usize, 0x55), 1);
        assert_eq!(context(), 0x55);
        assert_eq!(std::thread::spawn(context).join().unwrap(), 0x55);
        assert_eq!(options() & SSRVOPT_SETCONTEXT, 0);
        assert_eq!(SymbolServerSetOptionsW(SSRVOPT_PARENTWIN as usize, 0x1234), 1);
        assert_eq!(STATE.lock().unwrap().parent, 0x1234);
    }
}

#[test]
fn version_reports_console_api_and_rejects_null() {
    let _g = guard();
    unsafe {
        SetLastError(0);
        assert_eq!(SymbolServerGetVersion(core::ptr::null_mut()), 0);
        assert_eq!(GetLastError(), 87);
        let mut v = [0u32; 2];
        assert_eq!(SymbolServerGetVersion(v.as_mut_ptr()), 1);
        assert_eq!(v, [0xa, 0x2908]);
    }
}

#[test]
fn index_strings_follow_param_type() {
    let _g = guard();
    unsafe {
        assert_eq!(String::from_utf16_lossy(&index_string(0x5f3e7a10usize as *const u8, 0x1b000, 0, SSRVOPT_DWORD)), "5f3e7a101b000");
        let dw = 0xabcu32;
        assert_eq!(String::from_utf16_lossy(&index_string(&dw as *const u32 as *const u8, 0, 2, SSRVOPT_DWORDPTR)), "abc2");
        let g: [u8; 16] = [0x78, 0x56, 0x34, 0x12, 0xbc, 0x9a, 0xf0, 0xde, 1, 2, 3, 4, 5, 6, 7, 0x88];
        assert_eq!(String::from_utf16_lossy(&index_string(g.as_ptr(), 3, 0, SSRVOPT_GUIDPTR)), "123456789ABCDEF001020304050607883");
        assert_eq!(String::from_utf16_lossy(&index_string(g.as_ptr(), 1, 0, SSRVOPT_OLDGUIDPTR)), "123456789abcdef0 1 2 3 4 5 6 7881");
        let only = [0x01u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(String::from_utf16_lossy(&index_string(only.as_ptr(), 0, 0, SSRVOPT_GUIDPTR)), "00000001");
        let s = wz("MyIndex");
        assert_eq!(String::from_utf16_lossy(&index_string(s.as_ptr() as *const u8, 0xa, 0, SSRVOPT_STRING)), "MyIndexa");
        assert_eq!(String::from_utf16_lossy(&index_string(core::ptr::null(), 0, 0, 0)), "");
        SymbolServerSetOptionsW(SSRVOPT_PARAMTYPE as usize, SSRVOPT_DWORD);
        let mut o = [0u16; 32];
        assert_eq!(SymbolServerGetIndexStringW(0x10usize as *const u8, 0x20, 0, o.as_mut_ptr(), 32), 1);
        assert_eq!(out_str(&o), "1020");
        let mut a = [0u8; 32];
        assert_eq!(SymbolServerGetIndexString(0x10usize as *const u8, 0x20, 0, a.as_mut_ptr(), 32), 1);
        assert_eq!(&a[..5], b"1020\0");
        SetLastError(0);
        assert_eq!(SymbolServerGetIndexStringW(0x10usize as *const u8, 0x20, 0, o.as_mut_ptr(), 3), 0);
        assert_eq!(GetLastError(), 122);
        assert_eq!(SymbolServerGetIndexStringW(0x10usize as *const u8, 0, 0, core::ptr::null_mut(), 3), 0);
        assert_eq!(GetLastError(), 87);
    }
}

#[test]
fn delta_name_joins_both_indexes() {
    let _g = guard();
    unsafe {
        SymbolServerSetOptionsW(SSRVOPT_PARAMTYPE as usize, SSRVOPT_DWORD);
        let n = wz("ntdll.dll");
        let mut o = [0u16; 64];
        assert_eq!(SymbolServerDeltaNameW(n.as_ptr(), 0x11usize as *const u8, 0x22, 0, 0x33usize as *const u8, 0x44, 0, o.as_mut_ptr(), 64), 1);
        assert_eq!(out_str(&o), "1122-3344.ntdll.dll");
        let mut a = [0u8; 64];
        assert_eq!(SymbolServerDeltaName(b"a.dll\0".as_ptr(), 1usize as *const u8, 0, 0, 2usize as *const u8, 0, 0, a.as_mut_ptr(), 64), 1);
        assert_eq!(&a[..10], b"1-2.a.dll\0");
        SetLastError(0);
        assert_eq!(SymbolServerDeltaNameW(wz("").as_ptr(), 1usize as *const u8, 0, 0, 2usize as *const u8, 0, 0, o.as_mut_ptr(), 64), 0);
        assert_eq!(GetLastError(), 87);
    }
}

#[test]
fn params_split_and_classify() {
    let v = split_params(&w(" c:\\down *http://msdl.microsoft.com/download/symbols* \\\\srv\\s "));
    let s: Vec<String> = v.iter().map(|x| String::from_utf16_lossy(x)).collect();
    assert_eq!(s, ["c:\\down", "http://msdl.microsoft.com/download/symbols", "\\\\srv\\s"]);
    let many = split_params(&w(&"a*".repeat(20)));
    assert_eq!(many.len(), 12);
    assert_eq!(String::from_utf16_lossy(&many[11]), "a*a*a*a*a*a*a*a*a*");
    assert_eq!(classify(&w("HTTP://x")), 0);
    assert_eq!(classify(&w("https://x")), 1);
    assert_eq!(classify(&w("c:\\x")), 2);
}

#[test]
fn lookup_copies_down_from_upstream_file_store() {
    let _g = guard();
    let d = tmp("copydown");
    let up = d.join("up");
    let down = d.join("down");
    put(&up.join("a.pdb").join("ABC1").join("a.pdb"), b"symbols");
    unsafe {
        SymbolServerSetOptionsW(SSRVOPT_PARAMTYPE as usize, SSRVOPT_DWORD);
        let params = wz(&format!("{}*{}", ps(&down), ps(&up)));
        let f = wz("a.pdb");
        let mut out = [0u16; MAX_PATH_W];
        assert_eq!(SymbolServerW(params.as_ptr(), f.as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 1);
        let got = out_str(&out);
        assert_eq!(got, ps(&down.join("a.pdb").join("abc1").join("a.pdb")));
        assert_eq!(std::fs::read(&got).unwrap(), b"symbols");
        assert!(!down.join("a.pdb").join("abc1").join("download.error").exists());
        let mut ao = [0u8; MAX_PATH_W];
        let pa = format!("{}*{}\0", ps(&down), ps(&up));
        assert_eq!(SymbolServer(pa.as_ptr(), b"a.pdb\0".as_ptr(), 0xabcusize as *const u8, 1, 0, ao.as_mut_ptr()), 1);
        SymbolServerSetOptionsW(SSRVOPT_NOCOPY as usize, 1);
        let p2 = wz(&ps(&up));
        assert_eq!(SymbolServerW(p2.as_ptr(), f.as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 1);
        assert_eq!(out_str(&out), ps(&up.join("a.pdb").join("abc1").join("a.pdb")));
        SymbolServerSetOptionsW(SSRVOPT_NOCOPY as usize, 0);
        SetLastError(0);
        assert_eq!(SymbolServerW(params.as_ptr(), f.as_ptr(), 0x999usize as *const u8, 1, 0, out.as_mut_ptr()), 0);
        assert_eq!(GetLastError(), 2);
        assert_eq!(out[0], 0);
        assert_eq!(SymbolServerW(params.as_ptr(), f.as_ptr(), core::ptr::null(), 0, 0, out.as_mut_ptr()), 0);
        assert_eq!(GetLastError(), 87);
        let long = wz(&"x".repeat(0x65));
        assert_eq!(SymbolServerW(params.as_ptr(), long.as_ptr(), 1usize as *const u8, 0, 0, out.as_mut_ptr()), 0);
        assert_eq!(GetLastError(), 87);
        let idx = wz("abc1");
        let mut o2 = [0u16; MAX_PATH_W];
        assert_eq!(SymbolServerByIndexW(params.as_ptr(), idx.as_ptr(), f.as_ptr(), o2.as_mut_ptr()), 1);
        assert_eq!(out_str(&o2), got);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn lookup_expands_compressed_files_unless_told_not_to() {
    let _g = guard();
    let d = tmp("compressed");
    let up = d.join("up");
    let down = d.join("down");
    let src = d.join("b.pdb");
    std::fs::write(&src, b"compressed symbols payload").unwrap();
    let slot = up.join("b.pdb").join("F00D1");
    std::fs::create_dir_all(&slot).unwrap();
    makecab(&src, &slot.join("b.pd_"));
    unsafe {
        SymbolServerSetOptionsW(SSRVOPT_PARAMTYPE as usize, SSRVOPT_DWORD);
        let params = wz(&format!("{}*{}", ps(&down), ps(&up)));
        let f = wz("b.pdb");
        let mut out = [0u16; MAX_PATH_W];
        assert_eq!(SymbolServerW(params.as_ptr(), f.as_ptr(), 0xf00dusize as *const u8, 1, 0, out.as_mut_ptr()), 1);
        let got = out_str(&out);
        assert!(got.ends_with("b.pdb"));
        assert_eq!(std::fs::read(&got).unwrap(), b"compressed symbols payload");
        std::fs::remove_dir_all(&down).unwrap();
        SymbolServerSetOptionsW(SSRVOPT_DONT_UNCOMPRESS as usize, 1);
        assert_eq!(SymbolServerW(params.as_ptr(), f.as_ptr(), 0xf00dusize as *const u8, 1, 0, out.as_mut_ptr()), 1);
        assert!(out_str(&out).ends_with("b.pd_"));
        SymbolServerSetOptionsW(SSRVOPT_DONT_UNCOMPRESS as usize, 0);
        let only = wz(&ps(&up));
        SetLastError(0);
        assert_eq!(SymbolServerW(only.as_ptr(), f.as_ptr(), 0xf00dusize as *const u8, 1, 0, out.as_mut_ptr()), 0);
        assert_eq!(GetLastError(), 2);
        let ds = wz(&ps(&d.join("ds")));
        SymbolServerSetOptionsW(SSRVOPT_DOWNSTREAM_STORE as usize, ds.as_ptr() as u64);
        assert_eq!(SymbolServerW(only.as_ptr(), f.as_ptr(), 0xf00dusize as *const u8, 1, 0, out.as_mut_ptr()), 1);
        assert_eq!(std::fs::read(out_str(&out)).unwrap(), b"compressed symbols payload");
        assert_eq!(SymbolServerClose(), 1);
        assert!(downstream().is_empty());
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn file_ptr_redirects_and_tier2_layout() {
    let _g = guard();
    let d = tmp("ptr");
    let up = d.join("up");
    std::fs::create_dir_all(&up).unwrap();
    std::fs::write(up.join("index2.txt"), b"").unwrap();
    let real = d.join("elsewhere.pdb");
    std::fs::write(&real, b"pointed").unwrap();
    put(&up.join("c.").join("c.pdb").join("1A").join("file.ptr"), format!("MSG: moved\r\nPATH:{}", ps(&real)).as_bytes());
    let mut s = Store::new(w(&ps(&up)));
    assert_eq!(s.tier(), 2);
    assert!(s.find(&w("c.pdb"), &w("1A")));
    assert_eq!(String::from_utf16_lossy(&s.found), ps(&real));
    assert_eq!(String::from_utf16_lossy(&s.msg), " moved");
    let mut t = Store::new(w(&ps(&up)));
    assert_eq!(String::from_utf16_lossy(&t.layout(&w("c.pdb"), &w("1A/B"))), ps(&up.join("c.").join("c.pdb").join("1A").join("B").join("c.pdb")));
    let mut r = Store::new(w("q:\\store"));
    assert!(matches!(r.parse_ptr(b"STORE:x"), store::Ptr::Other));
    assert!(matches!(r.parse_ptr(b"sub\\f.pdb"), store::Ptr::Path));
    assert_eq!(String::from_utf16_lossy(&r.found), "q:\\store\\sub\\f.pdb");
    let mut u: Vec<u8> = vec![0xff, 0xfe];
    for c in "\\\\srv\\x.pdb".encode_utf16() {
        u.extend_from_slice(&c.to_le_bytes());
    }
    assert!(matches!(r.parse_ptr(&u), store::Ptr::Path));
    assert_eq!(String::from_utf16_lossy(&r.found), "\\\\srv\\x.pdb");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn store_file_variants() {
    let _g = guard();
    let d = tmp("storefile");
    let st = d.join("store");
    let src = d.join("e.dll");
    std::fs::write(&src, b"image bytes").unwrap();
    unsafe {
        SymbolServerSetOptionsW(SSRVOPT_PARAMTYPE as usize, SSRVOPT_DWORD);
        let sp = wz(&ps(&st));
        let fp = wz(&ps(&src));
        let mut out = [0u16; MAX_PATH_W];
        assert_eq!(SymbolServerStoreFileW(sp.as_ptr(), fp.as_ptr(), 0x5eedusize as *const u8, 0x2000, 0, out.as_mut_ptr(), MAX_PATH_W, 0), 1);
        let p = out_str(&out);
        assert_eq!(p, ps(&st.join("e.dll").join("5eed2000").join("e.dll")));
        assert_eq!(std::fs::read(&p).unwrap(), b"image bytes");
        assert!(st.join("pingme.txt").exists());
        SetLastError(0);
        assert_eq!(SymbolServerStoreFileW(sp.as_ptr(), fp.as_ptr(), 0x5eedusize as *const u8, 0x2000, 0, out.as_mut_ptr(), MAX_PATH_W, 0), 0);
        assert_eq!(GetLastError(), 0x50);
        assert_eq!(SymbolServerStoreFileW(sp.as_ptr(), fp.as_ptr(), 0x5eedusize as *const u8, 0x2000, 0, out.as_mut_ptr(), MAX_PATH_W, SYMSTOREOPT_PASS_IF_EXISTS), 1);
        assert_eq!(SymbolServerStoreFileW(sp.as_ptr(), fp.as_ptr(), 0x5eedusize as *const u8, 0x2000, 0, out.as_mut_ptr(), MAX_PATH_W, SYMSTOREOPT_OVERWRITE | SYMSTOREOPT_RETURNINDEX), 1);
        assert_eq!(out_str(&out), "5eed2000");
        assert_eq!(SymbolServerStoreFileW(sp.as_ptr(), fp.as_ptr(), 0x1usize as *const u8, 0, 0, out.as_mut_ptr(), MAX_PATH_W, SYMSTOREOPT_POINTER), 1);
        assert_eq!(std::fs::read(st.join("e.dll").join("1").join("file.ptr")).unwrap(), ps(&src).as_bytes());
        assert_eq!(SymbolServerStoreFileW(sp.as_ptr(), fp.as_ptr(), 0x2usize as *const u8, 0, 0, out.as_mut_ptr(), MAX_PATH_W, SYMSTOREOPT_COMPRESS), 1);
        let cab = st.join("e.dll").join("2").join("e.dl_");
        assert!(cab.exists());
        let back = d.join("back.dll");
        assert_eq!(cab::fdi_expand(&w(&ps(&cab)), &w(&ps(&back))), 0);
        assert_eq!(std::fs::read(&back).unwrap(), b"image bytes");
        let http = wz("http://symbols.example/s");
        SetLastError(0);
        assert_eq!(SymbolServerStoreFileW(http.as_ptr(), fp.as_ptr(), 1usize as *const u8, 0, 0, out.as_mut_ptr(), MAX_PATH_W, 0), 0);
        assert_eq!(GetLastError(), 87);
        let mut ao = [0u8; MAX_PATH_W];
        let sa = format!("{}\0", ps(&st));
        let fa = format!("{}\0", ps(&src));
        assert_eq!(SymbolServerStoreFile(sa.as_ptr(), fa.as_ptr(), 3usize as *const u8, 0, 0, ao.as_mut_ptr(), MAX_PATH_W, 0), 1);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn supplements_round_trip() {
    let _g = guard();
    let d = tmp("supp");
    let st = d.join("store");
    let src = d.join("notes.txt");
    std::fs::write(&src, b"supp").unwrap();
    unsafe {
        let sp = wz(&ps(&st));
        let node = wz("node1");
        let fp = wz(&ps(&src));
        let mut out = [0u16; MAX_PATH_W];
        assert_eq!(SymbolServerStoreSupplementW(sp.as_ptr(), node.as_ptr(), fp.as_ptr(), out.as_mut_ptr(), MAX_PATH_W, 0), 1);
        assert_eq!(out_str(&out), ps(&st.join("supplement").join("node1").join("notes.txt")));
        let mut g = [0u16; MAX_PATH_W];
        let name = wz("notes.txt");
        assert_eq!(SymbolServerGetSupplementW(sp.as_ptr(), node.as_ptr(), name.as_ptr(), g.as_mut_ptr(), MAX_PATH_W), 1);
        assert_eq!(out_str(&g), out_str(&out));
        SetLastError(0);
        let other = wz("missing.txt");
        assert_eq!(SymbolServerGetSupplementW(sp.as_ptr(), node.as_ptr(), other.as_ptr(), g.as_mut_ptr(), MAX_PATH_W), 0);
        assert_eq!(GetLastError(), 2);
        assert_eq!(SymbolServerStoreSupplementW(sp.as_ptr(), node.as_ptr(), wz("").as_ptr(), out.as_mut_ptr(), MAX_PATH_W, 0), 0);
        assert_eq!(GetLastError(), 87);
        let mut a = [0u8; MAX_PATH_W];
        let sa = format!("{}\0", ps(&st));
        assert_eq!(SymbolServerGetSupplement(sa.as_ptr(), b"node1\0".as_ptr(), b"notes.txt\0".as_ptr(), a.as_mut_ptr(), MAX_PATH_W), 1);
        let fa = format!("{}\0", ps(&src));
        assert_eq!(SymbolServerStoreSupplement(sa.as_ptr(), b"node2\0".as_ptr(), fa.as_ptr(), a.as_mut_ptr(), MAX_PATH_W, SYMSTOREOPT_COMPRESS), 1);
        assert!(st.join("supplement").join("node2").join("notes.tx_").exists());
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn is_store_and_ping() {
    let _g = guard();
    let d = tmp("ping");
    unsafe {
        let p = wz(&ps(&d));
        assert_eq!(SymbolServerIsStoreW(p.as_ptr()), 0);
        std::fs::write(d.join("pingme.txt"), b"").unwrap();
        assert_eq!(SymbolServerIsStoreW(p.as_ptr()), 1);
        assert_eq!(SymbolServerPingW(p.as_ptr()), 1);
        assert_eq!(SymbolServerIsStoreW(wz("a*b").as_ptr()), 1);
        assert_eq!(SymbolServerIsStoreW(wz("https://x/y").as_ptr()), 1);
        SetLastError(0);
        assert_eq!(SymbolServerIsStoreW(wz("").as_ptr()), 0);
        assert_eq!(GetLastError(), 87);
        let a = format!("{}\0", ps(&d));
        assert_eq!(SymbolServerIsStore(a.as_ptr()), 1);
        assert_eq!(SymbolServerPing(a.as_ptr()), 1);
        let h = wz("http://symbols.example/s");
        SetLastError(0);
        assert_eq!(SymbolServerPingW(h.as_ptr()), 0);
        assert_eq!(GetLastError(), http::OFFLINE_ERROR);
        assert_eq!(SymbolServerPingWEx(h.as_ptr()), 0);
        SymbolServerSetOptionsW(SSRVOPT_DISABLE_PING_HOST as usize, 1);
        assert_eq!(SymbolServerPingWEx(h.as_ptr()), 1);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn http_entry_points_validate_and_report_offline() {
    let _g = guard();
    unsafe {
        let mut hf = 5isize;
        let mut hs = 5isize;
        SetLastError(0);
        assert_eq!(http::httpOpenFileHandleW(wz("c:\\x").as_ptr(), wz("a.pdb").as_ptr(), 0, &mut hf, &mut hs), 0);
        assert_eq!(GetLastError(), 87);
        assert_eq!(http::httpOpenFileHandleW(core::ptr::null(), wz("notaurl").as_ptr(), 0, &mut hf, &mut hs), 0);
        assert_eq!(GetLastError(), 87);
        assert_eq!(http::httpOpenFileHandleW(wz("http://symbols.example").as_ptr(), wz("http://symbols.example/a.pdb/1/a.pdb").as_ptr(), 0, &mut hf, &mut hs), 0);
        assert_eq!(GetLastError(), http::OFFLINE_ERROR);
        assert_eq!((hf, hs), (0, 0));
        assert_eq!(http::httpOpenFileHandle(core::ptr::null(), b"https://symbols.example/a.pdb\0".as_ptr(), 0, &mut hf, &mut hs), 0);
        assert_eq!(GetLastError(), http::OFFLINE_ERROR);
        let (site, path) = http::split_url(&w("https://h:81/a\\b/c")).unwrap();
        assert_eq!(String::from_utf16_lossy(&site), "https://h:81");
        assert_eq!(String::from_utf16_lossy(&path), "a/b/c");
        let (s2, p2) = http::resolve(wz("http://h").as_ptr(), wz("http://h/x/y").as_ptr()).unwrap();
        assert_eq!((String::from_utf16_lossy(&s2), String::from_utf16_lossy(&p2)), ("http://h".into(), "x/y".into()));
        let mut n = 0u32;
        let mut buf = [0u8; 4];
        assert_eq!(http::httpQueryDataAvailable(0, &mut n, 0, 0), 0);
        assert_eq!(http::httpReadFile(0, buf.as_mut_ptr(), 4, &mut n), 0);
        assert_eq!(http::httpCloseHandle(0), 0);
        SymbolServerSetOptionsW(SSRVOPT_WINHTTP as usize, 1);
        assert_eq!(http::engine(), SSRVOPT_WINHTTP);
        assert_eq!(http::httpReadFile(0, buf.as_mut_ptr(), 4, &mut n), 0);
        SymbolServerSetOptionsW(SSRVOPT_WININET as usize, 1);
        assert_eq!(http::engine(), SSRVOPT_WININET);
    }
}

#[test]
fn http_store_in_path_is_skipped_offline() {
    let _g = guard();
    let d = tmp("httpmix");
    let up = d.join("up");
    put(&up.join("h.pdb").join("7").join("h.pdb"), b"local");
    unsafe {
        SymbolServerSetOptionsW(SSRVOPT_PARAMTYPE as usize, SSRVOPT_DWORD);
        let f = wz("h.pdb");
        let mut out = [0u16; MAX_PATH_W];
        let p = wz(&format!("http://symbols.example/s*{}", ps(&up)));
        assert_eq!(SymbolServerW(p.as_ptr(), f.as_ptr(), 7usize as *const u8, 0, 0, out.as_mut_ptr()), 1);
        assert_eq!(out_str(&out), ps(&up.join("h.pdb").join("7").join("h.pdb")));
        let miss = wz("http://symbols.example/s");
        SetLastError(0);
        assert_eq!(SymbolServerW(miss.as_ptr(), f.as_ptr(), 7usize as *const u8, 0, 0, out.as_mut_ptr()), 0);
        assert_eq!(GetLastError(), http::OFFLINE_ERROR);
        let p = wz(&format!("{}*http://symbols.example/s", ps(&up)));
        assert_eq!(SymbolServerW(p.as_ptr(), f.as_ptr(), 7usize as *const u8, 0, 0, out.as_mut_ptr()), 1);
        assert_eq!(std::fs::read(out_str(&out)).unwrap(), b"local");
        let mut ext = ExtendedOutput { size: 0x214, version: 1, file_ptr_msg: [0; MAX_PATH_W] };
        assert_eq!(SymbolServerWEx(p.as_ptr(), f.as_ptr(), 7usize as *const u8, 0, 0, out.as_mut_ptr(), &mut ext), 1);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn lz_szdd_files_expand() {
    let d = tmp("lz");
    let src = d.join("f.tx_");
    let mut v = SZDD.to_vec();
    v.push(b'A');
    v.push(b't');
    v.extend_from_slice(&8u32.to_le_bytes());
    v.push(0xff);
    v.extend_from_slice(b"ABCDEFGH");
    std::fs::write(&src, &v).unwrap();
    let dst = d.join("f.txt");
    assert!(cab::is_szdd(&w(&ps(&src))));
    assert_eq!(cab::lz_expand(&w(&ps(&src)), &w(&ps(&dst))), 0);
    assert_eq!(std::fs::read(&dst).unwrap(), b"ABCDEFGH");
    let plain = d.join("plain");
    std::fs::write(&plain, b"not a cabinet").unwrap();
    assert_eq!(cab::lz_expand(&w(&ps(&plain)), &w(&ps(&d.join("x")))), 0x570);
    assert_ne!(cab::expand(&w(&ps(&plain)), &w(&ps(&d.join("y")))), 0);
    assert_eq!(cab::lz_error(-1), 0x1e);
    assert_eq!(cab::lz_error(-2), 0x1d);
    assert_eq!(cab::lz_error(-5), 8);
    assert_eq!(cab::lz_error(-8), 0xd);
    assert_eq!(cab::lz_error(-9), 1);
    let _ = std::fs::remove_dir_all(&d);
}

use super::cab::SZDD;

#[test]
fn eula_dialog_proc_template_and_rundll() {
    let _g = guard();
    unsafe {
        assert_eq!(eula::EulaDlgProc(0, eula::WM_CLOSE, 0, 0), 1);
        assert_eq!(eula::EulaDlgProc(0, eula::WM_COMMAND, 6, 0), 1);
        assert_eq!(eula::EulaDlgProc(0, eula::WM_COMMAND, 7, 0), 1);
        assert_eq!(eula::EulaDlgProc(0, eula::WM_COMMAND, 1, 0), 0);
        assert_eq!(eula::EulaDlgProc(0, 0x200, 0, 0), 0);
        let t = eula::dialog_template();
        assert_eq!(t[0] & 0x8000_0000, 0x8000_0000);
        assert_eq!(t[2] & 0xffff, 3);
        assert_eq!(eula::answer_code(6), 1);
        assert_eq!(eula::answer_code(7), 2);
        assert_eq!(eula::answer_code(-1), 0);
        let yes = eula::answer_file(true);
        let no = eula::answer_file(false);
        std::fs::write(String::from_utf16_lossy(&yes), b"").unwrap();
        std::fs::write(String::from_utf16_lossy(&no), b"").unwrap();
        eula::RunDllEntry(0, 0, wz("resettou").as_ptr(), 0);
        assert!(!exists(&yes) && !exists(&no));
        std::fs::write(String::from_utf16_lossy(&yes), b"").unwrap();
        SymbolServerSetOptionsW(SSRVOPT_RESETTOU as usize, 1);
        assert!(!exists(&yes));
        SymbolServerSetOptionsW(SSRVOPT_UNATTENDED as usize, 1);
        assert_eq!(eula::show_terms(), 2);
        eula::RunDllEntry(0x10, 0, wz("terms").as_ptr(), 0);
        assert_ne!(options() & SSRVOPT_PARENTWIN, 0);
        assert!(!exists(&yes) && !exists(&no));
        eula::RunDllEntry(0, 0, wz("TERMS").as_ptr(), 0);
        assert_eq!(options() & SSRVOPT_PARENTWIN, 0);
    }
}

static EVENTS: M<Vec<(usize, String)>> = M::new(Vec::new());
static CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

unsafe fn cstr(p: *const u8) -> String {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf8_lossy(core::slice::from_raw_parts(p, n)).to_string()
}

unsafe extern "system" fn recorder(action: usize, data: u64, ctx: u64) -> i32 {
    let text = match action {
        trace::SSRVACTION_TRACE => cstr(data as *const u8),
        trace::SSRVACTION_EVENT => cstr((*(data as *const trace::Event<u8>)).desc),
        trace::SSRVACTION_EVENTW => String::from_utf16_lossy(wstr((*(data as *const trace::Event<u16>)).desc)),
        trace::SSRVACTION_SIZE => format!("{}", *(data as *const i64)),
        trace::SSRVACTION_QUERYCANCEL => {
            if CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                *(data as *mut u64) = 1;
                EVENTS.lock().unwrap().push((action, format!("{ctx}")));
                return 1;
            }
            String::new()
        }
        _ => String::new(),
    };
    EVENTS.lock().unwrap().push((action, text));
    0
}

fn take_events() -> Vec<(usize, String)> {
    std::mem::take(&mut *EVENTS.lock().unwrap())
}

fn joined(ev: &[(usize, String)], action: usize) -> String {
    ev.iter().filter(|e| e.0 == action).map(|e| e.1.clone()).collect::<Vec<_>>().join("")
}

unsafe fn arm_callback(wide: bool) {
    take_events();
    let f = recorder as *const () as u64;
    SymbolServerSetOptionsW(if wide { SSRVOPT_CALLBACKW } else { SSRVOPT_CALLBACK } as usize, f);
    SymbolServerSetOptionsW(SSRVOPT_TRACE as usize, 1);
    SymbolServerSetOptionsW(SSRVOPT_SETCONTEXT as usize, 0x77);
    SymbolServerSetOptionsW(SSRVOPT_PARAMTYPE as usize, SSRVOPT_DWORD);
}

#[test]
fn wildcard_matcher_follows_console_tokens() {
    let m = |s: &str, p: &str| exclude::wild(&w(s), &w(p));
    assert!(m("foo.pdb", "*.pdb"));
    assert!(m("FOO.PDB", "foo.pdb"));
    assert!(!m("foo.dll", "*.pdb"));
    assert!(m("a1.pdb", "a?.pdb"));
    assert!(!m("a.pdb", "a?.pdb"));
    assert!(m("bx", "[a-c]x"));
    assert!(!m("dx", "[a-c]x"));
    assert!(m("qx", "[pq]x"));
    assert!(m("x", "[a]#x"));
    assert!(!m("x", "[a]x"));
    assert!(m("b", "a#b"));
    assert!(m("aaab", "a#b"));
    assert!(!m("b", "a+b"));
    assert!(m("aab", "a+b"));
    assert!(m("a*b", "a\\*b"));
    assert!(!m("axb", "a\\*b"));
    assert!(!m("ab", "a"));
    assert!(m("", ""));
    assert!(!m("x", ""));
    assert!(!m("x", "[a-]"));
    assert!(m("ntdll.pdb", "nt*"));
}

#[test]
fn exclusions_from_ini_and_registry_block_lookups() {
    let _g = guard();
    let d = tmp("excl");
    let ini = d.join("symsrv.ini");
    std::fs::write(&ini, "[exclusions]\r\nskip*.pdb\r\nother.dll\r\n").unwrap();
    assert!(exclude::load_ini(&w(&ps(&ini))));
    assert_eq!(exclude::PATTERNS.lock().unwrap().clone(), vec![w("skip*.pdb"), w("other.dll")]);
    exclude::reset();
    unsafe { SetLastError(0) };
    assert!(!exclude::load_ini(&w(&ps(&d.join("none.ini")))));
    assert_eq!(unsafe { GetLastError() }, 2);
    std::fs::write(&ini, "[other]\r\na=b\r\n").unwrap();
    assert!(exclude::load_ini(&w(&ps(&ini))));
    assert!(exclude::PATTERNS.lock().unwrap().is_empty());

    #[link(name = "advapi32")]
    extern "system" {
        fn RegCreateKeyExW(k: isize, s: *const u16, r: u32, c: *const u16, o: u32, sam: u32, sa: *const u8, out: *mut isize, disp: *mut u32) -> i32;
        fn RegSetValueExW(k: isize, n: *const u16, r: u32, t: u32, d: *const u8, dn: u32) -> i32;
        fn RegCloseKey(k: isize) -> i32;
        fn RegDeleteTreeW(k: isize, s: *const u16) -> i32;
    }
    let sub = format!("Software\\TomBoxSymsrvTest{}", std::process::id());
    let key = format!("{sub}\\Exclusions");
    unsafe {
        let mut h = 0isize;
        assert_eq!(RegCreateKeyExW(exclude::HKEY_CURRENT_USER, wz(&key).as_ptr(), 0, core::ptr::null(), 0, 0x20006, core::ptr::null(), &mut h, core::ptr::null_mut()), 0);
        assert!(!exclude::load_reg(exclude::HKEY_CURRENT_USER, &key));
        let one = 1u32.to_le_bytes();
        assert_eq!(RegSetValueExW(h, wz("excl*.pdb").as_ptr(), 0, 4, one.as_ptr(), 4), 0);
        RegCloseKey(h);
        assert!(exclude::load_reg(exclude::HKEY_CURRENT_USER, &key));
        RegDeleteTreeW(exclude::HKEY_CURRENT_USER, wz(&sub).as_ptr());
    }
    assert_eq!(exclude::PATTERNS.lock().unwrap().clone(), vec![w("excl*.pdb")]);
    exclude::LOADED.store(true, std::sync::atomic::Ordering::SeqCst);
    let up = d.join("up");
    put(&up.join("exclme.pdb").join("ABC1").join("exclme.pdb"), b"x");
    put(&up.join("keep.pdb").join("ABC1").join("keep.pdb"), b"x");
    unsafe {
        arm_callback(false);
        let params = wz(&ps(&up));
        let mut out = [0u16; MAX_PATH_W];
        assert_eq!(SymbolServerW(params.as_ptr(), wz("exclme.pdb").as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 0);
        assert_eq!(GetLastError(), 2);
        let ev = take_events();
        assert!(joined(&ev, trace::SSRVACTION_EVENT).contains("SYMSRV:  exclme.pdb is in the file exclusion list\n"));
        assert_eq!(SymbolServerW(params.as_ptr(), wz("keep.pdb").as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 1);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn copy_progress_drives_size_cancel_and_trace_callbacks() {
    let _g = guard();
    let d = tmp("progress");
    let up = d.join("up");
    let down = d.join("down");
    put(&up.join("p.pdb").join("ABC1").join("p.pdb"), &[7u8; 5000]);
    unsafe {
        arm_callback(false);
        let params = wz(&format!("{}*{}", ps(&down), ps(&up)));
        let f = wz("p.pdb");
        let mut out = [0u16; MAX_PATH_W];
        assert_eq!(SymbolServerW(params.as_ptr(), f.as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 1);
        let ev = take_events();
        assert_eq!(ev.iter().find(|e| e.0 == trace::SSRVACTION_SIZE).map(|e| e.1.clone()), Some("5000".to_string()));
        assert!(ev.iter().any(|e| e.0 == trace::SSRVACTION_QUERYCANCEL));
        let ev_text = joined(&ev, trace::SSRVACTION_EVENT);
        let tr_text = joined(&ev, trace::SSRVACTION_TRACE);
        assert_eq!(ev_text, tr_text);
        assert!(ev_text.contains(&format!("SYMSRV:  p.pdb from {}: 5000 bytes -  ", ps(&up))));
        assert!(ev_text.contains("  0 percent"));
        assert!(ev_text.contains("\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8} copied         \n"));
        assert!(ev_text.contains("p.pdb - file not found\n"));
        CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
        let down2 = d.join("down2");
        let p2 = wz(&format!("{}*{}", ps(&down2), ps(&up)));
        assert_eq!(SymbolServerW(p2.as_ptr(), f.as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 1);
        CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(out_str(&out), ps(&up.join("p.pdb").join("abc1").join("p.pdb")));
        let ev = take_events();
        assert!(ev.iter().any(|e| e.0 == trace::SSRVACTION_QUERYCANCEL && e.1 == "119"));
        assert!(joined(&ev, trace::SSRVACTION_EVENT).contains(" cancelled      \n"));
        assert!(!down2.join("p.pdb").join("abc1").join("p.pdb").exists());
        arm_callback(true);
        let down3 = d.join("down3");
        let p3 = wz(&format!("{}*{}", ps(&down3), ps(&up)));
        assert_eq!(SymbolServerW(p3.as_ptr(), f.as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 1);
        let ev = take_events();
        assert!(joined(&ev, trace::SSRVACTION_EVENTW).contains(" copied         \n"));
        assert!(!ev.iter().any(|e| e.0 == trace::SSRVACTION_EVENT));
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn failure_messages_and_disabled_stores() {
    let _g = guard();
    let d = tmp("msgs");
    let up = d.join("up");
    std::fs::create_dir_all(&up).unwrap();
    let bad = format!("{}\\bad|store", ps(&d));
    unsafe {
        arm_callback(false);
        let params = wz(&format!("{bad}*{}", ps(&up)));
        let f = wz("m.pdb");
        let mut out = [0u16; MAX_PATH_W];
        assert_eq!(SymbolServerW(params.as_ptr(), f.as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 0);
        let first = joined(&take_events(), trace::SSRVACTION_EVENT);
        let err = store::disabled_error(&w(&bad)).expect("bad store disabled");
        assert_ne!(err, 0);
        assert!(first.contains(&format!("SYMSRV:  store {bad} - {}\n", trace::error_text(err))));
        assert!(first.contains(&format!("{}\\m.pdb\\abc1\\m.pdb - file not found\n", ps(&up))));
        assert!(first.contains(&format!("SYMSRV:  {bad}: not available\n")));
        assert!(first.contains(&format!("SYMSRV:  {}\\m.pdb\\abc1\\m.pdb not found\n", ps(&up))));
        assert_eq!(SymbolServerW(params.as_ptr(), f.as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 0);
        let second = joined(&take_events(), trace::SSRVACTION_EVENT);
        assert!(second.contains(&format!("store {bad} - ")));
        let long = wz(&"y".repeat(0x65));
        assert_eq!(SymbolServerW(params.as_ptr(), long.as_ptr(), 1usize as *const u8, 0, 0, out.as_mut_ptr()), 0);
        assert!(joined(&take_events(), trace::SSRVACTION_EVENT).contains(" - filename cannot exceed 100 characters\n"));
        let empty = wz("   ");
        assert_eq!(lookup(wstr(empty.as_ptr()), &w("m.pdb"), &w("abc1"), out.as_mut_ptr(), core::ptr::null_mut()), 0);
        assert_eq!(GetLastError(), 87);
        SymbolServerSetOptionsW(SSRVOPT_TRACE as usize, 0);
        assert_eq!(SymbolServerW(params.as_ptr(), f.as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 0);
        assert!(take_events().is_empty());
        assert!(!trace::error_text(2).ends_with('\n'));
        assert!(trace::error_text(0x3fff_ff00).starts_with("error 0x"));
        SymbolServerSetOptionsW(SSRVOPT_NOCOPY as usize, 1);
        let p2 = wz(&format!("{}*{bad}", ps(&up)));
        assert_eq!(SymbolServerW(p2.as_ptr(), f.as_ptr(), 0xabcusize as *const u8, 1, 0, out.as_mut_ptr()), 1);
        assert_eq!(out_str(&out), format!("{}\\m.pdb\\abc1\\m.pdb", ps(&up)));
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn service_ini_reload_timer_cycles_then_reloads() {
    let _g = guard();
    let t = exclude::reload_timer();
    assert_ne!(t, 0);
    let start = std::time::Instant::now();
    let mut hits = 0;
    exclude::reload_cycle(t, -200_000, 3, &mut || hits += 1);
    assert_eq!(hits, 1);
    assert!(start.elapsed() >= std::time::Duration::from_millis(55));
    let mut later = 0;
    exclude::reload_cycle(0, -200_000, 3, &mut || later += 1);
    assert_eq!(later, 1);
    assert_eq!(exclude::RELOAD_DUE, -600_000_000);
    assert_eq!(exclude::RELOAD_TICKS, 30);
    unsafe {
        SymbolServerSetOptionsW(SSRVOPT_SERVICE as usize, 1);
        assert!(http::is_service());
        exclude::start_reload_thread();
        let h = exclude::RELOAD_THREAD.load(std::sync::atomic::Ordering::SeqCst);
        assert!(h != -1 && h != 0);
        exclude::start_reload_thread();
        assert_eq!(exclude::RELOAD_THREAD.load(std::sync::atomic::Ordering::SeqCst), h);
    }
}
