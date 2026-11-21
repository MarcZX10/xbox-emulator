fn main() {
    xtfcommon::exports::emit_link_args("exports.txt");
    println!("cargo:rustc-check-cfg=cfg(fileio_b)");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-env-changed=XTFFILEIO_WITHOUT_B");
    let skip = std::env::var("XTFFILEIO_WITHOUT_B").map(|v| v == "1").unwrap_or(false);
    if !skip && std::path::Path::new("src/fileio_b.rs").exists() {
        println!("cargo:rustc-cfg=fileio_b");
    }
}
