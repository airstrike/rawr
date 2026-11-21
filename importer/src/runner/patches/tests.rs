use std::path::Path;
use std::process::Command;

use toml_edit::DocumentMut;

/// Write each `(path, text)` under `root`.
fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// Cargo's `subcommand` on the runner at `runner`, offline, as the runner's
/// settle and build run it: its status and its stderr.
fn cargo(runner: &Path, subcommand: &[&str]) -> (bool, String) {
    let output = Command::new("cargo")
        .args(subcommand)
        .arg("--offline")
        .arg("--manifest-path")
        .arg(runner.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", runner.join("target"))
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

/// The runner's manifest, with `patches` as its `[patch.crates-io]`.
fn patched(patches: &str) -> String {
    format!(
        "[workspace]\n\n[package]\nname = \"rawr-generated-runner\"\nversion = \"0.0.0\"\n\
         edition = \"2024\"\n\n[patch.crates-io]\n{patches}\n[dependencies]\n\
         app = {{ path = \"app\" }}\n"
    )
}

/// A package only a feature reaches: the program's crate turns on
/// `mid/test`, and `mid`, a crate outside the workspace as a git crate is,
/// then needs `ghost`, which only the project's `[patch]` provides. The
/// harness, which builds the program's crate with no features, never reaches
/// `ghost`, so its lock lists that patch as unused. Under the runner's root
/// the program's crate is a member, and cargo settles its lock with every
/// feature on: without the patch, the settle fails as the generator's
/// pruned manifest made it fail. With every patch, it settles, and only the
/// patch nothing uses leaves the runner.
#[test]
fn a_patch_only_a_feature_reaches_stays_in_the_runner() {
    let project = crate::store::TestProject::new("rawr-feature-patch");
    let root = project.path();
    let package = |name: &str, rest: &str| {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.15.0-dev\"\nedition = \"2024\"\n{rest}"
        )
    };
    let app = package(
        "app",
        "\n[dependencies]\nmid = \"0.15.0-dev\"\n\n[features]\npreview = [\"mid/test\"]\n",
    );
    let mid = package(
        "mid",
        "\n[dependencies]\nghost = { version = \"0.15.0-dev\", optional = true }\n\n\
         [features]\ntest = [\"dep:ghost\"]\n",
    );
    let (ghost, spare) = (package("ghost", ""), package("spare", ""));
    write(
        root,
        &[
            ("runner/src/main.rs", "fn main() {}\n"),
            ("runner/app/Cargo.toml", &app),
            ("runner/app/src/lib.rs", ""),
            ("mid/Cargo.toml", &mid),
            ("mid/src/lib.rs", ""),
            ("ghost/Cargo.toml", &ghost),
            ("ghost/src/lib.rs", ""),
            ("spare/Cargo.toml", &spare),
            ("spare/src/lib.rs", ""),
        ],
    );
    let runner = root.join("runner");
    let settle = ["update", "--workspace", "--quiet"];
    let mid_patch = "mid = { path = \"../mid\" }\n";
    // The patches the harness's lock used, as the generator kept them.
    write(&runner, &[("Cargo.toml", &patched(mid_patch))]);
    let (settled, stderr) = cargo(&runner, &settle);
    assert!(!settled, "a runner without the patch settles: {stderr}");
    assert!(stderr.contains("ghost"), "{stderr}");
    let every = format!(
        "{mid_patch}ghost = {{ path = \"../ghost\" }}\nspare = {{ path = \"../spare\" }}\n"
    );
    write(&runner, &[("Cargo.toml", &patched(&every))]);
    let _ = std::fs::remove_file(runner.join("Cargo.lock"));
    let (settled, stderr) = cargo(&runner, &settle);
    assert!(settled, "{stderr}");
    let manifest = std::fs::read_to_string(runner.join("Cargo.toml")).unwrap();
    let lock = std::fs::read_to_string(runner.join("Cargo.lock")).unwrap();
    assert!(lock.contains("name = \"ghost\""), "{lock}");
    let (manifest, lock) = super::pruned(&manifest, &lock).expect("`spare` is unused");
    assert!(
        manifest.contains("ghost = { path = \"../ghost\" }"),
        "{manifest}"
    );
    assert!(
        manifest.contains("mid = { path = \"../mid\" }"),
        "{manifest}"
    );
    assert!(!manifest.contains("spare"), "{manifest}");
    assert!(!lock.contains("spare"), "{lock}");
    write(&runner, &[("Cargo.toml", &manifest), ("Cargo.lock", &lock)]);
    let (built, stderr) = cargo(&runner, &["metadata", "--locked", "--format-version=1"]);
    assert!(built, "{stderr}");
    assert!(!stderr.contains("was not used"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(runner.join("Cargo.lock")).unwrap(),
        lock
    );
}

/// A name that one registry's patch uses keeps the patch another registry
/// leaves unused, as cargo keeps both, and a lock that lists no unused
/// patch changes nothing.
#[test]
fn a_name_the_lock_uses_keeps_every_patch_of_it() {
    let manifest = "[patch.crates-io]\nui = { git = \"https://example.invalid/fork\" }\n\n\
                    [patch.\"https://example.invalid/ui\"]\nui = { git = \"https://example.invalid/fork\" }\n";
    let lock = "version = 4\n\n[[package]]\nname = \"ui\"\nversion = \"0.1.0\"\n\
                source = \"git+https://example.invalid/fork#0123\"\n\n[[patch.unused]]\n\
                name = \"ui\"\nversion = \"0.1.0\"\nsource = \"git+https://example.invalid/fork#0123\"\n";
    assert!(super::pruned(manifest, lock).is_none());
    let mut manifest = manifest.parse::<DocumentMut>().unwrap();
    let mut lock = "version = 4\n".parse::<DocumentMut>().unwrap();
    assert!(super::prune(&mut manifest, &mut lock).is_none());
}
