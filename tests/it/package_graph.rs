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
    project.write("src/main.rawr", "pub mod controls;\nmod internal;\n");
    project.write("src/controls.rawr", "pub mod icon;\npub struct Controls;\n");
    project.write("src/controls/icon.rawr", "pub struct Icon;\n");
    project.write("src/internal.rawr", "struct Hidden;\n");
    project.write("src/unreferenced.rawr", "this is not parsed");

    let target = rawr::project::Target::discover(project.path()).unwrap();
    let graph = Graph::load(&Engine::new(), &target).unwrap();

    assert_eq!(graph.packages().len(), 1);
    assert_eq!(graph.entry_package().name(), "main");
    assert_eq!(graph.sources().len(), 4);
    assert!(graph.sources().iter().any(|source| source.modules() == ["controls", "icon"]));
    assert!(!graph.sources().iter().any(|source| source.path().ends_with("unreferenced.rawr")));
}

#[test]
fn package_import_retains_a_host_item_and_module_at_one_path() {
    let project = Project::new("host-module-item");
    project.write(
        "src/main.rawr",
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
fn local_dependencies_keep_manifest_identity_not_alias() {
    let workspace = Project::new("dependencies");
    let app = workspace.path().join("app");
    let controls = workspace.path().join("controls");
    write(&app.join("src/main.rawr"), "");
    write(&app.join("rawr.toml"), "[package]\nname = \"counter-app\"\n\n[dependencies]\nui = { path = \"../controls\" }\n");
    write(&controls.join("src/lib.rawr"), "pub struct Button;\n");
    write(&controls.join("rawr.toml"), "[package]\nname = \"controls\"\n");

    let target = rawr::project::Target::discover(&app).unwrap();
    let graph = Graph::load(&Engine::new(), &target).unwrap();

    assert_eq!(graph.entry_package().name(), "counter-app");
    assert_eq!(graph.entry_package().dependencies()[0].alias(), "ui");
    assert_eq!(graph.packages()[1].name(), "controls");
    assert_eq!(graph.sources()[1].package(), "controls");
}

#[test]
fn child_parse_errors_retain_the_child_source() {
    let project = Project::new("child-parse");
    project.write("src/main.rawr", "mod broken;\n");
    project.write("src/broken.rawr", "struct Broken {\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Parse);
    assert!(report.labels().iter().all(|label| label.source().identity().ends_with("src/broken.rawr")));
}

#[test]
fn missing_modules_point_to_the_declaration() {
    let project = Project::new("missing-module");
    project.write("src/main.rawr", "mod absent;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resource);
    assert_eq!(report.code(), code::ENVIRONMENT_PACKAGE_RESOURCE);
    assert_eq!(report.labels()[0].span().start(), Position::new(1, 1));
    assert!(report.labels()[0].source().identity().ends_with("src/main.rawr"));
}

#[test]
fn cyclic_path_dependencies_report_both_manifests() {
    let workspace = Project::new("dependency-cycle");
    let app = workspace.path().join("app");
    let library = workspace.path().join("library");
    write(&app.join("src/main.rawr"), "");
    write(&app.join("rawr.toml"), "[package]\nname = \"app\"\n[dependencies]\nlib = { path = \"../library\" }\n");
    write(&library.join("src/lib.rawr"), "");
    write(&library.join("rawr.toml"), "[package]\nname = \"library\"\n[dependencies]\napp = { path = \"../app\" }\n");
    let target = rawr::project::Target::discover(&app).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::DependencyCycle);
    assert_eq!(report.code(), code::RESOLVE_PACKAGE_CYCLE);
    assert_eq!(report.labels().len(), 2);
    assert!(report.labels()[0].source().identity().ends_with("library/rawr.toml"));
    assert!(report.labels()[1].source().identity().ends_with("app/rawr.toml"));
}

#[test]
fn dependency_packages_require_a_stable_name() {
    let workspace = Project::new("missing-name");
    let app = workspace.path().join("app");
    let library = workspace.path().join("library");
    write(&app.join("src/main.rawr"), "");
    write(&app.join("rawr.toml"), "[dependencies]\nlib = { path = \"../library\" }\n");
    write(&library.join("src/lib.rawr"), "");
    write(&library.join("rawr.toml"), "[dependencies]\n");
    let target = rawr::project::Target::discover(&app).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Manifest);
    assert!(report.sources().iter().any(|source| source.identity().ends_with("library/rawr.toml")));
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
    let project = Project::new("aggregate-types");
    project.write("src/main.rawr", "pub mod controls;\nuse crate::controls::Button;\npub fn accept(value: Button) {}\n");
    project.write("src/controls.rawr", "pub struct Button;\n");
    project.write("rawr.toml", "[package]\nname = \"counter-app\"\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let executable = Engine::new().compile_package_executable(&target).unwrap();
    let accept = executable.items().find(|item| item.key().name() == "accept").unwrap();

    assert_eq!(executable.type_count(), 1);
    assert_eq!(accept.key().package(), "counter-app");
    assert!(accept.key().modules().is_empty());
}

#[test]
fn unit_and_tuple_structs_through_module_paths() {
    let project = Project::new("struct-paths");
    project.write(
        "src/main.rawr",
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
    project.write("src/units.rawr", "pub struct Origin;\npub struct Meters(pub i64);\n");
    project.write("rawr.toml", "[package]\nname = \"struct-paths\"\n");
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
    project.write("src/main.rawr", "mod controls;\nmod other;\n");
    project.write("src/controls.rawr", "mod inner;\n");
    project.write("src/controls/inner.rawr", "pub struct Button;\n");
    project.write("src/other.rawr", "use crate::controls::inner::Button;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_INVALID_IMPORT);
    assert_eq!(report.labels().len(), 2);
    assert!(report.labels()[0].source().identity().ends_with("src/other.rawr"));
    assert!(report.labels()[1].source().identity().ends_with("src/controls.rawr"));
}

#[test]
fn rust_module_privacy_paths_and_constants() {
    let project = Project::new("rust-modules");
    project.write(
        "src/main.rawr",
        "mod signal;\nmod config;\nuse crate::signal::State;\npub fn answer() -> i64 { let s: State = signal::make(); signal::level(s) + config::GAP }\n",
    );
    project.write("src/config.rawr", "pub const GAP: i64 = 8;\n");
    project.write(
        "src/signal.rawr",
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
fn aggregate_compile_rejects_non_entry_statements_at_their_source() {
    let project = Project::new("module-statements");
    project.write("src/main.rawr", "mod controls;\n");
    project.write("src/controls.rawr", "let hidden = 1;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert!(report.labels()[0].source().identity().ends_with("src/controls.rawr"));
}

#[test]
fn aggregate_widget_errors_retain_the_declaring_child_source() {
    let project = Project::new("widget-child-diagnostic");
    project.write("src/main.rawr", "mod controls;\n");
    project.write("src/controls.rawr", "pub struct Dial;\n#[widget]\nimpl Dial { fn paint(&self) { () } }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let mut engine = Engine::new();
    let facade = host::facade::Builder::new("iced", host::facade::Abi::new(1).unwrap()).unwrap().finish().unwrap();
    engine.register_host_facade(facade).unwrap();

    let error = engine.compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(report.code(), code::RESOLVE_WIDGET);
    assert!(report.labels().iter().all(|label| label.source().identity().ends_with("src/controls.rawr")));
}

#[test]
fn imported_functions_lower_once_with_module_qualified_runtime_symbols() {
    let project = Project::new("module-functions");
    project.write("src/main.rawr", "pub mod left;\npub mod right;\nuse crate::left::double;\npub fn answer(value: i64) -> i64 { double(value) }\n");
    project.write("src/left.rawr", "pub fn double(value: i64) -> i64 { value + value }\n");
    project.write("src/right.rawr", "pub fn double(value: i64) -> i64 { value + value + 1 }\n");
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
fn dependency_aliases_resolve_public_reexports_without_changing_identity() {
    let workspace = Project::new("dependency-reexport");
    let app = workspace.path().join("app");
    let controls = workspace.path().join("controls");
    write(&app.join("src/main.rawr"), "use ui::{State, add_one};\npub fn answer(value: i64) -> i64 { add_one(value) }\npub fn keep(value: State) -> State { value }\n");
    write(&app.join("rawr.toml"), "[package]\nname = \"counter-app\"\n[dependencies]\nui = { path = \"../controls\" }\n");
    write(&controls.join("src/lib.rawr"), "pub mod model;\npub use crate::model::{State, add_one};\n");
    write(&controls.join("src/model.rawr"), "pub struct State { value: i64 }\npub fn add_one(value: i64) -> i64 { value + 1 }\n");
    write(&controls.join("rawr.toml"), "[package]\nname = \"controls-kit\"\n");
    let target = rawr::project::Target::discover(&app).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap();
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();
    let dependency = authority.items().find(|value| value.key().name() == "add_one").unwrap();

    assert_eq!(dependency.key().package(), "controls-kit");
    assert_eq!(dependency.key().modules(), ["model"]);
    assert_eq!(authority.invoke(&answer, &mut [Dynamic::from(41_i64)]).unwrap().as_int().unwrap(), 42);
}

#[test]
fn qualified_free_function_calls_use_exact_package_and_module_items() {
    let workspace = Project::new("qualified-free-functions");
    let app = workspace.path().join("app");
    let controls = workspace.path().join("controls");
    let dial = workspace.path().join("dial");
    let left = workspace.path().join("left");
    let right = workspace.path().join("right");
    write(
        &app.join("src/main.rawr"),
        r#"
            pub mod local;
            use controls::imported;

            pub fn answer() -> i64 {
                controls::view()
                    + dial::view(2)
                    + controls::nested::view(4)
                    + crate::local::view(8)
                    + local::view(16)
                    + controls::reexported(32)
                    + imported(64)
                    + left::same()
                    + right::same()
            }
        "#,
    );
    write(&app.join("src/local.rawr"), "pub fn view(value: i64) -> i64 { value }\n");
    write(
        &app.join("rawr.toml"),
        r#"
            [dependencies]
            controls = { path = "../controls" }
            dial = { path = "../dial" }
            left = { path = "../left" }
            right = { path = "../right" }
        "#,
    );
    write(
        &controls.join("src/lib.rawr"),
        r#"
            pub mod nested;
            pub use crate::nested::reexported;

            pub fn view() -> i64 { 1 }
            pub fn imported(value: i64) -> i64 { value }
        "#,
    );
    write(
        &controls.join("src/nested.rawr"),
        r#"
            pub fn view(value: i64) -> i64 { value }
            pub fn reexported(value: i64) -> i64 { value }
        "#,
    );
    write(&controls.join("rawr.toml"), "[package]\nname = \"controls-kit\"\n");
    write(&dial.join("src/lib.rawr"), "pub fn view(value: i64) -> i64 { value }\n");
    write(&dial.join("rawr.toml"), "[package]\nname = \"dial-kit\"\n");
    write(&left.join("src/lib.rawr"), "pub fn same() -> i64 { 128 }\n");
    write(&left.join("rawr.toml"), "[package]\nname = \"left-kit\"\n");
    write(&right.join("src/lib.rawr"), "pub fn same() -> i64 { 256 }\n");
    write(&right.join("rawr.toml"), "[package]\nname = \"right-kit\"\n");
    let target = rawr::project::Target::discover(&app).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap();
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();

    assert_eq!(authority.invoke(&answer, &mut []).unwrap().as_int().unwrap(), 511);
}

#[test]
fn qualified_free_function_failures_preserve_package_authority_and_positions() {
    let unknown_package = Project::new("unknown-qualified-function-package");
    unknown_package.write("src/main.rawr", "pub fn make() -> i64 { missing::view() }\n");
    let target = rawr::project::Target::discover(unknown_package.path()).unwrap();
    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Compilation);
    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_TYPE);
    assert!(report.message().contains("missing"));
    assert_eq!(report.labels()[0].span().start(), Position::new(1, 24));

    let workspace = Project::new("invalid-qualified-function");
    let app = workspace.path().join("app");
    let controls = workspace.path().join("controls");
    write(&app.join("rawr.toml"), "[dependencies]\ncontrols = { path = \"../controls\" }\n");
    write(&controls.join("src/lib.rawr"), "pub fn view() -> i64 { 1 }\nmod internal;\n");
    write(&controls.join("src/internal.rawr"), "pub fn hidden() -> i64 { 2 }\n");
    write(&controls.join("rawr.toml"), "[package]\nname = \"controls-kit\"\n");

    write(&app.join("src/main.rawr"), "pub fn make() -> i64 { controls::missing() }\n");
    let target = rawr::project::Target::discover(&app).unwrap();
    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_IMPORT);
    assert!(report.message().contains("controls::missing"));
    assert_eq!(report.labels()[0].span().start(), Position::new(1, 24));

    write(&app.join("src/main.rawr"), "pub fn make() -> i64 { controls::internal::hidden() }\n");
    let target = rawr::project::Target::discover(&app).unwrap();
    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_INVALID_IMPORT);
    assert!(report.message().contains("private item"));
    assert_eq!(report.labels()[0].span().start(), Position::new(1, 24));
    assert_eq!(report.labels().len(), 2);
    assert!(report.labels()[1].source().identity().ends_with("controls/src/lib.rawr"));
}

#[test]
fn qualified_associated_calls_use_exact_package_and_module_owners() {
    let workspace = Project::new("qualified-associated-calls");
    let app = workspace.path().join("app");
    let counter = workspace.path().join("counter");
    write(
        &app.join("src/main.rawr"),
        r#"
            pub mod local;
            use counter::State;

            pub struct Root { value: i64 }

            impl Root {
                pub fn new(value: i64) -> Root { Root { value } }
                pub fn value(&self) -> i64 { self.value }
            }

            pub fn answer() -> i64 {
                let imported = State::new(1);
                let dependency = counter::State::new(2);
                let nested = counter::model::Nested::new(4);
                let root = crate::Root::new(8);
                let local = local::Local::new(16);
                imported.value()
                    + dependency.value()
                    + nested.value()
                    + root.value()
                    + local.value()
            }
        "#,
    );
    write(
        &app.join("src/local.rawr"),
        r#"
            pub struct Local { value: i64 }

            impl Local {
                pub fn new(value: i64) -> Local { Local { value } }
                pub fn value(&self) -> i64 { self.value }
            }
        "#,
    );
    write(&app.join("rawr.toml"), "[dependencies]\ncounter = { path = \"../counter\" }\n");
    write(
        &counter.join("src/lib.rawr"),
        r#"
            pub mod model;

            pub struct State { value: i64 }

            impl State {
                pub fn new(value: i64) -> State { State { value } }
                pub fn value(&self) -> i64 { self.value }
            }
        "#,
    );
    write(
        &counter.join("src/model.rawr"),
        r#"
            pub struct Nested { value: i64 }

            impl Nested {
                pub fn new(value: i64) -> Nested { Nested { value } }
                pub fn value(&self) -> i64 { self.value }
            }
        "#,
    );
    write(&counter.join("rawr.toml"), "[package]\nname = \"counter-kit\"\n");
    let target = rawr::project::Target::discover(&app).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap();
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();

    assert_eq!(authority.invoke(&answer, &mut []).unwrap().as_int().unwrap(), 31);
}

#[test]
fn qualified_recursive_patterns_use_exact_dependency_owner_proofs() {
    let workspace = Project::new("qualified-recursive-patterns");
    let app = workspace.path().join("app");
    let controls = workspace.path().join("controls");
    write(
        &app.join("src/main.rawr"),
        r#"
            use controls::{Action, Event};

            pub enum Message {
                Controls(Action),
                Dial(Event),
            }

            pub fn inspect(message: Message) -> i64 {
                match message {
                    Message::Controls(controls::Action::Activate) => 2,
                    Message::Controls(controls::Action::Deactivate) => 3,
                    Message::Dial(controls::Event::Changed(value)) => value,
                }
            }

            pub fn answer() -> i64 {
                inspect(Message::Controls(Action::Activate))
                    + inspect(Message::Controls(Action::Deactivate))
                    + inspect(Message::Dial(Event::Changed(40)))
            }
        "#,
    );
    write(&app.join("rawr.toml"), "[dependencies]\ncontrols = { path = \"../controls\" }\n");
    write(
        &controls.join("src/lib.rawr"),
        r#"
            pub enum Action { Activate, Deactivate }
            pub enum Event { Changed(i64) }
        "#,
    );
    write(&controls.join("rawr.toml"), "[package]\nname = \"control-kit\"\n");
    let target = rawr::project::Target::discover(&app).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap();
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();

    assert_eq!(authority.invoke(&answer, &mut []).unwrap().as_int().unwrap(), 45);
}

#[test]
fn qualified_associated_call_failures_preserve_owner_authority_and_positions() {
    let unknown_package = Project::new("unknown-associated-package");
    unknown_package.write("src/main.rawr", "pub fn make() -> i64 { missing::State::new() }\n");
    let target = rawr::project::Target::discover(unknown_package.path()).unwrap();
    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Compilation);
    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_TYPE);
    assert!(report.message().contains("missing::State"));
    assert_eq!(report.labels()[0].span().start(), Position::new(1, 24));

    let workspace = Project::new("invalid-associated-owner");
    let app = workspace.path().join("app");
    let counter = workspace.path().join("counter");
    write(&app.join("rawr.toml"), "[dependencies]\ncounter = { path = \"../counter\" }\n");
    write(&counter.join("src/lib.rawr"), "pub struct State;\n");
    write(&counter.join("rawr.toml"), "[package]\nname = \"counter-kit\"\n");

    write(&app.join("src/main.rawr"), "pub fn make() -> i64 { counter::Missing::new() }\n");
    let target = rawr::project::Target::discover(&app).unwrap();
    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_IMPORT);
    assert!(report.message().contains("counter::Missing"));
    assert_eq!(report.labels()[0].span().start(), Position::new(1, 24));

    write(&app.join("src/main.rawr"), "pub fn make() -> i64 { counter::State::missing() }\n");
    let target = rawr::project::Target::discover(&app).unwrap();
    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Compilation);
    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_MEMBER);
    assert!(report.message().contains("counter::State::missing"));
    assert_eq!(report.labels()[0].span().start(), Position::new(1, 40));
}

#[test]
fn qualified_associated_calls_do_not_cross_private_package_items() {
    let workspace = Project::new("private-associated-owner");
    let app = workspace.path().join("app");
    let counter = workspace.path().join("counter");
    write(&app.join("src/main.rawr"), "pub fn make() -> i64 { counter::State::new() }\n");
    write(&app.join("rawr.toml"), "[dependencies]\ncounter = { path = \"../counter\" }\n");
    write(&counter.join("src/lib.rawr"), "struct State;\nimpl State { pub fn new() -> State { State } }\n");
    write(&counter.join("rawr.toml"), "[package]\nname = \"counter-kit\"\n");
    let target = rawr::project::Target::discover(&app).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_INVALID_IMPORT);
    assert!(report.message().contains("private item"));
    assert_eq!(report.labels()[0].span().start(), Position::new(1, 24));
    assert_eq!(report.labels().len(), 2);
}

