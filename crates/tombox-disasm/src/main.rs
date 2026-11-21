//! disasm — disassemble a range of a PE by RVA and resolve direct call/branch
//! targets to UE symbol names (from the decompressed `-Symbols.bin`).
//!
//! Usage: disasm <pe> <rva_hex> [len] [--sym <symfile>] [--follow]

use iced_x86::{Decoder, DecoderOptions, FlowControl, Formatter, Instruction, IntelFormatter, OpKind};
use std::collections::VecDeque;

struct Sym {
    rva: u32,
    size: u32,
    name: String,
}

fn load_syms(path: &str) -> Vec<Sym> {
    let b = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("warning: cannot read symbols {path}: {e}");
            return Vec::new();
        }
    };
    let u32at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let count = u32at(0x2c) as usize;
    let rec_base = 0x30usize;
    let str_base = rec_base + count * 12;
    let mut syms: Vec<Sym> = Vec::with_capacity(count);
    for i in 0..count {
        let o = rec_base + i * 12;
        syms.push(Sym { rva: u32at(o), size: u32at(o + 4), name: String::new() });
    }
    // strings: skip repeated count u32, then count length-prefixed strings
    let mut so = str_base + 4;
    for s in syms.iter_mut() {
        if so + 4 > b.len() {
            break;
        }
        let len = u32::from_le_bytes([b[so], b[so + 1], b[so + 2], b[so + 3]]) as usize;
        so += 4;
        if len == 0 || so + len > b.len() {
            break;
        }
        s.name = String::from_utf8_lossy(&b[so..so + len - 1]).into_owned();
        so += len;
    }
    syms.sort_by_key(|s| s.rva);
    syms
}

fn resolve<'a>(syms: &'a [Sym], rva: u32) -> Option<(String, u32)> {
    if syms.is_empty() {
        return None;
    }
    let idx = match syms.binary_search_by(|s| s.rva.cmp(&rva)) {
        Ok(i) => i,
        Err(0) => return None,
        Err(i) => i - 1,
    };
    let s = &syms[idx];
    let off = rva - s.rva;
    let span = if s.size > 0 { s.size } else { 0x40 };
    if off < span || off < 0x800 {
        Some((s.name.clone(), off))
    } else {
        None
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: disasm <pe> <rva_hex> [len] [--sym <symfile>] [--follow]");
        std::process::exit(2);
    }
    let pe_path = &args[1];
    let start_rva = u32::from_str_radix(args[2].trim_start_matches("0x"), 16).expect("rva hex");
    let mut len = 0x200usize;
    let mut symfile = None;
    let mut follow = false;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--sym" => { i += 1; symfile = args.get(i).cloned(); }
            "--follow" => follow = true,
            s => { if let Ok(n) = usize::from_str_radix(s.trim_start_matches("0x"), 16) { len = n; } }
        }
        i += 1;
    }

    let data = std::fs::read(pe_path).expect("read pe");
    let pe = tombox_pe::PeFile::parse(&data).expect("parse pe");
    let image_base = pe.opt.image_base;
    let syms = symfile.as_deref().map(load_syms).unwrap_or_default();
    if !syms.is_empty() {
        eprintln!("[loaded {} symbols]", syms.len());
    }

    // Disassemble one range starting at rva; with --follow, also queue direct
    // call targets (one level) so we can see the functions WinMain invokes.
    let mut queue: VecDeque<(u32, usize)> = VecDeque::new();
    queue.push_back((start_rva, len));
    let mut done = std::collections::HashSet::new();

    while let Some((rva, length)) = queue.pop_front() {
        if !done.insert(rva) {
            continue;
        }
        let off = match pe.rva_to_offset(rva) {
            Ok(o) => o,
            Err(e) => { eprintln!("rva {rva:#x}: {e}"); continue; }
        };
        let end = (off + length).min(data.len());
        let bytes = &data[off..end];
        let name = resolve(&syms, rva).map(|(n, o)| format!("{n}+{o:#x}")).unwrap_or_default();
        println!("\n==== {:#x} (rva {:#x}) {} ====", image_base + rva as u64, rva, name);

        let mut decoder = Decoder::with_ip(64, bytes, image_base + rva as u64, DecoderOptions::NONE);
        let mut fmt = IntelFormatter::new();
        let mut instr = Instruction::default();
        let mut text = String::new();
        while decoder.can_decode() {
            decoder.decode_out(&mut instr);
            text.clear();
            fmt.format(&instr, &mut text);
            let va = instr.ip();
            let irva = (va - image_base) as u32;

            // Resolve a direct call/branch target to a symbol.
            let mut annot = String::new();
            let fc = instr.flow_control();
            if matches!(fc, FlowControl::Call | FlowControl::ConditionalBranch | FlowControl::UnconditionalBranch)
                && instr.op0_kind() == OpKind::NearBranch64
            {
                let tgt = instr.near_branch_target();
                let trva = (tgt - image_base) as u32;
                if let Some((n, o)) = resolve(&syms, trva) {
                    annot = format!("   ; -> {n}{}", if o == 0 { String::new() } else { format!("+{o:#x}") });
                }
                if follow && fc == FlowControl::Call && !done.contains(&trva) {
                    queue.push_back((trva, 0x120));
                }
            }
            println!("  {:#011x} {}{}", irva, text, annot);
            if instr.flow_control() == FlowControl::Return {
                // keep going a little past the first ret to catch tails
            }
        }
    }
}
