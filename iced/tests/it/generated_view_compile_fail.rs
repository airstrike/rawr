use std::path::PathBuf;
use std::process::Command;

#[test]
fn native_element_and_callback_cannot_escape_the_view_region() {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = package.join("tests/fixtures/view_escape/Cargo.toml");
    let output = Command::new(cargo())
        .args(["check", "--offline", "-j1", "--manifest-path"])
        .arg(manifest)
        .env("CARGO_TARGET_DIR", package.join("../target"))
        .output()
        .expect("the compile-fail fixture launches");
    assert!(
        !output.status.success(),
        "the escape fixture unexpectedly compiled"
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("lifetime may not live long enough")
            || error.contains("does not live long enough")
            || error.contains("must outlive"),
        "the fixture failed for an unrelated reason:\n{error}"
    );
}

fn cargo() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}
