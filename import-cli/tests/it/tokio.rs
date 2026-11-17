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
    let run = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
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
    let run = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
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
    let run = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
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
        crate::support::rawr_import()
            .arg("run")
            .arg(&*root)
            .env("RAWR_HOME", crate::support::runtime_home()),
    );
    let cargo = output(Command::new("cargo").args(["run", "-q"]));
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "3 bytes to out/notes.txt\nQUIET\n10\n42\nInter\nmissing\nout/notes.txt quiet 4 21 Inter\n"
    );
}

/// A program's `async fn` that takes `&mut` parameters holds each borrow
/// of the caller's place in its future across its awaits (#176): a `Vec`,
/// a program struct through a free function and its own `&mut self`
/// method, and a crate's connection whose `&mut self` async method the
/// future calls, each called twice. The output is `cargo run`'s.
#[test]
fn an_async_fn_holds_its_mut_parameters_across_its_awaits() {
    let mutable = BORROWED.read("variants/mutable.rs");
    let root = BORROWED.lease(&[("src/main.rs", &mutable)]);
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
        crate::support::rawr_import()
            .arg("run")
            .arg(&*root)
            .env("RAWR_HOME", crate::support::runtime_home()),
    );
    let cargo = output(Command::new("cargo").args(["run", "-q"]));
    assert_eq!(rawr, cargo);
}

/// An async `main` whose script code runs only as the steps its runtime
/// polls keeps running across a save to a function its loop calls: the
/// save is taken where the loop resumes, with its count kept (#2127). An
/// edit to `main`'s loop restarts it, saying why.
#[test]
fn an_async_main_takes_a_save_where_its_loop_resumes_and_restarts_for_its_own_edit() {
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
    let swapped = text.replace("    1\n}", "    2\n}");
    fs::write(&main, &swapped).unwrap();
    let kept = run.until(|line| line.starts_with("version 2"));
    let tick = kept
        .strip_prefix("version 2 tick ")
        .and_then(|tick| tick.parse::<u64>().ok())
        .unwrap();
    assert!(tick > 3, "{}", run.seen.join("\n"));
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );

    fs::write(&main, swapped.replace("tick += 1;", "tick += 1000;")).unwrap();
    let restarted = run.until(|line| line == "version 2 tick 1000");
    assert_eq!(restarted, "version 2 tick 1000");
    assert!(
        run.seen
            .iter()
            .any(|line| line == "stderr: rawr: `main` changed; restarting the program"),
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
    let run = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
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

/// A crate's type that implements `Future` is a value of that type, as
/// rustc holds it (#2132): tokio's `JoinHandle<T>` keeps `abort` and
/// `is_finished`, a program's function names it as its output, `Sleep`
/// keeps `is_elapsed`, and each is awaited, or joined by `join!`, as the
/// core's future of its `Output`, a script struct's among them. The output
/// is `cargo run`'s.
#[test]
fn a_crates_future_type_keeps_its_methods_and_is_awaited() {
    let main = r#"// rawr
use std::time::Duration;

#[derive(Debug)]
struct Point {
    x: i32,
    label: String,
}

fn blocking() -> tokio::task::JoinHandle<bool> {
    tokio::task::spawn_blocking(|| true)
}

#[tokio::main]
async fn main() {
    let slow = tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(5)).await;
        1
    });
    println!("finished {}", slow.is_finished());
    slow.abort();
    println!("cancelled {}", slow.await.unwrap_err().is_cancelled());
    println!("blocking {}", blocking().await.unwrap());
    let point = tokio::spawn(async { Point { x: 3, label: String::from("p") } });
    let point = point.await.unwrap();
    println!("point {} {} {:?}", point.x, point.label, point);
    let sleep = tokio::time::sleep(Duration::from_millis(1));
    println!("elapsed {}", sleep.is_elapsed());
    sleep.await;
    let (a, b) = tokio::join!(tokio::spawn(async { 1 }), tokio::spawn(async { 2 }));
    println!("joined {} {}", a.unwrap(), b.unwrap());
}
"#;
    let root = SPAWN.lease(&[("src/main.rs", main)]);
    let run = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
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
        "finished false\ncancelled true\nblocking true\n\
         point 3 p Point { x: 3, label: \"p\" }\nelapsed false\njoined 1 2\n"
    );
}

