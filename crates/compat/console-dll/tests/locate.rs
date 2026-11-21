use console_dll::*;
use std::path::Path;

static ABSENT: Console = Console::new(Spec {
    name: "tombox_absent_console.dll",
    env: "TOMBOX_ABSENT_CONSOLE_TEST",
    alias: "tombox_absent_console_alias.dll",
    probe: "Nothing",
    emulated_imports: &[],
});

static WRONG_PROBE: Console = Console::new(Spec {
    name: "dxbc2dxil_xs.dll",
    env: "TOMBOX_WRONG_PROBE_TEST",
    alias: "tombox_wrong_probe_alias.dll",
    probe: "NotAnExportOfTheConsoleDll",
    emulated_imports: &[],
});

static NEEDS_COMPAT: Console = Console::new(Spec {
    name: "dxbc2dxil_xs.dll",
    env: "TOMBOX_NEEDS_COMPAT_TEST",
    alias: "tombox_needs_compat_alias.dll",
    probe: "SCDxil_ConvertDxbcToDxil",
    emulated_imports: &["tombox_missing_compat.dll"],
});

#[test]
fn absent_console_dll_is_reported_not_faked() {
    assert_eq!(ABSENT.module(), 0);
    assert!(ABSENT.proc("Nothing").is_none());
    assert!(ABSENT.path().is_none());
}

#[test]
fn a_dll_without_the_probe_export_is_never_used() {
    assert!(!WRONG_PROBE.candidates().is_empty());
    assert_eq!(WRONG_PROBE.module(), 0);
}

#[test]
fn missing_emulator_compat_import_blocks_the_console_load() {
    assert_eq!(NEEDS_COMPAT.module(), 0);
}

#[test]
fn dumps_prefer_the_newest_console_os_build() {
    let all = dumps(Path::new(GAMES), "dxcompiler_xs.dll");
    assert!(all.len() >= 2, "{all:?}");
    let builds: Vec<u64> = all.iter().map(|p| os_build(p)).collect();
    assert!(builds.windows(2).all(|w| w[0] >= w[1]), "{builds:x?}");
    assert!(builds[0] > 0);
    for p in &all {
        assert!(p.ends_with(r"Windows\System32\dxcompiler_xs.dll"), "{}", p.display());
    }
}

#[test]
fn fixed_file_version_is_read_from_the_resource() {
    let all = dumps(Path::new(GAMES), "dxbc2dxil_xs.dll");
    let v = file_version(&all[0]);
    assert_eq!(v >> 48, 2210, "{v:x}");
}
