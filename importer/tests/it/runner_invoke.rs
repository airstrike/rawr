use crate::p3;

use rawr_import::process::value::Value;

#[test]
fn admitted_runner_invokes_an_exact_compiled_adapter_and_rejects_wrong_arity() {
    let _turn = rawr_import::store::test_turn();
    let project = rawr_import::store::TestProject::new("rawr-runner-invoke");
    let root = project.path().to_path_buf();
    let generated = p3::generated_at(&root);
    let entry = generated
        .generated_manifest()
        .entries()
        .find(|entry| entry.arguments() == 0)
        .copied()
        .expect("the symbolic fixture has a zero-argument adapter");
    let cache = rawr_import::cache::Writer::new(&root).expect("the cache binds");
    let cargo = rawr_import::cargo::Owner::acquire().expect("the Cargo lane is free");
    let built = generated
        .build(&cargo, &cache)
        .expect("the invocation runner builds");
    drop(cargo);

    let process = rawr_import::process::Owner::new(&root).expect("the process owner binds");
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let mut candidate = supervisor
        .bootstrap(built, p3::snapshot_transport(), &process)
        .expect("the invocation runner admits");

    let output = candidate
        .invoke(entry, Box::default())
        .expect("the exact compiled adapter executes");
    assert!(matches!(
        output,
        Value::Unit | Value::I64(_) | Value::Handle(_)
    ));
    assert!(matches!(
        candidate.invoke(entry, Box::from([Value::Unit])),
        Err(rawr_import::process::RunnerError::Invocation(
            rawr_import::process::invoke::Error::Cardinality {
                expected: 0,
                actual: 1
            }
        ))
    ));

    candidate.shutdown().expect("the runner reaps");
}
