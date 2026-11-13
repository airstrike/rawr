//! A runner's reveal reuses what an earlier reveal read of each body whose
//! text and reads are as they were (#1755): what it reuses is what a fresh
//! reveal reads.

use crate::package::compile::captured::tests::compiler;
use crate::{host, package, resolve, Engine};

/// The package: a screen whose struct has a private field, a widget
/// module that names a `State` of its own, as an iced app's modules do,
/// and a reader module whose function takes the screen's `State`, which
/// its lowering checks against the struct's schema where the call enters
/// (`validate_parameters`).
fn snapshot(screen: &str) -> package::snapshot::Snapshot {
    let root = std::path::Path::new("/reveal-fixture");
    let entry = root.join("src/main.rs");
    let widget = "// rawr\npub struct State { pub count: i64 }\n\
        pub fn shown<T: Clone>(value: T) -> T { native::echo(value) }\n\
        pub fn count(state: &State) -> i64 { shown(state.count) }\n";
    let reader = "// rawr\nuse crate::screen;\n\
        pub fn read(state: &screen::State) -> i64 { state.ticks() }\n";
    let main = "// rawr\nmod reader;\nmod screen;\nmod widget;\n\
        pub fn main() { let mut state = screen::State::new(); state.tick(); \
        reader::read(&state); widget::count(&widget::State { count: 1 }); }\n";
    package::snapshot::Snapshot::new(
        entry.clone(),
        vec![
            (
                root.join("Cargo.toml"),
                "[package]\nname = \"reveal\"\n[dependencies]\nnative = { path = \"native\" }\n"
                    .into(),
            ),
            (entry, main.into()),
            (root.join("src/screen.rs"), screen.into()),
            (root.join("src/widget.rs"), widget.into()),
            (root.join("src/reader.rs"), reader.into()),
        ],
    )
    .unwrap()
}

const SCREEN: &str = "// rawr\npub struct State { ticks: i64 }\n\
    impl State {\n\
    pub fn new() -> Self { Self { ticks: 0 } }\n\
    pub fn tick(&mut self) { self.ticks = crate::widget::shown(self.ticks + 1); }\n\
    pub fn ticks(&self) -> i64 { self.ticks }\n\
    }\n";

/// What `engine`'s reveal of `snapshot`'s program finds: the instances and
/// the items and methods it gives the program.
fn revealed(engine: &Engine, snapshot: &package::snapshot::Snapshot) -> String {
    let mut allocation = host::TestImportAllocation::default();
    let graph = engine.load_snapshot_kept(snapshot).unwrap();
    let native = resolve::native::Capture::from_graph(&graph, &mut allocation).unwrap();
    let mut program =
        resolve::package::resolve_captured(graph, &engine.host_registry, native).unwrap();
    let host = engine.host_registry.share();
    super::reveal(engine, &mut program, &host, &mut allocation)
        .unwrap_or_else(|_| panic!("the fixture reveals"));
    format!(
        "{:?}\n{:?}\n{:?}",
        program.instances, program.items, program.methods
    )
}

/// A save that adds a private field to the screen's `State` and edits one
/// body: the keeping compiler's reveal, which reuses what it read of the
/// widget's bodies, finds what a fresh compiler's finds.
#[test]
fn a_reveal_that_reuses_unchanged_bodies_finds_what_a_fresh_one_does() {
    let mut keeping = compiler();
    keeping.keep_lowerings();
    assert_eq!(
        revealed(&keeping, &snapshot(SCREEN)),
        revealed(&compiler(), &snapshot(SCREEN))
    );
    let saved = SCREEN
        .replace("{ ticks: i64 }", "{ ticks: i64, held: u32 }")
        .replace("{ ticks: 0 }", "{ ticks: 0, held: 0 }")
        .replace("self.ticks + 1", "self.ticks + 2");
    assert_eq!(
        revealed(&keeping, &snapshot(&saved)),
        revealed(&compiler(), &snapshot(&saved))
    );
    // And back, as a restore saves the text it had.
    assert_eq!(
        revealed(&keeping, &snapshot(SCREEN)),
        revealed(&compiler(), &snapshot(SCREEN))
    );
}

