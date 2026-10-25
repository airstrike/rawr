//! A program whose `main` is native Rust and calls a marked module (D501),
//! run by the real `rawr-import` binary while its marked file is edited: a
//! body edit reaches the running program at its next call, and a struct's
//! new field restarts it with a one-line reason. Build the driver first, as
//! for `plain`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::support::{copy, start};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands");

/// `tick value`, from a line the fixture prints.
fn parse(line: &str) -> Option<(i64, i64)> {
    let (tick, value) = line.split_once(" value ")?;
    Some((tick.trim().parse().ok()?, value.trim().parse().ok()?))
}

fn edit(path: &Path, from: &str, to: &str) {
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{text}");
    fs::write(path, text.replace(from, to)).unwrap();
}

#[test]
fn a_native_main_sees_a_body_edit_at_its_next_call_and_restarts_on_a_new_field() {
    let driver = Path::new(env!("CARGO_BIN_EXE_rawr-import")).with_file_name(format!(
        "rawr-import-driver{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        driver.is_file(),
        "build {} first: cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver",
        driver.display()
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("islands");
    let _ = fs::remove_file(root.join("stop"));
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
    let physics = root.join("src/physics.rs");
    let mut run = start(&root, &[("ISLANDS_STOP", root.join("stop"))]);

    // The first run builds the runner.
    let first = run.until(Duration::from_secs(900), |line| {
        parse(line).is_some_and(|(tick, _)| tick >= 3)
    });
    assert_eq!(parse(&first), Some((3, 3)));

    // A body edit: the running native loop keeps its count, and its next
    // calls run the new code.
    edit(&physics, "const SCALE: i64 = 1;", "const SCALE: i64 = 100;");
    let swapped = run.until(Duration::from_secs(120), |line| {
        parse(line).is_some_and(|(tick, value)| value == tick * 100)
    });
    let (tick, _) = parse(&swapped).unwrap();
    assert!(tick > 3, "{}", run.seen.join("\n"));
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );

    // A new field changes what native code sees: a new native build, and
    // the program starts again from its first tick.
    edit(
        &physics,
        "    pub ticks: i64,\n}",
        "    pub ticks: i64,\n    pub label: String,\n}",
    );
    edit(
        &physics,
        "Counter { ticks: 0 }",
        "Counter { ticks: 0, label: String::new() }",
    );
    let restarted = run.until(Duration::from_secs(600), |line| {
        parse(line) == Some((1, 100))
    });
    assert_eq!(restarted, "1 value 100");
    assert!(
        run.seen.iter().any(|line| line
            == "stderr: rawr: `physics::Counter` changed its fields; restarting the program"),
        "{}",
        run.seen.join("\n")
    );

    fs::write(root.join("stop"), "").unwrap();
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// winit's event loop must run on the process's main thread, so the
/// runner keeps that thread for a native `main` and runs its protocol on
/// another one.
#[test]
fn a_native_main_runs_on_the_process_main_thread() {
    const MAIN_THREAD: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-main-thread"
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("islands-main-thread");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(MAIN_THREAD), &root);
    let mut run = start(&root, &[]);
    let line = run.until(Duration::from_secs(900), |line| {
        line.starts_with("main thread ")
    });
    assert_eq!(line, "main thread true", "{}", run.seen.join("\n"));
    run.until(Duration::from_secs(30), |line| line == "answer 42");
    // The interpreter keeps a `u64` in an `i64`'s bits.
    run.until(Duration::from_secs(30), |line| {
        line == "wide 13907096814508502785"
    });
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

#[test]
fn a_native_main_gets_its_arguments_and_stdin_and_swaps_while_it_waits_on_stdin() {
    use std::io::Write as _;

    const ARGUMENTS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-arguments"
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("islands-arguments");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(ARGUMENTS), &root);
    let words = root.join("src/words.rs");
    let mut run = crate::support::start_with(
        &root,
        &[],
        &["one", "two words"],
        std::process::Stdio::piped(),
    );
    let mut stdin = run.child.stdin.take().unwrap();

    // `cargo run` names the program by its binary.
    let name = run.until(Duration::from_secs(900), |line| line.starts_with("name "));
    assert!(
        name.ends_with("/target/debug/islands-arguments"),
        "{}",
        run.seen.join("\n")
    );
    run.until(Duration::from_secs(30), |line| line == "arg ONE");
    run.until(Duration::from_secs(30), |line| line == "arg TWO WORDS");
    stdin.write_all(b"alpha\n").unwrap();
    run.until(Duration::from_secs(30), |line| line == "line ALPHA");

    // While the program waits on stdin, a body edit swaps in for its next
    // call.
    std::thread::sleep(Duration::from_secs(2));
    edit(&words, "text.to_uppercase()", "format!(\"{text}!\")");
    std::thread::sleep(Duration::from_secs(5));
    if let Err(error) = stdin.write_all(b"beta\n") {
        let _ = run.until(Duration::from_secs(2), |_| false);
        panic!("{error}: {}", run.seen.join("\n"));
    }
    run.until(Duration::from_secs(60), |line| line == "line beta!");
    drop(stdin);
    run.until(Duration::from_secs(30), |line| line == "done");
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );
}
