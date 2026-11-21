use crate::crt::{_wcsicmp, _wcsnicmp, swprintf_s, wcschr, wprintf};
use crate::stl::{RawVec, WString};
use crate::w;
use core::ffi::c_void;

pub type ArgHandler = unsafe extern "C" fn(this: *mut ParseArgs, index: u32, context: *mut c_void, result: *mut i32) -> i32;

#[repr(C)]
pub struct ArgDefinition {
    pub id: u32,
    pub name: WString,
    pub min_match: u32,
    pub handler: Option<ArgHandler>,
    pub kind: u32,
    pub alias: WString,
    pub short_help: WString,
    pub detailed_help: WString,
    pub sub_options: [u32; 100],
}

#[repr(C)]
pub struct ArgEntry {
    pub def: ArgDefinition,
    pub count: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct ArgSymbol {
    pub id: u32,
    pub arg: WString,
    pub value: WString,
    pub error: u8,
}

#[repr(C)]
pub struct ParseArgs {
    pub entries: RawVec,
    pub symbols: RawVec,
    pub tool_name: WString,
    pub description: WString,
    pub build_info: WString,
    pub version: [u16; 24],
    pub duplicate_id: u8,
    pub id_out_of_range: u8,
    pub no_definitions: u8,
    pub help_requested: u8,
}

pub const ENTRY: usize = core::mem::size_of::<ArgEntry>();
pub const SYMBOL: usize = core::mem::size_of::<ArgSymbol>();
const ENTRY_MAX: usize = isize::MAX as usize / ENTRY;
const SYMBOL_MAX: usize = isize::MAX as usize / SYMBOL;
pub const KIND_COMMAND: u32 = 1;
pub const KIND_TOPIC: u32 = 3;

const _: () = assert!(core::mem::size_of::<ArgDefinition>() == 0x230);
const _: () = assert!(ENTRY == 0x238);
const _: () = assert!(SYMBOL == 0x50);
const _: () = assert!(core::mem::offset_of!(ParseArgs, version) == 0x90);
const _: () = assert!(core::mem::offset_of!(ParseArgs, duplicate_id) == 0xc0);
const _: () = assert!(core::mem::offset_of!(ArgDefinition, handler) == 0x30);
const _: () = assert!(core::mem::offset_of!(ArgDefinition, kind) == 0x38);
const _: () = assert!(core::mem::offset_of!(ArgDefinition, sub_options) == 0xa0);
const _: () = assert!(core::mem::offset_of!(ArgSymbol, error) == 0x48);

unsafe fn entry_init(p: *mut u8) {
    core::ptr::write_bytes(p, 0, ENTRY);
    let e = p as *mut ArgEntry;
    WString::init(&mut (*e).def.name);
    WString::init(&mut (*e).def.alias);
    WString::init(&mut (*e).def.short_help);
    WString::init(&mut (*e).def.detailed_help);
}

unsafe fn entry_drop(p: *mut u8) {
    let e = p as *mut ArgEntry;
    WString::tidy(&mut (*e).def.detailed_help);
    WString::tidy(&mut (*e).def.short_help);
    WString::tidy(&mut (*e).def.alias);
    WString::tidy(&mut (*e).def.name);
}

unsafe fn symbol_init(p: *mut u8) {
    core::ptr::write_bytes(p, 0, SYMBOL);
    let s = p as *mut ArgSymbol;
    WString::init(&mut (*s).arg);
    WString::init(&mut (*s).value);
}

unsafe fn symbol_drop(p: *mut u8) {
    let s = p as *mut ArgSymbol;
    WString::tidy(&mut (*s).value);
    WString::tidy(&mut (*s).arg);
}

pub unsafe fn entry(this: *const ParseArgs, i: usize) -> *mut ArgEntry {
    RawVec::at(&(*this).entries, ENTRY, i) as *mut ArgEntry
}

pub unsafe fn symbol(this: *const ParseArgs, i: usize) -> *mut ArgSymbol {
    RawVec::at(&(*this).symbols, SYMBOL, i) as *mut ArgSymbol
}

pub unsafe fn entry_count(this: *const ParseArgs) -> usize {
    RawVec::len(&(*this).entries, ENTRY)
}

pub unsafe fn symbol_count(this: *const ParseArgs) -> usize {
    RawVec::len(&(*this).symbols, SYMBOL)
}

unsafe fn module_version(out: &mut [u16; 4]) -> bool {
    let mut module: *mut c_void = core::ptr::null_mut();
    if GetModuleHandleExW(6, module_version as *const u16, &mut module) == 0 {
        return false;
    }
    let res = FindResourceW(module, 1 as *const u16, 16 as *const u16);
    if res.is_null() {
        return false;
    }
    let size = SizeofResource(module, res) as usize;
    let data = LoadResource(module, res);
    if data.is_null() {
        return false;
    }
    let base = LockResource(data) as *const u8;
    if base.is_null() || size < 0x5c {
        return false;
    }
    let blob = core::slice::from_raw_parts(base, size);
    let word = |o: usize| u16::from_le_bytes([blob[o], blob[o + 1]]);
    if (word(0) as usize) > size || word(2) != 0x34 || !key_is(blob, 6, "VS_VERSION_INFO") {
        return false;
    }
    let fixed = 0x28;
    out[0] = word(fixed + 0xa);
    out[1] = word(fixed + 0x8);
    out[2] = word(fixed + 0xe);
    out[3] = word(fixed + 0xc);
    let end = word(0) as usize;
    let Some(sfi) = find_child(blob, fixed + 0x34, end, "StringFileInfo") else {
        return false;
    };
    let sfi_end = sfi + word(sfi) as usize;
    let Some(table) = first_child(blob, sfi, sfi_end) else {
        return false;
    };
    let table_end = (table + word(table) as usize).min(sfi_end);
    let Some(first) = first_child(blob, table, table_end) else {
        return false;
    };
    find_child(blob, first, table_end, "FileVersion").is_some()
}

fn align4(v: usize) -> usize {
    (v + 3) & !3
}

fn key_is(blob: &[u8], at: usize, key: &str) -> bool {
    let mut o = at;
    for c in key.encode_utf16().chain(Some(0)) {
        if o + 2 > blob.len() || u16::from_le_bytes([blob[o], blob[o + 1]]) != c {
            return false;
        }
        o += 2;
    }
    true
}

fn key_end(blob: &[u8], at: usize) -> usize {
    let mut o = at;
    while o + 2 <= blob.len() && (blob[o] != 0 || blob[o + 1] != 0) {
        o += 2;
    }
    o + 2
}

fn first_child(blob: &[u8], block: usize, end: usize) -> Option<usize> {
    let value_len = u16::from_le_bytes([blob[block + 2], blob[block + 3]]) as usize;
    let text = u16::from_le_bytes([blob[block + 4], blob[block + 5]]) == 1;
    let value = align4(key_end(blob, block + 6));
    let child = align4(value + if text { value_len * 2 } else { value_len });
    (child + 6 <= end).then_some(child)
}

fn find_child(blob: &[u8], mut child: usize, end: usize, key: &str) -> Option<usize> {
    while child + 6 <= end.min(blob.len()) {
        let len = u16::from_le_bytes([blob[child], blob[child + 1]]) as usize;
        if len == 0 {
            return None;
        }
        if key_is(blob, child + 6, key) {
            return Some(child);
        }
        child = align4(child + len);
    }
    None
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut *mut c_void) -> i32;
    fn FindResourceW(module: *mut c_void, name: *const u16, kind: *const u16) -> *mut c_void;
    fn SizeofResource(module: *mut c_void, res: *mut c_void) -> u32;
    fn LoadResource(module: *mut c_void, res: *mut c_void) -> *mut c_void;
    fn LockResource(data: *mut c_void) -> *mut c_void;
}

