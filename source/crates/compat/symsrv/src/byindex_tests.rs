use super::*;

#[test]
fn ansi_by_index_validates_finds_and_converts_like_the_wide_call() {
    let _g = crate::tests::guard();
    let d = std::env::temp_dir().join(format!("symsrv-byindex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let up = d.join("up");
    std::fs::create_dir_all(up.join("a.pdb").join("abc1")).unwrap();
    std::fs::write(up.join("a.pdb").join("abc1").join("a.pdb"), b"symbols").unwrap();
    std::fs::write(up.join("pingme.txt"), b"").unwrap();
    let srv = format!("{}\0", up.to_string_lossy());
    unsafe {
        let mut out = [0xeeu8; MAX_PATH_W];
        for (s, i, f) in [(core::ptr::null(), b"abc1\0".as_ptr(), b"a.pdb\0".as_ptr()), (srv.as_ptr(), core::ptr::null(), b"a.pdb\0".as_ptr()), (srv.as_ptr(), b"abc1\0".as_ptr(), core::ptr::null())] {
            SetLastError(0);
            assert_eq!(SymbolServerByIndex(s, i, f, out.as_mut_ptr()), 0);
            assert_eq!(GetLastError(), 87);
        }
        SetLastError(0);
        assert_eq!(SymbolServerByIndex(srv.as_ptr(), b"abc1\0".as_ptr(), b"\0".as_ptr(), out.as_mut_ptr()), 0);
        assert_eq!(GetLastError(), 87);
        assert_eq!(out[0], 0xee);
        assert_eq!(SymbolServerByIndex(srv.as_ptr(), b"abc1\0".as_ptr(), b"a.pdb\0".as_ptr(), out.as_mut_ptr()), 1);
        let n = out.iter().position(|&c| c == 0).unwrap();
        let got = String::from_utf8(out[..n].to_vec()).unwrap();
        assert_eq!(got, up.join("a.pdb").join("abc1").join("a.pdb").to_string_lossy());
        let mut wout = [0u16; MAX_PATH_W];
        let (ws, wi, wf) = (z(&w(&up.to_string_lossy())), z(&w("abc1")), z(&w("a.pdb")));
        assert_eq!(SymbolServerByIndexW(ws.as_ptr(), wi.as_ptr(), wf.as_ptr(), wout.as_mut_ptr()), 1);
        assert_eq!(String::from_utf16_lossy(wstr(wout.as_ptr())), got);
        assert_eq!(SymbolServerByIndex(srv.as_ptr(), b"abc1\0".as_ptr(), b"a.pdb\0".as_ptr(), core::ptr::null_mut()), 1);
        SetLastError(0);
        let mut miss = [0xeeu8; MAX_PATH_W];
        assert_eq!(SymbolServerByIndex(srv.as_ptr(), b"zzz9\0".as_ptr(), b"a.pdb\0".as_ptr(), miss.as_mut_ptr()), 0);
        assert_eq!(GetLastError(), 2);
        assert_eq!(miss[0], 0xee);
    }
    let _ = std::fs::remove_dir_all(&d);
}
