//! A runner's reveal reuses what an earlier reveal read of each body whose
//! text and reads are as they were (#1755): what it reuses is what a fresh
//! reveal reads.

use crate::package::compile::captured::tests::compiler;
use crate::{host, package, resolve, Engine};

/// The package: a screen whose struct has a private field, and a widget
/// module that names a `State` of its own, as an iced app's modules do.
fn snapshot(screen: &str) -> package::snapshot::Snapshot {
    let root = std::path::Path::new("/reveal-fixture");
    let entry = root.join("src/main.rs");
    let widget = "// rawr\npub struct State { pub count: i64 }\n\
        pub fn shown<T: Clone>(value: T) -> T { native::echo(value) }\n\
        pub fn count(state: &State) -> i64 { shown(state.count) }\n";
    let main = "// rawr\nmod screen;\nmod widget;\n\
        pub fn main() { let mut state = screen::State::new(); state.tick(); \
        widget::count(&widget::State { count: 1 }); }\n";
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
        ],
    )
    .unwrap()
}

const SCREEN: &str = "// rawr\npub struct State { ticks: i64 }\n\
    impl State {\n\
    pub fn new() -> Self { Self { ticks: 0 } }\n\
    pub fn tick(&mut self) { self.ticks = crate::widget::shown(self.ticks + 1); }\n\
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

/// What the widget module's lowering and its bodies' reveal read of
/// `snapshot`'s program.
fn widget_reads(snapshot: &package::snapshot::Snapshot) -> ([u8; 32], [u8; 32]) {
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
    let widget = (0..program.source.texts().count())
        .map(crate::program::source::Id::from_index)
        .find(|source| program.source.identity_at(*source).ends_with("widget.rs"))
        .unwrap();
    (interface.of(widget), interface.revealed(widget))
}

/// The widget names a `State` of its own, so it reaches the screen's by
/// name. A private field the screen's `State` gains is one the widget's
/// bodies can't name: their reveal reads the program as it was, though
/// their lowering, whose drops read every field, reads it again. A public
/// one changes both.
#[test]
fn another_module_s_private_field_keeps_what_a_body_s_reveal_reads() {
    let (lowering, reading) = widget_reads(&snapshot(SCREEN));
    let private = widget_reads(&snapshot(
        &SCREEN
            .replace("{ ticks: i64 }", "{ ticks: i64, held: u32 }")
            .replace("{ ticks: 0 }", "{ ticks: 0, held: 0 }"),
    ));
    assert_ne!(private.0, lowering);
    assert_eq!(private.1, reading);
    let public = widget_reads(&snapshot(
        &SCREEN
            .replace("{ ticks: i64 }", "{ ticks: i64, pub held: u32 }")
            .replace("{ ticks: 0 }", "{ ticks: 0, held: 0 }"),
    ));
    assert_ne!(public.0, lowering);
    assert_ne!(public.1, reading);
}
