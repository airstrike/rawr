mod helper;

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    for name in [
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
        "RAWR_IMPORT_PLAN",
        "RAWR_IMPORT_PROTOCOL",
    ] {
        assert!(env::var_os(name).is_none(), "Rawr control leaked as {name}");
    }
    println!("cargo:rerun-if-changed=relative.txt");
    println!("cargo:rerun-if-changed=watched");
    println!("cargo:rerun-if-changed=missing.txt");
    println!(
        "cargo:rerun-if-changed={}",
        env::var("FRESH_ABSOLUTE").expect("the fixture receives its absolute declared input")
    );
    println!("cargo:rerun-if-env-changed=FRESH_ENV");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_NONEXISTENT");
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!("cargo:rerun-if-env-changed=RUSTDOC");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");
    println!("cargo:rerun-if-env-changed=CARGO_INCREMENTAL");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    let generated = if env::var_os("FRESH_EMBED_OUT_DIR").is_some() {
        format!(
            "pub const GENERATED: &str = {:?};\n",
            output.to_string_lossy()
        )
    } else {
        helper::generated().to_owned()
    };
    fs::write(output.join("generated.rs"), generated)
        .expect("the fixture can publish generated source");
}
