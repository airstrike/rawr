//! A marked `#[tokio::main] async fn main` run by the real `rawr-import`
//! binary on the project's own tokio (D513): the shadow crate's entry carries
//! the attribute as written, script futures are std futures, a channel
//! carries a script struct, and a save restarts the program. Build the
//! driver first, as for `plain`.

use std::fs;
use std::process::Command;

use crate::support::{start, Fixture};

static ASYNC: Fixture = Fixture::new("tokio-async");
static BORROWED: Fixture = Fixture::new("tokio-borrowed");
static TICKS: Fixture = Fixture::new("tokio-ticks");
static SPAWN: Fixture = Fixture::new("tokio-spawn");
static DERIVES: Fixture = Fixture::new("tokio-derives");

/// `job N at T ms: ..`: `N` and `T`.
fn job(line: &str) -> Option<(u64, u64)> {
    let rest = line.strip_prefix("job ")?;
    let (id, rest) = rest.split_once(" at ")?;
    let (millis, _) = rest.split_once(" ms")?;
    Some((id.parse().ok()?, millis.parse().ok()?))
}

#[test]
fn an_async_main_runs_on_the_projects_tokio_with_a_channel_join_and_spawn_blocking() {
    let root = ASYNC.lease(&[]);
    let directory = crate::support::scratch();
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 4, "{stdout}");
    // Each job arrives after its producer's real 100 ms sleeps, in order,
    // carrying the script struct its `Debug` prints.
    for (index, line) in lines[..3].iter().enumerate() {
        let id = index as u64 + 1;
        let (got, millis) = job(line).unwrap_or_else(|| panic!("{stdout}"));
        assert_eq!(got, id, "{stdout}");
        assert!(millis >= id * 100, "{stdout}");
        assert!(
            line.ends_with(&format!("Job {{ id: {id}, name: \"job {id}\" }}")),
            "{stdout}"
        );
    }
    assert_eq!(lines[3], "total 6, 303 primes");
    // The runner links the project's tokio, with no iced.
    let runner = fs::read_to_string(root.join("target/rawr/runner/Cargo.toml")).unwrap();
    assert!(runner.contains("tokio = "), "{runner}");
    assert!(!runner.contains("iced"), "{runner}");
    // The shadow crate's entry carries the attribute as written.
    let entry = fs::read_to_string(root.join("target/rawr/crate/lib.rs")).unwrap();
    assert!(
        entry.contains("    #[tokio::main]\n    async fn main() -> i32 {"),
        "{entry}"
    );
}

/// A crate's `async fn` that borrows its argument, as fount's `load(font:
/// &Font)`, leases the value to its future for the future's life, shared
/// or mutably (D683.2), and the value is whole again after the await. A
/// future borrows through any input type, as fount's `google::load(family:
/// &str, catalog: Option<&Catalog>)` does, with `Some` and with `None`, and
/// through a tuple of borrows (#677).
#[test]
fn an_async_fn_borrowing_its_argument_leases_it_to_its_future() {
    let root = BORROWED.lease(&[]);
    let directory = crate::support::scratch();
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout,
        "Inter 3 3\nInter listed; Georgia uncataloged; Inter unlisted of 1; 9\n"
    );
}

/// A program module's function names another crate's type as rustc
/// resolves a path into a dependency, through that crate: `label::describe`
/// of a program's own `mod label;` takes `&shelf::Font`, `label::owned`
/// takes one by value and `label::make` gives one (#685). `cargo run`
/// prints the same.
#[test]
fn a_module_function_names_another_crates_type() {
    let root = BORROWED.lease(&[]);
    fs::write(
        root.join("src/label.rs"),
        "pub fn describe(font: &shelf::Font) -> String {\n    format!(\"{} of {}\", font.family, font.bytes.len())\n}\n\npub fn owned(font: shelf::Font) -> String {\n    font.family\n}\n\npub fn make(family: &str) -> shelf::Font {\n    shelf::font(String::from(family), 2)\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        "// rawr\nmod label;\n\nfn main() {\n    let font = shelf::font(String::from(\"Georgia\"), 3);\n    println!(\"{}\", label::describe(&font));\n    let made = label::make(\"Inter\");\n    println!(\"{}\", label::describe(&made));\n    println!(\"{}\", label::owned(made));\n}\n",
    )
    .unwrap();
    let directory = crate::support::scratch();
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(stdout, "Georgia of 3\nInter of 2\nInter\n");
}

