//! A program whose `main` is native Rust and calls a marked module (D501),
//! run by the real `rawr-import` binary while its marked file is edited: a
//! body edit reaches the running program at its next call, and a struct's
//! new field restarts it with a one-line reason. Build the driver first, as
//! for `plain`.

use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::support::{copy, start, Started};

/// The ticking fixture, whose run ends at once when told to stop by a file
/// that is there.
static ISLANDS: Started = Started::new("islands", &[("ISLANDS_STOP", "Cargo.toml")]);

/// The theme fixture, whose run prints and ends.
static THEME: Started = Started::new("islands-theme", &[]);

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
    let root = ISLANDS.project();
    let physics = root.join("src/physics.rs");
    let mut run = start(&root, &[("ISLANDS_STOP", root.join("stop"))]);

    // The first run builds the runner.
    let first = run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 3));
    assert_eq!(parse(&first), Some((3, 3)));

    // A body edit: the running native loop keeps its count, and its next
    // calls run the new code.
    edit(&physics, "const SCALE: i64 = 1;", "const SCALE: i64 = 100;");
    let swapped = run.until(|line| parse(line).is_some_and(|(tick, value)| value == tick * 100));
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
    let restarted = run.until(|line| parse(line) == Some((1, 100)));
    assert_eq!(restarted, "1 value 100");
    assert!(
        run.seen.iter().any(|line| line
            == "stderr: rawr: `physics::Counter` changed its fields; restarting the program"),
        "{}",
        run.seen.join("\n")
    );

    fs::write(root.join("stop"), "").unwrap();
    let status = run.wait();
    assert!(status.success(), "{status}\n{}", run.seen.join("\n"));
}

/// A private field on a type native code holds builds natively once, with
/// the preparation that found it (#1761): the save prepares again, says
/// why, builds the new runner from that preparation and starts the
/// program from it, with no second preparation, check or start. The old
/// program ticks on while the new runner builds.
#[test]
fn a_held_type_s_private_field_builds_its_runner_once_from_the_save_s_preparation() {
    let root = ISLANDS.project();
    let physics = root.join("src/physics.rs");
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", root.join("stop")),
            ("RAWR_VERBOSE", "1".into()),
        ],
    );
    run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 3));
    let saved = run.seen.len();
    // One write: the field, and the label's scale, which says the new
    // program runs the save.
    let text = fs::read_to_string(&physics).unwrap();
    fs::write(
        &physics,
        text.replace(
            "    pub ticks: i64,\n}",
            "    pub ticks: i64,\n    seen: i64,\n}",
        )
        .replace("Counter { ticks: 0 }", "Counter { ticks: 0, seen: 0 }")
        .replace("const SCALE: i64 = 1;", "const SCALE: i64 = 100;"),
    )
    .unwrap();
    run.until(|line| parse(line) == Some((1, 100)));
    fs::write(root.join("stop"), "").unwrap();
    let status = run.wait();
    let seen = run.seen.join("\n");
    assert!(status.success(), "{status}\n{seen}");
    let after = &run.seen[saved..];
    let said = |text: &str| after.iter().filter(|line| line.contains(text)).count();
    assert_eq!(
        said("rawr: preparing again: `physics::Counter` changed its fields"),
        1,
        "{seen}"
    );
    assert_eq!(said("restarting the program"), 1, "{seen}");
    // The save's own preparation is the only one: no new session starts,
    // prepares or loads the program again.
    assert_eq!(said("rawr-import: a start ran"), 0, "{seen}");
    assert_eq!(
        said("rawr-import: a new session's load parsed"),
        0,
        "{seen}"
    );
    // The new runner's start walked its read set twice: its storage as its
    // Prepared pin bound, and the whole of it as it observed its inputs
    // (#1760).
    assert_eq!(
        said("rawr-import: a new runner started, read-set walks 2"),
        1,
        "{seen}"
    );
    // The old program ran until the new one replaced it, a tick each
    // 100 ms: what it printed says how long the save took to run.
    let ticked = after
        .iter()
        .filter(|line| parse(line).is_some_and(|(tick, value)| value == tick))
        .count();
    eprintln!("the old program ticked {ticked} times after the save");
}

/// Native code built without a derive's impl doesn't call it: a native
/// type's new derives swap in the running program, native code edited to
/// call one builds again with it, and a derive that goes away builds again
/// and says so (#1737).
#[test]
fn a_pub_type_s_new_derives_swap_and_a_native_use_or_lost_derive_builds_again() {
    let root = ISLANDS.project();
    let physics = root.join("src/physics.rs");
    let mut run = start(&root, &[("ISLANDS_STOP", root.join("stop"))]);
    run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 3));

    // New derives, and a body edit that says the program runs the save,
    // in one write.
    let text = fs::read_to_string(&physics).unwrap();
    fs::write(
        &physics,
        text.replace(
            "pub struct Counter {",
            "#[derive(Clone, Debug)]\npub struct Counter {",
        )
        .replace("const SCALE: i64 = 1;", "const SCALE: i64 = 100;"),
    )
    .unwrap();
    let swapped = run.until(|line| parse(line).is_some_and(|(tick, value)| value == tick * 100));
    let (tick, _) = parse(&swapped).unwrap();
    let seen = run.seen.join("\n");
    assert!(tick > 3, "{seen}");
    assert!(
        run.seen
            .iter()
            .all(|line| !line.contains("preparing again") && !line.contains("restarting")),
        "{seen}"
    );

    // Native code that calls the new impl: a new native build has it.
    edit(
        &root.join("src/main.rs"),
        "physics::Counter::new();",
        "physics::Counter::new().clone();",
    );
    run.until(|line| parse(line) == Some((1, 100)));
    let restarts = run.seen.len();

    // A derive gone: native code may call it, so it builds again.
    edit(&physics, "#[derive(Clone, Debug)]", "#[derive(Clone)]");
    run.until(|line| parse(line) == Some((1, 100)));
    let seen = run.seen.join("\n");
    assert!(
        run.seen[restarts..].iter().any(|line| line
            == "stderr: rawr: `physics::Counter` no longer derives Debug; restarting the program"),
        "{seen}"
    );

    fs::write(root.join("stop"), "").unwrap();
    let status = run.wait();
    assert!(status.success(), "{status}\n{}", run.seen.join("\n"));
}

/// Native code built without an item never names it: a new pub struct
/// and a new pub fn swap in the running program (#1741), and once a native
/// build has them, taking them away swaps too, as the build's check
/// recorded that native code names neither (D935.3).
#[test]
fn a_new_pub_item_swaps_and_a_lost_one_native_code_doesn_t_name_swaps() {
    let root = ISLANDS.project();
    let physics = root.join("src/physics.rs");
    let mut run = start(&root, &[("ISLANDS_STOP", root.join("stop"))]);
    run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 3));

    // New items marked code uses, and a body edit that says the program
    // runs the save, in one write.
    let text = fs::read_to_string(&physics).unwrap();
    fs::write(
        &physics,
        text.replace("const SCALE: i64 = 1;", "const SCALE: i64 = 100;")
            .replace(
                "counter.ticks * SCALE)",
                "counter.ticks * SCALE + inset().radius as i64 - 1)",
            )
            + "\n#[derive(Debug)]\npub struct Inset {\n    pub radius: f32,\n}\n\n\
               pub fn inset() -> Inset {\n    Inset { radius: 1.0 }\n}\n",
    )
    .unwrap();
    let swapped = run.until(|line| parse(line).is_some_and(|(tick, value)| value == tick * 100));
    let (tick, _) = parse(&swapped).unwrap();
    let seen = run.seen.join("\n");
    assert!(tick > 3, "{seen}");
    assert!(
        run.seen
            .iter()
            .all(|line| !line.contains("preparing again") && !line.contains("restarting")),
        "{seen}"
    );

    // A native edit that names neither: its new native build has them.
    edit(
        &root.join("src/main.rs"),
        "from_millis(100)",
        "from_millis(99)",
    );
    run.until(|line| parse(line) == Some((1, 100)));
    let restarts = run.seen.len();

    // Both gone, and a body edit that says the program runs the save:
    // native code names neither, so it keeps running.
    let text = fs::read_to_string(&physics).unwrap();
    let (kept, _) = text.split_once("\n#[derive(Debug)]").unwrap();
    fs::write(
        &physics,
        kept.replace(" + inset().radius as i64 - 1)", ")")
            .replace("const SCALE: i64 = 100;", "const SCALE: i64 = 10;")
            + "\n",
    )
    .unwrap();
    let swapped = run.until(|line| parse(line).is_some_and(|(tick, value)| value == tick * 10));
    let seen = run.seen.join("\n");
    assert!(parse(&swapped).is_some_and(|(tick, _)| tick > 1), "{seen}");
    assert!(
        run.seen[restarts..]
            .iter()
            .all(|line| !line.contains("preparing again") && !line.contains("restarting")),
        "{seen}"
    );

    fs::write(root.join("stop"), "").unwrap();
    let status = run.wait();
    assert!(status.success(), "{status}\n{}", run.seen.join("\n"));
}

/// A pub item native code names is recorded by the native build's check:
/// taking it away prepares a new native build, and the line names it, so
/// rustc says what native code still names (D935.3).
#[test]
fn removing_a_pub_item_native_code_names_builds_again_naming_it() {
    let root = ISLANDS.project();
    let physics = root.join("src/physics.rs");
    let events = root.join("events.jsonl");
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", root.join("stop")),
            ("RAWR_MCP_EVENTS", events.clone()),
        ],
    );
    run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 3));

    let text = fs::read_to_string(&physics).unwrap();
    let (kept, _) = text.split_once("\npub fn label").unwrap();
    fs::write(&physics, format!("{kept}\n")).unwrap();
    // The new native build fails where native code still names it, as
    // rustc says, and the last good program keeps running.
    let told = run.told(&events, |text| text.contains("\"event\":\"failed\""));
    assert!(told.contains("E0425"), "{told}");
    run.child.kill().unwrap();
    run.wait();
    let seen = run.seen.join("\n");
    assert!(
        run.seen.iter().any(|line| line
            == "stderr: rawr: preparing again: native code can no longer see `physics::label`"),
        "{seen}"
    );
    assert!(!seen.contains("restarting"), "{seen}");
}

/// A name native code resolves in a scope that glob-imports a marked
/// module is recorded with the module: a new item of that name shadows
/// what native code found, here the prelude's `Result`, so it builds
/// again, as cargo would (#1744). A new item of another name swaps.
#[test]
fn a_new_name_a_glob_import_of_a_marked_module_shadows_builds_again() {
    let root = ISLANDS.project();
    let physics = root.join("src/physics.rs");
    let main = root.join("src/main.rs");
    edit(&main, "mod physics;\n", "mod physics;\nuse physics::*;\n");
    edit(
        &main,
        "fn main() {",
        "fn ticks(counter: &Counter) -> Result<i64, String> {\n    \
         Ok(counter.ticks)\n}\n\nfn main() {",
    );
    edit(
        &main,
        "counter.step();",
        "counter.step();\n        let _ = ticks(&counter);",
    );
    let mut run = start(&root, &[("ISLANDS_STOP", root.join("stop"))]);
    run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 3));

    // Another name: in place.
    let text = fs::read_to_string(&physics).unwrap();
    fs::write(
        &physics,
        text.replace("const SCALE: i64 = 1;", "const SCALE: i64 = 100;")
            + "\npub type Outcome = std::result::Result<i64, String>;\n",
    )
    .unwrap();
    let swapped = run.until(|line| parse(line).is_some_and(|(tick, value)| value == tick * 100));
    assert!(parse(&swapped).is_some_and(|(tick, _)| tick > 3));
    let restarts = run.seen.len();

    // The prelude's name: native code would find the module's now.
    let text = fs::read_to_string(&physics).unwrap();
    fs::write(
        &physics,
        text + "\npub type Result<T, E = String> = std::result::Result<T, E>;\n",
    )
    .unwrap();
    run.until(|line| parse(line) == Some((1, 100)));
    let seen = run.seen.join("\n");
    assert!(
        run.seen[..restarts]
            .iter()
            .all(|line| !line.contains("preparing again") && !line.contains("restarting")),
        "{seen}"
    );
    assert!(
        run.seen[restarts..].iter().any(|line| line
            == "stderr: rawr: native code now finds `physics::Result` through a glob import; \
                restarting the program"),
        "{seen}"
    );

    fs::write(root.join("stop"), "").unwrap();
    let status = run.wait();
    assert!(status.success(), "{status}\n{seen}");
}

/// A supervised run whose marked file fails its check says `failed` and
/// waits for a save. A wait for the program's output ends there, with the
/// error rustc gives, instead of waiting for that save (#1585).
#[test]
fn a_wait_ends_with_the_error_when_the_run_fails_its_check() {
    let root = ISLANDS.project();
    let events = root.join("events.jsonl");
    edit(
        &root.join("src/physics.rs"),
        "const SCALE: i64 = 1;",
        "const SCALE: i64 = \"one\";",
    );
    let mut run = start(&root, &[("RAWR_MCP_EVENTS", events)]);
    let waited = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run.until(|line| parse(line).is_some())
    }));
    let failure = waited.expect_err("the program ran");
    let failure = failure.downcast_ref::<String>().map_or("", String::as_str);
    assert!(failure.starts_with("the run failed:"), "{failure}");
    assert!(failure.contains("mismatched types"), "{failure}");
}

/// A supervised run whose program returns from `main` says `ended` and
/// ends with the program's status 0, instead of waiting for a save
/// (#1800). A wait for a line the program never printed ends there, with
/// what the run printed.
#[test]
fn a_program_that_returns_from_main_ends_the_run_with_status_0() {
    let root = ISLANDS.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    // The program stops before its first tick.
    fs::write(&stop, "").unwrap();
    let mut run = start(
        &root,
        &[("ISLANDS_STOP", stop), ("RAWR_MCP_EVENTS", events.clone())],
    );
    let waited = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run.until(|line| parse(line).is_some())
    }));
    let failure = waited.expect_err("the program ticked");
    let failure = failure.downcast_ref::<String>().map_or("", String::as_str);
    assert!(failure.contains(" without it\n"), "{failure}");
    let status = run.wait();
    let seen = run.seen.join("\n");
    assert!(status.success(), "{status}\n{seen}");
    let told = fs::read_to_string(&events).unwrap();
    assert!(
        told.contains("{\"event\":\"ended\",\"status\":0}"),
        "{told}"
    );
    assert!(!seen.contains("a save runs it again"), "{seen}");
}

/// A package whose `target` holds a big subtree that git ignores, as a
/// real app's does: the watcher stands for the program's inputs, not the
/// tree, so a save of the marked file still reaches the running program,
/// and writes under `target` while it runs are no save.
#[test]
fn a_save_reaches_a_native_main_beside_a_big_ignored_target() {
    let root = ISLANDS.project();
    fs::write(root.join(".gitignore"), "/target\n").unwrap();
    let big = root.join("target/big");
    for directory in 0..200 {
        let directory = big.join(directory.to_string());
        fs::create_dir_all(&directory).unwrap();
        for file in 0..50 {
            fs::write(directory.join(format!("{file}.o")), "object").unwrap();
        }
    }
    let physics = root.join("src/physics.rs");
    let mut run = start(&root, &[("ISLANDS_STOP", root.join("stop"))]);
    let first = run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 3));
    assert_eq!(parse(&first), Some((3, 3)));

    // Writes under `target`, as a build's, reach no reload.
    for file in 0..200 {
        fs::write(big.join(format!("0/{file}.d")), "written").unwrap();
    }
    edit(&physics, "const SCALE: i64 = 1;", "const SCALE: i64 = 100;");
    let swapped = run.until(|line| parse(line).is_some_and(|(tick, value)| value == tick * 100));
    let (tick, _) = parse(&swapped).unwrap();
    assert!(tick > 3, "{}", run.seen.join("\n"));
    assert!(
        !run.seen
            .iter()
            .any(|line| line.contains("restarting") || line.contains("file watcher")),
        "{}",
        run.seen.join("\n")
    );
    fs::write(root.join("stop"), "").unwrap();
    let status = run.wait();
    assert!(status.success(), "{status}\n{}", run.seen.join("\n"));
}

/// An idle program is a healthy one (#1165): a native `main` waiting on its
/// events makes no call into marked code, and its runner may not run at
/// all for a while (an occluded window that macOS naps on a busy machine;
/// here a SIGSTOP). The owner's periodic exchanges and a save wait for the
/// runner's reply instead of stopping it, and the program's next call into
/// marked code runs the save.
#[cfg(unix)]
#[test]
fn an_idle_native_main_outlives_a_stop_and_takes_a_save_at_its_next_call() {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    const IDLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-idle");
    // How long the runner can't answer.
    const STOPPED: Duration = Duration::from_secs(12);
    let root = crate::support::fresh("islands-idle");
    let go = root.join("go");
    let events = root.join("events.jsonl");
    let _ = fs::remove_file(&go);
    let _ = fs::remove_file(&events);
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(IDLE), &root);
    let physics = root.join("src/physics.rs");
    let mut run = start(
        &root,
        &[("IDLE_GO", go.clone()), ("RAWR_MCP_EVENTS", events.clone())],
    );
    let pid = run.until(|line| line.starts_with("pid "));
    let pid = Pid::from_raw(pid["pid ".len()..].parse().unwrap());
    run.until(|line| line == "value 1");
    // A runner the owner stopped is gone, and its output says why.
    let pause = |run: &mut crate::support::Run| {
        kill(pid, Signal::SIGSTOP).unwrap();
        std::thread::sleep(STOPPED);
        assert!(
            kill(pid, Signal::SIGCONT).is_ok(),
            "the owner stopped the runner:\n{}",
            run.seen.join("\n")
        );
    };

    // The owner polls an idle program while it can't answer.
    pause(&mut run);
    // A save reaches it while it can't answer either.
    edit(&physics, "const SCALE: i64 = 1;", "const SCALE: i64 = 100;");
    pause(&mut run);
    run.told(&events, |text| text.contains("\"queued\""));
    assert!(
        run.child.try_wait().unwrap().is_none(),
        "{}",
        run.seen.join("\n")
    );

    fs::write(&go, "").unwrap();
    run.until(|line| line == "value 100");
    let seen = run.seen.join("\n");
    assert!(!seen.contains("restarting"), "{seen}");
}