#[export_name = "??0ParseArgs@@QEAA@V?$basic_string@GU?$char_traits@G@std@@V?$allocator@G@2@@std@@00PEAUArgDefinition@0@I@Z"]
pub unsafe extern "C" fn parse_args_ctor(this: *mut ParseArgs, tool_name: *mut WString, description: *mut WString, build_info: *mut WString, defs: *const ArgDefinition, count: u32) -> *mut ParseArgs {
    core::ptr::write_bytes(&mut (*this).entries, 0, 1);
    core::ptr::write_bytes(&mut (*this).symbols, 0, 1);
    WString::init(&mut (*this).tool_name);
    WString::init(&mut (*this).description);
    WString::init(&mut (*this).build_info);
    if count < 1 || defs.is_null() {
        (*this).no_definitions = 1;
    } else {
        WString::assign_str(&mut (*this).tool_name, tool_name);
        WString::assign_str(&mut (*this).description, description);
        WString::assign_str(&mut (*this).build_info, build_info);
        (*this).duplicate_id = 0;
        (*this).id_out_of_range = 0;
        (*this).no_definitions = 0;
        (*this).help_requested = 0;
        let mut v = [0u16; 4];
        if module_version(&mut v) {
            let args = [v[0] as usize, v[1] as usize, v[2] as usize, v[3] as usize];
            swprintf_s((*this).version.as_mut_ptr(), 24, w!("%u.%u.%u.%u"), &args);
        } else {
            (*this).version[0] = 0;
        }
        RawVec::resize(&mut (*this).entries, ENTRY, ENTRY_MAX, count as usize, entry_init, entry_drop);
        for i in 0..count as usize {
            let d = defs.add(i);
            let id = (*d).id;
            if id >= count {
                (*this).id_out_of_range = 1;
                break;
            }
            let e = entry(this, id as usize);
            if (*e).def.id != 0 {
                (*this).duplicate_id = 1;
                break;
            }
            (*e).def.id = id;
            WString::assign_str(&mut (*e).def.name, &(*d).name);
            (*e).def.min_match = (*d).min_match;
            (*e).def.handler = (*d).handler;
            (*e).def.kind = (*d).kind;
            WString::assign_str(&mut (*e).def.alias, &(*d).alias);
            WString::assign_str(&mut (*e).def.short_help, &(*d).short_help);
            WString::assign_str(&mut (*e).def.detailed_help, &(*d).detailed_help);
            (*e).def.sub_options = (*d).sub_options;
        }
    }
    WString::tidy(build_info);
    WString::tidy(description);
    WString::tidy(tool_name);
    this
}

