use super::*;
use crate::input::{Allocation, Usage};
use crate::{cargo, node};

fn metadata() -> MetadataProof {
    MetadataProof::fixture_package("a", "alpha", "1.0", "", "/work".into()).with_fixture_package(
        "b",
        "beta",
        "2.0",
        "registry+x",
        "/work/nested".into(),
    )
}

#[test]
fn current_replacements_match_legacy_root_then_output_order() {
    let protected = metadata();
    let evidence = Evidence {
        usage: Usage { items: 0, bytes: 0 },
        packages: Default::default(),
        scripts: vec![crate::input::build::Script {
            compiler: None,
            package_id: "b".into(),
            out_dir: "/outside/out".into(),
            cfg: Box::default(),
            environment: Box::default(),
        }]
        .into_boxed_slice(),
        artifacts: Box::default(),
        environment: Box::default(),
    };
    let limits = cargo::Limits::standard();
    for source in [
        "/work/nested/lib.rs /work/other /outside/out/generated",
        "",
        "pré/work/nested/λ /outside/out",
    ] {
        let expected = protected.normalized_text(source).replace(
            "/outside/out",
            &format!(
                "$OUT/{}",
                protected.package_token("b").expect("fixture package")
            ),
        );
        let mut allocation = Allocation::for_proof(
            Usage {
                items: 7,
                bytes: 31,
            },
            &limits,
        );
        let graph = node::GraphBudget::new();
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        assert_eq!(
            value(source, &evidence, &protected, &mut budget).expect("current normalization"),
            expected
        );
    }
}

#[test]
fn token_matches_legacy_and_hash_overflow_precedes_work() {
    let protected = metadata();
    let package = protected.package("b").expect("fixture package");
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    assert_eq!(
        token_text(&token(package, &mut budget).expect("admitted token")).expect("hex"),
        protected.package_token("b").expect("legacy")
    );
    let token_bytes = package.token_hash_bytes().expect("bounded key");
    graph
        .hash(
            usize::try_from(node::MAX_HASHED_BYTES_PER_GRAPH).expect("addressable cap")
                - token_bytes,
        )
        .expect("fill remaining hash work");
    // One more token crosses the hash target by exactly its hash bytes;
    // the token is still produced, the axis is recorded and nothing latches.
    token(package, &mut budget).expect("the hash target is recorded, not refused");
    let axis = *graph
        .exceedances()
        .axis(node::HASHED_RESOURCE)
        .expect("the hash axis records the token work");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (
            node::MAX_HASHED_BYTES_PER_GRAPH,
            node::MAX_HASHED_BYTES_PER_GRAPH + token_bytes as u64,
            node::MAX_HASHED_BYTES_PER_GRAPH + token_bytes as u64
        )
    );
    budget
        .check()
        .expect("an accounting exceedance never latches the account");
    // Checked-arithmetic overflow of the hash account is a genuine failure:
    // it refuses before any token work and latches the original account.
    let hashed = graph.census_usage().expect("graph census").2;
    graph
        .hash(usize::try_from(u64::MAX - hashed).expect("addressable remainder"))
        .expect("saturate the hash account");
    assert!(matches!(
        token(package, &mut budget),
        Err(Error::Graph(node::Error::Allocation))
    ));
    assert!(matches!(token(package, &mut budget), Err(Error::Refused)));
}

#[test]
fn duplicate_output_roots_keep_legacy_equal_length_sort_behavior() {
    let protected = metadata();
    let evidence = Evidence {
        usage: Usage { items: 0, bytes: 0 },
        packages: Default::default(),
        scripts: (0..64)
            .map(|index| crate::input::build::Script {
                compiler: None,
                package_id: if index % 2 == 0 { "a" } else { "b" }.into(),
                out_dir: "/outside/duplicate".into(),
                cfg: Box::default(),
                environment: Box::default(),
            })
            .collect(),
        artifacts: Box::default(),
        environment: Box::default(),
    };
    let source = "/outside/duplicate/file";
    let mut expected = protected.normalized_text(source);
    let mut outputs = evidence
        .scripts
        .iter()
        .filter_map(|script| {
            Some((
                script.out_dir.to_str()?,
                protected.package_token(&script.package_id)?,
            ))
        })
        .collect::<Vec<_>>();
    outputs.sort_unstable_by_key(|(path, _)| std::cmp::Reverse(path.len()));
    for (path, token) in outputs {
        expected = expected.replace(path, &format!("$OUT/{token}"));
    }
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    assert_eq!(
        value(
            source,
            &evidence,
            &protected,
            &mut Budget::borrow(&limits, &mut allocation, &graph)
        )
        .expect("current replacement"),
        expected
    );
}

#[test]
fn absent_roots_and_outputs_cost_no_token_work() {
    let protected = metadata();
    let evidence = Evidence {
        usage: Usage { items: 0, bytes: 0 },
        packages: Default::default(),
        scripts: vec![crate::input::build::Script {
            compiler: None,
            package_id: "b".into(),
            out_dir: "/outside/out".into(),
            cfg: Box::default(),
            environment: Box::default(),
        }]
        .into_boxed_slice(),
        artifacts: Box::default(),
        environment: Box::default(),
    };
    let limits = cargo::Limits::standard();
    let mut allocation = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let source = "--cfg feature=\"std\" /elsewhere/lib.rs";
    assert_eq!(
        value(source, &evidence, &protected, &mut budget).expect("current normalization"),
        protected.normalized_text(source)
    );
    // Only the output index's own token is hashed; no root token is.
    let output = protected.package("b").expect("fixture package");
    assert_eq!(
        graph.census_usage().expect("graph census").2,
        output.token_hash_bytes().expect("bounded key") as u64
    );
}
