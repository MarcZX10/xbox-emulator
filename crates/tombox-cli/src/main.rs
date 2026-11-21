//! pe-inspect — inspect a PE/COFF image and report raw, verified facts.
//!
//! Usage:
//!   pe-inspect <file> [--json] [--imports] [--max N]
//!
//! Emits a human report by default, or a machine-readable JSON summary with
//! --json (suitable for writing into analysis/). Reports only what it parses;
//! Xbox/GDK module classification is explicitly labelled heuristic.

mod json;

use std::process::ExitCode;
use tombox_pe::constants as c;
use tombox_pe::directories::ImportEntry;
use tombox_pe::PeFile;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut path: Option<String> = None;
    let mut as_json = false;
    let mut imports_only = false;
    let mut max: usize = 60;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => as_json = true,
            "--imports" => imports_only = true,
            "--max" => {
                i += 1;
                max = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(60);
            }
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            s if !s.starts_with("--") => path = Some(s.to_string()),
            other => {
                eprintln!("unknown argument: {other}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let Some(path) = path else {
        print_help();
        return ExitCode::from(2);
    };

    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::from(1);
        }
    };

    let pe = match PeFile::parse(&data) {
        Ok(pe) => pe,
        Err(e) => {
            eprintln!("parse error in {path}: {e}");
            return ExitCode::from(1);
        }
    };

    if as_json {
        print_json(&path, data.len(), &pe);
    } else if imports_only {
        print_imports(&pe, max);
    } else {
        print_report(&path, data.len(), &pe, max);
    }
    ExitCode::SUCCESS
}

fn print_help() {
    println!("pe-inspect <file> [--json] [--imports] [--max N]");
    println!("  --json      emit a machine-readable JSON summary");
    println!("  --imports   print only the import list (grouped by module)");
    println!("  --max N     cap long lists at N entries in text output (0 = unlimited)");
}

fn cap(n: usize, max: usize) -> usize {
    if max == 0 {
        n
    } else {
        n.min(max)
    }
}