#[export_name = "??AParseArgs@@QEAAAEAUArgEntry@0@I@Z"]
pub unsafe extern "C" fn parse_args_index(this: *mut ParseArgs, index: u32) -> *mut ArgEntry {
    entry(this, index as usize)
}

#[export_name = "?GetDictionarySize@ParseArgs@@QEAAIXZ"]
pub unsafe extern "C" fn get_dictionary_size(this: *mut ParseArgs) -> u32 {
    entry_count(this) as u32
}

#[export_name = "?IsHelp@ParseArgs@@QEAA_NXZ"]
pub unsafe extern "C" fn is_help(this: *mut ParseArgs) -> bool {
    (*this).help_requested != 0
}

#[export_name = "?GetSymbols@ParseArgs@@QEAA?AV?$vector@UArgSymbol@ParseArgs@@V?$allocator@UArgSymbol@ParseArgs@@@std@@@std@@XZ"]
pub unsafe extern "C" fn get_symbols(this: *mut ParseArgs, out: *mut RawVec) -> *mut RawVec {
    let n = symbol_count(this);
    (*out).first = core::ptr::null_mut();
    (*out).last = core::ptr::null_mut();
    (*out).end = core::ptr::null_mut();
    if n != 0 {
        if n > SYMBOL_MAX {
            std::process::abort();
        }
        let p = crate::stl::allocate(n * SYMBOL);
        (*out).first = p;
        (*out).last = p;
        (*out).end = p.add(n * SYMBOL);
        for i in 0..n {
            let src = symbol(this, i);
            let dst = p.add(i * SYMBOL) as *mut ArgSymbol;
            core::ptr::write_bytes(dst as *mut u8, 0, SYMBOL);
            (*dst).id = (*src).id;
            WString::copy_construct(&mut (*dst).arg, &(*src).arg);
            WString::copy_construct(&mut (*dst).value, &(*src).value);
            (*dst).error = (*src).error;
            (*out).last = p.add((i + 1) * SYMBOL);
        }
    }
    out
}

#[export_name = "?FindSymbol@ParseArgs@@QEAAHW4SymbolSearch@1@II@Z"]
pub unsafe extern "C" fn find_symbol(this: *mut ParseArgs, mode: u32, id: u32, start: u32) -> i32 {
    let mut i = 0u32;
    if mode != 0 {
        if start as usize >= symbol_count(this) {
            return -1;
        }
        i = start.wrapping_add(1);
    }
    let n = symbol_count(this);
    while (i as usize) < n {
        if (*symbol(this, i as usize)).id == id {
            return i as i32;
        }
        if mode == 2 {
            return -1;
        }
        i = i.wrapping_add(1);
    }
    -1
}

