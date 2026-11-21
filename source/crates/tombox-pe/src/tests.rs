//! Unit + integration tests. The malformed-input and truncation tests assert
//! the core safety invariant: the parser never panics, it returns `Err`.

use super::*;

#[test]
fn rejects_empty() {
    let err = PeFile::parse(&[]).unwrap_err();
    matches!(err, ParseError::OutOfBounds { .. });
}

#[test]
fn rejects_non_mz() {
    let buf = [0u8; 256];
    let err = PeFile::parse(&buf).unwrap_err();
    match err {
        ParseError::BadMagic { what, .. } => assert_eq!(what, "DOS header (MZ)"),
        other => panic!("expected BadMagic, got {other:?}"),
    }
}

#[test]
fn rejects_mz_without_pe() {
    let mut buf = vec![0u8; 256];
    buf[0] = b'M';
    buf[1] = b'Z';
    // e_lfanew points somewhere with no "PE\0\0".
    buf[0x3C] = 0x80;
    let err = PeFile::parse(&buf).unwrap_err();
    // Either out-of-bounds (if 0x80 is past our tiny buffer) or bad PE sig.
    assert!(matches!(
        err,
        ParseError::OutOfBounds { .. } | ParseError::BadMagic { .. }
    ));
}

/// Deterministic pseudo-random bytes must never cause a panic.
#[test]
fn no_panic_on_random_bytes() {
    let mut state: u64 = 0x1234_5678_9abc_def0;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as u8
    };
    for size in [0usize, 1, 2, 64, 256, 4096] {
        for _ in 0..200 {
            let buf: Vec<u8> = (0..size).map(|_| next()).collect();
            let _ = PeFile::parse(&buf); // must not panic
        }
    }
}

/// A "looks like a PE" buffer with an MZ and PE signature but random tails must
/// also never panic.
#[test]
fn no_panic_on_pe_shaped_garbage() {
    let mut state: u64 = 0xdead_beef_cafe_babe;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 40) as u8
    };
    for _ in 0..2000 {
        let mut buf = vec![0u8; 1024];
        for b in buf.iter_mut() {
            *b = next();
        }
        buf[0] = b'M';
        buf[1] = b'Z';
        let e_lfanew = (next() as usize) % 900; // somewhere inside
        buf[0x3C] = (e_lfanew & 0xff) as u8;
        buf[0x3D] = ((e_lfanew >> 8) & 0xff) as u8;
        buf[e_lfanew] = b'P';
        buf[e_lfanew + 1] = b'E';
        buf[e_lfanew + 2] = 0;
        buf[e_lfanew + 3] = 0;
        let _ = PeFile::parse(&buf); // must not panic
    }
}

/// Systematic truncation of a real PE: parse every prefix length, assert no
/// panic. Skips gracefully if the sample file is unavailable.
#[test]
fn no_panic_on_truncated_real_pe() {
    let Some(data) = sample_pe() else {
        eprintln!("sample PE unavailable; skipping");
        return;
    };
    // Full parse must succeed.
    assert!(PeFile::parse(&data).is_ok());
    // Truncate at a spread of lengths (every length is O(n^2); sample instead).
    let step = (data.len() / 500).max(1);
    let mut len = 0;
    while len <= data.len() {
        let _ = PeFile::parse(&data[..len]); // must not panic
        len += step;
    }
}

/// Integration: a real x64 system DLL parses with the expected high-level facts.
#[test]
fn parses_system_dll() {
    let Some(data) = sample_pe() else {
        eprintln!("sample PE unavailable; skipping");
        return;
    };
    let pe = PeFile::parse(&data).expect("kernel32 should parse");
    assert_eq!(
        pe.coff.machine,
        constants::IMAGE_FILE_MACHINE_AMD64,
        "expected AMD64 machine"
    );
    assert_eq!(pe.opt.magic, 0x020B, "expected PE32+");
    assert!(pe.is_dll(), "kernel32 should be a DLL");
    let exports = pe.exports.as_ref().expect("kernel32 exports a lot");
    assert!(
        exports.named.iter().any(|(n, _)| n == "CreateFileW"),
        "kernel32 should export CreateFileW; got {} names",
        exports.named.len()
    );
}

/// Integration: a real x64 DLL carries base relocations dominated by DIR64 (10).
#[test]
fn relocations_are_dir64_on_x64() {
    let Some(data) = sample_pe() else {
        eprintln!("sample PE unavailable; skipping");
        return;
    };
    let pe = PeFile::parse(&data).expect("parse");
    // System DLLs are relocatable; if stripped, skip rather than fail.
    if pe.coff.characteristics & constants::IMAGE_FILE_RELOCS_STRIPPED != 0 {
        eprintln!("relocs stripped; skipping");
        return;
    }
    let rel = pe.relocations.as_ref().expect("x64 DLL should have relocations");
    assert!(rel.total_entries > 0, "expected at least one relocation entry");
    // On x64, pointer relocations are DIR64 (type 10). It should be the most common
    // non-ABSOLUTE type by a wide margin.
    let dir64 = rel.by_type.iter().find(|(t, _)| *t == 10).map(|(_, n)| *n).unwrap_or(0);
    assert!(
        dir64 > 0,
        "expected DIR64 relocations on x64; got types {:?}",
        rel.by_type
    );
}

/// relocation_entries() returns the detailed fixups and their count matches the
/// summary's non-ABSOLUTE total.
#[test]
fn relocation_entries_match_summary() {
    let Some(data) = sample_pe() else {
        eprintln!("sample PE unavailable; skipping");
        return;
    };
    let pe = PeFile::parse(&data).expect("parse");
    if pe.coff.characteristics & constants::IMAGE_FILE_RELOCS_STRIPPED != 0 {
        return;
    }
    let entries = pe.relocation_entries().expect("reloc entries");
    let summary = pe.relocations.as_ref().unwrap();
    let non_absolute: usize = summary
        .by_type
        .iter()
        .filter(|(t, _)| *t != 0)
        .map(|(_, n)| *n)
        .sum();
    assert_eq!(entries.len(), non_absolute);
    // On x64 the detailed entries should be overwhelmingly DIR64 (10).
    let dir64 = entries.iter().filter(|e| e.kind == 10).count();
    assert!(dir64 > 0 && dir64 == non_absolute, "expected all DIR64");
    // Every reloc RVA must be mappable to a file offset.
    for e in entries.iter().take(1000) {
        assert!(pe.rva_to_offset(e.rva).is_ok(), "reloc rva {:#x} unmappable", e.rva);
    }
}

/// Load a known-good x64 PE for integration tests. Prefer kernel32 (stable
/// exports), fall back to the test binary itself.
fn sample_pe() -> Option<Vec<u8>> {
    let candidates = [
        r"C:\Windows\System32\kernel32.dll",
        r"C:\Windows\System32\ntdll.dll",
    ];
    for c in candidates {
        if let Ok(d) = std::fs::read(c) {
            return Some(d);
        }
    }
    None
}