#[test]
fn unknown_public_reexports_fail_in_the_declaring_dependency() {
    let workspace = Project::new("unknown-reexport");
    let app = workspace.path().join("app");
    let library = workspace.path().join("library");
    write(&app.join("src/main.rawr"), "");
    write(&app.join("rawr.toml"), "[dependencies]\nlib = { path = \"../library\" }\n");
    write(&library.join("src/lib.rawr"), "pub use crate::missing::Thing;\n");
    write(&library.join("rawr.toml"), "[package]\nname = \"library\"\n");
    let target = rawr::project::Target::discover(&app).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_IMPORT);
    assert!(report.labels()[0].source().identity().ends_with("library/src/lib.rawr"));
}

#[test]
fn lowering_failures_keep_the_exact_child_source() {
    let project = Project::new("child-lowering");
    project.write("src/main.rawr", "pub mod broken;\n");
    project.write("src/broken.rawr", "pub fn wrong(value: i64) -> bool { value }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Compilation);
    assert!(report.labels()[0].source().identity().ends_with("src/broken.rawr"));
}

#[test]
fn capability_failures_keep_the_exact_dependency_source() {
    let workspace = Project::new("dependency-capability");
    let app = workspace.path().join("app");
    let library = workspace.path().join("library");
    write(&app.join("src/main.rawr"), "#[derive(Clone)]\npub struct Broken;\n");
    write(&app.join("rawr.toml"), "[dependencies]\nlib = { path = \"../library\" }\n");
    write(&library.join("src/lib.rawr"), "#[derive(Send)]\npub struct Broken;\n");
    write(&library.join("rawr.toml"), "[package]\nname = \"library\"\n");
    let target = rawr::project::Target::discover(&app).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_DERIVE);
    assert!(report.labels().iter().all(|label| label.source().identity().ends_with("library/src/lib.rawr")));
}

