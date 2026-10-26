//! D558's second slice: values that borrow what they're made from live in
//! a native scope frame for the Rust scope they are bound in. A marked
//! crate runs pulldown-cmark's `for event in Parser::new(&text)` and
//! quick-xml's `read_event_into` loop as each crate's docs write them, and
//! prints what `cargo run` prints. Build the driver first, as for `plain`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::support::copy;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/scope-parsers");

fn fixture(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
    root
}

fn rawr_import(root: &Path) -> Output {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
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

/// Every event pulldown-cmark's parser gives, quick-xml's README loop with
/// its attributes, names and decoded text, and the buffer `read_event_into`
/// fills, given back once the event that borrows it is dropped, are what
/// `cargo run` prints.
#[test]
fn borrowing_parsers_in_scope_frames_print_what_cargo_run_prints() {
    let root = fixture("scope-parsers");
    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", text(&native.stderr));
    let expected = text(&native.stdout);
    assert!(expected.contains("Text(Borrowed(\"Title\"))"), "{expected}");
    assert!(
        expected.contains("attributes values: [[116, 101, 115, 116]]"),
        "{expected}"
    );
    assert!(expected.contains("buffered 3 [60, 97, 62]"), "{expected}");

    let run = rawr_import(&root);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(
        text(&run.stdout),
        expected,
        "stderr:\n{}",
        text(&run.stderr)
    );
}

/// `read_event` belongs to quick-xml's `impl<'a> Reader<&'a [u8]>`. Rawr
/// admits it on every `Reader`, a difference `cargo check` reports: on a
/// `Reader<String>` the program stops before it prints, and `cargo check`
/// gives rustc's E0599. It never runs with another meaning.
#[test]
fn a_member_of_one_instance_on_another_fails_loudly() {
    let root = fixture("scope-parsers-instance");
    fs::write(
        root.join("src/main.rs"),
        "// rawr\nuse quick_xml::reader::Reader;\n\nfn main() {\n    let mut reader = Reader::from_reader(String::from(\"<a/>\"));\n    let event = reader.read_event();\n    println!(\"read {event:?}\");\n}\n",
    )
    .unwrap();
    for module in ["buffer.rs", "events.rs", "frames.rs", "reader.rs"] {
        fs::remove_file(root.join("src").join(module)).unwrap();
    }
    let run = rawr_import(&root);
    let stdout = text(&run.stdout);
    let stderr = text(&run.stderr);
    assert!(
        !run.status.success(),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!stdout.contains("read "), "{stdout}");
    let all = format!("{stdout}{stderr}");
    assert!(all.contains("E0599"), "{all}");
}
