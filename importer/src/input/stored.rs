//! Canonical cold input data without current source or execution authority.

use super::{fingerprint, proof, Allocation, Baseline, Usage};
use crate::{cargo, node};

mod budget;
mod build;
mod codec;
mod entries;
pub(crate) mod observe;
mod prefix;
mod projection;
mod recipe;
mod record;
#[cfg(feature = "rustc-driver")]
mod replay;
pub(super) mod scalar;
pub(super) mod view;

pub(crate) use budget::Budget;
pub(crate) use projection::Projection;
pub(crate) use recipe::{Package, Recipe};
#[cfg(feature = "rustc-driver")]
pub(crate) use record::Commitment;
pub(crate) use record::Record;
#[cfg(feature = "rustc-driver")]
pub(crate) use replay::{Compared, Completed};

#[cfg(test)]
mod tests;

const MAGIC: &[u8] = b"rawr-input-baseline\0\x01";

/// Historical cold evidence. Decoding does not make the observations current.
pub(crate) struct Stored {
    schema: u32,
    usage: Usage,
    entries: Box<[proof::Entry]>,
    fingerprint: fingerprint::Fingerprint,
    recipes: Vec<Recipe>,
    projection: Option<Projection>,
}

/// The consuming capture outcome and its original allocation account.
///
/// Failure retains spent work and terminal refusal just as success does. This
/// account has no graph identity: each operation borrows the current owner's
/// graph budget independently of the historical record.
pub(crate) struct Capture {
    result: Result<Stored>,
    allocation: Allocation,
    #[cfg(test)]
    allocations: usize,
}

impl Capture {
    pub(crate) fn into_parts(self) -> (Result<Stored>, Allocation) {
        (self.result, self.allocation)
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Input(#[from] crate::Error),
    #[error(transparent)]
    Graph(#[from] crate::node::Error),
    #[error("resolution bundle: {0}")]
    Bundle(crate::node::Error),
    #[error(transparent)]
    Observation(#[from] observe::Error),
    #[error("stored baseline allocation failed")]
    Allocation(#[from] std::collections::TryReserveError),
    #[error("invalid stored baseline: {0}")]
    Encoding(&'static str),
    #[error("complete package membership witness is unavailable")]
    IncompleteWitness,
    #[error("stored baseline size calculation overflowed")]
    Overflow,
    #[error("stored baseline account has already refused work")]
    Refused,
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

impl Stored {
    /// Move the original cold data, refusing packages without a complete witness.
    pub(crate) fn capture(
        baseline: Baseline,
        limits: &cargo::Limits,
        graph: &node::GraphBudget,
    ) -> Capture {
        let Baseline {
            proof,
            recipes,
            recipe_allocation,
            mut allocation,
            universe_build,
            package_replay,
        } = baseline;
        // Execution ownership is not serializable source evidence.
        drop(package_replay);
        let mut budget = Budget::borrow(limits, &mut allocation, graph);
        let result = Self::retain(
            proof,
            recipes,
            recipe_allocation,
            universe_build,
            &mut budget,
        );
        #[cfg(test)]
        let allocations = budget.allocations();
        Capture {
            result,
            allocation,
            #[cfg(test)]
            allocations,
        }
    }

    fn retain(
        proof: proof::Proof,
        recipes: Box<[proof::Recipe]>,
        recipe_allocation: Usage,
        universe_build: Option<cargo::universe::BuildProjection>,
        budget: &mut Budget<'_>,
    ) -> Result<Self> {
        budget.check()?;
        for recipe in &recipes {
            if matches!(
                recipe,
                proof::Recipe::Package(super::package::Set::Path(_) | super::package::Set::Git(_))
            ) {
                return Err(Error::IncompleteWitness);
            }
        }
        let mut retained = budget.items::<Recipe>(recipes.len())?;
        for recipe in recipes {
            retained.push(Recipe::capture(recipe)?);
        }
        let (schema, entries, fingerprint) = proof.into_stored_parts();
        Ok(Self {
            schema,
            usage: recipe_allocation,
            entries,
            fingerprint,
            recipes: retained,
            projection: universe_build.map(Projection::capture),
        })
    }

    /// Decode completely under the caller's existing ownership and work account.
    pub(crate) fn decode(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Self> {
        budget.check()?;
        budget.wire_section(bytes)?;
        let mut reader = codec::Reader::new(bytes, budget);
        let (schema, usage) = reader.header()?;
        let entries = reader.entries()?;
        let fingerprint = fingerprint::Fingerprint::from_bytes(reader.digest()?);
        let count = reader.count()?;
        if count > reader.remaining() {
            return Err(Error::Encoding("impossible recipe count"));
        }
        let mut recipes = reader.budget.items::<Recipe>(count)?;
        for _ in 0..count {
            recipes.push(reader.recipe()?);
        }
        let projection = reader.projection()?;
        reader.finish()?;
        let actual = entries::fingerprint(schema, &entries, budget)?;
        if actual != fingerprint {
            return Err(Error::Encoding("cold proof fingerprint differs"));
        }
        Ok(Self {
            schema,
            usage,
            entries,
            fingerprint,
            recipes,
            projection,
        })
    }

    /// Produce one exact canonical record; no retained account is refunded.
    #[cfg(test)]
    pub(crate) fn encode(&self, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
        codec::encode(budget, |writer| self.write(writer))
    }

    fn write(&self, writer: &mut codec::Writer<'_>) -> Result<()> {
        writer.header(self.schema, self.usage)?;
        writer.entries(&self.entries)?;
        writer.raw(self.fingerprint.as_bytes())?;
        writer.count(self.recipes.len())?;
        for recipe in &self.recipes {
            writer.recipe(recipe)?;
        }
        writer.projection(self.projection.as_ref())
    }

    #[cfg(test)]
    pub(crate) const fn schema(&self) -> u32 {
        self.schema
    }
    #[cfg(test)]
    pub(crate) const fn usage(&self) -> Usage {
        self.usage
    }
    #[cfg(test)]
    pub(crate) const fn fingerprint(&self) -> fingerprint::Fingerprint {
        self.fingerprint
    }
    pub(crate) fn entries(&self) -> &[proof::Entry] {
        &self.entries
    }
    pub(crate) fn recipes(&self) -> &[Recipe] {
        &self.recipes
    }
    #[cfg(test)]
    pub(crate) fn projection(&self) -> Option<&Projection> {
        self.projection.as_ref()
    }
}
