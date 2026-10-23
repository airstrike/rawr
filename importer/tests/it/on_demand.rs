//! A dependency Rawr names is extracted from the paths it
//! names, each with its closure (signatures, public fields, inherent and
//! trait impl tables), instead of from every public path.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_import::api::identity::Definition as Id;
use rawr_import::api::ledger::Lookup;
use rawr_import::api::{ledger, model};
use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
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
            "rawr-on-demand-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("the unique on-demand project can be created");
        let root = fs::canonicalize(root).expect("the on-demand project has a canonical root");
        Self { root }
    }
}

#[test]
fn a_named_type_extracts_only_its_closure() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let graph = extract(&project, &[&["app", "widget", "Button"]]);

    let button = imported(&graph, &["widget", "Button"]).expect("the named type is exported");
    let button = definition(&graph, button.target());
    assert!(
        inherent_member(&graph, button.id(), "new").is_some(),
        "the named type brings its inherent impl table"
    );
    assert!(
        implementations_of(&graph, button.id()).any(|implementation| trait_named(
            &graph,
            implementation,
            "Clone"
        )),
        "the named type brings its trait impl table"
    );
    assert!(
        named(&graph, "Element"),
        "a `From` impl in the type's table brings its target's shape"
    );
    for unnamed in ["Slider", "Unnamed", "slider", "button"] {
        assert!(
            !named(&graph, unnamed),
            "`{unnamed}` is not named, so it is not extracted"
        );
    }
    assert!(
        imported(&graph, &["widget", "Unnamed"]).is_none(),
        "an unextracted item has no export"
    );
}

#[test]
fn a_new_demanded_item_is_extracted_and_demand_order_is_irrelevant() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let first = extract(&project, &[&["app", "widget", "Button"]]);
    assert!(imported(&first, &["widget", "slider"]).is_none());

    // A second prepare naming a new item through a glob re-export, plus a
    // typo, which extracts nothing and stays a missing item.
    let demand: &[&[&str]] = &[
        &["app", "widget", "Button"],
        &["app", "widget", "slider"],
        &["app", "widget", "Missing"],
    ];
    let forward = extract(&project, demand);
    assert!(
        forward.exports().any(|outcome| match outcome {
            ledger::Path::Imported(export) =>
                export.key().path().eq(["widget", "slider"])
                    && matches!(
                        definition(&forward, export.target()).kind(),
                        model::Kind::Function { .. }
                    ),
            _ => false,
        }),
        "the newly named function is extracted through the glob re-export"
    );
    assert!(
        imported(&forward, &["widget", "slider", "Slider"]).is_some()
            && imported(&forward, &["widget", "Slider"]).is_some(),
        "the type its signature returns is named through each public spelling"
    );
    assert!(imported(&forward, &["widget", "Missing"]).is_none());
    assert!(imported(&forward, &["widget", "Button"]).is_some());

    let reversed = demand.iter().rev().copied().collect::<Vec<_>>();
    let backward = extract(&project, &reversed);
    assert_eq!(
        forward.canonical_bytes(),
        backward.canonical_bytes(),
        "the extracted API is a function of the demand set, not its order"
    );
    let again = extract(&project, demand);
    assert_eq!(forward.canonical_bytes(), again.canonical_bytes());
}

#[test]
fn a_demanded_type_keeps_its_cross_crate_from_impls() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let graph = extract(&project, &[&["app", "Element"]]);

    let element = imported(&graph, &["Element"]).expect("the named type is exported");
    let element = definition(&graph, element.target()).id();
    let deep = graph
        .definitions()
        .find(|definition| {
            definition.display_path().last() == Some("Deep")
                && definition.id().unit().crate_name() == "inner_api"
        })
        .expect("the source type of the cross-crate `From` impl is extracted");
    let conversions = implementations_of(&graph, element)
        .filter(|implementation| trait_named(&graph, implementation, "From"))
        .collect::<Vec<_>>();
    assert_eq!(
        conversions.len(),
        2,
        "both `From` impls on the demanded type are present"
    );
    assert!(
        conversions
            .iter()
            .any(|implementation| match implementation.trait_ref() {
                model::Trait::Trait { arguments, .. } => arguments.iter().any(|argument| matches!(
                    argument,
                    model::Argument::Type(model::Type::Definition { id, .. }) if id == deep.id()
                )),
                model::Trait::Inherent => false,
            }),
        "`From<inner_api::Deep> for Element` is present"
    );
    assert!(
        imported(&graph, &["inner_api", "Deep"]).is_some(),
        "the foreign type is named through the wholesale re-export"
    );
}

