//! Native modules: unmarked `.rs` modules beside marked ones in one crate.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rawr::Engine;
use rawr::package::Graph;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixture() -> PathBuf {
    fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/polyglot")).unwrap()
}

#[test]
fn polyglot_discovery_records_rust_modules_and_compiles_only_rawr() {
    let root = fixture();
    let target = rawr::project::Target::discover(&root).unwrap();
    let graph = Graph::load(&Engine::new(), &target).unwrap();
    let sources = graph.sources().iter().map(|source| source.path().to_path_buf()).collect::<Vec<_>>();
    assert_eq!(sources, [root.join("src/main.rs"), root.join("src/view.rs")]);
    let rust = graph.entry_package().rust_modules();
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].modules(), ["model"]);
    assert_eq!(rust[0].path(), root.join("src/model.rs"));
}

#[test]
fn polyglot_discovery_follows_mod_rs_and_rust_children() {
    let project = Project::new("children");
    project.write("src/main.rs", "// rawr\nmod ui;\nmod data;\n");
    project.write("src/ui/mod.rs", "// rawr\npub mod widgets;\n");
    project.write("src/ui/widgets.rs", "pub mod button;\nmod inner { mod deep; }\n#[path = \"x.rs\"] mod moved;\n");
    project.write("src/ui/widgets/button.rs", "pub struct Button;\n");
    project.write("src/ui/widgets/inner/deep.rs", "");
    project.write("src/data/mod.rs", "");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let graph = Graph::load(&Engine::new(), &target).unwrap();
    assert_eq!(graph.sources().len(), 2);
    let modules = graph.entry_package().rust_modules().iter().map(|module| module.modules().join("::")).collect::<Vec<_>>();
    assert_eq!(modules, ["ui::widgets::button", "ui::widgets::inner::deep", "ui::widgets", "data"]);
}

#[test]
fn a_module_with_both_files_is_an_error() {
    let project = Project::new("both");
    project.write("src/main.rs", "// rawr\nmod model;\n");
    project.write("src/model.rs", "// rawr\n");
    project.write("src/model/mod.rs", "");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let error = Graph::load(&Engine::new(), &target).unwrap_err().to_string();
    assert!(error.contains("file for module `model` found at both") && error.contains("model.rs") && error.contains("model/mod.rs"), "{error}");
}

#[test]
fn a_native_module_declaring_a_marked_module_is_an_error() {
    let project = Project::new("one-way");
    project.write("src/main.rs", "// rawr\nmod model;\n");
    project.write("src/model.rs", "pub mod view;\n");
    project.write("src/model/view.rs", "// rawr\n");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let error = Graph::load(&Engine::new(), &target).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("native module `crate::model` cannot declare the marked module `mod view;`") && message.contains(") yet. Declare it from the crate root"), "{message}");
    let report = error.report().unwrap();
    assert!(report.labels()[0].source().identity().ends_with("src/model.rs"));
}

struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("rawr-polyglot-{}-{id}-{name}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Cargo.toml"), format!("[package]\nname = \"poly-{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n")).unwrap();
        Self { root: fs::canonicalize(root).unwrap() }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A `use` of a `.rs` module item (`crate::model::…`, `super::model::…`)
