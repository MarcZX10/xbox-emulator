fn main() {
    println!("cargo:rerun-if-changed=exports.txt");
    let text = std::fs::read_to_string("exports.txt").expect("exports.txt");
    if std::env::var("CARGO_CFG_TARGET_ENV").map(|e| e == "msvc").unwrap_or(false) {
        for e in xone_exports::parse(&text).into_iter().filter(|e| e.status != "missing") {
            println!("cargo:rustc-cdylib-link-arg=/EXPORT:{},@{}", e.name, e.ordinal);
        }
    }
}
