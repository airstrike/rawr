use super::*;
use rawr::reload::source;
use rawr_import::input::proof::Watch;
use std::fs;

struct Fixture {
    root: PathBuf,
    engine: rawr::Shared<rawr::Engine>,
    target: rawr::project::Target,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-child-observer-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        let root = fs::canonicalize(root).unwrap();
        crate::write_marked(&root, "src/main.rs", r#"
use iced::Element;
use iced::widget::text;
#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> { text("Observed") }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#);
        let mut engine = rawr::Engine::new();
        crate::install(&mut engine).unwrap();
        let target = rawr::project::Target::discover(&root).unwrap();
        Self {
            root,
            engine: rawr::Shared::new(engine),
            target,
        }
    }
    fn candidate(&self) -> rawr::reload::Candidate {
        self.engine
            .compile_package_generation(&self.target)
            .unwrap()
    }
    fn bound(&self) -> (BoundGeneration, source::Inputs) {
        let candidate = self.candidate();
        let inputs = source::Inputs::from_graph(candidate.source());
        let bound = bind_generation(
            candidate,
            &self.engine,
            runtime(&self.engine, &[]).unwrap(),
            [7; 32],
            Mode::Run,
            self.root.clone(),
        )
        .unwrap();
        (bound, inputs)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn runtime(
    _: &rawr::Engine,
    _: &[host::facade::Definition],
) -> Result<host::runtime::Registry, host::runtime::Error> {
    let mut registry = host::runtime::Registry::new();
    crate::install_test_runtime(&mut registry);
    Ok(registry)
}

#[test]
fn child_startup_rejects_events_or_suspension_during_parent_validation() {
    for suspend in [false, true] {
        let fixture = Fixture::new(if suspend { "suspend" } else { "event" });
        let (mut bound, inputs) = fixture.bound();
        bound
            .observe(&inputs.encode_observation().unwrap())
            .unwrap();
        let observed = bound.observation.as_ref().unwrap();
        assert!(Arc::ptr_eq(&observed.gate, &observed.observer.gate()));
        assert!(Arc::ptr_eq(
            &observed.publication,
            &observed.observer.publication()
        ));
        if suspend {
            observed.gate.suspend();
        } else {
            observed.gate.observe(Instant::now()).unwrap();
        }
        assert!(bound
            .start(fixture.engine.clone(), Box::default(), runtime)
            .is_err());
    }
}

#[test]
fn child_startup_revalidates_rawr_bytes_and_requires_exact_coverage() {
    let fixture = Fixture::new("bytes");
    let (mut bound, inputs) = fixture.bound();
    assert!(bound.observe(b"invalid").is_err());
    let mut empty = vec![1];
    empty.extend_from_slice(&0_u32.to_le_bytes());
    empty.extend_from_slice(&0_u32.to_le_bytes());
    assert!(bound.observe(&empty).is_err());
    bound
        .observe(&inputs.encode_observation().unwrap())
        .unwrap();
    assert!(bound
        .observe(&inputs.encode_observation().unwrap())
        .is_err());
    crate::write_marked(&fixture.root, "src/main.rs", "changed");
    assert!(bound
        .start(fixture.engine.clone(), Box::default(), runtime)
        .is_err());
}

#[test]
fn running_owner_keeps_child_observation_alive_after_protocol_transport_drops() {
    let fixture = Fixture::new("lifetime");
    let (mut bound, inputs) = fixture.bound();
    bound
        .observe(&inputs.encode_observation().unwrap())
        .unwrap();
    let (running, transport) = bound
        .start(fixture.engine.clone(), Box::default(), runtime)
        .unwrap();
    let control = Arc::downgrade(running._observer.as_ref().unwrap());
    assert!(Arc::ptr_eq(
        &transport.gate,
        &transport.observer.as_ref().unwrap().gate()
    ));
    drop(transport);
    assert!(control.upgrade().is_some());
    drop(running);
    assert!(control.upgrade().is_none());
}

#[test]
fn generated_transport_retains_and_drains_healthy_refresh_intent() {
    let fixture = Fixture::new("refresh");
    let (mut bound, inputs) = fixture.bound();
    bound
        .observe(&inputs.encode_observation().unwrap())
        .unwrap();
    let (running, transport) = bound
        .start(fixture.engine.clone(), Box::default(), runtime)
        .unwrap();
    let service = match &running.live {
        LiveGeneration::Application(_, service) | LiveGeneration::Preview(_, service) => service,
        LiveGeneration::Main(_) => unreachable!("the fixture is an application"),
    };
    for _ in 0..3 {
        assert!(!transport.take_refresh());
    }
    for _ in 0..3 {
        service.retry();
    }
    assert!(transport.take_refresh());
    assert!(!transport.take_refresh());
    service.retry();
    assert!(transport.take_refresh());
    assert!(!transport.take_refresh());
    assert_eq!(transport.publication.read().staged(), 0);
}

#[test]
fn generated_retirement_waits_for_ui_exit_and_transfers_refresh_once() {
    let fixture = Fixture::new("refresh-retirement");
    let (mut bound, inputs) = fixture.bound();
    bound
        .observe(&inputs.encode_observation().unwrap())
        .unwrap();
    let (mut running, transport) = bound
        .start(fixture.engine.clone(), Box::default(), runtime)
        .unwrap();
    let service = match &mut running.live {
        LiveGeneration::Application(_, service) | LiveGeneration::Preview(_, service) => service,
        LiveGeneration::Main(_) => unreachable!("the fixture is an application"),
    };
    std::thread::scope(|scope| {
        let retirement = scope.spawn(move || {
            let result = transport.retire();
            (transport, result)
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let wake = loop {
            if let Some(wake) = service.take_wake() {
                break wake;
            }
            assert!(Instant::now() < deadline, "retirement must reach the UI");
            std::thread::yield_now();
        };
        assert!(matches!(
            service.take_latest(wake).unwrap().into_input(),
            crate::reload::Input::Exit
        ));
        // A control accepted before handling Exit belongs to the predecessor.
        service.retry();
        assert!(!retirement.is_finished());
        service.exiting();
        // A control already queued behind Exit cannot resurrect its request.
        service.retry();
        let (transport, result) = retirement.join().unwrap();
        assert!(result.unwrap());
        assert!(!transport.take_refresh());
        service.retry();
        assert!(!transport.take_refresh());
        assert_eq!(transport.publication.read().staged(), 0);
    });
}

#[test]
fn resident_current_check_requires_observation_and_never_refreshes_original_work() {
    let fixture = Fixture::new("resident-current");
    let (mut bound, inputs) = fixture.bound();
    bound
        .observe(&inputs.encode_observation().unwrap())
        .unwrap();
    let (running, transport) = bound
        .start(fixture.engine.clone(), Box::default(), runtime)
        .unwrap();
    let mut pending = transport.begin_install().unwrap();
    assert!(transport.request_is_current(&pending).is_err());
    transport
        .observe_install(&mut pending, &inputs.encode_observation().unwrap())
        .unwrap();
    assert!(transport.request_is_current(&pending).unwrap());
    assert!(transport.request_is_current(&pending).unwrap());
    transport.gate.observe(Instant::now()).unwrap();
    assert!(!transport.request_is_current(&pending).unwrap());
    transport.cancel_install(pending).unwrap();
    assert_eq!(transport.publication.read().staged(), 0);
    assert!(transport.gate.start(Instant::now()).is_some());
    drop(transport);
    drop(running);
}

#[test]
fn observed_child_requires_hot_inputs_and_native_callbacks_revoke_during_preparation() {
    let fixture = Fixture::new("native");
    fs::create_dir(fixture.root.join("native")).unwrap();
    let (mut bound, inputs) = fixture.bound();
    bound
        .observe(&inputs.encode_observation().unwrap())
        .unwrap();
    let (running, transport) = bound
        .start(fixture.engine.clone(), Box::default(), runtime)
        .unwrap();
    let pending = transport.begin_install().unwrap();
    assert!(matches!(
        transport.install(pending, fixture.candidate()),
        Err(InstallError::NotInstalled(_))
    ));
    let mut pending = transport.begin_install().unwrap();
    let native = fixture.root.join("native/new.rs");
    let full = inputs.with_imported([Watch::Exact(native.clone())]);
    transport
        .observe_install(&mut pending, &full.encode_observation().unwrap())
        .unwrap();
    assert!(transport.publication.read().contains(&native));
    fs::write(&native, "pub fn changed() {}").unwrap();
    let notice = transport
        .observer
        .as_ref()
        .unwrap()
        .notices()
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    assert!(matches!(notice, rawr_watch::Notice::Changed(_)));
    assert!(matches!(
        pending.complete(&transport.gate, ()),
        Err(InstallError::Superseded(_))
    ));
    transport.reconcile_observation().unwrap();
    assert!(!transport.publication.read().contains(&native));
    drop(transport);
    drop(running);
}