#[test]
fn duplicate_methods_retain_both_declaring_sources() {
    let project = Project::new("duplicate-method-sources");
    project.write("src/main.rawr", "pub mod first;\npub mod second;\npub struct State;\n");
    project.write("src/first.rawr", "use crate::State;\nimpl State { pub fn value(&self) -> i64 { 1 } }\n");
    project.write("src/second.rawr", "use crate::State;\nimpl State { pub fn value(&self) -> i64 { 2 } }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_DUPLICATE_METHOD);
    assert_eq!(report.labels().len(), 2);
    assert!(report.labels()[0].source().identity().ends_with("src/second.rawr"));
    assert!(report.labels()[1].source().identity().ends_with("src/first.rawr"));
}

#[test]
fn qualified_header_paths_resolve_without_an_import_binding() {
    let project = Project::new("qualified-header");
    project.write("src/main.rawr", "pub mod model;\npub fn keep(value: crate::model::State) -> crate::model::State { value }\n");
    project.write("src/model.rawr", "pub struct State { value: i64 }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let executable = Engine::new().compile_package_executable(&target).unwrap();

    assert_eq!(executable.type_count(), 1);
}

#[test]
fn child_module_types_resolve_relatively_in_every_header_position() {
    let project = Project::new("relative-header-types");
    project.write(
        "src/main.rawr",
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
    project.write("src/controls.rawr", "pub enum Message { Tick }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let executable = Engine::new().compile_package_executable(&target).unwrap();

    assert_eq!(executable.type_count(), 3);
}

#[test]
fn nested_child_module_types_resolve_from_their_containing_module() {
    let project = Project::new("nested-relative-header-type");
    project.write("src/main.rawr", "pub mod screen;\n");
    project.write("src/screen.rawr", "pub mod controls;\npub struct Model { message: controls::Message }\n");
    project.write("src/screen/controls.rawr", "pub enum Message { Tick }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let executable = Engine::new().compile_package_executable(&target).unwrap();

    assert_eq!(executable.type_count(), 2);
}

#[test]
fn relative_header_types_preserve_private_module_boundaries() {
    let project = Project::new("private-relative-header-type");
    // `controls` is open to the crate root, but its private `inner` is not.
    project.write("src/main.rawr", "mod controls;\npub struct Model { message: controls::inner::Message }\n");
    project.write("src/controls.rawr", "mod inner;\n");
    project.write("src/controls/inner.rawr", "pub enum Message { Tick }\n");
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
    project.write("src/main.rawr", "pub mod controls;\npub struct Model { message: controls::Missing }\n");
    project.write("src/controls.rawr", "pub enum Message { Tick }\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_IMPORT);
    assert!(report.message().contains("controls::Missing"));
    assert_eq!(report.labels()[0].span().start(), Position::new(2, 29));
}

#[test]
fn super_imports_resolve_in_the_parent_module() {
    let project = Project::new("super-import");
    project.write("src/main.rawr", "pub mod child;\n");
    project.write("src/child.rawr", "use super::State;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_IMPORT);
    assert!(report.message().contains("State"), "{}", report.message());
    assert!(report.labels()[0].source().identity().ends_with("src/child.rawr"));
}

#[test]
fn cyclic_public_reexports_are_rejected_before_lowering() {
    let project = Project::new("reexport-cycle");
    project.write("src/main.rawr", "pub mod a;\npub mod b;\n");
    project.write("src/a.rawr", "pub use crate::b::Thing;\n");
    project.write("src/b.rawr", "pub use crate::a::Thing;\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = Engine::new().compile_package_executable(&target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resolution);
    assert_eq!(report.code(), code::RESOLVE_INVALID_IMPORT);
    assert!(report.message().contains("cyclic public re-export"));
}

#[test]
fn duplicate_dependency_package_names_are_rejected() {
    let workspace = Project::new("duplicate-package-name");
    let app = workspace.path().join("app");
    let first = workspace.path().join("first");
    let second = workspace.path().join("second");
    write(&app.join("src/main.rawr"), "");
    write(&app.join("rawr.toml"), "[dependencies]\nfirst = { path = \"../first\" }\nsecond = { path = \"../second\" }\n");
    for dependency in [&first, &second] {
        write(&dependency.join("src/lib.rawr"), "");
        write(&dependency.join("rawr.toml"), "[package]\nname = \"shared\"\n");
    }
    let target = rawr::project::Target::discover(&app).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::DuplicatePackage);
    assert_eq!(report.code(), code::RESOLVE_PACKAGE_IDENTITY);
    assert_eq!(report.labels().len(), 2);
}

#[cfg(feature = "rust-import")]
#[test]
fn absent_rust_manifest_section_produces_no_import_request() {
    let project = Project::new("no-rust-import");
    project.write("src/main.rawr", "let ready = true;\n");
    project.write("rawr.toml", "[package]\nname = \"no-rust-import\"\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let graph = Graph::load(&Engine::new(), &target).unwrap();

    assert!(
        graph
            .rust_import(rawr_import::cargo::config::Environment::empty(), &rawr_import::cargo::Limits::standard(),)
            .expect("an absent Rust section is a valid no-import state")
            .is_none()
    );
}

#[cfg(feature = "rust-import")]
#[test]
fn dependency_rust_manifest_section_rejects_entry_authority() {
    let workspace = Project::new("dependency-rust-import");
    let app = workspace.path().join("app");
    let library = workspace.path().join("library");
    write(&app.join("src/main.rawr"), "");
    write(&app.join("rawr.toml"), "[package]\nname = \"app\"\n[dependencies]\nlibrary = { path = \"../library\" }\n");
    write(&library.join("src/lib.rawr"), "");
    write(&library.join("rawr.toml"), "[package]\nname = \"library\"\n[rust]\n");
    let target = rawr::project::Target::discover(&app).unwrap();

    let error = Graph::load(&Engine::new(), &target).expect_err("a non-entry Rust authority cannot enter the package graph");
    assert_eq!(error.category(), Category::Manifest);
    assert!(error.report().unwrap().message().contains("only the entry Rawr package"));
}

#[cfg(feature = "rust-import")]
#[test]
fn manifest_cannot_claim_the_reserved_internal_alias() {
    let project = Project::new("reserved-rust-alias");
    project.write("src/main.rawr", "");
    project.write("rawr.toml", &format!("[package]\nname = \"reserved-rust-alias\"\n[rust]\n[rust.dependencies]\n__rawr_internal = {{ path = {:?} }}\n", project.path()));
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let graph = Graph::load(&Engine::new(), &target).unwrap();

    let error = graph
        .rust_import(rawr_import::cargo::config::Environment::empty(), &rawr_import::cargo::Limits::standard())
        .expect_err("the crate-owned alias cannot be forged by manifest text");
    assert!(format!("{error}").contains("reserved for Rawr"));
}

#[cfg(unix)]
#[test]
fn canonical_module_targets_cannot_escape_or_alias() {
    use std::os::unix::fs::symlink;

    let workspace = Project::new("canonical-modules");
    let escaping = workspace.path().join("escaping");
    let outside = workspace.path().join("outside.rawr");
    write(&outside, "pub struct Outside;\n");
    write(&escaping.join("src/main.rawr"), "mod outside;\n");
    symlink(&outside, escaping.join("src/outside.rawr")).unwrap();
    let target = rawr::project::Target::discover(&escaping).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Resource);
    assert!(report.message().contains("outside package root"));
    assert!(report.labels()[0].source().identity().ends_with("src/main.rawr"));

    let aliasing = workspace.path().join("aliasing");
    write(&aliasing.join("src/main.rawr"), "mod first;\nmod second;\n");
    write(&aliasing.join("src/shared.rawr"), "pub struct Shared;\n");
    symlink("shared.rawr", aliasing.join("src/first.rawr")).unwrap();
    symlink("shared.rawr", aliasing.join("src/second.rawr")).unwrap();
    let target = rawr::project::Target::discover(&aliasing).unwrap();

    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(error.category(), Category::Module);
    assert_eq!(report.labels().len(), 2);
}

struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("rawr-package-graph-{}-{id}-{name}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
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

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn rust_use_forms_and_qualified_types() {
    let project = Project::new("rust-use-forms");
    project.write(
        "src/main.rawr",
        "mod signal;\nmod app;\n",
    );
    project.write(
        "src/app.rawr",
        "use super::signal::{self, State as Level, make};\nuse crate::signal::double as twice;\npub fn answer() -> i64 { let s: signal::State = make(); let t: Level = signal::make(); signal::level(s) + twice(signal::level(t)) }\n",
    );
    project.write(
        "src/signal.rawr",
        "mod inner;\npub struct State { level: i64 }\npub fn make() -> State { State { level: inner::seed() } }\npub fn level(state: State) -> i64 { state.level }\npub fn double(n: i64) -> i64 { n * 2 }\n",
    );
    project.write(
        "src/signal/inner.rawr",
        "pub fn seed() -> i64 { let s: super::State = super::State { level: 5 }; super::level(s) }\npub fn keep(state: super::State) -> super::State { state }\n",
    );
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let engine = Shared::new(Engine::new());

    let executable = engine.compile_package_executable(&target).unwrap_or_else(|error| panic!("{error}"));
    let authority = engine.authorize(executable).unwrap();
    let answer = authority.items().find(|value| value.key().name() == "answer").unwrap().clone();
    assert_eq!(authority.invoke(&answer, &mut []).unwrap().as_int().unwrap(), 5 + 10);

    // As in Rust, `self` cannot import a module that is already in scope.
    project.write("src/main.rawr", "mod signal;\nuse signal::{self};\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    assert!(Engine::new().compile_package_executable(&target).is_err());
}