/// `rawr-import run` on the `tokio-spawn` fixture with `main` as its
/// `src/main.rs`: its whole output.
fn ran(main: &str) -> String {
    let root = SPAWN.lease(&[("src/main.rs", main)]);
    let run = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
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

/// A crate future anywhere in what a call gives is awaited as one (#2135):
/// a `JoinHandle<i32>` in the `Option` a program's function gives, in a
/// `Vec` that `collect` makes, out of a channel's `recv`, and in the
/// `Option<Result<..>>` of `JoinSet::join_next`. The output is `cargo
/// run`'s.
#[test]
fn a_crate_future_nested_in_what_a_call_gives_is_awaited() {
    let main = r#"// rawr
use tokio::task::JoinHandle;

fn maybe(spawn: bool) -> Option<JoinHandle<i32>> {
    spawn.then(|| tokio::spawn(async { 7 }))
}

#[tokio::main]
async fn main() {
    println!("some {}", maybe(true).unwrap().await.unwrap());
    let handles: Vec<JoinHandle<i32>> = (0..3).map(|n| tokio::spawn(async move { n * 2 })).collect();
    let mut total = 0;
    for handle in handles {
        total += handle.await.unwrap();
    }
    println!("total {total}");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<JoinHandle<i32>>(1);
    tx.send(tokio::spawn(async { 5 })).await.unwrap();
    println!("received {}", rx.recv().await.unwrap().await.unwrap());
    let mut set = tokio::task::JoinSet::new();
    set.spawn(async { 11 });
    println!("set {}", set.join_next().await.unwrap().unwrap());
}
"#;
    assert_eq!(ran(main), "some 7\ntotal 6\nreceived 5\nset 11\n");
}

/// `tokio::time::timeout(d, f)` over `F: IntoFuture` gives `Timeout<
/// F::IntoFuture>`, which core's `impl<F: Future> IntoFuture for F` makes
/// `Timeout<F>` (#2136): of an async block, of `Sleep` and of a
/// `JoinHandle`, each awaited as a `Result` of its future's output. The
/// output is `cargo run`'s.
#[test]
fn timeout_of_a_script_or_crate_future_is_awaited() {
    let main = r#"// rawr
use std::time::Duration;

#[tokio::main]
async fn main() {
    let quick = tokio::time::timeout(Duration::from_secs(5), async { 3 }).await;
    println!("quick {:?}", quick);
    let slow = tokio::time::timeout(Duration::from_millis(10), tokio::time::sleep(Duration::from_secs(5))).await;
    println!("slow {}", slow.is_err());
    let handle = tokio::spawn(async { 4 });
    let joined = tokio::time::timeout(Duration::from_secs(5), handle).await;
    println!("joined {}", joined.unwrap().unwrap());
}
"#;
    assert_eq!(ran(main), "quick Ok(3)\nslow true\njoined 4\n");
}

/// `select!` of several branches gives the body of the first future to
/// end, whose pattern binds its output, and drops the others (#1152): a
/// `Sleep` against a channel's `recv`, two sleeps, and a `biased;` select
/// in a loop with an `else`. The output is `cargo run`'s.
#[test]
fn select_of_several_futures_runs_the_first_to_end() {
    let main = r#"// rawr
use std::time::Duration;

#[tokio::main]
async fn main() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(4);
    tx.send(9).await.unwrap();
    let got = tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(5)) => -1,
        value = rx.recv() => value.unwrap(),
    };
    println!("got {got}");
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(5)) => println!("slept"),
        _ = tokio::time::sleep(Duration::from_secs(5)) => println!("late"),
    }
    tx.send(1).await.unwrap();
    tx.send(2).await.unwrap();
    drop(tx);
    let mut sum = 0;
    loop {
        tokio::select! {
            biased;
            Some(value) = rx.recv() => sum += value,
            else => break,
        }
    }
    println!("sum {sum}");
}
"#;
    assert_eq!(ran(main), "got 9\nslept\nsum 3\n");
}

