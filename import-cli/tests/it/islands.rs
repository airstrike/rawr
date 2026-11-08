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
/// `&mut`, which writes back (#1139). `rawr run` prints what `cargo run`
/// prints.
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
/// native struct as the struct crosses (#1272).
/// `rawr check` passes as `cargo check` does, and `rawr run` prints what
/// `cargo run` prints.
#[test]
fn a_marked_module_names_the_items_of_a_native_module() {
    const MODULE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/islands-native-module"
    );
    const PRINTED: [&str; 7] = ["home", "home", "about", "31", "41", "10", "home"];
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

/// A marked leaf writes its page as a native module's enum, whose fields
/// are `&'static str`, `Option<&'static str>` and `&'static [Block]`, from
/// literals: a `pub static BLOCKS` native code reads as a Rust item, and
/// functions that give a built block back by value, lend built blocks to
/// a native function, give one to it by value and read `BLOCKS` themselves
/// (#1283). `rawr check` passes as `cargo check` does, and `rawr run`
/// prints what `cargo run` prints.
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
        printed.ends_with("Count\n3\n  md A badge marks a count.\n3\n"),
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