/// A crate's `async fn` whose future borrows std's types and Rust's own
/// leases each borrow to the future, whatever the type (#714): `&Path` and
/// `&[u8]`, `&String`, `&Vec<u32>`, `&u32`, and a `Result<&Font, String>`
/// with either variant. Each value is whole again after the await, and the
/// output is `cargo run`'s.
#[test]
fn an_async_fn_leases_every_borrow_its_inputs_hold() {
    let store = BORROWED.read("variants/store.rs");
    let root = BORROWED.lease(&[("src/main.rs", &store)]);
    let directory = crate::support::scratch();
    let output = |command: &mut Command| {
        let run = command
            .current_dir(&root)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("RAWR_VERBOSE")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
        stdout
    };
    let rawr = output(
        Command::new(env!("CARGO_BIN_EXE_rawr-import"))
            .arg("run")
            .arg(&*root)
            .env("RAWR_HOME", directory.join("rawr-home")),
    );
    let cargo = output(Command::new("cargo").args(["run", "-q"]));
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "3 bytes to out/notes.txt\nQUIET\n10\n42\nInter\nmissing\nout/notes.txt quiet 4 21 Inter\n"
    );
}

#[test]
fn a_save_restarts_an_async_main_on_its_runtime_as_its_attribute_says() {
    let root = TICKS.lease(&[]);
    let main = root.join("src/main.rs");
    let mut run = start(&root, &[]);

    // The first run builds the runner, on a current-thread runtime.
    let first = run.until(|line| line == "version 1 tick 3");
    assert_eq!(first, "version 1 tick 3");
    let entry = fs::read_to_string(root.join("target/rawr/crate/lib.rs")).unwrap();
    assert!(
        entry.contains("#[tokio::main(flavor = \"current_thread\")]"),
        "{entry}"
    );

    let text = fs::read_to_string(&main).unwrap();
    fs::write(
        &main,
        text.replace("const VERSION: i64 = 1;", "const VERSION: i64 = 2;"),
    )
    .unwrap();
    let restarted = run.until(|line| line.starts_with("version 2"));
    assert_eq!(restarted, "version 2 tick 1");
    assert!(
        run.seen
            .iter()
            .any(|line| line == "stderr: rawr: the source changed; restarting the program"),
        "{}",
        run.seen.join("\n")
    );
}

/// `rawr-import run` on the `tokio-spawn` fixture with `attribute` in place
/// of its `#[tokio::main]`: its whole output.
fn spawned(attribute: &str) -> String {
    let main = SPAWN
        .read("src/main.rs")
        .replace("#[tokio::main]", attribute);
    let root = SPAWN.lease(&[("src/main.rs", &main)]);
    let directory = crate::support::scratch();
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    assert!(
        run.status.success(),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    stdout
}

/// D527's falsifying test: the owner thread runs the runtime entry, so a
/// spawned script future runs though nobody awaits it, `.await` on its
/// handle gives its value, and an unawaited `spawn_blocking` runs, under
/// both of tokio's runtimes.
#[test]
fn a_spawned_script_future_runs_unawaited_and_its_handle_gives_its_value() {
    for attribute in [
        "#[tokio::main]",
        "#[tokio::main(flavor = \"current_thread\")]",
    ] {
        assert_eq!(
            spawned(attribute),
            "ran\njoined 42\nblocking ran\n",
            "{attribute}"
        );
    }
}

/// Program structs a crate decodes on one of tokio's worker threads, as
/// reqwest's `Response::json()` decodes iced's pokedex's `Entry` (D740.3):
/// nested `Deserialize` structs with a `Vec` field, defined in an async
/// fn's body, whose visitors run off the owner thread, and the error one
/// of them gives. The output is `cargo run`'s.
#[test]
fn a_crate_decodes_program_structs_on_a_worker_thread() {
    let root = DERIVES.lease(&[]);
    let directory = crate::support::scratch();
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout,
        "Err(\"invalid type: integer `1`, expected a string at line 1 column 9\")\n\
         Ok(\"BULBASAUR A strange seed true\")\n"
    );
}

/// futures' `try_join(fetch, size(..)).await?` of an async block and an
/// async fn's future, as iced's pokedex's `search` joins them (#1674):
/// `Fut1: TryFuture` takes a script future, as the trait's one blanket
/// impl `impl<F, T, E> TryFuture for F where F: Future<Output = Result<T,
/// E>>` reads it, and `TryJoin`'s `Output` names `Fut1::Ok` and
/// `Fut1::Error`. The output is `cargo run`'s.
#[test]
fn futures_try_join_takes_script_futures_through_its_blanket_trait() {
    let program = DERIVES.read("joined.rs");
    let root = DERIVES.lease(&[("src/main.rs", &program)]);
    let cargo = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&*root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let expected = String::from_utf8_lossy(&cargo.stdout);
    assert!(
        cargo.status.success(),
        "{}",
        String::from_utf8_lossy(&cargo.stderr)
    );
    assert_eq!(
        expected,
        "Ok(\"BULBASAUR 4\")\n\
         Err(\"invalid type: integer `1`, expected a string at line 1 column 1\")\n"
    );
    let directory = crate::support::scratch();
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(stdout, expected);
}
