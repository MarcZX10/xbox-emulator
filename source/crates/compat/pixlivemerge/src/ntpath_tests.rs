use crate::ntpath::*;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn s(v: &[u16]) -> String {
    String::from_utf16(v).unwrap()
}

fn sample() -> NtPathMap {
    let vols = [
        Volume { device: w("\\Device\\HarddiskVolume3"), mount: Some(w("C:\\")) },
        Volume { device: w("\\Device\\HarddiskVolume1"), mount: None },
        Volume { device: w("\\Device\\HarddiskVolume5"), mount: Some(w("D:\\mnt")) },
    ];
    NtPathMap::from_parts(w("C:\\Windows\\system32"), w("C:\\Windows"), &vols).unwrap()
}

#[test]
fn from_parts_builds_console_table_order_with_slash_flags() {
    let m = sample();
    let got: Vec<(String, String)> = m.entries.iter().map(|(p, r)| (s(p), s(r))).collect();
    let want: Vec<(String, String)> = [
        ("\\??\\", ""),
        ("\\SystemRoot\\", "C:\\Windows\\"),
        ("\\Device\\HarddiskVolume3\\", "C:\\"),
        ("\\Device\\HarddiskVolume5\\", "D:\\mnt\\"),
        ("\\Device\\LanmanRedirector\\", "\\\\"),
        ("\\Device\\Mup\\", "\\\\"),
        ("\\Device\\vmsmb\\", "\\\\?\\VMSMB\\"),
        ("\\GLOBAL??\\", "\\\\?\\"),
        ("\\\\", "\\\\"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(got, want);
    assert_eq!(m.unmounted.iter().map(|u| s(u)).collect::<Vec<_>>(), vec!["\\Device\\HarddiskVolume1\\".to_string()]);
    assert_eq!(s(&m.sysdir), "C:\\Windows\\system32");
    assert_eq!(s(&m.windir), "C:\\Windows\\");
}

#[test]
fn from_parts_rejects_windir_without_drive_and_too_long() {
    assert_eq!(NtPathMap::from_parts(w("x"), w("\\Windows"), &[]).err(), Some(E_UNEXPECTED));
    assert_eq!(NtPathMap::from_parts(w("x"), w("C"), &[]).err(), Some(E_UNEXPECTED));
    let long: String = std::iter::once("C:").chain(std::iter::repeat("a").take(MAX_PATH)).collect();
    assert_eq!(NtPathMap::from_parts(w("x"), w(&long), &[]).err(), Some(E_INSUFFICIENT_BUFFER));
    let mut ok = long.clone();
    ok.truncate(MAX_PATH - 1);
    assert!(NtPathMap::from_parts(w("x"), w(&ok), &[]).is_ok());
}

#[test]
fn translate_follows_console_matcher_steps() {
    let m = sample();
    assert_eq!(m.translate("\\??\\C:\\x.dll"), ("C:\\x.dll".into(), true));
    assert_eq!(m.translate("\\SystemRoot\\System32\\ntdll.dll"), ("C:\\Windows\\System32\\ntdll.dll".into(), true));
    assert_eq!(m.translate("\\Device\\HarddiskVolume3\\a\\b.exe"), ("C:\\a\\b.exe".into(), true));
    assert_eq!(m.translate("\\device\\harddiskvolume3\\a.exe"), ("C:\\device\\harddiskvolume3\\a.exe".into(), true));
    assert_eq!(m.translate("\\Device\\HarddiskVolume1\\a.exe"), ("".into(), false));
    assert_eq!(m.translate("\\Device\\Mup\\srv\\share\\f"), ("\\\\srv\\share\\f".into(), true));
    assert_eq!(m.translate("\\Device\\vmsmb\\x\\y"), ("\\\\?\\VMSMB\\x\\y".into(), true));
    assert_eq!(m.translate("\\GLOBAL??\\C:\\z"), ("\\\\?\\C:\\z".into(), true));
    assert_eq!(m.translate("\\\\srv\\s"), ("\\\\srv\\s".into(), true));
    assert_eq!(m.translate("tcpip.sys"), ("C:\\Windows\\system32\\drivers\\tcpip.sys".into(), true));
    assert_eq!(m.translate("\\;Z:0000000000012345\\srv\\share\\f"), ("\\\\srv\\share\\f".into(), true));
    assert_eq!(m.translate("D:\\abs\\p"), ("D:\\abs\\p".into(), true));
    assert_eq!(m.translate("\\Device\\Other\\p"), ("".into(), false));
    assert_eq!(m.translate("\\FI_UNKNOWN"), ("\\FI_UNKNOWN".into(), true));
    assert_eq!(m.translate("\\Program Files\\a"), ("C:\\Program Files\\a".into(), true));
    assert_eq!(m.translate("rel\\p"), ("C:\\Windows\\rel\\p".into(), true));
}

#[test]
fn build_reads_real_system_and_volume_state() {
    let m = NtPathMap::build().unwrap();
    assert_eq!(m.windir[1], b':' as u16);
    assert_eq!(*m.windir.last().unwrap(), b'\\' as u16);
    assert!(!m.sysdir.is_empty());
    let drive = String::from_utf16(&m.windir[..3]).unwrap().to_uppercase();
    assert!(m.entries.iter().any(|(p, r)| s(p).starts_with("\\Device\\") && s(r).to_uppercase() == drive));
    assert_eq!(s(&m.entries.last().unwrap().0), "\\\\");
}
