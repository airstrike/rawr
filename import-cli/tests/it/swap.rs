//! A marked `main` that hands control to a native loop keeps running across
//! a save (D553): the loop's next call into script code runs the new body,
//! in the same process, and a script type's new field restarts the program
//! with a one-line reason. Build the driver first, as for `islands`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::support::{copy, start};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/marked-ticker");

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
fn a_marked_main_in_a_native_loop_takes_a_body_edit_at_its_next_call() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("marked-ticker");
    let _ = fs::remove_file(root.join("stop"));
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
    let main = root.join("src/main.rs");
    let mut run = start(&root, &[("TICKER_STOP", root.join("stop"))]);

    // The first run builds the runner.
    let pid = run.until(Duration::from_secs(900), |line| line.starts_with("pid "));
    let first = run.until(Duration::from_secs(60), |line| {
        parse(line).is_some_and(|(tick, _)| tick >= 3)
    });
    assert_eq!(parse(&first), Some((3, 3)));

    // A body edit: the native loop keeps its count and its process, and
    // its next calls run the new code.
    edit(&main, "Scale { factor: 1 }", "Scale { factor: 100 }");
    let swapped = run.until(Duration::from_secs(60), |line| {
        parse(line).is_some_and(|(tick, value)| value == tick * 100)
    });
    let (tick, _) = parse(&swapped).unwrap();
    assert!(tick > 3, "{}", run.seen.join("\n"));
    let seen = run.seen.join("\n");
    assert!(!seen.contains("restarting"), "{seen}");
    assert_eq!(
        run.seen
            .iter()
            .filter(|line| line.starts_with("pid "))
            .count(),
        1,
        "{seen}"
    );
    assert!(run.seen.contains(&pid));

    // A new field: native code may hold the old shape, so the program
    // restarts from its first tick with the reason.
    edit(
        &main,
        "    factor: i64,\n}",
        "    factor: i64,\n    name: String,\n}",
    );
    edit(
        &main,
        "Scale { factor: 100 }",
        "Scale { factor: 100, name: String::new() }",
    );
    let restarted = run.until(Duration::from_secs(600), |line| {
        parse(line) == Some((1, 100))
    });
    assert_eq!(restarted, "1 value 100");
    assert!(
        run.seen
            .iter()
            .any(|line| line == "stderr: rawr: `Scale` changed its fields; restarting the program"),
        "{}",
        run.seen.join("\n")
    );

    // `main` ran once: an edit to it restarts the program.
    edit(
        &main,
        "ticker::run(step, 3000);",
        "ticker::run(step, 2999);",
    );
    run.until(Duration::from_secs(600), |line| {
        parse(line) == Some((1, 100))
    });
    assert!(
        run.seen
            .iter()
            .any(|line| line == "stderr: rawr: `main` changed; restarting the program"),
        "{}",
        run.seen.join("\n")
    );

    fs::write(root.join("stop"), "").unwrap();
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}
