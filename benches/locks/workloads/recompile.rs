//! A1, B4 and B6: a runner's compile on save. The runner's compiler keeps
//! its parses and lowerings behind two `Mutex`es on the shared `Engine`
//! (A1), each compile is an attempt on the import account, whose refusal
//! latch every charge locks (B4), and each resolve keeps its native-miss
//! record behind an `Arc<Mutex>` it never shares (B6).
//!
//! This is the path a generated runner takes on each save
//! (`Engine::compile_package_snapshot_resident_plain`), on a compiler
//! that retained a native account as a runner's does. Each run is one
//! save: one body edit of the entry, compiled against the kept parses and
//! lowerings of the earlier saves.

use rawr::package::import::resident::Outcome;
use rawr::package::import::runner::account;
use rawr::package::snapshot::Snapshot;
use std::path::PathBuf;

use crate::workload::{About, Ran, Workload};

pub const NAME: &str = "recompile";

/// The package's manifest, which names no dependency.
const MANIFEST: &str =
    "[package]\nname = \"lock-bench\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";

/// The entry; each save changes `factor`'s body.
const MAIN: &str = r#"// rawr
mod model;

use model::{Counter, Message};

fn update(counter: &mut Counter, message: Message) {
    match message {
        Message::Increment => counter.value += factor(),
        Message::Decrement => counter.value -= factor(),
        Message::Reset => counter.value = 0,
    }
}

fn view(counter: &Counter) -> String {
    format!("{} ({} saves)", counter.value, counter.history.len())
}

fn main() {
    let mut counter = Counter::default();
    for message in [Message::Increment, Message::Decrement, Message::Reset] {
        update(&mut counter, message);
        println!("{}", view(&counter));
    }
}
"#;

/// A module no save changes, whose lowering a compile keeps.
const MODEL: &str = r#"// rawr
#[derive(Default, Debug, Clone)]
pub struct Counter {
    pub value: i64,
    pub history: Vec<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    Increment,
    Decrement,
    Reset,
}

impl Counter {
    pub fn record(&mut self) {
        self.history.push(self.value);
    }

    pub fn total(&self) -> i64 {
        self.history.iter().sum()
    }
}
"#;

/// The save `generation` made: the entry with `factor` giving it.
fn saved(generation: u64) -> Snapshot {
    let root = PathBuf::from("/lock-bench");
    let entry = root.join("src/main.rs");
    let main = format!("{MAIN}\nfn factor() -> i64 {{ {generation} }}\n");
    Snapshot::new(
        entry.clone(),
        vec![
            (root.join("Cargo.toml"), MANIFEST.into()),
            (entry, main.into()),
            (root.join("src/model.rs"), MODEL.into()),
        ],
    )
    .expect("three distinct absolute paths make a snapshot")
}

pub fn workload() -> Workload {
    let carrier = rawr_import::runner::manifest::carrier_fixture(&[]);
    let native = account::Native::decode(&carrier).expect("the fixture carrier decodes");
    let engine = rawr::Engine::new()
        .retain_imported_native(native)
        .expect("a fresh engine retains no native account yet");
    // The first compile is the runner's start; each run after is a save.
    let mut generation = 0;
    compile(&engine, generation);
    let about = About {
        name: NAME,
        smells: &["A1", "B4", "B6"],
        run: "one save: a body edit of a two-source program, compiled as a runner compiles it",
    };
    let fixture = format!("{MAIN}\n// model.rs\n{MODEL}");
    Workload::new(about, fixture, move || {
        generation += 1;
        compile(&engine, generation);
        Ran::default()
    })
}

fn compile(engine: &rawr::Engine, generation: u64) {
    let outcome = engine
        .compile_package_snapshot_resident_plain(&saved(generation))
        .unwrap_or_else(|error| panic!("the saved program compiles: {error}"));
    assert!(
        matches!(outcome, Outcome::Compiled(_)),
        "a plain program needs nothing the empty carrier lacks"
    );
}