/// `swapped` is in the events file when the program's call that admitted
/// the save runs, not at some later event: a `rawr mcp` waiting on it
/// hears it once the swap happens. The program stays idle after its save
/// is queued, and only its next call into marked code admits it.
#[test]
fn swapped_is_told_before_the_call_that_admits_the_save_runs() {
    const ADMIT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-admit");
    let root = crate::support::fresh("islands-admit");
    let go = root.join("go");
    let stop = root.join("stop");
    let events = root.join("events.jsonl");
    for file in [&go, &stop, &events] {
        let _ = fs::remove_file(file);
    }
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(ADMIT), &root);
    let mut run = start(
        &root,
        &[
            ("IDLE_GO", go.clone()),
            ("IDLE_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
        ],
    );
    run.until(|line| line == "value 1");
    edit(
        &root.join("src/physics.rs"),
        "const SCALE: i64 = 1;",
        "const SCALE: i64 = 100;",
    );
    let queued = run.told(&events, |text| text.contains("\"queued\""));
    assert!(!queued.contains("\"swapped\""), "{queued}");

    fs::write(&go, "").unwrap();
    run.until(|line| line == "value 100");
    let told = fs::read_to_string(&events).unwrap();
    let names: Vec<&str> = told
        .lines()
        .filter_map(|line| {
            let event = line.split_once("\"event\":\"")?.1;
            event.split_once('"').map(|(name, _)| name)
        })
        .collect();
    let queued = names.iter().position(|name| *name == "queued");
    let swapped = names.iter().position(|name| *name == "swapped");
    assert!(
        queued.is_some() && swapped > queued,
        "{told}\n{}",
        run.seen.join("\n")
    );

    // A program that returns from `main` ends the run with it (#1800).
    fs::write(&stop, "").unwrap();
    let ended = run.told(&events, |text| text.contains("\"ended\""));
    assert!(
        ended.contains("{\"event\":\"ended\",\"status\":0}"),
        "{ended}"
    );
    let seen = run.seen.join("\n");
    assert!(!seen.contains("restarting"), "{seen}");
}

/// An idle event loop draws a save at once (#1479): a native `main` whose
/// winit loop waits for its next event makes no call into marked code, so
/// rawr asks the program's windows to redraw when it queues a generation.
/// The frame that follows calls into marked code, which admits the save,
/// and draws with it, without any input.
#[cfg(target_os = "macos")]
#[test]
fn an_idle_event_loop_redraws_with_a_save_without_input() {
    const REDRAW: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-redraw");
    let root = crate::support::fresh("islands-redraw");
    let events = root.join("events.jsonl");
    let _ = fs::remove_file(&events);
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(REDRAW), &root);
    let mut run = start(&root, &[("RAWR_MCP_EVENTS", events.clone())]);
    run.until(|line| line == "shade 1");
    let saved = std::time::Instant::now();
    edit(
        &root.join("src/scene.rs"),
        "const SHADE: i64 = 1;",
        "const SHADE: i64 = 2;",
    );
    run.until(|line| line == "shade 2");
    eprintln!("save to the new frame: {:?}", saved.elapsed());
    run.told(&events, |text| text.contains("\"swapped\""));
    let seen = run.seen.join("\n");
    assert!(!seen.contains("restarting"), "{seen}");
}

/// winit's event loop must run on the process's main thread, so a native
/// `main` runs there, and so does the marked code it calls (#1119). A
/// marked function gives back a `Result` of types that cross, and an alias
/// of one whose `Err` is a native type, as `iced::Result` (#1118).
#[test]
fn a_native_main_runs_on_the_process_main_thread() {
    const MAIN_THREAD: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-main-thread"
    );
    let root = crate::support::fresh("islands-main-thread");
    copy(Path::new(MAIN_THREAD), &root);
    let mut run = start(&root, &[]);
    let line = run.until(|line| line.starts_with("main thread "));
    assert_eq!(line, "main thread true", "{}", run.seen.join("\n"));
    let line = run.until(|line| line.starts_with("marked main thread "));
    assert_eq!(line, "marked main thread true", "{}", run.seen.join("\n"));
    run.until(|line| line == "answer 42");
    // The interpreter keeps a `u64` in an `i64`'s bits.
    run.until(|line| line == "wide 13907096814508502785");
    for expected in [
        "checked Ok(3)",
        "checked Err(\"0 is not positive\")",
        "run Ok(())",
        "run Err(Failure(\"no window\"))",
    ] {
        run.until(|line| line == expected);
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// A native `main` reaches marked modules named `import`, `switch` and
/// `import::print` through `crate::`, and a marked struct's method, as
/// `cargo run` does: words the old engine read as keywords are ordinary
/// names, in the files Rawr opens and the Rust it writes.
#[test]
fn a_native_main_calls_marked_modules_named_as_old_engine_keywords() {
    const NAMES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-names");
    let root = crate::support::fresh("islands-names");
    copy(Path::new(NAMES), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&native.stdout), "1 2 20 true\n");
    let mut run = start(&root, &[]);
    run.until(|line| line == "1 2 20 true");
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// Native code gets the native root's own values back from a marked leaf,
/// as an iced app gets its `Message` (#1203): bare, inside an `Option`, a
/// `Vec` and a tuple, and a `ui::Element<'a, Message>` of a local crate
/// whose lifetime the signature names. It passes them in by value and by
/// `&mut`, which writes back (#1139). A marked function makes a crate's
/// value and lends it `&mut` to another, which writes it through the
/// crate's `&mut` method, called from the main thread and from another,
/// whose call has an arena of its own, as inboard's avatar is made on a
/// tokio worker. `rawr run` prints what `cargo run` prints.
#[test]
fn a_native_main_takes_and_gives_its_own_values_to_a_marked_leaf() {
    const NATIVES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-natives"
    );
    let root = crate::support::fresh("islands-natives");
    copy(Path::new(NATIVES), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&native.stdout).into_owned();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert!(
        printed.ends_with("view [press 7] -> Some(Pressed(7))\n"),
        "{printed}"
    );
    let mut run = start(&root, &[]);
    for expected in printed.lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// A marked leaf module of an existing app names the items of the native
/// crate root it lives in, as rustc lets a descendant: the root's message
/// enum and its variants, a type alias, a constant, a helper function and
/// a `use` re-export, through `crate::` and `super::` (#1185). `rawr run`
/// prints what `cargo run` prints, and the crate builds unchanged.
#[test]
fn a_marked_module_names_the_items_of_its_native_crate_root() {
    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-root");
    const PRINTED: [&str; 5] = [
        "Page(0, Open(0))",
        "Noop",
        "Page(2, Open(20))",
        "narrow x10",
        "wide x10",
    ];
    let root = crate::support::fresh("islands-root");
    copy(Path::new(ROOT), &root);
    let cargo = |arguments: &[&str]| {
        std::process::Command::new("cargo")
            .args(arguments)
            .current_dir(&root)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .output()
            .unwrap()
    };
    let native = cargo(&["run", "--quiet"]);
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&native.stdout),
        format!("{}\n", PRINTED.join("\n"))
    );
    let mut run = start(&root, &[]);
    for line in PRINTED {
        let seen = run.until(|_| true);
        assert_eq!(seen, line, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
    let release = cargo(&["build", "--release", "--quiet"]);
    assert!(
        release.status.success(),
        "{}",
        String::from_utf8_lossy(&release.stderr)
    );
}

/// A marked leaf module names the items of a native module that is not the
/// crate root, as rustc lets any module of the crate: its trait, function,
/// constant and a nested module's alias and constant, through `crate::`,
/// through `use crate::page;` and through `use crate::page::Page;`. A marked
/// function takes and gives the native trait's objects (`Box<dyn
/// page::Page>`), in its signature and in its body's annotations, and
/// native code calls the trait's methods on what it gets back (#1263).
/// Marked functions take and give the nested alias of `usize`, by value
/// and by a shared borrow, as `usize` crosses, and take an alias of a
/// native struct as the struct crosses (#1272). A `&mut` of the alias
/// crosses as `&mut usize` does, copied in and back, beside a `&mut` of a
/// native struct, which is lent (#1297).
/// `rawr check` passes as `cargo check` does, and `rawr run` prints what
/// `cargo run` prints.
#[test]
fn a_marked_module_names_the_items_of_a_native_module() {
    const MODULE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-native-module"
    );
    const PRINTED: [&str; 8] = [
        "home",
        "home",
        "about",
        "31",
        "41",
        "10",
        "home",
        "10 17 140",
    ];
    let root = crate::support::fresh("islands-native-module");
    copy(Path::new(MODULE), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&native.stdout),
        format!("{}\n", PRINTED.join("\n"))
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(&root, &[]);
    for line in PRINTED {
        let seen = run.until(|_| true);
        assert_eq!(seen, line, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// A marked module names a workspace crate's aliases through a module its
/// `use` binds (`use model::extension;`), as inboard's `home` does: an
/// alias of another crate's type in a re-exported module (`pub type
/// Update = check::Update;`) in a parameter, a method's parameter and a
/// `let`, an alias of a `Vec` of that type in a field, and aliases of std
/// types, a crate's (`type Names = Vec<String>`) and a native module's
/// (#694, #1202). Nothing names the aliases by their crate's path, so each
/// is demanded through the module the `use` binds. `rawr check` passes as
/// `cargo check` does, and `rawr run` prints what `cargo run` prints.
#[test]
fn a_marked_module_names_crate_aliases_through_a_used_module() {
    const ALIASES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-aliases"
    );
    const PRINTED: [&str; 2] = ["mail 2 / 7 of 2 / mail,notes / inbox,home", "4"];
    let root = crate::support::fresh("islands-aliases");
    copy(Path::new(ALIASES), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&native.stdout),
        format!("{}\n", PRINTED.join("\n"))
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(&root, &[]);
    for line in PRINTED {
        let seen = run.until(|_| true);
        assert_eq!(seen, line, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// Any file is markable (#1467): a marked module two native modules below
/// the crate root, as `workspace::screen::home`, names the root's private
/// `Message`, its parent's private constant and its private sibling's
/// function and `pub(super)` constant through `super::`, and its native
/// child's function through `self::`. Native code calls its functions from
/// its native sibling, its native grandparent, its native child and the
/// root, and builds its `State` by a struct literal. `cargo run`, `rawr
/// check` and `rawr run` agree, and a body edit swaps at its next call.
#[test]
fn a_marked_module_below_native_modules_runs_and_swaps_a_body_edit() {
    const NESTED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-nested");
    const PRINTED: [&str; 7] = [
        "home 2 20",
        "Opened(20)",
        "Closed",
        "about home 2 20",
        "summary home 3 30",
        "wizard home 2 20",
        "7",
    ];
    let root = crate::support::fresh("islands-nested");
    copy(Path::new(NESTED), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("ISLANDS_STOP")
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&native.stdout),
        format!("{}\n", PRINTED.join("\n"))
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(&root, &[("ISLANDS_STOP", root.join("stop"))]);
    for line in PRINTED {
        let seen = run.until(|_| true);
        assert_eq!(seen, line, "{}", run.seen.join("\n"));
    }
    let tick = |line: &str| -> Option<(usize, usize)> {
        let (tick, value) = line.strip_prefix("tick ")?.split_once(" value ")?;
        Some((tick.parse().ok()?, value.parse().ok()?))
    };
    let first = run.until(|line| tick(line).is_some_and(|(tick, _)| tick >= 3));
    assert_eq!(tick(&first), Some((3, 3)));
    edit(
        &root.join("src/workspace/screen/home.rs"),
        "const FACTOR: usize = 1;",
        "const FACTOR: usize = 100;",
    );
    let swapped = run.until(|line| tick(line).is_some_and(|(tick, value)| value == tick * 100));
    assert!(
        tick(&swapped).is_some_and(|(tick, _)| tick > 3),
        "{}",
        run.seen.join("\n")
    );
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );
    fs::write(root.join("stop"), "").unwrap();
    let status = run.wait();
    assert!(status.success(), "{status}\n{}", run.seen.join("\n"));
}

/// A native root's `mod` declarations are read under the `cfg`s Cargo
/// builds the package with, from `rustc --print cfg` (#1337): the marked
/// module under this host's `target_os` loads, the one under its `not`,
/// which would not compile, is never read, and a `#[path]` applied by a
/// `#[cfg_attr(unix, ..)]` is Rust's choice, so the broken marked file the
/// module tree would name is never read either. `cargo run`, `rawr check`
/// and `rawr run` print the same.
#[test]
fn a_native_root_s_modules_load_as_cargo_configures_them_for_the_target() {
    const CFG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-cfg");
    let printed = ["42", if cfg!(unix) { "unix" } else { "other" }];
    let root = crate::support::fresh("islands-cfg");
    copy(Path::new(CFG), &root);
    edit(
        &root.join("src/main.rs"),
        "\"OS\"",
        &format!("{:?}", std::env::consts::OS),
    );
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&native.stdout),
        format!("{}\n", printed.join("\n"))
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(&root, &[]);
    for line in printed {
        let seen = run.until(|_| true);
        assert_eq!(seen, line, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// A marked type becomes a native trait's object (#1264): a marked leaf
/// writes `impl page::Page for BadgePage` and `new() -> Box<dyn
/// page::Page>`, and native code keeps the boxes beside a native page in a
/// `Vec<Box<dyn Page>>`, calls their `&self` methods and their `&mut self`
/// `update`, whose changes a later call reads, with a native `Message` in
/// and a native `Task` out, and drops one. `Page: Debug + PageClone`
/// (#1301): the box prints by the page's derived `Debug`, and clones by
/// its derived `Clone` into a page of its own. A `&self` `summary` calls
/// a native helper that calls `title()` on the same object while it runs
/// (#1302). `label(&self) -> &str` and `items(&self) -> &[String]` give
/// borrows of the page that native code reads after their calls, and an
/// `update` once they are gone sees the page whole; an `update` while one
/// lives is E0502 (#1305). A `&str` borrow is the program's own text, read
/// where it is however often (#1322), and `draw(&self) -> Drawn<'_>`, a
/// native type that borrows the page as `view(&self) -> Element<'_,
/// Message>` does, borrows that same text, its pins the page's own and let
/// go at its next `update` (#1321). A marked page lent to a native
/// function as a `&dyn page::Page`, written `&page as &dyn page::Page` and
/// `&page`, and given as an `Rc<dyn page::Page>` and an `Arc<dyn
/// page::Page>`, runs its impl while marked code keeps it (#1304). `rawr
/// check` passes as `cargo check` does, and `rawr run` prints what `cargo
/// run` prints.
#[test]
fn a_marked_type_becomes_a_native_trait_object() {
    const OBJECTS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-objects"
    );
    let root = crate::support::fresh("islands-objects");
    copy(Path::new(OBJECTS), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&native.stdout).into_owned();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert!(printed.contains("badge 1: clicks 8\n"), "{printed}");
    assert!(
        printed.contains(
            "BadgePage { page_index: 2, count: 1, label: \"count\", names: [\"clicks\"] } \
             BadgePage { page_index: 2, count: 11, label: \"count\", names: [\"clicks\"] }\n"
        ),
        "{printed}"
    );
    // `label` and `items` borrow the page: native code reads them after
    // their calls, and the page's next `update` sees the change (#1305).
    assert!(
        printed.contains("count [\"clicks\"] badge 2\nheld [\"clicks\", \"count\"]\n"),
        "{printed}"
    );
    assert!(printed.contains("held 1 sees badge 2\n"), "{printed}");
    // A `label` read many times is the page's own text each time, and a
    // `draw(&self) -> Drawn<'_>` the marked impl builds borrows that same
    // text, drawn and renamed in a loop (#1322, #1321).
    assert!(
        printed.contains("held true\nheld true\nheld true\nheld true\nname 99 true home\n"),
        "{printed}"
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(&root, &[]);
    for expected in printed.lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
    drop(run);
    // An `update` while a borrowed result still lives is rustc's E0502
    // under `rawr check`, as under `cargo check`: the stand-in's methods
    // carry the trait's lifetimes (#1305).
    edit(
        &root.join("src/main.rs"),
        "    println!(\"{label} {items:?} {}\", pages[1].title());\n    \
         pages[1].update(Message::Rename(\"held\".to_string()));\n",
        "    pages[1].update(Message::Rename(\"held\".to_string()));\n    \
         println!(\"{label} {items:?} {}\", pages[1].title());\n",
    );
    let mut cargo = std::process::Command::new("cargo");
    cargo.arg("check").current_dir(&root);
    let mut rawr = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"));
    rawr.arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home());
    for (tool, command) in [("cargo check", &mut cargo), ("rawr check", &mut rawr)] {
        let checked = command
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("RAWR_VERBOSE")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&checked.stderr);
        assert!(!checked.status.success(), "{tool}: {stderr}");
        assert!(stderr.contains("error[E0502]"), "{tool}: {stderr}");
    }
}

/// A private marked type whose value the running program holds, inside the
/// page native code keeps as a `Box<dyn Page>`, though native code never
/// names it (#1746): a body edit swaps in and the page keeps its count, but
/// the type's new field, and its removal, restart the program with their
/// reason, which says a live value holds it. Neither restart needs a
/// native build: a new process of the running runner starts the program
/// from the save, with no check, and the page counts from the start.
#[test]
fn a_held_private_type_s_new_field_or_removal_restarts_saying_a_live_value_holds_it() {
    const OBJECTS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-objects"
    );
    const STEP: &str = "/// How far a spare page counts at each update.\n\
                        #[derive(Clone, Debug)]\nstruct Step {\n    by: usize,\n    \
                        twice: bool,\n}\n\n";
    let root = crate::support::fresh("islands-objects-held");
    let stop = root.join("stop");
    let _ = fs::remove_file(&stop);
    copy(Path::new(OBJECTS), &root);
    let badge = root.join("src/component/badge.rs");
    let mut run = start(
        &root,
        &[("ISLANDS_STOP", stop.clone()), ("RAWR_VERBOSE", "1".into())],
    );
    // `tick N {word} M`, for the word the page's view says.
    let ticks = |word: &'static str| {
        move |line: &str| -> Option<(usize, usize)> {
            let (tick, rest) = line.strip_prefix("tick ")?.split_once(' ')?;
            let count = rest.strip_prefix(word)?.strip_prefix(' ')?;
            Some((tick.parse().ok()?, count.parse().ok()?))
        }
    };
    let first = ticks("spare");
    run.until(|line| first(line).is_some_and(|(tick, _)| tick >= 3));
    // Each save is one write: the changes, and the view's word, which says
    // the program runs the save.
    let said = std::cell::RefCell::new("spare".to_owned());
    let save = |changes: &[(&str, &str)], word: &str| {
        let view = |word: &str| format!("format!(\"{word} {{}}\", self.ticks)");
        let (was, now) = (view(&said.borrow()), view(word));
        let mut text = fs::read_to_string(&badge).unwrap();
        for (from, to) in changes.iter().copied().chain([(&*was, &*now)]) {
            assert!(text.contains(from), "{from}\n{text}");
            text = text.replace(from, to);
        }
        fs::write(&badge, text).unwrap();
        *said.borrow_mut() = word.to_owned();
    };

    // A body edit: the page native code keeps counts on in the new code.
    save(&[], "kept");
    let kept = ticks("kept");
    let line = run.until(|line| kept(line).is_some());
    let (tick, count) = kept(&line).unwrap();
    let seen = run.seen.join("\n");
    assert!(tick > 3 && count == tick, "{seen}");
    assert!(!seen.contains("restarting"), "{seen}");

    // The step's new field: the kept page's step has the old fields.
    save(
        &[
            ("    by: usize,\n}", "    by: usize,\n    twice: bool,\n}"),
            ("Step { by: 1 }", "Step { by: 1, twice: false }"),
        ],
        "grown",
    );
    let grown = ticks("grown");
    run.until(|line| grown(line) == Some((1, 1)));

    // The step removed: the kept page still holds one.
    save(
        &[
            (STEP, ""),
            ("    step: Step,\n", ""),
            ("self.ticks += self.step.by;", "self.ticks += 1;"),
            ("        step: Step { by: 1, twice: false },\n", ""),
        ],
        "plain",
    );
    let plain = ticks("plain");
    run.until(|line| plain(line) == Some((1, 1)));

    fs::write(&stop, "").unwrap();
    let status = run.wait();
    let seen = run.seen.join("\n");
    assert!(status.success(), "{status}\n{seen}");
    let restarts = run
        .seen
        .iter()
        .filter(|line| line.contains("restarting the program"))
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        restarts,
        [
            "stderr: rawr: `component::badge::Step` changed its fields, but a live value holds \
             it; restarting the program",
            "stderr: rawr: `component::badge::Step` was removed, but a live value holds it; \
             restarting the program",
        ],
        "{seen}"
    );
    assert!(!seen.contains("preparing again"), "{seen}");
    // Each restart started a new runner process with no check: the one
    // start is the program's first.
    let said = |text: &str| run.seen.iter().filter(|line| line.contains(text)).count();
    assert_eq!(
        said("rawr-import: a restart respawned its runner with no check"),
        2,
        "{seen}"
    );
    assert_eq!(said("rawr-import: a start ran"), 1, "{seen}");
}

/// A save whose code panics on the running program's state, as a
/// framework's own check of its hooks can, restarts the program with the
/// save (D553.6). It needs no native build: a new process of the running
/// runner starts the program with no check, and the new program counts
/// from the start, where the save's code doesn't panic.
#[test]
fn a_panic_after_a_swap_restarts_the_program_in_a_new_process_of_its_runner() {
    const OBJECTS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-objects"
    );
    let root = crate::support::fresh("islands-objects-panic");
    let stop = root.join("stop");
    copy(Path::new(OBJECTS), &root);
    let badge = root.join("src/component/badge.rs");
    let mut run = start(
        &root,
        &[("ISLANDS_STOP", stop.clone()), ("RAWR_VERBOSE", "1".into())],
    );
    let ticks = |word: &'static str| {
        move |line: &str| -> Option<(usize, usize)> {
            let (tick, rest) = line.strip_prefix("tick ")?.split_once(' ')?;
            let count = rest.strip_prefix(word)?.strip_prefix(' ')?;
            Some((tick.parse().ok()?, count.parse().ok()?))
        }
    };
    let spare = ticks("spare");
    run.until(|line| spare(line).is_some_and(|(tick, _)| tick >= 40));
    // One write: a check the running page's count fails at once, and the
    // view's word, which says the program runs the save.
    let text = fs::read_to_string(&badge).unwrap();
    let text = text
        .replace(
            "self.ticks += self.step.by;\n",
            "self.ticks += self.step.by;\n        assert!(self.ticks < 40, \"counted past 40\");\n",
        )
        .replace(
            "format!(\"spare {}\", self.ticks)",
            "format!(\"fresh {}\", self.ticks)",
        );
    fs::write(&badge, text).unwrap();
    let fresh = ticks("fresh");
    run.until(|line| fresh(line) == Some((1, 1)));
    fs::write(&stop, "").unwrap();
    let status = run.wait();
    let seen = run.seen.join("\n");
    assert!(status.success(), "{status}\n{seen}");
    let said = |text: &str| run.seen.iter().filter(|line| line.contains(text)).count();
    assert_eq!(
        said("rawr: the program panicked after a save; restarting the program"),
        1,
        "{seen}"
    );
    // The new process holds the old one's pins, and walks its read set
    // once, as it observes its inputs (#1760).
    assert_eq!(
        said("rawr-import: a restart respawned its runner with no check, read-set walks 1"),
        1,
        "{seen}"
    );
    // The program's first start walked its read set twice: its storage
    // as its Prepared pin bound, and the whole of it as it observed.
    assert_eq!(said("rawr-import: a start ran"), 1, "{seen}");
    assert_eq!(said(", read-set walks 2"), 1, "{seen}");
}

