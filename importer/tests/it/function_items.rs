use crate::p4;

use rawr_import::classify::representability::convert::{Builtin, Constant, Type};
use rawr_import::surface::instance::Arguments;

#[test]
fn concrete_function_item_generates_one_exact_rooted_thunk() {
    let (_project, contract, limits) = p4::contract();
    let outcome = contract
        .outcomes()
        .find(|outcome| outcome.export().path().eq(["identity"]))
        .expect("the generic function is exported");
    let selector = outcome
        .identity()
        .selector()
        .try_clone()
        .expect("the selector duplicates");
    let base = outcome.identity().clone();
    let mut compiler = contract
        .compiler((), p4::snapshot(&limits), &limits)
        .expect("the concrete compiler starts");
    compiler
        .require_instance(
            contract.accept(&base, &selector).unwrap(),
            Arguments::new(
                Box::from([Type::Builtin(Builtin::I64)]),
                Box::default(),
                Box::default(),
                None,
                &limits,
            )
            .expect("the concrete arguments seal"),
            "application.rs",
            0,
            1,
        )
        .expect("the concrete function is demanded");
    let constant = contract
        .outcomes()
        .find(|outcome| outcome.export().path().eq(["add_const"]))
        .expect("the const-generic function is exported");
    assert!(
        matches!(
            constant.representability(),
            rawr_import::classify::representability::Representability::FunctionItem(_)
        ),
        "const-generic classification: {:?}",
        constant.representability()
    );
    compiler
        .require_instance(
            contract
                .accept(
                    constant.identity(),
                    &constant
                        .identity()
                        .selector()
                        .try_clone()
                        .expect("the const selector duplicates"),
                )
                .unwrap(),
            Arguments::new(
                Box::default(),
                Box::from([
                    Constant::checked("usize", "3").expect("the finite const argument seals")
                ]),
                Box::default(),
                None,
                &limits,
            )
            .expect("the const substitution matches"),
            "application.rs",
            1,
            2,
        )
        .expect("the const-generic function is demanded");
    let demand = compiler.seal().expect("the concrete demand seals");
    let proof = contract.into_native_proof();
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let revision = supervisor.revision().expect("the first revision exists");
    let manifest = demand
        .request(proof, rawr_import::profile::Proof::neutral(), revision)
        .expect("the concrete request seals")
        .generate(&limits)
        .expect("the concrete runner generates");
    // The runner is `src/main.rs` plus one module per imported crate (D415).
    let source = manifest
        .files()
        .filter(|file| file.path().starts_with("src/"))
        .map(|file| std::str::from_utf8(file.bytes()).expect("the source is UTF-8"))
        .collect::<String>();
    let source = source.as_str();
    assert_eq!(source.matches("generic::identity::<i64>(").count(), 1);
    assert_eq!(source.matches("generic::add_const::<3>(").count(), 1);

    // Building links the table as well as typechecking it.
    let project = crate::shared::generated_crate(&manifest, "rawr-p4-function-item");
    let output = crate::shared::cargo_on(&project, &["build"]);
    crate::shared::assert_success(&output, "the generated generic crate's build");
}
