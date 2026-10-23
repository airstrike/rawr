#![cfg(feature = "rust-import-driver-tests")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rawr::{Engine, package, project, reload};
use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::classify;
use rawr_import::extract;
use rawr_import::process::Native;

static NEXT: AtomicU64 = AtomicU64::new(1);

#[test]
fn real_extracted_contract_is_snapshot_bound_positioned_and_symbolic() {
    let _turn = rawr_import::store::test_turn();
    let capture = Fixture::new("");
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let contract = extracted_contract(&capture.root, &native, &limits).expect("the unchanged Rust fixture extracts and classifies");
    let record = contract
        .outcomes()
        .find(|outcome| outcome.export().root_alias() == "fixture" && outcome.export().path().eq(["Record"]))
        .expect("the selected facade retains Record");
    let record_associated = record
        .implementations()
        .flat_map(rawr_import::classify::ImplementationOutcome::associated)
        .map(|outcome| (outcome.name(), outcome.representability(), outcome.identity().selector()))
        .collect::<Vec<_>>();
    assert!(
        record_associated
            .iter()
            .any(|(name, representability, selector)| { *name == "shared" && matches!(representability, rawr_import::classify::representability::Representability::Method(_)) && matches!(selector, rawr_import::surface::identity::Selector::Method { .. }) }),
        "Record associated outcomes: {record_associated:#?}"
    );
    let engine = Engine::new();

    let positive = Fixture::new("use fixture::integer_only; integer_only()\n");
    let demand = engine
        .compile_imported(&contract, snapshot(&engine, &positive.source))
        .expect("the extracted free function type-checks symbolically");
    assert_eq!(demand.entries().len(), 1);
    assert!(matches!(demand.entries().next().map(|entry| entry.identity().selector()), Some(rawr_import::surface::identity::Selector::Function { .. })));
    assert_eq!(demand.contract_fingerprint(), contract.fingerprint());
    assert!(!demand.canonical_bytes().is_empty());

    let pathless_return = Fixture::new("use fixture::make_record; let value = make_record();\n");
    let pathless_demand = engine
        .compile_imported(&contract, snapshot(&engine, &pathless_return.source))
        .expect("a free return stays semantic when its type has same-facade aliases");
    assert_eq!(pathless_demand.entries().len(), 1);

    let ambiguous_member = Fixture::new("use fixture::make_record; make_record().left\n");
    let error = match engine.compile_imported(&contract, snapshot(&engine, &ambiguous_member.source)) {
        Ok(_) => panic!("a member use without an originating alias must be positioned ambiguity"),
        Err(error) => error,
    };
    let report = error.report().expect("alias ambiguity keeps exact Rust evidence");
    assert_eq!(error.category(), package::Category::Resolution);
    assert_eq!(report.labels().len(), 1);
    assert!(!report.external_labels().is_empty());
    assert!(error.to_string().contains("choose one of:"));

    let grammar = Fixture::new(
        r#"
use fixture::{Forms, Record, Tuple, Unit, private_record};
Unit;
let tuple = Tuple(1, 2);
tuple.0;
let record = Record { left: 1, right: 2 };
record.shared();
Forms::Unit;
Forms::Tuple(1, 2);
Forms::Named { left: 1, right: 2 };
let opaque = private_record();
opaque.visible
"#,
    );
    let grammar_demand = engine
        .compile_imported(&contract, snapshot(&engine, &grammar.source))
        .expect("all admitted constructor forms and readable public fields stay symbolic");
    assert!(grammar_demand.entries().len() >= 10);

    for source in [
        "use fixture::Record; Record::new(1, 2).shared()\n",
        "use fixture::Record; let mut value = Record { left: 1, right: 2 }; value.mutable()\n",
        "use fixture::Record; let value = Record { left: 1, right: 2 }; value.owned()\n",
        "use fixture::Record; Record::ONE\n",
    ] {
        let associated = Fixture::new(source);
        engine
            .compile_imported(&contract, snapshot(&engine, &associated.source))
            .expect("associated function, constant, and receiver mode remain symbolic");
    }

    let mut recursive_aliases = Vec::new();
    for root in ["fixture", "fixture_b"] {
        let alias = Fixture::new(&format!("use {root}::Record; let value = Record {{ left: 1, right: 2 }}; value.recursive().left\n"));
        let demand = engine
            .compile_imported(&contract, snapshot(&engine, &alias.source))
            .expect("recursive output remains on its exact selected facade alias");
        recursive_aliases.push(
            demand
                .entries()
                .find(|entry| matches!(entry.identity().selector(), rawr_import::surface::identity::Selector::Method { .. }))
                .expect("the recursive method is demanded")
                .identity()
                .clone(),
        );
    }
    assert_ne!(recursive_aliases[0], recursive_aliases[1]);

    let duplicate = Fixture::new("use fixture::integer_only; let first = integer_only(); integer_only()\n");
    let duplicated = engine
        .compile_imported(&contract, snapshot(&engine, &duplicate.source))
        .expect("duplicate positioned uses remain valid demand");
    assert_eq!(duplicated.entries().len(), 2);
    let entries = duplicated.entries().collect::<Vec<_>>();
    assert_eq!(entries[0].identity(), entries[1].identity());
    assert_ne!(entries[0].origin(), entries[1].origin());

    let deferred = Fixture::new("use fixture::PrimitiveAlias; PrimitiveAlias\n");
    let error = match engine.compile_imported(&contract, snapshot(&engine, &deferred.source)) {
        Ok(_) => panic!("a contract-known deferred import must reject"),
        Err(error) => error,
    };
    let report = error.report().expect("dual-origin evidence is valid");
    assert_eq!(error.category(), package::Category::Resolution);
    assert_eq!(report.labels().len(), 1);
    assert!(!report.external_labels().is_empty());

    let namespace_collision = Fixture::new("use fixture::ImplSurface; ImplSurface\n");
    let first = engine
        .compile_imported(&contract, snapshot(&engine, &namespace_collision.source))
        .expect("an admitted type sibling resolves without unavailable value evidence");
    let repeated_collision = Fixture::new("use fixture::ImplSurface; ImplSurface\n");
    let second = engine
        .compile_imported(&contract, snapshot(&engine, &repeated_collision.source))
        .expect("the admitted sibling remains deterministic");
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert!(
        first
            .entries()
            .all(|entry| { matches!(entry.identity().selector(), rawr_import::surface::identity::Selector::StructConstructor(_)) })
    );

    let unknown = Fixture::new("use fixture::missing; missing\n");
    let error = match engine.compile_imported(&contract, snapshot(&engine, &unknown.source)) {
        Ok(_) => panic!("a genuinely unknown import must reject"),
        Err(error) => error,
    };
    let report = error.report().expect("unknown import evidence remains valid");
    assert_eq!(error.category(), package::Category::Resolution);
    assert_eq!(report.labels().len(), 1);
    assert!(report.external_labels().is_empty());

    for source in [
        "use fixture::Record; let value = Record { left: 1, right: 2 }; value.missing\n",
        "use fixture::Tuple; Tuple(1)\n",
        "use fixture::Forms; Forms::Missing\n",
        "use fixture::integer_only; integer_only(1)\n",
        "use fixture::Record; let value = Record { left: 1, right: 2 }; value.mutable()\n",
        "use fixture::private_record; let value = private_record(); value.owned(); value.hidden()\n",
        "use fixture::PrivateRecord; PrivateRecord { visible: 1 }\n",
    ] {
        let invalid = Fixture::new(source);
        let error = match engine.compile_imported(&contract, snapshot(&engine, &invalid.source)) {
            Ok(_) => panic!("invalid admitted imported grammar must reject: {source}"),
            Err(error) => error,
        };
        let report = error.report().expect("invalid imported grammar retains diagnostic evidence");
        assert!(matches!(error.category(), package::Category::Compilation | package::Category::Resolution), "{source}");
        assert!(!report.labels().is_empty(), "{source}");
        assert!(!report.external_labels().is_empty(), "{source}");
    }

    let stale = Fixture::new("use fixture::integer_only; integer_only()\n");
    let stale_snapshot = snapshot(&engine, &stale.source);
    fs::write(&stale.source, "use fixture::integer_only; integer_only() // changed\n").expect("fixture body changes after snapshot");
    let error = match engine.compile_imported(&contract, stale_snapshot) {
        Ok(_) => panic!("stale source cannot mint sealed demand"),
        Err(error) => error,
    };
    assert_eq!(error.category(), package::Category::Resource);
}