/// What the lowering of the source whose path ends with `file` and its
/// bodies' reveal read of `snapshot`'s program.
fn reads(snapshot: &package::snapshot::Snapshot, file: &str) -> ([u8; 32], [u8; 32]) {
    let engine = compiler();
    let mut allocation = host::TestImportAllocation::default();
    let graph = engine.load_snapshot_kept(snapshot).unwrap();
    let native = resolve::native::Capture::from_graph(&graph, &mut allocation).unwrap();
    let program = resolve::package::resolve_captured(graph, &engine.host_registry, native).unwrap();
    let runtime =
        crate::program::model::Model::new(crate::program::GenerationId::issue(), &program);
    let models =
        super::models::Models::new(&program, runtime, engine.host_registry.share(), &engine);
    let native = super::interface::Native::new(&program, &models.host);
    let interface = super::interface::Interface::new(
        &engine,
        &program,
        &models,
        &mut super::interface::Outlined::new(),
        &mut super::interface::Digests::new(),
        &native,
    )
    .unwrap();
    let source = (0..program.source.texts().count())
        .map(crate::program::source::Id::from_index)
        .find(|source| program.source.identity_at(*source).ends_with(file))
        .unwrap();
    (interface.of(source), interface.revealed(source))
}

/// The widget names a `State` of its own, alone, so it never reaches the
/// screen's (#1886): a field the screen's `State` gains, private or
/// public, keeps what the widget's lowering and its bodies' reveal read.
/// The root names the screen's through `screen::`, and the reader through
/// its import, so each reads it again: the reader's lowering holds the
/// struct's schema, which a private field changes (#1885).
#[test]
fn a_field_of_another_module_s_state_keeps_what_a_module_with_its_own_reads() {
    let private = SCREEN
        .replace("{ ticks: i64 }", "{ ticks: i64, held: u32 }")
        .replace("{ ticks: 0 }", "{ ticks: 0, held: 0 }");
    let public = SCREEN
        .replace("{ ticks: i64 }", "{ ticks: i64, pub held: u32 }")
        .replace("{ ticks: 0 }", "{ ticks: 0, held: 0 }");
    let widget = reads(&snapshot(SCREEN), "widget.rs");
    assert_eq!(reads(&snapshot(&private), "widget.rs"), widget);
    assert_eq!(reads(&snapshot(&public), "widget.rs"), widget);
    let main = reads(&snapshot(SCREEN), "main.rs");
    assert_ne!(reads(&snapshot(&private), "main.rs").0, main.0);
    assert_ne!(reads(&snapshot(&public), "main.rs"), main);
    let reader = reads(&snapshot(SCREEN), "reader.rs");
    assert_ne!(reads(&snapshot(&private), "reader.rs").0, reader.0);
}

/// What `engine` lowers `snapshot`'s program to.
fn lowered(engine: &Engine, snapshot: &package::snapshot::Snapshot) -> String {
    let output = package::compile::captured::Output::transform_plain(
        engine,
        snapshot,
        &mut host::TestImportAllocation::default(),
    )
    .unwrap_or_else(|_| panic!("the fixture compiles"));
    let (_, ast) = output.candidate.with_top_level();
    format!("{ast:?}")
}

/// A save that adds a private field to the screen's `State`, which the
/// widget's own `State` shares a name with: the keeping compiler, which
/// reuses the widget's lowering and lowers the reader's again, lowers the
/// program to what a fresh compiler does, and so after a save back. A
/// reader lowering reused with the old schema would name its check by it.
#[test]
fn a_compile_that_reuses_another_state_s_module_lowers_what_a_fresh_one_does() {
    let mut keeping = compiler();
    keeping.keep_lowerings();
    assert_eq!(
        lowered(&keeping, &snapshot(SCREEN)),
        lowered(&compiler(), &snapshot(SCREEN))
    );
    let saved = SCREEN
        .replace("{ ticks: i64 }", "{ ticks: i64, held: u32 }")
        .replace("{ ticks: 0 }", "{ ticks: 0, held: 0 }");
    assert_eq!(
        lowered(&keeping, &snapshot(&saved)),
        lowered(&compiler(), &snapshot(&saved))
    );
    assert_eq!(
        lowered(&keeping, &snapshot(SCREEN)),
        lowered(&compiler(), &snapshot(SCREEN))
    );
}