/// names the app's own crate under its crate name; here a host facade
/// stands in for the imported crate.
#[test]
fn a_path_into_a_rust_module_names_the_app_crate() {
    use rawr::host;
    let project = Project::new("crate-path");
    project.write("Cargo.toml", "[package]\nname = \"poly-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    project.write("src/main.rs", "// rawr\nmod model;\nmod view;\nuse crate::model::answer;\n\npub fn main() -> i64 {\n    view::total() + answer()\n}\n");
    project.write("src/view.rs", "// rawr\nuse super::model::answer;\n\npub fn total() -> i64 {\n    answer()\n}\n");
    project.write("src/model.rs", "pub fn answer() -> i64 { 21 }\n");

    let target = rawr::project::Target::discover(project.path()).unwrap();
    let graph = Graph::load(&Engine::new(), &target).unwrap();
    assert_eq!(graph.entry_package().rust_crate(), Some("poly_app"));

    let mut facade = host::facade::Builder::new("poly_app", host::facade::Abi::new(1).unwrap()).unwrap();
    facade.module("model").unwrap();
    let mut answer = host::signature::Builder::new();
    answer.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    facade.item(host::item::Definition::function("model::answer", answer.finish()).unwrap()).unwrap();
    let mut engine = Engine::new();
    engine.register_host_facade(facade.finish().unwrap()).unwrap();
    engine.compile_package_executable(&target).unwrap();

    project.write("src/view.rs", "// rawr\nuse crate::model::missing;\n\npub fn total() -> i64 {\n    missing()\n}\n");
    let error = engine.compile_package_executable(&target).unwrap_err().to_string();
    assert!(error.contains("poly_app::model::missing"), "{error}");
}

/// A fully qualified expression path into a `.rs` module
/// (`crate::model::answer()`, `super::model::answer()`) needs no `use`.
#[test]
fn a_qualified_expression_path_into_a_rust_module_names_the_app_crate() {
    use rawr::host;
    let project = Project::new("qualified-path");
    project.write("Cargo.toml", "[package]\nname = \"poly-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    project.write("src/main.rs", "// rawr\nmod model;\nmod view;\n\npub fn main() -> i64 {\n    view::total() + crate::model::answer()\n}\n");
    project.write("src/view.rs", "// rawr\npub fn total() -> i64 {\n    super::model::answer()\n}\n");
    project.write("src/model.rs", "pub fn answer() -> i64 { 21 }\n");

    let target = rawr::project::Target::discover(project.path()).unwrap();
    let mut facade = host::facade::Builder::new("poly_app", host::facade::Abi::new(1).unwrap()).unwrap();
    facade.module("model").unwrap();
    let mut answer = host::signature::Builder::new();
    answer.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    facade.item(host::item::Definition::function("model::answer", answer.finish()).unwrap()).unwrap();
    let mut engine = Engine::new();
    engine.register_host_facade(facade.finish().unwrap()).unwrap();
    engine.compile_package_executable(&target).unwrap();

    #[cfg(feature = "rust-import")]
    {
        let graph = Graph::load(&engine, &target).unwrap();
        let seed = graph.rust_demand_seed();
        assert!(seed.contains(&Box::<str>::from("poly_app::model::answer")), "{seed:?}");
    }
}

/// A qualified path into a `.rs` module in type position
/// (`let t: crate::model::Thing = ..`) names the app crate without a `use`.
#[test]
fn a_qualified_type_path_into_a_rust_module_names_the_app_crate() {
    use rawr::host;
    struct Thing;
    let project = Project::new("qualified-type");
    project.write("Cargo.toml", "[package]\nname = \"poly-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    project.write("src/main.rs", "// rawr\nmod model;\nmod view;\n\npub fn main() -> i64 {\n    let t: crate::model::Thing = crate::model::make();\n    view::total()\n}\n");
    project.write(
        "src/view.rs",
        "// rawr\npub fn keep(t: super::model::Thing) -> super::model::Thing {\n    t\n}\n\npub fn total() -> i64 {\n    let t: super::model::Thing = keep(super::model::make());\n    1\n}\n",
    );
    project.write("src/model.rs", "pub struct Thing;\npub fn make() -> Thing { Thing }\n");

    let target = rawr::project::Target::discover(project.path()).unwrap();
    let mut facade = host::facade::Builder::new("poly_app", host::facade::Abi::new(1).unwrap()).unwrap();
    facade.module("model").unwrap();
    facade.type_::<Thing>("model::Thing", &[], &[]).unwrap();
    let mut make = host::signature::Builder::new();
    make.output(host::type_ref::Type::registered("model::Thing").unwrap());
    facade.item(host::item::Definition::function("model::make", make.finish()).unwrap()).unwrap();
    let mut engine = Engine::new();
    engine.register_host_facade(facade.finish().unwrap()).unwrap();
    engine.compile_package_executable(&target).unwrap();
}

/// A `use` that binds a `.rs` module (`use crate::model;`, `use
/// crate::model as m;`, `use super::model::inner;`) binds it as a module:
/// its constants, functions, types and nested modules are reached through
/// the name, as rustc reaches them (#1117).
#[test]
fn a_use_bound_rust_module_names_the_app_crate() {
    use rawr::host;
    struct Thing;
    let project = Project::new("bound-module");
    project.write("Cargo.toml", "[package]\nname = \"poly-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    project.write("src/main.rs", "// rawr\nmod model;\nmod view;\n\npub fn main() -> i64 {\n    view::total()\n}\n");
    project.write(
        "src/view.rs",
        "// rawr\nuse crate::model;\nuse crate::model as m;\nuse super::model::inner;\nuse model::BONUS;\n\n\
         pub fn keep(t: model::Thing) -> m::Thing {\n    t\n}\n\n\
         pub fn total() -> i64 {\n    let t: m::Thing = keep(model::make());\n    \
         model::BONUS + m::BONUS + BONUS + model::answer() + m::answer() + inner::deep() + model::inner::deep()\n}\n",
    );
    project.write(
        "src/model.rs",
        "pub const BONUS: i64 = 7;\npub struct Thing;\npub fn make() -> Thing { Thing }\npub fn answer() -> i64 { 21 }\npub mod inner {\n    pub fn deep() -> i64 { 1 }\n}\n",
    );

    let target = rawr::project::Target::discover(project.path()).unwrap();
    let i64 = || host::type_ref::Type::builtin(host::type_ref::Builtin::I64);
    let mut facade = host::facade::Builder::new("poly_app", host::facade::Abi::new(1).unwrap()).unwrap();
    facade.module("model").unwrap();
    facade.module("model::inner").unwrap();
    facade.type_::<Thing>("model::Thing", &[], &[]).unwrap();
    facade.item(host::item::Definition::value("model::BONUS", i64()).unwrap()).unwrap();
    let mut make = host::signature::Builder::new();
    make.output(host::type_ref::Type::registered("model::Thing").unwrap());
    facade.item(host::item::Definition::function("model::make", make.finish()).unwrap()).unwrap();
    for function in ["model::answer", "model::inner::deep"] {
        let mut signature = host::signature::Builder::new();
        signature.output(i64());
        facade.item(host::item::Definition::function(function, signature.finish()).unwrap()).unwrap();
    }
    let mut engine = Engine::new();
    engine.register_host_facade(facade.finish().unwrap()).unwrap();
    engine.compile_package_executable(&target).unwrap();

    #[cfg(feature = "rust-import")]
    {
        let seed = Graph::load(&engine, &target).unwrap().rust_demand_seed();
        for path in ["poly_app::model::BONUS", "poly_app::model::answer", "poly_app::model::inner::deep", "poly_app::model::Thing"] {
            assert!(seed.contains(&Box::<str>::from(path)), "{path} in {seed:?}");
        }
    }
}

/// A marked module of a native crate root names the root's own items
/// (`crate::helper`, `super::SCALE`, `use crate::Thing;`) as rustc does:
/// the root is native, so they are the app crate's items (#1185). The
/// root's marked and native modules keep their own spellings.
#[test]
fn a_marked_module_names_its_native_roots_items_through_the_app_crate() {
    use rawr::host;
    struct Thing;
    let project = Project::new("root-items");
    project.write("Cargo.toml", "[package]\nname = \"poly-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    project.write(
        "src/main.rs",
        "mod badge;\nmod units;\n\nconst SCALE: i64 = 3;\nstruct Thing;\nfn make() -> Thing { Thing }\nfn helper(n: i64) -> i64 { n * 2 }\n\n\
         fn main() {\n    println!(\"{}\", badge::total());\n}\n",
    );
    project.write(
        "src/badge.rs",
        "// rawr\nuse crate::Thing;\nuse crate::helper;\n\n\
         pub fn keep(t: crate::Thing) -> Thing {\n    t\n}\n\n\
         pub fn total() -> i64 {\n    let t: Thing = keep(crate::make());\n    \
         crate::helper(1) + helper(2) + super::SCALE + crate::units::BONUS\n}\n",
    );
    project.write("src/units.rs", "pub const BONUS: i64 = 7;\n");

    let target = rawr::project::Target::discover(project.path()).unwrap();
    let i64 = || host::type_ref::Type::builtin(host::type_ref::Builtin::I64);
    let mut facade = host::facade::Builder::new("poly_app", host::facade::Abi::new(1).unwrap()).unwrap();
    facade.module("units").unwrap();
    facade.module("__rawr_island").unwrap();
    facade.type_::<Thing>("Thing", &[], &[]).unwrap();
    facade.item(host::item::Definition::value("SCALE", i64()).unwrap()).unwrap();
    facade.item(host::item::Definition::value("units::BONUS", i64()).unwrap()).unwrap();
    let mut make = host::signature::Builder::new();
    make.output(host::type_ref::Type::registered("Thing").unwrap());
    facade.item(host::item::Definition::function("make", make.finish()).unwrap()).unwrap();
    let mut helper = host::signature::Builder::new();
    helper.parameter(i64());
    helper.output(i64());
    facade.item(host::item::Definition::function("helper", helper.finish()).unwrap()).unwrap();
    facade
        .item(host::item::Definition::function("__rawr_island::main", host::signature::Builder::new().finish()).unwrap())
        .unwrap();
    let mut engine = Engine::new();
    engine.register_host_facade(facade.finish().unwrap()).unwrap();
    engine.compile_package_executable(&target).unwrap();

    #[cfg(feature = "rust-import")]
    {
        let seed = Graph::load(&engine, &target).unwrap().rust_demand_seed();
        for path in ["poly_app::Thing", "poly_app::helper", "poly_app::make", "poly_app::SCALE", "poly_app::units::BONUS"] {
            assert!(seed.contains(&Box::<str>::from(path)), "{path} in {seed:?}");
        }
        assert!(!seed.iter().any(|path| path.starts_with("poly_app::badge")), "{seed:?}");
    }
}

/// The app's own crate is a Cargo dependency generated from its
/// `Cargo.toml` and its native modules; `view.rs` uses `crate::model::Thing`
/// defined in `model.rs`, through the real importer.
#[cfg(feature = "rust-import-driver-tests")]
#[test]
fn the_app_crate_is_imported_and_a_rawr_module_uses_its_rust_module() {
    use rawr_import::cargo::config::Environment;
    use rawr_import::process::Native;
    use rawr_import::{classify, extract};

    let _turn = rawr_import::store::test_turn();
    let project = Project::new("import");
    for file in ["Cargo.toml", "src/main.rs", "src/view.rs", "src/model.rs"] {
        project.write(file, &fs::read_to_string(fixture().join(file)).unwrap());
    }
    let engine = Engine::new();
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let graph = Graph::load(&engine, &target).unwrap();
    assert!(graph.declares_rust_import(), "a Cargo package with .rs modules declares a Rust import");
    assert_eq!(graph.rust_demand_seed(), [Box::<str>::from("polyglot::model::Thing::new")]);

    let limits = graph.rust_limits();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let environment = Environment::capture().and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true")).unwrap();
    let import = |graph: &Graph| {
        graph
            .rust_import(environment.clone(), &limits)
            .expect("the app crate request seals")
            .expect("the package declares a Rust import")
            .resolve(&native, &limits)
            .and_then(|resolution| extract::from_resolution(resolution, &native, &limits))
            .expect("the generated app crate builds without its marked modules and extracts")
            .classify(&classify::Policy::standard(), &limits)
            .expect("the app crate classifies")
    };
    let contract = import(&graph);
    let generated = fs::read_to_string(project.path().join("target/rawr/crate/lib.rs")).unwrap();
    assert!(generated.contains("pub mod model;") && !generated.contains("view"), "{generated}");

    let source = rawr::reload::source::Graph::from_package(&graph);
    let snapshot = rawr::reload::source::Snapshot::from_graph(&source);
    assert!(snapshot.inputs().paths().iter().any(|path| path.ends_with("src/model.rs")));
    assert!(!snapshot.inputs().is_source_only(&project.path().join("src/model.rs")));
    let demand = engine.compile_imported(&contract, snapshot).expect("crate::model::Thing resolves and type-checks");
    assert!(demand.entries().len() >= 2, "Thing::new and doubled are demanded");

    // An edit to the `.rs` module reaches the next import, and a
    // `pub(crate)` method is visible to the marked modules of its crate.
    let model = fs::read_to_string(project.path().join("src/model.rs")).unwrap();
    project.write("src/model.rs", &model.replace("    pub fn doubled", "    pub(crate) fn tripled(&self) -> i64 {\n        self.count * 3\n    }\n\n    pub fn doubled"));
    project.write("src/view.rs", "// rawr\nuse crate::model::Thing;\n\npub fn total() -> i64 {\n    Thing::new(1).tripled()\n}\n");
    let graph = Graph::load(&engine, &target).unwrap();
    let contract = import(&graph);
    let snapshot = rawr::reload::source::Snapshot::from_graph(&rawr::reload::source::Graph::from_package(&graph));
    engine.compile_imported(&contract, snapshot).expect("the new pub(crate) Rust method is imported");
    let copy = fs::read_to_string(project.path().join("target/rawr/crate/src/model.rs")).unwrap();
    assert!(copy.contains("    pub        fn tripled"), "{copy}");
}
