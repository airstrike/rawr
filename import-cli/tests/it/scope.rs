//! D558's second slice: values that borrow what they're made from live in
//! a native scope frame for the Rust scope they are bound in. A marked
//! crate runs pulldown-cmark's `for event in Parser::new(&text)` and
//! quick-xml's `read_event_into` loop as each crate's docs write them, and
//! prints what `cargo run` prints. Such a value also goes to a native call
//! as an argument, as pulldown-cmark's `push_html` takes its parser. Build
//! the driver first, as for `plain`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::support::copy;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/scope-parsers");

/// The fixture's modules besides `main.rs`.
const MODULES: [&str; 5] = [
    "buffer.rs",
    "events.rs",
    "frames.rs",
    "html.rs",
    "reader.rs",
];

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

/// A value a scope frame holds goes to a native call as an argument:
/// pulldown-cmark's README passes its `Parser` to `html::push_html(&mut
/// html_output, parser)`, which moves it out of the frame's region and
/// writes into the `String` it borrows mutably, given back after the call.
/// Rawr prints what `cargo run` prints.
#[test]
fn push_html_takes_the_parser_a_frame_holds_and_writes_the_string_it_borrows() {
    let root = fixture("scope-parsers-html");
    fs::write(
        root.join("src/main.rs"),
        "// rawr\nmod html;\n\nfn main() {\n    html::main();\n}\n",
    )
    .unwrap();
    for module in MODULES.iter().filter(|module| **module != "html.rs") {
        fs::remove_file(root.join("src").join(module)).unwrap();
    }
    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", text(&native.stderr));
    let expected = text(&native.stdout);
    assert!(
        expected.starts_with("<p>Hello world, this is a <del>complicated</del>"),
        "{expected}"
    );
    assert!(
        expected.contains("<!-- before -->\n<h1>Title</h1>"),
        "{expected}"
    );

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
    for module in MODULES {
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

/// A frame opens ahead of its statement, so a call written before the
/// borrowing call in the same statement would run after it. Rawr refuses
/// the statement, naming its line, rather than run them out of order.
#[test]
fn a_call_before_a_frame_in_its_statement_is_refused() {
    let root = fixture("scope-parsers-order");
    fs::write(
        root.join("src/main.rs"),
        "// rawr\nuse pulldown_cmark::Parser;\n\nfn note(text: &str) -> usize {\n    println!(\"note\");\n    text.len()\n}\n\nfn main() {\n    let text = String::from(\"*a*\");\n    let total = note(&text) + Parser::new(&text).count();\n    println!(\"total {total}\");\n}\n",
    )
    .unwrap();
    for module in MODULES {
        fs::remove_file(root.join("src").join(module)).unwrap();
    }
    let run = rawr_import(&root);
    let stdout = text(&run.stdout);
    let stderr = text(&run.stderr);
    assert!(
        !run.status.success(),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!stdout.contains("note"), "{stdout}");
    assert!(
        stderr.contains("in the statement at line 11")
            && stderr.contains("not supported in Rawr yet"),
        "{stderr}"
    );
}

/// The frame owns a copy of `text`, but the parser still borrows it in
/// Rust: rawr's own borrow check refuses a change to `text` while the
/// parser is used, with rustc's E0502, before anything runs (D572).
#[test]
fn a_parser_keeps_its_text_borrowed() {
    let root = fixture("scope-parsers-borrowed");
    fs::write(
        root.join("src/main.rs"),
        "// rawr\nuse pulldown_cmark::Parser;\n\nfn main() {\n    let mut text = String::from(\"*a*\");\n    println!(\"start\");\n    let parser = Parser::new(&text);\n    text.push_str(\"b\");\n    println!(\"events {}\", parser.count());\n}\n",
    )
    .unwrap();
    for module in MODULES {
        fs::remove_file(root.join("src").join(module)).unwrap();
    }
    let run = rawr_import(&root);
    let stdout = text(&run.stdout);
    let stderr = text(&run.stderr);
    assert!(
        !run.status.success(),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!stdout.contains("start"), "{stdout}");
    assert!(stderr.contains("E0502"), "{stderr}");
}