/// A marked leaf writes its page as a native module's enum, whose fields
/// are `&'static str`, `Option<&'static str>` and `&'static [Block]`, from
/// literals: a `pub static BLOCKS` native code reads as a Rust item, and
/// functions that give a built block back by value, lend built blocks to
/// a native function, give one to it by value and read `BLOCKS` themselves
/// (#1283). Marked code reads native code's `BLOCKS` where it is: its
/// length, a range of it lent to native code, each block through a
/// pattern and a group's `children`, indexed, walked and lent (#1291). A
/// marked page native code holds as a `Box<dyn Page>` gives back its
/// `&'static Meta` and `&'static [Block]` statics themselves and a `&State`
/// of its own field, lent in place (#1442). Each `&self` call is a shared
/// borrow: while native code holds that `&State` and a preview that
/// borrows the page, `scrollable_id()` clones a field of the lent state,
/// which stays whole (#1445). `rawr check` passes as `cargo check` does,
/// and `rawr run` prints what `cargo run` prints.
/// A marked `theme.rs`, as inboard's, declares native child modules and
/// defines the app's `Theme` over one's own `Theme`, as `enum Theme {
/// Selected(palette::Theme) }`: neither `palette::Theme` nor the crate's
/// `ui::Theme` is the marked one (#1466). Its impls of `Default`, `From`
/// from the native and the crate's theme, `From<Theme>` for the crate's
/// theme, the crate's `Design` and `Base` traits and `From<&Theme>` for the
/// crate's `Settings` are native code's, each function a trampoline, and a
/// native child module implements the crate's `Catalog` for it by matching
/// its variant. A borrow of a theme lends the interpreter its native part
/// where it is, `&mut self` too, and a value moves it, so a run clones a
/// theme only where `cargo run` does, which counts its clones, and a part
/// that isn't `Clone` crosses (#1469). `Theme::colors(&self) ->
/// &palette::Colors`, which native style functions call in a loop, gives
/// `&self.0.colors` where it is, to native code and to the theme's own
/// `Design` impl, and `name(&self) -> &str` its text (#1468). A marked
/// `home`, as inboard's, imports the marked `theme` beside `widget`'s own
/// `text` and names the theme's native child modules through it:
/// `theme::text::SIZE`, and `theme::text::base` and
/// `theme::container::base`, whose `&Theme` is the marked theme, passed to
/// the crate's `style` of a `Label<Theme>`, each its own instance, as each
/// Rust function is its own type. A marked `home::view` native code lends
/// the theme to gives it on by `&Theme`, as inboard's `inbox::view(..,
/// theme)` does: to `theme::text::base`, a native `style::button`, the
/// crate's generic `Label::<Theme>::draw` and `ui::base`, each given native
/// code's own reference, which no clone or copy stands for, and a marked
/// `badge<'a>` gives it to a native function whose `Badge<'a, Theme>`
/// keeps it borrowed as long as native code holds the badge. A native alias
/// `Element<'a, Message>` over the marked `Theme`, re-exported at the crate
/// root as inboard's is, names the marked type in a marked fn's output and
/// in the native `void()` it returns (#1484). A marked `inbox<'a>` maps
/// the element a native `inbox::view(.., theme)` gives over the lent
/// theme, whose `Theme` is spelled through the crate root's re-export, as
/// inboard's `home::State::view` does: both spellings name one type, so
/// the element is one value to `map`. That `view`'s `&str` title is
/// borrowed for its call alone, and a marked `titled<'a>` gives a native
/// fn both a lent `&'a str` and the lent theme, which its element keeps
/// (#1509). A marked `page<'a>` maps the grid element native code builds
/// and gives it by value inside an `Option`, as inboard's
/// `home::State::view` maps its `home_grid`. A marked `home::State::view`,
/// kept by native code, follows
/// inboard's: a native grid element mapped through the crate's generic
/// `Widget::map` and boxed into a `Column<Element>`, the crate's generic
/// `scrollable` and `container` over the marked theme reached through the
/// app's `widget` re-exports, a native style function as a container's
/// style, and, with its wizard open, the wizard's element over the
/// catalog native code gives for the call, stacked over the page with
/// `opaque`; native code maps the element it returns. A marked
/// `caption<'a>(label: &'a str)`, as inboard's `sidebar::subtitle`, gives
/// native code the crate's `Text<'a>`, invariant in its lifetime as iced's
/// is, which borrows native code's own `label` and which native code
/// keeps. A marked `Panel<'a>` of native code's `&'a str` and `Element<'a,
/// Message>`s, as inboard's `Sidebar<'a, Message>`, is built by native code
/// through a chain of marked methods by value and by `&mut`, kept between
/// calls and drawn, and its title is native code's own text (#1557). A
/// marked `Sidebar<'a, Message>` bounds its generic impl's `content` by the
/// crate's `IntoContent<'a, Message>` over the impl's own parameters, and a
/// non-generic marked fn calls it. A
/// marked `sidebar(theme: &Theme) -> Element<'_, Message>` gives the theme
/// native code lends it on to the marked `inbox`, whose native call gets
/// native code's own reference. A marked `sidebar` module follows inboard's
/// `widget/sidebar.rs`: structs of `Box<dyn Iterator<Item = Element>>`,
/// `.chain(&mut self.actions)` and `column(&mut self.content)`, a native
/// crate's `space!()`, a struct update through the crate's own `Default`
/// of a generic struct with `[4, 0].into()` into a crate `Padding`, a
/// blanket `IntoContent<'a, Message>` impl, a dependency's
/// `IntoFragment<'a>`, closures annotated `&Theme` through the crate
/// root's re-export and an `impl Fn(String) -> Message` handed on to
/// `on_input`.
/// A marked `Sidebar<'a, Message>` generic
/// over the message, as inboard's, is made by a marked generic fn with an
/// `impl IntoIterator` argument, built by its generic methods by value,
/// with an `impl Into<Pixels>` and by `&mut self`, read by `&self` and
/// turned into an element, at two of native code's message types, and a
/// marked `From<Group<'a, Message>> for Element<'a, Message>` gives an
/// element by `.into()` (D869). inboard's `sidebar::header(..).content(
/// ..).spacing(4)` keeps native code's elements in a `Box<dyn Iterator>`
/// marked code makes, which native code pulls from (#1600), and names the
/// crate's `pub use Length::Fill;` by its path, `ui::Fill`, as inboard
/// names `iced::Center`. `rawr check` passes as `cargo check` does, and
/// `rawr run` prints what `cargo run` prints.
#[test]
fn a_marked_theme_over_a_native_theme_converts_and_implements_crate_traits() {
    let root = THEME.project();
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&native.stdout).into_owned();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert!(
        printed.contains(
            "hello at 14 with code at 12\nsettings Settings { text_size: 14.0, code_size: 12.0 }\n"
        ),
        "{printed}"
    );
    assert!(
        printed.contains("view 21 text styled 21 127 base light\n"),
        "{printed}"
    );
    assert!(
        printed.contains("void styled 0 idle\nhome badge on dark\ninbox inbox styled 120 Some(Inbox(Opened(120)))\nsidebar inbox styled 250 Some(Inbox(Opened(250)))\ntitled mail styled 254 idle\npage grid styled 121 Some(Grid(Tile(120)))\n"),
        "{printed}"
    );
    assert!(
        printed.contains("[welcome styled 120, void styled 0, grid styled 251]]))) Some(Home(Inbox(Opened(120))))\n")
            && printed.contains("over opaque sales styled 253] Some(Home(Wizard(Step(3))))\ncatalog read 1\n"),
        "{printed}"
    );
    assert!(
        printed.contains("\ncaption at 12 in 7 on native text true\n"),
        "{printed}"
    );
    assert!(
        printed.contains(
            "\npanel [inbox styled 120, mail styled 254 idle, inbox styled 250]\npanel on native text true\n"
        ),
        "{printed}"
    );
    assert!(printed.contains("\nsections 2\n"), "{printed}");
    // A marked `row_group` generic over the message, at two native message
    // types, directly, through `.into()` and a crate's column, and from a
    // generic native helper (D869).
    assert!(
        printed.contains(
            "sidebar tiles row styled 0 Some(Tile(4))\nsidebar column Shrink by 0 [home row styled 0]\n\
             sidebar helper row styled 0 Some(Home(Grid(Tile(6))))\nsidebar grid row styled 0 Some(Tile(7))\n"
        ),
        "{printed}"
    );
    // A marked `Sidebar<'a, Message>` made by a marked generic fn with an
    // `impl IntoIterator` argument and built by its generic methods, at two
    // native message types, its content an `impl` of a marked trait given by
    // the blanket impl over an array and a `Vec` and by `Content`'s own
    // (#1586), and a marked `From<Group<'a, Message>>` for the element (D869
    // for methods and generic marked types).
    assert!(
        printed.contains(
            "sidebar tiles on native text true\n\
             sidebar column Shrink by 0 [tiles sidebar styled 0, column Shrink by 3 \
             [add row styled 0, first row styled 0, pushed row styled 0]] Some(Tile(8))\n\
             sidebar column Shrink by 0 [home sidebar styled 0, column Shrink by 0 \
             [inbox row styled 0]] Some(Home(Grid(Tile(10))))\n\
             sidebar column Shrink by 0 [given sidebar styled 0, column Shrink by 2 \
             [whole row styled 0]] Some(Tile(14))\n\
             sidebar column Shrink by 0 [rows group styled 0, column Shrink by 0 \
             [row row styled 0]] Some(Tile(11))\n"
        ),
        "{printed}"
    );
    // A marked generic at a message type of a private module, which no
    // path from the crate root names (`grid::tiles`).
    assert!(
        printed.contains("\nsidebar strip row styled 0 Some(Strip(13))\n"),
        "{printed}"
    );
    // inboard's `sidebar::header(..).content(..).spacing(4)`: the marked
    // builders keep native code's elements in a `Box<dyn Iterator>` they
    // make, which native code pulls from one at a time, and a marked `&mut
    // self` method pulls from in between (#1600).
    assert!(
        printed.contains(
            "\npulled Some(\"add row styled 0\")\npulled Some(Some(Tile(16)))\npulled 1 more\n\
             pulled Some(\"first row styled 0\") of tools\n"
        ),
        "{printed}"
    );
    // ... and inboard's `to_element(&mut self)` of the sidebar native code
    // keeps: a crate call copies its lent `Pixels` (#1584), and a row mixes
    // a crate element the program's path to `ui` makes with one `common`'s
    // `space!()` makes through its re-export (#1588). Another `&mut self`
    // method moves the elements native code keeps out (#1587), a
    // non-generic marked panel borrows native code's `&Theme` by the
    // crate root's `pub use theme::Theme`, cloning nothing, and a `Vec`
    // mixes an element marked code builds with one native code gives
    // (#1588).
    assert!(
        printed.contains(
            "\nsidebar column Fill by 5 [row [tools styled 21 idle, space FillxFill idle, \
             add row styled 0], column Fill by 4 [first row styled 0]] Some(Tile(19))\n\
             sidebar column Shrink by 3 [left row styled 0, right row styled 0] Some(Tile(21)) \
             leaves 0\n\
             panel column Bounded { max: 120.0 } by 0 [column Fill by 5 [row [tools styled 21 \
             idle, space FillxFill idle, + styled 0 idle], column Fill by 4 [row [one styled 0 \
             idle, two styled 0 idle], container ShrinkxShrink styled 121 (row [input 50 of 100, \
             % styled 21 idle])]], column Fill by 5 [row [rows styled 21 idle, space FillxFill \
             idle]]] Some(Typed(\"100\"))\n\
             mixed column Shrink by 0 [built styled 0, given styled 0] idle\n"
        ),
        "{printed}"
    );
    // inboard's sidebar styles: a crate calls marked closures with the
    // `&Theme` it lends, and holds the opaque `impl Fn(&Theme, Status)`
    // a native function and `Ternary::with` give (#766, #1583).
    assert!(
        printed.contains(
            "\nnavigation column Shrink by 0 [area styled 0 (column Shrink by 0 [container \
             FillxShrink centered styled 0 (one styled 0), rule 1 styled 120] idle), area \
             styled 7 (container ShrinkxShrink styled 0 (two styled 0) idle), button Style { \
             text_color: 87, background: 122 } (+ styled 0 idle), button Style { text_color: \
             8, background: 122 } (home styled 0 idle)] Some(Tab(1))\n"
        ),
        "{printed}"
    );
    // A crate's conversion named as `From`'s, as num-traits' `NumCast`, is
    // in the runner's scope, which names `From` for itself.
    assert!(printed.contains("\nnarrowed Some(7)\n"), "{printed}");
    // The trait's impl for `u64` gives it `to_u32` in scope and
    // `<u64 as NumCast>::from` (#1613, #1614).
    assert!(printed.contains("\nwidened Some(9)\n"), "{printed}");
    // inboard's inbox item style: a typed `let` of a `Box<dyn Fn(&Theme)
    // -> Padding>` reaches each `if` arm, whose boxed closure coerces to it,
    // and the box fills the crate's `impl Fn(&T) -> Padding`, as std's `impl
    // Fn for Box<F>` lets it.
    assert!(
        printed.contains(
            "\nitem Padding { vertical: 3.0, horizontal: 120.5 } \
             Padding { vertical: 0.0, horizontal: 250.0 }\n"
        ),
        "{printed}"
    );
    // The one clone is `theme.clone()` in `main`.
    assert!(printed.ends_with("\nclones 1\n"), "{printed}");
    // A check from no kept state: its first build records the instances
    // the shadow crate's table lacks and stops before its probe, and two
    // demand rounds follow. Cargo resolves once, the nightly builds for the
    // first two rounds alone, and each round's demand is probed once
    // (#446, #1618).
    let _ = fs::remove_dir_all(root.join("target/rawr"));
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env("RAWR_VERBOSE", "1")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&check.stderr);
    assert!(check.status.success(), "{said}");
    assert!(
        said.contains(
            "rawr-import: check rounds 4, Cargo resolutions 1, nightly builds 2, probes 3\n"
        ),
        "{said}"
    );
    // Each round says what its stages did: the first reads no API, and
    // each demand round probes once and reads, classifies and generates
    // the whole API it grew to. No round parses the program again: the
    // check reads its sources' parses from their cells once and remembers
    // them. A demand round after the build's first probe binds no harness,
    // copies no build output and seals no bundle: it probes the demand
    // over that build and replays its grown tables (#1618).
    let round = |number: usize| {
        said.lines()
            .find_map(|line| line.strip_prefix(&format!("rawr-import: round {number}: ")))
            .unwrap_or_else(|| panic!("no round {number}: {said}"))
            .to_owned()
    };
    assert!(
        round(1)
            .contains("probes 0, read 0, canonicalized 0, classified 0, generated 0, parsed 0,"),
        "{said}"
    );
    for number in 2..=4 {
        let stages = round(number);
        assert!(stages.contains(", probes 1, read "), "{said}");
        assert!(!stages.contains("read 0,"), "{said}");
        assert!(stages.contains(", parsed 0, Cargo views "), "{said}");
        if number > 2 {
            assert!(
                stages.starts_with("harnesses 0, compiled 0, copied 0, sealed 0, probes 1,"),
                "{said}"
            );
        }
    }
    let mut run = start(&root, &[]);
    for expected in printed.lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// An install of another Rawr build takes the instance record the last
