use crate::p3;

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn generated_closed_subset_executes_direct_and_arena_backed_thunks() {
    let _turn = rawr_import::store::test_turn();
    let manifest = p3::generated();
    let root = std::env::temp_dir().join(format!(
        "rawr-monomorphic-execution-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("src")).expect("the generated test crate exists");
    for file in manifest.files() {
        let path = root.join(file.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("the generated parent exists");
        }
        fs::write(path, file.bytes()).expect("the exact generated input is written");
    }
    fs::write(
        root.join("rawr-generated-manifest.carrier"),
        manifest
            .generated_manifest()
            .carrier()
            .expect("the derived carrier encodes"),
    )
    .expect("the derived carrier is written");

    let output = Command::new(cargo())
        .args(["test", "--offline", "--locked", "-j1", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .expect("the generated runtime tests launch");
    let _ = fs::remove_dir_all(root);
    assert!(
        output.status.success(),
        "generated runtime tests failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn cargo() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}
