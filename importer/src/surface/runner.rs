//! Explicit owned configuration for an audited compiled Runner integration.

use super::{facade, Demand, Error, Request, Runner, Sealed};

pub(crate) struct Inputs {
    compile_snapshot: bool,
    pub(super) facade: Box<[u8]>,
    provenance: Binding,
    entry: crate::node::read_set::Entry,
}

impl<B> Demand<Sealed, B> {
    /// Select immutable Runner inputs for an audited compiled integration.
    ///
    /// This is an explicit semantic trust assertion, not a purity audit. These
    /// values replace Runner callbacks for identity and rendering. Ordinary
    /// demands keep their callbacks; calling them once never grants this route.
    /// The facade must belong to this demand's exact authenticated contract and
    /// graph account. Its additional owned copy is precharged to this demand.
    #[doc(hidden)]
    pub fn with_trusted_runner(
        self,
        compile_snapshot: bool,
        facade: &facade::Cached,
        contract: &crate::Contract<crate::Classified>,
    ) -> Result<Self, Error> {
        let supplied = contract.node_chain().ok_or(Error::ForeignRunner)?;
        self.with_trusted_runner_parts(compile_snapshot, facade, contract.fingerprint(), supplied)
    }

    /// The same selection from the contract identity and its generic node alone.
    pub(crate) fn with_trusted_runner_parts(
        mut self,
        compile_snapshot: bool,
        facade: &facade::Cached,
        contract: crate::contract::fingerprint::Fingerprint,
        supplied: &crate::node::Chain,
    ) -> Result<Self, Error> {
        let original = self.contract_chain.as_ref().ok_or(Error::ForeignRunner)?;
        if self.runner.is_some() || self.contract != contract || !original.matches(supplied) {
            return Err(Error::ForeignRunner);
        }
        facade.require_contract_parts(contract, supplied)?;
        self._allocation.retain(1, facade.bytes().len())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(facade.bytes().len())
            .map_err(|_| Error::Allocation)?;
        bytes.extend_from_slice(facade.bytes());
        self.runner = Some(Inputs {
            compile_snapshot,
            facade: bytes.into_boxed_slice(),
            provenance: Binding::capture(facade.node_chain()),
            entry: facade.entry(),
        });
        Ok(self)
    }
}

impl<B: Runner> Request<B> {
    pub(crate) fn compile_snapshot(&self) -> bool {
        match &self.runner {
            Some(inputs) => inputs.compile_snapshot,
            None => self._brand.compile_snapshot(),
        }
    }

    pub(crate) fn facade_plan(&self) -> Option<&[u8]> {
        match &self.runner {
            Some(inputs) => Some(&inputs.facade),
            None => self._brand.facade_plan(),
        }
    }
}

/// Comparison evidence only; the shared account is an Arc clone, with no copied
/// context strings, ancestor arrays, validators, or new current authority.
pub(super) struct Binding {
    context: [u8; 32],
    graph: crate::node::GraphBudget,
    action: crate::node::ActionKey,
    content: crate::node::Digest,
}

impl Binding {
    pub(super) fn capture(chain: &crate::node::Chain) -> Self {
        Self {
            context: chain.context().identity(),
            graph: chain.context().graph_budget().clone(),
            action: chain.current_parent().action(),
            content: chain.current_parent().content(),
        }
    }

    fn matches(&self, chain: &crate::node::Chain) -> bool {
        self.context == chain.context().identity()
            && self.graph.shares_with(chain.context().graph_budget())
            && self.action == chain.current_parent().action()
            && self.content == chain.current_parent().content()
    }
}

impl<B> Request<B> {
    pub(crate) fn require_runner_profile(
        &self,
        profile: &crate::profile::Proof,
    ) -> Result<(), crate::bridge::request::Error> {
        let Some(inputs) = &self.runner else {
            return Ok(());
        };
        let failure = || crate::bridge::request::Error::Drift {
            field: "owned runner facade ancestry",
        };
        let chain = profile.node_chain().ok_or_else(failure)?;
        if inputs.provenance.context != chain.context().identity()
            || !inputs
                .provenance
                .graph
                .shares_with(chain.context().graph_budget())
            || !chain
                .current_parent()
                .read_set()
                .contains_entry(&inputs.entry)
        {
            return Err(failure());
        }
        Ok(())
    }
}

impl<B> Request<B> {
    pub(crate) fn require_retained_runner_profile(
        &self,
        pair: &crate::composition::profile::Pair,
    ) -> Result<(), crate::bridge::request::Error> {
        let failure = || crate::bridge::request::Error::Drift {
            field: "owned runner facade ancestry",
        };
        let inputs = self.runner.as_ref().ok_or_else(failure)?;
        pair.retention
            .covers_parents(std::slice::from_ref(&pair.generated))
            .map_err(|error| crate::bridge::request::Error::Node(Box::new(error)))?;
        if inputs.provenance.context != pair.context.identity()
            || !inputs
                .provenance
                .graph
                .shares_with(pair.context.graph_budget())
            || !pair.generated_fragment().contains_entry(&inputs.entry)
        {
            return Err(failure());
        }
        Ok(())
    }
}
