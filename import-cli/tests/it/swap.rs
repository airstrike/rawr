//! A marked `main` that hands control to a native loop keeps running across
//! a save (D553): the loop's next call into script code runs the new body,
//! in the same process, and a script type's new field restarts the program
//! with a one-line reason. Build the driver first, as for `islands`.

use std::fs;
use std::path::Path;

use crate::support::{copy, start, Scratch};

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
    let root = crate::support::fresh("marked-ticker");
    copy(Path::new(FIXTURE), &root);
    let main = root.join("src/main.rs");
    let mut run = start(&root, &[("TICKER_STOP", root.join("stop"))]);

    // The first run builds the runner.
    let pid = run.until(|line| line.starts_with("pid "));
    let first = run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 3));
    assert_eq!(parse(&first), Some((3, 3)));

    // A body edit: the native loop keeps its count and its process, and
    // its next calls run the new code.
    edit(&main, "Scale { factor: 1 }", "Scale { factor: 100 }");
    let swapped = run.until(|line| parse(line).is_some_and(|(tick, value)| value == tick * 100));
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
    let restarted = run.until(|line| parse(line) == Some((1, 100)));
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
    run.until(|line| parse(line) == Some((1, 100)));
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

const RELOAD_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/reload-ticker");

/// The reload-ticker fixture under `name` in the scratch directory, its
/// host naming `rawr-reload` by its absolute path.
fn reload_ticker(name: &str) -> Scratch {
    let root = crate::support::fresh(name);
    copy(Path::new(RELOAD_FIXTURE), &root);
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    edit(
        &root.join("ticker/Cargo.toml"),
        "path = \"../../../../../reload\"",
        &format!(
            "path = {:?}",
            repository.join("reload").display().to_string()
        ),
    );
    root
}

/// A program with an editor of its own hears what each save did: a body
/// edit `swapped` (after `queued`, when it hears that), a failing one
/// `failed` with its position, and a new field restarts it, and the new
/// process hears `restarted` with the reason first (#1166). Under cargo it
/// hears nothing.
#[test]
fn a_program_hears_what_each_save_did_and_why_it_restarted() {
    let root = reload_ticker("reload-ticker");
    let main = root.join("src/main.rs");
    let mut run = start(&root, &[("TICKER_STOP", root.join("stop"))]);
    let first_pid = run.until(|line| line.starts_with("pid "));
    run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 2));
    let seen = run.seen.join("\n");
    assert!(
        !seen.contains("outcome "),
        "a first start hears no restart:\n{seen}"
    );
    assert!(!seen.contains("no reload outcomes"), "{seen}");

    edit(&main, "Scale { factor: 1 }", "Scale { factor: 100 }");
    run.until(|line| line == "outcome swapped");
    let position = |wanted: &str| run.seen.iter().position(|line| line == wanted);
    if let Some(queued) = position("outcome queued") {
        assert!(
            queued < position("outcome swapped").unwrap(),
            "{}",
            run.seen.join("\n")
        );
    }

    // The program keeps running the last good code.
    edit(&main, "tick * scale.factor", "tick * scale.missing");
    let failed = run.until(|line| line.starts_with("outcome failed at "));
    let at = failed.strip_prefix("outcome failed at ").unwrap();
    let canonical = main.canonicalize().unwrap();
    assert!(
        [&canonical, &main]
            .iter()
            .any(|file| at.starts_with(&format!("{}:11:", file.display()))),
        "{failed}\n{}",
        run.seen.join("\n")
    );
    assert!(failed.contains("missing"), "{failed}");
    edit(&main, "tick * scale.missing", "tick * scale.factor");
    run.until(|line| line == "outcome swapped");

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
    run.until(|line| line.starts_with("pid ") && *line != first_pid);
    let restarted = run.until(|line| line.starts_with("outcome "));
    assert_eq!(
        restarted,
        "outcome restarted: `Scale` changed its fields",
        "{}",
        run.seen.join("\n")
    );

    fs::write(root.join("stop"), "").unwrap();
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));

    let output = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env("TICKER_STOP", root.join("stop"))
        .env_remove("RAWR_RELOAD_EVENTS")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.lines().any(|line| line == "no reload outcomes"),
        "{stdout}"
    );
}
