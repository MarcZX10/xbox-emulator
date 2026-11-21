//! zip-inspect — safely inventory and vet a ZIP archive before extraction.
//!
//! Reads only the archive tail (EOCD + central directory); performs no
//! decompression and writes nothing. Exits non-zero if any entry is unsafe to
//! extract, so it can gate an extraction step.
//!
//! Usage: zip-inspect <file.zip> [--list] [--top N] [--ratio R]

use std::collections::BTreeMap;
use std::process::ExitCode;
use tombox_archive::{read_inventory, validate, Severity};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut path = None;
    let mut list = false;
    let mut top = 15usize;
    let mut ratio = 1000.0f64;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--list" => list = true,
            "--top" => {
                i += 1;
                top = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(15);
            }
            "--ratio" => {
                i += 1;
                ratio = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1000.0);
            }
            s if !s.starts_with("--") => path = Some(s.to_string()),
            other => {
                eprintln!("unknown arg: {other}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    let Some(path) = path else {
        eprintln!("usage: zip-inspect <file.zip> [--list] [--top N] [--ratio R]");
        return ExitCode::from(2);
    };

    let inv = match read_inventory(&path) {
        Ok(inv) => inv,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    let report = validate(&inv, ratio);

    println!("== Archive ==");
    println!("  path                 {path}");
    println!("  ZIP64                {}", inv.zip64);
    println!("  entries (declared)   {}", inv.entry_count);
    println!("  entries (parsed)     {}", inv.entries.len());
    println!("  central dir          offset={} size={}", inv.cd_offset, inv.cd_size);
    println!("\n== Contents ==");
    println!("  files                {}", report.file_count);
    println!("  directories          {}", report.dir_count);
    println!("  symlinks             {}", report.symlink_count);
    println!(
        "  total uncompressed   {} bytes ({:.2} GiB)",
        report.total_uncompressed,
        report.total_uncompressed as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    println!(
        "  total compressed     {} bytes ({:.2} GiB)",
        report.total_compressed,
        report.total_compressed as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    println!("  max per-entry ratio  {:.1}x", report.max_ratio);

    // Top-level directory breakdown.
    let mut top_level: BTreeMap<String, usize> = BTreeMap::new();
    for e in &inv.entries {
        let first = e.name.replace('\\', "/");
        let seg = first.split('/').next().unwrap_or("").to_string();
        *top_level.entry(seg).or_insert(0) += 1;
    }
    println!("\n== Top-level entries ==");
    for (k, v) in &top_level {
        println!("  {:<40} {} entries", if k.is_empty() { "<root>" } else { k }, v);
    }

    // Largest entries by uncompressed size.
    let mut by_size: Vec<_> = inv.entries.iter().filter(|e| !e.is_dir).collect();
    by_size.sort_by_key(|e| std::cmp::Reverse(e.uncomp_size));
    println!("\n== Largest {} files ==", top.min(by_size.len()));
    for e in by_size.iter().take(top) {
        println!(
            "  {:>14} B  method={:<2}  {}",
            e.uncomp_size, e.method, e.name
        );
    }

    // Method histogram.
    let mut methods: BTreeMap<u16, usize> = BTreeMap::new();
    for e in &inv.entries {
        *methods.entry(e.method).or_insert(0) += 1;
    }
    println!("\n== Compression methods ==");
    for (m, n) in &methods {
        let name = match m {
            0 => "store",
            8 => "deflate",
            9 => "deflate64",
            12 => "bzip2",
            14 => "lzma",
            93 => "zstd",
            95 => "xz",
            _ => "other",
        };
        println!("  method {:<3} {:<10} {}", m, name, n);
    }

    if list {
        println!("\n== All entries ==");
        for e in &inv.entries {
            println!(
                "  {}{}  ({} -> {} bytes, m{})",
                e.name,
                if e.is_symlink { " [symlink]" } else { "" },
                e.comp_size,
                e.uncomp_size,
                e.method
            );
        }
    }

    println!("\n== Safety findings ({}) ==", report.findings.len());
    if report.findings.is_empty() {
        println!("  none");
    }
    for f in &report.findings {
        let sev = match f.severity {
            Severity::Block => "BLOCK",
            Severity::Warn => "warn",
        };
        println!("  [{sev}] {}  — {}", f.entry, f.issue);
    }

    if report.is_safe_to_extract() {
        println!("\nVERDICT: safe to extract (no blocking findings).");
        ExitCode::SUCCESS
    } else {
        let blocks = report
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Block)
            .count();
        println!("\nVERDICT: UNSAFE — {blocks} blocking finding(s). Do not extract as-is.");
        ExitCode::from(3)
    }
}