/// A path through a wholesale crate re-export (`pub use inner_api;`)
/// that extraction only named is reported as demand; importing again with it
/// demanded resolves the path and type-checks the call.
#[test]
fn a_path_through_a_reexported_crate_is_demanded_then_resolves() {
    let _turn = rawr_import::store::test_turn();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let engine = Engine::new();
    let source = Fixture::new("use outer::inner_api::nested::deeper; let value = deeper();\n");

    let undemanded = reexport_contract(&source.root, &native, &limits, &[]);
    let error = match engine.compile_imported(&undemanded, snapshot(&engine, &source.source)) {
        Ok(_) => panic!("an unextracted path through the alias cannot resolve"),
        Err(error) => error,
    };
    assert_eq!(error.rust_demand(), Some("outer::inner_api::nested::deeper"));

    let demanded = reexport_contract(&source.root, &native, &limits, &["outer", "inner_api", "nested", "deeper"]);
    let demand = engine
        .compile_imported(&demanded, snapshot(&engine, &source.source))
        .expect("the demanded path resolves and its call type-checks");
    assert!(
        demand
            .entries()
            .any(|entry| matches!(entry.identity().selector(), rawr_import::surface::identity::Selector::Function { .. }))
    );

    let unrelated = Fixture::new("use outer::missing; missing()\n");
    let error = match engine.compile_imported(&undemanded, snapshot(&engine, &unrelated.source)) {
        Ok(_) => panic!("a missing path outside the alias stays unknown"),
        Err(error) => error,
    };
    assert_eq!(error.rust_demand(), None);
}

