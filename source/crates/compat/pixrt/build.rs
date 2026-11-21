use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let table = dir.join("exports.txt");
    println!("cargo:rerun-if-changed={}", table.display());
    println!("cargo:rustc-env=PIXRT_EXPORTS={}", table.display());
    println!("cargo::rustc-check-cfg=cfg(pixrt_classic)");
    let lib = std::env::var("CARGO_PKG_NAME").unwrap();
    if lib == "pixrt" {
        println!("cargo:rustc-cfg=pixrt_classic");
    }
    let text = std::fs::read_to_string(&table).unwrap_or_else(|e| panic!("{}: {e}", table.display()));
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").map(|e| e == "msvc").unwrap_or(false);
    for e in xone_exports::parse(&text) {
        match e.status.as_str() {
            "implemented" => {
                if msvc {
                    println!("cargo:rustc-cdylib-link-arg=/EXPORT:{},@{}", e.name, e.ordinal);
                }
            }
            "missing" => {}
            s => panic!("{}: status {s} not allowed", e.name),
        }
    }
}
