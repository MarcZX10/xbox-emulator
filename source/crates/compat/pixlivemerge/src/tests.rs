use super::*;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn blob(entries: &[&str]) -> Vec<u16> {
    let mut v = Vec::new();
    for e in entries {
        v.extend(w(e));
        v.push(0);
        v.push(0);
    }
    v.push(0);
    v
}

const G1: &str = "12345678-9abc-def0-1122-334455667788";
const G2: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

fn g1() -> [u8; 16] {
    [0x78, 0x56, 0x34, 0x12, 0xbc, 0x9a, 0xf0, 0xde, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]
}

#[test]
fn parse_reads_name_guid_age_pdb_and_sorts_by_name_length() {
    let mut b = blob(&[&format!("kernelbase.dll@{{{G1}}},3 kernelbase.pdb"), &format!("a.dll@{{{G2}}}:17 c:\\sym\\a.pdb")]);
    let mut v = RecordVec::new();
    let n = b.len();
    assert_eq!(unsafe { ParsePdbInfo(b.as_mut_ptr(), n, &mut v) }, 0);
    unsafe {
        let s = v.as_slice();
        assert_eq!(s.len(), 2);
        assert_eq!(String::from_utf16(s[0].name.as_slice()).unwrap(), "a.dll");
        assert_eq!(s[0].age, 17);
        assert_eq!(s[0].pdb.size, PDB_CHARS);
        assert_eq!(s[0].pdb.cap, 0x30f);
        assert_eq!(&s[0].pdb.as_slice()[..12], b"c:\\sym\\a.pdb");
        assert!(s[0].pdb.as_slice()[12..].iter().all(|c| *c == 0));
        assert_eq!(String::from_utf16(s[1].name.as_slice()).unwrap(), "kernelbase.dll");
        assert_eq!(s[1].name.cap, 15);
        assert_eq!(s[1].guid, g1());
        assert_eq!(s[1].age, 3);
        v.clear_and_free();
    }
}

#[test]
fn parse_rejects_malformed_entries_with_e_fail() {
    for bad in ["nodelim", "x.dll@{G}", &format!("x.dll@{{{G1}}}"), &format!("x.dll@{{{G1}}},1"), "x.dll@{not-a-guid},1 p.pdb"] {
        let mut b = blob(&[bad]);
        let mut v = RecordVec::new();
        let n = b.len();
        assert_eq!(unsafe { ParsePdbInfo(b.as_mut_ptr(), n, &mut v) }, E_FAIL, "{bad}");
        unsafe { assert_eq!(v.len(), 0) };
    }
}

#[test]
fn parse_keeps_records_before_a_failure_unsorted() {
    let mut b = blob(&[&format!("long-name.dll@{{{G1}}},1 a.pdb"), &format!("b.dll@{{{G2}}},2 b.pdb"), "broken"]);
    let mut v = RecordVec::new();
    let n = b.len();
    assert_eq!(unsafe { ParsePdbInfo(b.as_mut_ptr(), n, &mut v) }, E_FAIL);
    unsafe {
        assert_eq!(v.len(), 2);
        assert_eq!(v.as_slice()[0].name.size, 13);
        v.clear_and_free();
    }
}

#[test]
fn parse_empty_contents_succeeds() {
    let mut b = vec![0u16; 4];
    let mut v = RecordVec::new();
    assert_eq!(unsafe { ParsePdbInfo(b.as_mut_ptr(), 4, &mut v) }, 0);
    unsafe { assert_eq!(v.len(), 0) };
}

#[test]
fn vector_growth_follows_msvc_policy() {
    let mut entries = Vec::new();
    for i in 0..5 {
        entries.push(format!("m{i}.dll@{{{G1}}},{i} p.pdb"));
    }
    let refs: Vec<&str> = entries.iter().map(|s| s.as_str()).collect();
    let mut b = blob(&refs);
    let mut v = RecordVec::new();
    let n = b.len();
    assert_eq!(unsafe { ParsePdbInfo(b.as_mut_ptr(), n, &mut v) }, 0);
    unsafe {
        assert_eq!(v.len(), 5);
        assert_eq!(v.cap(), 6);
        v.clear_and_free();
    }
}

