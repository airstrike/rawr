//! One fragment read against the catalog on its own. Reading a fragment
//! touches nothing another fragment's reading does, so fragments read across
//! cores and join the candidate in their own order.

use std::collections::BTreeSet;

use crate::api::{self, identity, ledger, model};
use crate::cargo;
use crate::{Error, Result};

use super::{conflict, protocol, Allocation, Catalog, Export, Rejected, ShapeFailure};

/// A fragment's records in the order a loop over the fragment meets them.
/// When reading fails, the records stop where that loop would have stopped
/// and the failure follows them, so joining the reads in fragment order
/// fails with the error the loop gives.
pub(super) struct Read {
    pub(super) index: Vec<ledger::Index>,
    pub(super) invocation: Option<Invocation>,
    pub(super) definitions: Vec<Definition>,
    pub(super) reached: Vec<Reached>,
    pub(super) implementations: Vec<Implementation>,
    pub(super) proofs: Vec<model::ScalarPredicateProof>,
    /// The fragment's reachable paths, implementations and proofs, each
    /// counted as it is met.
    pub(super) edges: usize,
    pub(super) failure: Option<Error>,
}

/// The fragment's own invocation and the unit a conflict report names.
pub(super) struct Invocation {
    pub(super) invocation: api::Invocation,
    pub(super) source: conflict::Source,
}

/// A definition whose crate has authority.
#[expect(
    clippy::large_enum_variant,
    reason = "nearly every definition is modeled; boxing it would allocate once more per definition"
)]
pub(super) enum Definition {
    Modeled {
        id: identity::Definition,
        /// The fragment's own crate defines it.
        local: bool,
        definition: model::Definition,
    },
    Rejected {
        id: identity::Definition,
        outcome: Rejected,
    },
}

/// A reachable path and how it binds.
pub(super) enum Reached {
    Nameable(Export),
    AnonymousImport(Export),
}

/// An implementation whose crate has authority.
#[expect(
    clippy::large_enum_variant,
    reason = "nearly every implementation is modeled; boxing it would allocate once more per implementation"
)]
pub(super) enum Implementation {
    Modeled(model::Implementation),
    /// Its shape is one the extractor cannot model.
    Unmodeled(identity::Definition),
}

impl Read {
    /// Reads `fragment`, charging `allocation` for what reading it owns.
    /// The records' own lists are not charged: the candidate charges each
    /// record as it joins.
    pub(super) fn new(
        fragment: protocol::Fragment,
        catalog: &Catalog,
        limits: &cargo::Limits,
        allocation: &mut Allocation,
    ) -> Self {
        let mut read = Self {
            index: Vec::new(),
            invocation: None,
            definitions: Vec::new(),
            reached: Vec::new(),
            implementations: Vec::new(),
            proofs: Vec::new(),
            edges: 0,
            failure: None,
        };
        if let Err(error) = read.fill(fragment, catalog, limits, allocation) {
            read.failure = Some(error);
        }
        read
    }

    fn fill(
        &mut self,
        mut fragment: protocol::Fragment,
        catalog: &Catalog,
        limits: &cargo::Limits,
        allocation: &mut Allocation,
    ) -> Result<()> {
        let protocol::Root::Selected { aliases } = &fragment.handshake.root;
        let mut root_aliases = BTreeSet::new();
        for alias in aliases {
            allocation.items(1)?;
            root_aliases.insert(alias.as_str());
        }
        if !fragment.index.is_empty() {
            reserve(&mut self.index, root_aliases.len(), allocation)?;
            for alias in &root_aliases {
                allocation.items(fragment.index.len())?;
                self.index.push(ledger::Index::new(
                    (*alias).into(),
                    fragment.index.clone().into_boxed_slice(),
                ));
            }
        }
        let mut fragment_definitions = BTreeSet::new();
        for raw in &fragment.definitions {
            allocation.items(1)?;
            if !fragment_definitions.insert(&raw.id) {
                return Err(super::extraction(
                    "duplicate-definition",
                    &fragment.handshake.unit_correlation,
                ));
            }
        }
        self.invocation = Some(Invocation {
            invocation: super::invocation(&fragment, allocation)?,
            source: conflict::Source::new(&fragment.handshake),
        });
        let definitions = std::mem::take(&mut fragment.definitions);
        let reachable = std::mem::take(&mut fragment.reachable);
        let implementations = std::mem::take(&mut fragment.implementations);
        let scalar_predicate_proofs = std::mem::take(&mut fragment.scalar_predicate_proofs);
        let handshake = &fragment.handshake;
        reserve(&mut self.definitions, definitions.len(), allocation)?;
        for raw in definitions {
            if let Some(definition) = read_definition(raw, handshake, catalog, limits, allocation)?
            {
                self.definitions.push(definition);
            }
        }
        reserve(&mut self.reached, reachable.len(), allocation)?;
        for raw in reachable {
            self.edges = self.edges.saturating_add(1);
            if !root_aliases.contains(raw.root_alias.as_str()) {
                return Err(super::extraction("mismatched-root-alias", &raw.root_alias));
            }
            let binding = raw.binding;
            let export = super::export(raw, catalog, allocation)?;
            self.reached.push(match binding {
                protocol::Binding::Nameable => Reached::Nameable(export),
                protocol::Binding::AnonymousImport => Reached::AnonymousImport(export),
            });
        }
        reserve(&mut self.implementations, implementations.len(), allocation)?;
        for raw in implementations {
            self.edges = self.edges.saturating_add(1);
            if let Some(implementation) = read_implementation(raw, catalog, limits, allocation)? {
                self.implementations.push(implementation);
            }
        }
        reserve(&mut self.proofs, scalar_predicate_proofs.len(), allocation)?;
        for raw in scalar_predicate_proofs {
            self.edges = self.edges.saturating_add(1);
            self.proofs
                .push(super::scalar_predicate_proof(raw, catalog, allocation)?);
        }
        Ok(())
    }
}

