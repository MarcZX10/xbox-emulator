use crate::forward::*;
use core::ffi::c_void;

pub const SC_OK: u32 = 0;
pub const SC_FAILED_TO_INITIATE: u32 = 3;
pub const SC_INCOMPATIBLE: u32 = 4;
pub const SC_INTERFACE_VERSION: u32 = 0x188;
pub const SC_MAX_MAJOR: u32 = 0x10000;
pub const EXPORT_TABLE_BYTES: usize = 0x188;

#[no_mangle]
pub extern "system" fn SCCheckCompatibility(version: u32, interface: u32) -> u32 {
    if (version & 0xffff_0000) <= SC_MAX_MAJOR && interface == SC_INTERFACE_VERSION { SC_OK } else { SC_INCOMPATIBLE }
}

#[no_mangle]
pub extern "system" fn SCGetInstructionSet(chip: i32) -> u32 {
    match chip {
        110 => 11,
        120 | 125 => 12,
        130 => 13,
        _ => 0,
    }
}

fn bit(index: i32) -> (usize, u32) {
    let i = index as i64 as u64;
    ((i >> 5) as usize, 1u32 << (index as u32 & 31))
}

#[no_mangle]
pub unsafe extern "system" fn SCGetSCOption(options: *const u32, index: i32) -> u8 {
    let (w, m) = bit(index);
    (*options.wrapping_add(w) & m != 0) as u8
}

#[no_mangle]
pub unsafe extern "system" fn SCSetSCOption(options: *mut u32, index: i32, value: i32) {
    let (w, m) = bit(index);
    let p = options.wrapping_add(w);
    if value != 0 {
        *p |= m;
    } else {
        *p &= !m;
    }
}

#[no_mangle]
pub extern "system" fn SCMergeShaderPair(compiler: *mut c_void) -> u32 {
    if compiler.is_null() { SC_FAILED_TO_INITIATE } else { SC_OK }
}

#[no_mangle]
pub extern "system" fn SCCompileShaderPair(compiler: *mut c_void) -> u32 {
    if compiler.is_null() { SC_FAILED_TO_INITIATE } else { SC_OK }
}

macro_rules! retail_empty {
    ($($name:ident),*) => {$(
        #[no_mangle]
        pub extern "system" fn $name() {}
    )*};
}

retail_empty!(SCDumpILShader, SCDumpILTranslation, SCDumpSrcShaderData, SCDumpHwShader, SCDumpHwShaderData, SCDumpRawShader, SCDumpHwShaderRaw, SC_AssembleHandle_Destroy, SC_MetadataHandle_Destroy);

pub extern "system" fn sc_table_reserved() {}

#[no_mangle]
pub extern "system" fn SC_Assemble() -> usize {
    0
}

#[no_mangle]
pub extern "system" fn SC_AssembleHandle_GetShader() -> u8 {
    0
}

#[no_mangle]
pub extern "system" fn SC_CompileShaderFromMetadata() -> u32 {
    SC_INCOMPATIBLE
}

#[no_mangle]
pub unsafe extern "system" fn OG_GetCompactOptionSet(generator: *mut c_void, set: *const u8, out: *mut u8) -> u32 {
    if generator.is_null() || set.is_null() {
        return SC_FAILED_TO_INITIATE;
    }
    core::ptr::copy(set, out, 32);
    SC_OK
}

unsafe fn tagged(handle: *mut c_void, tag: u32) -> *mut c_void {
    if !handle.is_null() && *(handle as *const u32) & 0x3ff == tag { handle } else { core::ptr::null_mut() }
}

#[no_mangle]
pub unsafe extern "system" fn SC_DebugMapHandle_GetMap(handle: *mut c_void) -> *mut c_void {
    tagged(handle, 1)
}

#[no_mangle]
pub unsafe extern "system" fn SC_RegisterLifetimeHandle_GetMap(handle: *mut c_void) -> *mut c_void {
    tagged(handle, 2)
}