#[export_name = "?HandleArgument@ParseArgs@@QEAA?AW4HandlerResult@1@IPEAXPEAH@Z"]
pub unsafe extern "C" fn handle_argument(this: *mut ParseArgs, index: u32, context: *mut c_void, result: *mut i32) -> i32 {
    if index as usize >= symbol_count(this) {
        return 1;
    }
    let id = (*symbol(this, index as usize)).id;
    match (*entry(this, id as usize)).def.handler {
        None => 2,
        Some(h) => h(this, index, context, result),
    }
}

unsafe fn find_separator(arg: *const u16) -> *const u16 {
    if arg.is_null() {
        return core::ptr::null();
    }
    let colon = wcschr(arg, b':' as u16);
    let equal = wcschr(arg, b'=' as u16);
    let mut sep = equal;
    if !colon.is_null() {
        sep = colon;
        if !equal.is_null() && equal < colon {
            sep = equal;
        }
    }
    if sep.is_null() {
        sep
    } else {
        sep.add(1)
    }
}

unsafe fn find_separator_v1(arg: *const u16) -> *const u16 {
    let mut sep = wcschr(arg, b':' as u16);
    if sep.is_null() {
        sep = wcschr(arg, b'=' as u16);
    }
    if sep.is_null() {
        sep
    } else {
        sep.add(1)
    }
}

unsafe fn match_argument(this: *mut ParseArgs, arg: *const u16, value: *mut WString, v1: bool) -> i32 {
    if !value.is_null() {
        WString::clear(value);
    }
    let n = entry_count(this);
    let mut i = 0u32;
    while (i as usize) < n {
        let e = entry(this, i as usize);
        if (*e).def.name.size == 0 {
            if arg.is_null() || *arg == 0 {
                return i as i32;
            }
        } else {
            let mut len = 260usize;
            let sep = if v1 {
                if arg.is_null() { core::ptr::null() } else { find_separator_v1(arg) }
            } else {
                find_separator(arg)
            };
            if !sep.is_null() {
                len = ((sep as usize - arg as usize) >> 1).wrapping_sub(1);
            }
            let min = (*e).def.min_match as usize;
            if min != 0 && min < len {
                len = min;
            }
            if _wcsnicmp(arg, WString::data(&(*e).def.name), len) == 0 {
                if !value.is_null() && !sep.is_null() {
                    WString::assign_cstr(value, sep);
                }
                return i as i32;
            }
        }
        i += 1;
    }
    -1
}

unsafe fn parse_prologue(this: *mut ParseArgs, argc: u32, argv: *const *const u16) -> Option<u32> {
    if argv.is_null() {
        return Some(3);
    }
    if argc < 2 {
        return Some(2);
    }
    if (*this).no_definitions != 0 {
        return Some(6);
    }
    if (*this).duplicate_id != 0 {
        return Some(4);
    }
    if (*this).id_out_of_range != 0 {
        return Some(5);
    }
    for i in 0..entry_count(this) {
        let e = entry(this, i);
        (*e).count = 0;
        (*e).reserved = 0;
    }
    RawVec::resize(&mut (*this).symbols, SYMBOL, SYMBOL_MAX, 0, symbol_init, symbol_drop);
    RawVec::resize(&mut (*this).symbols, SYMBOL, SYMBOL_MAX, argc as usize, symbol_init, symbol_drop);
    WString::assign_cstr(&mut (*symbol(this, 0)).arg, *argv);
    None
}

#[export_name = "?Parse@ParseArgs@@QEAA?AW4ParseResult@1@IPEAPEBG_N@Z"]
pub unsafe extern "C" fn parse(this: *mut ParseArgs, argc: u32, argv: *const *const u16, allow_help: bool) -> u32 {
    if let Some(r) = parse_prologue(this, argc, argv) {
        return r;
    }
    let command = match_argument(this, core::ptr::null(), core::ptr::null_mut(), false);
    let mut out = 1usize;
    for i in 1..argc as usize {
        let arg = *argv.add(i);
        if allow_help && (_wcsicmp(arg, w!("/?").as_ptr()) == 0 || _wcsicmp(arg, w!("help").as_ptr()) == 0) {
            (*this).help_requested = 1;
            continue;
        }
        let idx = match_argument(this, arg, &mut (*symbol(this, i)).value, false);
        let s = symbol(this, out);
        if idx >= 0 {
            (*entry(this, idx as usize)).count += 1;
            (*s).id = idx as u32;
            WString::assign_cstr(&mut (*s).arg, arg);
        } else if command < 0 {
            (*s).error = 1;
            WString::assign_cstr(&mut (*s).arg, arg);
            return 1;
        } else {
            (*entry(this, command as usize)).count += 1;
            (*s).id = command as u32;
            WString::assign_cstr(&mut (*s).arg, arg);
            let sep = find_separator(arg);
            if !sep.is_null() {
                WString::assign_cstr(&mut (*s).value, sep);
            }
        }
        out += 1;
    }
    RawVec::resize(&mut (*this).symbols, SYMBOL, SYMBOL_MAX, out, symbol_init, symbol_drop);
    0
}

