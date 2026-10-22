use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    fs::write(
        output.join("fallback.rs"),
        "pub const FALLBACK: u32 = 7;\n",
    )
    .expect("the fixture can publish generated source");
}
