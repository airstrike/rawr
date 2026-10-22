//! Complete input comparison against independently associated owner observations.

use std::path::Path;

use super::observe::slots::Slots;
use super::{build, entries, prefix, Budget, Error, Result, Stored};
use crate::{cargo, extract, input, node};

/// Complete compared inputs, bound to the same Build and target used for coverage.
/// This is not current owner authority; projection and owner joins still follow.
pub(crate) struct Compared<'owner, 'budget, 'account> {
    budget: &'budget mut Budget<'account>,
    stored: &'owner Stored,
    entries: Box<[input::proof::Entry]>,
    build: &'owner input::build::Retained,
    target: &'owner str,
    native: input::native::Inputs,
}

/// Fresh protected facts derived only after the complete input comparison.
pub(crate) struct Replayed<'budget, 'account> {
    budget: &'budget mut Budget<'account>,
    projection: cargo::universe::BuildProjection,
    schema: u32,
    entries: Box<[input::proof::Entry]>,
    fingerprint: input::fingerprint::Fingerprint,
    native: input::native::Inputs,
}

/// Compared input storage retained after successful universe completion.
/// This is neither a current graph receipt nor permission to publish. A later
/// owner must join bundle/read-set admission before consuming this evidence.
pub(crate) struct Completed {
    schema: u32,
    entries: Box<[input::proof::Entry]>,
    fingerprint: input::fingerprint::Fingerprint,
    native: input::native::Inputs,
    graph: node::GraphBudget,
    complete: [u8; 32],
}

impl Completed {
    /// Borrow owned identity only; this does not grant publication authority.
    pub(crate) const fn fingerprint(&self) -> input::fingerprint::Fingerprint {
        self.fingerprint
    }

    /// Consume compared inputs only beside the matching current owner receipt.
    /// The resulting graph must retain that owner's read-set through admission.
    pub(crate) fn publish(
        self,
        owner: &node::Validated<crate::cargo::resolution::owner::Product>,
        graph: &node::GraphBudget,
    ) -> crate::Result<input::proof::Proof> {
        self.require_account(graph)
            .map_err(|_| crate::Error::Input {
                code: "completed-input-account",
                reason: "completed input graph account differs".into(),
            })?;
        if &self.complete != owner.value().completed_identity() {
            return Err(crate::Error::Input {
                code: "completed-input-owner",
                reason: "completed input owner identity differs".into(),
            });
        }
        Ok(input::proof::Proof::from_replayed_parts(
            self.schema,
            self.entries,
            self.fingerprint,
            self.native,
        ))
    }

