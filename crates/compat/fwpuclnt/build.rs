fn main() {
    println!("cargo:rerun-if-changed=exports.txt");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").map(|e| e == "msvc").unwrap_or(false);
    if !msvc {
        return;
    }
    let text = std::fs::read_to_string("exports.txt").expect("exports.txt");
    for e in xone_exports::parse(&text) {
        if e.status == "missing" {
            continue;
        }
        match e.note.strip_prefix("PE forwarder to ").and_then(|r| r.split(';').next()) {
            Some(target) => println!("cargo:rustc-cdylib-link-arg=/EXPORT:{}={},@{}", e.name, target.trim(), e.ordinal),
            None => println!("cargo:rustc-cdylib-link-arg=/EXPORT:{},@{}", e.name, e.ordinal),
        }
    }
}
