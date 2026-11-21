use crate::image::Image;
use crate::names::{is_identifier, is_loader_api};
use iced_x86::{Decoder, DecoderOptions, FlowControl, Instruction, Mnemonic, OpKind, Register};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcRef {
    pub name: Option<String>,
    pub call_rva: u32,
    pub null_checked: bool,
}

#[derive(Debug, Clone)]
pub struct DynSite {
    pub dll: String,
    pub wide: bool,
    pub string_rva: u32,
    pub lea_rva: u32,
    pub loader: Option<String>,
    pub loader_call_rva: Option<u32>,
    pub null_checked: bool,
    pub failure_branch_rva: Option<u32>,
    pub procs: Vec<ProcRef>,
}

pub fn literal_strings(img: &Image) -> HashMap<u32, (String, bool)> {
    let mut out = HashMap::new();
    for s in img.sections.iter().filter(|s| !s.exec) {
        let lo = s.raw_off as usize;
        let hi = (lo + s.raw_size as usize).min(img.bytes.len());
        if lo >= hi {
            continue;
        }
        let data = &img.bytes[lo..hi];
        let mut i = 0usize;
        while i < data.len() {
            let start = i;
            while i < data.len() && (0x20..0x7f).contains(&data[i]) {
                i += 1;
            }
            if i - start >= 3 && i < data.len() && data[i] == 0 {
                out.insert(s.rva + start as u32, (String::from_utf8_lossy(&data[start..i]).into_owned(), false));
            }
            i += 1;
        }
        let mut i = 0usize;
        while i + 1 < data.len() {
            let start = i;
            let mut n = 0usize;
            while i + 1 < data.len() && (0x20..0x7f).contains(&data[i]) && data[i + 1] == 0 {
                i += 2;
                n += 1;
            }
            if n >= 3 && i + 1 < data.len() && data[i] == 0 && data[i + 1] == 0 {
                let txt: String = data[start..i].iter().step_by(2).map(|&c| c as char).collect();
                out.entry(s.rva + start as u32).or_insert((txt, true));
            }
            i = if n == 0 { start + 2 } else { i + 2 };
        }
    }
    out
}

fn decode_exec(img: &Image) -> Vec<Instruction> {
    let mut v = Vec::new();
    for s in img.sections.iter().filter(|s| s.exec) {
        let lo = s.raw_off as usize;
        let hi = (lo + s.raw_size.min(s.vsize.max(s.raw_size)) as usize).min(img.bytes.len());
        if lo >= hi {
            continue;
        }
        let mut d = Decoder::with_ip(64, &img.bytes[lo..hi], img.image_base + s.rva as u64, DecoderOptions::NONE);
        let mut ins = Instruction::default();
        while d.can_decode() {
            d.decode_out(&mut ins);
            v.push(ins);
        }
    }
    v
}

fn rip_target(ins: &Instruction, base: u64) -> Option<u32> {
    if ins.is_ip_rel_memory_operand() {
        let t = ins.ip_rel_memory_address();
        if t >= base && t - base < u32::MAX as u64 {
            return Some((t - base) as u32);
        }
    }
    None
}