/// The demand a launch discovers is remembered beside the project
/// cache, so the next preparation starts from it and resolves on its first
/// attempt; a malformed file is an empty set.
#[test]
fn a_remembered_crate_demand_resolves_on_the_first_attempt() {
    let _turn = rawr_import::store::test_turn();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let engine = Engine::new();
    let source = Fixture::new("use outer::inner_api::nested::deeper; let value = deeper();\n");
    let path = "outer::inner_api::nested::deeper";

    fs::create_dir_all(source.root.join("target/rawr/cache")).expect("the project cache exists");
    fs::write(source.root.join("target/rawr/cache/demand"), "outer\n").expect("a malformed demand file");
    assert!(rawr_import::crate_demand::load(&source.root).is_empty());

    rawr_import::crate_demand::store(&source.root, &[path.into()]);
    let remembered = rawr_import::crate_demand::load(&source.root);
    assert_eq!(remembered, [Box::<str>::from(path)]);
    let segments = remembered[0].split("::").collect::<Vec<_>>();
    let contract = reexport_contract(&source.root, &native, &limits, &segments);
    engine
        .compile_imported(&contract, snapshot(&engine, &source.source))
        .expect("the remembered demand resolves on the first attempt");
}

/// A demanded dependency publishes its complete name index, so a typo
/// is an unresolved import with a suggestion, never demand (no extra probe).
#[test]
fn a_typo_in_a_demanded_path_is_unresolved_without_demand() {
    let _turn = rawr_import::store::test_turn();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let engine = Engine::new();
    let source = Fixture::new("use app::widget::buton; buton(1)\n");
    let contract = demand_contract(&source.root, &native, &limits, &["app::widget::Button", "app::widget::buton"]);
    let error = match engine.compile_imported(&contract, snapshot(&engine, &source.source)) {
        Ok(_) => panic!("a typo cannot resolve"),
        Err(error) => error,
    };
    assert_eq!(error.rust_miss(), None, "a path the index lacks is never demanded");
    let message = error.to_string();
    assert!(message.contains("unresolved import `app::widget::buton`") && message.contains("did you mean `button`?"), "{message}");
}

/// A glob import demands every public name of its module, so names it
/// binds (here through the module's own glob re-export) resolve.
#[test]
fn a_glob_import_expands_and_resolves() {
    let _turn = rawr_import::store::test_turn();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let engine = Engine::new();
    let source = Fixture::new("use app::widget::*; let value = slider(1); let made = button(2); Button::new(3)\n");
    let contract = demand_contract(&source.root, &native, &limits, &["app::widget::*"]);
    engine
        .compile_imported(&contract, snapshot(&engine, &source.source))
        .expect("the names the glob binds resolve and type-check");
}

/// A path the index holds but extraction did not is demanded once;
/// importing again with it demanded resolves it.
#[test]
fn a_path_the_index_holds_is_demanded_once_then_resolves() {
    let _turn = rawr_import::store::test_turn();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let engine = Engine::new();
    let source = Fixture::new("use app::widget::{Button, slider}; let value = slider(1); Button::new(2)\n");
    let first = demand_contract(&source.root, &native, &limits, &["app::widget::Button"]);
    let error = match engine.compile_imported(&first, snapshot(&engine, &source.source)) {
        Ok(_) => panic!("an undemanded item is not extracted"),
        Err(error) => error,
    };
    assert_eq!(error.rust_miss(), Some("app::widget::slider"));
    let second = demand_contract(&source.root, &native, &limits, &["app::widget::Button", "app::widget::slider"]);
    engine.compile_imported(&second, snapshot(&engine, &source.source)).expect("the demanded path resolves");
}

