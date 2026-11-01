use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use super::{account, Fixture, PRIOR};
use crate::cargo::{self, unit};
use crate::input::build;
use crate::input::dependency::{self, Dependency};
use crate::input::proof::{Entry, Role};
use crate::input::replay::{self, observed::dep_info};
use crate::input::stored::{self, Budget, Recipe};
use crate::input::Usage;
use crate::node;

fn data() -> Dependency {
    Dependency {
        sources: Box::from([PathBuf::from("/a/é.rs"), PathBuf::from("/z/\"\\\t.rs")]),
        environment: Box::from([
            (Box::from("EMPTY"), Some(Box::from(""))),
            (Box::from("K"), Some(Box::from("é\"\\\r\0\t"))),
            (Box::from("UNSET"), None),
        ]),
    }
}

fn serialized(data: &Dependency) -> Vec<u8> {
    serde_json::to_vec(&replay::Dependency {
        sources: &data.sources,
        environment: &data.environment,
    })
    .expect("reference schema")
}

fn ownership(package: &str, execution: &Path) -> Usage {
    Usage {
        items: 3,
        bytes: 2 * package.len() + 1 + execution.as_os_str().len() + 64,
    }
}

#[test]
fn associated_dep_info_matches_legacy_after_file_removal_without_source_copies() {
    let fixture = Fixture::new();
    let path = fixture.0.join("./target.d");
    fs::write(fixture.0.join("a.rs"), b"pub fn a() {}\n").expect("first source");
    fs::write(fixture.0.join("b file.rs"), b"pub fn b() {}\n").expect("escaped source");
    fs::write(&path, format!(
        "target: b\\ file.rs a.rs a.rs\n# env-dep:Z=v=more\n# env-dep:EMPTY=\n# env-dep:UNSET\n# env-dep:OUT_DIR={}\n",
        fixture.out().display(),
    )).expect("associated dep-info");
    let mut build = fixture.build();
    build.build.evidence.artifacts = Box::from([build::Artifact {
        package_id: "pkg".into(),
        outputs: Box::default(),
        dep_info: path.clone(),
        key: build::Key {
            package_id: "pkg".into(),
            target: fixture.0.join("a.rs"),
            target_kinds: Box::from([Box::from("lib")]),
            crate_types: Box::from([Box::from("lib")]),
            crate_name: "pkg".into(),
            features: Box::default(),
        },
        profile: unit::ArtifactProfile::from_build(
            &serde_json::json!({"opt_level":"0", "debuginfo":2,
                "debug_assertions":true, "overflow_checks":true, "test":false}),
        )
        .expect("artifact profile"),
        fresh: false,
    }]);
    let directories = Box::from([fixture.out()]);
    let recipe = Recipe::DepInfo {
        package_id: "pkg".into(),
        path: path.clone(),
        package_root: fixture.0.clone(),
        out_directories: directories,
    };
    let graph = node::GraphBudget::new();
    let mut allocation = account();
    let data = dependency::observed::read(
        recipe.view(),
        &build,
        &fixture.0,
        &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
    )
    .expect("associated current dep-info");
    let expected =
        replay::dep_info_entry("pkg", &path, &fixture.0, &[fixture.out()], &mut account())
            .expect("complete legacy fact");
    let pointers = (
        data.sources.as_ptr(),
        data.sources[0].as_os_str().as_encoded_bytes().as_ptr(),
        data.environment.as_ptr(),
        data.environment[0].0.as_ptr(),
    );
    fs::remove_file(&path).expect("remove parsed dep-info");
    fs::remove_file(fixture.0.join("a.rs")).expect("remove observed source");
    fs::remove_file(fixture.0.join("b file.rs")).expect("remove other source");
    graph.exhaust_paths();
    graph.exhaust_comparisons();
    let before = allocation.usage();
    let actual = dep_info(
        &data,
        "pkg",
        &path,
        &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
    )
    .expect("borrowed formatting performs no I/O");
    assert!(actual == expected, "current fact differs from legacy");
    let Entry::Fact {
        role,
        package_id,
        name,
        ..
    } = actual
    else {
        panic!("Fact")
    };
    assert_eq!(role, Role::DepInfo);
    assert_eq!(package_id.as_deref(), Some("pkg"));
    assert_eq!(
        name.as_ref(),
        format!("pkg:{}", path.to_str().expect("fixture path"))
    );
    assert!(name.contains("/./target.d"));
    assert_eq!(
        pointers,
        (
            data.sources.as_ptr(),
            data.sources[0].as_os_str().as_encoded_bytes().as_ptr(),
            data.environment.as_ptr(),
            data.environment[0].0.as_ptr()
        )
    );
    let added = ownership("pkg", &path);
    assert_eq!(
        allocation.usage(),
        Usage {
            items: before.items + added.items,
            bytes: before.bytes + added.bytes
        }
    );
}

#[test]
fn shared_sources_then_environment_schema_has_exact_strings_and_escaping() {
    let data = data();
    let expected = r#"{"sources":["/a/é.rs","/z/\"\\\t.rs"],"environment":[["EMPTY",""],["K","é\"\\\r\u0000\t"],["UNSET",null]]}"#;
    assert_eq!(serialized(&data), expected.as_bytes());
    let converted: Vec<Box<str>> = data
        .sources
        .iter()
        .map(|path| path.to_str().expect("Unicode source").into())
        .collect();
    assert_eq!(
        serde_json::to_vec(&replay::Dependency {
            sources: &converted,
            environment: &data.environment,
        })
        .expect("legacy string schema"),
        expected.as_bytes()
    );
    let actual = dep_info(
        &data,
        "pkg",
        Path::new("relative/./target.d"),
        &mut Budget::borrow(
            &cargo::Limits::standard(),
            &mut account(),
            &node::GraphBudget::new(),
        ),
    )
    .expect("ordinary fact");
    let Entry::Fact { name, value, .. } = actual else {
        panic!("Fact")
    };
    assert_eq!(name.as_ref(), "pkg:relative/./target.d");
    assert_eq!(
        value.as_ref(),
        format!("{:x}", Sha256::digest(expected.as_bytes()))
    );
}

