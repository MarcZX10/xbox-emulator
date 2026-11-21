use compat_common::gdk::{self, save, Guid};
use core::ffi::c_void;

unsafe fn method(clsid: Guid, iid: Guid, slot: usize) -> (*mut c_void, usize) {
    let mut obj: *mut c_void = core::ptr::null_mut();
    assert_eq!(gdk::query_api_impl(&clsid, &iid, &mut obj), 0);
    (obj, *(*(obj as *const *const usize)).add(slot))
}

fn tmp(n: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("tombox_save_{n}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

type Write = unsafe extern "system" fn(*mut c_void, usize, *const u8, *const u8, usize) -> i32;

unsafe fn update(p: *mut save::Provider, container: &[u8], blobs: &[(&[u8], usize)], deletes: &[&[u8]]) -> i32 {
    let c = save::CLSID;
    let (o, create) = method(c, c, 16);
    let create: unsafe extern "system" fn(*mut c_void, *mut save::Provider, *const u8, *mut usize) -> i32 = core::mem::transmute(create);
    let mut cont = 0usize;
    assert_eq!(create(o, p, container.as_ptr(), &mut cont), 0);
    let (_, upd) = method(c, c, 23);
    let upd: unsafe extern "system" fn(*mut c_void, usize, *const u8, *mut usize) -> i32 = core::mem::transmute(upd);
    let mut u = 0usize;
    assert_eq!(upd(o, cont, core::ptr::null(), &mut u), 0);
    let (_, write) = method(c, c, 25);
    let write: Write = core::mem::transmute(write);
    for (name, len) in blobs {
        let data = vec![0x5au8; *len];
        assert_eq!(write(o, u, name.as_ptr(), data.as_ptr(), *len), 0);
    }
    let (_, del) = method(c, c, 26);
    let del: unsafe extern "system" fn(*mut c_void, usize, *const u8) -> i32 = core::mem::transmute(del);
    for name in deletes {
        assert_eq!(del(o, u, name.as_ptr()), 0);
    }
    let (_, submit) = method(c, c, 27);
    let submit: unsafe extern "system" fn(*mut c_void, usize) -> i32 = core::mem::transmute(submit);
    let hr = submit(o, u);
    let (_, close_u) = method(c, c, 24);
    let close_u: unsafe extern "system" fn(*mut c_void, usize) = core::mem::transmute(close_u);
    close_u(o, u);
    let (_, close_c) = method(c, c, 17);
    let close_c: unsafe extern "system" fn(*mut c_void, usize) = core::mem::transmute(close_c);
    close_c(o, cont);
    hr
}

#[test]
fn interrupted_commit_is_all_or_nothing() {
    let root = tmp("journal");
    let s = save::Store::open(root.clone()).unwrap();
    s.commit("Slot1", Some("First"), &[("a".into(), b"old-a".to_vec()), ("b".into(), b"old-b".to_vec())], &[]).unwrap();
    let d = root.join("c_Slot1");
    std::fs::write(d.join("b_a.blob.tmp"), b"half").unwrap();
    std::fs::write(d.join("b_c.blob.tmp"), b"half").unwrap();
    drop(s);
    let s = save::Store::open(root.clone()).unwrap();
    assert_eq!(s.read_blob("Slot1", "a").unwrap(), b"old-a");
    assert_eq!(s.blobs("Slot1").len(), 2);
    assert!(!d.join("b_a.blob.tmp").exists() && !d.join("b_c.blob.tmp").exists());

    std::fs::write(d.join("b_a.blob.tmp"), b"new-a").unwrap();
    std::fs::write(d.join("b_c.blob.tmp"), b"new-c").unwrap();
    std::fs::write(d.join(".journal"), "W a\nW c\nD b\n").unwrap();
    drop(s);
    let s = save::Store::open(root.clone()).unwrap();
    assert_eq!(s.read_blob("Slot1", "a").unwrap(), b"new-a");
    assert_eq!(s.read_blob("Slot1", "c").unwrap(), b"new-c");
    assert!(s.read_blob("Slot1", "b").is_none());
    assert!(!d.join(".journal").exists());
    let info = s.container_info("Slot1").unwrap();
    assert_eq!((info.display.as_str(), info.blob_count, info.total_size), ("First", 2, 10));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn quota_and_update_size_limits_match_console() {
    std::env::set_var("TOMBOX_SAVE_QUOTA_MB", "1");
    unsafe {
        let root = tmp("quota");
        let p = Box::into_raw(Box::new(save::Provider { store: save::Store::open(root.clone()).unwrap() }));
        assert_eq!(update(p, b"A\0", &[(b"x\0", 600 << 10)], &[]), 0);
        assert_eq!(update(p, b"B\0", &[(b"y\0", 600 << 10)], &[]), save::E_GS_QUOTA_EXCEEDED);
        assert!((*p).store.container_info("B").map(|c| c.blob_count).unwrap_or(0) == 0);
        assert_eq!(update(p, b"A\0", &[(b"x\0", 900 << 10)], &[]), 0);
        assert_eq!(update(p, b"A\0", &[(b"z\0", 50 << 10)], &[b"x\0"]), 0);
        assert_eq!(update(p, b"B\0", &[(b"y\0", 900 << 10)], &[]), 0);

        let (o, quota) = method(save::CLSID, save::CLSID, 7);
        let quota: unsafe extern "system" fn(*mut c_void, *mut save::Provider, *mut i64) -> i32 = core::mem::transmute(quota);
        let mut left = 0i64;
        assert_eq!(quota(o, p, &mut left), 0);
        assert_eq!(left, (1 << 20) - (950 << 10));

        std::env::set_var("TOMBOX_SAVE_QUOTA_MB", "64");
        assert_eq!(update(p, b"C\0", &[(b"big\0", 17 << 20)], &[]), save::E_GS_UPDATE_TOO_BIG);
        assert_eq!(update(p, b"C\0", &[(b"big\0", 15 << 20)], &[]), 0);
        std::env::remove_var("TOMBOX_SAVE_QUOTA_MB");
        let _ = std::fs::remove_dir_all(root);
    }
}