/// build's check kept as its first guess. Its host directory is its own,
/// but its harness names rawr at the Cargo view of the host's manifests,
/// which no source edit changes, so it asks Cargo nothing, and with the
/// record's demand the tables the last build shared answer it: one round
/// with no resolution and no nightly build. Read as none,
/// the record cost a first round that built the crates only to record the
/// same instances, then a second. The record it keeps then names this
/// build.
#[test]
fn another_rawr_build_checks_with_the_instances_the_last_build_recorded() {
    let root = THEME.project();
    let check = |host: &str| {
        let checked = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
            .arg("check")
            .arg(&root)
            .env("RAWR_HOME", crate::support::runtime_home())
            .env(
                "RAWR_RUNTIME_HOME",
                crate::support::fresh(format!("islands-theme-install-{host}")),
            )
            .env("RAWR_VERBOSE", "1")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&checked.stderr).into_owned();
        assert!(checked.status.success(), "{said}");
        said
    };
    check("first");
    let location = root.join("target/rawr/instances.json");
    let kept = fs::read(&location).unwrap();
    assert!(
        String::from_utf8_lossy(&kept).contains("\"instances\":[{"),
        "the fixture's native code calls a marked generic"
    );
    // The same record, as another build of rawr kept it.
    let mut other = kept.clone();
    other[..64].fill(b'f');
    fs::write(&location, other).unwrap();
    let said = check("second");
    assert!(
        said.contains(
            "rawr-import: check rounds 1, Cargo resolutions 0, nightly builds 0, probes 0\n"
        ),
        "{said}"
    );
    assert_eq!(fs::read(&location).unwrap(), kept, "this build keeps it");
}

/// Each file under `directory` with its modification time.
fn modified(
    directory: &Path,
) -> std::collections::BTreeMap<std::path::PathBuf, std::time::SystemTime> {
    let mut files = std::collections::BTreeMap::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap().flatten() {
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                files.insert(entry.path(), entry.metadata().unwrap().modified().unwrap());
            }
        }
    }
    files
}

/// A second `rawr run` of an unchanged project rebuilds nothing (#1604):
/// no Cargo resolution, nightly build, probe or runner build, nothing read,
/// classified or generated, and no file under `target` written again, so
/// the runner it starts is the first run's. It prints what the first run
/// printed.
#[test]
fn a_second_run_of_an_unchanged_project_rebuilds_nothing() {
    const SOURCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-theme");
    let root = crate::support::fresh("islands-theme-warm");
    copy(Path::new(SOURCE), &root);
    let mut first = start(&root, &[]);
    let status = first.wait();
    assert!(status.success(), "{}", first.seen.join("\n"));
    let printed = first
        .seen
        .iter()
        .filter(|line| !line.starts_with("stderr: "))
        .fold(String::new(), |text, line| text + line.as_str() + "\n");
    let built = modified(&root.join("target"));
    let second = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env("RAWR_VERBOSE", "1")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&second.stderr);
    assert!(second.status.success(), "{said}");
    assert_eq!(String::from_utf8_lossy(&second.stdout), printed, "{said}");
    // The run line, and the round's counts of work done again; the round
    // line names each stage, so its counts are read by name.
    assert!(
        said.contains(
            "rawr-import: run rounds 1, Cargo resolutions 0, nightly builds 0, probes 0, \
             runner builds 0, runner units compiled 0\n"
        ),
        "{said}"
    );
    let round = said
        .lines()
        .find(|line| line.starts_with("rawr-import: round 1: "))
        .unwrap_or_else(|| panic!("{said}"));
    for count in [
        "harnesses 0,",
        "compiled 0,",
        "probes 0,",
        "read 0,",
        "canonicalized 0,",
        "classified 0,",
        "generated 0,",
    ] {
        assert!(round.contains(count), "{count} in {round}");
    }
    let rebuilt = modified(&root.join("target"));
    let written = rebuilt
        .iter()
        .filter(|(path, time)| built.get(*path) != Some(*time))
        .map(|(path, _)| path.display().to_string())
        .collect::<Vec<_>>();
    assert!(written.is_empty(), "written again: {written:?}\n{said}");
}

/// A start under another import context, here a `CFLAGS` no crate of the
/// fixture reads, keeps the API's classification and its bindings plan:
/// they are computed from the API alone, so they are not keyed by the
/// context, as an install that moved `CONTEXT_SCHEMA` would have had them
/// computed again (#1766).
#[test]
fn a_start_under_another_context_classifies_and_generates_nothing() {
    const SOURCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-theme");
    let root = crate::support::fresh("islands-theme-context");
    copy(Path::new(SOURCE), &root);
    let mut first = start(&root, &[]);
    let status = first.wait();
    assert!(status.success(), "{}", first.seen.join("\n"));
    let second = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env("RAWR_VERBOSE", "1")
        .env("CFLAGS", "-DRAWR_ANOTHER_CONTEXT")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&second.stderr);
    assert!(second.status.success(), "{said}");
    let round = said
        .lines()
        .find(|line| line.starts_with("rawr-import: round 1: "))
        .unwrap_or_else(|| panic!("{said}"));
    // The context moved: its API is read again, to the same content.
    assert!(!round.contains(" read 0,"), "{round}");
    for count in ["classified 0,", "generated 0,"] {
        assert!(round.contains(count), "{count} in {round}");
    }
}

/// A save that drops a crate call from a marked function swaps, and the
/// next start keeps the runner it ran on, which still covers the smaller
/// demand, instead of building one for it (#1743): no runner build.
#[test]
fn a_start_after_a_save_that_dropped_a_native_use_builds_no_runner() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
        ],
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick row styled 0"));
    // `ui::base` at the marked theme: its only marked call.
    edit(
        &root.join("src/home.rs"),
        "        ui::base(theme)\n",
        "        \"base\"\n",
    );
    edit(
        &root.join("src/widget/sidebar.rs"),
        "\"{label} row\"",
        "\"{label} item\"",
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick item styled 0"));
    let told = run.told(&events, |text| text.contains("\"swapped\""));
    assert!(!told.contains("\"preparing\""), "{told}");
    fs::write(&stop, "").unwrap();
    run.told(&events, |text| {
        text.contains("{\"event\":\"ended\",\"status\":0}")
    });
    run.child.kill().unwrap();
    run.wait();

    // The next start, which ends at once.
    let second = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env("RAWR_VERBOSE", "1")
        .env("ISLANDS_STOP", &stop)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&second.stderr);
    assert!(second.status.success(), "{said}");
    let started = said
        .lines()
        .find(|line| line.starts_with("rawr-import: a start ran "))
        .unwrap_or_else(|| panic!("{said}"));
    assert!(started.contains(", runner builds 0,"), "{started}\n{said}");
}

/// A marked function generic over the message that native code calls
/// through a generic native helper, as inboard's `small_input` calls
/// `row_group`, swaps a body edit into the running program: the instance
/// native code calls is compiled from the record of the native check
/// (D866, D869), and the edit needs no new native build.
#[test]
fn a_marked_generic_native_code_calls_through_a_generic_helper_swaps() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
        ],
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick row styled 0"));
    edit(
        &root.join("src/widget/sidebar.rs"),
        "\"{label} row\"",
        "\"{label} item\"",
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick item styled 0"));
    let told = run.told(&events, |text| text.contains("\"swapped\""));
    assert!(!told.contains("\"preparing\""), "{told}");
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );
    fs::write(&stop, "").unwrap();
    run.told(&events, |text| {
        text.contains("{\"event\":\"ended\",\"status\":0}")
    });
}

/// A save of a marked file in a running program parses that file again and
/// nothing else: the owner and the runner each keep every other source's
/// parse across saves, and neither asks Cargo to resolve, reads an API
/// table or registers a facade. The owner keeps the native part of the
/// watch from the running build, so a body edit stages no watcher root. A
/// save costs the same however many dependencies the program has (#1604).
/// The runner lowers that file again and reuses every other file's
/// lowering, one that reads a file and a variable outside the program
/// included, as inboard's `theme.rs` reads its logo (#1646), and on a
/// save that adds a line above a function, after which an error below it
/// names its own line. `RAWR_VERBOSE` says what each save did, in the owner
/// and in the runner.
/// A save that changes only a literal a marked body gives a native call,
/// as inboard's `4.0.into()` into a `Radius` or `.spacing(0)` here, swaps
/// in place: the native uses it needs are the ones the running build
/// already covers, whatever the literal's value.
#[test]
fn a_save_that_changes_a_literal_a_native_call_takes_swaps_in_place() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    // inboard's `fn corners(&self) -> Radius { 4.0.into() }`: a float a
    // crate's `From<f32>` converts, given back to the caller.
    let home = root.join("src/home.rs");
    let text = fs::read_to_string(&home).unwrap();
    assert!(text.contains("column![top_bar].spacing(0)"), "{text}");
    let text = text.replace(
        "column![top_bar].spacing(0)",
        "column![top_bar].spacing(gap())",
    ) + "\nfn gap() -> ui::Pixels {\n    4.0.into()\n}\n";
    fs::write(&home, text).unwrap();
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
            ("RAWR_VERBOSE", "1".into()),
        ],
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick row styled 0"));
    // A save of this islands program is queued for its next native call,
    // or prepares again; count both.
    let settled = |text: &str| {
        text.matches("\"event\":\"queued\"").count()
            + text.matches("\"event\":\"preparing\"").count()
    };
    edit(&home, "    4.0.into()", "    6.0.into()");
    run.told(&events, |text| settled(text) >= 1);
    edit(&home, "    6.0.into()", "    4.0.into()");
    let told = run.told(&events, |text| settled(text) >= 2);
    assert!(!told.contains("\"event\":\"preparing\""), "{told}");
    fs::write(&stop, "").unwrap();
    run.told(&events, |text| {
        text.contains("{\"event\":\"ended\",\"status\":0}")
    });
    run.child.kill().unwrap();
    run.wait();
    let seen = run.seen.join("\n");
    assert!(!seen.contains("preparing again"), "{seen}");
}

/// A crate item two `pub use` routes reach is one native use whatever
/// route a marked file names it by, as inboard's `iced::theme::Mode` is
/// also `iced::advanced::graphics::core::theme::Mode`: the compile the
/// runner is built for and the runner's own compile of a save name it by
/// one path, so a body edit elsewhere in the file swaps in place.
#[test]
fn a_save_beside_an_item_two_routes_reach_swaps_in_place() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let theme = root.join("src/theme.rs");
    let text = fs::read_to_string(&theme).unwrap();
    assert!(text.contains("ui::theme::Mode::Light"), "{text}");
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
            ("RAWR_VERBOSE", "1".into()),
        ],
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick row styled 0"));
    // A save of this islands program is queued for its next native call,
    // or prepares again.
    let settled = |text: &str| {
        text.matches("\"event\":\"queued\"").count()
            + text.matches("\"event\":\"preparing\"").count()
    };
    edit(&theme, "        4.0\n", "        5.0\n");
    let told = run.told(&events, |text| settled(text) >= 1);
    fs::write(&stop, "").unwrap();
    run.told(&events, |text| {
        text.contains("{\"event\":\"ended\",\"status\":0}")
    });
    run.child.kill().unwrap();
    run.wait();
    // Under `RAWR_VERBOSE` the runner says which use its build lacks.
    let seen = run.seen.join("\n");
    assert!(
        !told.contains("\"event\":\"preparing\"") && !seen.contains("preparing again"),
        "{told}\n{seen}"
    );
}