/// A `select!` branch's precondition (`, if condition`) is evaluated before
/// any future is polled, and a false one disables the branch, whose future
/// is never polled (#2140): a ready branch left out, a lone branch reaching
/// `else`, and a loop that stops receiving once its channel ends. The
/// output is `cargo run`'s.
#[test]
fn select_skips_a_branch_whose_precondition_is_false() {
    let main = r#"// rawr
use std::cell::Cell;
use std::time::Duration;

async fn counted(polls: &Cell<u32>, value: i32) -> i32 {
    polls.set(polls.get() + 1);
    value
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let polls = Cell::new(0);
    let ready = false;
    let picked = tokio::select! {
        v = counted(&polls, 1), if ready => v,
        v = async { tokio::time::sleep(Duration::from_millis(5)).await; 2 } => v,
    };
    println!("{picked} {}", polls.get());
    let picked = tokio::select! {
        v = counted(&polls, 8), if ready => v,
        else => 9,
    };
    println!("{picked} {}", polls.get());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(4);
    tx.send(1).await.unwrap();
    tx.send(2).await.unwrap();
    drop(tx);
    let (mut open, mut sum, mut ticks) = (true, 0, 0);
    loop {
        tokio::select! {
            biased;
            got = rx.recv(), if open => match got {
                Some(n) => sum += n,
                None => open = false,
            },
            _ = tokio::time::sleep(Duration::from_millis(1)), if ticks < 2 => ticks += 1,
            else => break,
        }
    }
    println!("{sum} {ticks} {open}");
}
"#;
    assert_eq!(ran(main), "2 0\n9 0\n3 2 false\n");
}

/// Program structs a crate decodes on one of tokio's worker threads, as
/// reqwest's `Response::json()` decodes iced's pokedex's `Entry` (D740.3):
/// nested `Deserialize` structs with a `Vec` field, defined in an async
/// fn's body, whose visitors run off the owner thread, and the error one
/// of them gives. The output is `cargo run`'s.
#[test]
fn a_crate_decodes_program_structs_on_a_worker_thread() {
    let root = DERIVES.lease(&[]);
    let run = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
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
    let run = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
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

/// iced's pokedex's run with a local server in reqwest's shape (#1018):
/// `Task::perform` runs `Pokemon::search()` on one of tokio's workers,
/// `try_join` joins `get(&url).await?.json().await` into nested
/// `Deserialize` structs defined in the fn's body with `fetch_image`'s
/// `Result<Handle, wire::Error>`, `rand`'s `OsRng` picks the id, and a
/// failed request becomes the program's `Error` through its `From` impl.
/// The output is `cargo run`'s.
#[test]
fn pokedex_searches_on_a_worker_thread_as_cargo_runs_it() {
    let program = DERIVES.read("pokedex.rs");
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
        "Loading - Pokédex\n\
         BULBASAUR - Pokédex\n\
         true A strange seed was planted on its back at birth.\n\
         Handle(Bytes([137, 80, 78, 71]))\n\
         Loading - Pokédex\n\
         PokemonFound(Err(APIError))\n\
         Whoops! - Pokédex\n"
    );
    let run = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(stdout, expected);
    // `dbg!` in the `From` impl shows the crate's error.
    assert!(stderr.contains("error sending request for url"), "{stderr}");
}

/// A crate's type the program never names, `wire::Bytes` of
/// `wire::get(..).await?.bytes().await?`, has the `Debug` its crate
/// derives: the check demands the type, which brings its impls (#1018).
#[test]
fn an_unnamed_crate_type_from_a_future_has_its_debug() {
    let program = DERIVES.read("unnamed.rs");
    let root = DERIVES.lease(&[("src/main.rs", &program)]);
    let run = crate::support::rawr_import()
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(stdout, "Ok(\"Bytes([137, 80, 78, 71])\")\n");
}
