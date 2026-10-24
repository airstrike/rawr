#![cfg(not(feature = "no_std"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rawr::diagnostic::code;
use rawr::package::{Category, Graph};
use rawr::{Dynamic, Engine, ParseErrorType, Position, Shared, host, item};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn declared_modules_form_the_only_source_graph() {
    let project = Project::new("declared");
    project.write("src/main.rs", "pub mod controls;\nmod internal;\n");
    project.write("src/controls.rs", "pub mod icon;\npub struct Controls;\n");
    project.write("src/controls/icon.rs", "pub struct Icon;\n");
    project.write("src/internal.rs", "struct Hidden;\n");
    project.write("src/unreferenced.rs", "this is not parsed");

    let target = rawr::project::Target::discover(project.path()).unwrap();
    let graph = Graph::load(&Engine::new(), &target).unwrap();

    assert_eq!(graph.packages().len(), 1);
    assert_eq!(graph.entry_package().name(), "declared");
    assert_eq!(graph.sources().len(), 4);
    assert!(graph.sources().iter().any(|source| source.modules() == ["controls", "icon"]));
    assert!(!graph.sources().iter().any(|source| source.path().ends_with("unreferenced.rs")));
}

#[test]
fn package_import_retains_a_host_item_and_module_at_one_path() {
    let project = Project::new("host-module-item");
    project.write(
        "src/main.rs",
        r#"
            use api::widget::{button};

            pub fn styled() -> i64 {
                let style = button::primary;
                button(style)
            }
        "#,
    );

    let mut facade = host::facade::Builder::new("api", host::facade::Abi::new(1).unwrap()).unwrap();
    facade.module("widget").unwrap();
    facade.module("widget::button").unwrap();
    let mut button = host::signature::Builder::new();
    button
        .parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64))
        .output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    facade.item(host::item::Definition::function("widget::button", button.finish()).unwrap()).unwrap();
    facade
        .item(host::item::Definition::value("widget::button::primary", host::type_ref::Type::builtin(host::type_ref::Builtin::I64)).unwrap())
        .unwrap();

    let target = rawr::project::Target::discover(project.path()).unwrap();
    let mut engine = Engine::new();
    engine.register_host_facade(facade.finish().unwrap()).unwrap();
    engine.compile_package_executable(&target).unwrap();
}

