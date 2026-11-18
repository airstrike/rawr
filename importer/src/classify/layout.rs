//! The order a stored classification writes its definition plans, support
//! edges and implementation ledgers in.
//!
//! A classification keeps its definition plans in semantic-digest order, its
//! support edges in the API's identity order, which leads with rustc's
//! definition-path hash, and each outcome's implementation ledgers in graph
//! order, which is identity order too. So the plans, edges and ledgers one
//! demand round adds fall all through the value. The store cuts a value
//! into content-defined parts (`store::parts`), so each of them would
//! rewrite a part of its own. The stored classification writes them in
//! source order instead, as a stored API writes its ledgers
//! (`api::layout`): a plan by its definition's span, a support edge by the
//! span of the item that required it, an implementation ledger by its
//! implementation's span, each then by identity. A round adds the items of
//! the modules its demand newly reaches, which sit together in their files,
//! so they land in a few parts and every other part keeps its bytes.
//! Reading a stored classification puts each back in the order the
//! classification keeps.

use crate::api::{self, ledger, model};
use crate::surface::identity;

use super::{DefinitionPlan, Error, ImplementationRecord, OutcomeRecord};

/// A permutation of a ledger's indices: the order the store writes the
/// ledger in.
pub(super) struct Order(Box<[u32]>);

impl Order {
    /// The ledger's indices, in the order the store writes them.
    pub(super) fn indices(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        self.0.iter().map(|&index| index as usize)
    }

    /// The indices of `len` entries, sorted by `key`.
    fn sorted<K: Ord>(len: usize, key: impl Fn(usize) -> K) -> Result<Self, Error> {
        let len = u32::try_from(len).map_err(|_| Error::IndexOverflow)?;
        let mut indices = Vec::new();
        indices
            .try_reserve_exact(len as usize)
            .map_err(|_| Error::Allocation)?;
        indices.extend(0..len);
        indices.sort_unstable_by_key(|&index| key(index as usize));
        Ok(Self(indices.into_boxed_slice()))
    }
}

/// The order of `plans`, `graph`'s classified definitions in digest order,
/// by their definitions' spans.
pub(super) fn definitions<P: api::Phase>(
    graph: &api::Graph<P>,
    plans: &[DefinitionPlan],
) -> Result<Order, Error> {
    let definitions = collect(graph.definitions())?;
    let surface = graph.surface_definitions();
    if plans.len() != definitions.len() || surface.len() != definitions.len() {
        return Err(Error::Cardinality);
    }
    let order = Order::sorted(definitions.len(), |index| {
        (definitions[index].span(), definitions[index].id())
    })?;
    let mut indices = Vec::new();
    indices
        .try_reserve_exact(plans.len())
        .map_err(|_| Error::Allocation)?;
    for index in order.indices() {
        let plan = plans
            .binary_search_by_key(&surface[index], DefinitionPlan::definition)
            .map_err(|_| Error::Cardinality)?;
        indices.push(u32::try_from(plan).map_err(|_| Error::IndexOverflow)?);
    }
    Ok(Order(indices.into_boxed_slice()))
}

/// The order of `graph`'s support edges, by the spans of the definitions
/// and implementations that required them.
pub(super) fn support<P: api::Phase>(graph: &api::Graph<P>) -> Result<Order, Error> {
    let definitions = collect(graph.definitions())?;
    let implementations = collect(graph.implementations())?;
    let support = collect(graph.support())?;
    let mut spans: Vec<Option<&model::Span>> = Vec::new();
    spans
        .try_reserve_exact(support.len())
        .map_err(|_| Error::Allocation)?;
    spans.extend(support.iter().map(|edge| {
        match edge.role() {
            ledger::SupportRole::Definition => graph
                .definition_index(edge.provenance())
                .and_then(|index| definitions.get(index))
                .map(|definition| definition.span()),
            ledger::SupportRole::Implementation => graph
                .implementation_index(edge.provenance())
                .and_then(|index| implementations.get(index))
                .map(|implementation| implementation.span()),
        }
    }));
    Order::sorted(support.len(), |index| {
        let edge = support[index];
        (spans[index], edge.provenance(), edge.id(), edge.role())
    })
}

/// The semantic identity of each of a graph's implementations, with the
/// implementation's index, in identity order: what puts an outcome's
/// implementation ledgers in the order the store writes them, and back.
///
/// An outcome keeps the ledgers of the graph's implementations first, in
/// graph order, the order the graph relates them to the outcome's
/// definition in, and then any other, as the opaque implementations an
/// `impl Trait` output gives, in the order classification made them. The
/// store writes the graph's by span, each then by identity, and the others
/// after them as they are kept.
pub(super) struct Implementations(Box<[(identity::Implementation, u32)]>);

impl Implementations {
    /// The identities of `graph`'s implementations.
    pub(super) fn of<P: api::Phase>(graph: &api::Graph<P>) -> Result<Self, Error> {
        let mut identities = Vec::new();
        identities
            .try_reserve_exact(graph.implementations().len())
            .map_err(|_| Error::Allocation)?;
        for (index, implementation) in graph.surface_implementations().iter().enumerate() {
            let index = u32::try_from(index).map_err(|_| Error::IndexOverflow)?;
            identities.push((*implementation, index));
        }
        identities.sort_unstable();
        Ok(Self(identities.into_boxed_slice()))
    }

    /// The graph index of the implementation `identity` names, if it is one
    /// of the graph's.
    fn index(&self, identity: identity::Implementation) -> Option<u32> {
        self.0
            .binary_search_by_key(&identity, |(identity, _)| *identity)
            .ok()
            .map(|found| self.0[found].1)
    }

    /// `records`, one list of implementation ledgers as the classification
    /// keeps it, in the order the store writes them.
    pub(super) fn order<'r>(
        &self,
        records: &'r [ImplementationRecord],
    ) -> Result<Vec<&'r ImplementationRecord>, Error> {
        let mut ordered = Vec::new();
        ordered
            .try_reserve_exact(records.len())
            .map_err(|_| Error::Allocation)?;
        ordered.extend(records);
        // Stable: the others keep their order.
        ordered.sort_by_cached_key(|&record| {
            self.index(record.identity)
                .map(|_| (&record.span, record.identity))
                .ok_or(())
        });
        Ok(ordered)
    }

    /// Put each list of implementation ledgers of `outcomes` and of their
    /// associated outcomes, as the store wrote it or a classification made
    /// it, in the order the classification keeps.
    pub(super) fn restore(&self, outcomes: &mut [OutcomeRecord]) {
        for outcome in outcomes {
            // Stable: the others keep their order.
            outcome
                .implementations
                .sort_by_cached_key(|record| self.index(record.identity).ok_or(()));
            for implementation in &mut outcome.implementations {
                self.restore(&mut implementation.associated);
            }
        }
    }
}

/// The items of `items`, for indexing.
fn collect<'a, T>(items: impl ExactSizeIterator<Item = &'a T>) -> Result<Vec<&'a T>, Error> {
    let mut collected = Vec::new();
    collected
        .try_reserve_exact(items.len())
        .map_err(|_| Error::Allocation)?;
    collected.extend(items);
    Ok(collected)
}