#[test]
fn exact_original_remaining_ownership_and_refusal_preserve_prior_usage() {
    let data = data();
    let execution = Path::new("relative/./target.d");
    let added = ownership("pkg", execution);
    let mut allocation = account();
    let result = dep_info(
        &data,
        "pkg",
        execution,
        &mut Budget::borrow(
            &cargo::Limits::standard(),
            &mut allocation,
            &node::GraphBudget::new(),
        ),
    );
    result.expect("admitted");
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + added.items,
            bytes: PRIOR.bytes + added.bytes,
        }
    );
    // A genuine failure latches the original account, preserving the
    // spent usage; a latched account does no further work.
    allocation.refuse();
    let before = allocation.usage();
    assert!(matches!(
        dep_info(
            &data,
            "pkg",
            execution,
            &mut Budget::borrow(
                &cargo::Limits::standard(),
                &mut allocation,
                &node::GraphBudget::new()
            )
        ),
        Err(stored::Error::Refused)
    ));
    assert_eq!(allocation.usage(), before);
    assert!(matches!(
        allocation.items(0),
        Err(crate::Error::InputAccountRefused)
    ));
}

#[test]
fn json_hash_and_owned_entries_are_charged_exactly() {
    let data = data();
    let execution = Path::new("target.d");
    let length = serialized(&data).len();
    let added = ownership("pkg", execution);
    // Owned entries are copies charged once to the original allocation; the
    // graph's decode axis does not measure them, and the structured JSON is
    // hashed once.
    let graph = node::GraphBudget::new();
    let mut allocation = account();
    dep_info(
        &data,
        "pkg",
        execution,
        &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
    )
    .expect("dep-info fact");
    assert_eq!(
        graph.census_usage().expect("graph census"),
        (0, 0, length as u64)
    );
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + added.items,
            bytes: PRIOR.bytes + added.bytes,
        }
    );
}

#[test]
fn source_preflight_is_ordered_borrowed_and_precedes_hashing() {
    let invalid = PathBuf::from(OsString::from_vec(b"/invalid/\xff".to_vec()));
    let oversized = PathBuf::from("a".repeat(crate::input::LINE_BYTES + 1));
    for reverse in [false, true] {
        let sources = if reverse {
            [oversized.clone(), invalid.clone()]
        } else {
            [invalid.clone(), oversized.clone()]
        };
        let data = Dependency {
            sources: Box::from(sources),
            environment: Box::default(),
        };
        let graph = node::GraphBudget::new();
        let mut allocation = account();
        let result = dep_info(
            &data,
            "pkg",
            Path::new("target.d"),
            &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
        );
        // The invalid encoding is a semantic rejection in both orders.
        assert!(matches!(
            result,
            Err(stored::Error::Encoding(
                "structured replay path is not Unicode"
            ))
        ));
        allocation
            .items(0)
            .expect("an encoding rejection does not latch");
        assert_eq!(allocation.usage(), PRIOR);
        // Nothing was hashed or decoded.
        assert_eq!(graph.census_usage().expect("graph census"), (0, 0, 0));
    }
}

#[test]
fn execution_path_bounds_and_formatted_name_keep_distinct_scalar_semantics() {
    let data = data();
    let limits = cargo::Limits::standard();
    let exact = PathBuf::from("x".repeat(crate::input::LINE_BYTES));
    let mut exact_account = account();
    let Entry::Fact { name, value, .. } = dep_info(
        &data,
        "pkg",
        &exact,
        &mut Budget::borrow(&limits, &mut exact_account, &node::GraphBudget::new()),
    )
    .expect("formatted name and digest are not scalar strings") else {
        panic!("Fact")
    };
    assert_eq!(name.len(), crate::input::LINE_BYTES + 4);
    assert_eq!(value.len(), 64);

    // An execution path one byte over the line bound still forms the fact
    // after the structured hash.
    let oversized = PathBuf::from("x".repeat(crate::input::LINE_BYTES + 1));
    let graph = node::GraphBudget::new();
    let mut allocation = account();
    dep_info(
        &data,
        "pkg",
        &oversized,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("an oversized execution path forms the fact");
    let added = ownership("pkg", &oversized);
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + added.items,
            bytes: PRIOR.bytes + added.bytes,
        }
    );
    assert_eq!(
        graph.census_usage().expect("graph census").2,
        serialized(&data).len() as u64
    );

    // An invalid execution encoding is still a semantic rejection after the
    // structured hash, with no ownership.
    let invalid = PathBuf::from(OsString::from_vec(b"execution\xff".to_vec()));
    let graph = node::GraphBudget::new();
    let mut allocation = account();
    assert!(matches!(
        dep_info(
            &data,
            "pkg",
            &invalid,
            &mut Budget::borrow(&limits, &mut allocation, &graph)
        ),
        Err(stored::Error::Encoding(
            "structured replay path is not Unicode"
        ))
    ));
    assert_eq!(allocation.usage(), PRIOR);
    assert_eq!(
        graph.census_usage().expect("graph census").2,
        serialized(&data).len() as u64
    );
}