fn print_report(path: &str, file_len: usize, pe: &PeFile, max: usize) {
    println!("== File ==");
    println!("  path                 {path}");
    println!("  size                 {file_len} bytes ({:.2} MiB)", file_len as f64 / (1024.0 * 1024.0));
    println!("  (use PowerShell Get-FileHash for SHA-256)");

    println!("\n== COFF header ==");
    println!(
        "  machine              {:#06x}  {}",
        pe.coff.machine,
        c::machine_name(pe.coff.machine)
    );
    println!("  sections             {}", pe.coff.number_of_sections);
    println!("  timestamp            {:#010x}", pe.coff.time_date_stamp);
    println!(
        "  characteristics      {:#06x}  {}",
        pe.coff.characteristics,
        c::characteristics_flags(pe.coff.characteristics).join(" | ")
    );

    println!("\n== Optional header (PE32+) ==");
    let o = &pe.opt;
    println!("  linker version       {}.{}", o.major_linker_version, o.minor_linker_version);
    println!("  entry point (RVA)    {:#010x}", o.address_of_entry_point);
    println!("  image base           {:#018x}", o.image_base);
    println!("  size of image        {:#010x} ({} bytes)", o.size_of_image, o.size_of_image);
    println!("  size of headers      {:#010x}", o.size_of_headers);
    println!("  section alignment    {:#x}", o.section_alignment);
    println!("  file alignment       {:#x}", o.file_alignment);
    println!("  OS version           {}.{}", o.major_os_version, o.minor_os_version);
    println!("  subsystem version    {}.{}", o.major_subsystem_version, o.minor_subsystem_version);
    println!(
        "  subsystem            {}  {}",
        o.subsystem,
        c::subsystem_name(o.subsystem)
    );
    println!(
        "  dll characteristics  {:#06x}  {}",
        o.dll_characteristics,
        c::dll_characteristics_flags(o.dll_characteristics).join(" | ")
    );
    println!("  stack reserve/commit {:#x} / {:#x}", o.size_of_stack_reserve, o.size_of_stack_commit);
    println!("  heap reserve/commit  {:#x} / {:#x}", o.size_of_heap_reserve, o.size_of_heap_commit);

    println!("\n== Data directories ==");
    for (idx, d) in o.data_directories.iter().enumerate() {
        if d.virtual_address != 0 || d.size != 0 {
            println!(
                "  [{idx:2}] {:<22} rva={:#010x} size={:#x}",
                c::directory_name(idx),
                d.virtual_address,
                d.size
            );
        }
    }

    println!("\n== Sections ==");
    for s in &pe.sections {
        println!(
            "  {:<8} va={:#010x} vsize={:#010x} raw={:#010x} rawsize={:#010x}  {}",
            s.name,
            s.virtual_address,
            s.virtual_size,
            s.pointer_to_raw_data,
            s.size_of_raw_data,
            c::section_flags(s.characteristics).join("|")
        );
    }

    print_imports(pe, max);

    if let Some(di) = &pe.delay_imports {
        println!("\n== Delay-load imports ({} modules) ==", di.dlls.len());
        for d in &di.dlls {
            println!("  {} ({} functions)", d.name, d.functions.len());
        }
    }

    if let Some(ex) = &pe.exports {
        println!("\n== Exports ==");
        println!("  dll name             {}", ex.dll_name);
        println!("  ordinal base         {}", ex.ordinal_base);
        println!("  functions / names    {} / {}", ex.number_of_functions, ex.number_of_names);
        if !ex.forwarders.is_empty() {
            println!("  forwarders           {}", ex.forwarders.len());
        }
        let show = cap(ex.named.len(), max);
        for (n, ord) in ex.named.iter().take(show) {
            println!("    @{ord:<6} {n}");
        }
        if show < ex.named.len() {
            println!("    ... {} more named exports", ex.named.len() - show);
        }
    }

    if let Some(rel) = &pe.relocations {
        println!("\n== Base relocations ==");
        println!("  blocks               {}", rel.total_blocks);
        println!("  entries              {}", rel.total_entries);
        for (t, n) in &rel.by_type {
            println!("    type {:<2} {:<28} {}", t, c::reloc_type_name(*t), n);
        }
    } else if pe.coff.characteristics & c::IMAGE_FILE_RELOCS_STRIPPED != 0 {
        println!("\n== Base relocations ==\n  (stripped)");
    }

    if let Some(tls) = &pe.tls {
        println!("\n== TLS ==");
        println!("  index addr (VA)      {:#018x}", tls.address_of_index);
        println!("  callbacks (VA)       {:#018x}", tls.address_of_callbacks);
        println!("  callback count       {}", tls.callback_count);
        println!("  zero fill            {:#x}", tls.size_of_zero_fill);
    }

    print_xbox_classification(pe);

    if !pe.notes.0.is_empty() {
        println!("\n== Parser notes ({}) ==", pe.notes.0.len());
        for n in &pe.notes.0 {
            println!("  - {n}");
        }
    }
}

fn print_imports(pe: &PeFile, max: usize) {
    match &pe.imports {
        None => println!("\n== Imports ==\n  (none / not present)"),
        Some(imp) => {
            let total_funcs: usize = imp.dlls.iter().map(|d| d.functions.len()).sum();
            println!(
                "\n== Imports ({} modules, {} functions) ==",
                imp.dlls.len(),
                total_funcs
            );
            for d in &imp.dlls {
                let tag = if d.used_iat_fallback { " [ILT absent; read IAT]" } else { "" };
                println!("  {} ({} functions){}", d.name, d.functions.len(), tag);
                let show = cap(d.functions.len(), max);
                for f in d.functions.iter().take(show) {
                    match f {
                        ImportEntry::ByName { hint, name } => {
                            println!("      {name}  (hint {hint})");
                        }
                        ImportEntry::ByOrdinal(o) => println!("      #{o} (by ordinal)"),
                    }
                }
                if show < d.functions.len() {
                    println!("      ... {} more", d.functions.len() - show);
                }
            }
        }
    }
}

