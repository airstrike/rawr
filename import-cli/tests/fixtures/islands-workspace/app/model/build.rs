//! Turns `grammar.txt` into `OUT_DIR/grammar.rs`, as a parser generator
//! does, and prints no `rerun-if` line.
fn main() {
    let rule = std::fs::read_to_string("grammar.txt").unwrap();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(
        out.join("grammar.rs"),
        format!("pub const RULE: &str = {:?};\n", rule.trim()),
    )
    .unwrap();
}
