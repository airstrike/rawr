//! Complete retained-record interpretation without execution or publication.

use std::path::Path;
use std::sync::Arc;

use super::layout;
use crate::cargo::{self, internal, unit, universe, Limits, Request, RequestAllocation, Selection};
use crate::extract::plan::{namespace, Capture, Plan};
use crate::input::{build, stored};

/// Pure interpretation of one sealed record's exact bytes. The owner
/// may share it across the replays of one graph evaluation whose observation
/// names the same commitment; it grants no freshness, so every replay still runs
/// each current observation over the bundle, its inputs and its completion.
pub(in crate::cargo) struct Decoded {
    pub(in crate::cargo) metadata: unit::Metadata,
    pub(in crate::cargo) graph: unit::BuildGraph,
    pub(in crate::cargo) selected: Box<[Selection]>,
    pub(in crate::cargo) internal: internal::Recipe,
    retained: build::Retained,
    namespace: Option<Arc<namespace::Retained>>,
    stored: stored::Stored,
    pending: universe::Pending,
}

impl Decoded {
    /// Fixed shared-holder storage charged when the interpretation is retained.
    pub(in crate::cargo) const HOLDER_BYTES: usize =
        std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>();
    const NAMESPACE_HOLDER_BYTES: usize =
        std::mem::size_of::<namespace::Retained>() + 2 * std::mem::size_of::<usize>();

    /// The recorded input entries this interpretation's replay compares.
    pub(in crate::cargo) fn entries(&self) -> &[crate::input::proof::Entry] {
        self.stored.entries()
    }
}

/// The complete replay result, not a current node receipt or published owner.
pub(in crate::cargo) struct Replay {
    pub(in crate::cargo) decoded: Arc<Decoded>,
    pub(in crate::cargo) capture: Capture,
    pub(in crate::cargo) completion: universe::Completion,
    pub(in crate::cargo) input: stored::Completed,
    /// Stamps taken before this replay read its inputs.
    pub(in crate::cargo) watched: super::watched::Watched,
}

