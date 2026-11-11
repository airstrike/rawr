use std::path::PathBuf;

use super::{Bindings, Error, Outcome, Universe};
use crate::host::facade::Definition;
use crate::host::runtime::{self, Registry};
use crate::package::import::runner::account;
use crate::{future, Access, Dynamic, Engine, Shared};

/// Bindings with no crate in them: std alone, as a runner with no
/// dependencies has.
struct Plain;

impl Bindings for Plain {
    fn engine(&self, access: Access) -> std::io::Result<(Shared<Engine>, Box<[Definition]>)> {
        let mut engine = Engine::new();
        engine.set_access(access);
        engine
            .register_host_facade(future::facade::definition().map_err(std::io::Error::other)?)
            .map_err(std::io::Error::other)?;
        let native = account::Native::transform(account::tests::carrier());
        let engine = engine
            .retain_imported_native(native)
            .map_err(std::io::Error::other)?;
        Ok((engine, Box::default()))
    }

    fn runtime(
        &self,
        engine: &Engine,
        definitions: &[Definition],
    ) -> Result<Registry, runtime::Error> {
        let mut registry = Registry::new();
        registry.install(future::facade::runtime().map_err(runtime::Error::profile)?)?;
        let mut native = engine.begin_imported_runtime()?;
        native.authenticate_structural(definitions, &[])?;
        native.finish(registry)
    }
}

const PROGRAM: &str = "// rawr
pub struct Counter {
    pub count: i64,
}

pub fn total(a: i64, b: i64) -> i64 {
    a + b
}

pub fn counter() -> Counter {
    Counter { count: 1 }
}

fn main() {}
";

fn total(program: &super::Program, a: i64, b: i64) -> i64 {
    program
        .call("total", &mut [Dynamic::from(a), Dynamic::from(b)])
        .unwrap()
        .cast::<i64>()
}

