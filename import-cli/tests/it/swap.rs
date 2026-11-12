//! A marked `main` that hands control to a native loop keeps running across
//! a save (D553): the loop's next call into script code runs the new body,
//! in the same process, a new field of a script type no live value holds
//! swaps in too, and an edit to `main` restarts the program with a one-line
//! reason. An edit to the native loop builds a new runner while the old
//! program runs. Build the driver first, as for `islands`.

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

    // A new field, in one write with a body edit that says the program
    // runs it: no value of `Scale` outlives a call, so it swaps in too
    // (#1746).
    let text = fs::read_to_string(&main).unwrap();
    fs::write(
        &main,
        text.replace(
            "    factor: i64,\n}",
            "    factor: i64,\n    name: String,\n}",
        )
        .replace(
            "Scale { factor: 100 }",
            "Scale { factor: 200, name: String::new() }",
        ),
    )
    .unwrap();
    let swapped = run.until(|line| parse(line).is_some_and(|(tick, value)| value == tick * 200));
    let (grown, _) = parse(&swapped).unwrap();
    let seen = run.seen.join("\n");
    assert!(grown > tick, "{seen}");
    assert!(!seen.contains("restarting"), "{seen}");

    // `main` ran once: an edit to it restarts the program.
    edit(
        &main,
        "ticker::run(step, 3000);",
        "ticker::run(step, 2999);",
    );
    run.until(|line| parse(line) == Some((1, 200)));
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

/// An edit to the native loop a marked `main` calls needs a new runner:
/// the old program keeps ticking while it builds, ends before the new
/// `main` runs, so the lock it held is free, and the new one starts in the
/// same run. A new runner that fails to build leaves the program running
/// and says why (#1810).
#[test]
fn a_marked_main_keeps_running_while_its_new_runner_builds() {
    let root = crate::support::fresh("marked-ticker-rebuild");
    copy(Path::new(FIXTURE), &root);
    let ticker = root.join("ticker/src/lib.rs");
    let mut run = start(
        &root,
        &[
            ("TICKER_STOP", root.join("stop")),
            ("TICKER_LOCK", root.join("lock")),
        ],
    );
    run.until(|line| line == "lock held");
    let pid = run.until(|line| line.starts_with("pid "));
    run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 2));

    // The new loop says each tick was rebuilt.
    edit(
        &ticker,
        "println!(\"{}\", step(tick));",
        "println!(\"{} rebuilt\", step(tick));",
    );
    let next = run.until(|line| line.starts_with("pid ") && *line != pid);
    let rebuilt = |line: &str| {
        line.strip_suffix(" rebuilt")
            .and_then(parse)
            .map(|(tick, _)| tick)
    };
    run.until(|line| rebuilt(line).is_some());
    let seen = run.seen.clone();
    let text = seen.join("\n");
    let at = |wanted: &dyn Fn(&str) -> bool| seen.iter().position(|line| wanted(line));
    let restarting = at(&|line| line == "stderr: rawr: the source changed; restarting the program")
        .unwrap_or_else(|| panic!("no restart line:\n{text}"));
    let started = at(&|line| line == next).unwrap();
    // The old program ticked on, unbroken, as the new runner built.
    let old: Vec<i64> = seen[..started]
        .iter()
        .filter_map(|line| parse(line).map(|(tick, _)| tick))
        .collect();
    assert!(old.windows(2).all(|pair| pair[1] == pair[0] + 1), "{text}");
    assert!(
        seen[restarting..started]
            .iter()
            .any(|line| parse(line).is_some()),
        "the old program stopped before its runner built:\n{text}"
    );
    // It ended before the new `main` ran: the new one got its lock, and no
    // old tick follows.
    let locked: Vec<usize> = (0..seen.len())
        .filter(|&index| seen[index] == "lock held")
        .collect();
    assert_eq!(locked.len(), 2, "{text}");
    assert!(locked[1] < started, "{text}");
    assert!(
        seen[locked[1]..].iter().all(|line| parse(line).is_none()),
        "{text}"
    );
    assert!(!text.contains("lock busy"), "{text}");

    // A symbol no library defines checks and fails to link: the program
    // keeps running the last build, and says why.
    edit(
        &ticker,
        "    println!(\"pid {}\", std::process::id());",
        "    unsafe extern \"C\" {\n        fn rawr_ticker_missing();\n    }\n    \
         unsafe { rawr_ticker_missing() };\n    println!(\"pid {}\", std::process::id());",
    );
    let failed = "the save failed; the last good program keeps running";
    let mut last = 0;
    while !run.seen.iter().any(|line| line.contains(failed)) {
        last = rebuilt(&run.until(|line| rebuilt(line).is_some())).unwrap();
    }
    let after = rebuilt(&run.until(|line| rebuilt(line).is_some())).unwrap();
    let text = run.seen.join("\n");
    assert!(after > last, "{text}");
    assert_eq!(
        run.seen
            .iter()
            .filter(|line| line.starts_with("pid "))
            .count(),
        2,
        "{text}"
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
/// `failed` with its position, a new field of a type no live value holds
/// `swapped` (#1746), and an edit to `main` restarts it, and the new
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

    // A new field of a type no live value holds swaps in (#1746).
    let text = fs::read_to_string(&main).unwrap();
    fs::write(
        &main,
        text.replace(
            "    factor: i64,\n}",
            "    factor: i64,\n    name: String,\n}",
        )
        .replace(
            "Scale { factor: 100 }",
            "Scale { factor: 100, name: String::new() }",
        ),
    )
    .unwrap();
    run.until(|line| line == "outcome swapped");

    edit(
        &main,
        "ticker::run(step, 3000);",
        "ticker::run(step, 2999);",
    );
    run.until(|line| line.starts_with("pid ") && *line != first_pid);
    let restarted = run.until(|line| line.starts_with("outcome "));
    assert_eq!(
        restarted,
        "outcome restarted: `main` changed",
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
