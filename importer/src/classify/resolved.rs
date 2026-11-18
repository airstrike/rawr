//! What a facade reads of an outcome, by digest (#1618, #2221). An outcome
//! with each plan it names in place is all a facade generates from, so a
//! generation of the same digest is the one kept.
//!
//! The digest is of the outcome's canonical bytes and of each plan it
//! names, at any depth, in the order the outcome names them. A
//! classification has both as each item's fragment, kept with its record
//! (`items`), so it reads the digest of an outcome it took whole from what
//! it took, encoding nothing again.

use sha2::{Digest as _, Sha256};

use super::items::Fragment;
use super::premise::Digest;
use super::{layout, DefinitionPlan, Error, OutcomeEvidence, OutcomeRecord};

const DOMAIN: &[u8] = b"rawr.classified.resolved/v1\0";

/// The digest of `outcome`, whose canonical bytes are `fragment`, with the
/// digest `plan` gives of each plan it names, by its index.
pub(super) fn digest(
    outcome: &OutcomeRecord,
    fragment: &Fragment,
    plan: impl Fn(u32) -> Option<Digest>,
) -> Digest {
    let mut digest = Sha256::new();
    digest.update(DOMAIN);
    digest.update(fragment.digest);
    named(outcome, &mut |index| {
        if let Some(plan) = plan(index) {
            digest.update(plan);
        }
    });
    digest.finalize().into()
}

/// The digest of each of `outcomes`, `graph`'s classified with `plans`,
/// when the items' fragments aren't at hand: each outcome, and each plan
/// one names, encoded once.
pub(super) fn digests<P: crate::api::Phase>(
    graph: &crate::api::Graph<P>,
    outcomes: &[OutcomeRecord],
    plans: &[DefinitionPlan],
) -> Result<std::sync::Arc<[Digest]>, Error> {
    let implementations = layout::Implementations::of(graph)?;
    let mut fragments = vec![None; plans.len()];
    let mut named_plans = Vec::new();
    for outcome in outcomes {
        named(outcome, &mut |index| named_plans.push(index as usize));
    }
    for index in named_plans {
        if let (Some(slot @ None), Some(plan)) = (fragments.get_mut(index), plans.get(index)) {
            *slot = Some(Fragment::of(plan, Vec::new())?.digest);
        }
    }
    outcomes
        .iter()
        .map(|outcome| {
            let fragment = Fragment::of(
                &super::Laid {
                    records: outcome,
                    implementations: &implementations,
                },
                Vec::new(),
            )?;
            Ok(digest(outcome, &fragment, |index| {
                fragments.get(index as usize).copied().flatten()
            }))
        })
        .collect()
}

/// Give `plan` the index of each plan `outcome` names, its own first, then
/// its associated outcomes' in order.
fn named(outcome: &OutcomeRecord, plan: &mut impl FnMut(u32)) {
    if let OutcomeEvidence::Plan { index, .. } = &outcome.evidence {
        plan(*index);
    }
    for implementation in &outcome.implementations {
        for associated in &implementation.associated {
            named(associated, plan);
        }
    }
}