/// One program through every outcome, on the thread that owns it. The
/// steps share one test: what a reload offers is the process's.
#[test]
fn a_program_is_called_reloaded_and_keeps_its_last_good_text() {
    let universe = Universe::new(&Plain, Access::none()).unwrap();
    let entry = PathBuf::from("/universe-fixture/src/main.rs");
    let program = universe.program(entry, PROGRAM, Vec::new()).unwrap();
    let outcomes = program.outcomes();
    assert_eq!(total(&program, 2, 3), 5);
    assert!(matches!(
        program.call("missing", &mut []),
        Err(Error::Function(name)) if &*name == "missing"
    ));
    assert_eq!(program.run(), 0);

    // A body edit compiles where the reload runs, and the owner takes it at
    // its next call.
    let doubled = PROGRAM.replace("a + b", "a * b");
    assert_eq!(program.replace(doubled.as_str()), Outcome::Queued);
    assert_eq!(program.replace(doubled.as_str()), Outcome::Unchanged);
    assert_eq!(total(&program, 2, 3), 6);
    assert_eq!(outcomes.try_recv(), Ok(Outcome::Queued));
    assert_eq!(outcomes.try_recv(), Ok(Outcome::Swapped));

    // A compile error keeps the last good text, and the same text again
    // doesn't compile again.
    let wrong = PROGRAM.replace("a + b", "\"no\"");
    let Outcome::Failed { message } = program.replace(wrong.as_str()) else {
        panic!("a type error fails");
    };
    assert!(message.contains("E0308"), "{message}");
    assert_eq!(program.replace(wrong.as_str()), Outcome::Unchanged);
    assert_eq!(total(&program, 2, 3), 6);

    // A script type whose fields changed can't swap in while a value of
    // it lives (D553.6), here the one the caller holds (#1746).
    let held = program.call("counter", &mut []).unwrap();
    let grown = doubled
        .replace("pub count: i64,", "pub count: i64,\n    pub step: i64,")
        .replace("Counter { count: 1 }", "Counter { count: 1, step: 2 }");
    assert_eq!(
        program.replace(grown),
        Outcome::Restarting {
            reason: "`Counter` changed its fields, but a live value holds it".to_owned()
        }
    );
    assert_eq!(total(&program, 2, 3), 6);
    // Once nothing holds one, its fields change in place.
    drop(held);
    let grown = doubled
        .replace("pub count: i64,", "pub count: i64,\n    pub step: u8,")
        .replace("Counter { count: 1 }", "Counter { count: 1, step: 2 }");
    assert_eq!(program.replace(grown.as_str()), Outcome::Queued);
    assert_eq!(total(&program, 2, 3), 6);
    let said = outcomes.try_iter().collect::<Vec<_>>();
    assert!(
        said.ends_with(&[Outcome::Queued, Outcome::Swapped]),
        "{said:?}"
    );

    // A program on disk rereads its package.
    let directory = std::env::temp_dir().join(format!("rawr-universe-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("src")).unwrap();
    let path = directory.join("src/main.rs");
    std::fs::write(&path, PROGRAM).unwrap();
    std::fs::write(
        directory.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    let loaded = universe.load(&path).unwrap();
    assert_eq!(total(&loaded, 4, 5), 9);
    assert_eq!(loaded.reload(), Outcome::Unchanged);
    std::fs::write(&path, PROGRAM.replace("a + b", "a - b")).unwrap();
    assert_eq!(loaded.reload(), Outcome::Queued);
    assert_eq!(total(&loaded, 4, 5), -1);
    std::fs::remove_dir_all(&directory).unwrap();

    // A save's admission checks again what depends on its old types
    // (#1746).
    a_value_made_after_a_save_compiled_refuses_it_where_it_would_be_admitted(&universe);
    a_kept_callable_that_makes_the_old_type_refuses_a_save_where_it_would_be_admitted(&universe);
    a_save_that_changes_a_type_nothing_holds_or_makes_swaps_in(&universe);
}

/// A snippet's statements outside any function, as ?play runs them, call
/// std's facade with the runtime bound, as its functions do (#1460).
#[test]
fn a_programs_top_level_calls_std_through_the_bound_runtime() {
    let mut universe = Universe::new(&Plain, Access::none()).unwrap();
    let printed = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let sink = std::sync::Arc::clone(&printed);
    assert!(universe.on_output(move |_, text| sink.lock().unwrap().push_str(text)));
    let source = "// rawr
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

let path = PathBuf::from(\"src\").join(\"main.rs\");
let after = SystemTime::now().duration_since(UNIX_EPOCH).is_ok();
println!(\"{} {after}\", path.display());

fn main() {
    let path = PathBuf::from(\"lib.rs\");
    println!(\"{:?} {}\", path.extension(), UNIX_EPOCH.elapsed().is_ok());
}
";
    let entry = PathBuf::from("/universe-fixture/src/main.rs");
    let program = universe.program(entry, source, Vec::new()).unwrap();
    assert_eq!(program.run(), 0);
    assert_eq!(
        *printed.lock().unwrap(),
        "src/main.rs true\nSome(\"rs\") true\n"
    );
}

const GAUGE: &str = "// rawr
pub struct Gauge {
    pub level: i64,
}

pub fn make() -> Gauge {
    Gauge { level: 1 }
}

pub fn total(a: i64, b: i64) -> i64 {
    a + b
}

fn main() {}
";

/// `GAUGE` with a new field, whose `make` takes the step, so a callable of
/// the old one doesn't rebind in it.
fn grown_gauge() -> String {
    GAUGE
        .replace("pub level: i64,", "pub level: i64,\n    pub step: i64,")
        .replace(
            "pub fn make() -> Gauge {\n    Gauge { level: 1 }",
            "pub fn make(step: i64) -> Gauge {\n    Gauge { level: 1, step }",
        )
}

/// The running generation's `make`, as native code holds a callable it
/// was given.
fn kept_maker() -> future::Callable {
    crate::island::callable("make").unwrap()
}

/// A save compiles while nothing holds a value of the type it changes,
/// and then a call under way makes one: its admission, at the next call
/// from native code, checks again, refuses it and keeps the running
/// program (#1746).
fn a_value_made_after_a_save_compiled_refuses_it_where_it_would_be_admitted(universe: &Universe) {
    let entry = PathBuf::from("/universe-gauge/src/main.rs");
    let program = universe.program(entry, GAUGE, Vec::new()).unwrap();
    let outcomes = program.outcomes();
    let maker = kept_maker();
    let held = {
        // A call from native code under way: the save compiles beside it,
        // and the call goes on in the running generation.
        let _entry = future::entry();
        assert_eq!(program.replace(grown_gauge()), Outcome::Queued);
        maker.call(&mut []).unwrap()
    };
    assert_eq!(total(&program, 2, 3), 5);
    let said = outcomes.try_iter().collect::<Vec<_>>();
    assert_eq!(
        said,
        [
            Outcome::Queued,
            Outcome::Restarting {
                reason: "`Gauge` changed its fields, but a live value holds it".to_owned()
            }
        ]
    );
    drop(held);
}

/// A callable native code holds that won't rebind in a save runs the code
/// of the generation that made it, which makes the old type: the save's
/// admission refuses it though no value of the type lives (#1746).
fn a_kept_callable_that_makes_the_old_type_refuses_a_save_where_it_would_be_admitted(
    universe: &Universe,
) {
    let entry = PathBuf::from("/universe-gauge-kept/src/main.rs");
    let program = universe.program(entry, GAUGE, Vec::new()).unwrap();
    let outcomes = program.outcomes();
    let maker = kept_maker();
    assert_eq!(program.replace(grown_gauge()), Outcome::Queued);
    assert_eq!(total(&program, 2, 3), 5);
    let said = outcomes.try_iter().collect::<Vec<_>>();
    assert_eq!(
        said,
        [
            Outcome::Queued,
            Outcome::Restarting {
                reason: "`Gauge` changed its fields, but a closure native code holds can still \
                         make the old one"
                    .to_owned()
            }
        ]
    );
    drop(maker);
}

/// Once native code drops the callable, the same save swaps in.
fn a_save_that_changes_a_type_nothing_holds_or_makes_swaps_in(universe: &Universe) {
    let entry = PathBuf::from("/universe-gauge-dropped/src/main.rs");
    let program = universe.program(entry, GAUGE, Vec::new()).unwrap();
    let outcomes = program.outcomes();
    drop(kept_maker());
    assert_eq!(program.replace(grown_gauge()), Outcome::Queued);
    assert_eq!(total(&program, 2, 3), 5);
    let said = outcomes.try_iter().collect::<Vec<_>>();
    assert_eq!(said, [Outcome::Queued, Outcome::Swapped]);
}