#[test]
fn child_parse_errors_retain_the_child_source() {
    let project = Project::new("child-parse");
    project.write("src/main.rs", "mod broken;\n");
    project.write("src/broken.rs", "struct Broken {\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Parse);
    assert!(report.labels().iter().all(|label| label.source().identity().ends_with("src/broken.rs")));
}

#[test]
fn missing_modules_point_to_the_declaration() {
    let project = Project::new("missing-module");
    project.write("src/main.rs", "mod absent;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resource);
    assert_eq!(report.code(), code::ENVIRONMENT_PACKAGE_RESOURCE);
    assert_eq!(report.labels()[0].span().start(), Position::new(2, 1));
    assert!(report.labels()[0].source().identity().ends_with("src/main.rs"));
}

#[test]
fn legacy_string_compilation_never_loads_file_modules() {
    let error = Engine::new().compile("mod controls;").unwrap_err();

    let ParseErrorType::Resolution(error) = error.err_type() else { panic!("expected a positioned resolution error") };
    assert_eq!(error.primary_span().start(), Position::new(1, 1));
    assert!(error.to_string().contains("project/package compilation"));
}

#[test]
fn aggregate_compile_resolves_module_types_and_stable_item_keys() {
    let project = Project::new("aggregate_types");
    project.write("src/main.rs", "pub mod controls;\nuse crate::controls::Button;\npub fn accept(value: Button) {}\n");
    project.write("src/controls.rs", "pub struct Button;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let executable = Engine::new().compile_package_executable(&target).unwrap();
    let accept = executable.items().find(|item| item.key().name() == "accept").unwrap();

    assert_eq!(executable.type_count(), 1);
    assert_eq!(accept.key().package(), "aggregate_types");
    assert!(accept.key().modules().is_empty());
}

#[test]
fn unit_and_tuple_structs_through_module_paths() {
    let project = Project::new("struct-paths");
    project.write(
        "src/main.rs",
        r#"
            pub mod units;
            pub fn answer() -> i64 {
                let origin = units::Origin;
                let at = match origin { units::Origin => 1 };
                let units::Meters(m) = units::Meters(40);
                let wrapped = Some(1).map(units::Meters).unwrap();
                at + m + wrapped.0
            }
        "#,
    );
    project.write("src/units.rs", "pub struct Origin;\npub struct Meters(pub i64);\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap();
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();

    assert_eq!(authority.invoke(&answer, &mut []).unwrap().as_int().unwrap(), 42);
}

#[test]
fn public_type_aliases_are_read_across_modules() {
    let project = Project::new("type-aliases");
    project.write(
        "src/main.rs",
        r#"
            pub mod geometry;
            use crate::geometry::{Coord, Path};
            pub fn answer() -> i64 {
                let start: Coord = (40, 0);
                let path: Path = vec![start, (1, 1)];
                geometry::first(&path) + path.len() as i64
            }
        "#,
    );
    project.write(
        "src/geometry.rs",
        "pub type Path = Vec<Coord>;\npub type Coord = (i64, i64);\npub fn first(path: &Path) -> i64 { path[0].0 }\n",
    );
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap();
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();

    assert_eq!(authority.invoke(&answer, &mut []).unwrap().as_int().unwrap(), 42);
}

#[test]
fn aggregate_compile_rejects_private_import_hops_with_both_sources() {
    // As in Rust, a private module is open to its parent but not to a sibling.
    let project = Project::new("private-hop");
    project.write("src/main.rs", "mod controls;\nmod other;\n");
    project.write("src/controls.rs", "mod inner;\n");
    project.write("src/controls/inner.rs", "pub struct Button;\n");
    project.write("src/other.rs", "use crate::controls::inner::Button;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_INVALID_IMPORT);
    assert_eq!(report.labels().len(), 2);
    assert!(report.labels()[0].source().identity().ends_with("src/other.rs"));
    assert!(report.labels()[1].source().identity().ends_with("src/controls.rs"));
}

#[test]
fn rust_module_privacy_paths_and_constants() {
    let project = Project::new("rust-modules");
    project.write(
        "src/main.rs",
        "mod signal;\nmod config;\nuse crate::signal::State;\npub fn answer() -> i64 { let s: State = signal::make(); signal::level(s) + config::GAP }\n",
    );
    project.write("src/config.rs", "pub const GAP: i64 = 8;\n");
    project.write(
        "src/signal.rs",
        "use super::config;\nuse crate::config::GAP;\npub struct State { level: i64 }\npub fn make() -> State { State { level: GAP * 4 } }\npub fn level(state: State) -> i64 { state.level + config::GAP }\n",
    );
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap_or_else(|error| panic!("{error}"));
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();

    assert_eq!(authority.invoke(&answer, &mut []).unwrap().as_int().unwrap(), 32 + 8 + 8);
}

#[test]
fn crate_paths_reach_root_imports_from_inline_modules() {
    let project = Project::new("crate-root-import");
    project.write(
        "src/main.rs",
        "mod preset;\nuse preset::Preset;\nconst BASE: i64 = 2;\nmod grid {\n    use crate::Preset;\n    use crate::BASE;\n    pub fn size(preset: Preset) -> i64 { preset.cells + BASE + inner::extra() }\n    mod inner {\n        use crate::Preset;\n        use super::super::BASE;\n        pub fn extra() -> i64 { let p = Preset { cells: 10 }; p.cells * BASE }\n    }\n}\npub fn answer() -> i64 { grid::size(Preset { cells: 3 }) }\n",
    );
    project.write("src/preset.rs", "pub struct Preset { pub cells: i64 }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap_or_else(|error| panic!("{error}"));
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();

    assert_eq!(authority.invoke(&answer, &mut []).unwrap().as_int().unwrap(), 3 + 2 + 20);
}

#[test]
fn aggregate_compile_rejects_non_entry_statements_at_their_source() {
    let project = Project::new("module-statements");
    project.write("src/main.rs", "mod controls;\n");
    project.write("src/controls.rs", "let hidden = 1;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert!(report.labels()[0].source().identity().ends_with("src/controls.rs"));
}

#[test]
fn aggregate_widget_errors_retain_the_declaring_child_source() {
    let project = Project::new("widget-child-diagnostic");
    project.write("src/main.rs", "mod controls;\n");
    project.write("src/controls.rs", "pub struct Dial;\n#[widget]\nimpl Dial { fn paint(&self) { () } }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let mut engine = Engine::new();
    let facade = host::facade::Builder::new("iced", host::facade::Abi::new(1).unwrap()).unwrap().finish().unwrap();
    engine.register_host_facade(facade).unwrap();

    let error = engine.compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(report.code(), code::RESOLVE_WIDGET);
    assert!(report.labels().iter().all(|label| label.source().identity().ends_with("src/controls.rs")));
}

#[test]
fn imported_functions_lower_once_with_module_qualified_runtime_symbols() {
    let project = Project::new("module-functions");
    project.write("src/main.rs", "pub mod left;\npub mod right;\nuse crate::left::double;\npub fn answer(value: i64) -> i64 { double(value) }\n");
    project.write("src/left.rs", "pub fn double(value: i64) -> i64 { value + value }\n");
    project.write("src/right.rs", "pub fn double(value: i64) -> i64 { value + value + 1 }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap();
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();
    let left = authority
        .items()
        .find(|value| value.key().modules() == ["left"] && value.kind() == item::Kind::Function)
        .unwrap()
        .clone();
    let right = authority
        .items()
        .find(|value| value.key().modules() == ["right"] && value.kind() == item::Kind::Function)
        .unwrap()
        .clone();

    assert_eq!(authority.invoke(&answer, &mut [Dynamic::from(21_i64)]).unwrap().as_int().unwrap(), 42);
    assert_eq!(authority.invoke(&left, &mut [Dynamic::from(20_i64)]).unwrap().as_int().unwrap(), 40);
    assert_eq!(authority.invoke(&right, &mut [Dynamic::from(20_i64)]).unwrap().as_int().unwrap(), 41);
}

#[test]
fn lowering_failures_keep_the_exact_child_source() {
    let project = Project::new("child-lowering");
    project.write("src/main.rs", "pub mod broken;\n");
    project.write("src/broken.rs", "pub fn wrong(value: i64) -> bool { value }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Compilation);
    assert!(report.labels()[0].source().identity().ends_with("src/broken.rs"));
}

#[test]
fn duplicate_methods_retain_both_declaring_sources() {
    let project = Project::new("duplicate-method-sources");
    project.write("src/main.rs", "pub mod first;\npub mod second;\npub struct State;\n");
    project.write("src/first.rs", "use crate::State;\nimpl State { pub fn value(&self) -> i64 { 1 } }\n");
    project.write("src/second.rs", "use crate::State;\nimpl State { pub fn value(&self) -> i64 { 2 } }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_DUPLICATE_METHOD);
    assert_eq!(report.labels().len(), 2);
    assert!(report.labels()[0].source().identity().ends_with("src/second.rs"));
    assert!(report.labels()[1].source().identity().ends_with("src/first.rs"));
}

#[test]
fn qualified_header_paths_resolve_without_an_import_binding() {
    let project = Project::new("qualified-header");
    project.write("src/main.rs", "pub mod model;\npub fn keep(value: crate::model::State) -> crate::model::State { value }\n");
    project.write("src/model.rs", "pub struct State { value: i64 }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let executable = Engine::new().compile_package_executable(&target).unwrap();

    assert_eq!(executable.type_count(), 1);
}

#[test]
fn child_module_types_resolve_relatively_in_every_header_position() {
    let project = Project::new("relative-header-types");
    project.write(
        "src/main.rs",
        r#"
            pub mod controls;

            pub struct Envelope {
                message: controls::Message,
            }

            pub enum Action {
                Controls(controls::Message),
            }

            pub fn relay(message: controls::Message) -> controls::Message {
                message
            }

            impl Envelope {
                pub fn relay(&self, message: controls::Message) -> controls::Message {
                    message
                }
            }
        "#,
    );
    project.write("src/controls.rs", "pub enum Message { Tick }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let executable = Engine::new().compile_package_executable(&target).unwrap();

    assert_eq!(executable.type_count(), 3);
}

#[test]
fn nested_child_module_types_resolve_from_their_containing_module() {
    let project = Project::new("nested-relative-header-type");
    project.write("src/main.rs", "pub mod screen;\n");
    project.write("src/screen.rs", "pub mod controls;\npub struct Model { message: controls::Message }\n");
    project.write("src/screen/controls.rs", "pub enum Message { Tick }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let executable = Engine::new().compile_package_executable(&target).unwrap();

    assert_eq!(executable.type_count(), 2);
}

#[test]
fn relative_header_types_preserve_private_module_boundaries() {
    let project = Project::new("private-relative-header-type");
    // `controls` is open to the crate root, but its private `inner` is not.
    project.write("src/main.rs", "mod controls;\npub struct Model { message: controls::inner::Message }\n");
    project.write("src/controls.rs", "mod inner;\n");
    project.write("src/controls/inner.rs", "pub enum Message { Tick }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_INVALID_IMPORT);
    assert!(report.message().contains("private module"));
    assert_eq!(report.labels().len(), 2);
}

#[test]
fn unknown_types_under_a_relative_module_are_positioned_import_errors() {
    let project = Project::new("unknown-relative-header-type");
    project.write("src/main.rs", "pub mod controls;\npub struct Model { message: controls::Missing }\n");
    project.write("src/controls.rs", "pub enum Message { Tick }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_IMPORT);
    assert!(report.message().contains("controls::Missing"));
    assert_eq!(report.labels()[0].span().start(), Position::new(3, 29));
}

#[test]
fn super_imports_resolve_in_the_parent_module() {
    let project = Project::new("super-import");
    project.write("src/main.rs", "pub mod child;\n");
    project.write("src/child.rs", "use super::State;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_IMPORT);
    assert!(report.message().contains("State"), "{}", report.message());
    assert!(report.labels()[0].source().identity().ends_with("src/child.rs"));
}

#[test]
fn cyclic_public_reexports_are_rejected_before_lowering() {
    let project = Project::new("reexport-cycle");
    project.write("src/main.rs", "pub mod a;\npub mod b;\n");
    project.write("src/a.rs", "pub use crate::b::Thing;\n");
    project.write("src/b.rs", "pub use crate::a::Thing;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_INVALID_IMPORT);
    assert!(report.message().contains("cyclic public re-export"));
}

#[cfg(feature = "rust-import")]
#[test]
fn a_crate_without_dependencies_produces_no_import_request() {
    let project = Project::new("no-rust-import");
    project.write("src/main.rs", "let ready = true;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let graph = Graph::load(&Engine::new(), &target).unwrap();

    assert!(
        graph
            .rust_import(rawr_import::cargo::config::Environment::empty(), &rawr_import::cargo::Limits::standard(),)
            .expect("a crate without dependencies is a valid no-import state")
            .is_none()
    );
}

#[cfg(feature = "rust-import")]
#[cfg(feature = "rust-import")]
#[test]
fn manifest_cannot_claim_the_reserved_internal_alias() {
    let project = Project::new("reserved-rust-alias");
    project.write("src/main.rs", "");
    project.write("Cargo.toml", &format!("[package]\nname = \"reserved-rust-alias\"\n[dependencies]\n__rawr_internal = {{ path = {:?} }}\n", project.path()));
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let graph = Graph::load(&Engine::new(), &target).unwrap();

    let error = graph
        .rust_import(rawr_import::cargo::config::Environment::empty(), &rawr_import::cargo::Limits::standard())
        .expect_err("the crate-owned alias cannot be forged by Cargo.toml");
    assert!(format!("{error}").contains("reserved for Rawr"));
}

#[cfg(unix)]
#[test]
fn canonical_module_targets_cannot_escape_or_alias() {
    use std::os::unix::fs::symlink;

    let workspace = Project::new("canonical-modules");
    let escaping = workspace.path().join("escaping");
    write(&escaping.join("Cargo.toml"), "[package]\nname = \"escaping\"\n");
    let outside = workspace.path().join("outside.rs");
    write(&outside, "pub struct Outside;\n");
    write(&escaping.join("src/main.rs"), "mod outside;\n");
    symlink(&outside, escaping.join("src/outside.rs")).unwrap();
    let target = rawr::project::Target::discover(&escaping).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resource);
    assert!(report.message().contains("outside package root"));
    assert!(report.labels()[0].source().identity().ends_with("src/main.rs"));

    let aliasing = workspace.path().join("aliasing");
    write(&aliasing.join("Cargo.toml"), "[package]\nname = \"aliasing\"\n");
    write(&aliasing.join("src/main.rs"), "mod first;\nmod second;\n");
    write(&aliasing.join("src/shared.rs"), "pub struct Shared;\n");
    symlink("shared.rs", aliasing.join("src/first.rs")).unwrap();
    symlink("shared.rs", aliasing.join("src/second.rs")).unwrap();
    let target = rawr::project::Target::discover(&aliasing).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Module);
    assert_eq!(report.labels().len(), 2);
}

/// A marked file's untyped `fn sink(value)` is not Rust, but Rawr still
/// knows the function its call names.
#[test]
fn marked_untyped_function_is_known_to_its_caller() {
    let project = Project::new("untyped-sink");
    project.write("src/main.rs", "fn sink(value) {}\nfn main() { sink(1); }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    Engine::new()
        .compile_package_executable(&target)
        .unwrap_or_else(|error| panic!("{error}"));
}

struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("rawr-package-graph-{}-{id}-{name}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        write(&root.join("Cargo.toml"), &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"));
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, path: &str, text: &str) {
        write(&self.root.join(path), text);
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

/// Write `text` at `path`. A `.rs` file gets `// rawr` as its first line,
/// so it is a marked module.
fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    if path.extension().is_some_and(|extension| extension == "rs") {
        fs::write(path, format!("// rawr\n{text}")).unwrap();
    } else {
        fs::write(path, text).unwrap();
    }
}

#[test]
fn rust_use_forms_and_qualified_types() {
    let project = Project::new("rust-use-forms");
    project.write(
        "src/main.rs",
        "mod signal;\nmod app;\n",
    );
    project.write(
        "src/app.rs",
        "use super::signal::{self, State as Level, make};\nuse crate::signal::double as twice;\npub fn answer() -> i64 { let s: signal::State = make(); let t: Level = signal::make(); signal::level(s) + twice(signal::level(t)) }\n",
    );
    project.write(
        "src/signal.rs",
        "mod inner;\npub struct State { level: i64 }\npub fn make() -> State { State { level: inner::seed() } }\npub fn level(state: State) -> i64 { state.level }\npub fn double(n: i64) -> i64 { n * 2 }\n",
    );
    project.write(
        "src/signal/inner.rs",
        "pub fn seed() -> i64 { let s: super::State = super::State { level: 5 }; super::level(s) }\npub fn keep(state: super::State) -> super::State { state }\n",
    );
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap_or_else(|error| panic!("{error}"));
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();
    assert_eq!(authority.invoke(&answer, &mut []).unwrap().as_int().unwrap(), 5 + 10);

    // As in Rust, `self` cannot import a module that is already in scope.
    project.write("src/main.rs", "mod signal;\nuse signal::{self};\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    assert!(Engine::new().compile_package_executable(&target).is_err());
}