#[test]
fn a_save_parses_only_the_file_it_changed() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
            ("RAWR_VERBOSE", "1".into()),
        ],
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick row styled 0"));
    let sidebar = root.join("src/widget/sidebar.rs");
    edit(&sidebar, "\"{label} row\"", "\"{label} item\"");
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick item styled 0"));
    edit(&sidebar, "\"{label} item\"", "\"{label} entry\"");
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick entry styled 0"));
    // A method's body: the other files' lowering reads its impl's
    // declarations, not its body.
    edit(&sidebar, "\"{title} sidebar\"", "\"{title} panel\"");
    run.until(|line| line.starts_with("tock ") && line.contains("[bar panel styled 0, "));
    // A line above a function moves the file's later declarations, which
    // the other files' lowering reads, but never their places.
    let text = fs::read_to_string(&sidebar).unwrap();
    assert!(text.contains("/// A sidebar row:"), "{text}");
    let text = text
        .replace("/// A sidebar row:", "// The rows.\n/// A sidebar row:")
        .replace("\"{label} entry\"", "\"{label} line\"");
    fs::write(&sidebar, text).unwrap();
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick line styled 0"));
    // What another file reads of this one, and what native code is built
    // from, is its declarations by what they are, not by where they are: a
    // doc comment, a lint, a blank line or a signature written over more
    // lines keeps every other file's lowering and the native build.
    let saves = [
        (
            "/// A sidebar row:",
            "/// The first of the rows.\n/// A sidebar row:",
            "doc",
        ),
        (
            "pub fn row_group<",
            "#[allow(dead_code)]\npub fn row_group<",
            "lint",
        ),
        (
            "pub struct Sidebar<",
            "\n/// Its parts.\npub struct Sidebar<",
            "spaced",
        ),
        (
            "pub fn title(&self) -> &'a str {",
            "pub fn title(\n        &self\n    ) -> &'a str {",
            "split",
        ),
    ];
    let mut label = "line";
    let mut save = |from: &str, to: &str, next: &'static str| {
        let text = fs::read_to_string(&sidebar).unwrap();
        assert!(text.contains(from), "{text}");
        let text = text.replace(from, to).replace(
            &format!("\"{{label}} {label}\""),
            &format!("\"{{label}} {next}\""),
        );
        fs::write(&sidebar, text).unwrap();
        label = next;
    };
    for (from, to, next) in saves {
        save(from, to, next);
        let tick = format!(" tick {next} styled 0");
        run.until(|line| line.starts_with("tick ") && line.ends_with(&tick));
    }
    // A new field of a type native code keeps and a `cfg` change what
    // native code is built from: a new native build, whose runner lowers
    // again only the files that name what changed, and the program runs
    // on. A new derive swaps in place instead (#1737). The save's wait ends
    // on the event that settles it, whichever that is: a program that took
    // it in place prints no new start for a wait on that to end on.
    let settled = |text: &str| {
        ["queued", "preparing", "restarting"]
            .iter()
            .map(|event| text.matches(&format!("\"event\":\"{event}\"")).count())
            .sum::<usize>()
    };
    let before = fs::read_to_string(&events).unwrap();
    let home = root.join("src/home.rs");
    edit(
        &home,
        "    title: String,\n",
        "    title: String,\n    visits: u32,\n",
    );
    edit(
        &home,
        "            wizard: None,\n",
        "            wizard: None,\n            visits: 0,\n",
    );
    let told = run.told(&events, |text| settled(text) > settled(&before));
    assert!(
        told[before.len()..].contains("\"event\":\"preparing\""),
        "{told}\n{}",
        run.seen.join("\n")
    );
    // The new program's start.
    run.until(|line| line.starts_with("default Selected("));
    save(
        "#[allow(dead_code)]\npub fn default<",
        "#[cfg(test)]\npub fn default<",
        "configured",
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick configured styled 0"));
    // An error below the moved lines names its own line.
    edit(
        &sidebar,
        "    ui::label(&format!(\"{label} configured\"))",
        "    let _ = unknown_row;\n    ui::label(&format!(\"{label} configured\"))",
    );
    let text = fs::read_to_string(&sidebar).unwrap();
    let line = text
        .lines()
        .position(|line| line.contains("unknown_row"))
        .unwrap()
        + 1;
    let told = run.told(&events, |text| text.contains("\"event\":\"failed\""));
    let failed = told
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|event| event["event"] == "failed")
        .unwrap();
    // The runner's error is rustc's, in its own file, at its line and
    // column (#397).
    let rendered = failed["rendered"].as_str().unwrap_or_default();
    assert!(
        failed["message"] == "cannot find value `unknown_row` in this scope"
            && failed["file"] == sidebar.to_string_lossy().as_ref()
            && failed["line"] == line
            && failed["column"] == 13
            && failed["diagnostics"][0]["code"] == "E0425"
            && rendered
                .starts_with("error[E0425]: cannot find value `unknown_row` in this scope\n")
            && rendered.contains(&format!("src/widget/sidebar.rs:{line}:13-{line}:24\n"))
            && rendered.contains("let _ = unknown_row;"),
        "{failed}"
    );
    // The program ends; `rawr run` keeps watching for the next save.
    fs::write(&stop, "").unwrap();
    run.told(&events, |text| {
        text.contains("{\"event\":\"ended\",\"status\":0}")
    });
    run.child.kill().unwrap();
    run.wait();
    let seen = run.seen.join("\n");
    // Only the field builds native code again: native code holds a
    // `State`, and names no `widget::sidebar::default`, which the `cfg`
    // takes away in place (D935.3). The runner the field starts lowers
    // again only the file that names what the save changed, from what the
    // earlier runner kept (#1725).
    let prepared = run
        .seen
        .iter()
        .filter_map(|line| line.strip_prefix("stderr: rawr: preparing again: "))
        .collect::<Vec<_>>();
    assert_eq!(prepared, ["`home::State` changed its fields"], "{seen}");
    let started = run
        .seen
        .iter()
        .filter_map(|line| line.strip_prefix("stderr: rawr runner: a start parsed "))
        .map(|line| {
            let (_, rest) = line.split_once(" sources, revealed ").unwrap();
            let (revealed, rest) = rest.split_once(" files, lowered ").unwrap();
            let lowered = rest.strip_suffix(" files").unwrap();
            [revealed, lowered].map(|count| count.parse::<usize>().unwrap())
        })
        .collect::<Vec<_>>();
    // The field: `home.rs` alone, which declares `State`.
    assert_eq!(started[1..], [[1, 1]], "{seen}");
    // Each save says what it did; one the watcher heard twice compiles the
    // same text again and parses nothing.
    let parsed = |prefix: &str, rest: &str| {
        let counts = run
            .seen
            .iter()
            .filter_map(|line| line.strip_prefix(prefix))
            .map(|line| {
                let (parsed, said) = line.split_once(" sources").unwrap();
                assert_eq!(said, rest, "{seen}");
                parsed.parse::<usize>().unwrap()
            })
            .collect::<Vec<_>>();
        assert!(counts.iter().all(|count| *count <= 1), "{seen}");
        counts.iter().sum::<usize>()
    };
    assert_eq!(
        parsed(
            "stderr: rawr-import: a save parsed ",
            ", Cargo resolutions 0, API tables loaded 0, facades registered 0, watch roots staged 0"
        ),
        9,
        "{seen}"
    );
    // The runner reads for its instances and lowers again only the file
    // the save changed: each edit is to a body, which nothing another file
    // reads, or moves declarations or adds inert attributes, which change
    // nothing another file reads (#1646). A save the watcher heard twice
    // does nothing.
    let runner = run
        .seen
        .iter()
        .filter_map(|line| line.strip_prefix("stderr: rawr runner: a save parsed "))
        .map(|line| {
            let (parsed, rest) = line.split_once(" sources, revealed ").unwrap();
            let (revealed, rest) = rest.split_once(" files, lowered ").unwrap();
            let (lowered, rest) = rest.split_once(" files").unwrap();
            assert_eq!(rest, ", registered 0 facades", "{seen}");
            [parsed, revealed, lowered].map(|count| count.parse::<usize>().unwrap())
        })
        .filter(|counts| *counts != [0; 3])
        .collect::<Vec<_>>();
    // The `cfg` swaps in place, and lowers again the sidebar it changed
    // and `sidebar.rs` and `theme.rs`, which write `default`. The save with
    // the error fails as it reveals, and lowers nothing.
    let mut saved = vec![[1, 1, 1]; 8];
    saved.push([1, 3, 3]);
    saved.push([1, 1, 0]);
    assert_eq!(runner, saved, "{seen}");
}

/// A save lowers again the file it changed and the files that name what
/// it changed, and no other (#1718): a new function, a derive, a name a
/// file wrote before any item had it. Each is private, which native code
/// doesn't see, so the runner swaps it in: the sidebar's own marked child
/// module, which names the sidebar's private items through `super`.
#[test]
fn a_save_lowers_only_the_files_that_name_what_it_changed() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let sidebar = root.join("src/widget/sidebar.rs");
    // The child alone names the sidebar's `Aside` and calls its
    // `aside_rows!`, which textual scope lets into the child, and writes
    // `spare`, a local's name, which no item has yet.
    let mut text = fs::read_to_string(&sidebar).unwrap();
    text.push_str(
        "\nmacro_rules! aside_rows {\n    () => {\n        1\n    };\n}\n\n\
         mod aside;\n\n/// What a sidebar keeps aside, which only its child names.\n\
         #[derive(Debug)]\nstruct Aside {\n    rows: usize,\n}\n",
    );
    fs::write(&sidebar, text).unwrap();
    fs::create_dir_all(root.join("src/widget/sidebar")).unwrap();
    fs::write(
        root.join("src/widget/sidebar/aside.rs"),
        "// rawr\n//! The rows a sidebar keeps aside.\n\n\
         pub fn rows() -> usize {\n    let spare = super::Aside { rows: aside_rows!() };\n    \
         spare.rows\n}\n",
    )
    .unwrap();
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
            ("RAWR_VERBOSE", "1".into()),
        ],
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick row styled 0"));
    // A new function: the sidebar, and the child, which writes its name.
    let mut text = fs::read_to_string(&sidebar)
        .unwrap()
        .replace("\"{label} row\"", "\"{label} item\"");
    text.push_str("\nfn spare() -> usize {\n    2\n}\n");
    fs::write(&sidebar, text).unwrap();
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick item styled 0"));
    // A derive of `Aside`: the sidebar, and the child, which names it.
    edit(
        &sidebar,
        "#[derive(Debug)]\nstruct Aside",
        "#[derive(Debug, Hash)]\nstruct Aside",
    );
    edit(&sidebar, "\"{label} item\"", "\"{label} entry\"");
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick entry styled 0"));
    // A private struct before the sidebar's types, which no other file
    // names: the sidebar alone, though the child names `Aside` and calls
    // `aside_rows!`, both written after it (#1726, #1730).
    const CORNERS: &str = "#[derive(Debug)]\nstruct Corners {\n    radius: f32,\n}\n\n";
    let text = fs::read_to_string(&sidebar)
        .unwrap()
        .replace(
            "/// A sidebar row:",
            &format!("{CORNERS}/// A sidebar row:"),
        )
        .replace("\"{label} entry\"", "\"{label} cell\"");
    fs::write(&sidebar, text).unwrap();
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick cell styled 0"));
    // The struct removed: the sidebar alone again. No value of it lives,
    // so the running program takes it in place (#1746).
    let text = fs::read_to_string(&sidebar)
        .unwrap()
        .replace(CORNERS, "")
        .replace("\"{label} cell\"", "\"{label} line\"");
    fs::write(&sidebar, text).unwrap();
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick line styled 0"));
    fs::write(&stop, "").unwrap();
    run.told(&events, |text| {
        text.contains("{\"event\":\"ended\",\"status\":0}")
    });
    run.child.kill().unwrap();
    run.wait();
    let seen = run.seen.join("\n");
    assert!(
        run.seen
            .iter()
            .all(|line| !line.contains("preparing again") && !line.contains("restarting")),
        "{seen}"
    );
    // What each save parsed, revealed and lowered: none lowers `home.rs`,
    // `theme.rs` or `sidebar.rs`, which name nothing they changed, and the
    // struct's saves, above the macro, lower the widget's sidebar alone.
    let runner = run
        .seen
        .iter()
        .filter_map(|line| line.strip_prefix("stderr: rawr runner: a save parsed "))
        .map(|line| {
            let (parsed, rest) = line.split_once(" sources, revealed ").unwrap();
            let (revealed, rest) = rest.split_once(" files, lowered ").unwrap();
            let (lowered, _) = rest.split_once(" files").unwrap();
            [parsed, revealed, lowered].map(|count| count.parse::<usize>().unwrap())
        })
        .filter(|counts| *counts != [0; 3])
        .collect::<Vec<_>>();
    assert_eq!(
        runner,
        [[1, 2, 2], [1, 2, 2], [1, 1, 1], [1, 1, 1]],
        "{seen}"
    );
}

/// A private struct's new derives, new field and removal swap in place
/// while no value of it lives: nothing the running program holds depends
/// on it (#1746). A static of a script type holds no value either, as each
/// read is its initializer (D862), so a struct only a static and the row
/// read takes its new field and its removal in place too. No save
/// prepares a native build or restarts the program.
#[test]
fn a_private_struct_nothing_holds_swaps_its_derives_fields_and_removal() {
    const DERIVED: &str = "#[derive(Debug, Clone, Copy, PartialEq)]\nstruct Corners {\n";
    const INSET: &str = "#[derive(Debug)]\nstruct Inset {\n    width: f32,\n}\n\n\
                         static INSET: Inset = Inset { width: 1.0 };\n\n";
    const GROWN: &str = "#[derive(Debug)]\nstruct Inset {\n    width: f32,\n    \
                         depth: f32,\n}\n\nstatic INSET: Inset = Inset { width: 1.0, depth: 2.0 };\n\n";
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let sidebar = root.join("src/widget/sidebar.rs");
    edit(
        &sidebar,
        "/// A sidebar row:",
        "#[derive(Debug)]\nstruct Corners {\n    radius: f32,\n}\n\n/// A sidebar row:",
    );
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
            ("RAWR_VERBOSE", "1".into()),
        ],
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick row styled 0"));
    // Each save is one write: the structs' changes, and the row's label,
    // which says the program runs the save.
    let save = |changes: &[(&str, &str)], label: &str| {
        let mut text = fs::read_to_string(&sidebar).unwrap();
        let (_, last) = text.split_once("\"{label} ").unwrap();
        let (last, _) = last.split_once('"').unwrap();
        let last = format!("\"{{label}} {last}\"");
        for (from, to) in changes {
            assert!(text.contains(from), "{from}\n{text}");
            text = text.replace(from, to);
        }
        fs::write(
            &sidebar,
            text.replace(&last, &format!("\"{{label}} {label}\"")),
        )
        .unwrap();
    };
    let ticks = |label: &str| {
        let ending = format!(" tick {label} styled 0");
        move |line: &str| line.starts_with("tick ") && line.ends_with(&ending)
    };
    // New derives, the fields the same.
    save(&[("#[derive(Debug)]\nstruct Corners {\n", DERIVED)], "item");
    run.until(ticks("item"));
    // A new field.
    save(
        &[("    radius: f32,\n", "    radius: f32,\n    depth: f32,\n")],
        "entry",
    );
    run.until(ticks("entry"));
    // The struct removed.
    save(
        &[(
            &format!("{DERIVED}    radius: f32,\n    depth: f32,\n}}\n\n"),
            "",
        )],
        "cell",
    );
    run.until(ticks("cell"));
    // A struct a static gives, which the row reads each tick.
    save(
        &[
            ("/// A sidebar row:", &format!("{INSET}/// A sidebar row:")),
            (
                "    ui::label(&format!(",
                "    let _ = INSET.width;\n    ui::label(&format!(",
            ),
        ],
        "line",
    );
    run.until(ticks("line"));
    // Its new field.
    save(
        &[
            ("    width: f32,\n", "    width: f32,\n    depth: f32,\n"),
            ("Inset { width: 1.0 }", "Inset { width: 1.0, depth: 2.0 }"),
        ],
        "word",
    );
    run.until(ticks("word"));
    // The struct and its static removed.
    save(&[(GROWN, ""), ("    let _ = INSET.width;\n", "")], "name");
    run.until(ticks("name"));
    fs::write(&stop, "").unwrap();
    run.told(&events, |text| {
        text.contains("{\"event\":\"ended\",\"status\":0}")
    });
    run.child.kill().unwrap();
    run.wait();
    let seen = run.seen.join("\n");
    assert!(
        run.seen
            .iter()
            .all(|line| !line.contains("restarting") && !line.contains("preparing again")),
        "{seen}"
    );
    // One runner, and each save compiled in it.
    let count = |prefix: &str| {
        run.seen
            .iter()
            .filter(|line| line.starts_with(prefix))
            .count()
    };
    assert_eq!(
        (
            count("stderr: rawr runner: a start parsed "),
            count("stderr: rawr runner: a save parsed ")
        ),
        (1, 6),
        "{seen}"
    );
}

/// A save that adds a function native code calls needs a new native build
/// and a new runner, whose start lowers again only the file the save
/// changed and the files that name what it added, from what the earlier
/// runner kept (#1725): its own build of rawr wrote them, and each file's
/// key holds what it reads of the native surface. Native code's call is
/// what needs the build, as a new function no native code calls swaps
/// (#1741). A new public struct that native code names, which the runner
/// holds as its mirror, lowers again its own file and native code's, and
/// no marked file that doesn't name it (#1740).
#[test]
fn a_new_runner_lowers_only_the_files_that_name_a_new_native_function() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let _ = fs::remove_dir_all(root.join("target/rawr/lowered"));
    // `home.rs` writes `spare_rows`, a local's name, which no item has yet.
    let home = root.join("src/home.rs");
    let mut text = fs::read_to_string(&home).unwrap();
    text.push_str("\nfn spared() -> usize {\n    let spare_rows = 1;\n    spare_rows\n}\n");
    fs::write(&home, text).unwrap();
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
            ("RAWR_VERBOSE", "1".into()),
        ],
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick row styled 0"));
    // A new public function of the marked `sidebar.rs`, and native code
    // that calls it, so the program starts again with a new native build.
    let sidebar = root.join("src/sidebar.rs");
    let mut text = fs::read_to_string(&sidebar).unwrap();
    text.push_str("\npub fn spare_rows() -> usize {\n    2\n}\n");
    fs::write(&sidebar, text).unwrap();
    edit(
        &root.join("src/main.rs"),
        "    println!(\"clones {}\", theme::clones());\n",
        "    println!(\"clones {}\", theme::clones());\n    \
         println!(\"spare {}\", sidebar::spare_rows());\n",
    );
    // The new program runs, and ends when told to.
    run.told(&events, |text| {
        text.matches("{\"event\":\"running\"}").count() >= 2
    });
    // A new public struct with a derive, which no other marked file names,
    // and native code that names it: the runner holds it as its mirror.
    let mut text = fs::read_to_string(&sidebar).unwrap();
    text.push_str("\n#[derive(Debug)]\npub struct Inset {\n    pub radius: f32,\n}\n");
    fs::write(&sidebar, text).unwrap();
    edit(
        &root.join("src/main.rs"),
        "    println!(\"spare {}\", sidebar::spare_rows());\n",
        "    println!(\"spare {}\", sidebar::spare_rows());\n    \
         println!(\"inset {}\", sidebar::Inset { radius: 1.5 }.radius);\n",
    );
    run.told(&events, |text| {
        text.matches("{\"event\":\"running\"}").count() >= 3
    });
    fs::write(&stop, "").unwrap();
    let told = run.told(&events, |text| text.contains("{\"event\":\"ended\""));
    run.child.kill().unwrap();
    run.wait();
    let seen = run.seen.join("\n");
    assert!(
        told.contains("{\"event\":\"ended\",\"status\":0}")
            && run.seen.iter().any(|line| line == "spare 2")
            && run.seen.iter().any(|line| line == "inset 1.5")
            && run
                .seen
                .iter()
                .filter(|line| line.starts_with("default Selected("))
                .count()
                == 3,
        "{told}\n{seen}"
    );
    // What each runner's start revealed and lowered: the first every file;
    // the second `sidebar.rs` and `main.rs`, which the edit changed, and
    // `home.rs`, which names what it added; the third `sidebar.rs` and
    // `main.rs` alone, which the struct's edit changed.
    let started = run
        .seen
        .iter()
        .filter_map(|line| line.strip_prefix("stderr: rawr runner: a start parsed "))
        .map(|line| {
            let (_, rest) = line.split_once(" sources, revealed ").unwrap();
            let (revealed, rest) = rest.split_once(" files, lowered ").unwrap();
            let lowered = rest.strip_suffix(" files").unwrap();
            [revealed, lowered].map(|count| count.parse::<usize>().unwrap())
        })
        .collect::<Vec<_>>();
    assert!(
        started.len() == 3 && started[0][1] > 2 && started[1] == [3, 3] && started[2] == [2, 2],
        "{started:?}\n{seen}"
    );
}