#[export_name = "?Parse@ParseArgs@@QEAA?AW4ParseResult@1@IPEAPEBG@Z"]
pub unsafe extern "C" fn parse_v1(this: *mut ParseArgs, argc: u32, argv: *const *const u16) -> u32 {
    if let Some(r) = parse_prologue(this, argc, argv) {
        return r;
    }
    let command = match_argument(this, core::ptr::null(), core::ptr::null_mut(), true);
    for i in 1..argc as usize {
        let arg = *argv.add(i);
        let s = symbol(this, i);
        let idx = match_argument(this, arg, &mut (*s).value, true);
        if idx >= 0 {
            (*entry(this, idx as usize)).count += 1;
            (*s).id = idx as u32;
            WString::assign_cstr(&mut (*s).arg, arg);
        } else if command < 0 {
            (*s).error = 1;
            WString::assign_cstr(&mut (*s).arg, arg);
            return 1;
        } else {
            (*entry(this, command as usize)).count += 1;
            (*s).id = command as u32;
            WString::assign_cstr(&mut (*s).arg, arg);
            if !arg.is_null() {
                let sep = find_separator_v1(arg);
                if !sep.is_null() {
                    WString::assign_cstr(&mut (*s).value, sep);
                }
            }
        }
    }
    0
}

unsafe fn label(e: *const ArgEntry) -> *const u16 {
    if (*e).def.name.size == 0 {
        w!("<any>").as_ptr()
    } else {
        WString::data(&(*e).def.name)
    }
}

unsafe fn alias_or_name(e: *const ArgEntry) -> *const WString {
    if (*e).def.alias.size != 0 {
        &(*e).def.alias
    } else {
        &(*e).def.name
    }
}

#[export_name = "?PrintHelpSummary@ParseArgs@@QEAAXXZ"]
pub unsafe extern "C" fn print_help_summary(this: *mut ParseArgs) {
    wprintf(w!("\r\n%s - "), &[WString::data(&(*this).tool_name) as usize]);
    print_with_breaks(((*this).tool_name.size as u32).wrapping_add(3), true, WString::data(&(*this).description));
    wprintf(w!("\r\nBuild %s.\r\n\r\n"), &[(*this).version.as_ptr() as usize]);
    print_with_breaks(0, false, WString::data(&(*this).build_info));
    wprintf(w!("\r\nThe following topics are available:\r\n"), &[]);
    wprintf(w!("  commands /?    for a list of commands this tool provides\r\n"), &[]);
    wprintf(w!("  <command> /?   for detailed help on a specific command\r\n"), &[]);
    let mut width = 9u32;
    for i in 0..entry_count(this) {
        let e = entry(this, i);
        if (*e).def.kind == KIND_TOPIC && (*e).def.name.size > width as usize {
            width = (*e).def.name.size as u32;
        }
    }
    let field = width.wrapping_add(3);
    let mut i = 0usize;
    while i < entry_count(this) {
        let e = entry(this, i);
        if (*e).def.kind == KIND_TOPIC {
            wprintf(w!("  %-*s /?"), &[field as usize, label(e) as usize]);
            print_with_breaks(field.wrapping_add(7), true, WString::data(&(*entry(this, i)).def.short_help));
        }
        i += 1;
    }
    wprintf(w!("\r\n"), &[]);
}

#[export_name = "?PrintHelpMenu@ParseArgs@@QEAAXXZ"]
pub unsafe extern "C" fn print_help_menu(this: *mut ParseArgs) {
    let mut width = 0u32;
    for i in 0..entry_count(this) {
        let e = entry(this, i);
        if (*e).def.kind == KIND_COMMAND && (*e).def.name.size > width as usize {
            width = (*e).def.name.size as u32;
        }
    }
    wprintf(w!("\r\n"), &[]);
    wprintf(w!("Available %s commands:\r\n"), &[WString::data(&(*this).tool_name) as usize]);
    wprintf(w!("\r\n"), &[]);
    let mut i = 0usize;
    while i < entry_count(this) {
        let e = entry(this, i);
        if (*e).def.kind == KIND_COMMAND {
            wprintf(w!("   %-*s"), &[width.wrapping_add(3) as usize, label(e) as usize]);
            print_with_breaks(width.wrapping_add(6), true, WString::data(&(*entry(this, i)).def.short_help));
        }
        i += 1;
    }
    wprintf(w!("\r\n"), &[]);
}

