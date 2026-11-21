#![allow(non_snake_case, clippy::missing_safety_doc)]

use console_image::ConsoleDll;

pub const E_FAIL: u32 = 0x8000_4005;

pub static CONSOLE: ConsoleDll = ConsoleDll::new("newbe_xs.dll", "TOMBOX_NEWBE_XS_CONSOLE", "newbe_xs_console.dll", "NB_GetRegPressure", &[]);

pub fn own() -> usize {
    own as *const () as usize
}

console_image::forward! {
    CONSOLE, own();
    NB_CompilePipeline = E_FAIL;
    NB_CompileShader = E_FAIL;
    NB_CreateOptimizer = E_FAIL;
    NB_CreatePixDwarfApi = E_FAIL;
    NB_GetRegPressure = E_FAIL;
    NB_GetShaderStats = E_FAIL;
}
