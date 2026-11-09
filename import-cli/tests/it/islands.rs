//! A program whose `main` is native Rust and calls a marked module (D501),
//! run by the real `rawr-import` binary while its marked file is edited: a
//! body edit reaches the running program at its next call, and a struct's
//! new field restarts it with a one-line reason. Build the driver first, as
//! for `plain`.

use std::fs;
use std::path::Path;
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
    let root = crate::support::scratch().join("islands");
    let _ = fs::remove_file(root.join("stop"));
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
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

/// A package whose `target` holds a big subtree that git ignores, as a
/// real app's does: the watcher stands for the program's inputs, not the
/// tree, so a save of the marked file still reaches the running program,
/// and writes under `target` while it runs are no save.
#[test]
fn a_save_reaches_a_native_main_beside_a_big_ignored_target() {
    let root = crate::support::scratch().join("islands-big-target");
    let _ = fs::remove_dir_all(&root);
    copy(Path::new(FIXTURE), &root);
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
    let root = crate::support::scratch().join("islands-idle");
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
    let root = crate::support::scratch().join("islands-admit");
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

    // A supervised run waits for the next save once its program ends.
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
    let root = crate::support::scratch().join("islands-redraw");
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
    let root = crate::support::scratch().join("islands-main-thread");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-names");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-natives");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-root");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-native-module");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-aliases");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-nested");
    let _ = fs::remove_file(root.join("stop"));
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-cfg");
    let _ = fs::remove_dir_all(root.join("src"));
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
/// go at its next `update` (#1321). `rawr check` passes as `cargo check` does, and
/// `rawr run` prints what `cargo run` prints.
#[test]
fn a_marked_type_becomes_a_native_trait_object() {
    const OBJECTS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-objects"
    );
    let root = crate::support::scratch().join("islands-objects");
    let _ = fs::remove_dir_all(root.join("src"));
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
/// keeps. A marked `Sidebar<'a, Message>`, as inboard's, bounds its
/// generic impl's `content` by the crate's `IntoContent<'a, Message>` over
/// the impl's own parameters, and a non-generic marked fn calls it. `rawr
/// check` passes as `cargo check` does, and `rawr run` prints what `cargo
/// run` prints.
#[test]
fn a_marked_theme_over_a_native_theme_converts_and_implements_crate_traits() {
    const THEME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-theme");
    let root = crate::support::scratch().join("islands-theme");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(THEME), &root);
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
        printed.contains("void styled 0 idle\nhome badge on dark\ninbox inbox styled 120 Some(Inbox(Opened(120)))\ntitled mail styled 254 idle\npage grid styled 121 Some(Grid(Tile(120)))\n"),
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
    assert!(printed.contains("\nsidebar 2\n"), "{printed}");
    // The one clone is `theme.clone()` in `main`.
    assert!(printed.ends_with("\nclones 1\n"), "{printed}");
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
    let root = crate::support::scratch().join("islands-consts");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-blocks");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-blocks-local");
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
        let at = format!("badge.rs:{line}:{column}-");
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
        // Cargo's lock notice comes first when another build holds the
        // directory; the report under test follows it.
        let stderr = stderr
            .lines()
            .skip_while(|line| {
                line.trim_start()
                    .starts_with("Blocking waiting for file lock")
            })
            .collect::<Vec<_>>()
            .join("\n");
        // rawr's own span, which runs to the borrow's end, before cargo
        // check's report.
        assert!(
            !check.status.success()
                && stderr.starts_with(&format!("error[E0597]: {local} does not live long enough"))
                && stderr.contains(&at),
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
    let root = crate::support::scratch().join("islands-blocks-icon");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-supertrait");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-supertrait-run");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-arguments");
    let _ = fs::remove_dir_all(root.join("src"));
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
    // Supervised, the run outlives its program, which a save would run again.
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
    let project = crate::support::scratch().join("islands-assets");
    let _ = fs::remove_dir_all(project.join("app/src"));
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
    let project = crate::support::scratch().join("islands-workspace");
    let _ = fs::remove_dir_all(&project);
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
    let root = crate::support::scratch().join("islands-home");
    let _ = fs::remove_file(root.join("stop"));
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-home-impl");
    let _ = fs::remove_dir_all(root.join("src"));
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
    let root = crate::support::scratch().join("islands-literal");
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
    let root = crate::support::scratch().join("islands-greeting");
    let _ = fs::remove_dir_all(root.join("src"));
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
/// the `fmt::Arguments` a crate function takes.
#[test]
fn a_marked_module_uses_log_s_and_tracing_s_macro_pieces() {
    const LOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-log");
    let root = crate::support::scratch().join("islands-log");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(LOG), &root);
    let printed = cargo_prints(&root);
    assert!(
        printed.starts_with(
            "compiled true current false off true filter false equal true unequal true \
             same true log true\n\
             ab1 islands_log::workspace::inbox event x34 ab1 2\n\
             src/workspace/inbox.rs:42:5 in islands_log::workspace::inbox\n"
        ) && printed.contains("; islands_log::workspace::inbox::folder islands_log::")
            && printed.contains(
                "Message { subject: \"hello\", unread: true }; [hello*]; count=2 (Some(\"two\")); \
                 [Some(Ok(Message { subject: \"old\", unread: false })), None]; 0xff; \
                 <hello has 2>; unread\n"
            ),
        "{printed}"
    );
    checks_and_prints(&root, &printed);
}
