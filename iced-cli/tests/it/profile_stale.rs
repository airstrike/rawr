use crate::message;
use crate::p5;

use message::Tick;

#[test]
fn foreign_activation_capability_is_rejected_while_old_route_stays_live() {
    let _turn = rawr_import::store::test_turn();
    let project = p5::Project::dag("profile-stale");
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let first = supervisor.revision().expect("the first revision exists");
    let (generated, snapshot) =
        p5::generated(&project, p5::Mode::Success, None, b"old route", first);
    let mut lane =
        rawr_iced_cli::watch::native::Lane::new(project.path()).expect("the old native lane binds");
    let old = lane
        .replace(generated, snapshot)
        .expect("the old profile route admits");
    let queued = lane
        .message(Tick(9))
        .expect("the old route publishes one authenticated message");
    lane.validate_message(&queued)
        .expect("the message is current before route rotation");

    let second = supervisor
        .revision()
        .expect("the candidate revision exists");
    let (generated, snapshot) = p5::generated(
        &project,
        p5::Mode::Success,
        None,
        b"candidate route",
        second,
    );
    let cargo = rawr_import::cargo::Owner::acquire().expect("the native Cargo lane is free");
    let cache = rawr_import::cache::Writer::new(project.path()).expect("the cache binds");
    let built = generated
        .build(&cargo, &cache)
        .expect("the candidate builds");
    drop(cargo);
    let process =
        rawr_import::process::Owner::new(project.path()).expect("the process owner binds");
    let epoch = supervisor.epoch().expect("the candidate epoch exists");
    let spawned = built
        .spawn(epoch, &process)
        .expect("the candidate authenticates");
    let prepared = spawned.prepare(snapshot).expect("the candidate prepares");
    let foreign = supervisor
        .activation(old)
        .expect("the stale old-route capability issues");
    assert!(matches!(
        prepared.activate(foreign),
        Err(rawr_import::process::RunnerError::Activation(_))
    ));

    assert_eq!(lane.current_key(), Some(old));
    let third = supervisor.revision().expect("the hot revision exists");
    let (request, snapshot) = p5::generation_request(
        &project,
        p5::Mode::Success,
        None,
        b"hot route rotation",
        third,
    );
    assert!(matches!(
        lane.compile(request, snapshot),
        Ok(rawr_import::process::hot::Outcome::Hit(_))
    ));
    assert!(matches!(
        lane.validate_message(&queued),
        Err(rawr_iced_cli::watch::native::Error::Process(
            rawr_import::process::RunnerError::Stale
        ))
    ));
    assert_eq!(
        lane.message(Tick(9))
            .expect("the rotated route retains native profile state")
            .message(),
        &Tick(11)
    );
    lane.shutdown().expect("the old route retires");
}