fn null_check(code: &[Instruction], call_idx: usize) -> (bool, Option<u64>) {
    let mut regs: HashSet<Register> = HashSet::new();
    regs.insert(Register::RAX);
    let end = (call_idx + 12).min(code.len());
    let mut k = call_idx + 1;
    while k < end {
        let ins = &code[k];
        let r0 = ins.op0_register().full_register();
        match ins.mnemonic() {
            Mnemonic::Mov if ins.op0_kind() == OpKind::Register && ins.op1_kind() == OpKind::Register => {
                if regs.contains(&ins.op1_register().full_register()) {
                    regs.insert(r0);
                } else {
                    regs.remove(&r0);
                }
            }
            Mnemonic::Neg if ins.op0_kind() == OpKind::Register && regs.contains(&r0) => return (true, None),
            Mnemonic::Test | Mnemonic::Cmp if ins.op0_kind() == OpKind::Register && regs.contains(&r0) => {
                let zero_cmp = ins.mnemonic() == Mnemonic::Test && ins.op1_kind() == OpKind::Register && ins.op1_register().full_register() == r0
                    || ins.mnemonic() == Mnemonic::Cmp && matches!(ins.op1_kind(), OpKind::Immediate8to64 | OpKind::Immediate8to32 | OpKind::Immediate32to64 | OpKind::Immediate8 | OpKind::Immediate32) && ins.immediate(1) == 0;
                if zero_cmp {
                    for b in code.iter().take((k + 4).min(code.len())).skip(k + 1) {
                        if b.flow_control() == FlowControl::ConditionalBranch {
                            return (true, Some(b.near_branch_target()));
                        }
                        if b.flow_control() != FlowControl::Next {
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
        if matches!(ins.flow_control(), FlowControl::Call | FlowControl::IndirectCall | FlowControl::Return | FlowControl::UnconditionalBranch | FlowControl::IndirectBranch) {
            break;
        }
        k += 1;
    }
    (false, None)
}

fn stored_globals(code: &[Instruction], call_idx: usize, base: u64) -> Vec<u32> {
    let mut regs: HashSet<Register> = HashSet::new();
    regs.insert(Register::RAX);
    let mut out = Vec::new();
    for ins in code.iter().take((call_idx + 8).min(code.len())).skip(call_idx + 1) {
        if ins.mnemonic() == Mnemonic::Mov && ins.op1_kind() == OpKind::Register {
            let src = ins.op1_register().full_register();
            if ins.op0_kind() == OpKind::Memory && regs.contains(&src) {
                if let Some(g) = rip_target(ins, base) {
                    out.push(g);
                }
            } else if ins.op0_kind() == OpKind::Register && regs.contains(&src) {
                regs.insert(ins.op0_register().full_register());
            }
        }
        if ins.flow_control() != FlowControl::Next {
            break;
        }
    }
    out
}

fn callee_name(c: &Instruction, base: u64, iat: &HashMap<u32, (String, String)>, alias: &HashMap<Register, String>) -> Option<String> {
    if c.op0_kind() == OpKind::Memory {
        rip_target(c, base).and_then(|tt| iat.get(&tt)).map(|x| x.1.clone())
    } else if c.op0_kind() == OpKind::Register {
        alias.get(&c.op0_register().full_register()).cloned()
    } else {
        None
    }
}

fn is_gpa_wrapper(code: &[Instruction], by_ip: &HashMap<u64, usize>, target: u64, base: u64, iat: &HashMap<u32, (String, String)>, memo: &mut HashMap<u64, bool>) -> bool {
    if let Some(v) = memo.get(&target) {
        return *v;
    }
    let mut found = false;
    if let Some(&start) = by_ip.get(&target) {
        let empty = HashMap::new();
        for c in code.iter().skip(start).take(200) {
            if c.flow_control() == FlowControl::Return || c.mnemonic() == Mnemonic::Int3 {
                break;
            }
            if c.mnemonic() == Mnemonic::Call && callee_name(c, base, iat, &empty).as_deref() == Some("GetProcAddress") {
                found = true;
                break;
            }
        }
    }
    memo.insert(target, found);
    found
}

pub fn listing(img: &Image, rva: u32, count: usize) -> Vec<String> {
    use iced_x86::{Formatter, IntelFormatter};
    let Some(off) = img.rva_to_off(rva) else { return Vec::new() };
    let end = (off + count * 15).min(img.bytes.len());
    let iat = img.iat_map();
    let mut d = Decoder::with_ip(64, &img.bytes[off..end], img.image_base + rva as u64, DecoderOptions::NONE);
    let mut f = IntelFormatter::new();
    let mut out = Vec::new();
    let mut ins = Instruction::default();
    while d.can_decode() && out.len() < count {
        d.decode_out(&mut ins);
        let mut s = String::new();
        f.format(&ins, &mut s);
        let r = (ins.ip() - img.image_base) as u32;
        if let Some((m, n)) = rip_target(&ins, img.image_base).and_then(|t| iat.get(&t)) {
            s.push_str(&format!("  ; {m}!{n}"));
        }
        out.push(format!("{r:#x}: {s}"));
    }
    out
}

pub fn analyze(img: &Image) -> Vec<DynSite> {
    let strings = literal_strings(img);
    let iat = img.iat_map();
    let code = decode_exec(img);
    analyze_code(&code, img.image_base, &strings, &iat)
}

const ARG_REGS: [Register; 3] = [Register::RDX, Register::R8, Register::R9];

pub fn analyze_code(code: &[Instruction], base: u64, strings: &HashMap<u32, (String, bool)>, iat: &HashMap<u32, (String, String)>) -> Vec<DynSite> {
    let is_dll_lit = |t: u32| strings.get(&t).filter(|(s, _)| s.to_ascii_lowercase().ends_with(".dll"));
    let ident = |t: u32| strings.get(&t).filter(|(s, w)| !*w && is_identifier(s)).map(|(s, _)| s.clone());
    let by_ip: HashMap<u64, usize> = code.iter().enumerate().map(|(i, c)| (c.ip(), i)).collect();
    let mut memo: HashMap<u64, bool> = HashMap::new();
    let mut out: Vec<DynSite> = Vec::new();
    let mut handle_globals: Vec<(usize, u32)> = Vec::new();
    for (i, ins) in code.iter().enumerate() {
        if ins.mnemonic() != Mnemonic::Lea {
            continue;
        }
        let Some(t) = rip_target(ins, base) else { continue };
        let Some((dll, wide)) = is_dll_lit(t) else { continue };
        let mut site = DynSite {
            dll: dll.clone(),
            wide: *wide,
            string_rva: t,
            lea_rva: (ins.ip() - base) as u32,
            loader: None,
            loader_call_rva: None,
            null_checked: false,
            failure_branch_rva: None,
            procs: Vec::new(),
        };
        let mut alias: HashMap<Register, String> = HashMap::new();
        let mut arg_names: HashMap<Register, String> = HashMap::new();
        let mut int3_run = 0;
        let limit = (i + 4000).min(code.len());
        for j in i + 1..limit {
            let c = &code[j];
            if c.mnemonic() == Mnemonic::Int3 {
                int3_run += 1;
                if int3_run >= 2 {
                    break;
                }
                continue;
            }
            int3_run = 0;
            if c.flow_control() == FlowControl::Return {
                break;
            }
            if c.mnemonic() == Mnemonic::Lea {
                let r = c.op0_register().full_register();
                if let Some(tt) = rip_target(c, base) {
                    if site.loader.is_some() && is_dll_lit(tt).is_some() {
                        break;
                    }
                    match ident(tt) {
                        Some(n) if ARG_REGS.contains(&r) => {
                            arg_names.insert(r, n);
                        }
                        _ => {
                            arg_names.remove(&r);
                        }
                    }
                } else {
                    arg_names.remove(&r);
                }
                continue;
            }
            if c.mnemonic() == Mnemonic::Mov && c.op0_kind() == OpKind::Register {
                let r = c.op0_register().full_register();
                arg_names.remove(&r);
                if c.op1_kind() == OpKind::Memory {
                    if let Some(name) = rip_target(c, base).and_then(|tt| iat.get(&tt)) {
                        alias.insert(r, name.1.clone());
                        continue;
                    }
                }
                alias.remove(&r);
                continue;
            }
            if c.mnemonic() != Mnemonic::Call {
                continue;
            }
            let callee = callee_name(c, base, iat, &alias);
            let call_rva = (c.ip() - base) as u32;
            match callee.as_deref() {
                Some(n) if site.loader.is_none() && is_loader_api(n) => {
                    let (chk, br) = null_check(code, j);
                    site.loader = Some(n.to_string());
                    site.loader_call_rva = Some(call_rva);
                    site.null_checked = chk;
                    site.failure_branch_rva = br.map(|b| (b - base) as u32);
                    for g in stored_globals(code, j, base) {
                        handle_globals.push((out.len(), g));
                    }
                }
                Some("GetProcAddress") if site.loader.is_some() => {
                    let (chk, _) = null_check(code, j);
                    site.procs.push(ProcRef { name: arg_names.remove(&Register::RDX), call_rva, null_checked: chk });
                }
                None if site.loader.is_some() && c.op0_kind() == OpKind::NearBranch64 => {
                    if is_gpa_wrapper(code, &by_ip, c.near_branch_target(), base, iat, &mut memo) {
                        if let Some(n) = ARG_REGS.iter().find_map(|r| arg_names.get(r).cloned()) {
                            site.procs.push(ProcRef { name: Some(n), call_rva, null_checked: false });
                        }
                    }
                }
                _ => {}
            }
            for r in ARG_REGS {
                arg_names.remove(&r);
            }
        }
        if site.loader.is_some() {
            out.push(site);
        }
    }
    if !handle_globals.is_empty() {
        let empty = HashMap::new();
        for (j, c) in code.iter().enumerate() {
            if c.mnemonic() != Mnemonic::Call || callee_name(c, base, iat, &empty).as_deref() != Some("GetProcAddress") {
                continue;
            }
            let mut rcx_global = None;
            let mut rdx_name = None;
            for p in code[j.saturating_sub(10)..j].iter() {
                let r = p.op0_register().full_register();
                if p.mnemonic() == Mnemonic::Mov && r == Register::RCX && p.op1_kind() == OpKind::Memory {
                    rcx_global = rip_target(p, base);
                }
                if p.mnemonic() == Mnemonic::Lea && r == Register::RDX {
                    rdx_name = rip_target(p, base).and_then(ident);
                }
            }
            let (Some(g), Some(name)) = (rcx_global, rdx_name) else { continue };
            for (si, hg) in &handle_globals {
                if *hg == g && !out[*si].procs.iter().any(|p| p.name.as_deref() == Some(name.as_str())) {
                    let (chk, _) = null_check(code, j);
                    out[*si].procs.push(ProcRef { name: Some(name.clone()), call_rva: (c.ip() - base) as u32, null_checked: chk });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testpe::{self, IMAGE_BASE, TEXT_RVA};
    use std::path::PathBuf;

    fn rel(at: u32, len: u32, target: u32) -> [u8; 4] {
        ((target as i64 - (at + len) as i64) as i32).to_le_bytes()
    }

    fn code(rv: &HashMap<String, u32>, ll: u32, gpa: u32) -> Vec<u8> {
        let mut c = Vec::new();
        let mut at = TEXT_RVA;
        let emit =|c: &mut Vec<u8>, at: &mut u32, op: &[u8], target: Option<u32>| {
            c.extend_from_slice(op);
            if let Some(t) = target {
                let len = op.len() as u32 + 4;
                c.extend_from_slice(&rel(*at, len, t));
                *at += len;
            } else {
                *at += op.len() as u32;
            }
        };
        emit(&mut c, &mut at, &[0x48, 0x8D, 0x0D], Some(rv["AudioHALWrapper.dll"]));
        emit(&mut c, &mut at, &[0x33, 0xD2], None);
        emit(&mut c, &mut at, &[0x45, 0x33, 0xC0], None);
        emit(&mut c, &mut at, &[0xFF, 0x15], Some(ll));
        emit(&mut c, &mut at, &[0x48, 0x89, 0x07], None);
        emit(&mut c, &mut at, &[0x48, 0x85, 0xC0], None);
        emit(&mut c, &mut at, &[0x74, 0x20], None);
        emit(&mut c, &mut at, &[0x48, 0x8B, 0xC8], None);
        emit(&mut c, &mut at, &[0x48, 0x8D, 0x15], Some(rv["DecodePacket"]));
        emit(&mut c, &mut at, &[0xFF, 0x15], Some(gpa));
        emit(&mut c, &mut at, &[0x48, 0x85, 0xC0], None);
        emit(&mut c, &mut at, &[0x75, 0x02], None);
        emit(&mut c, &mut at, &[0x48, 0x8B, 0x0F], None);
        emit(&mut c, &mut at, &[0x48, 0x8D, 0x15], Some(rv["EncodeFrame"]));
        emit(&mut c, &mut at, &[0xFF, 0x15], Some(gpa));
        emit(&mut c, &mut at, &[0x48, 0x89, 0x47, 0x08], None);
        emit(&mut c, &mut at, &[0xC3], None);
        c
    }

    #[test]
    fn handle_stored_in_global_and_resolved_elsewhere() {
        let strs: &[(&str, bool)] = &[("xgameruntime.dll", false), ("QueryApiImpl", false), ("GLOBALSLOT", false)];
        let imports: &[(&str, &[&str])] = &[("api-ms-win-core-libraryloader-l1-2-0.dll", &["LoadLibraryExA", "GetProcAddress"])];
        let (first, rv) = testpe::build_full(&[], imports, &[], &[0xC3], strs);
        let img = Image::from_bytes(PathBuf::from("b.dll"), first).unwrap();
        let rows = img.import_rows();
        let ll = rows.iter().find(|r| r.entry.label() == "LoadLibraryExA").unwrap().iat_rva;
        let gpa = rows.iter().find(|r| r.entry.label() == "GetProcAddress").unwrap().iat_rva;
        let g = rv["GLOBALSLOT"];
        let mut c = Vec::new();
        let mut at = TEXT_RVA;
        let mut emit = |op: &[u8], target: Option<u32>| {
            c.extend_from_slice(op);
            let len = op.len() as u32 + if target.is_some() { 4 } else { 0 };
            if let Some(t) = target {
                c.extend_from_slice(&rel(at, len, t));
            }
            at += len;
        };
        emit(&[0x48, 0x8D, 0x0D], Some(rv["xgameruntime.dll"]));
        emit(&[0xFF, 0x15], Some(ll));
        emit(&[0x48, 0x89, 0x05], Some(g));
        emit(&[0x48, 0xF7, 0xD8], None);
        emit(&[0xC3, 0xCC, 0xCC], None);
        emit(&[0x48, 0x8B, 0x0D], Some(g));
        emit(&[0x48, 0x8D, 0x15], Some(rv["QueryApiImpl"]));
        emit(&[0xFF, 0x15], Some(gpa));
        emit(&[0xC3], None);
        let (bytes, _) = testpe::build_full(&[], imports, &[], &c, strs);
        let img = Image::from_bytes(PathBuf::from("b.dll"), bytes).unwrap();
        let sites = analyze(&img);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].loader.as_deref(), Some("LoadLibraryExA"));
        assert!(sites[0].null_checked);
        assert_eq!(sites[0].procs.len(), 1);
        assert_eq!(sites[0].procs[0].name.as_deref(), Some("QueryApiImpl"));
        let l = listing(&img, TEXT_RVA, 3);
        assert_eq!(l.len(), 3);
        assert!(l[1].contains("LoadLibraryExA"), "{l:?}");
    }

    #[test]
    fn finds_loader_site_and_procs() {
        let strs: &[(&str, bool)] = &[("AudioHALWrapper.dll", true), ("DecodePacket", false), ("EncodeFrame", false)];
        let imports: &[(&str, &[&str])] = &[("api-ms-win-core-libraryloader-l1-2-0.dll", &["LoadLibraryExW", "GetProcAddress"])];
        let (first, rv) = testpe::build_full(&[], imports, &[], &[0xC3], strs);
        let img = Image::from_bytes(PathBuf::from("a.dll"), first).unwrap();
        let rows = img.import_rows();
        let ll = rows.iter().find(|r| r.entry.label() == "LoadLibraryExW").unwrap().iat_rva;
        let gpa = rows.iter().find(|r| r.entry.label() == "GetProcAddress").unwrap().iat_rva;
        let text = code(&rv, ll, gpa);
        let (bytes, rv2) = testpe::build_full(&[], imports, &[], &text, strs);
        assert_eq!(rv, rv2);
        let img = Image::from_bytes(PathBuf::from("a.dll"), bytes).unwrap();
        assert_eq!(img.image_base, IMAGE_BASE);
        let lits = literal_strings(&img);
        assert_eq!(lits.get(&rv["AudioHALWrapper.dll"]).unwrap(), &("AudioHALWrapper.dll".to_string(), true));
        let sites = analyze(&img);
        assert_eq!(sites.len(), 1);
        let s = &sites[0];
        assert_eq!(s.dll, "AudioHALWrapper.dll");
        assert!(s.wide);
        assert_eq!(s.loader.as_deref(), Some("LoadLibraryExW"));
        assert!(s.null_checked);
        assert!(s.failure_branch_rva.is_some());
        assert_eq!(s.procs.len(), 2);
        assert_eq!(s.procs[0].name.as_deref(), Some("DecodePacket"));
        assert!(s.procs[0].null_checked);
        assert_eq!(s.procs[1].name.as_deref(), Some("EncodeFrame"));
        assert!(!s.procs[1].null_checked);
    }
}
