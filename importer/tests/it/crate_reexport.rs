//! A crate re-exported wholesale (`pub use inner_api;`) is an
//! alias export. Its items are extracted only when selected, needed by a
//! selected signature, or named through the alias by Rawr, are named
//! through the alias only then, and are one definition either way.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_import::api::identity::Namespace;
use rawr_import::api::{ledger, model};
use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Builder, Limits, Request};
use rawr_import::extract::Policy;
use rawr_import::process::Native;

type Graph = rawr_import::api::Graph<rawr_import::api::Extracted>;

static TEST_ID: AtomicU64 = AtomicU64::new(1);

struct Project {
    root: PathBuf,
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Project {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-crate-reexport-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("the unique re-export project can be created");
        let root = fs::canonicalize(root).expect("the re-export project has a canonical root");
        Self { root }
    }
}

#[test]
fn a_wholesale_reexport_is_an_alias_export_that_is_not_walked() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let graph = extract(&project, &[("outer", "outer-api")]);

    let alias = imported(&graph, "outer", &["inner_api"]).expect("the re-export is exported");
    assert!(matches!(
        definition(&graph, alias.target()).kind(),
        model::Kind::Module
    ));
    assert!(
        graph.exports().all(|outcome| {
            let path = key(outcome).path().collect::<Vec<_>>();
            !path.starts_with(&["inner_api", "nested"])
        }),
        "an unnamed module of the re-exported crate is not walked for extraction"
    );
    assert!(
        graph
            .definitions()
            .all(|definition| definition.display_path().last() != Some("Deeper")),
        "unnamed items of the re-exported crate are not extracted"
    );
    let deep = imported(&graph, "outer", &["inner_api", "Deep"])
        .expect("a type the selected signatures use is nameable through the alias");
    assert_eq!(
        definition(&graph, deep.target())
            .display_path()
            .collect::<Vec<_>>(),
        ["inner_api", "Deep"]
    );
    let leaf = imported(&graph, "outer", &["hop_api", "Leaf"])
        .expect("a re-export hop through a crate with no extracted items is named");
    assert_eq!(leaf.reexports().count(), 2);
}

#[test]
fn a_reexported_crate_selected_as_its_own_root_gives_one_definition() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let graph = extract(&project, &[("outer", "outer-api"), ("inner", "inner-api")]);

    let direct = imported(&graph, "inner", &["Deep"]).expect("the selected crate is walked");
    let alias = imported(&graph, "outer", &["inner_api"]).expect("the re-export is exported");
    let inner_root = definition(&graph, alias.target());
    assert_eq!(inner_root.id().unit(), direct.target().unit());
    let deep = graph
        .definitions()
        .filter(|definition| {
            definition.display_path().last() == Some("Deep")
                && definition.id().namespace() == Namespace::Type
        })
        .collect::<Vec<_>>();
    assert_eq!(deep.len(), 1, "one definition for `Deep`");
    assert_eq!(deep[0].id(), direct.target());
    assert_eq!(
        deep[0].display_path().collect::<Vec<_>>(),
        ["Deep"],
        "the merged definition keeps its own crate's spelling"
    );
    assert!(imported(&graph, "inner", &["nested", "Deeper"]).is_some());
    let through_alias = imported(&graph, "outer", &["inner_api", "Deep"])
        .expect("the alias names the extracted definition");
    assert_eq!(through_alias.target(), direct.target());
}

#[test]
fn a_path_named_through_the_alias_selects_that_item() {
    let _turn = rawr_import::store::test_turn();
    let demand: &[&[&str]] = &[&["outer", "inner_api", "nested", "deeper"]];
    let project = Project::new();
    let graph = extract_demanding(&project, &[("outer", "outer-api")], demand);
    let deeper = imported(&graph, "outer", &["inner_api", "nested", "deeper"])
        .expect("the demanded function is extracted and named through the alias");
    let function = definition(&graph, deeper.target());
    assert!(matches!(function.kind(), model::Kind::Function { .. }));
    assert_eq!(function.id().unit().crate_name(), "inner_api");
    assert!(
        imported(&graph, "outer", &["inner_api", "nested"]).is_some(),
        "the module on the demanded path is named"
    );
    assert!(
        imported(&graph, "outer", &["inner_api", "nested", "Deeper"]).is_some(),
        "the type its signature needs is named through the alias too"
    );

    let project = Project::new();
    let graph = extract_demanding(
        &project,
        &[("outer", "outer-api"), ("inner", "inner-api")],
        demand,
    );
    let through_alias = imported(&graph, "outer", &["inner_api", "nested", "deeper"])
        .expect("the demanded function is named through the alias");
    let direct = imported(&graph, "inner", &["nested", "deeper"])
        .expect("the selected crate names its own function");
    assert_eq!(through_alias.target(), direct.target());
    assert_eq!(
        graph
            .definitions()
            .filter(|definition| definition.display_path().last() == Some("deeper"))
            .count(),
        1,
        "one definition for `deeper`"
    );
}

