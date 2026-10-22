use crate::p3;

#[test]
fn cold_and_warm_epochs_swap_only_after_complete_staged_handshake() {
    let _turn = rawr_import::store::test_turn();
    let project = rawr_import::store::TestProject::new("rawr-p3-process");
    let root = project.path().to_path_buf();
    let generated = p3::generated_at(&root);
    let cache = rawr_import::cache::Writer::new(&root).expect("the runner cache binds");
    let cargo = rawr_import::cargo::Owner::acquire().expect("the Cargo lane is free");
    let built = generated
        .build(&cargo, &cache)
        .expect("the fixed-path runner builds");
    drop(cargo);

    let process = rawr_import::process::Owner::new(&root).expect("the socket owner seals");
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let mut slot = rawr_import::supervisor::slot::Slot::new();
    let first = supervisor
        .bootstrap(built, p3::snapshot_transport(), &process)
        .expect("the cold candidate reaches admitted state");
    let first_key = first.key();
    assert!(slot.install(first).is_none());
    assert_eq!(slot.key(), Some(first_key));

    let warm = p3::generated_at(&root);
    let cargo = rawr_import::cargo::Owner::acquire().expect("the Cargo lane is released");
    let warm = warm
        .build(&cargo, &cache)
        .expect("the sealed runner is a warm hit");
    drop(cargo);
    let second = supervisor
        .bootstrap(warm, p3::snapshot_transport(), &process)
        .expect("the warm candidate reaches admitted state");
    let second_key = second.key();
    assert_ne!(first_key, second_key);
    let retired = slot.install(second).expect("the cold epoch is retired");
    assert_eq!(retired.key(), first_key);
    assert_eq!(slot.key(), Some(second_key));
    retired
        .shutdown()
        .expect("the retired cold process exits and reaps");

    let rejected = p3::generated_at(&root);
    let cargo = rawr_import::cargo::Owner::acquire().expect("the Cargo lane remains reusable");
    let rejected = rejected
        .build(&cargo, &cache)
        .expect("the next candidate is another warm hit");
    drop(cargo);
    assert!(matches!(
        supervisor.bootstrap(rejected, p3::other_snapshot_transport(), &process),
        Err(rawr_import::supervisor::bootstrap::Error::Snapshot(
            rawr_import::supervisor::snapshot::Error::Mismatch
        ))
    ));
    assert_eq!(slot.key(), Some(second_key));

    let rejected = p3::generated_at(&root);
    let cargo = rawr_import::cargo::Owner::acquire().expect("the Cargo lane remains reusable");
    let rejected = rejected
        .build(&cargo, &cache)
        .expect("the activation candidate is another warm hit");
    drop(cargo);
    let spawned = rejected
        .spawn(
            supervisor.epoch().expect("the rejected epoch exists"),
            &process,
        )
        .expect("the rejected runner authenticates");
    let foreign = supervisor
        .activation(second_key)
        .expect("the foreign activation capability is issued");
    let prepared = spawned
        .prepare(p3::snapshot_transport())
        .expect("the rejected runner prepares without side effects");
    assert!(prepared.activate(foreign).is_err());
    assert_eq!(slot.key(), Some(second_key));

    slot.take()
        .expect("the warm epoch remains current")
        .shutdown()
        .expect("the exact current runner exits and reaps");
}