/// Heuristic classification of imported modules into likely Xbox/GDK vs. common
/// Windows categories. This is a *naming* heuristic to orient analysis — not a
/// claim about semantics. Unknown modules are listed as such.
fn print_xbox_classification(pe: &PeFile) {
    let mut modules: Vec<String> = Vec::new();
    if let Some(imp) = &pe.imports {
        modules.extend(imp.dlls.iter().map(|d| d.name.to_ascii_lowercase()));
    }
    if let Some(di) = &pe.delay_imports {
        modules.extend(di.dlls.iter().map(|d| d.name.to_ascii_lowercase()));
    }
    if modules.is_empty() {
        return;
    }
    modules.sort();
    modules.dedup();

    // Known/likely Xbox + GDK module name fragments (public GDK surface).
    let xbox_fragments = [
        "xgameruntime", "xgameplatform", "xgame", "xsapi", "xcurl", "xgameui",
        "gameinput", "d3d12_x", "d3d12_xs", "d3dx12", "pixevt", "xg_x", "xg_xs",
        "xmem", "xaudio2_x", "xapilib", "xboxservices", "durango", "scarlett",
    ];
    let mut xbox = Vec::new();
    let mut windowsish = Vec::new();
    let mut other = Vec::new();
    for m in &modules {
        if xbox_fragments.iter().any(|f| m.contains(f)) {
            xbox.push(m.clone());
        } else if m.ends_with(".dll")
            && (m.starts_with("api-ms-")
                || matches!(
                    m.as_str(),
                    "kernel32.dll" | "kernelbase.dll" | "ntdll.dll" | "user32.dll"
                        | "advapi32.dll" | "ole32.dll" | "oleaut32.dll" | "ws2_32.dll"
                        | "bcrypt.dll" | "crypt32.dll" | "dbghelp.dll" | "shell32.dll"
                        | "gdi32.dll" | "d3d12.dll" | "dxgi.dll" | "d3dcompiler_47.dll"
                        | "xinput1_4.dll" | "winmm.dll" | "msvcp140.dll" | "vcruntime140.dll"
                ))
        {
            windowsish.push(m.clone());
        } else {
            other.push(m.clone());
        }
    }

    println!("\n== Dependency classification (heuristic, by module name) ==");
    println!("  Likely Xbox/GDK modules ({}):", xbox.len());
    for m in &xbox {
        println!("    * {m}");
    }
    if xbox.is_empty() {
        println!("    (none matched the known-name heuristic — inspect 'other' below)");
    }
    println!("  Standard Windows modules ({}):", windowsish.len());
    for m in &windowsish {
        println!("    - {m}");
    }
    println!("  Other / unclassified modules ({}):", other.len());
    for m in &other {
        println!("    ? {m}");
    }
}