/// A facade resolves only the types it declares. A function whose output
/// only another selected crate exports is left out of its own facade, and
/// the rest of both facades stays.
#[test]
fn an_item_naming_another_facades_type_is_unavailable_by_itself() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let limits = Limits::standard();
    let graph = extract(&project, &[("split", "split-api"), ("leaf", "leaf-api")]);
    let contract = graph
        .classify(&rawr_import::classify::Policy::standard(), &limits)
        .expect("the split fixture classifies");
    let plan = rawr_import::surface::facade::Plan::encode(&contract, &limits)
        .and_then(|bytes| rawr_import::surface::facade::Plan::decode(&bytes, &limits))
        .expect("the split facade plan round-trips");
    let facade = |root: &str| {
        plan.facades()
            .find(|facade| facade.root() == root)
            .unwrap_or_else(|| panic!("`{root}` has a facade"))
    };
    let split = facade("split");
    let items = split.items().map(|item| item.path()).collect::<Vec<_>>();
    assert!(items.contains(&"own"), "{items:?}");
    assert!(!items.contains(&"leaf"), "{items:?}");
    assert!(
        facade("leaf").types().any(|ty| ty.path() == "Leaf"),
        "the other crate still declares its own type"
    );
}

fn extract(project: &Project, dependencies: &[(&str, &str)]) -> Graph {
    extract_demanding(project, dependencies, &[])
}

fn extract_demanding(
    project: &Project,
    dependencies: &[(&str, &str)],
    demand: &[&[&str]],
) -> Graph {
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let mut request = request(project, &limits, dependencies);
    for path in demand {
        request = request
            .demand(path.iter().copied())
            .expect("the demanded path fits the request");
    }
    let resolution = request
        .build()
        .expect("the re-export request seals")
        .resolve(&native, &limits)
        .expect("the re-export fixture extraction seals");
    rawr_import::extract::from_resolution(resolution, &native, &limits)
        .expect("the fixture compiler evidence canonicalizes")
}

fn key(outcome: &ledger::Path) -> &rawr_import::api::identity::Export {
    match outcome {
        ledger::Path::Imported(export) => export.key(),
        ledger::Path::Excluded { key, .. } | ledger::Path::Rejected { key, .. } => key,
    }
}

fn imported<'a>(graph: &'a Graph, root: &str, path: &[&str]) -> Option<&'a ledger::Export> {
    graph.exports().find_map(|outcome| match outcome {
        ledger::Path::Imported(export)
            if export.key().root_alias() == root
                && export.key().path().eq(path.iter().copied()) =>
        {
            Some(export)
        }
        _ => None,
    })
}

fn definition<'a>(
    graph: &'a Graph,
    id: &rawr_import::api::identity::Definition,
) -> &'a model::Definition {
    graph
        .definitions()
        .find(|definition| definition.id() == id)
        .expect("an exported target has its definition")
}

fn request(project: &Project, limits: &Limits, dependencies: &[(&str, &str)]) -> Builder {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the test environment is sealable");
    let mut builder = Request::builder(
        &project.root,
        &project.root.join("target/rawr/cache"),
        limits,
    )
    .expect("the limits-bound request builder seals")
    .environment(environment)
    .policy(Policy::manifest(limits).seal())
    .expect("the manifest policy fits the request");
    for (alias, package) in dependencies {
        builder = builder
            .dependency(
                Dependency::path(alias, &fixture(package), limits)
                    .and_then(|dependency| dependency.package(package))
                    .expect("the fixture selection is exact"),
            )
            .expect("the fixture dependency fits the request");
    }
    builder
}

fn fixture(name: &str) -> PathBuf {
    fs::canonicalize(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("the immutable re-export fixture exists")
}
