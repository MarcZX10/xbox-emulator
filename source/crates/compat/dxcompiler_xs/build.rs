fn main() {
    println!("cargo:rerun-if-changed=src/exports.txt");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").map(|e| e == "msvc").unwrap_or(false);
    if !msvc {
        return;
    }
    let text = std::fs::read_to_string("src/exports.txt").expect("src/exports.txt");
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (ord, name) = (f[0], f[1]);
        if let Some(feat) = f.get(2) {
            let var = format!("CARGO_FEATURE_{}", feat.to_uppercase().replace('-', "_"));
            if std::env::var_os(var).is_none() {
                continue;
            }
        }
        println!("cargo:rustc-cdylib-link-arg=/EXPORT:{name},@{ord}");
    }
}
