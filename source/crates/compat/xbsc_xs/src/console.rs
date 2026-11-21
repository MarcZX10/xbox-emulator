use console_image::ConsoleDll;

pub static CONSOLE: ConsoleDll = ConsoleDll::new(
    "xbsc_xs.dll",
    "TOMBOX_XBSC_XS_CONSOLE",
    "xbsc_xs_console.dll",
    "XBSC_DisassembleHwPipelineWithCallback",
    &["newbe_xs.dll", "dxbc2dxil_xs.dll"],
);

pub fn own() -> usize {
    own as *const () as usize
}

pub unsafe fn is_console_handle(h: *const crate::result::Handle) -> bool {
    !h.is_null() && (*h).vtbl != 0
}

pub unsafe fn call(name: &str, args: &[usize]) -> Option<usize> {
    let p = CONSOLE.proc(own(), name)?;
    let f: console_image::Fwd = core::mem::transmute(p);
    let mut a = [0usize; 12];
    a[..args.len()].copy_from_slice(args);
    Some(f(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10], a[11]))
}

console_image::forward! { CONSOLE, own();
    SCCompileMultiShadersXbox = 0;
    SCDumpHwShader = 0;
    SCDumpHwShaderData = 0;
    SCDumpMergedHwShader = 0;
    SCDumpMergedHwShaderData = 0;
    SCDumpRawShader = 0;
    SC_ConvertRootSignatureToSCInputs = 0;
    SC_OutputHandle_GetShaderObject = 0;
    SC_SCDX12TableDesc_Destroy = 0;
    XBSC_CompileRootSignature = 0;
    XBSC_CreatePixDwarfApi = 1;
    XBSC_DeserializePipelineCompileOutput = 0;
    XBSC_DeserializeSrcPipeline = 0;
    XBSC_DisassembleHwPipeline = 0;
    XBSC_DisassembleHwPipelineWithCallback = 0;
    XBSC_DisassembleRawShader = 0;
    XBSC_DumpProgramShaderStats = 0;
    XBSC_DumpShaderStats = 0;
    XBSC_GetRegPressure = 1;
    XBSC_GetShaderStats = 1;
    XBSC_PipelineCompile = 0;
    XBSC_SerializePipelineCompileInput = 0;
    XBSC_SerializePipelineCompileOutput = 0;
    XBSC_SerializeSrcPipeline = 0;
}