impl Request {
    /// Join authenticated bundle bytes to current semantic replay, carrying the
    /// original input account through framing and every interpretation step.
    /// A `staged` interpretation skips only the pure decode of record bytes the
    /// observation has already hashed against their sealed commitment; the
    /// caller is responsible for matching every key component exactly.
    #[allow(
        clippy::too_many_arguments,
        reason = "one replay binds the complete owner inputs and original allocation accounts"
    )]
    pub(in crate::cargo) fn replay_observed_in(
        &self,
        cached: &cargo::CachedToolchain,
        observed: super::bundle::PendingReplay<'_, '_, '_>,
        cargo_home: Option<&Path>,
        definition: &universe::Definition,
        lock: universe::Candidate,
        request: cargo::Digest,
        context: universe::ContextProof,
        receipt: Option<universe::Receipt>,
        staged: Option<Arc<Decoded>>,
        limits: &Limits,
        cargo_limits: &Limits,
        allocation: &mut RequestAllocation,
        cargo_allocation: &mut unit::Allocation,
    ) -> stored::Result<Replay> {
        let (paths, bytes, inventory, budget) = observed.into_parts();
        let result = (|| {
            let decoded = match staged {
                Some(decoded) => decoded,
                None => {
                    let record = stored::Record::decode(&bytes, budget)?;
                    self.decode_record_in(
                        &paths,
                        &record,
                        definition,
                        lock,
                        request,
                        context,
                        receipt,
                        limits,
                        cargo_limits,
                        allocation,
                        cargo_allocation,
                        budget,
                    )?
                }
            };
            self.replay_decoded_in(
                cached, &paths, cargo_home, decoded, inventory, limits, allocation, budget,
            )
        })();
        result.map_err(|error| budget.failure(error))
    }

    /// Interpret all five historical sections using current supplied associations
    /// and the caller's existing accounts. This is pure over the record's exact
    /// bytes; bundle/current-node validation and every current input observation
    /// remain the responsibility of the replay that consumes the result.
    #[allow(
        clippy::too_many_arguments,
        reason = "one decode binds the complete owner inputs and original allocation accounts"
    )]
    fn decode_record_in(
        &self,
        paths: &layout::Paths,
        record: &stored::Record<'_>,
        definition: &universe::Definition,
        lock: universe::Candidate,
        request: cargo::Digest,
        context: universe::ContextProof,
        receipt: Option<universe::Receipt>,
        limits: &Limits,
        cargo_limits: &Limits,
        allocation: &mut RequestAllocation,
        cargo_allocation: &mut unit::Allocation,
        budget: &mut stored::Budget<'_>,
    ) -> stored::Result<Arc<Decoded>> {
        budget.check()?;
        let (metadata, selected, internal, graph, pending) = self.bind_retained_documents_in(
            paths,
            cargo::DocumentBytes::from_record(record),
            definition,
            lock,
            request,
            context,
            receipt,
            limits,
            cargo_limits,
            allocation,
            cargo_allocation,
        )?;
        let retained = build::Retained::decode(
            record.build(),
            &graph,
            paths.final_form().build(),
            &self.target,
            budget,
        )?;
        let namespace = match record.decode_namespace(budget)? {
            Some(namespace) => {
                cargo::ControlAccount::current(allocation, budget.graph())
                    .holder(Decoded::NAMESPACE_HOLDER_BYTES)?;
                Some(Arc::new(namespace))
            }
            None => None,
        };
        let stored = stored::Stored::decode(record.input(), budget)?;
        cargo::ControlAccount::current(allocation, budget.graph()).holder(Decoded::HOLDER_BYTES)?;
        Ok(Arc::new(Decoded {
            metadata,
            graph,
            selected,
            internal,
            retained,
            namespace,
            stored,
            pending,
        }))
    }

    /// Run every current observation over the bundle and its inputs against one
    /// decoded interpretation, then join the exact universe completion. `cargo_home`
    /// borrows the owner's already-acquired normalized home for this Request's
    /// captured environment; replay does not acquire ambient path authority.
    #[allow(
        clippy::too_many_arguments,
        reason = "one replay binds the complete owner inputs and original allocation accounts"
    )]
    fn replay_decoded_in(
        &self,
        cached: &cargo::CachedToolchain,
        paths: &layout::Paths,
        cargo_home: Option<&Path>,
        decoded: Arc<Decoded>,
        inventory: super::bundle::Inventory<'_>,
        limits: &Limits,
        allocation: &mut RequestAllocation,
        budget: &mut stored::Budget<'_>,
    ) -> stored::Result<Replay> {
        budget.check()?;
        let watched = super::watched::Watched::observe(
            paths.root(),
            decoded.stored.entries(),
            decoded.namespace.as_deref(),
        );
        let toolchain = cached.attestation.value().proof();
        let capture = Plan::replay_with_namespace(
            paths,
            &decoded.graph,
            toolchain,
            &self.target,
            decoded.retained.build(),
            &decoded.metadata,
            limits,
            budget.graph(),
            decoded.namespace.clone(),
            Some(inventory),
            &self.demand,
        )
        .map_err(|error| match error {
            crate::Error::Node { source, .. } => stored::Error::Bundle(*source),
            error => stored::Error::Input(error),
        })?;
        let config = self.config.input_config_accounted(allocation)?;
        let cargo_env_overrides = config.cargo_env_overrides;
        drop(config);
        let manifest = budget.digest(
            cargo::render_manifest_accounted(self, Some(&decoded.internal), limits, allocation)?
                .as_bytes(),
        )?;
        let source = budget.digest(cargo::HARNESS_SOURCE.as_bytes())?;
        let final_form = paths.final_form();
        let compared = decoded.stored.require_inputs(
            &self.config,
            &self.inputs,
            &capture,
            [
                final_form.manifest(),
                final_form.source(),
                final_form.lock(),
            ],
            toolchain,
            &decoded.internal,
            &self.target,
            &self.profile,
            &decoded.retained,
            &decoded.metadata,
            cargo_env_overrides,
            cargo_home,
            &self.project_root,
            &decoded.graph,
            final_form.build(),
            budget,
        )?;
        let (completion, input) = decoded.pending.complete_replayed(
            manifest,
            source,
            decoded.metadata.digest(),
            decoded.graph.digest(),
            compared,
            limits,
        )?;
        Ok(Replay {
            decoded,
            capture,
            completion,
            input,
            watched,
        })
    }
}