#[test]
fn find_matches_case_insensitive_suffix() {
    let mut b = blob(&[&format!("ntdll.dll@{{{G1}}},1 ntdll.pdb"), &format!("Kernel32.DLL@{{{G2}}},2 k.pdb")]);
    let mut v = RecordVec::new();
    let n = b.len();
    unsafe { ParsePdbInfo(b.as_mut_ptr(), n, &mut v) };
    let q = wide("c:\\windows\\system32\\kernel32.dll");
    assert_eq!(unsafe { FindPdbInfo(q.as_ptr(), &v) }, 1);
    let q = wide("NTDLL.dll");
    assert_eq!(unsafe { FindPdbInfo(q.as_ptr(), &v) }, 0);
    let q = wide("tdll.dll");
    assert_eq!(unsafe { FindPdbInfo(q.as_ptr(), &v) }, -1);
    assert_eq!(unsafe { FindPdbInfo(core::ptr::null(), &v) }, -1);
    unsafe { v.clear_and_free() };
}

#[test]
fn write_info_assigns_guid_age_and_string() {
    let mut b = blob(&[&format!("x.dll@{{{G1}}},9 x.pdb")]);
    let mut v = RecordVec::new();
    let n = b.len();
    unsafe { ParsePdbInfo(b.as_mut_ptr(), n, &mut v) };
    let mut out = vec![0u64; 7];
    unsafe {
        let o = out.as_mut_ptr() as *mut u8;
        core::ptr::write(o.add(0x18) as *mut AString, AString::new());
        write_info(&v.as_slice()[0], o);
        assert_eq!(core::slice::from_raw_parts(o, 16), &g1());
        assert_eq!(*(o.add(0x10) as *const u32), 9);
        let s = &mut *(o.add(0x18) as *mut AString);
        assert_eq!(s.size, PDB_CHARS);
        assert_eq!(s.cap, 0x30f);
        assert_eq!(&s.as_slice()[..5], b"x.pdb");
        s.drop_in_place();
        v.clear_and_free();
    }
}

#[test]
fn vbi_lookup_on_host_registry_reports_no_records() {
    let _g = crate::test_lock();
    ResetVbiRecordsSearched();
    assert!(!GetVbiRecordsSearched());
    let q = wide("ntdll.dll");
    let mut out = vec![0u64; 7];
    unsafe { core::ptr::write((out.as_mut_ptr() as *mut u8).add(0x18) as *mut AString, AString::new()) };
    let first = unsafe { PdbInfoFromVBI(q.as_ptr(), out.as_mut_ptr() as *mut u8) };
    assert!(GetVbiRecordsSearched());
    assert_eq!(first, hr_win32(259));
    let second = unsafe { PdbInfoFromVBI(q.as_ptr(), out.as_mut_ptr() as *mut u8) };
    assert_eq!(second, HR_NOT_FOUND);
    ResetVbiRecordsSearched();
    assert!(!GetVbiRecordsSearched());
}

#[test]
fn vbi_missing_key_reports_file_not_found() {
    let _g = crate::test_lock();
    let mut v = RecordVec::new();
    assert_eq!(unsafe { load_vbi_from("System\\NoSuchKeyForPixLiveMerge", &mut v) }, hr_win32(2));
}

#[test]
fn image_name_cache_flag_round_trips() {
    let _g = crate::test_lock();
    SetImageFileNameCacheUsage(true);
    assert!(image_name_cache_enabled());
    SetImageFileNameCacheUsage(false);
    assert!(!image_name_cache_enabled());
    SetImageFileNameCacheUsage(true);
}

#[test]
fn age_parse_is_decimal_wcstol() {
    let v = wide("  42x");
    assert_eq!(unsafe { wcstol10(v.as_ptr()) }, 42);
    let v = wide("-1");
    assert_eq!(unsafe { wcstol10(v.as_ptr()) }, u32::MAX);
    let v = wide("99999999999");
    assert_eq!(unsafe { wcstol10(v.as_ptr()) }, i32::MAX as u32);
}