/// A private field added to a marked pub struct native code holds, as
/// inboard's `State`, builds native code again. Its preparation probes the
/// new build once, and canonicalizes only the fragments whose API changed:
/// none, as the private field is none of the crate's API, and the path
/// crates `common` and `ui` changed nothing. The field made public changes
/// the project crate's API, whose fragment alone is read again (#1776).
/// A private field reads no definition at all: its fragments' bodies are
/// the last settlement's, which it takes whole (#1786). A save after one
/// that built probes its own build once, never the last build's artifacts
/// first (#1794), and each restart says what its runner build compiled
/// (#1795). Each save's nightly check compiles only the project's copy and
/// its harness, the first after the start too (#1815), and each restart
/// writes only the runner sources the edit changed (#1816).
#[test]
fn a_held_pub_type_s_private_field_reads_only_the_project_crate_s_api() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
            ("RAWR_VERBOSE", "1".into()),
        ],
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick row styled 0"));
    let home = root.join("src/home.rs");
    let text = fs::read_to_string(&home).unwrap();
    let edited = text
        .replace(
            "    wizard: Option<wizard::State>,\n}",
            "    wizard: Option<wizard::State>,\n    seen: u8,\n}",
        )
        .replace(
            "            wizard: None,\n        }",
            "            wizard: None,\n            seen: 0,\n        }",
        );
    assert_ne!(edited, text);
    fs::write(&home, edited).unwrap();
    run.told(&events, |text| {
        text.matches("{\"event\":\"running\"}").count() >= 2
    });
    // Another private field, after a save that built: inboard's case.
    edit(
        &home,
        "    seen: u8,\n}",
        "    seen: u8,\n    seen_again: u8,\n}",
    );
    edit(
        &home,
        "            seen: 0,\n        }",
        "            seen: 0,\n            seen_again: 0,\n        }",
    );
    run.told(&events, |text| {
        text.matches("{\"event\":\"running\"}").count() >= 3
    });
    edit(&home, "    seen: u8,\n", "    pub seen: u8,\n");
    run.told(&events, |text| {
        text.matches("{\"event\":\"running\"}").count() >= 4
    });
    fs::write(&stop, "").unwrap();
    let told = run.told(&events, |text| text.contains("{\"event\":\"ended\""));
    run.child.kill().unwrap();
    run.wait();
    let seen = run.seen.join("\n");
    assert!(
        told.contains("{\"event\":\"ended\",\"status\":0}")
            && seen.contains("rawr: preparing again: `home::State` changed its fields"),
        "{told}\n{seen}"
    );
    let rounds = run
        .seen
        .iter()
        .filter_map(|line| line.strip_prefix("stderr: rawr-import: a save's round: "))
        .collect::<Vec<_>>();
    // A round for each save, each probing its own build once. The first
    // two take every fragment's read and the settlement the launch or the
    // last save kept, and read no definition; the third reads the project
    // crate's fragment again, and takes `common`'s and `ui`'s.
    assert!(
        rounds.len() == 3
            && rounds[0].contains(", probes 1, read 0, canonicalized 0, classified 0, ")
            && rounds[1].contains(", probes 1, read 0, canonicalized 0, classified 0, ")
            && rounds[2].contains(", probes 1, ")
            && rounds[2].contains(", canonicalized 1, "),
        "{rounds:?}\n{seen}"
    );
    eprintln!("the saves' rounds:\n{}", rounds.join("\n"));
    // Each save's nightly check goes on from the last one: it compiles the
    // project's copy and the harness, as every save does, the first after
    // the start too (#1815).
    let checked = run
        .seen
        .iter()
        .filter_map(|line| line.strip_prefix("stderr: rawr-import: the nightly check compiled "))
        .collect::<Vec<_>>();
    assert!(
        checked.len() == 3
            && checked.iter().all(|line| line.starts_with("2 units in ")
                && line.ends_with(": islands_theme, rawr_resolution_harness")),
        "{checked:?}\n{seen}"
    );
    eprintln!("the nightly checks:\n{}", checked.join("\n"));
    // Each restart generates the runner's sources and stages them, writing
    // only the files the edit changed (#1816).
    let staged = run
        .seen
        .iter()
        .filter_map(|line| {
            line.strip_prefix("stderr: rawr-import: the runner's sources were staged in ")
        })
        .collect::<Vec<_>>();
    let generated = run
        .seen
        .iter()
        .filter(|line| {
            line.starts_with("stderr: rawr-import: the runner's sources were generated in ")
        })
        .count();
    assert!(
        staged.len() == 3
            && generated >= 3
            && staged.iter().all(|line| {
                let (written, files) = line
                    .split_once(": wrote ")
                    .and_then(|(_, counts)| counts.strip_suffix(" files"))
                    .and_then(|counts| counts.split_once(" of "))
                    .unwrap();
                written.parse::<usize>().unwrap() <= 2 && files.parse::<usize>().unwrap() > 2
            }),
        "{staged:?}\n{seen}"
    );
    eprintln!("the runner's staging:\n{}", staged.join("\n"));
    // Each restart's runner build says what it compiled.
    let built = run
        .seen
        .iter()
        .filter_map(|line| line.strip_prefix("stderr: rawr-import: the runner build compiled "))
        .collect::<Vec<_>>();
    assert!(built.len() >= 3, "{built:?}\n{seen}");
    eprintln!("the runner builds:\n{}", built.join("\n"));
}

/// An edit to a function of a path crate of the project changes what
/// native code is built from: a new native build, and a new runner, whose
/// start lowers again only the files that name the function, as each
/// entry of a path crate's facade is a declaration of its own (#1735).
#[test]
fn an_edit_to_a_path_crate_s_function_lowers_only_the_files_that_name_it() {
    // Twice at one path: the project put back the second time puts the
    // path crate's file back with the time it had before the first edit,
    // older than what rawr's home built from the edit, so only the file's
    // contents tell that its units must build again (#1750).
    let mut root = THEME.project();
    edit_a_path_crate_s_function(&root);
    root.again();
    edit_a_path_crate_s_function(&root);
}

fn edit_a_path_crate_s_function(root: &Path) {
    // No kept lowering: the first start lowers every file.
    let _ = fs::remove_dir_all(root.join("target/rawr/lowered"));
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let mut run = start(
        root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
            ("RAWR_VERBOSE", "1".into()),
        ],
    );
    run.until(|line| line.starts_with("tick ") && line.ends_with(" tick row styled 0"));
    // `ui::base` gives the length of the theme's name: `home.rs`, which
    // calls it, and `sidebar.rs`, which writes `base`, read it.
    edit(
        &root.join("ui/src/lib.rs"),
        "pub fn base<B: Base>(base: &B) -> String {\n    format!(\"base {}\", base.name())\n}",
        "pub fn base<B: Base>(base: &B) -> usize {\n    base.name().len()\n}",
    );
    run.told(&events, |text| {
        text.matches("{\"event\":\"running\"}").count() >= 2
    });
    fs::write(&stop, "").unwrap();
    let told = run.told(&events, |text| text.contains("{\"event\":\"ended\""));
    run.child.kill().unwrap();
    run.wait();
    let seen = run.seen.join("\n");
    assert!(
        told.contains("{\"event\":\"ended\",\"status\":0}")
            && run.seen.iter().any(|line| line.starts_with("base "))
            && run.seen.iter().any(|line| line.parse::<usize>().is_ok()),
        "{told}\n{seen}"
    );
    // What each runner's start revealed and lowered: the first every file.
    // The second reads `home.rs` and `sidebar.rs` again, which name `base`,
    // and lowers them and the widget's sidebar, whose kept lowering calls
    // the adapters of `ui::Pixels`: an adapter names its crate by its
    // facade's proof, which the edit changed, so the new runner admitted
    // other ones (#1734).
    let started = run
        .seen
        .iter()
        .filter_map(|line| line.strip_prefix("stderr: rawr runner: a start parsed "))
        .map(|line| {
            let (_, rest) = line.split_once(" sources, revealed ").unwrap();
            let (revealed, rest) = rest.split_once(" files, lowered ").unwrap();
            let lowered = rest.strip_suffix(" files").unwrap();
            [revealed, lowered].map(|count| count.parse::<usize>().unwrap())
        })
        .collect::<Vec<_>>();
    assert!(
        started.len() == 2 && started[0][1] > 2 && started[1] == [2, 3],
        "{started:?}\n{seen}"
    );
}

/// A runner keeps each marked source's lowering and readings under the
/// project's `target/rawr`, so the next runner's start lowers and reads
/// only the sources whose key changed. A corrupt file is a miss, made
/// again and written over (#1638).
#[test]
fn a_start_lowers_only_the_files_an_earlier_runner_did_not_keep() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let lowered = root.join("target/rawr/lowered");
    let _ = fs::remove_dir_all(&lowered);
    // One run to its first tick and its end: what its runner's start
    // parsed, revealed and lowered.
    let started = || {
        let _ = fs::remove_file(&events);
        let _ = fs::remove_file(&stop);
        let mut run = start(
            &root,
            &[
                ("ISLANDS_STOP", stop.clone()),
                ("RAWR_MCP_EVENTS", events.clone()),
                ("RAWR_VERBOSE", "1".into()),
            ],
        );
        run.until(|line| line.starts_with("tick ") && line.contains(" styled 0"));
        fs::write(&stop, "").unwrap();
        run.told(&events, |text| {
            text.contains("{\"event\":\"ended\",\"status\":0}")
        });
        run.child.kill().unwrap();
        run.wait();
        let seen = run.seen.join("\n");
        let counts = run
            .seen
            .iter()
            .filter_map(|line| line.strip_prefix("stderr: rawr runner: a start parsed "))
            .map(|line| {
                let (parsed, rest) = line.split_once(" sources, revealed ").unwrap();
                let (revealed, rest) = rest.split_once(" files, lowered ").unwrap();
                let lowered = rest.strip_suffix(" files").unwrap();
                [parsed, revealed, lowered].map(|count| count.parse::<usize>().unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(counts.len(), 1, "{seen}");
        (counts[0], seen)
    };
    let ([_, revealed, first], seen) = started();
    assert!(revealed > 0 && first > 0, "{seen}");
    // Nothing changed: the start reads every source `rawr` parsed from its
    // parse, and lowers and reads nothing again.
    let (counts, seen) = started();
    assert_eq!(counts, [0, 0, 0], "{seen}");
    // A body's edit: its file alone.
    edit(
        &root.join("src/widget/sidebar.rs"),
        "\"{label} row\"",
        "\"{label} item\"",
    );
    let (counts, seen) = started();
    assert_eq!(counts, [0, 1, 1], "{seen}");
    // A corrupt file is lowered again and written over.
    let mut bodies = fs::read_dir(&lowered)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "bodies")
        })
        .collect::<Vec<_>>();
    bodies.sort();
    assert!(!bodies.is_empty(), "{seen}");
    fs::write(&bodies[0], "RKLW not a lowering").unwrap();
    let (counts, seen) = started();
    assert_eq!(counts, [0, 0, 1], "{seen}");
    assert!(fs::read(&bodies[0]).unwrap().len() > 100, "{seen}");
}

/// A marked function that panics in a call from native `main` prints the
/// panic Cargo's build prints: its place in the marked file and its
/// message, with nothing of rawr's (#1657).
#[test]
fn a_marked_function_s_panic_in_a_native_call_prints_as_cargo_s_build_prints_it() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    edit(
        &root.join("src/widget/sidebar.rs"),
        "    ui::label(&format!(\"{label} row\"))",
        "    if label == \"tick\" {\n        panic!(\"no tick line\");\n    }\n    ui::label(&format!(\"{label} row\"))",
    );
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env("ISLANDS_STOP", &stop)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RUST_BACKTRACE")
        .output()
        .unwrap();
    assert_eq!(native.status.code(), Some(101));
    let stderr = String::from_utf8_lossy(&native.stderr);
    // Current nightly's thread id is #36's.
    let panicked = stderr
        .lines()
        .map(|line| match line.split_once("thread 'main' (") {
            Some(("", rest)) => rest.split_once(") ").map_or_else(
                || line.to_owned(),
                |(_, rest)| format!("thread 'main' {rest}"),
            ),
            _ => line.to_owned(),
        })
        .skip_while(|line| !line.starts_with("thread 'main' panicked at "))
        .collect::<Vec<_>>();
    assert!(
        panicked.first().is_some_and(
            |line| line.starts_with("thread 'main' panicked at src/widget/sidebar.rs:")
        ) && panicked.get(1).is_some_and(|line| line == "no tick line"),
        "{stderr}"
    );
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
        ],
    );
    run.told(&events, |text| {
        text.contains("{\"event\":\"ended\",\"status\":101}")
    });
    run.child.kill().unwrap();
    run.wait();
    let printed = run
        .seen
        .iter()
        .filter_map(|line| line.strip_prefix("stderr: "))
        .skip_while(|line| !line.starts_with("thread 'main' panicked at "))
        .take(panicked.len())
        .collect::<Vec<_>>();
    let seen = run.seen.join("\n");
    assert_eq!(printed, panicked, "{seen}");
    assert!(
        !seen.contains("failed: ") && !seen.contains("(line "),
        "{seen}"
    );
}

/// A generic method of a marked type generic over the message, which
/// native code builds and keeps, as inboard's `Sidebar<'a, Message>`,
/// swaps a body edit into the running program: native code calls the
/// instance its check recorded (D869), and the edit needs no new native
/// build.
#[test]
fn a_generic_marked_types_method_native_code_calls_swaps() {
    let root = THEME.project();
    let events = root.join("events.jsonl");
    let stop = root.join("stop");
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", stop.clone()),
            ("RAWR_MCP_EVENTS", events.clone()),
        ],
    );
    run.until(|line| line.starts_with("tock ") && line.contains("[bar sidebar styled 0, "));
    edit(
        &root.join("src/widget/sidebar.rs"),
        "\"{title} sidebar\"",
        "\"{title} panel\"",
    );
    run.until(|line| line.starts_with("tock ") && line.contains("[bar panel styled 0, "));
    let told = run.told(&events, |text| text.contains("\"swapped\""));
    assert!(!told.contains("\"preparing\""), "{told}");
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );
    fs::write(&stop, "").unwrap();
    run.told(&events, |text| {
        text.contains("{\"event\":\"ended\",\"status\":0}")
    });
}

/// A marked module names a native workspace crate's `&'static str`, byte
/// string and slice constants and `&'static str` static through the crate's
/// module, as inboard's `theme.rs` names `fonts::UI_FAMILY`, and gives the
/// text to the crate's `Font::new(name: &'static str)`, and the constant of
/// a crate that has no other public item (#1146). `rawr
/// check` passes as `cargo check` does, and `rawr run` prints what `cargo
/// run` prints.
#[test]
fn a_marked_module_reads_a_native_crate_s_static_text_and_slice_constants() {
    const CONSTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-consts");
    let root = crate::support::fresh("islands-consts");
    copy(Path::new(CONSTS), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&native.stdout).into_owned();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert!(
        printed.contains("family Inter Tight has 11 letters\nversion 1.2.3\n"),
        "{printed}"
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(&root, &[]);
    for expected in printed.lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));

    // An item of the module that can't be used yet is rustc's E0425 for
    // the module, with the reason under it, never a type's E0599.
    let fonts = root.join("common/src/fonts.rs");
    let mut text = fs::read_to_string(&fonts).unwrap();
    text.push_str("\npub const PAIR: &(u8, u8) = &(1, 2);\n");
    fs::write(&fonts, text).unwrap();
    let theme = root.join("src/theme.rs");
    let mut text = fs::read_to_string(&theme).unwrap();
    text.push_str("\npub fn pair() -> u8 {\n    fonts::PAIR.0\n}\n");
    fs::write(&theme, text).unwrap();
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(!check.status.success(), "{stderr}");
    assert!(
        stderr.contains("error[E0425]: cannot find value `PAIR` in module `fonts`")
            && stderr.contains("= note: `PAIR` can't be used yet:"),
        "{stderr}"
    );
}