/// `None` for a definition of a crate with no authority, which joins
/// nothing.
fn read_definition(
    raw: protocol::Definition,
    handshake: &protocol::Handshake,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Option<Definition>> {
    let local = super::is_local_id(&raw.id, handshake);
    let sysroot_shape = matches!(&raw.shape, protocol::Shape::Sysroot { .. });
    if sysroot_shape
        && (local
            || !catalog.is_sysroot_id(&raw.id)
            || !super::sysroot_owner_matches_kind(&raw.owner, &raw.shape)
            || !matches!(&raw.visibility, protocol::Visibility::Public))
    {
        return Err(super::extraction(
            "mismatched-local-definition",
            &handshake.unit_correlation,
        ));
    }
    if catalog.unavailable_identity(&raw.id, allocation)?.is_some() {
        super::validate_unavailable_definition(&raw, catalog)?;
        return Ok(None);
    }
    allocation.protocol_id(&raw.id)?;
    allocation.cloned_ownership(1);
    let id = catalog.id_bounded(raw.id.clone(), allocation)?;
    allocation.protocol_span(&raw.span)?;
    allocation.cloned_ownership(1);
    let span = super::span(raw.span.clone(), allocation)?;
    let reason = match super::definition(raw, catalog, limits, allocation) {
        Ok(definition) => {
            return Ok(Some(Definition::Modeled {
                id,
                local,
                definition,
            }));
        }
        Err(ShapeFailure::Unknown(kind)) => ledger::Reason::UnknownCompilerShape {
            definition: allocation.clone_identity(&id)?,
            compiler_kind: kind,
        },
        Err(ShapeFailure::UnnameablePrivate) => {
            ledger::Reason::UnnameablePrivate(allocation.clone_identity(&id)?)
        }
        Err(ShapeFailure::ExternalAuthorityUnavailable {
            crate_name,
            stable_crate_id,
        }) => ledger::Reason::ExternalAuthorityUnavailable {
            crate_name,
            stable_crate_id,
        },
        Err(ShapeFailure::Fatal(error)) => return Err(error),
    };
    Ok(Some(Definition::Rejected {
        id,
        outcome: Rejected { reason, span },
    }))
}

/// `None` for an implementation of a crate with no authority, which is
/// unavailable by itself: its types and trait stay.
fn read_implementation(
    raw: protocol::Implementation,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Option<Implementation>> {
    if catalog.unavailable_identity(&raw.id, allocation)?.is_some() {
        super::validate_unavailable_implementation(&raw, catalog)?;
        allocation.protocol_span(&raw.span)?;
        return Ok(None);
    }
    // The relevance and span are read for their checks and charges; the
    // implementation reads its own.
    allocation.temporary_conversions(raw.relevance.len());
    for anchor in &raw.relevance {
        if catalog.unavailable_identity(anchor, allocation)?.is_none() {
            allocation.protocol_id(anchor)?;
            allocation.cloned_ownership(1);
            catalog.id_bounded(anchor.clone(), allocation)?;
            allocation.items(1)?;
        }
    }
    allocation.protocol_span(&raw.span)?;
    allocation.cloned_ownership(1);
    super::span(raw.span.clone(), allocation)?;
    // An implementation the extractor cannot model is unavailable by
    // itself. Its self type, its trait and every other implementation of
    // either stay available: one `impl Debug for f16` with an unknown
    // constant must not take `Debug` and every type that implements it
    // with it. Its crate's inventory lists it as unsupported, so no proof
    // reads its trait's domain as complete.
    let id = raw.id.clone();
    match super::implementation(raw, catalog, limits, allocation) {
        Ok(implementation) => Ok(Some(Implementation::Modeled(implementation))),
        Err(
            ShapeFailure::ExternalAuthorityUnavailable { .. }
            | ShapeFailure::Unknown(_)
            | ShapeFailure::UnnameablePrivate,
        ) => {
            allocation.items(1)?;
            Ok(Some(Implementation::Unmodeled(
                catalog.id_bounded(id, allocation)?,
            )))
        }
        Err(ShapeFailure::Fatal(error)) => Err(error),
    }
}

/// Room for `amount` more records, which the candidate charges as each
/// joins.
fn reserve<T>(values: &mut Vec<T>, amount: usize, allocation: &mut Allocation) -> Result<()> {
    values
        .try_reserve_exact(amount)
        .map_err(|error| allocation.allocation_failure("fragment read allocation", error))
}
