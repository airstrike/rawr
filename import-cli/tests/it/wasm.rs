//! A package whose `.cargo/config.toml` sets `build.target` to wasm32 is
//! read for wasm32, as Cargo builds it (#1394): its crates' cfgs follow the
//! target, so an item only wasm32 has is there and one it lacks is not. It
//! needs the wasm32 target: `rustup target add wasm32-unknown-unknown`.
//! Build the driver first, as for `plain`.

use std::path::Path;
use std::process::{Command, Output};

use crate::support::Fixture;

static WASM: Fixture = Fixture::new("wasm-target");

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `rawr-import check` in the package at `root`.
fn check(root: &Path, home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .current_dir(root)
        .env("RAWR_HOME", home)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap()
}

#[test]
fn a_package_built_for_wasm32_is_read_for_wasm32() {
    let home = crate::support::scratch().join("rawr-home");

    let root = WASM.lease(&[]);
    let checked = check(&root, &home);
    assert!(
        checked.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&checked.stdout),
        text(&checked.stderr)
    );
    drop(root);

    let root = WASM.lease(&[(
        "src/main.rs",
        "// rawr\nfn main() {\n    println!(\"{}\", facts::native());\n}\n",
    )]);
    let checked = check(&root, &home);
    let stderr = text(&checked.stderr);
    assert!(!checked.status.success(), "{stderr}");
    assert!(
        stderr.contains("cannot find function `native` in crate `facts`"),
        "{stderr}"
    );
}
