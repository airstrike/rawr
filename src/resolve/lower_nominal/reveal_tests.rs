//! A runner's reveal reuses what an earlier reveal read of each body whose
//! text and reads are as they were (#1755): what it reuses is what a fresh
//! reveal reads.

use crate::package::compile::captured::tests::compiler;
use crate::{host, package, resolve, Engine};

/// The package: a screen whose struct has a private field, a widget
/// module that names a `State` of its own, as an iced app's modules do,
/// and a reader module whose function takes the screen's `State`, which
/// its lowering checks against the struct's schema where the call enters
/// (`validate_parameters`), and which it clones, formats, passes to native
/// code and binds in a pattern, each of which holds the schema too.
fn snapshot(screen: &str) -> package::snapshot::Snapshot {
    let root = std::path::Path::new("/reveal-fixture");
    let entry = root.join("src/main.rs");
    let widget = "// rawr\npub struct State { pub count: i64 }\n\
        pub fn shown<T: Clone>(value: T) -> T { native::echo(value) }\n\
        pub fn count(state: &State) -> i64 { shown(state.count) }\n";
    let reader = "// rawr\nuse crate::screen;\n\
        pub fn read(state: &screen::State) -> i64 {\n\
        let seen = native::echo(state.clone());\n\
        let text = format!(\"{seen:?}\");\n\
        match Some(seen) { Some(held) => held.ticks() + state.count + text.len() as i64, None => 0 }\n\
        }\n";
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

const SCREEN: &str =
    "// rawr\n#[derive(Clone, Debug)]\npub struct State { pub count: i64, ticks: i64 }\n\
    impl State {\n\
    pub fn new() -> Self { Self { count: 0, ticks: 0 } }\n\
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
        .replace("ticks: i64 }", "ticks: i64, held: u32 }")
        .replace("ticks: 0 }", "ticks: 0, held: 0 }")
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
        &mut super::interface::Remembered::new(),
        None,
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
/// its import, and neither sees its private fields: a private field of a
/// plain type keeps what each lowering reads, which renews the struct's
/// schema where it holds it (#1885), and a public one changes it.
#[test]
fn a_field_of_another_module_s_state_keeps_what_a_module_with_its_own_reads() {
    let private = SCREEN
        .replace("ticks: i64 }", "ticks: i64, held: u32 }")
        .replace("ticks: 0 }", "ticks: 0, held: 0 }");
    let public = SCREEN
        .replace("ticks: i64 }", "ticks: i64, pub held: u32 }")
        .replace("ticks: 0 }", "ticks: 0, held: 0 }");
    let widget = reads(&snapshot(SCREEN), "widget.rs");
    assert_eq!(reads(&snapshot(&private), "widget.rs"), widget);
    assert_eq!(reads(&snapshot(&public), "widget.rs"), widget);
    for file in ["main.rs", "reader.rs"] {
        let read = reads(&snapshot(SCREEN), file);
        assert_eq!(reads(&snapshot(&private), file), read, "{file}");
        assert_ne!(reads(&snapshot(&public), file).0, read.0, "{file}");
    }
}

/// A private field whose type a lowering asks about, a cell's, which makes
/// the struct hold one, or a reference's, which a borrow's check reads,
/// changes what another module's lowering reads (#1885).
#[test]
fn a_private_field_a_lowering_asks_about_changes_what_another_module_reads() {
    let reader = reads(&snapshot(SCREEN), "reader.rs").0;
    let cell = SCREEN
        .replace("ticks: i64 }", "ticks: i64, held: std::cell::Cell<u32> }")
        .replace("ticks: 0 }", "ticks: 0, held: std::cell::Cell::new(0) }");
    assert_ne!(reads(&snapshot(&cell), "reader.rs").0, reader);
    let referent = SCREEN
        .replace("ticks: i64 }", "ticks: i64, held: &'static str }")
        .replace("ticks: 0 }", "ticks: 0, held: \"\" }");
    assert_ne!(reads(&snapshot(&referent), "reader.rs").0, reader);
}

/// What `engine` lowers `snapshot`'s program to.
fn lowered(engine: &Engine, snapshot: &package::snapshot::Snapshot) -> String {
    let output = package::compile::captured::Output::transform_plain(
        engine,
        snapshot,
        &mut host::TestImportAllocation::default(),
    )
    .unwrap_or_else(|_| panic!("the fixture compiles"));
    let (_, mut ast) = output.candidate.with_top_level();
    let lowered = format!("{ast:?}");
    // The schemas a body's patterns check, which their `Debug` leaves out.
    let mut schemas = Vec::new();
    ast.for_each_fn_def_mut(|function| {
        schemas.push(crate::program::schema::symbol_id(&function.body));
    });
    format!("{lowered}\n{schemas:?}")
}

/// A save that adds a private field to the screen's `State`, which the
/// widget's own `State` shares a name with: the keeping compiler, which
/// reuses the widget's lowering and the reader's, each op and pattern of
/// the reader's renewed with the struct's new schema (#1885), lowers the
/// program to what a fresh compiler does, and so after a save back. A
/// reader lowering reused with the old schema would name its check, its
/// clone, its native call and its pattern by it.
#[test]
fn a_compile_that_reuses_another_state_s_module_lowers_what_a_fresh_one_does() {
    let mut keeping = compiler();
    keeping.keep_lowerings();
    assert_eq!(
        lowered(&keeping, &snapshot(SCREEN)),
        lowered(&compiler(), &snapshot(SCREEN))
    );
    let saved = SCREEN
        .replace("ticks: i64 }", "ticks: i64, held: u32 }")
        .replace("ticks: 0 }", "ticks: 0, held: 0 }");
    assert_eq!(
        lowered(&keeping, &snapshot(&saved)),
        lowered(&compiler(), &snapshot(&saved))
    );
    assert_eq!(
        lowered(&keeping, &snapshot(SCREEN)),
        lowered(&compiler(), &snapshot(SCREEN))
    );
}
