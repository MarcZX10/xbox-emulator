//! Named PE/COFF constants and small decode helpers. Values come from the
//! Microsoft PE/COFF specification. Unknown values are reported numerically,
//! never silently dropped.

// --- Machine types (IMAGE_FILE_MACHINE_*) ---
pub const IMAGE_FILE_MACHINE_UNKNOWN: u16 = 0x0000;
pub const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;
pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
pub const IMAGE_FILE_MACHINE_ARM64: u16 = 0xAA64;
pub const IMAGE_FILE_MACHINE_ARMNT: u16 = 0x01C4;

pub fn machine_name(m: u16) -> &'static str {
    match m {
        IMAGE_FILE_MACHINE_UNKNOWN => "UNKNOWN",
        IMAGE_FILE_MACHINE_I386 => "I386 (x86)",
        IMAGE_FILE_MACHINE_AMD64 => "AMD64 (x86-64)",
        IMAGE_FILE_MACHINE_ARM64 => "ARM64",
        IMAGE_FILE_MACHINE_ARMNT => "ARM Thumb-2",
        _ => "other",
    }
}

// --- COFF Characteristics ---
pub const IMAGE_FILE_RELOCS_STRIPPED: u16 = 0x0001;
pub const IMAGE_FILE_EXECUTABLE_IMAGE: u16 = 0x0002;
pub const IMAGE_FILE_LARGE_ADDRESS_AWARE: u16 = 0x0020;
pub const IMAGE_FILE_DLL: u16 = 0x2000;

pub fn characteristics_flags(c: u16) -> Vec<&'static str> {
    let mut v = Vec::new();
    let bits: &[(u16, &str)] = &[
        (0x0001, "RELOCS_STRIPPED"),
        (0x0002, "EXECUTABLE_IMAGE"),
        (0x0004, "LINE_NUMS_STRIPPED"),
        (0x0008, "LOCAL_SYMS_STRIPPED"),
        (0x0010, "AGGRESSIVE_WS_TRIM"),
        (0x0020, "LARGE_ADDRESS_AWARE"),
        (0x0080, "BYTES_REVERSED_LO"),
        (0x0100, "32BIT_MACHINE"),
        (0x0200, "DEBUG_STRIPPED"),
        (0x0400, "REMOVABLE_RUN_FROM_SWAP"),
        (0x0800, "NET_RUN_FROM_SWAP"),
        (0x1000, "SYSTEM"),
        (0x2000, "DLL"),
        (0x4000, "UP_SYSTEM_ONLY"),
        (0x8000, "BYTES_REVERSED_HI"),
    ];
    for (bit, name) in bits {
        if c & bit != 0 {
            v.push(*name);
        }
    }
    v
}

// --- Subsystems ---
pub fn subsystem_name(s: u16) -> &'static str {
    match s {
        0 => "UNKNOWN",
        1 => "NATIVE",
        2 => "WINDOWS_GUI",
        3 => "WINDOWS_CUI (console)",
        5 => "OS2_CUI",
        7 => "POSIX_CUI",
        9 => "WINDOWS_CE_GUI",
        10 => "EFI_APPLICATION",
        11 => "EFI_BOOT_SERVICE_DRIVER",
        12 => "EFI_RUNTIME_DRIVER",
        13 => "EFI_ROM",
        14 => "XBOX",
        16 => "WINDOWS_BOOT_APPLICATION",
        _ => "other",
    }
}

// --- DLL characteristics ---
pub fn dll_characteristics_flags(c: u16) -> Vec<&'static str> {
    let mut v = Vec::new();
    let bits: &[(u16, &str)] = &[
        (0x0020, "HIGH_ENTROPY_VA"),
        (0x0040, "DYNAMIC_BASE (ASLR)"),
        (0x0080, "FORCE_INTEGRITY"),
        (0x0100, "NX_COMPAT (DEP)"),
        (0x0200, "NO_ISOLATION"),
        (0x0400, "NO_SEH"),
        (0x0800, "NO_BIND"),
        (0x1000, "APPCONTAINER"),
        (0x2000, "WDM_DRIVER"),
        (0x4000, "GUARD_CF (Control Flow Guard)"),
        (0x8000, "TERMINAL_SERVER_AWARE"),
    ];
    for (bit, name) in bits {
        if c & bit != 0 {
            v.push(*name);
        }
    }
    v
}

// --- Section characteristics ---
pub fn section_flags(c: u32) -> Vec<&'static str> {
    let mut v = Vec::new();
    let bits: &[(u32, &str)] = &[
        (0x0000_0020, "CNT_CODE"),
        (0x0000_0040, "CNT_INITIALIZED_DATA"),
        (0x0000_0080, "CNT_UNINITIALIZED_DATA"),
        (0x0200_0000, "MEM_DISCARDABLE"),
        (0x0400_0000, "MEM_NOT_CACHED"),
        (0x0800_0000, "MEM_NOT_PAGED"),
        (0x1000_0000, "MEM_SHARED"),
        (0x2000_0000, "MEM_EXECUTE"),
        (0x4000_0000, "MEM_READ"),
        (0x8000_0000, "MEM_WRITE"),
    ];
    for (bit, name) in bits {
        if c & bit != 0 {
            v.push(*name);
        }
    }
    v
}

// --- Data directory indices ---
pub const DIR_EXPORT: usize = 0;
pub const DIR_IMPORT: usize = 1;
pub const DIR_RESOURCE: usize = 2;
pub const DIR_EXCEPTION: usize = 3;
pub const DIR_SECURITY: usize = 4;
pub const DIR_BASERELOC: usize = 5;
pub const DIR_DEBUG: usize = 6;
pub const DIR_ARCHITECTURE: usize = 7;
pub const DIR_GLOBALPTR: usize = 8;
pub const DIR_TLS: usize = 9;
pub const DIR_LOAD_CONFIG: usize = 10;
pub const DIR_BOUND_IMPORT: usize = 11;
pub const DIR_IAT: usize = 12;
pub const DIR_DELAY_IMPORT: usize = 13;
pub const DIR_CLR: usize = 14;

pub fn directory_name(i: usize) -> &'static str {
    match i {
        0 => "Export",
        1 => "Import",
        2 => "Resource",
        3 => "Exception (.pdata)",
        4 => "Security (certificates)",
        5 => "BaseRelocation",
        6 => "Debug",
        7 => "Architecture",
        8 => "GlobalPtr",
        9 => "TLS",
        10 => "LoadConfig",
        11 => "BoundImport",
        12 => "IAT",
        13 => "DelayImport",
        14 => "CLR/.NET",
        15 => "Reserved",
        _ => "?",
    }
}

// --- Base relocation types ---
pub fn reloc_type_name(t: u16) -> &'static str {
    match t {
        0 => "ABSOLUTE",
        1 => "HIGH",
        2 => "LOW",
        3 => "HIGHLOW",
        4 => "HIGHADJ",
        5 => "MIPS_JMPADDR / ARM_MOV32 / RISCV_HIGH20",
        7 => "ARM_MOV32 / RISCV_LOW12I",
        8 => "RISCV_LOW12S / LOONGARCH",
        9 => "MIPS_JMPADDR16",
        10 => "DIR64",
        _ => "other",
    }
}
