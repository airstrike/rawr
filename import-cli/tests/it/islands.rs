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
/// Selected(palette::Theme) }`: a value crosses with its native part given
/// as a clone and taken back as the value the interpreter moves, and
/// neither `palette::Theme` nor the crate's `ui::Theme` is the marked one
/// (#1466). Its impls of `Default`, `From` from the native and the crate's
/// theme, `From<Theme>` for the crate's theme, the crate's `Design` trait
/// and `From<&Theme>` for the crate's `Settings` are native code's, each
/// function a trampoline, and a native child module implements the
/// crate's `Catalog` for it by matching its variant. `rawr check` passes as
/// `cargo check` does, and `rawr run` prints what `cargo run` prints.
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
