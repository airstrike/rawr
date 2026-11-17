//! The order a stored API writes its ledgers in.
//!
//! A graph keeps its definitions, implementations and support edges in
//! identity order, and an identity leads with rustc's definition-path hash,
//! so the entries one demand round adds fall all through a ledger in that
//! order. The store cuts a value into content-defined parts
//! (`store::parts`), so every entry a round adds would rewrite a part of
//! its own. A stored API writes each of these ledgers in source order
//! instead: an item by its span, a support edge by the span of the item
//! that required it, each then by identity. A round adds the items of the
//! modules its demand newly reaches, which sit together in their files, so
//! they land in a few parts and every other part keeps its bytes. Reading a
//! stored API puts each ledger back in identity order ([`restore`]).

use std::collections::BTreeMap;

use super::identity;
use super::ledger;
use super::model;

/// Put `definitions` in source order.
pub(crate) fn definitions(definitions: &mut [&model::Definition]) {
    definitions
        .sort_unstable_by(|left, right| (left.span(), left.id()).cmp(&(right.span(), right.id())));
}

/// Put `implementations` in source order.
pub(crate) fn implementations(implementations: &mut [&model::Implementation]) {
    implementations
        .sort_unstable_by(|left, right| (left.span(), left.id()).cmp(&(right.span(), right.id())));
}

/// Put `support` in the source order of each edge's provenance, the
/// definition or implementation among `definitions` and `implementations`
/// whose shape required it.
pub(crate) fn support(
    support: &mut [&ledger::Support],
    definitions: &[model::Definition],
    implementations: &[model::Implementation],
) {
    let spans: BTreeMap<&identity::Definition, &model::Span> = definitions
        .iter()
        .map(|definition| (definition.id(), definition.span()))
        .chain(
            implementations
                .iter()
                .map(|implementation| (implementation.id(), implementation.span())),
        )
        .collect();
    support.sort_unstable_by(|left, right| {
        let left = (
            spans.get(left.provenance()),
            left.provenance(),
            left.id(),
            left.role(),
        );
        let right = (
            spans.get(right.provenance()),
            right.provenance(),
            right.id(),
            right.role(),
        );
        left.cmp(&right)
    });
}

/// Put a stored API's ledgers back in the identity order a graph keeps.
pub(crate) fn restore(
    definitions: &mut [model::Definition],
    implementations: &mut [model::Implementation],
    support: &mut [ledger::Support],
) {
    definitions.sort_unstable_by(|left, right| left.id().cmp(right.id()));
    implementations.sort_unstable_by(|left, right| left.id().cmp(right.id()));
    support.sort_unstable_by(|left, right| {
        (left.id(), left.provenance(), left.role()).cmp(&(
            right.id(),
            right.provenance(),
            right.role(),
        ))
    });
}
