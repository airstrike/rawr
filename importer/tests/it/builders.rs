use crate::p4;

use rawr_import::classify::representability::ownership::Receiver;
use rawr_import::surface::builder::{Plan, Step, Value};
use rawr_import::surface::Origin;

#[test]
fn consuming_builder_generates_one_ordered_single_invocation_replay() {
    let (_project, contract, limits) = p4::contract();
    let builder = contract
        .outcomes()
        .find(|outcome| outcome.export().path().eq(["Builder"]))
        .expect("the builder type is exported");
    let mut new = None;
    let mut add = None;
    let mut finish = None;
    for implementation in builder.implementations() {
        for associated in implementation.associated() {
            match associated.name() {
                "new" => new = Some(associated),
                "add" => add = Some(associated),
                "finish" => finish = Some(associated),
                _ => {}
            }
        }
    }
    let new = new.expect("Builder::new is classified");
    let add = add.expect("Builder::add is classified");
    let finish = finish.expect("Builder::finish is classified");
    assert!(matches!(
        add.identity().selector(),
        rawr_import::surface::identity::Selector::Method {
            receiver: Receiver::Owned,
            ..
        }
    ));
    let origin = |start| {
        Origin::checked("application.rs", start, start + 1).expect("the builder origin is bounded")
    };
    let plan = Plan::new(
        finish.identity().clone(),
        Box::from([
            Step::construct(new, origin(0), Box::from([Value::argument(0)])),
            Step::consume(add, origin(1), Box::from([Value::argument(1)])),
            Step::consume(finish, origin(2), Box::default()),
        ]),
    )
    .expect("the exact-once builder plan seals");
    let mut compiler = contract
        .compiler((), p4::snapshot(&limits), &limits)
        .expect("the builder compiler starts");
    compiler
        .require_builder(plan)
        .expect("the builder replay is contract-backed");
    let demand = compiler.seal().expect("the builder demand seals");
    assert_eq!(demand.entries().len(), 0, "no native receiver is retained");
    let proof = contract.into_native_proof();
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let revision = supervisor.revision().expect("the revision exists");
    let manifest = demand
        .request(proof, rawr_import::profile::Proof::neutral(), revision)
        .expect("the builder request seals")
        .generate()
        .expect("the builder runner generates");
    // The runner is `src/main.rs` plus one module per imported crate (D415).
    let source = manifest
        .files()
        .filter(|file| file.path().starts_with("src/"))
        .map(|file| std::str::from_utf8(file.bytes()).expect("the source is UTF-8"))
        .collect::<String>();
    let source = source.as_str();
    let construct = source
        .find("generic::Builder::new(")
        .expect("constructor replay");
    // The runner template has its own `builder.finish(`; read the replay in order.
    let consume = construct
        + source[construct..]
            .find("builder.add(")
            .expect("consume replay");
    source[consume..]
        .find("builder.finish(")
        .expect("finish replay");
    assert!(!source.contains("take_native::<generic::Builder>"));

    let project = crate::shared::generated_crate(&manifest, "rawr-p4-builder");
    let output = crate::shared::cargo_on(
        &project,
        &[
            "test",
            "generated_execution_tests::consuming_builder_replays_in_one_native_invocation",
        ],
    );
    crate::shared::assert_success(&output, "the generated builder crate's replay test");
}