fn print_json(path: &str, file_len: usize, pe: &PeFile) {
    use json::esc;
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"path\": {},\n", esc(path)));
    out.push_str(&format!("  \"size_bytes\": {},\n", file_len));
    out.push_str(&format!("  \"machine\": {},\n", pe.coff.machine));
    out.push_str(&format!("  \"machine_name\": {},\n", esc(c::machine_name(pe.coff.machine))));
    out.push_str(&format!("  \"is_dll\": {},\n", pe.is_dll()));
    out.push_str(&format!("  \"entry_point_rva\": {},\n", pe.opt.address_of_entry_point));
    out.push_str(&format!("  \"image_base\": {},\n", pe.opt.image_base));
    out.push_str(&format!("  \"size_of_image\": {},\n", pe.opt.size_of_image));
    out.push_str(&format!("  \"subsystem\": {},\n", pe.opt.subsystem));
    out.push_str(&format!("  \"subsystem_name\": {},\n", esc(c::subsystem_name(pe.opt.subsystem))));

    // sections
    out.push_str("  \"sections\": [\n");
    for (i, s) in pe.sections.iter().enumerate() {
        out.push_str(&format!(
            "    {{ \"name\": {}, \"va\": {}, \"vsize\": {}, \"raw\": {}, \"rawsize\": {}, \"characteristics\": {} }}{}\n",
            esc(&s.name), s.virtual_address, s.virtual_size, s.pointer_to_raw_data, s.size_of_raw_data, s.characteristics,
            if i + 1 < pe.sections.len() { "," } else { "" }
        ));
    }
    out.push_str("  ],\n");

    // imports
    out.push_str("  \"imports\": [\n");
    if let Some(imp) = &pe.imports {
        for (i, d) in imp.dlls.iter().enumerate() {
            out.push_str(&format!("    {{ \"module\": {}, \"functions\": [", esc(&d.name)));
            for (j, f) in d.functions.iter().enumerate() {
                let s = match f {
                    ImportEntry::ByName { name, .. } => esc(name),
                    ImportEntry::ByOrdinal(o) => esc(&format!("#{o}")),
                };
                out.push_str(&s);
                if j + 1 < d.functions.len() {
                    out.push_str(", ");
                }
            }
            out.push_str("] }");
            out.push_str(if i + 1 < imp.dlls.len() { ",\n" } else { "\n" });
        }
    }
    out.push_str("  ],\n");

    // delay imports (module names only)
    out.push_str("  \"delay_imports\": [");
    if let Some(di) = &pe.delay_imports {
        for (i, d) in di.dlls.iter().enumerate() {
            out.push_str(&esc(&d.name));
            if i + 1 < di.dlls.len() {
                out.push_str(", ");
            }
        }
    }
    out.push_str("],\n");

    // exports (named only, with ordinals)
    out.push_str("  \"exports\": ");
    match &pe.exports {
        None => out.push_str("null,\n"),
        Some(ex) => {
            out.push_str("{\n");
            out.push_str(&format!("    \"dll_name\": {},\n", esc(&ex.dll_name)));
            out.push_str(&format!("    \"number_of_functions\": {},\n", ex.number_of_functions));
            out.push_str(&format!("    \"number_of_names\": {},\n", ex.number_of_names));
            out.push_str(&format!("    \"forwarder_count\": {},\n", ex.forwarders.len()));
            out.push_str("    \"named\": [");
            for (j, (n, ord)) in ex.named.iter().enumerate() {
                out.push_str(&format!("{{\"name\": {}, \"ordinal\": {}}}", esc(n), ord));
                if j + 1 < ex.named.len() {
                    out.push_str(", ");
                }
            }
            out.push_str("]\n  },\n");
        }
    }

    // relocations summary
    out.push_str("  \"relocations\": ");
    match &pe.relocations {
        None => out.push_str("null,\n"),
        Some(rel) => {
            out.push_str(&format!(
                "{{ \"total_blocks\": {}, \"total_entries\": {}, \"by_type\": [",
                rel.total_blocks, rel.total_entries
            ));
            for (j, (t, n)) in rel.by_type.iter().enumerate() {
                out.push_str(&format!("{{\"type\": {t}, \"count\": {n}}}"));
                if j + 1 < rel.by_type.len() {
                    out.push_str(", ");
                }
            }
            out.push_str("] },\n");
        }
    }

    // tls
    out.push_str("  \"tls\": ");
    match &pe.tls {
        None => out.push_str("null,\n"),
        Some(tls) => out.push_str(&format!(
            "{{ \"address_of_callbacks\": {}, \"callback_count\": {}, \"size_of_zero_fill\": {} }},\n",
            tls.address_of_callbacks, tls.callback_count, tls.size_of_zero_fill
        )),
    }

    out.push_str(&format!("  \"notes\": {}\n", notes_json(&pe.notes.0)));
    out.push_str("}\n");
    print!("{out}");
}

fn notes_json(notes: &[String]) -> String {
    let mut s = String::from("[");
    for (i, n) in notes.iter().enumerate() {
        s.push_str(&json::esc(n));
        if i + 1 < notes.len() {
            s.push_str(", ");
        }
    }
    s.push(']');
    s
}