fn demand_contract(root: &Path, native: &Native, limits: &Limits, demand: &[&str]) -> rawr_import::Contract<rawr_import::Classified> {
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the fixture environment is sealable");
    let mut builder = Request::builder(root, &root.join("target/rawr/cache"), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(extract::Policy::manifest(limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("app", &rust_fixture("demand-api"), limits)
                .and_then(|dependency| dependency.package("demand-api"))
                .expect("the demand fixture selection is exact"),
        )
        .expect("the fixture dependency fits the request");
    for path in demand {
        builder = builder.demand(path.split("::")).expect("the demanded path fits the request");
    }
    builder
        .build()
        .expect("the fixture request seals")
        .resolve(native, limits)
        .and_then(|resolution| extract::from_resolution(resolution, native, limits))
        .expect("the demand fixture extracts")
        .classify(&classify::Policy::standard(), limits)
        .expect("the demand fixture classifies")
}

fn reexport_contract(root: &Path, native: &Native, limits: &Limits, demand: &[&str]) -> rawr_import::Contract<rawr_import::Classified> {
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the fixture environment is sealable");
    let mut builder = Request::builder(root, &root.join("target/rawr/cache"), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(extract::Policy::manifest(limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("outer", &rust_fixture("outer-api"), limits)
                .and_then(|dependency| dependency.package("outer-api"))
                .expect("the re-exporting fixture selection is exact"),
        )
        .expect("the fixture dependency fits the request");
    if !demand.is_empty() {
        builder = builder.demand(demand.iter().copied()).expect("the demanded path fits the request");
    }
    builder
        .build()
        .expect("the fixture request seals")
        .resolve(native, limits)
        .and_then(|resolution| extract::from_resolution(resolution, native, limits))
        .expect("the re-export fixture extracts")
        .classify(&classify::Policy::standard(), limits)
        .expect("the re-export fixture classifies")
}

fn extracted_contract(root: &Path, native: &Native, limits: &Limits) -> Result<rawr_import::Contract<rawr_import::Classified>, String> {
    let extraction_limits = Limits::standard();
    request(root, &extraction_limits)
        .resolve(native, &extraction_limits)
        .and_then(|resolution| extract::from_resolution(resolution, native, &extraction_limits))
        .map_err(|error| error.to_string())
        .and_then(|graph| graph.classify(&classify::Policy::standard(), limits).map_err(|error| error.to_string()))
}

fn request(root: &Path, limits: &Limits) -> Request {
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the fixture environment is sealable");
    Request::builder(root, &root.join("target/rawr/cache"), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(extract::Policy::manifest(limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("fixture", &rust_fixture("symbolic-api"), limits)
                .and_then(|dependency| dependency.package("symbolic-api"))
                .expect("the real Rust fixture selection is exact"),
        )
        .expect("the fixture dependency fits the request")
        .dependency(
            Dependency::path("fixture_b", &rust_fixture("symbolic-api"), limits)
                .and_then(|dependency| dependency.package("symbolic-api"))
                .expect("the second Rust fixture alias is exact"),
        )
        .expect("the second fixture alias fits the request")
        .build()
        .expect("the fixture request seals")
}

fn snapshot(engine: &Engine, source: &Path) -> reload::source::Snapshot {
    let target = project::Target::discover(source).expect("fixture target discovers");
    let graph = package::Graph::load(engine, &target).expect("fixture package graph loads");
    let source = reload::source::Graph::from_package(&graph);
    reload::source::Snapshot::from_graph(&source)
}

fn rust_fixture(name: &str) -> PathBuf {
    fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("importer/tests/fixtures").join(name)).expect("the immutable Rust fixture exists")
}

struct Fixture {
    root: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new(source: &str) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("rawr-p2-symbolic-{}-{id}", std::process::id()));
        fs::create_dir_all(&root).expect("fixture directory creates");
        let root = fs::canonicalize(root).expect("fixture directory is canonical");
        let source_path = root.join("fixture.rs");
        fs::write(&source_path, source).expect("fixture source writes");
        Self { root, source: source_path }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