#[export_name = "?PrintHelpDetailed@ParseArgs@@QEAA_NI@Z"]
pub unsafe extern "C" fn print_help_detailed(this: *mut ParseArgs, index: u32) -> bool {
    if index as usize >= entry_count(this) {
        return false;
    }
    wprintf(w!("\r\n"), &[]);
    let e = entry(this, index as usize);
    if (*e).def.detailed_help.size == 0 {
        wprintf(w!("Help not available for command '%s'.\r\n"), &[WString::data(&(*e).def.name) as usize]);
    } else {
        wprintf(w!("%s %s\r\n\r\n"), &[WString::data(&(*this).tool_name) as usize, WString::data(&(*e).def.name) as usize]);
        print_with_breaks(0, false, WString::data(&(*entry(this, index as usize)).def.detailed_help));
        let e = entry(this, index as usize);
        if (*e).def.kind != KIND_TOPIC {
            wprintf(w!("\r\nUsage:\r\n\r\n   %s %s\r\n"), &[WString::data(&(*this).tool_name) as usize, WString::data(alias_or_name(e)) as usize]);
            let mut width = 0u32;
            for k in 0..100 {
                let sub = (*entry(this, index as usize)).def.sub_options[k];
                if sub == 0 {
                    break;
                }
                let s = entry(this, sub as usize);
                let len = if (*s).def.alias.size != 0 { (*s).def.alias.size as u32 } else { (*s).def.name.size as u32 };
                if len > width {
                    width = len;
                }
            }
            for k in 0..100 {
                let sub = (*entry(this, index as usize)).def.sub_options[k];
                if sub == 0 {
                    break;
                }
                let s = entry(this, sub as usize);
                wprintf(w!("\r\n   %-*s"), &[width.wrapping_add(3) as usize, WString::data(alias_or_name(s)) as usize]);
                print_with_breaks(width.wrapping_add(6), true, WString::data(&(*entry(this, sub as usize)).def.short_help));
            }
        }
    }
    wprintf(w!("\r\n"), &[]);
    true
}

fn is_blank(c: u16) -> bool {
    c == 9 || c == 32
}

#[export_name = "?PrintWithBreaks@ParseArgs@@SAXI_NPEBG@Z"]
pub unsafe extern "C" fn print_with_breaks(indent: u32, first_line_continues: bool, text: *const u16) {
    if text.is_null() {
        return;
    }
    let mut skip_indent = first_line_continues;
    let indent = indent.min(75);
    let mut line = text;
    let mut brk = text;
    let mut p = text;
    let mut col = indent;
    let empty = w!("").as_ptr() as usize;
    while *p != 0 {
        let c = *p;
        if is_blank(c) {
            brk = p;
        }
        if c == 10 {
            p = p.add(1);
            if skip_indent {
                skip_indent = false;
            } else {
                wprintf(w!("%*s"), &[indent as usize, empty]);
            }
            wprintf(w!("%.*s"), &[(p as usize - line as usize) >> 1, line as usize]);
            while *p != 0 && is_blank(*p) {
                p = p.add(1);
            }
            col = indent;
            line = p;
            brk = p;
        } else if col >= 78 {
            if skip_indent {
                skip_indent = false;
            } else {
                wprintf(w!("%*s"), &[indent as usize, empty]);
            }
            wprintf(w!("%.*s\r\n"), &[(brk as usize - line as usize) >> 1, line as usize]);
            while *brk != 0 && is_blank(*brk) {
                brk = brk.add(1);
            }
            let q = if p >= brk { p } else { brk };
            line = brk;
            col = (((q as usize - brk as usize) >> 1) as u32).wrapping_add(indent);
            p = q.add(1);
        } else {
            p = p.add(1);
            col = col.wrapping_add(1);
        }
    }
    if col != 0 {
        if !skip_indent {
            wprintf(w!("%*s"), &[indent as usize, empty]);
        }
        wprintf(w!("%s\r\n"), &[line as usize]);
    }
}
