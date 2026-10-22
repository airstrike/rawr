use super::*;
use crate::reload::scheduler::{Admission, Gate};
use rawr_import::input::proof::Watch;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Fixture {
    root: PathBuf,
    graph: source::Graph,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-observed-publication-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let graph = graph(&root.join("main.rawr"));
        Self { root, graph }
    }

    fn inputs(&self) -> source::Inputs {
        source::Inputs::from_graph(&self.graph)
    }

    fn observed(&self) -> source::Inputs {
        self.inputs().with_imported([
            Watch::Exact(self.root.join("native.rs")),
            Watch::Exact(self.root.join("missing/config.toml")),
            Watch::Tree(self.root.join("native-tree")),
        ])
    }

    fn snapshot(&self) -> source::Snapshot {
        source::Snapshot::from_graph(&self.graph)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn graph(path: &Path) -> source::Graph {
    fs::write(path, "let value = 1;").unwrap();
    let target = crate::project::Target::discover(path).unwrap();
    let graph = crate::package::Graph::load(&crate::Engine::new(), &target).unwrap();
    source::Graph::from_package(&graph)
}

#[test]
fn full_observation_survives_source_binding_validation_and_publication() {
    let fixture = Fixture::new("publish");
    let previous = graph(&fixture.root.join("previous.rawr"));
    let slot = Arc::new(Slot::new(source::Inputs::from_graph(&previous)));
    let observed = slot.register(fixture.observed());
    let native = fixture.root.join("native.rs");
    assert!(slot.read().contains(&native));
    assert!(!slot.read().current().contains(&native));
    let staged = observed.bind(fixture.snapshot()).unwrap();
    assert!(staged.inputs().contains(&native));
    let validated = staged.revalidate().unwrap();
    assert!(validated.inputs().contains(&native));
    let retired = slot.reserve(&validated).unwrap().publish(validated);
    assert_eq!(slot.read().staged(), 0);
    assert!(slot.read().current().contains(&native));
    assert!(slot
        .read()
        .contains(&fixture.root.join("missing/config.toml")));
    assert!(slot
        .read()
        .contains(&fixture.root.join("native-tree/new.rs")));
    assert!(!slot.read().contains(&fixture.root.join("previous.rawr")));
    assert!(retired
        .previous()
        .contains(&fixture.root.join("previous.rawr")));
    retired.release();
}

#[test]
fn withdrawing_one_registration_preserves_other_and_current_coverage() {
    let fixture = Fixture::new("withdraw");
    let slot = Arc::new(Slot::new(fixture.inputs()));
    let first = slot.register(fixture.observed());
    let second = slot.register(
        fixture
            .inputs()
            .with_imported([Watch::Exact(fixture.root.join("other.rs"))]),
    );
    drop(first);
    assert!(!slot.read().contains(&fixture.root.join("native.rs")));
    assert!(slot.read().contains(&fixture.root.join("other.rs")));
    assert!(slot.read().contains(&fixture.root.join("main.rawr")));
    drop(second);
    assert_eq!(slot.read().staged(), 0);
    assert!(!slot.read().contains(&fixture.root.join("other.rs")));
}

#[test]
fn recursive_coverage_cannot_replace_an_exact_rawr_identity() {
    let fixture = Fixture::new("coverage");
    let other = graph(&fixture.root.join("other.rawr"));
    let slot = Arc::new(Slot::new(fixture.inputs()));
    let incomplete =
        source::Inputs::from_graph(&other).with_imported([Watch::Tree(fixture.root.clone())]);
    assert!(incomplete.contains(&fixture.root.join("main.rawr")));
    let observed = slot.register(incomplete);
    assert!(matches!(
        observed.bind(fixture.snapshot()),
        Err(IncompleteObservation)
    ));
    assert_eq!(slot.read().staged(), 0);
    assert!(!slot.read().contains(&fixture.root.join("other.rawr")));
}

#[test]
fn observation_cannot_advertise_source_roles_outside_the_compiled_snapshot() {
    let fixture = Fixture::new("source-role-binding");
    let other = graph(&fixture.root.join("other.rawr"));
    let slot = Arc::new(Slot::new(fixture.inputs()));
    let observation = source::Inputs::from_graph(&other)
        .with_imported([Watch::Exact(fixture.root.join("main.rawr"))]);
    assert!(observation.contains(&fixture.root.join("main.rawr")));
    let observed = slot.register(observation);
    assert!(matches!(
        observed.bind(fixture.snapshot()),
        Err(IncompleteObservation)
    ));
    assert_eq!(slot.read().staged(), 0);
}

#[test]
fn full_observation_does_not_weaken_rawr_byte_validation() {
    let fixture = Fixture::new("bytes");
    let slot = Arc::new(Slot::new(fixture.inputs()));
    let observed = slot.register(fixture.observed());
    fs::write(fixture.root.join("main.rawr"), "let value = 2;").unwrap();
    assert!(observed
        .bind(fixture.snapshot())
        .unwrap()
        .revalidate()
        .is_err());
    assert_eq!(slot.read().staged(), 0);
    assert!(!slot.read().contains(&fixture.root.join("native.rs")));
}

#[test]
fn bound_observation_cannot_publish_into_a_foreign_slot() {
    let fixture = Fixture::new("foreign");
    let origin = Arc::new(Slot::new(fixture.inputs()));
    let foreign = Arc::new(Slot::new(fixture.inputs()));
    let validated = origin
        .register(fixture.observed())
        .bind(fixture.snapshot())
        .unwrap()
        .revalidate()
        .unwrap();
    assert!(foreign.reserve(&validated).is_none());
    assert_eq!(origin.read().staged(), 1);
    drop(validated);
    assert_eq!(origin.read().staged(), 0);
    assert_eq!(foreign.read().staged(), 0);
}

#[test]
fn an_added_native_event_revokes_the_original_request_before_binding() {
    let fixture = Fixture::new("revoke");
    let slot = Arc::new(Slot::new(fixture.inputs()));
    let gate = Gate::new(Duration::ZERO);
    let now = Instant::now();
    gate.observe(now).unwrap();
    let request = gate.start(now).unwrap();
    let observed = slot.register(fixture.observed());
    assert!(gate
        .observe_if(now, || slot
            .read()
            .contains(&fixture.root.join("native.rs")))
        .unwrap());
    let validated = observed
        .bind(fixture.snapshot())
        .unwrap()
        .revalidate()
        .unwrap();
    let result = gate.admit_current(request.complete(validated), |_| {
        panic!("obsolete publication")
    });
    let Admission::Obsolete(validated) = result;
    drop(validated);
    assert_eq!(slot.read().staged(), 0);
    assert!(!slot
        .read()
        .current()
        .contains(&fixture.root.join("native.rs")));
    assert!(gate.start(now).is_some(), "newer event remains pending");
}

#[test]
fn decoded_current_identities_share_the_current_inputs_and_others_derive_their_own() {
    let fixture = Fixture::new("decoded");
    let slot = Arc::new(Slot::new(fixture.observed()));
    let shared = |observed: &Observed| {
        let inputs = &observed.0.as_ref().unwrap().inputs;
        Arc::ptr_eq(inputs, &read(&slot.state).current)
    };
    let same = source::observation::decode(&fixture.observed().encode_observation().unwrap())
        .unwrap();
    let observed = slot.register_decoded(same);
    assert!(shared(&observed));
    assert_eq!(slot.read().staged(), 1);
    drop(observed);

    let other = fixture
        .inputs()
        .with_imported([Watch::Exact(fixture.root.join("other.rs"))]);
    let observed = slot.register_decoded(
        source::observation::decode(&other.encode_observation().unwrap()).unwrap(),
    );
    assert!(!shared(&observed));
    assert!(slot.read().contains(&fixture.root.join("other.rs")));
    assert_eq!(*observed.0.as_ref().unwrap().inputs, other);
}
