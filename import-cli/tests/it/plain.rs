//! A program with one Cargo dependency and no GUI, run by the real
//! `rawr-import` binary. It finds `rawr-import-driver` beside itself, as an
//! install does: build it first with
//! `cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver`.
//! The project sits under Cargo's test directory and keeps its `target/`
//! between runs, as a user's project does.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/plain-dependency"
);

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

fn rawr_import(root: &Path, arguments: &[&str]) -> Output {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .args(arguments)
        .arg(root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_plain_program_with_a_dependency_runs_in_a_runner_without_iced() {
    let driver = Path::new(env!("CARGO_BIN_EXE_rawr-import")).with_file_name(format!(
        "rawr-import-driver{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        driver.is_file(),
        "build {} first: cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver",
        driver.display()
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plain-dependency");
    copy(Path::new(FIXTURE), &root);

    let run = rawr_import(&root, &["run"]);

    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(text(&run.stdout), "parsed: 1.2.3\nmajor: 1\n");
    let runner = fs::read_to_string(root.join("target/rawr/runner/Cargo.toml")).unwrap();
    assert!(
        runner.contains("__rawr_internal = { package = \"rawr\""),
        "{runner}"
    );
    assert!(runner.contains("semver = "), "{runner}");
    assert!(!runner.contains("iced"), "{runner}");

    let checked = rawr_import(&root, &["check"]);
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    // `cargo check` runs beside Rawr's check and reports after it.
    assert_eq!(
        text(&checked.stdout),
        format!(
            "Checked {}\ncargo check passed\n",
            root.join("src/main.rs").display()
        )
    );
}

/// A project that imports Rust without the importer's two files is told
/// which ones to add, before any build.
#[test]
fn a_dependency_without_the_importers_files_names_them() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plain-dependency-bare");
    let _ = fs::remove_dir_all(&root);
    copy(Path::new(FIXTURE), &root);
    fs::remove_file(root.join(".cargo/config.toml")).unwrap();

    let run = rawr_import(&root, &["run"]);

    assert!(!run.status.success());
    let stderr = text(&run.stderr);
    assert!(
        stderr.contains("RAWR-ENVIRONMENT-PROJECT-BUILD"),
        "{stderr}"
    );
    assert!(stderr.contains("so add `.cargo/config.toml`"), "{stderr}");
    assert!(stderr.contains("target = \"host-tuple\""), "{stderr}");
    fs::remove_dir_all(&root).unwrap();
}
