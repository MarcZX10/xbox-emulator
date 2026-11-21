use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("shaders");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("query_accum.dxil");
    println!("cargo:rerun-if-changed=shaders/query_accum.hlsl");
    println!("cargo:rerun-if-changed=shaders/query_accum.dxil");
    let hlsl = std::fs::read_to_string(dir.join("query_accum.hlsl")).expect("shaders/query_accum.hlsl");
    let code = match tombox_xshader::dxc::compile(&hlsl, "cs_6_0") {
        Ok(c) => c,
        Err(e) => {
            println!("cargo:warning=query_accum.hlsl not recompiled ({e}); using the checked-in shaders/query_accum.dxil");
            std::fs::read(dir.join("query_accum.dxil")).expect("shaders/query_accum.dxil")
        }
    };
    std::fs::write(&out, code).expect("write query_accum.dxil");
}
