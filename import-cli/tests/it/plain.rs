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

/// A marked `main` gets the arguments after `--` and stdin, as under
/// `cargo run`, and a save while it waits on stdin restarts it on the same
/// stdin (D500.1).
#[test]
fn a_marked_main_gets_its_arguments_and_stdin_and_restarts_on_a_save() {
    use std::io::Write as _;
    use std::time::Duration;

    const ARGUMENTS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/plain-arguments"
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plain-arguments");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(ARGUMENTS), &root);
    let main = root.join("src/main.rs");
    let mut run = crate::support::start_with(
        &root,
        &[],
        &["one", "two words"],
        std::process::Stdio::piped(),
    );
    let mut stdin = run.child.stdin.take().unwrap();

    let name = run.until(Duration::from_secs(900), |line| line.starts_with("name "));
    assert!(
        name.ends_with("/target/debug/plain-arguments"),
        "{}",
        run.seen.join("\n")
    );
    run.until(Duration::from_secs(30), |line| line == "arg one");
    run.until(Duration::from_secs(30), |line| line == "arg two words");
    stdin.write_all(b"1.2.3\n").unwrap();
    run.until(Duration::from_secs(30), |line| line == "major 1");

    // The program waits on stdin; a save restarts it with the same
    // arguments, and the next line goes to the new one.
    std::thread::sleep(Duration::from_secs(2));
    let text = fs::read_to_string(&main).unwrap();
    fs::write(&main, text.replace("\"major\"", "\"Major\"")).unwrap();
    run.until(Duration::from_secs(120), |line| line.starts_with("name "));
    assert!(
        run.seen
            .iter()
            .any(|line| line.ends_with("rawr: the source changed; restarting the program")),
        "{}",
        run.seen.join("\n")
    );
    run.until(Duration::from_secs(30), |line| line == "arg two words");
    stdin.write_all(b"4.5.6\n").unwrap();
    run.until(Duration::from_secs(30), |line| line == "Major 4");
    drop(stdin);
    run.until(Duration::from_secs(30), |line| line == "done");
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// A dependency's `macro_rules!` macros expand from its source as rustc
/// expands them, and the program prints what `cargo run` prints: `$crate`
/// paths into a crate the program does not name and to `#[doc(hidden)]`
/// items, repetitions with separators, a macro calling itself through
/// `$crate`, and a local of the macro's own beside the caller's (T215).
#[test]
fn a_dependencys_macro_rules_expand_from_its_source() {
    const MACROS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plain-macros");
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plain-macros");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(MACROS), &root);

    let run = rawr_import(&root, &["run"]);

    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(text(&run.stdout), "0 3 22\n8\n40 8\n116\n");
}
