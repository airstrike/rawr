//! Internal pure composition without current-parent or execution authority.

use super::content::Edge;
use super::replay::Staged;
use super::{Chain, Direct, Error, Evaluator, Lookup, Miss, Parent, Validated, Value};

/// A staged recipe never escapes as a public current-parent direct recipe.
pub(crate) struct Recipe<K> {
    direct: Direct<K>,
    current: bool,
}

/// A borrowed edge with authority determined by its importer-owned origin.
pub(crate) struct Input<'a> {
    edge: &'a dyn Edge,
    current: bool,
}

impl<'a> Input<'a> {
    pub(crate) fn current(parent: &'a Parent) -> Self {
        Self {
            edge: parent,
            current: true,
        }
    }
}

impl Edge for Input<'_> {
    fn action(&self) -> super::ActionKey {
        self.edge.action()
    }
    fn kind(&self) -> super::Kind {
        self.edge.kind()
    }
    fn schema(&self) -> u32 {
        self.edge.schema()
    }
    fn content(&self) -> super::Digest {
        self.edge.content()
    }
    fn read_set(&self) -> &super::Fragment {
        self.edge.read_set()
    }
}

/// Owned data never silently upgrades a staged result to current authority.
pub(crate) enum Owned<K> {
    Current(Validated<K>),
    Staged(Staged<K>),
}

pub(crate) enum Outcome<K> {
    Hit(Owned<K>),
    Miss(Miss),
}

impl<K: Value> Owned<K> {
    pub(crate) fn value(&self) -> &K {
        match self {
            Self::Current(value) => value.value(),
            Self::Staged(value) => value.value(),
        }
    }

    pub(crate) fn input(&self) -> Input<'_> {
        Input {
            edge: self,
            current: matches!(self, Self::Current(_)),
        }
    }
}

impl<K: Value> Edge for Owned<K> {
    fn action(&self) -> super::ActionKey {
        match self {
            Self::Current(value) => value.receipt().action(),
            Self::Staged(value) => value.action(),
        }
    }
    fn kind(&self) -> super::Kind {
        K::KIND
    }
    fn schema(&self) -> u32 {
        K::SCHEMA
    }
    fn content(&self) -> super::Digest {
        match self {
            Self::Current(value) => value.content().digest(),
            Self::Staged(value) => value.content().digest(),
        }
    }
    fn read_set(&self) -> &super::Fragment {
        match self {
            Self::Current(value) => value.receipt().read_set(),
            Self::Staged(value) => value.fragment(),
        }
    }
}

impl<K: Value> Recipe<K> {
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn from_current_direct(direct: Direct<K>) -> Self {
        Self {
            direct,
            current: true,
        }
    }

    pub(crate) fn from_chain(chain: &Chain, inputs: &[u8], limit: usize) -> Result<Self, Error> {
        chain.current_parent().read_set().charge_copy()?;
        chain
            .context()
            .direct_with_parents(inputs, limit, std::slice::from_ref(chain.current_parent()))
            .map(|direct| Self {
                direct,
                current: true,
            })
    }

    pub(crate) fn child<P: Value>(
        context: &crate::context::Context,
        parent: &Staged<P>,
        inputs: &[u8],
        limit: usize,
    ) -> Result<Self, Error> {
        parent.fragment().charge_copy()?;
        Direct::capture_edges(
            context.identity(),
            inputs,
            limit,
            std::slice::from_ref(parent),
            context.limits().edges,
        )
        .map(|direct| Self {
            direct,
            current: false,
        })
    }

