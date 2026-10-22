use crate::message;
use crate::p5;

use message::Tick;

#[test]
fn failed_exclusive_activation_keeps_the_old_sole_owner_and_allows_retry() {
    let _turn = rawr_import::store::test_turn();
    let project = p5::Project::new("profile-exclusive");
    let lease = project.path().join("exclusive.lease");
    let mut revisions = rawr_import::supervisor::Owner::new();
    let first = revisions.revision().expect("the first revision exists");
    let (generated, snapshot) = p5::generated(
        &project,
        p5::Mode::Exclusive,
        Some(lease.clone()),
        b"old exclusive owner",
        first,
    );
    let mut lane = rawr_iced_cli::watch::native::Lane::new(project.path())
        .expect("the exclusive native lane binds");
    let old = lane
        .replace(generated, snapshot)
        .expect("the old exclusive owner admits");
    assert!(lease.exists());

    let second = revisions.revision().expect("the candidate revision exists");
    let (generated, snapshot) = p5::generated(
        &project,
        p5::Mode::Exclusive,
        Some(lease.clone()),
        b"candidate exclusive owner",
        second,
    );
    assert!(matches!(
        lane.replace(generated, snapshot),
        Err(rawr_iced_cli::watch::native::Error::Bootstrap(_))
    ));
    assert_eq!(lane.current_key(), Some(old));
    assert!(lease.exists());
    assert_eq!(
        lane.message(Tick(3))
            .expect("the old owner retains exact message authority")
            .message(),
        &Tick(4)
    );

    lane.shutdown().expect("the old owner retires and releases");
    assert!(!lease.exists());

    let third = revisions.revision().expect("the retry revision exists");
    let (generated, snapshot) = p5::generated(
        &project,
        p5::Mode::Exclusive,
        Some(lease.clone()),
        b"activated candidate",
        third,
    );
    let cargo = rawr_import::cargo::Owner::acquire().expect("the native Cargo lane is free");
    let cache = rawr_import::cache::Writer::new(project.path()).expect("the cache binds");
    let built = generated
        .build(&cargo, &cache)
        .expect("the activated candidate builds");
    drop(cargo);
    let process =
        rawr_import::process::Owner::new(project.path()).expect("the process owner binds");
    let epoch = revisions.epoch().expect("the candidate epoch exists");
    let spawned = built
        .spawn(epoch, &process)
        .expect("the activated candidate authenticates");
    let prepared = spawned
        .prepare(snapshot)
        .expect("the activated candidate prepares");
    let activation = revisions
        .activation(prepared.key())
        .expect("the exact activation capability issues");
    let activated = prepared
        .activate(activation)
        .expect("the candidate acquires the exclusive lease");
    assert!(lease.exists());
    drop(activated);
    assert!(!lease.exists());

    let fourth = revisions
        .revision()
        .expect("the final retry revision exists");
    let (generated, snapshot) = p5::generated(
        &project,
        p5::Mode::Exclusive,
        Some(lease.clone()),
        b"valid retry",
        fourth,
    );
    lane.replace(generated, snapshot)
        .expect("the retry becomes the sole admitted owner");
    assert!(lease.exists());
    lane.shutdown().expect("the retry owner retires");
    assert!(!lease.exists());
}
