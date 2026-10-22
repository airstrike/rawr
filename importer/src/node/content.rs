use std::marker::PhantomData;
use std::sync::Arc;

use super::{ActionKey, Digest, Fragment, Kind, Value};

/// Typed semantic identity of one canonical node output.
pub struct ContentRef<K> {
    digest: Digest,
    _kind: PhantomData<fn() -> K>,
}

impl<K> Clone for ContentRef<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for ContentRef<K> {}

impl<K> ContentRef<K> {
    pub(crate) const fn new(digest: Digest) -> Self {
        Self {
            digest,
            _kind: PhantomData,
        }
    }

    /// Borrow the stable content digest for downstream semantic recipes.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.digest
    }
}

/// One current validated parent retained as a semantic edge plus receipt chain.
#[derive(Clone)]
pub struct Parent {
    kind: Kind,
    schema: u32,
    action: ActionKey,
    content: Digest,
    read_set: Fragment,
}

/// Owned semantic edge and historical witnesses, without current authority.
#[derive(Clone)]
pub(crate) struct Provenance {
    kind: Kind,
    schema: u32,
    action: ActionKey,
    content: Digest,
    read_set: Fragment,
}

pub(crate) trait Edge {
    fn action(&self) -> ActionKey;
    fn kind(&self) -> Kind;
    fn schema(&self) -> u32;
    fn content(&self) -> Digest;
    fn read_set(&self) -> &Fragment;
}

impl Provenance {
    pub(super) fn from_edge(parent: &impl Edge) -> Self {
        Self {
            kind: parent.kind(),
            schema: parent.schema(),
            action: parent.action(),
            content: parent.content(),
            read_set: parent.read_set().clone(),
        }
    }

    pub(crate) const fn action(&self) -> ActionKey {
        self.action
    }
}

impl Edge for Provenance {
    fn action(&self) -> ActionKey {
        self.action
    }
    fn kind(&self) -> Kind {
        self.kind
    }
    fn schema(&self) -> u32 {
        self.schema
    }
    fn content(&self) -> Digest {
        self.content
    }
    fn read_set(&self) -> &Fragment {
        &self.read_set
    }
}

impl Edge for Parent {
    fn action(&self) -> ActionKey {
        self.action
    }
    fn kind(&self) -> Kind {
        self.kind
    }
    fn schema(&self) -> u32 {
        self.schema
    }
    fn content(&self) -> Digest {
        self.content
    }
    fn read_set(&self) -> &Fragment {
        &self.read_set
    }
}

/// Opaque current store context plus one validated downstream parent.
///
/// A chain can only be born from a [`Validated`] value. Stored content alone
/// cannot construct it, so downstream transforms inherit the current receipt
/// and its complete transitive read set.
#[derive(Clone)]
pub struct Chain {
    context: crate::context::Context,
    parent: Parent,
    retention: Option<Arc<super::read_set::Retention>>,
}

impl Parent {
    /// Erase only the value type after current validation, preserving its proof.
    #[must_use]
    pub fn validated<K: Value>(parent: &Validated<K>) -> Self {
        Self {
            kind: K::KIND,
            schema: K::SCHEMA,
            action: parent.receipt.action,
            content: parent.content.digest,
            read_set: parent.receipt.read_set.clone(),
        }
    }

    pub(crate) const fn kind(&self) -> Kind {
        self.kind
    }

    pub(crate) const fn action(&self) -> ActionKey {
        self.action
    }

    pub(crate) const fn content(&self) -> Digest {
        self.content
    }

    pub(crate) const fn read_set(&self) -> &Fragment {
        &self.read_set
    }
}

impl Chain {
    #[cfg(all(test, feature = "test-fixtures"))]
    pub(crate) fn fixture_bundled_retained(
        context: crate::context::Context,
        parents: &[Parent],
    ) -> Result<Self, super::Error> {
        let store = context.open_store()?;
        let (value, commitment) =
            super::tests::registered_retention_parent_in(&store, context.identity(), parents);
        let retained = super::read_set::Retention::from_validated(&value, Some(commitment))?;
        Self::validated_retained(context, &value, retained)
    }

    pub(crate) const fn current_parent(&self) -> &Parent {
        &self.parent
    }

    pub(crate) fn validated<K: Value>(
        context: crate::context::Context,
        parent: &Validated<K>,
    ) -> Self {
        Self {
            context,
            parent: Parent::validated(parent),
            retention: None,
        }
    }

    pub(crate) fn validated_retained<K: Value>(
        context: crate::context::Context,
        parent: &Validated<K>,
        retention: super::read_set::Retention,
    ) -> Result<Self, super::Error> {
        let parent = Parent::validated(parent);
        retention.require_parents(std::slice::from_ref(&parent))?;
        context
            .graph_budget()
            .decode(std::mem::size_of::<super::read_set::Retention>())?;
        Ok(Self {
            context,
            parent,
            retention: Some(Arc::new(retention)),
        })
    }

    pub(crate) fn retention(&self) -> Option<&super::read_set::Retention> {
        self.retention.as_deref()
    }

    /// Evaluate one exact child under this current validated parent.
    pub fn direct<K: Value>(
        &self,
        semantic_inputs: &[u8],
        execute: impl FnOnce() -> Result<K, super::Error>,
    ) -> Result<Validated<K>, super::Error> {
        self.direct_with_parents(semantic_inputs, &[], execute)
    }

