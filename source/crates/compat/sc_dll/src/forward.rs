use crate::console;

pub type Fwd = unsafe extern "system" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize) -> usize;

pub const FAIL_INIT: usize = 3;
pub const FAIL_COMPRESS: usize = 1;
pub const FAIL_NONE: usize = 0;

macro_rules! forward {
    ($($name:ident = $fail:expr;)*) => {
        $(
            #[no_mangle]
            pub unsafe extern "system" fn $name(a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize, a8: usize, a9: usize, a10: usize, a11: usize, a12: usize) -> usize {
                match console::proc(concat!(stringify!($name), "\0")) {
                    Some(p) => {
                        let f: Fwd = core::mem::transmute(p);
                        f(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12)
                    }
                    None => {
                        console::missing(stringify!($name), $fail);
                        $fail
                    }
                }
            }
        )*
        pub const FORWARDED: &[(&str, Fwd, usize)] = &[$((stringify!($name), $name as Fwd, $fail)),*];
    };
}

forward! {
    SCCreate = FAIL_NONE;
    SCDestroy = FAIL_INIT;
    SCCompileShader = FAIL_INIT;
    SCSimpleCompileShader = FAIL_INIT;
    SCSimpleDumpShader = FAIL_NONE;
    SC_OutputHandle_CreateFromExtent = FAIL_NONE;
    SC_OutputHandle_CreateFromShaderObject = FAIL_NONE;
    SC_OutputHandle_GetSerializedObjectExtent = FAIL_NONE;
    SC_OutputHandle_ReleaseSerializedObjectExtent = FAIL_NONE;
    SC_OutputHandle_GetShaderObject = FAIL_NONE;
    SC_OutputHandle_Destroy = FAIL_NONE;
    SC_InstantiateVIHFromDXBC = FAIL_NONE;
    SC_InstantiateTypedVIHFromDXBC = FAIL_NONE;
    SC_VIH_GetPDBName = FAIL_NONE;
    SC_VIH_GetPDBNameLengthInBytes = FAIL_NONE;
    SC_VIH_GetNamedFlags = FAIL_NONE;
    SC_VIH_GetCompileSettings = FAIL_NONE;
    SC_VIH_CloseVIH = FAIL_NONE;
    SC_VIH_GetUniqueHash = FAIL_NONE;
    SC_VIH_GetOutputStats = FAIL_NONE;
    SC_GetShaderStatsFromDXBC = FAIL_NONE;
    SC_GetOriginalShaderOffsetInBytes = FAIL_NONE;
    SC_DebugMapHandle_Destroy = FAIL_NONE;
    SC_RegisterLifetimeHandle_Destroy = FAIL_NONE;
    SC_IdMapHandle_Destroy = FAIL_NONE;
    SC_IdMapHandle_GetIdMap = FAIL_NONE;
    SC_IdMap_Release = FAIL_NONE;
    SC_CBUsageHandle_Destroy = FAIL_NONE;
    SC_DXBC_GetExternalILInterface = FAIL_NONE;
    SC_CompressExtent = FAIL_COMPRESS;
    SC_CompressExtent_Destroy = FAIL_NONE;
    SC_SCDX12TableDesc_Destroy = FAIL_NONE;
    SC_ConvertRootSignatureToSCInputs = FAIL_INIT;
    OG_InitOptionGenerator = FAIL_INIT;
    OG_OptionGeneratorDestroy = FAIL_INIT;
    OG_GetNextOptionSet = FAIL_INIT;
    OG_GetOptionSetID = FAIL_INIT;
    OG_ApplyOptionSetToSIS = FAIL_INIT;
    OG_SetOptionPerf = FAIL_INIT;
    OG_GetOptionPerf = FAIL_INIT;
    OG_GetBestOption = FAIL_INIT;
    OG_GetSerializedOptionSet = FAIL_INIT;
    OG_ApplyCompactOptionSetToSIS = FAIL_INIT;
    OG_ApplySISToCompactOptionSet = FAIL_INIT;
    OG_GetSerializedCompactOptionSet = FAIL_INIT;
}
