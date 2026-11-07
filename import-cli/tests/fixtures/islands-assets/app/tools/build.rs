//! Reads `icons/list.txt` from the package and writes one name per line to
//! `OUT_DIR/icons.rs`, as an app's build script generates its icons. Its
//! module `icons` is `tools/icons.rs`, beside it, and that module's own
//! module is `tools/icons/names.rs` (#1143).
mod icons;

fn main() {
    println!("cargo:rerun-if-changed=icons/list.txt");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("icons.rs"), icons::source("icons/list.txt")).unwrap();
}