#[test]
fn a_marked_static_of_native_blocks_is_read_by_native_code() {
    const BLOCKS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-blocks");
    let root = crate::support::fresh("islands-blocks");
    copy(Path::new(BLOCKS), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&native.stdout).into_owned();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert!(
        printed.ends_with(
            "Count\n3\n  md A badge marks a count.\n3\n3 blocks\nexample demo \"\"\n\
             md A badge marks a count.\ngroup Examples, 2 children\n\
             first Dot Some(\"A badge with no count.\")\n  md Counts past 99 show 99+.\n\
             \x20   example dot \"Dot\" Some(\"A badge with no count.\") \"badge()\"\n\
             \x20   md Counts past 99 show 99+.\npage 4 of 3 blocks\n\
             \x20 example demo \"\" None \"\"\n  md A badge marks a count.\n\
             \x20 group Examples\n\
             \x20   example dot \"Dot\" Some(\"A badge with no count.\") \"badge()\"\n\
             \x20   md Counts past 99 show 99+.\npage 5 of 1 blocks\n\
             \x20 md A badge marks a count.\npage 6 of 1 blocks\n  md promoted\n\
             Badge: Displays a badge.\nexample demo \"\" None \"\"\n\
             md A badge marks a count.\ngroup Examples\n\
             \x20 example dot \"Dot\" Some(\"A badge with no count.\") \"badge()\"\n\
             \x20 md Counts past 99 show 99+.\npage 7 of 3 blocks\n\
             \x20 example demo \"\" None \"\"\n  md A badge marks a count.\n\
             \x20 group Examples\n\
             \x20   example dot \"Dot\" Some(\"A badge with no count.\") \"badge()\"\n\
             \x20   md Counts past 99 show 99+.\nindex 7\ntrue true true\n\
             badge 7 Id(\"page-7\") Id(\"page-7\") true\npage 7 of 3 blocks\n\
             \x20 example demo \"\" None \"\"\n  md A badge marks a count.\n\
             \x20 group Examples\n\
             \x20   example dot \"Dot\" Some(\"A badge with no count.\") \"badge()\"\n\
             \x20   md Counts past 99 show 99+.\nbadge 7\n"
        ),
        "{printed}"
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(&root, &[]);
    for expected in printed.lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// The blocks app's badge borrowing a local where native code takes a
/// borrow for `'static`: an array where `State::new(page_index, blocks:
/// &'static [Block])` takes the page's blocks, and a `String` where
/// `named(name: impl Into<Cow<'static, str>>)` takes a name. `rawr check`
/// gives cargo check's E0597 at the borrow, while the fixture's `static`,
/// its range and a promoted literal pass (#1064).
#[test]
fn a_local_borrowed_where_native_code_takes_static_does_not_live_long_enough() {
    const BLOCKS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-blocks");
    const NAMED: &str = "\n/// Names a page, for as long as the app runs.\n\
         pub fn named(name: impl Into<std::borrow::Cow<'static, str>>) -> usize {\n    \
         name.into().len()\n}\n";
    const ARRAY: &str = "\n/// A state that keeps a local array.\n\
         pub fn local() {\n    \
         let blocks = [page::Block::Md(\"local\")];\n    \
         page::State::new(8, &blocks).show();\n}\n";
    const STRING: &str = "\n/// A page named by a local string.\n\
         pub fn named() -> usize {\n    \
         let text = String::from(\"local\");\n    \
         page::named(&text)\n}\n";
    let root = crate::support::fresh("islands-blocks-local");
    // cargo check's positions: the borrow `&blocks` at column 25 and
    // `&text` at column 17 of the line that holds it, which the fixture's
    // length places.
    for (badge, local, borrow, column) in [
        (ARRAY, "`blocks`", "&blocks", 25),
        (STRING, "`text`", "&text", 17),
    ] {
        let _ = fs::remove_dir_all(root.join("src"));
        copy(Path::new(BLOCKS), &root);
        for (file, added) in [("src/page.rs", NAMED), ("src/component/badge.rs", badge)] {
            let mut text = fs::read_to_string(root.join(file)).unwrap();
            text.push_str(added);
            fs::write(root.join(file), text).unwrap();
        }
        let line = fs::read_to_string(root.join("src/component/badge.rs"))
            .unwrap()
            .lines()
            .position(|text| text.contains(borrow))
            .expect("the added function holds the borrow")
            + 1;
        let at = format!("badge.rs:{line}:{column}");
        let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
            .arg("check")
            .arg(&root)
            .env("RAWR_HOME", crate::support::runtime_home())
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("RAWR_VERBOSE")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&check.stderr);
        assert!(!check.status.success(), "{stderr}");
        let cargo = crate::support::cargo_s_report(&stderr);
        assert!(
            cargo.contains(&format!("error[E0597]: {local} does not live long enough"))
                && cargo.contains(&at),
            "{stderr}"
        );
    }
}

/// The blocks app's badge importing a native module it never uses, `use
/// crate::icon;`, beside the `use crate::page;` it does use: rustc only
/// warns, so `rawr check` passes, and `rawr run` prints what `cargo run`
/// prints. It used to be E0432, "`icon` can't be used yet: it is a module"
/// (#1436).
#[test]
fn an_unused_import_of_a_native_module_is_no_error() {
    const BLOCKS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-blocks");
    let root = crate::support::fresh("islands-blocks-icon");
    copy(Path::new(BLOCKS), &root);
    fs::write(
        root.join("src/icon.rs"),
        "//! Native: the app's icons.\n\n/// The badge's glyph.\npub fn glyph() -> char {\n    '*'\n}\n",
    )
    .unwrap();
    edit(
        &root.join("src/main.rs"),
        "pub mod page;",
        "pub mod icon;\npub mod page;",
    );
    edit(
        &root.join("src/component/badge.rs"),
        "use crate::page;",
        "use crate::icon;\nuse crate::page;",
    );
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(&root, &[]);
    for expected in String::from_utf8_lossy(&native.stdout).lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// A marked type that implements a native trait but not the trait's
/// supertrait `Debug` (#1310): `rawr check` fails with `cargo check`'s
/// E0277 at the program's own path, and with nothing of rawr's copy of the
/// crate, which fails to build for the same reason.
#[test]
fn a_marked_type_missing_a_native_supertrait_fails_with_cargo_s_error_alone() {
    const SUPERTRAIT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-supertrait"
    );
    let root = crate::support::fresh("islands-supertrait");
    copy(Path::new(SUPERTRAIT), &root);
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(!check.status.success(), "{printed}");
    assert!(printed.contains("error[E0277]"), "{printed}");
    assert!(printed.contains("src/badge.rs"), "{printed}");
    assert!(!printed.contains("RAWR-"), "{printed}");
    assert!(!printed.contains("__Rawr"), "{printed}");
    assert!(!printed.contains("target/rawr"), "{printed}");
}

/// [`a_marked_type_missing_a_native_supertrait_fails_with_cargo_s_error_alone`]
/// under `rawr run` (#1323): the run fails with `cargo check`'s E0277 and
/// nothing of rawr's copy of the crate.
#[test]
fn a_run_of_a_marked_type_missing_a_native_supertrait_fails_with_cargo_s_error_alone() {
    const SUPERTRAIT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-supertrait"
    );
    let root = crate::support::fresh("islands-supertrait-run");
    copy(Path::new(SUPERTRAIT), &root);
    let run = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(!run.status.success(), "{printed}");
    assert!(printed.contains("error[E0277]"), "{printed}");
    assert!(printed.contains("src/badge.rs"), "{printed}");
    assert!(!printed.contains("RAWR-"), "{printed}");
    assert!(!printed.contains("__Rawr"), "{printed}");
    assert!(!printed.contains("target/rawr"), "{printed}");
}

#[test]
fn a_native_main_gets_its_arguments_and_stdin_and_swaps_while_it_waits_on_stdin() {
    use std::io::Write as _;

    const ARGUMENTS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-arguments"
    );
    let root = crate::support::fresh("islands-arguments");
    copy(Path::new(ARGUMENTS), &root);
    let words = root.join("src/words.rs");
    let events = root.join("events.jsonl");
    let _ = fs::remove_file(&events);
    let mut run = crate::support::start_with(
        &root,
        &[("RAWR_MCP_EVENTS", events.clone())],
        &["one", "two words"],
        std::process::Stdio::piped(),
    );
    let mut stdin = run.child.stdin.take().unwrap();

    // `cargo run` names the program by its binary.
    let name = run.until(|line| line.starts_with("name "));
    assert!(
        name.ends_with("/target/debug/islands-arguments"),
        "{}",
        run.seen.join("\n")
    );
    run.until(|line| line == "arg ONE");
    run.until(|line| line == "arg TWO WORDS");
    stdin.write_all(b"alpha\n").unwrap();
    run.until(|line| line == "line ALPHA");

    // While the program waits on stdin, a body edit swaps in for its next
    // call.
    std::thread::sleep(Duration::from_secs(2));
    edit(&words, "text.to_uppercase()", "format!(\"{text}!\")");
    run.told(&events, |text| {
        text.contains("\"queued\"") || text.contains("\"swapped\"")
    });
    if let Err(error) = stdin.write_all(b"beta\n") {
        panic!("{error}: {}", run.seen.join("\n"));
    }
    run.until(|line| line == "line beta!");
    drop(stdin);
    run.until(|line| line == "done");
    // The program returns from `main`, which ends the run with it (#1800).
    let told = run.told(&events, |text| text.contains("\"ended\""));
    assert!(told.contains(r#""status":0"#), "{told}");
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );
}

/// The generated crate rawr builds a package's native side from sees the
/// package's files where Cargo's own build sees them (#1136): an asset
/// through `CARGO_MANIFEST_DIR`, one through a path relative to a source
/// file, one outside the package, and a build script's input and output.
/// It builds with the package's features as Cargo does (#1144): a default
/// feature gating a native function, and one that enables an optional
/// dependency with a feature of its own. Its build script at
/// `tools/build.rs` finds its modules beside it (#1143). It prints what
/// `cargo run` prints. Each file is in one target, so Cargo prints no
/// warning (#1135), and a body edit swaps in without building the native
/// crate again.
#[test]
fn a_native_crate_reads_its_package_files_where_cargo_does() {
    const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-assets");
    let project = crate::support::fresh("islands-assets");
    copy(Path::new(ASSETS), &project);
    let root = project.join("app");
    // With the stop file present, `cargo run` prints what the package read
    // and what its features enabled, then stops before its first tick.
    fs::write(root.join("stop"), "").unwrap();
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env("ISLANDS_STOP", root.join("stop"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&native.stdout).into_owned();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(
        printed,
        "logo logo bytes\nicon icon text\nshared shared note\nicons save,open\n\
         greet hello\nloud LOUD\nnever false\nmarked hello\n"
    );
    fs::remove_file(root.join("stop")).unwrap();
    let mut run = start(&root, &[("ISLANDS_STOP", root.join("stop"))]);
    for expected in printed.lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    run.until(|line| parse(line).is_some_and(|(tick, _)| tick >= 3));
    let built = run.seen.len();
    let native = native_builds(&root.join("target"));
    assert!(!native.is_empty(), "{}", run.seen.join("\n"));

    edit(
        &root.join("src/physics.rs"),
        "const SCALE: i64 = 1;",
        "const SCALE: i64 = 100;",
    );
    run.until(|line| parse(line).is_some_and(|(tick, value)| tick > 3 && value == tick * 100));
    assert_eq!(native_builds(&root.join("target")), native);
    fs::write(root.join("stop"), "").unwrap();
    let status = run.child.wait().unwrap();
    let seen = run.seen.join("\n");
    assert!(status.success(), "{status}\n{seen}");
    assert!(!seen.contains("multiple build targets"), "{seen}");
    assert!(
        !run.seen[built..]
            .iter()
            .any(|line| line.contains("restarting") || line.contains("Compiling")),
        "{seen}"
    );
}

/// Each library Cargo built for `islands-assets` under `target`, and each
/// output of its build script, with when it was written.
fn native_builds(target: &Path) -> Vec<(std::path::PathBuf, std::time::SystemTime)> {
    let mut found = Vec::new();
    let mut pending = vec![target.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap().map(Result::unwrap) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            } else if name.starts_with("libislands_assets-") && name.ends_with(".rlib")
                || name == "output" && path.to_string_lossy().contains("/islands-assets-")
            {
                found.push((path, entry.metadata().unwrap().modified().unwrap()));
            }
        }
    }
    found.sort();
    found
}

/// A workspace whose root package is a plain bin with two marked leaf
/// modules, a build script that generates its icons, and path dependencies
/// on two members and on a crate outside the workspace. One member reads an
/// asset at the workspace root; the other's build script prints no
/// `rerun-if` line, so Cargo runs it again when any file of that member
/// changes. `rawr run` prints what `cargo run` prints, a second run replays
/// without building the API again, and an edit to a file only that build
/// script reads reaches the next run as it reaches Cargo's.
#[test]
fn a_workspace_whose_member_build_script_watches_its_whole_package_runs_and_replays() {
    const WORKSPACE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-workspace"
    );
    let project = crate::support::fresh("islands-workspace");
    copy(Path::new(WORKSPACE), &project);
    let root = project.join("app");
    let cargo = || {
        let native = std::process::Command::new("cargo")
            .args(["run", "--quiet"])
            .current_dir(&root)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .output()
            .unwrap();
        assert!(
            native.status.success(),
            "{}",
            String::from_utf8_lossy(&native.stderr)
        );
        String::from_utf8_lossy(&native.stdout).into_owned()
    };
    let rawr = || {
        let run = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
            .arg("run")
            .arg(&root)
            .env("RAWR_HOME", crate::support::runtime_home())
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("RAWR_VERBOSE")
            .output()
            .unwrap();
        let printed = String::from_utf8_lossy(&run.stdout).into_owned();
        let reported = String::from_utf8_lossy(&run.stderr).into_owned();
        assert!(run.status.success(), "{printed}\n{reported}");
        (printed, reported)
    };
    let printed = cargo();
    assert_eq!(
        printed,
        "icons save,open\nlogo logo bytes\nsibling hello\n\
         panel expr = term (10 logo bytes) / hello\nbadge 9\n"
    );
    assert_eq!(rawr().0, printed);
    let (replayed, reported) = rawr();
    assert_eq!(replayed, printed);
    assert!(!reported.contains("building API"), "{reported}");

    // A file only the member's build script reads, which Cargo watches as
    // part of the member, and the root's declared input both reach the
    // next run as they reach Cargo's.
    edit(&root.join("model/grammar.txt"), "term", "factor");
    edit(&root.join("icons.txt"), "open", "close");
    let printed = cargo();
    assert_eq!(
        printed,
        "icons save,close\nlogo logo bytes\nsibling hello\n\
         panel expr = factor (10 logo bytes) / hello\nbadge 9\n"
    );
    assert_eq!(rawr().0, printed);
}

