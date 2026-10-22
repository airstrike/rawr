use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;

use crate::cargo::{unit, Limits};
use crate::input::stored::{Budget, Error};
use crate::input::{build, Allocation, Usage};
use crate::node;

fn retained() -> build::Retained {
    build::Retained {
        build: build::Build {
            evidence: build::Evidence {
                usage: Usage {
                    items: 3,
                    bytes: 17,
                },
                packages: build::packages::Table::from_cold(BTreeMap::new()),
                scripts: Box::default(),
                artifacts: Box::default(),
                environment: vec![(
                    OsString::from_vec(b"N\xff".to_vec()),
                    OsString::from_vec(b"V\xfe".to_vec()),
                )]
                .into_boxed_slice(),
            },
            package_files: BTreeMap::new(),
        },
    }
}

#[test]
fn native_environment_roundtrips_without_filesystem_interpretation() {
    let limits = Limits::standard();
    let graph = unit::BuildGraph::fixture_artifacts(Box::default());
    let mut account = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let work = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut account, &work);
    let before = budget.usage();
    let original = retained();
    let bytes = original.encode(&mut budget).expect("native record encodes");
    let decoded = build::Retained::decode(
        &bytes,
        &graph,
        std::path::Path::new("target"),
        "context",
        &mut budget,
    )
    .expect("native record decodes");
    assert_eq!(
        decoded.build.evidence.environment,
        original.build.evidence.environment
    );
    assert_eq!(decoded.build.evidence.usage, original.build.evidence.usage);
    assert!(decoded.build.package_files.is_empty());
    assert_eq!(decoded.encode(&mut budget).expect("canonical bytes"), bytes);
    assert!(budget.usage().items > before.items);
    assert!(budget.usage().bytes > before.bytes);
}

