use std::cell::Cell;
use std::fs;
use std::path::PathBuf;

use crate::node::{self, Evaluator, Kind, Parent};

const GENERIC: u8 = Kind::Generic as u8;
const OVERLAY: u8 = Kind::Overlay as u8;
const RESOLUTION: u8 = Kind::Resolution as u8;
const RUNNER: u8 = Kind::Runner as u8;

/// One edit's value of a kind; the generic and overlay are kept as parts.
struct Unit<const KIND: u8>(Box<[u8]>);

impl<const KIND: u8> node::Value for Unit<KIND> {
    const KIND: Kind = match Kind::from_code(KIND) {
        Some(kind) => kind,
        None => panic!("the fixture kind is a node kind"),
    };
    // The current schemas, which the store keeps (#630).
    const SCHEMA: u32 = match KIND {
        GENERIC => <crate::classify::cached::Product as node::Value>::SCHEMA,
        OVERLAY => crate::bridge::generate::cached::SCHEMA,
        _ => 1,
    };
    const PARTS: bool = KIND == GENERIC || KIND == OVERLAY;

    fn encode(&self) -> Result<Box<[u8]>, node::Error> {
        Ok(self.0.clone())
    }

    fn decode(bytes: &[u8]) -> Result<Self, node::Error> {
        Ok(Self(bytes.into()))
    }
}

/// A large value most of whose bytes every edit shares.
fn large(seed: u64, edit: usize) -> Box<[u8]> {
    let mut state = seed;
    let mut bytes = (0..2 * 1024 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect::<Vec<_>>();
    bytes.splice(
        1_000_000..1_000_000,
        format!("edit {edit} of this value").into_bytes(),
    );
    bytes.into_boxed_slice()
}

struct Fixture {
    base: PathBuf,
    store: crate::store::Store,
    executions: Cell<usize>,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let base =
            std::env::temp_dir().join(format!("rawr-generations-{label}-{}", std::process::id()));
        let _ = crate::store::remove_tree(&base);
        fs::create_dir_all(base.join("store")).unwrap();
        let store = crate::store::Store::open(&base.join("store"), [0x47; 32]).unwrap();
        Self {
            base,
            store,
            executions: Cell::new(0),
        }
    }

    /// Evaluate one edit: its resolution, generic, overlay and runner. The
    /// overlay is the runner's sibling, not its parent, as in a real run.
    fn edit(&self, edit: usize) {
        let evaluator = Evaluator::new(&self.store, &crate::metrics::Current);
        let context = [0x11; 32];
        let execute = |bytes: Box<[u8]>| {
            self.executions.set(self.executions.get() + 1);
            bytes
        };
        let resolution = evaluator
            .direct(
                &node::Direct::<Unit<RESOLUTION>>::capture(
                    context,
                    format!("resolution {edit}").as_bytes(),
                    &[],
                )
                .unwrap(),
                || Ok(Unit(execute(format!("lock {edit}").into_bytes().into()))),
            )
            .unwrap();
        let generic = evaluator
            .direct(
                &node::Direct::<Unit<GENERIC>>::capture(
                    context,
                    b"generic",
                    &[Parent::validated(&resolution)],
                )
                .unwrap(),
                || Ok(Unit(execute(large(1, edit)))),
            )
            .unwrap();
        let overlay = evaluator
            .direct(
                &node::Direct::<Unit<OVERLAY>>::capture(
                    context,
                    b"overlay",
                    &[Parent::validated(&generic)],
                )
                .unwrap(),
                || Ok(Unit(execute(large(2, edit)))),
            )
            .unwrap();
        assert_eq!(overlay.value().0, large(2, edit));
        let runner = evaluator
            .direct(
                &node::Direct::<Unit<RUNNER>>::capture(
                    context,
                    b"runner",
                    &[Parent::validated(&generic)],
                )
                .unwrap(),
                || Ok(Unit(execute(format!("runner {edit}").into_bytes().into()))),
            )
            .unwrap();
        assert_eq!(generic.value().0, large(1, edit));
        assert_eq!(&*runner.value().0, format!("runner {edit}").as_bytes());
    }

    fn actions(&self, kind: Kind) -> usize {
        fs::read_dir(
            self.store
                .root()
                .join("actions")
                .join(kind.directory())
                .join(node::STORAGE_DIRECTORY),
        )
        .map_or(0, Iterator::count)
    }

    fn parts(&self) -> std::collections::BTreeSet<String> {
        fs::read_dir(super::super::parts::directory(self.store.root()))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.len() == 64)
            .collect()
    }

    fn named(&self) -> std::collections::BTreeSet<String> {
        super::super::parts::named(self.store.root())
            .unwrap()
            .iter()
            .map(|digest| digest.iter().map(|byte| format!("{byte:02x}")).collect())
            .collect()
    }

    fn parts_directory(&self) -> PathBuf {
        super::super::parts::directory(self.store.root())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = crate::store::remove_tree(&self.base);
    }
}

fn kept() -> usize {
    super::KEPT
        .iter()
        .find(|(kind, _)| *kind == Kind::Runner)
        .map(|(_, keep)| *keep)
        .unwrap()
}

// Every edit is a generation: a resolution, a generic, its overlay and its
// runner. N edits leave the kept generations of each kind and no more, and
// a retired generation's parts go with it (#1356, #1358).
#[test]
fn n_edits_leave_at_most_the_kept_generations_of_each_kind() {
    let fixture = Fixture::new("retire");
    let edits = 3 * kept();
    for edit in 0..edits {
        fixture.edit(edit);
    }
    fixture.store.fixture_retire_kept().unwrap();
    for kind in [Kind::Resolution, Kind::Generic, Kind::Overlay, Kind::Runner] {
        assert!(
            fixture.actions(kind) <= kept(),
            "{kind:?} keeps {} of {edits} generations",
            fixture.actions(kind)
        );
    }
    assert_eq!(fixture.actions(Kind::Overlay), kept(), "siblings stay");
    assert_eq!(
        fixture.parts(),
        fixture.named(),
        "every kept part is named and every named part kept"
    );
}

// An undo inside the kept window hits every unit of the generation it
// returns to, its parts read back whole; one older executes again.
#[test]
fn a_kept_generations_parts_stay_and_an_undo_inside_the_window_hits() {
    let fixture = Fixture::new("undo");
    let edits = 2 * kept();
    for edit in 0..edits {
        fixture.edit(edit);
    }
    fixture.store.fixture_retire_kept().unwrap();
    let executed = fixture.executions.get();
    for edit in edits - kept()..edits {
        fixture.edit(edit);
    }
    assert_eq!(
        fixture.executions.get(),
        executed,
        "the kept generations hit"
    );
    fixture.edit(0);
    assert_eq!(
        fixture.executions.get(),
        executed + 4,
        "a retired generation executes again"
    );
}

// A part no stored manifest names goes at the next retirement; the parts the
// kept manifests name stay.
#[test]
fn a_part_no_manifest_names_goes() {
    let fixture = Fixture::new("unnamed");
    for edit in 0..=kept() {
        fixture.edit(edit);
    }
    let orphan = super::super::parts::write(&fixture.parts_directory(), &large(9, 0)).unwrap();
    let kept_parts = fixture.named();
    assert!(fixture.parts().len() > kept_parts.len());
    // The next edit retires the oldest generation.
    fixture.edit(kept() + 1);
    fixture.store.fixture_retire_kept().unwrap();
    assert!(super::super::parts::present(&fixture.parts_directory(), &orphan).is_err());
    assert_eq!(fixture.parts(), fixture.named());
}