pub fn export_table() -> Vec<(usize, &'static str, usize)> {
    vec![
        (0x00, "SCCreate", SCCreate as *const () as usize),
        (0x08, "SCDestroy", SCDestroy as *const () as usize),
        (0x10, "SCCompileShader", SCCompileShader as *const () as usize),
        (0x18, "SCSimpleCompileShader", SCSimpleCompileShader as *const () as usize),
        (0x30, "SCDumpILShader", SCDumpILShader as *const () as usize),
        (0x38, "SCDumpILTranslation", SCDumpILTranslation as *const () as usize),
        (0x40, "SCCompileShaderPair", SCCompileShaderPair as *const () as usize),
        (0x48, "SCGetInstructionSet", SCGetInstructionSet as *const () as usize),
        (0x60, "SCGetSCOption", SCGetSCOption as *const () as usize),
        (0x68, "SCSetSCOption", SCSetSCOption as *const () as usize),
        (0x70, "SCDumpSrcShaderData", SCDumpSrcShaderData as *const () as usize),
        (0x78, "SCDumpHwShader", SCDumpHwShader as *const () as usize),
        (0x80, "SCDumpHwShaderData", SCDumpHwShaderData as *const () as usize),
        (0x88, "SCDumpRawShader", SCDumpRawShader as *const () as usize),
        (0x90, "SCDumpHwShaderRaw", SCDumpHwShaderRaw as *const () as usize),
        (0xa0, "SC_InstantiateVIHFromDXBC", SC_InstantiateVIHFromDXBC as *const () as usize),
        (0xa8, "SC_VIH_GetPDBName", SC_VIH_GetPDBName as *const () as usize),
        (0xb0, "SC_VIH_GetPDBNameLengthInBytes", SC_VIH_GetPDBNameLengthInBytes as *const () as usize),
        (0xb8, "SC_VIH_CloseVIH", SC_VIH_CloseVIH as *const () as usize),
        (0xc0, "SC_GetOriginalShaderOffsetInBytes", SC_GetOriginalShaderOffsetInBytes as *const () as usize),
        (0xc8, "SC_DebugMapHandle_GetMap", SC_DebugMapHandle_GetMap as *const () as usize),
        (0xd0, "SC_DebugMapHandle_Destroy", SC_DebugMapHandle_Destroy as *const () as usize),
        (0xd8, "SC_RegisterLifetimeHandle_GetMap", SC_RegisterLifetimeHandle_GetMap as *const () as usize),
        (0xe0, "SC_RegisterLifetimeHandle_Destroy", SC_RegisterLifetimeHandle_Destroy as *const () as usize),
        (0xe8, "SC_VIH_GetNamedFlags", SC_VIH_GetNamedFlags as *const () as usize),
        (0xf0, "SC_Assemble", SC_Assemble as *const () as usize),
        (0xf8, "SC_AssembleHandle_GetShader", SC_AssembleHandle_GetShader as *const () as usize),
        (0x100, "SC_AssembleHandle_Destroy", SC_AssembleHandle_Destroy as *const () as usize),
        (0x108, "SC_VIH_GetUniqueHash", SC_VIH_GetUniqueHash as *const () as usize),
        (0x110, "SC_VIH_GetOutputStats", SC_VIH_GetOutputStats as *const () as usize),
        (0x118, "SC_InstantiateTypedVIHFromDXBC", SC_InstantiateTypedVIHFromDXBC as *const () as usize),
        (0x120, "SC_GetShaderStatsFromDXBC", SC_GetShaderStatsFromDXBC as *const () as usize),
        (0x128, "SC_IdMapHandle_GetIdMap", SC_IdMapHandle_GetIdMap as *const () as usize),
        (0x130, "SC_IdMap_Release", SC_IdMap_Release as *const () as usize),
        (0x138, "SC_IdMapHandle_Destroy", SC_IdMapHandle_Destroy as *const () as usize),
        (0x140, "SC_ConvertRootSignatureToSCInputs", SC_ConvertRootSignatureToSCInputs as *const () as usize),
        (0x148, "SC_VIH_GetCompileSettings", SC_VIH_GetCompileSettings as *const () as usize),
        (0x150, "SC_CompileShaderFromMetadata", SC_CompileShaderFromMetadata as *const () as usize),
        (0x158, "SC_MetadataHandle_Destroy", SC_MetadataHandle_Destroy as *const () as usize),
        (0x160, "sc_table_reserved", sc_table_reserved as *const () as usize),
        (0x168, "SC_SCDX12TableDesc_Destroy", SC_SCDX12TableDesc_Destroy as *const () as usize),
        (0x170, "SC_CompressExtent", SC_CompressExtent as *const () as usize),
        (0x178, "SC_CompressExtent_Destroy", SC_CompressExtent_Destroy as *const () as usize),
        (0x180, "SC_CBUsageHandle_Destroy", SC_CBUsageHandle_Destroy as *const () as usize),
    ]
}

pub const ZEROED_SLOTS: [usize; 2] = [0x20, 0x98];

#[no_mangle]
pub unsafe extern "system" fn SCGetExportFunctions(table: *mut u8) -> u32 {
    if table.is_null() {
        return SC_FAILED_TO_INITIATE;
    }
    for z in ZEROED_SLOTS {
        (table.add(z) as *mut usize).write_unaligned(0);
    }
    for (off, _, f) in export_table() {
        (table.add(off) as *mut usize).write_unaligned(f);
    }
    SC_OK
}
