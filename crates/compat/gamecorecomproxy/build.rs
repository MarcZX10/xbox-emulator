fn main() {
    println!("cargo:rerun-if-changed=data/proxy_19041.bin");
    xone_exports::emit_link_args("exports.txt");
}
