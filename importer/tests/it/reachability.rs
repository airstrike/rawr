use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_import::api::identity::Authority;
use rawr_import::api::{ledger, model};
use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;
use rawr_import::process::Native;

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
            "rawr-reachability-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("the unique reachability project can be created");
        let root = fs::canonicalize(root).expect("the reachability project has a canonical root");
        Self { root }
    }

    fn cache(&self) -> PathBuf {
        self.root.join(".rawr/cache")
    }
}

#[test]
fn reexports_keep_alias_evidence_while_sysroot_support_stays_leaf_only() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let resolution = request(&project, &limits)
        .resolve(&native, &limits)
        .expect("the one-pass fixture extraction seals");
    let compiler_commit = resolution.toolchain().rustc_commit().to_owned();
    let selected_target = resolution.target().to_owned();
    let graph = rawr_import::extract::from_resolution(resolution, &native, &limits)
        .expect("the fixture compiler evidence canonicalizes");

    let point = imported(&graph, "geometry", &["Point"]);
    let point_aliases = point.aliases().collect::<Vec<_>>();
    assert_eq!(point_aliases.len(), 1);
    assert_eq!(
        point_aliases[0].from().collect::<Vec<_>>(),
        ["shapes", "Point"]
    );
    assert_eq!(point_aliases[0].to().collect::<Vec<_>>(), ["Point"]);
    assert!(graph
        .exports()
        .all(|outcome| outcome.key().path().collect::<Vec<_>>() != ["shapes"]));

    let product = imported(&graph, "catalog", &["Product"]);
    let product_aliases = product.aliases().collect::<Vec<_>>();
    assert_eq!(product_aliases.len(), 1);
    assert_eq!(
        product_aliases[0].from().collect::<Vec<_>>(),
        ["inventory", "Product"]
    );
    assert_eq!(product_aliases[0].to().collect::<Vec<_>>(), ["Product"]);
    let inventory_product = imported(&graph, "catalog", &["inventory", "Product"]);
    assert_eq!(product.target(), inventory_product.target());

    assert!(graph.exports().all(|outcome| matches!(
        outcome.key().namespace().as_str(),
        "type" | "value" | "macro"
    )));
    assert!(graph.exports().all(|outcome| match outcome {
        ledger::Path::Imported(export) => {
            matches!(export.target().unit().authority(), Authority::Cargo(_))
        }
        ledger::Path::Excluded { .. } | ledger::Path::Rejected { .. } => true,
    }));

    let availability = graph
        .definitions()
        .find(|definition| definition.display_path().last() == Some("Availability"))
        .expect("catalog availability definition is retained");
    let model::Kind::Enum { variants, .. } = availability.kind() else {
        panic!("catalog availability remains an enum");
    };
    assert_eq!(variants.len(), 3);
    assert_eq!(variants[0].field_set().form(), model::FieldForm::Unit);
    assert_eq!(variants[1].field_set().form(), model::FieldForm::Tuple);
    assert_eq!(variants[2].field_set().form(), model::FieldForm::Record);
    assert!(variants
        .iter()
        .all(|variant| variant.field_set().public_complete()));

    let sysroot_definitions = graph
        .definitions()
        .filter(|definition| matches!(definition.id().unit().authority(), Authority::Sysroot(_)))
        .collect::<Vec<_>>();
    assert!(sysroot_definitions.iter().any(|definition| {
        definition.display_path().last() == Some("String")
            && matches!(definition.kind(), model::Kind::Sysroot { .. })
    }));
    assert!(sysroot_definitions.iter().any(|definition| {
        definition.display_path().last() == Some("Option")
            && matches!(definition.kind(), model::Kind::Sysroot { .. })
    }));
    assert!(sysroot_definitions
        .iter()
        .all(|definition| matches!(definition.kind(), model::Kind::Sysroot { .. })));

    let mut support = BTreeSet::new();
    let mut sysroot_support = BTreeSet::new();
    for outcome in graph.support() {
        let (id, provenance, role) = match outcome {
            ledger::Support::Complete {
                id,
                provenance,
                role,
            }
            | ledger::Support::Rejected {
                id,
                provenance,
                role,
                ..
            } => (id, provenance, role),
        };
        assert!(
            support.insert((id.clone(), provenance.clone(), role)),
            "every support identity/provenance/role edge has one outcome"
        );
        if let Authority::Sysroot(authority) = id.unit().authority() {
            assert_eq!(authority.compiler_commit(), compiler_commit.as_str());
            assert_eq!(authority.selected_target(), selected_target.as_str());
            assert_eq!(authority.stable_crate_id().len(), 16);
            assert!(authority
                .stable_crate_id()
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
            sysroot_support.insert(authority.crate_name().to_owned());
        }
    }
    assert!(sysroot_support.contains("alloc"));
    assert!(sysroot_support.contains("core"));
    assert!(graph.implementations().count() >= 2);
    assert!(graph
        .implementations()
        .all(|implementation| implementation.associated().len() >= 2));
}

fn imported<'a>(
    graph: &'a rawr_import::api::Graph<rawr_import::api::Extracted>,
    root: &str,
    expected_path: &[&str],
) -> &'a ledger::Export {
    graph
        .exports()
        .find_map(|outcome| match outcome {
            ledger::Path::Imported(export)
                if export.key().root_alias() == root
                    && export.key().path().eq(expected_path.iter().copied()) =>
            {
                Some(export)
            }
            ledger::Path::Imported(_)
            | ledger::Path::Excluded { .. }
            | ledger::Path::Rejected { .. } => None,
        })
        .unwrap_or_else(|| panic!("missing imported path {root}::{expected_path:?}"))
}

fn request(project: &Project, limits: &Limits) -> rawr_import::cargo::Request {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the test environment is sealable");
    Request::builder(&project.root, &project.cache(), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("geometry", &fixture("geometry-api"), limits)
                .and_then(|dependency| dependency.package("geometry-api"))
                .expect("the geometry fixture selection is exact"),
        )
        .expect("the geometry dependency fits the request")
        .dependency(
            Dependency::path("catalog", &fixture("catalog-api"), limits)
                .and_then(|dependency| dependency.package("catalog-api"))
                .expect("the catalog fixture selection is exact"),
        )
        .expect("the catalog dependency fits the request")
        .build()
        .expect("the reachability request seals")
}

fn fixture(name: &str) -> PathBuf {
    fs::canonicalize(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("the immutable reachability fixture exists")
}