    pub(crate) fn from_inputs(
        context: &crate::context::Context,
        parents: &[Input<'_>],
        inputs: &[u8],
        limit: usize,
    ) -> Result<Self, Error> {
        for parent in parents {
            parent.read_set().charge_copy()?;
        }
        let direct = Direct::capture_edges(
            context.identity(),
            inputs,
            limit,
            parents,
            context.limits().edges,
        )?;
        Ok(Self {
            direct,
            current: parents.iter().all(|parent| parent.current),
        })
    }

    /// Only recipes derived entirely from current parents can yield the
    /// established usable current hit. Mixed history stays staged.
    pub(crate) fn lookup(&self, evaluator: &Evaluator<'_>) -> Result<Lookup<K>, Error> {
        if !self.current {
            return Err(Error::State("staged recipe cannot mint a current hit"));
        }
        evaluator.lookup(&self.direct)
    }

    pub(crate) fn lookup_owned(&self, evaluator: &Evaluator<'_>) -> Result<Outcome<K>, Error> {
        if self.current {
            return self.lookup(evaluator).map(|outcome| match outcome {
                Lookup::Hit(value) => Outcome::Hit(Owned::Current(value)),
                Lookup::Miss(miss) => Outcome::Miss(miss),
            });
        }
        let result = evaluator
            .store
            .lookup_staged(&self.direct, evaluator.store.graph_budget());
        match result {
            Ok(Some(value)) => {
                // A mixed-history hit is reported by Batch only after its
                // retained continuation and required promotion succeed.
                Ok(Outcome::Hit(Owned::Staged(value)))
            }
            Ok(None) => Ok(Outcome::Miss(Miss::Absent)),
            Err(Error::CleanupRequired(_)) => {
                evaluator
                    .metrics
                    .node(K::KIND.metric(), crate::metrics::NodeEvent::RejectedHit);
                Ok(Outcome::Miss(Miss::RecoveryRequired))
            }
            Err(error) => {
                evaluator
                    .metrics
                    .node(K::KIND.metric(), crate::metrics::NodeEvent::RejectedHit);
                Err(error)
            }
        }
    }

    pub(crate) fn resume(
        &self,
        batch: &mut crate::store::Batch<'_>,
        miss: Miss,
        execute: impl FnOnce() -> Result<K, Error>,
    ) -> Result<Staged<K>, Error> {
        batch.resume(&self.direct, miss, execute)
    }

    pub(crate) fn evaluate(
        &self,
        batch: &mut crate::store::Batch<'_>,
        execute: impl FnOnce() -> Result<K, Error>,
    ) -> Result<Staged<K>, Error> {
        batch.evaluate(&self.direct, execute)
    }
}

/// Genuine consumer boundary for an exact branch. Seals the union once and
/// returns each output with its own original semantic recipe and closure.
pub(crate) fn promote_three<A: Value, B: Value, C: Value>(
    first: Owned<A>,
    second: Owned<B>,
    third: Owned<C>,
    store: &crate::store::Store,
) -> Result<(Validated<A>, Validated<B>, Validated<C>), Error> {
    super::replay::promote_three(first, second, third, store)
}

pub(crate) fn promote_four<A: Value, B: Value, C: Value, D: Value>(
    first: Owned<A>,
    second: Owned<B>,
    third: Owned<C>,
    fourth: Owned<D>,
    store: &crate::store::Store,
) -> Result<(Validated<A>, Validated<B>, Validated<C>, Validated<D>), Error> {
    super::replay::promote_four(first, second, third, fourth, store)
}
#[cfg(feature = "rustc-driver")]
pub(crate) fn promote_five<A: Value, B: Value, C: Value, D: Value, E: Value>(
    first: Owned<A>,
    second: Owned<B>,
    third: Owned<C>,
    fourth: Owned<D>,
    fifth: Owned<E>,
    store: &crate::store::Store,
) -> Result<
    (
        Validated<A>,
        Validated<B>,
        Validated<C>,
        Validated<D>,
        Validated<E>,
    ),
    Error,
> {
    super::replay::promote_five(first, second, third, fourth, fifth, store)
}

impl<K: Value> Edge for Staged<K> {
    fn action(&self) -> super::ActionKey {
        self.action()
    }
    fn kind(&self) -> super::Kind {
        K::KIND
    }
    fn schema(&self) -> u32 {
        K::SCHEMA
    }
    fn content(&self) -> super::Digest {
        self.content().digest()
    }
    fn read_set(&self) -> &super::Fragment {
        self.fragment()
    }
}

pub(crate) fn promote_pair<K, N>(
    first: Staged<K>,
    second: Staged<N>,
    store: &crate::store::Store,
) -> Result<(Validated<K>, Validated<N>), Error> {
    first.validate_pair(second, store)
}

pub(crate) fn promote_owned_pair<A: Value, B: Value>(
    first: Owned<A>,
    second: Owned<B>,
    store: &crate::store::Store,
) -> Result<(Validated<A>, Validated<B>), Error> {
    super::replay::promote_owned_pair(first, second, store)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Bytes;
    impl Value for Bytes {
        const KIND: super::super::Kind = super::super::Kind::Generic;
        const SCHEMA: u32 = 1;
        fn encode(&self, _: usize) -> Result<Box<[u8]>, Error> {
            Ok(Box::from(&b"bytes"[..]))
        }
        fn decode(bytes: &[u8], _: usize) -> Result<Self, Error> {
            if bytes == b"bytes" {
                Ok(Self)
            } else {
                Err(Error::Codec("fixture bytes changed"))
            }
        }
    }

    #[test]
    fn staged_parent_lookup_preserves_historical_authority() {
        let root =
            std::env::temp_dir().join(format!("rawr-staging-authority-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let environment =
            crate::cargo::config::Environment::from_pairs([("CARGO_HOME", root.to_str().unwrap())])
                .unwrap();
        let context = crate::context::Context::capture(
            &root,
            environment,
            "fixture",
            &crate::cargo::Limits::standard(),
        )
        .unwrap();
        let store = context.open_store().unwrap();
        let evaluator = Evaluator::new(&store, &crate::metrics::Current);
        let owner = evaluator
            .direct(&context.direct::<Bytes>(b"owner", 64).unwrap(), || {
                Ok(Bytes)
            })
            .unwrap();
        let retained = super::super::read_set::Retention::from_validated(&owner, None).unwrap();
        let parent = Parent::validated(&owner);
        let first =
            Recipe::<Bytes>::from_inputs(&context, &[Input::current(&parent)], b"first", 64)
                .unwrap();
        assert!(matches!(first.lookup(&evaluator).unwrap(), Lookup::Miss(_)));
        store
            .with_staged_batch(&retained, &crate::metrics::Current, |batch| {
                let first = Owned::Staged(first.evaluate(batch, || Ok(Bytes))?);
                let child = Recipe::<Bytes>::from_inputs(&context, &[first.input()], b"child", 64)?;
                assert!(matches!(
                    child.lookup(&evaluator),
                    Err(Error::State("staged recipe cannot mint a current hit"))
                ));
                let current = child.evaluate(batch, || Ok(Bytes))?;
                let Outcome::Hit(Owned::Staged(hit)) = child.lookup_owned(&evaluator)? else {
                    panic!("historical parent must retain staged hit authority")
                };
                assert_eq!(hit.action(), current.action());
                assert_eq!(hit.content().digest(), current.content().digest());
                Ok(())
            })
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