    /// Evaluate one large artifact child under the existing store-wide content cap.
    pub(crate) fn direct_artifact<K: Value>(
        &self,
        semantic_inputs: &[u8],
        execute: impl FnOnce() -> Result<K, super::Error>,
    ) -> Result<Validated<K>, super::Error> {
        let recipe = self.context.direct_with_parents::<K>(
            semantic_inputs,
            super::MAX_CONTENT_BYTES,
            std::slice::from_ref(&self.parent),
        )?;
        super::Evaluator::new(&self.context.open_store()?, &crate::metrics::Current)
            .direct(&recipe, execute)
    }

    /// Evaluate one exact child under this chain and additional current parents.
    pub fn direct_with_parents<K: Value>(
        &self,
        semantic_inputs: &[u8],
        additional: &[Parent],
        execute: impl FnOnce() -> Result<K, super::Error>,
    ) -> Result<Validated<K>, super::Error> {
        let mut parents = Vec::new();
        parents
            .try_reserve_exact(additional.len().saturating_add(1))
            .map_err(|_| super::Error::Allocation)?;
        parents.push(self.parent.clone());
        parents.extend_from_slice(additional);
        let recipe = self.context.direct_with_parents::<K>(
            semantic_inputs,
            self.context.limits().node_output_bytes(),
            &parents,
        )?;
        super::Evaluator::new(&self.context.open_store()?, &crate::metrics::Current)
            .direct(&recipe, execute)
    }

    /// Erase a newly validated child value into the next opaque chain link.
    #[must_use]
    pub fn child<K: Value>(&self, child: &Validated<K>) -> Self {
        Self::validated(self.context.clone(), child)
    }

    /// Borrow this link as an additional exact parent for a join node.
    #[must_use]
    pub fn parent(&self) -> Parent {
        self.parent.clone()
    }

    /// Seal the exact current transitive closure for later pinned admission.
    #[cfg(test)]
    pub(crate) fn prepare_read_set(&self) -> Result<super::read_set::Prepared, super::Error> {
        super::read_set::Prepared::seal(self.context.clone(), self.parent.read_set.clone())
    }

    pub(crate) const fn output_limit(&self) -> usize {
        self.context.limits().node_output_bytes()
    }

    pub(crate) const fn context(&self) -> &crate::context::Context {
        &self.context
    }

    /// Mint one deterministic test-only parent without process or product authority.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    pub fn fixture(context: crate::context::Context) -> Result<Self, super::Error> {
        let recipe = context.direct::<FixtureParent>(
            b"rawr.test.fixture-parent/v1",
            context.limits().node_output_bytes(),
        )?;
        let parent = super::Evaluator::new(&context.open_store()?, &crate::metrics::Current)
            .direct(&recipe, || Ok(FixtureParent))?;
        Ok(Self::validated(context, &parent))
    }
}

#[cfg(feature = "test-fixtures")]
struct FixtureParent;

#[cfg(feature = "test-fixtures")]
impl Value for FixtureParent {
    const KIND: Kind = Kind::ProfileProof;
    const SCHEMA: u32 = u32::MAX;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, super::Error> {
        const BYTES: &[u8] = b"rawr.test.fixture-parent/v1";
        if BYTES.len() > limit {
            return Err(super::Error::Limit {
                resource: "fixture parent bytes",
                limit,
                actual: BYTES.len(),
            });
        }
        Ok(Box::from(BYTES))
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, super::Error> {
        if bytes.len() > limit || bytes != b"rawr.test.fixture-parent/v1" {
            return Err(super::Error::Codec("fixture parent bytes changed"));
        }
        Ok(Self)
    }
}

/// Launch-local proof that current recipe validation admitted one output.
pub struct Receipt<K> {
    action: ActionKey,
    recipe: Digest,
    content: ContentRef<K>,
    read_set: Fragment,
}

impl<K> Receipt<K> {
    pub(crate) const fn new(
        action: ActionKey,
        recipe: Digest,
        content: ContentRef<K>,
        read_set: Fragment,
    ) -> Self {
        Self {
            action,
            recipe,
            content,
            read_set,
        }
    }

    /// Current action rebound by this non-serializable receipt.
    #[must_use]
    pub const fn action(&self) -> ActionKey {
        self.action
    }

    /// Current semantic-recipe identity rebound by this receipt.
    #[must_use]
    pub const fn recipe_digest(&self) -> Digest {
        self.recipe
    }

    /// Semantic output identity rebound by this current receipt.
    #[must_use]
    pub const fn content_digest(&self) -> Digest {
        self.content.digest
    }

    /// Current transitive recipe union carried toward final freshness proof.
    #[must_use]
    pub const fn read_set(&self) -> &Fragment {
        &self.read_set
    }
}

/// Typed value plus current receipt; the only output accepted from evaluation.
pub struct Validated<K> {
    value: Arc<K>,
    content: ContentRef<K>,
    receipt: Receipt<K>,
}

impl<K> Validated<K> {
    pub(crate) const fn new(value: Arc<K>, content: ContentRef<K>, receipt: Receipt<K>) -> Self {
        Self {
            value,
            content,
            receipt,
        }
    }

    /// Borrow the validated typed output.
    #[must_use]
    pub fn value(&self) -> &K {
        self.value.as_ref()
    }

    /// Borrow its typed semantic content identity.
    #[must_use]
    pub const fn content(&self) -> ContentRef<K> {
        self.content
    }

    /// Borrow the launch-local current-recipe receipt.
    #[must_use]
    pub const fn receipt(&self) -> &Receipt<K> {
        &self.receipt
    }
}
