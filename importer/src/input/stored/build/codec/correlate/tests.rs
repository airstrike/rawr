use super::{require, COMPARISONS};
use crate::cargo::unit;
use crate::input::build;
use crate::input::stored::{Budget, Error};
use crate::input::{Allocation, Usage};
use crate::{cargo::Limits, node};

fn artifact(output: &unit::Output) -> build::Artifact {
    build::Artifact {
        package_id: output.package_id().into(),
        outputs: Box::default(),
        dep_info: "fixture.d".into(),
        key: build::Key {
            package_id: output.package_id().into(),
            target: output.target().into(),
            target_kinds: output.target_kinds().into(),
            crate_types: output.crate_types().into(),
            crate_name: output.crate_name().into(),
            features: output.features().into(),
        },
        profile: output
            .artifact_profile()
            .retain_exact(output.artifact_profile().as_bytes().into())
            .expect("fixture profile"),
        fresh: true,
    }
}

#[test]
fn coverage_preserves_duplicate_semantics_and_missing_precedence() {
    let limits = Limits::standard();
    let work = node::GraphBudget::new();
    let mut account = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
    let mut budget = Budget::borrow(&limits, &mut account, &work);
    let output = || unit::Output::fixture("p", "p.rs", "p", "lib", "rlib");
    let expected = [output(), output()];
    require(&expected, &[artifact(&expected[0])], &mut budget)
        .expect("one public message covers duplicate contexts");
    require(
        &expected,
        &[artifact(&expected[0]), artifact(&expected[0])],
        &mut budget,
    )
    .expect("matching repeated artifacts preserve total coverage");
    let wrong = unit::Output::fixture("q", "q.rs", "q", "lib", "rlib");
    assert!(matches!(
        require(&expected, &[artifact(&wrong)], &mut budget),
        Err(Error::Encoding("missing-build-artifact"))
    ));
    assert!(matches!(
        require(
            &expected,
            &[artifact(&expected[0]), artifact(&wrong)],
            &mut budget
        ),
        Err(Error::Encoding("unexpected-build-artifact"))
    ));
}

#[test]
fn reverse_large_coverage_has_subquadratic_comparison_growth() {
    const COUNT: usize = 4096;
    let expected = (0..COUNT)
        .map(|i| unit::Output::fixture(&format!("package-{i:05}"), "source.rs", "p", "lib", "rlib"))
        .collect::<Vec<_>>();
    let artifacts = expected.iter().rev().map(artifact).collect::<Vec<_>>();
    let limits = Limits::standard();
    let work = node::GraphBudget::new();
    let mut account = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    COMPARISONS.with(|count| count.set(0));
    require(
        &expected,
        &artifacts,
        &mut Budget::borrow(&limits, &mut account, &work),
    )
    .expect("full reverse coverage");
    let comparisons = COMPARISONS.with(|count| count.get());
    assert!(
        comparisons < COUNT * 100,
        "{comparisons} comparisons must remain below quadratic growth"
    );
}
