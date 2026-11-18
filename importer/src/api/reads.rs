//! What one piece of work reads of an API graph, noted as it reads (#1618).
//!
//! While a thread records ([`record`]), each accessor of a graph's ledgers
//! notes what it gave: a definition or an implementation by its position,
//! an identity the graph has no entry for, a group of implementations by
//! the identity it was asked for, or a whole ledger it walked. The notes
//! name everything the work could see of the graph, so another graph that
//! gives each of them the same content gives the work the same inputs.
//! [`Digests`] says what a graph gives each, as a digest of its content.

use std::cell::{Cell, RefCell};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{identity, model, Graph, Phase};

thread_local! {
    /// Whether this thread's reads are noted.
    static RECORDING: Cell<bool> = const { Cell::new(false) };
    /// How deep this thread is in work whose reads are not its own
    /// ([`paused`]).
    static PAUSED: Cell<u32> = const { Cell::new(0) };
    static NOTED: RefCell<Noted> = RefCell::new(Noted::default());
}

/// A group of implementations an accessor gives for one identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum Group {
    /// The implementations of a trait (`Graph::implementations_of`).
    Trait,
    /// The implementations for a nominal type (`Graph::implementations_for`).
    SelfType,
    /// The implementations relevant to a definition
    /// (`Graph::relevant_implementations`).
    Relevant,
}

/// A ledger an accessor walks whole.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum Ledger {
    Definitions,
    Implementations,
    Exports,
    Support,
    Index,
    Anonymous,
    ScalarProofs,
    /// The implementations whose self type is one of their own parameters
    /// (`Graph::blanket_implementations`).
    Blankets,
}

/// What a recorded piece of work read, by position in the graph it read.
#[derive(Default)]
pub(crate) struct Noted {
    pub(crate) definitions: Vec<u32>,
    pub(crate) absent_definitions: Vec<identity::Definition>,
    pub(crate) implementations: Vec<u32>,
    pub(crate) absent_implementations: Vec<identity::Definition>,
    pub(crate) groups: Vec<(Group, identity::Definition)>,
    pub(crate) ledgers: Vec<Ledger>,
}

/// Run `work` noting what it reads of any graph, on this thread.
/// The work's own reads are noted whatever work around it pauses; the
/// thread's state around it is back when it ends, by return or unwind.
pub(crate) fn record<T>(work: impl FnOnce() -> T) -> (T, Noted) {
    let mut outer = Outer {
        recording: RECORDING.replace(true),
        paused: PAUSED.replace(0),
        noted: Some(NOTED.take()),
    };
    let done = work();
    let noted = NOTED.replace(
        outer
            .noted
            .take()
            .expect("the outer notes stay set aside while the work runs"),
    );
    drop(outer);
    (done, noted)
}

/// What a thread noted and how before a [`record`], put back when it ends.
struct Outer {
    recording: bool,
    paused: u32,
    noted: Option<Noted>,
}

impl Drop for Outer {
    fn drop(&mut self) {
        RECORDING.set(self.recording);
        PAUSED.set(self.paused);
        if let Some(noted) = self.noted.take() {
            NOTED.replace(noted);
        }
    }
}

/// Run `work` without noting its reads: what it computes is read once for
/// many pieces of work, each of which notes reading the result itself.
pub(crate) fn paused<T>(work: impl FnOnce() -> T) -> T {
    PAUSED.set(PAUSED.get() + 1);
    let _resume = Resume;
    work()
}

/// Ends a [`paused`] stretch, by return or unwind.
struct Resume;

impl Drop for Resume {
    fn drop(&mut self) {
        PAUSED.set(PAUSED.get() - 1);
    }
}

/// Whether this thread notes what it reads now: it records, and is not
/// paused.
pub(crate) fn recording() -> bool {
    RECORDING.get() && PAUSED.get() == 0
}

/// Note one read, when this thread records and is not paused.
fn note(read: impl FnOnce(&mut Noted)) {
    if recording() {
        NOTED.with_borrow_mut(read);
    }
}

pub(crate) fn definition(index: usize) {
    note(|noted| noted.definitions.push(index as u32));
}

pub(super) fn absent_definition(id: &identity::Definition) {
    note(|noted| noted.absent_definitions.push(id.clone()));
}

pub(super) fn implementation(index: usize) {
    note(|noted| noted.implementations.push(index as u32));
}

pub(super) fn absent_implementation(id: &identity::Definition) {
    note(|noted| noted.absent_implementations.push(id.clone()));
}

pub(super) fn group(group: Group, id: &identity::Definition) {
    note(|noted| noted.groups.push((group, id.clone())));
}

