use std::marker::PhantomData;
use std::sync::Arc;

use super::codec::Envelope;
use super::read_set::Witness;
use super::{ActionKey, ContentRef, Digest, Direct, Error, Fragment, Receipt, Validated, Value};

#[cfg(test)]
mod tests;

/// Authenticated owned output and historical provenance, without current authority.
///
/// Only full union validation can promote this value into a current receipt.
/// It carries neither a retention pin nor permission to consume external files.
pub(crate) struct Staged<K> {
    value: Arc<K>,
    content: ContentRef<K>,
    action: ActionKey,
    recipe: Digest,
    fragment: Fragment,
}

impl<K> Staged<K> {
    fn into_current(self) -> Validated<K> {
        let receipt = Receipt::new(self.action, self.recipe, self.content, self.fragment);
        Validated::new(self.value, self.content, receipt)
    }
    pub(crate) const fn action(&self) -> ActionKey {
        self.action
    }
    pub(crate) const fn fragment(&self) -> &Fragment {
        &self.fragment
    }

    pub(crate) fn value(&self) -> &K {
        self.value.as_ref()
    }

    pub(crate) const fn content(&self) -> ContentRef<K> {
        self.content
    }

    pub(crate) fn validate_current(
        self,
        store: &crate::store::Store,
    ) -> Result<Validated<K>, Error> {
        let proof = Fragment::seal(store, [self.fragment])?;
        let receipt = Receipt::new(
            self.action,
            self.recipe,
            self.content,
            proof.into_fragment(),
        );
        Ok(Validated::new(self.value, self.content, receipt))
    }

    /// Promote an exact two-output pure chain using its complete successor
    /// union once. Both receipts remain private until the entire seal succeeds.
    pub(super) fn validate_pair<N>(
        self,
        successor: Staged<N>,
        store: &crate::store::Store,
    ) -> Result<(Validated<K>, Validated<N>), Error> {
        successor
            .fragment
            .require_successor(&self.fragment, successor.action)?;
        let proof = Fragment::seal(store, [successor.fragment])?;
        let first = Receipt::new(self.action, self.recipe, self.content, self.fragment);
        let second = Receipt::new(
            successor.action,
            successor.recipe,
            successor.content,
            proof.into_fragment(),
        );
        Ok((
            Validated::new(self.value, self.content, first),
            Validated::new(successor.value, successor.content, second),
        ))
    }
}

/// Promote one exact pure descendant pair at its real consumer boundary.
pub(super) fn promote_owned_pair<A: Value, B: Value>(
    first: super::staging::Owned<A>,
    second: super::staging::Owned<B>,
    store: &crate::store::Store,
) -> Result<(Validated<A>, Validated<B>), Error> {
    use super::content::Edge;
    use super::staging::Owned;
    second
        .read_set()
        .require_successor(first.read_set(), second.action())?;
    second.read_set().charge_copy()?;
    Fragment::seal(store, [second.read_set().clone()])?;
    fn current<K>(value: Owned<K>) -> Validated<K> {
        match value {
            Owned::Current(value) => value,
            Owned::Staged(value) => value.into_current(),
        }
    }
    Ok((current(first), current(second)))
}

/// The private conversion is reachable only after every supplied branch's
/// original fragment has participated in this successful complete union.
pub(super) fn promote_three<A: Value, B: Value, C: Value>(
    first: super::staging::Owned<A>,
    second: super::staging::Owned<B>,
    third: super::staging::Owned<C>,
    store: &crate::store::Store,
) -> Result<super::staging::PromotedThree<A, B, C>, Error> {
    use super::content::Edge;
    use super::staging::Owned;
    let fragments = [first.read_set(), second.read_set(), third.read_set()];
    for fragment in fragments {
        fragment.charge_copy()?;
    }
    Fragment::seal(store, fragments.into_iter().cloned())?;
    fn current<K>(value: Owned<K>) -> Validated<K> {
        match value {
            Owned::Current(value) => value,
            Owned::Staged(value) => value.into_current(),
        }
    }
    Ok((current(first), current(second), current(third)))
}

pub(super) fn promote_four<A: Value, B: Value, C: Value, D: Value>(
    first: super::staging::Owned<A>,
    second: super::staging::Owned<B>,
    third: super::staging::Owned<C>,
    fourth: super::staging::Owned<D>,
    store: &crate::store::Store,
) -> Result<super::staging::PromotedFour<A, B, C, D>, Error> {
    use super::content::Edge;
    use super::staging::Owned;
    let fragments = [
        first.read_set(),
        second.read_set(),
        third.read_set(),
        fourth.read_set(),
    ];
    for fragment in fragments {
        fragment.charge_copy()?;
    }
    Fragment::seal(store, fragments.into_iter().cloned())?;
    fn current<K>(value: Owned<K>) -> Validated<K> {
        match value {
            Owned::Current(value) => value,
            Owned::Staged(value) => value.into_current(),
        }
    }
    Ok((
        current(first),
        current(second),
        current(third),
        current(fourth),
    ))
}

