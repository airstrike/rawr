use crate::message;
use crate::p5;

use message::Tick;

#[test]
fn profile_launch_hot_retention_and_cold_reset_are_exact() {
    let _turn = rawr_import::store::test_turn();
    let project = p5::Project::dag("profile-lifecycle");
    let mut revisions = rawr_import::supervisor::Owner::new();
    let first = revisions.revision().expect("the first revision exists");
    let (generated, snapshot) =
        p5::generated(&project, p5::Mode::Success, None, b"first body", first);
    let mut lane = rawr_iced_cli::watch::native::Lane::new(project.path())
        .expect("the profile native lane binds");
    let initial = lane
        .replace(generated, snapshot)
        .expect("the synthetic profile admits");
    let first_message = lane.message(Tick(40)).expect("the first message returns");
    assert_eq!(first_message.message(), &Tick(41));

    let second = revisions.revision().expect("the hot revision exists");
    let (request, snapshot) = p5::generation_request(
        &project,
        p5::Mode::Success,
        None,
        b"body-only change",
        second,
    );
    let rawr_import::process::hot::Outcome::Hit(hot) = lane
        .compile(request, snapshot)
        .expect("the same profile surface hot admits")
    else {
        panic!("an unchanged profile surface must not invoke Cargo")
    };
    assert_ne!(initial, hot);
    let retained = lane
        .message(Tick(40))
        .expect("the hot runner retains profile state");
    assert_eq!(retained.message(), &Tick(42));

    lane.shutdown().expect("the first profile runner retires");
    let third = revisions.revision().expect("the cold revision exists");
    let (generated, snapshot) =
        p5::generated(&project, p5::Mode::Success, None, b"cold body", third);
    lane.replace(generated, snapshot)
        .expect("the cold profile epoch admits");
    let reset = lane
        .message(Tick(40))
        .expect("the cold profile message returns");
    assert_eq!(reset.message(), &Tick(41));
    lane.shutdown().expect("the cold runner retires");
}
