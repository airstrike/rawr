//! T653 (D683.3): std's `PathBuf`, `&Path` and `io::Error` are the same
//! types in a program and in the crates it uses. A crate struct's `PathBuf`
//! field, a crate function taking `&Path` or giving an `io::Error`, and `?`
//! from an `io::Result` into a crate error that converts as thiserror's
//! `#[from]` writes it all print what `cargo run` prints. So do a crate
//! function and method taking `&mut PathBuf`, which change the program's
//! local and struct field, and a method giving `&io::Error`, which lends
//! the error its value holds (#680). Build the driver first, as for
//! `plain`.

use std::path::Path;
use std::process::{Command, Output};

use crate::support::{copy, Scratch};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/std-types");

fn fixture(name: &str) -> Scratch {
    let root = crate::support::fresh(name);
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
    assert!(
        expected.ends_with(
            "extended data/notes/b.txt 3\n\
             job job jobs/first\n\
             roomy true true\n\
             error lost.txt is missing NotFound\n\
             cause NotFound lost.txt\n\
             appended data/notes/b.txt/lost.txt jobs/first/lost.txt\n"
        ),
        "{expected}"
    );

    let directory = crate::support::scratch();
    let rawr = run(crate::support::rawr_import()
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