#[cfg(feature = "rustc-driver")]
pub(super) fn promote_five<A: Value, B: Value, C: Value, D: Value, E: Value>(
    first: super::staging::Owned<A>,
    second: super::staging::Owned<B>,
    third: super::staging::Owned<C>,
    fourth: super::staging::Owned<D>,
    fifth: super::staging::Owned<E>,
    store: &crate::store::Store,
) -> Result<super::staging::PromotedFive<A, B, C, D, E>, Error> {
    use super::content::Edge;
    use super::staging::Owned;
    let fragments = [
        first.read_set(),
        second.read_set(),
        third.read_set(),
        fourth.read_set(),
        fifth.read_set(),
    ];
    for fragment in fragments {
        fragment.charge_copy()?;
    }
    Fragment::seal(store, fragments.into_iter().cloned())?;
    fn current<K>(value: Owned<K>) -> Validated<K> {
        match value {
            Owned::Current(value) => value,
            Owned::Staged(value) => value.into_current(),
        }
    }
    Ok((
        current(first),
        current(second),
        current(third),
        current(fourth),
        current(fifth),
    ))
}

/// Capability-free decoded store bytes.
pub(crate) struct Stored<K> {
    envelope: Envelope,
    bytes: Box<[u8]>,
    bundled: bool,
    _kind: PhantomData<fn() -> K>,
}

/// Typed output decoded from bytes but not yet rebound to the current recipe.
pub(crate) struct Replayed<K> {
    envelope: Envelope,
    value: Arc<K>,
    bundled: bool,
    graph: super::GraphBudget,
}

impl<K: Value> Stored<K> {
    /// Only the store reader constructs this, after authenticating the bytes'
    /// length and content digest against the envelope; nothing rehashes them.
    pub(crate) fn new(envelope: Envelope, bytes: Box<[u8]>, bundled: bool) -> Self {
        Self {
            envelope,
            bytes,
            bundled,
            _kind: PhantomData,
        }
    }

    pub(crate) fn decode(
        self,
        current: &Direct<K>,
        graph: &super::GraphBudget,
    ) -> Result<Validated<K>, Error> {
        if !self.envelope.matches(current) {
            return Err(Error::Codec(
                "stored node does not match the current recipe",
            ));
        }
        let replayed = self.inspect(current.output_limit, graph)?;
        let content = ContentRef::new(replayed.envelope.content);
        let witness = Witness::direct(current, content, replayed.bundled);
        replayed.validate(current, witness, graph)
    }

    pub(crate) fn stage(
        self,
        current: &Direct<K>,
        graph: &super::GraphBudget,
    ) -> Result<Staged<K>, Error> {
        if !self.envelope.matches(current) {
            return Err(Error::Codec("stored node does not match the staged recipe"));
        }
        let replayed = self.inspect(current.output_limit, graph)?;
        let witness = Witness::direct(current, replayed.content(), replayed.bundled);
        replayed.stage(current, witness, graph)
    }

    /// Decode bytes whose content identity was authenticated by the store caller.
    pub(crate) fn inspect(
        self,
        output_limit: usize,
        graph: &super::GraphBudget,
    ) -> Result<Replayed<K>, Error> {
        if self.envelope.output_limit != output_limit {
            return Err(Error::Codec("stored node content limit changed"));
        }
        let value = Arc::new(K::decode_owned(self.bytes, output_limit)?);
        Ok(Replayed {
            envelope: self.envelope,
            value,
            bundled: self.bundled,
            graph: graph.clone(),
        })
    }
}

impl<K: Value> Replayed<K> {
    pub(crate) fn value(&self) -> &K {
        self.value.as_ref()
    }

    pub(crate) fn value_arc(&self) -> Arc<K> {
        Arc::clone(&self.value)
    }

    pub(crate) const fn content(&self) -> ContentRef<K> {
        ContentRef::new(self.envelope.content)
    }

    pub(crate) const fn bundled(&self) -> bool {
        self.bundled
    }

    pub(crate) fn validate(
        self,
        current: &Direct<K>,
        witness: Witness,
        graph: &super::GraphBudget,
    ) -> Result<Validated<K>, Error> {
        let read_set = self.compose(current, witness, graph)?;
        let content = self.content();
        let receipt = Receipt::new(current.action, current.recipe, content, read_set);
        Ok(Validated::new(self.value, content, receipt))
    }

    pub(crate) fn stage(
        self,
        current: &Direct<K>,
        witness: Witness,
        graph: &super::GraphBudget,
    ) -> Result<Staged<K>, Error> {
        let fragment = self.compose(current, witness, graph)?;
        Ok(Staged {
            content: self.content(),
            value: self.value,
            action: current.action,
            recipe: current.recipe,
            fragment,
        })
    }

    fn compose(
        &self,
        current: &Direct<K>,
        witness: Witness,
        graph: &super::GraphBudget,
    ) -> Result<Fragment, Error> {
        if !self.graph.shares_with(graph) {
            return Err(Error::State("replayed node belongs to another graph"));
        }
        if !self.envelope.matches(current) {
            return Err(Error::Codec("replayed node recipe became stale"));
        }
        let content = ContentRef::new(self.envelope.content);
        Fragment::compose::<K>(
            &current.parents,
            current.action,
            current.recipe,
            content,
            self.bundled,
            witness,
            current.context,
            graph,
        )
    }
}