/// A native workspace keeps a marked screen's `State`, whose editor
/// content can't be cloned, between calls, as inboard keeps `home::State`
/// (#44, #1249). It gives the marked `update` messages of the marked
/// `Message` with native payloads, and polls the crate's `Task<Message>`
/// that `update` gives, whose work maps a native child's task with
/// `Message::Wizard` and runs when native code polls it, and the crate's
/// `Element<'a, Message>` that `view` maps the same way. A marked
/// `view<'a>(&'a self, inbox: &'a inbox::State) -> Element<'a, Message>`
/// and a free `view(&State) -> Element<'_, Message>` borrow the screen's
/// text, the method reading the native inbox it is lent and mapping a
/// native child's element whose message `Message::Wizard` wraps at once,
/// and native code holds two of their elements at once and shows one each
/// frame (#1473). A `match`
/// on `Message::Wizard(wizard::Message::Step(..))` is exhaustive where
/// rustc's is. The state's open wizard, an `Option` of a native state,
/// and its tabs, a `Vec` of a native type, can't be cloned: native code
/// lends them where they are, `&mut` too, so marked methods open, advance
/// and close the wizard and push, rename and close tabs, and each closes
/// once (#1475). Its panes, a `Vec` of a marked type over a native tab,
/// cross the same way. Its `view` stacks its open wizard's element, which
/// borrows the catalog it takes, over a local page with the crate's
/// `stack![page, opaque(..)]`, as inboard's does: the page is boxed before
/// the wizard's view runs, as in Rust. `rawr check` passes, `rawr run` prints what `cargo
/// run` prints, and body edits of `update` and `view` swap with the state
/// native code keeps, tabs included. The inbox the catalog borrows caches
/// in a `RefCell` and each element owns a `dyn` widget, so neither is
/// `Send` or `Sync`, as inboard's `Document` and iced's `Element` aren't:
/// the wizard's view and a header row of elements still open their frames.
/// A banner native code built once and keeps, as iced keeps a view's
/// elements until the next message, still reads the literal it borrowed
/// after a save that edits that literal swaps in, while a fresh one reads
/// the new text: the swap lets go of nothing a held result borrows.
/// A marked fn whose element borrows only native inputs is anchored on
/// them (#1491). One whose result borrows nothing keeps its lends for the
/// call, so a crate fn's output that keeps them is read where it is made,
/// a lent theme kept beside a lent wizard included (#1491, #1503), and a
/// lent slice beside a lent `&T` reaches a crate fn that keeps neither
/// (#1500). A crate method that borrows its receiver keeps a `&'a str`
/// at that lifetime, in an anchored result and read where it is made
/// (#1503). Screens whose elements were held are moved and dropped
/// (#1492). A marked fn and a marked `&mut self` method assign fields of
/// the renderer native code lends them `&mut`, in place (#1535).
#[test]
fn a_native_workspace_keeps_a_marked_state_and_polls_its_tasks() {
    const HOME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-home");
    let root = crate::support::fresh("islands-home");
    copy(Path::new(HOME), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("ISLANDS_STOP")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&native.stdout).into_owned();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert!(
        printed.contains("shown hello Some(Inbox(Read(5))) and wizard\n")
            && printed.contains("shown home Some(Wizard(Step(2)))\nview hello\n")
            && printed.ends_with(
                "view count\npressed 2: hi! after 3 edits, count 5\nno wizard over []\n\
             advanced true\nwizard at 6 over []\nwizard at 6 over [inbox, ideas, drafts]\n\
             tab inbox closed\nwizard at 6 over [ideas, drafts]\nwizard closed at step 6\n\
             wizard at 5 over [ideas, drafts]\nwizard closed at step 5\nadvanced false\n\
             no wizard over [ideas, drafts]\ntab drafts closed\ntab ideas closed\n\
             no wizard over []\npanes [left | middle | right*]\ntab middle closed\n\
             panes [left | right*]\ntab left closed\ntab right closed\npanes []\n\
             grid shows grid Some(Wizard(Step(7)))\n\
             boxed home\nwizard view at 5\nboxed hello\nboxed hello\nstacked 2\n\
             wizard shows hello Some(Wizard(Step(5)))\nwizard closed at step 5\n\
             no updates show hello Some(Inbox(Read(0)))\n\
             updates show tiles 1.2 Some(Inbox(Read(0)))\n\
             boxed tiles 1.2\nwizard view at 6\nboxed dark\nboxed dark\nstacked 2\n\
             themed wizard shows dark Some(Wizard(Step(6)))\n\
             wizard closed at step 11\ncommitted at step 11\nbuilt 3 drew 6\n\
             wizard closed at step 6\nbuilt 2 drew 10 no wizard over []\nredrew 16\n\
             banner banner Some(Inbox(Read(3))) top tiles 1.2\n\
             pending 4 top label tiles 1.2\ntitled home label mail\nwizard view at 6\nwizard label dark\n\
             wizard closed at step 6\nread one two three four\nid server-7 Some(\"7\") true false\n\
             read one true server false of 4\nserver true of 6\nmarked Some(\"9\") true of 7\n\
             panel: -a / listed: -b -c / arrayed: -b -c\nlines -d / -bb -cc\n\
             screen 0 shows wizard\nmoved shows wizard\n\
             screen 1 shows wizard\nmoved shows wizard\n"
            ),
        "{printed}"
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    // The guarded arm alone leaves `Wizard(Step(_))` out, as rustc says
    // (rustc names both `Message`s by their modules).
    let home = root.join("src/workspace/home.rs");
    let whole = fs::read_to_string(&home).unwrap();
    edit(
        &home,
        "        Message::Wizard(wizard::Message::Step(step)) => {\n            state.count += step;\n            common::Action::none()\n        }\n",
        "",
    );
    let refused = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        !refused.status.success()
            && stderr.contains("error[E0004]")
            && stderr.contains("Message::Wizard(")
            && stderr.contains("Message::Step(_))` not covered"),
        "{stderr}"
    );
    fs::write(&home, whole).unwrap();
    let mut run = start(&root, &[("ISLANDS_STOP", root.join("stop"))]);
    for expected in printed.lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    // Each tick adds `STEP` to the count the workspace keeps, its kept tab
    // stays, and it shows the screen's element.
    let tick = |line: &str| -> Option<(u32, u32, u32, String)> {
        let (tick, rest) = line.strip_prefix("tick ")?.split_once(" from ")?;
        let (from, rest) = rest.split_once(" by ")?;
        let (by, shows) = rest.split_once(" shows ")?;
        let by = by.strip_suffix(" over [kept]")?;
        Some((
            tick.parse().ok()?,
            from.parse().ok()?,
            by.parse().ok()?,
            shows.to_owned(),
        ))
    };
    let first = run.until(|line| tick(line).is_some_and(|(tick, ..)| tick >= 3));
    let (_, kept, by, shows) = tick(&first).unwrap();
    assert_eq!((by, shows.as_str()), (1, "home"), "{}", run.seen.join("\n"));
    edit(
        &root.join("src/workspace/home.rs"),
        "const STEP: u32 = 1;",
        "const STEP: u32 = 100;",
    );
    let swapped = run.until(|line| tick(line).is_some_and(|(_, _, by, _)| by == 100));
    let (_, from, ..) = tick(&swapped).unwrap();
    assert!(from > kept, "{}", run.seen.join("\n"));
    // The method's body swaps too: its element borrows other text.
    edit(
        &root.join("src/workspace/home.rs"),
        "ui::button(&self.title, Message::Count)",
        "ui::button(&self.greeting, Message::Count)",
    );
    run.until(|line| tick(line).is_some_and(|(.., shows)| shows == "hello"));
    // A literal edit swaps in while native code keeps a banner the old
    // generation built: a fresh banner reads the new text, and the kept one
    // still reads what it borrowed, never memory the swap let go.
    let banner = |line: &str| -> Option<(String, String)> {
        let (fresh, kept) = line.strip_prefix("banner ")?.split_once(" kept ")?;
        Some((fresh.to_owned(), kept.to_owned()))
    };
    edit(
        &root.join("src/workspace/home.rs"),
        "\"banner one, a heading",
        "\"banner two, a heading",
    );
    const ONE: &str = "banner one, a heading longer than an interned string";
    const TWO: &str = "banner two, a heading longer than an interned string";
    run.until(|line| banner(line).is_some_and(|(fresh, _)| fresh == TWO));
    for _ in 0..5 {
        let line = run.until(|line| banner(line).is_some());
        assert_eq!(
            banner(&line),
            Some((TWO.to_owned(), ONE.to_owned())),
            "{}",
            run.seen.join("\n")
        );
    }
    assert!(
        run.seen
            .iter()
            .filter_map(|line| banner(line))
            .all(|(_, kept)| kept == ONE),
        "{}",
        run.seen.join("\n")
    );
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );
    fs::write(root.join("stop"), "").unwrap();
    let status = run.wait();
    assert!(status.success(), "{status}\n{}", run.seen.join("\n"));
}

/// Native code calls marked functions that take an argument-position
/// `impl Trait`, as inboard's `inbox.mark_all_read(ids)`: an `impl
/// IntoIterator<Item = Id>` given a `Vec` and an iterator that borrows a
/// local, and an `impl Into<String>` given a `String` and a `&String`. It
/// reads an id's text through `Id::as_str`, as inboard's workspace reads
/// `inbox_id.as_str()`: a `&str` into the id native code holds, the same
/// text at each call, as rustc gives it. The ids read keep their ids in a
/// native `HashSet`, as inboard's `inbox::State` keeps `read_ids`, which
/// the marked methods extend, look up and add to with ids the program
/// made, hashed and compared as Rust does. A marked builder method and a
/// marked free function take an `impl` of a trait the marked file declares,
/// as inboard's `sidebar::content(impl IntoContent)`, and are given a
/// marked value by its own impl, and a native `Vec` and array by the
/// marked blanket impl over `IntoIterator<Item = String>`, as rustc selects
/// them.
/// The program builds, as `cargo check` does, and `rawr run` prints what
/// `cargo run` prints.
#[test]
fn a_native_main_gives_marked_functions_any_value_of_an_impl_trait() {
    const HOME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-home");
    let root = crate::support::fresh("islands-home-impl");
    copy(Path::new(HOME), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("ISLANDS_STOP")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&native.stdout).into_owned();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert!(
        printed.contains(
            "\nread one two three four\nid server-7 Some(\"7\") true false\n\
             read one true server false of 4\nserver true of 6\nmarked Some(\"9\") true of 7\n\
             panel: -a / listed: -b -c / arrayed: -b -c\nlines -d / -bb -cc\n"
        ),
        "{printed}"
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(&root, &[]);
    for expected in printed.lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// A literal edited in a marked screen's private function swaps into the
/// running program, its state kept, when the screen passes a native
/// function by name further down, to a crate function, to a crate method
/// and to the last step of a builder chain, as inboard's `home.rs` passes
/// `theme::text::base` to `.style` and tooltip's chain passes
/// `container::rounded_box`, and calls a crate function generic over a
/// const length. The runner names that function, never where the program
/// writes it, so the edit that moves it needs no new runner and doesn't
/// restart the program. The first edit after start and the one after it
/// both compile in the running program: neither prepares again (#1534,
/// #1536, #1029).
#[test]
fn a_literal_edit_above_a_native_function_passed_by_name_swaps() {
    const LITERAL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-literal"
    );
    let root = crate::support::fresh("islands-literal");
    let events = root.join("events.jsonl");
    let _ = fs::remove_file(root.join("stop"));
    let _ = fs::remove_file(&events);
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(LITERAL), &root);
    let mut run = start(
        &root,
        &[
            ("ISLANDS_STOP", root.join("stop")),
            ("RAWR_MCP_EVENTS", events.clone()),
        ],
    );
    // `tick n at count says greeting`.
    let tick = |line: &str| -> Option<(u32, String)> {
        let (_, rest) = line.strip_prefix("tick ")?.split_once(" at ")?;
        let (count, says) = rest.split_once(" says ")?;
        Some((count.parse().ok()?, says.to_owned()))
    };
    let first = run.until(|line| tick(line).is_some_and(|(count, _)| count >= 3));
    let (kept, _) = tick(&first).unwrap();
    let home = root.join("src/home.rs");
    edit(
        &home,
        "\"Ready when you are.\"",
        "\"Ready when you are, rawr.\"",
    );
    let swapped = run.until(|line| {
        tick(line).is_some_and(|(_, says)| says == "Ready when you are, rawr. at 12")
    });
    let (count, _) = tick(&swapped).unwrap();
    assert!(count > kept, "{}", run.seen.join("\n"));
    edit(
        &home,
        "\"Ready when you are, rawr.\"",
        "\"Ready when you are.\"",
    );
    run.until(|line| tick(line).is_some_and(|(_, says)| says == "Ready when you are. at 12"));
    let told = run.told(&events, |text| text.matches("\"swapped\"").count() >= 2);
    assert!(!told.contains("\"preparing\""), "{told}");
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );
    fs::write(root.join("stop"), "").unwrap();
    run.told(&events, |text| {
        text.contains("{\"event\":\"ended\",\"status\":0}")
    });
}

/// A marked home screen with inboard's shapes (`workspace/screen/home.rs`):
/// a crate's `&mut self` trait method on the generator a call gives by
/// value (`rand::rng().random_range(0..100)`), a trait method of a clock a
/// call gives (`chrono::Local::now().hour()`), `&self` methods of the
/// elements of a slice a native method lends, through a `Vec<&Entry>`
/// sorted by `Reverse`, a drop-down built by value, a boxed field moved
/// out of a native struct variant, a native tuple of a value and a task,
/// an `impl Into<I>` the screen's own enum fills, a text editor's content
/// read and replaced, or-patterns of native payloads, the `&str` of a
/// chain of a lent config's borrows, and a native tuple of counts.
/// `rawr check` passes and `rawr run` prints what `cargo run` prints.
#[test]
fn a_marked_home_screen_with_inboard_s_shapes_runs_as_cargo_runs_it() {
    const GREETING: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-greeting"
    );
    let root = crate::support::fresh("islands-greeting");
    copy(Path::new(GREETING), &root);
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&native.stdout).into_owned();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert!(
        printed.starts_with("Afternoon, Ada. How can I help?\n")
            && printed.contains("[team (30m ago) on right opens Leave] [notes (60m ago)")
            && printed.contains("Some(Commit { target: Replace(2), infolet: Infolet"),
        "{printed}"
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(&root, &[]);
    for expected in printed.lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// What `cargo run` prints for the project at `root`, which must succeed.
fn cargo_prints(root: &Path) -> String {
    let native = std::process::Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    String::from_utf8_lossy(&native.stdout).into_owned()
}

/// `rawr check` of the project at `root`, which must pass, and its `rawr
/// run`, which must print `printed`, line by line.
fn checks_and_prints(root: &Path, printed: &str) {
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let mut run = start(root, &[]);
    for expected in printed.lines() {
        let line = run.until(|_| true);
        assert_eq!(line, expected, "{}", run.seen.join("\n"));
    }
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// A marked module uses the pieces of `log`'s and `tracing`'s macros, and
/// `rawr check` passes and `rawr run` prints what `cargo run` prints. It
/// compares their levels as the macros' enabled checks do, a `Level` with a
/// `LevelFilter` through the crate's `PartialOrd<LevelFilter>` and
/// `PartialEq<LevelFilter>`, as rustc selects them (#702). It names std's
/// macros by their paths and through a crate's re-export of them, as
/// `$crate::__private_api::module_path!()` (#1548). A crate macro's
/// `line!()`, `column!()`, `file!()` and `module_path!()`, nested or not,
/// are where its outermost call is, a caller's own `line!()` is where the
/// caller wrote it, and `module_path!()` in a constant or an inline module's
/// impl is cargo's. A marked value native code is lent as a `&dyn Debug`,
/// `&dyn Display` or `&dyn LowerHex` is written by its type's own impl,
/// marked or derived, when native code formats it, and `format_args!` is
/// the `fmt::Arguments` a crate function takes. A re-export of std's macro
/// a crate's macro names only inside `concat!` is asked for as any other
/// path, and `$($e:expr),* $(,)*` reads `"message", ,` as rustc does, as
/// tracing's callsite name and `fieldset!` need. Real `tracing::warn!` and
/// `info!` events, one through a crate's re-export of `warn!` as inboard's
/// `log::warn!` is, reach native code's subscriber as cargo's build gives
/// them: `format_args!` is `fmt::Arguments` coerced to `&dyn Value`, made in
/// the frame that lends the event's values (#1561), and `__CALLSITE.
/// metadata()` is a lasting place of the static (#1602). So does
/// `level_enabled!`'s check (#1603).
#[test]
fn a_marked_module_uses_log_s_and_tracing_s_macro_pieces() {
    const LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-log");
    let root = crate::support::fresh("islands-log");
    copy(Path::new(LOG), &root);
    let printed = cargo_prints(&root);
    assert!(
        printed.starts_with(
            "compiled true current true off true filter false equal true unequal true \
             same true log true\n\
             ab1 islands_log::workspace::inbox event x34 ab1 2\n\
             src/workspace/inbox.rs:42:5 in islands_log::workspace::inbox\n"
        ) && printed.contains("; islands_log::workspace::inbox::folder islands_log::")
            && printed.contains(
                "Message { subject: \"hello\", unread: true }; [hello*]; count=2 (Some(\"two\")); \
                 [Some(Ok(Message { subject: \"old\", unread: false })), None]; 0xff; \
                 <hello has 2>; unread\n\
                 event src/workspace/inbox.rs:142 [\"message\"]\n\
                 WARN islands_log::workspace::inbox line Some(126): message=3 unread in \"hello\"\n\
                 INFO islands_log::workspace::inbox line Some(127): message=read 2\n\
                 enabled true; a 3 [b] c 3\n"
            ),
        "{printed}"
    );
    checks_and_prints(&root, &printed);
}

/// A marked module logs as inboard's inbox does, through its own `log`
/// crate's re-export of tracing's `warn!` inside an `unwrap_or_else`
/// closure, and names neither tracing nor tracing-core, which only that
/// crate reaches. The macro's level check, `Level <= LevelFilter`, misses
/// `tracing_core::Level::le` as rustc names the type, and the member is
/// demanded under the `log` crate's hidden segment for tracing-core, the
/// one root that reaches it.
#[test]
fn a_marked_module_logs_through_a_crate_that_re_exports_tracing_s_macros() {
    const THROUGH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-log-through"
    );
    let root = crate::support::fresh("islands-log-through");
    copy(Path::new(THROUGH), &root);
    let printed = cargo_prints(&root);
    assert_eq!(
        printed,
        "WARN islands_log_through::workspace::notice line Some(21): message=bad created_at \
         \"yesterday\": input contains invalid characters\n\
         timestamp 0\n"
    );
    checks_and_prints(&root, &printed);
}

/// A marked module calls a workspace crate's method whose lifetimes are
/// elided, as inboard's inbox calls `Organization::user(&self, id: &Id) ->
/// Option<&User>`. rustc reads them as late-bound, `for<'a, 'b> fn(&'a
/// Self, &'b Id) -> Option<&'a User>`, and they are the call's own
/// lifetimes: the output borrows the receiver, and the id is lent for the
/// call. `rawr check` passes and `rawr run` prints what `cargo run` prints.
#[test]
fn a_marked_module_calls_a_method_with_elided_lifetimes_whose_output_borrows_self() {
    const ORG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-org");
    let root = crate::support::fresh("islands-org");
    copy(Path::new(ORG), &root);
    let printed = cargo_prints(&root);
    assert_eq!(printed, "invited by Ada\ninvited by Someone\n");
    checks_and_prints(&root, &printed);
}

/// The packages `cargo build` of `root` compiles, by the names its
/// `Compiling` lines give.
fn cargo_compiles(root: &Path) -> Vec<String> {
    let built = std::process::Command::new("cargo")
        .arg("build")
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&built.stderr);
    assert!(built.status.success(), "{said}");
    said.lines()
        .filter_map(|line| line.trim_start().strip_prefix("Compiling "))
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

/// `rawr-import run root`'s count of runner units compiled.
fn runner_units(root: &Path) -> String {
    let run = std::process::Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env("RAWR_VERBOSE", "1")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "{said}");
    said.lines()
        .find_map(|line| line.split_once("runner units compiled "))
        .map(|(_, count)| count.to_owned())
        .unwrap_or_else(|| panic!("{said}"))
}

/// A private field on a held `pub struct` rebuilds what names the program
/// and nothing else, and rawr and `cargo build` share the project's path
/// packages. The runner's base and its crates of adapters don't depend on
/// rawr's copy of the program's crate, so the field recompiles that copy
/// and the runner's binary, which holds what names the program (#1775,
/// #1793). rawr's copy names the path packages through the runner's links,
/// so they are one unit each, which `cargo build` after a run and a run
/// after `cargo build` find fresh (#1777).
#[test]
fn a_held_type_s_new_field_rebuilds_two_runner_units_and_cargo_shares_the_path_packages() {
    const SOURCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-theme");
    let root = crate::support::fresh("islands-theme-field-units");
    copy(Path::new(SOURCE), &root);
    runner_units(&root);
    // The project's own crate is all `cargo build` compiles after a run.
    assert_eq!(cargo_compiles(&root), ["islands-theme"]);
    assert_eq!(runner_units(&root), "0");
    let home = root.join("src/home.rs");
    edit(
        &home,
        "    wizard: Option<wizard::State>,\n}",
        "    wizard: Option<wizard::State>,\n    seen: u32,\n}",
    );
    edit(
        &home,
        "            wizard: None,\n        }",
        "            wizard: None,\n            seen: 0,\n        }",
    );
    assert_eq!(runner_units(&root), "2");
    assert_eq!(cargo_compiles(&root), ["islands-theme"]);
    assert_eq!(runner_units(&root), "0");
}