pub(super) fn ledger(ledger: Ledger) {
    note(|noted| noted.ledgers.push(ledger));
}

/// The digest of what a graph gives one read; [`ABSENT`] for an identity
/// it has no entry for.
pub(crate) type Digest = [u8; 32];

/// What a graph gives an identity it has no entry for.
pub(crate) const ABSENT: Digest = [0; 32];

/// Each entry's digest, computed at the first ask and shared by the
/// graph's copies.
#[derive(Default)]
pub(super) struct Digests {
    definitions: OnceLock<Box<[OnceLock<Digest>]>>,
    implementations: OnceLock<Box<[OnceLock<Digest>]>>,
    ledgers: [OnceLock<Digest>; 8],
}

/// The digest of `value`'s canonical encoding.
fn encoded<T: Serialize + ?Sized>(value: &T) -> Digest {
    let mut digest = Sha256::new();
    serde_json::to_writer(DigestWriter(&mut digest), value)
        .expect("a ledger entry encodes, as the graph's canonical bytes encode it");
    digest.finalize().into()
}

/// Writes JSON straight into a digest.
struct DigestWriter<'a>(&'a mut Sha256);

impl std::io::Write for DigestWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<P: Phase> Graph<P> {
    fn digests(&self) -> &Digests {
        &self.lookup.digests
    }

    /// The digest of the definition at `index`.
    pub(crate) fn definition_digest(&self, index: usize) -> Digest {
        let slots = self.digests().definitions.get_or_init(|| {
            (0..self.definitions.len())
                .map(|_| OnceLock::new())
                .collect()
        });
        *slots[index].get_or_init(|| encoded(&self.definitions[index]))
    }

    /// The digest of the implementation at `index`.
    pub(crate) fn implementation_digest(&self, index: usize) -> Digest {
        let slots = self.digests().implementations.get_or_init(|| {
            (0..self.implementations.len())
                .map(|_| OnceLock::new())
                .collect()
        });
        *slots[index].get_or_init(|| encoded(&self.implementations[index]))
    }

    /// The digest of the definition `id`: [`ABSENT`] when the graph has
    /// none.
    pub(crate) fn definition_identity_digest(&self, id: &identity::Definition) -> Digest {
        paused(|| self.definition_index(id)).map_or(ABSENT, |index| self.definition_digest(index))
    }

    /// The digest of the implementation `id`: [`ABSENT`] when the graph
    /// has none.
    pub(crate) fn implementation_identity_digest(&self, id: &identity::Definition) -> Digest {
        paused(|| self.implementation_index(id))
            .map_or(ABSENT, |index| self.implementation_digest(index))
    }

    /// The digest of the implementations `group` gives for `id`, in the
    /// order the accessor gives them.
    pub(crate) fn group_digest(&self, group: Group, id: &identity::Definition) -> Digest {
        let digest = |members: &mut dyn Iterator<Item = &model::Implementation>| {
            let mut digest = Sha256::new();
            let mut any = false;
            for member in members {
                let index = paused(|| self.implementation_index(member.id()))
                    .expect("a group's member is one of the graph's implementations");
                digest.update(self.implementation_digest(index));
                any = true;
            }
            if any {
                digest.finalize().into()
            } else {
                ABSENT
            }
        };
        paused(|| match group {
            Group::Trait => digest(&mut self.implementations_of(id)),
            Group::SelfType => digest(&mut self.implementations_for(id)),
            Group::Relevant => digest(&mut self.relevant_implementations(id)),
        })
    }

    /// The digest of the whole of `ledger`.
    pub(crate) fn ledger_digest(&self, ledger: Ledger) -> Digest {
        *self.digests().ledgers[ledger as usize].get_or_init(|| match ledger {
            Ledger::Definitions => {
                let mut digest = Sha256::new();
                for index in 0..self.definitions.len() {
                    digest.update(self.definition_digest(index));
                }
                digest.finalize().into()
            }
            Ledger::Implementations => {
                let mut digest = Sha256::new();
                for index in 0..self.implementations.len() {
                    digest.update(self.implementation_digest(index));
                }
                digest.finalize().into()
            }
            Ledger::Exports => encoded(&*self.exports),
            Ledger::Support => encoded(&*self.support),
            Ledger::Index => encoded(&*self.index),
            Ledger::Anonymous => encoded(&*self.anonymous),
            Ledger::ScalarProofs => encoded(&*self.scalar_predicate_proofs),
            Ledger::Blankets => {
                let mut digest = Sha256::new();
                for index in self.blanket_indices() {
                    digest.update(self.implementation_digest(*index as usize));
                }
                digest.finalize().into()
            }
        })
    }
}
