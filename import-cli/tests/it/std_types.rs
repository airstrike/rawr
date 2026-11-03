//! T653 (D683.3): std's `PathBuf`, `&Path` and `io::Error` are the same
//! types in a program and in the crates it uses. A crate struct's `PathBuf`
//! field, a crate function taking `&Path` or giving an `io::Error`, and `?`
//! from an `io::Result` into a crate error that converts as thiserror's
//! `#[from]` writes it all print what `cargo run` prints. Build the driver
//! first, as for `plain`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::support::copy;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/std-types");

fn fixture(name: &str) -> PathBuf {
    let root = crate::support::scratch().join(name);
    let _ = fs::remove_dir_all(root.join("src"));
    let _ = fs::remove_dir_all(root.join("files"));
    copy(Path::new(FIXTURE), &root);
    root
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn run(command: &mut Command) -> Output {
    command
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap()
}

#[test]
fn std_path_and_io_error_are_the_crates_own_types() {
    let root = fixture("std-types");
    let native = run(Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root));
    assert!(native.status.success(), "{}", text(&native.stderr));
    let expected = text(&native.stdout);
    assert!(
        expected.contains("entry data/notes/a.txt 3 a.txt\n"),
        "{expected}"
    );
    assert!(
        expected.contains("load io: No such file or directory"),
        "{expected}"
    );
    assert!(expected.contains("wrapped io: slow\n"), "{expected}");

    let directory = crate::support::scratch();
    let rawr = run(Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("RAWR_VERBOSE"));
    assert!(
        rawr.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&rawr.stdout),
        text(&rawr.stderr)
    );
    assert_eq!(
        text(&rawr.stdout),
        expected,
        "stderr:\n{}",
        text(&rawr.stderr)
    );
}
