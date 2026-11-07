//! Reads `icons/list.txt` from the package and writes one name per line to
//! `OUT_DIR/icons.rs`, as an app's build script generates its icons.
fn main() {
    println!("cargo:rerun-if-changed=icons/list.txt");
    let list = std::fs::read_to_string("icons/list.txt").unwrap();
    let names = list
        .lines()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(
        out.join("icons.rs"),
        format!("pub const ICONS: &[&str] = &[{names}];\n"),
    )
    .unwrap();
}