    /// Check account identity only; sharing an account does not refresh evidence.
    pub(crate) fn require_account(&self, graph: &node::GraphBudget) -> Result<()> {
        if !self.graph.shares_with(graph) {
            return Err(Error::Encoding("completed input graph account differs"));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn observed_parts(
        &self,
    ) -> (u32, &[input::proof::Entry], input::fingerprint::Fingerprint) {
        (self.schema, &self.entries, self.fingerprint)
    }
}

impl Replayed<'_, '_> {
    pub(crate) fn finish(
        self,
        complete: impl FnOnce(
            &cargo::universe::BuildProjection,
            input::fingerprint::Fingerprint,
            &mut dyn FnMut(usize) -> Result<()>,
        ) -> Result<cargo::universe::Completion>,
    ) -> Result<(cargo::universe::Completion, Completed)> {
        self.budget.check()?;
        let mut admit = |bytes| self.budget.hash(bytes);
        let completion = complete(&self.projection, self.fingerprint, &mut admit)?;
        self.budget.check()?;
        let input = Completed {
            schema: self.schema,
            entries: self.entries,
            fingerprint: self.fingerprint,
            native: self.native,
            graph: self.budget.graph().clone(),
            complete: *completion.proof().complete().as_bytes(),
        };
        Ok((completion, input))
    }
}

impl<'budget, 'account> Compared<'_, 'budget, 'account> {
    pub(crate) fn protect(
        self,
        protected: &cargo::universe::MetadataProof,
    ) -> Result<Replayed<'budget, 'account>> {
        let budget = self.budget;
        budget.check()?;
        let limits = budget.limits;
        let projection = input::proof::protected::project::current(
            &self.entries,
            &self.build.build().evidence,
            protected,
            self.target,
            &limits,
            budget,
        )?;
        let retained = self
            .stored
            .projection
            .as_ref()
            .ok_or(Error::Encoding("stored protected projection is absent"))?;
        if projection.bytes() != retained.bytes()
            || projection.digest().as_bytes() != retained.digest()
        {
            return Err(Error::Encoding(
                "fresh protected projection differs from stored inputs",
            ));
        }
        Ok(Replayed {
            budget,
            projection,
            schema: self.stored.schema,
            entries: self.entries,
            fingerprint: self.stored.fingerprint,
            native: self.native,
        })
    }
}

impl Stored {
    /// Compare every recipe and entry under one live account. This does not
    /// attest the supplied owners or confer a current graph-node receipt.
    #[allow(
        clippy::too_many_arguments,
        reason = "complete comparison borrows the associated owners without copying them"
    )]
    pub(crate) fn require_inputs<'owner, 'budget, 'account>(
        &'owner self,
        config: &cargo::config::Provenance,
        authority: &[cargo::Input],
        probe: &extract::plan::Capture,
        harness: [&Path; 3],
        toolchain: &cargo::toolchain::Proof,
        internal: &cargo::internal::Recipe,
        target: &'owner str,
        profile: &str,
        retained: &'owner input::build::Retained,
        metadata: &cargo::unit::Metadata,
        cargo_env_overrides: bool,
        cargo_home: Option<&Path>,
        base: &Path,
        graph: &cargo::unit::BuildGraph,
        command_build: &Path,
        budget: &'budget mut Budget<'account>,
    ) -> Result<Compared<'owner, 'budget, 'account>> {
        input::canonical::pass(|| {
            self.require_inputs_in(
                config,
                authority,
                probe,
                harness,
                toolchain,
                internal,
                target,
                profile,
                retained,
                metadata,
                cargo_env_overrides,
                cargo_home,
                base,
                graph,
                command_build,
                budget,
            )
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "complete comparison borrows the associated owners without copying them"
    )]
    fn require_inputs_in<'owner, 'budget, 'account>(
        &'owner self,
        config: &cargo::config::Provenance,
        authority: &[cargo::Input],
        probe: &extract::plan::Capture,
        harness: [&Path; 3],
        toolchain: &cargo::toolchain::Proof,
        internal: &cargo::internal::Recipe,
        target: &'owner str,
        profile: &str,
        retained: &'owner input::build::Retained,
        metadata: &cargo::unit::Metadata,
        cargo_env_overrides: bool,
        cargo_home: Option<&Path>,
        base: &Path,
        graph: &cargo::unit::BuildGraph,
        command_build: &Path,
        budget: &'budget mut Budget<'account>,
    ) -> Result<Compared<'owner, 'budget, 'account>> {
        budget.check()?;
        let mut native = input::native::Projection::bind(
            &retained.build().evidence,
            harness,
            graph,
            command_build,
            target,
            budget,
        )?;
        let mut recipes = self.recipes.iter();
        let mut observed = Slots::new();
        prefix::require(
            &mut recipes,
            &mut observed,
            config,
            authority,
            probe,
            harness,
            toolchain,
            internal,
            target,
            profile,
            retained,
            metadata,
            base,
            budget,
        )?;
        build::require(
            &mut recipes,
            &mut observed,
            retained,
            cargo_env_overrides,
            metadata,
            cargo_home,
            base,
            |index, dependency, budget| native.observe(index, dependency, budget),
            budget,
        )?;
        let observed = observed.unique(budget)?;
        if observed != self.entries {
            return Err(Error::Encoding("complete current input entries differ"));
        }
        if entries::fingerprint(self.schema, &observed, budget)? != self.fingerprint {
            return Err(Error::Encoding(
                "complete current input fingerprint differs",
            ));
        }
        let native = native.finish(&observed, budget)?;
        Ok(Compared {
            budget,
            stored: self,
            entries: observed,
            build: retained,
            target,
            native,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_refusal_cannot_return_retained_inputs() {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .unwrap();
        let mut allocation =
            input::Allocation::for_proof(input::Usage { items: 0, bytes: 0 }, &limits);
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let replayed = Replayed {
            budget: &mut budget,
            projection: cargo::universe::BuildProjection::from_canonical(Box::default()),
            schema: input::proof::SCHEMA,
            entries: Box::default(),
            fingerprint: input::fingerprint::Fingerprint::from_bytes([0; 32]),
            native: input::native::Inputs::fixture([0; 32]),
        };
        let mut entered = false;
        let result = replayed.finish(|_, _, admit| {
            entered = true;
            // One byte over the hashed target is admitted and recorded.
            admit(1).expect("an exceeding digest charge is recorded, not refused");
            // A genuine failure (checked-arithmetic overflow) is terminal.
            admit(usize::MAX)?;
            panic!("exhausted completion must return before minting authority")
        });
        assert!(entered);
        assert!(matches!(result, Err(Error::Graph(node::Error::Allocation))));
        let record = graph.exceedances();
        let axis = record
            .axis(node::HASHED_RESOURCE)
            .expect("hashed axis recorded");
        assert_eq!(axis.target, node::MAX_HASHED_BYTES_PER_GRAPH);
        assert_eq!(axis.high_water, node::MAX_HASHED_BYTES_PER_GRAPH + 1);
        assert!(matches!(budget.check(), Err(Error::Refused)));
        let replayed = Replayed {
            budget: &mut budget,
            projection: cargo::universe::BuildProjection::from_canonical(Box::default()),
            schema: input::proof::SCHEMA,
            entries: Box::default(),
            fingerprint: input::fingerprint::Fingerprint::from_bytes([0; 32]),
            native: input::native::Inputs::fixture([0; 32]),
        };
        assert!(matches!(
            replayed.finish(|_, _, _| panic!("a refused account cannot enter completion")),
            Err(Error::Refused)
        ));
    }
}