#[test]
fn every_truncation_and_trailing_data_reject() {
    let limits = Limits::standard();
    let graph = unit::BuildGraph::fixture_artifacts(Box::default());
    let mut account = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let work = node::GraphBudget::new();
    let bytes = retained()
        .encode(&mut Budget::borrow(&limits, &mut account, &work))
        .expect("record");
    for end in 0..bytes.len() {
        let mut account = Allocation::for_proof(
            Usage {
                items: 7,
                bytes: 31,
            },
            &limits,
        );
        assert!(
            build::Retained::decode(
                &bytes[..end],
                &graph,
                std::path::Path::new("target"),
                "context",
                &mut Budget::borrow(&limits, &mut account, &work)
            )
            .is_err(),
            "truncation {end}"
        );
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(matches!(
        build::Retained::decode(
            &trailing,
            &graph,
            std::path::Path::new("target"),
            "context",
            &mut Budget::borrow(&limits, &mut account, &work)
        ),
        Err(Error::Encoding("trailing record bytes"))
    ));
}

#[test]
fn decoder_cannot_replace_an_exhausted_original_account() {
    let limits = Limits::standard();
    let work = node::GraphBudget::new();
    let mut encode_account = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
    let bytes = retained()
        .encode(&mut Budget::borrow(&limits, &mut encode_account, &work))
        .expect("record");
    let mut account = Allocation::for_proof(
        Usage {
            items: limits.items,
            bytes: 0,
        },
        &limits,
    );
    let graph = unit::BuildGraph::fixture_artifacts(Box::default());
    let mut budget = Budget::borrow(&limits, &mut account, &work);
    // The decoder charges the ORIGINAL exhausted account; the items it
    // owns past the target are recorded on that account, never on a new one.
    build::Retained::decode(
        &bytes,
        &graph,
        std::path::Path::new("target"),
        "context",
        &mut budget,
    )
    .expect("decode is owned by the original account, not gated");
    let record = budget.exceedances();
    let axis = record
        .axis(crate::input::INPUT_ITEMS_RESOURCE)
        .expect("original item axis recorded");
    assert_eq!(axis.target as usize, limits.items);
    assert_eq!(axis.first_requested as usize, limits.items + 1);
    assert_eq!(axis.high_water as usize, budget.usage().items);
    assert!(budget.check().is_ok(), "an exceedance never latches");
    // A genuine failure latches the original account for every later decode.
    budget.refuse();
    assert!(matches!(
        build::Retained::decode(
            &bytes,
            &graph,
            std::path::Path::new("target"),
            "context",
            &mut budget
        ),
        Err(Error::Refused)
    ));
}

#[test]
fn nonempty_script_roundtrip_preserves_all_fields_and_limit_refusal() {
    let limits = Limits::standard();
    let mut original = retained();
    original.build.evidence.packages = build::packages::Table::from_cold(BTreeMap::from([(
        Box::from("package"),
        build::Package {
            manifest: "package/Cargo.toml".into(),
            root: "package".into(),
            build_script: Some("package/build.rs".into()),
        },
    )]));
    original.build.evidence.scripts = Box::from([build::Script {
        compiler: None,
        package_id: "package".into(),
        out_dir: "target/out".into(),
        cfg: Box::from([Box::from("feature=\"enabled\""), Box::from("flag")]),
        environment: Box::from([(Box::from("NAME"), Box::from("value"))]),
    }]);
    let output = unit::Output::fixture(
        "package",
        "package/build.rs",
        "build_script_build",
        "custom-build",
        "bin",
    );
    original.build.evidence.artifacts = Box::from([build::Artifact {
        package_id: "package".into(),
        outputs: Box::default(),
        dep_info: "target/build/script.d".into(),
        key: build::Key {
            package_id: "package".into(),
            target: output.target().into(),
            target_kinds: output.target_kinds().into(),
            crate_types: output.crate_types().into(),
            crate_name: output.crate_name().into(),
            features: output.features().into(),
        },
        profile: output.artifact_profile().clone(),
        fresh: false,
    }]);
    let run = unit::BuildScript::fixture_compiler(&output, None);
    let graph = unit::BuildGraph::fixture_artifacts(Box::from([output]))
        .with_fixture_scripts(Box::from([run]));
    let work = node::GraphBudget::new();
    let mut account = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let mut budget = Budget::borrow(&limits, &mut account, &work);
    let bytes = original.encode(&mut budget).expect("script record");
    let decoded = build::Retained::decode(
        &bytes,
        &graph,
        std::path::Path::new("target"),
        "context",
        &mut budget,
    )
    .expect("script graph association");
    assert_eq!(
        decoded.encode(&mut budget).expect("every field re-encodes"),
        bytes
    );
    let empty = unit::BuildGraph::fixture_artifacts(graph.artifacts().to_vec().into_boxed_slice());
    assert!(matches!(
        build::Retained::decode(
            &bytes,
            &empty,
            std::path::Path::new("target"),
            "context",
            &mut budget
        ),
        Err(Error::Encoding("build-script-unit-cardinality"))
    ));
    // The unit target is an accounting target, not a gate. A zero
    // target still decodes the record and records the artifact unit axis on
    // the original account with the truthful count; only a genuinely latched
    // account refuses the decode.
    let mut narrow = limits;
    narrow.units = 0;
    let mut account = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &narrow,
    );
    let mut budget = Budget::borrow(&narrow, &mut account, &work);
    build::Retained::decode(
        &bytes,
        &graph,
        std::path::Path::new("target"),
        "context",
        &mut budget,
    )
    .expect("a zero unit target records the artifact unit axis and still decodes");
    let axis = *budget
        .exceedances()
        .axis("Build compiler artifact units")
        .expect("the artifact unit count records its axis");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (0, 1, 1)
    );
    budget.refuse();
    assert!(matches!(
        build::Retained::decode(
            &bytes,
            &graph,
            std::path::Path::new("target"),
            "context",
            &mut budget
        ),
        Err(Error::Refused)
    ));
}

#[test]
fn current_graph_artifact_mismatch_returns_a_static_cause() {
    let limits = Limits::standard();
    let graph = unit::BuildGraph::fixture_artifacts(Box::from([unit::Output::fixture(
        "p", "p.rs", "p", "lib", "rlib",
    )]));
    let work = node::GraphBudget::new();
    let mut account = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let mut budget = Budget::borrow(&limits, &mut account, &work);
    let bytes = retained().encode(&mut budget).expect("record");
    assert!(matches!(
        build::Retained::decode(
            &bytes,
            &graph,
            std::path::Path::new("target"),
            "context",
            &mut budget
        ),
        Err(Error::Encoding("missing-build-artifact"))
    ));
}