/// A demanded dependency publishes its complete name index (every
/// public path, extracted or not), and a glob demand selects every public
/// child of its module.
#[test]
fn the_name_index_covers_every_public_path_and_a_glob_selects_children() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let graph = extract(&project, &[&["app", "widget", "Button"]]);
    let index = graph
        .name_index()
        .find(|index| index.root_alias() == "app")
        .expect("the demanded dependency has a name index");
    for present in [
        &["widget", "Unnamed"][..],
        &["widget", "Slider"],
        &["widget", "slider", "Slider"],
    ] {
        assert_eq!(index.lookup(present), Lookup::Present, "{present:?}");
    }
    assert_eq!(index.lookup(&["inner_api", "Deep"]), Lookup::ThroughCrate);
    assert_eq!(
        index.lookup(&["widget", "buton"]),
        Lookup::Missing {
            parent: "app::widget".into(),
            missing: "buton".into(),
            suggestion: Some("button".into()),
        }
    );

    let glob = extract(&project, &[&["app", "widget", "*"]]);
    for child in ["Button", "Unnamed", "button", "slider", "Slider"] {
        assert!(imported(&glob, &["widget", child]).is_some(), "{child}");
    }
}

fn named(graph: &Graph, name: &str) -> bool {
    graph
        .definitions()
        .any(|definition| definition.display_path().last() == Some(name))
}

fn implementations_of<'a>(
    graph: &'a Graph,
    id: &'a Id,
) -> impl Iterator<Item = &'a model::Implementation> {
    graph.implementations().filter(move |implementation| {
        matches!(implementation.self_type(), model::Type::Definition { id: target, .. } if target == id)
    })
}

fn inherent_member<'a>(
    graph: &'a Graph,
    owner: &'a Id,
    name: &str,
) -> Option<&'a model::Definition> {
    implementations_of(graph, owner)
        .filter(|implementation| matches!(implementation.trait_ref(), model::Trait::Inherent))
        .flat_map(model::Implementation::associated)
        .map(|id| definition(graph, id))
        .find(|definition| definition.display_path().last() == Some(name))
}

fn trait_named(graph: &Graph, implementation: &model::Implementation, name: &str) -> bool {
    match implementation.trait_ref() {
        model::Trait::Trait { id, .. } => graph.definitions().any(|definition| {
            definition.id() == id && definition.display_path().last() == Some(name)
        }),
        model::Trait::Inherent => false,
    }
}

fn imported<'a>(graph: &'a Graph, path: &[&str]) -> Option<&'a ledger::Export> {
    graph.exports().find_map(|outcome| match outcome {
        ledger::Path::Imported(export)
            if export.key().root_alias() == "app"
                && export.key().path().eq(path.iter().copied()) =>
        {
            Some(export)
        }
        _ => None,
    })
}

fn definition<'a>(graph: &'a Graph, id: &Id) -> &'a model::Definition {
    graph
        .definitions()
        .find(|definition| definition.id() == id)
        .expect("an exported target has its definition")
}

fn extract(project: &Project, demand: &[&[&str]]) -> Graph {
    rawr_import::store::isolate_for_tests();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the test environment is sealable");
    let fixture = fs::canonicalize(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demand-api"),
    )
    .expect("the immutable on-demand fixture exists");
    let mut request = Request::builder(&project.root, &project.root.join("target/rawr/cache"), &limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(Policy::manifest(&limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("app", &fixture, &limits)
                .and_then(|dependency| dependency.package("demand-api"))
                .expect("the fixture selection is exact"),
        )
        .expect("the fixture dependency fits the request");
    for path in demand {
        request = request
            .demand(path.iter().copied())
            .expect("the demanded path fits the request");
    }
    let resolution = request
        .build()
        .expect("the on-demand request seals")
        .resolve(&native, &limits)
        .expect("the on-demand fixture extraction seals");
    rawr_import::extract::from_resolution(resolution, &native, &limits)
        .expect("the fixture compiler evidence canonicalizes")
}
