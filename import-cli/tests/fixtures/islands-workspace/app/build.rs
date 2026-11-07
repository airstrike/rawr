//! Writes one icon name per line of `icons.txt` to `OUT_DIR/icons.rs`. It
//! prints no `rerun-if` line, so Cargo runs it again when any file of the
//! package changes.
fn main() {
    let names = std::fs::read_to_string("icons.txt").unwrap();
    let names: Vec<String> = names.lines().map(|name| format!("{name:?}")).collect();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(
        out.join("icons.rs"),
        format!("pub const ICONS: &[&str] = &[{}];\n", names.join(", ")),
    )
    .unwrap();
}
