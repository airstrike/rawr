//! What one item's classification reads, and whether another graph gives
//! it the same (#1618).
//!
//! An item is classified under a record of its reads: what the graph's
//! accessors give it (`api::reads`), and what it takes of the answers the
//! classification reads once for every item, such as a graph's instance
//! aliases or an opaque type's left-out captures. Each read is a [`Key`],
//! and what a graph gives it is the key's digest there. An item whose every
//! key has the same digest in a later graph reads the same there, so its
//! classification is the one it had: each item depends on nothing but its
//! reads, whichever worker classifies it and in whatever order (#1669).

use std::cell::RefCell;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{api, Context};

/// A shared answer an item reads, by what it asked.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(super) enum Memo {
    /// The crate type aliases that are the only name of their type.
    InstanceAliases,
    /// Each type whose only name is a passing alias, with that alias.
    SoleAliases,
    /// The definitions some import exports, by root.
    Exported,
    /// The types whose only name is a passing alias, with their roots.
    Passing,
    /// The implementations of the `core` trait of this name.
    Core(Box<str>),
    /// What a projection bound on the associated type reads through the
    /// trait.
    Projection {
        associated: api::identity::Definition,
        trait_id: api::identity::Definition,
    },
    /// `core`'s `Fn`, `FnOnce` and `FnOnce::Output`.
    Callable,
    /// The first definition whose semantic identity this is.
    Surface(crate::surface::identity::Definition),
    /// The captures an opaque type's own function leaves out.
    LeftOut(api::identity::Definition),
    /// The arguments an opaque type's own function gives its bindings.
    Bound(api::identity::Definition),
}

/// One read of an item's classification.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(super) enum Key {
    Definition(api::identity::Definition),
    Implementation(api::identity::Definition),
    Group(api::reads::Group, api::identity::Definition),
    Ledger(api::reads::Ledger),
    Memo(Memo),
}

thread_local! {
    static MEMOS: RefCell<Vec<Memo>> = const { RefCell::new(Vec::new()) };
}

/// Note that the item being classified on this thread read `memo`.
pub(super) fn note(memo: impl FnOnce() -> Memo) {
    if api::reads::recording() {
        MEMOS.with_borrow_mut(|memos| memos.push(memo()));
    }
}

/// Run `work`, one item's classification of `graph`, and give the keys it
/// read, each once, in order.
pub(super) fn record<T, P: api::Phase>(
    graph: &api::Graph<P>,
    work: impl FnOnce() -> T,
) -> (T, Vec<Key>) {
    let mut saved = Saved(Some(MEMOS.take()));
    let (done, noted) = api::reads::record(work);
    let memos = MEMOS.replace(
        saved
            .0
            .take()
            .expect("the outer memos stay set aside while the work runs"),
    );
    let mut keys = noted
        .reads(graph)
        .into_iter()
        .map(|read| match read {
            api::reads::Read::Definition(id) => Key::Definition(id),
            api::reads::Read::Implementation(id) => Key::Implementation(id),
            api::reads::Read::Group(group, id) => Key::Group(group, id),
            api::reads::Read::Ledger(ledger) => Key::Ledger(ledger),
        })
        .collect::<Vec<_>>();
    keys.extend(memos.into_iter().map(Key::Memo));
    keys.sort_unstable();
    keys.dedup();
    (done, keys)
}

/// The memos noted before a [`record`], put back when it ends, by return
/// or unwind.
struct Saved(Option<Vec<Memo>>);

impl Drop for Saved {
    fn drop(&mut self) {
        if let Some(memos) = self.0.take() {
            MEMOS.replace(memos);
        }
    }
}

/// The digest of what a graph gives one key.
pub(super) type Digest = api::reads::Digest;

/// The digest of `value`'s encoding.
pub(super) fn encoded<T: Serialize + ?Sized>(value: &T) -> Digest {
    Sha256::digest(
        serde_json::to_vec(value).expect("a classification's shared answer encodes as JSON"),
    )
    .into()
}

impl Context<'_> {
    /// What this classification's graph and shared answers give `key`.
    pub(super) fn digest(&self, key: &Key) -> Digest {
        let graph = self.graph;
        api::reads::paused(|| match key {
            Key::Definition(id) => graph.definition_identity_digest(id),
            Key::Implementation(id) => graph.implementation_identity_digest(id),
            Key::Group(group, id) => graph.group_digest(*group, id),
            Key::Ledger(ledger) => graph.ledger_digest(*ledger),
            Key::Memo(memo) => self.memo_digest(memo),
        })
    }

    fn memo_digest(&self, memo: &Memo) -> Digest {
        let graph = self.graph;
        let definitions = |indices: &mut dyn Iterator<Item = usize>| {
            let mut digest = Sha256::new();
            for index in indices {
                digest.update(graph.definition_digest(index));
            }
            <[u8; 32]>::from(digest.finalize())
        };
        match memo {
            Memo::InstanceAliases => definitions(&mut self.instance_aliases().iter().copied()),
            Memo::SoleAliases => definitions(
                &mut self
                    .sole_aliases()
                    .iter()
                    .flat_map(|(target, alias)| [*target, *alias]),
            ),
            Memo::Exported => encoded(self.exported()),
            Memo::Passing => {
                let mut digest = Sha256::new();
                for (root, alias, target) in self.passing() {
                    digest.update(root.as_bytes());
                    digest.update([0]);
                    digest.update(graph.definition_identity_digest(alias.id()));
                    digest.update(graph.definition_identity_digest(target.id()));
                }
                digest.finalize().into()
            }
            Memo::Core(name) => {
                let mut digest = Sha256::new();
                for implementation in self.core_implementations(name) {
                    digest.update(graph.implementation_identity_digest(implementation.id()));
                }
                digest.finalize().into()
            }
            Memo::Projection {
                associated,
                trait_id,
            } => {
                let Some(name) = graph
                    .definition_index(associated)
                    .and_then(|index| graph.definition_at(index))
                    .and_then(|definition| definition.display_path().last())
                else {
                    return api::reads::ABSENT;
                };
                let cases =
                    super::ProjectionCases::read(graph, trait_id, name, self.instance_aliases());
                let mut digest = Sha256::new();
                for case in cases.cases.iter() {
                    digest.update(encoded(&case.candidate));
                    digest.update(encoded(&case.value));
                }
                digest.finalize().into()
            }
            Memo::Callable => encoded(&self.callable_traits()),
            Memo::Surface(identity) => self
                .surface_definitions()
                .position(identity)
                .map_or(api::reads::ABSENT, |index| graph.definition_digest(index)),
            Memo::LeftOut(id) => {
                encoded(&self.shared.left_out.get().and_then(|table| table.get(id)))
            }
            Memo::Bound(id) => encoded(&self.shared.bound.get().and_then(|table| table.get(id))),
        }
    }
}
