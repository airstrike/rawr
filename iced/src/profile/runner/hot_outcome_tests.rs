use super::*;
use crate::reload::Feedback;
use std::sync::mpsc::{self, RecvTimeoutError};

#[test]
fn superseded_error_reports_leave_transport_ready_for_next_reload() {
    use rawr::reload::{
        scheduler::{publication, Gate},
        source,
    };
    let root = std::env::temp_dir().join(format!(
        "rawr-report-race-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("main.rawr");
    std::fs::write(&path, "let value = 1;").unwrap();
    let target = rawr::project::Target::discover(&path).unwrap();
    let engine = rawr::Shared::new(rawr::Engine::new());
    let graph = rawr::package::Graph::load(&engine, &target).unwrap();
    let graph = source::Graph::from_package(&graph);
    let publication = Arc::new(publication::Slot::new(source::Inputs::from_graph(&graph)));
    let gate = Arc::new(Gate::new(Duration::ZERO));
    let (channel, service) =
        crate::reload::channel(Arc::clone(&gate), Arc::clone(&publication), engine);
    let exporter = channel.exporter();
    let (sender, feedback, retry) = channel.into_parts();
    let transport = Transport {
        gate: Arc::clone(&gate),
        publication,
        sender,
        feedback,
        retry,
        feedback_uncertain: Cell::new(false),
        observer: None,
        native_baseline: None,
        exporter,
    };
    let report = || {
        rawr::diagnostic::Report::error(
            rawr::diagnostic::code::FRAMEWORK_NATIVE,
            "old compile failure".to_owned(),
        )
        .finish()
    };

    // The newer event wins before mailbox admission: no reply is enqueued.
    gate.observe(Instant::now()).unwrap();
    let request = gate.start(Instant::now()).unwrap();
    gate.observe(Instant::now()).unwrap();
    transport
        .send_report(crate::reload::Completion::failed(
            request.complete(report()),
        ))
        .unwrap();
    assert!(service.take_wake().is_none());
    assert!(!transport.feedback_uncertain.get());
    drop(transport.begin_install().unwrap());

    // The report reaches the mailbox, then a new edit wins before UI use.
    gate.observe(Instant::now()).unwrap();
    let request = gate.start(Instant::now()).unwrap();
    assert!(transport
        .sender
        .send(
            &gate,
            crate::reload::Completion::failed(request.complete(report()))
        )
        .unwrap());
    let completion = service.take_latest(service.take_wake().unwrap()).unwrap();
    gate.observe(Instant::now()).unwrap();
    let crate::reload::Input::Revision(rawr::reload::scheduler::mailbox::Revision::Candidate(
        candidate,
    )) = completion.into_input()
    else {
        panic!("report candidate")
    };
    let candidate = candidate.map(|outcome| match outcome {
        crate::reload::Outcome::Failed(report) => report,
        _ => panic!("failed report"),
    });
    assert!(service.failed_candidate(candidate).is_none());
    transport.feedback(ExpectedFeedback::Failed).unwrap();
    assert!(!transport.feedback_uncertain.get());
    drop(transport.begin_install().unwrap());

    // Settled only acknowledges reports; it is still invalid for exit.
    service.settled();
    assert!(transport.feedback(ExpectedFeedback::Exiting).is_err());
    assert!(transport.feedback_uncertain.get());
    assert!(matches!(
        transport.begin_install(),
        Err(InstallError::Uncertain(_))
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn only_definite_failed_or_settled_feedback_proves_non_installation() {
    let report = rawr::diagnostic::Report::error(
        rawr::diagnostic::code::FRAMEWORK_NATIVE,
        "candidate widget failed".to_owned(),
    )
    .finish();
    assert!(matches!(
        install_feedback(Ok(Feedback::Failed(report))),
        Err(InstallError::NotInstalled(Error::Reload(reason))) if reason.contains("candidate widget failed")
    ));
    assert!(matches!(
        install_feedback(Ok(Feedback::Settled)),
        Err(InstallError::Superseded(_))
    ));
    for result in [
        Ok(Feedback::Exiting),
        Err(RecvTimeoutError::Timeout),
        Err(RecvTimeoutError::Disconnected),
    ] {
        assert!(matches!(
            install_feedback(result),
            Err(InstallError::Uncertain(_))
        ));
    }
}

#[test]
fn preflight_rejects_stale_feedback_and_keeps_uncertainty_latched() {
    use std::cell::Cell;
    let (sender, receiver) = mpsc::channel();
    let uncertain = Cell::new(false);
    assert!(check_install_feedback(&receiver, &uncertain).is_ok());
    sender
        .send(Feedback::Settled)
        .expect("live feedback receiver");
    assert!(matches!(
        check_install_feedback(&receiver, &uncertain),
        Err(InstallError::Uncertain(_))
    ));
    assert!(uncertain.get());
    assert!(matches!(
        check_install_feedback(&receiver, &uncertain),
        Err(InstallError::Uncertain(_))
    ));
    let (sender, receiver) = mpsc::channel();
    let uncertain = Cell::new(false);
    drop(sender);
    assert!(matches!(
        check_install_feedback(&receiver, &uncertain),
        Err(InstallError::Uncertain(_))
    ));
    assert!(uncertain.get());
}

#[test]
fn retained_installation_rejects_edits_during_preparation() {
    use rawr::reload::scheduler::{Admission, Gate};
    let gate = Arc::new(Gate::new(Duration::ZERO));
    let pending = PendingInstall::begin(&gate).expect("original request");
    gate.observe(Instant::now())
        .expect("edit during compilation");
    assert!(matches!(
        pending.complete(&gate, ()),
        Err(InstallError::Superseded(_))
    ));
    let next = gate
        .start(Instant::now())
        .expect("newer edit remains pending");
    assert!(matches!(
        gate.admit_current(next.complete(()), |()| ()),
        Admission::Current(())
    ));
}

#[test]
fn retained_installation_is_bound_to_its_owner_and_backend_health() {
    use rawr::reload::scheduler::Gate;
    let first = Arc::new(Gate::new(Duration::ZERO));
    let second = Arc::new(Gate::new(Duration::ZERO));
    let pending = PendingInstall::begin(&first).expect("first owner request");
    let _other = PendingInstall::begin(&second).expect("same ordinal, different owner");
    assert!(matches!(
        pending.complete(&second, ()),
        Err(InstallError::NotInstalled(_))
    ));
    let pending = PendingInstall::begin(&first).expect("next request");
    first.suspend();
    assert!(matches!(
        pending.complete(&first, ()),
        Err(InstallError::NotInstalled(_))
    ));
    assert!(matches!(
        PendingInstall::begin(&first),
        Err(InstallError::NotInstalled(_))
    ));
}

#[test]
fn retained_installation_still_checks_events_after_transport_preflight() {
    use rawr::reload::scheduler::{Admission, Gate};
    let gate = Arc::new(Gate::new(Duration::ZERO));
    let pending = PendingInstall::begin(&gate).expect("original request");
    let candidate = pending
        .complete(&gate, ())
        .expect("current transport preflight");
    gate.observe(Instant::now())
        .expect("edit before UI admission");
    let result = gate.admit_current(candidate, |()| panic!("obsolete UI publication"));
    assert!(matches!(result, Admission::Obsolete(())));
}

#[test]
#[cfg(unix)]
fn hot_observation_is_single_use_owner_bound_and_does_not_refresh_revision() {
    use rawr::reload::{
        scheduler::{publication, Gate},
        source,
    };
    use rawr_import::input::proof::Watch;
    let root = std::env::temp_dir().join(format!(
        "rawr-hot-observation-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let path = root.join("main.rawr");
    std::fs::write(&path, "let value = 1;").unwrap();
    let target = rawr::project::Target::discover(&path).unwrap();
    let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target).unwrap();
    let graph = source::Graph::from_package(&graph);
    let inputs = source::Inputs::from_graph(&graph);
    let full = inputs
        .clone()
        .with_imported([Watch::Exact(root.join("native.rs"))]);
    let bytes = full.encode_observation().unwrap();
    let publication = Arc::new(publication::Slot::new(inputs));
    let gate = Arc::new(Gate::new(Duration::ZERO));
    let foreign = Arc::new(Gate::new(Duration::ZERO));
    let mut pending = PendingInstall::begin(&gate).unwrap();
    assert!(pending.observe(&foreign, &publication, &bytes).is_err());
    assert_eq!(publication.read().staged(), 0);
    assert!(pending.observe(&gate, &publication, b"malformed").is_err());
    assert_eq!(publication.read().staged(), 0);
    pending.observe(&gate, &publication, &bytes).unwrap();
    assert!(publication.read().contains(&root.join("native.rs")));
    assert!(pending.observe(&gate, &publication, &bytes).is_err());
    assert_eq!(publication.read().staged(), 1);
    gate.observe(Instant::now()).unwrap();
    assert!(matches!(
        pending.complete(&gate, ()),
        Err(InstallError::Superseded(_))
    ));
    assert_eq!(publication.read().staged(), 0);
    assert!(!publication.read().contains(&root.join("native.rs")));
    assert!(gate.start(Instant::now()).is_some());
    std::fs::remove_dir_all(root).unwrap();
}
