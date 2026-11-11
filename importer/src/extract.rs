//! Correlated sidecar decoding and deterministic normal-API canonicalization.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::api::{self, identity, ledger, model};
use crate::cargo;
use crate::{Error, Result};

const CANONICAL_EVIDENCE_ITEMS: usize = 25_000_000;
const CANONICAL_EVIDENCE_BYTES: usize = 2 * 1024 * 1024 * 1024;

mod conflict;
#[cfg(feature = "rustc-driver")]
#[allow(
    dead_code,
    reason = "shared with the driver, which answers and records the units"
)]
pub(crate) mod engine;
#[cfg(feature = "rustc-driver")]
pub(crate) mod plan;
pub(crate) mod protocol;
#[cfg(feature = "rustc-driver")]
pub(crate) mod shared;
pub(crate) mod sidecar;

#[cfg(feature = "rustc-driver")]
pub(crate) struct DirectPublication {
    context: api::Context,
    document: protocol::Document,
    invocations: Invocations,
    input: crate::input::Baseline,
    policy: Policy,
}

#[cfg(feature = "rustc-driver")]
// Both variants are large; boxing one would allocate per publication and barely shrink it.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Publication {
    Direct(DirectPublication),
    Cached {
        graph: api::Graph<api::Captured>,
        input: crate::input::Baseline,
        chain: crate::node::Chain,
    },
}

/// Exact scalar ownership observed after a private nonpublishing canonicalization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalizationEvidence {
    items: usize,
    bytes: usize,
    #[cfg(test)]
    counters: EvidenceCounters,
    #[cfg(all(test, feature = "rustc-driver"))]
    stages: crate::process::EvidenceStages,
}

#[cfg(feature = "rustc-driver")]
struct EvidenceCapability {
    publication: Publication,
    session: crate::process::EvidenceSession,
}

#[cfg(all(feature = "rustc-driver", feature = "test-fixtures"))]
struct ClassificationEvidenceCapability {
    publication: Publication,
}

#[cfg(all(feature = "rustc-driver", feature = "test-fixtures"))]
struct FacadeSurfaceEvidenceCapability {
    publication: Publication,
}

/// Transform the compiler sidecars retained by a completed resolution.
#[cfg(feature = "rustc-driver")]
pub fn from_resolution(
    resolution: cargo::Resolution,
    native: &crate::process::Native,
    limits: &cargo::Limits,
) -> Result<api::Graph<api::Extracted>> {
    resolution.publication(native, limits, |resolution| {
        resolution.into_extraction(limits)?.publish(limits)
    })
}

/// Publish only after the complete owner and API node have validated together.
#[cfg(feature = "rustc-driver")]
#[allow(
    clippy::too_many_arguments,
    reason = "publication consumes the complete owner observations with their exact request context"
)]
pub(crate) fn from_owner_parts(
    context: &crate::context::Context,
    request: &[u8; 32],
    owner: &crate::node::Validated<cargo::resolution::owner::Product>,
    retention: &crate::node::read_set::Retention,
    metadata: &cargo::unit::Metadata,
    build_unit: &cargo::unit::BuildGraph,
    selected: &[cargo::Selection],
    toolchain: &cargo::toolchain::Proof,
    target: &str,
    profile: &str,
    universe: cargo::universe::Proof,
    mut capture: plan::Capture,
    input: crate::input::stored::Completed,
    policy: &Policy,
    limits: &cargo::Limits,
) -> Result<api::Graph<api::Extracted>> {
    let policy_identity = policy.cache_identity()?;
    let (product, graph) = api::cached::evaluate_owner(
        context,
        request,
        &policy_identity,
        owner,
        retention,
        &crate::metrics::Current,
        |stored| {
            let context = api::Context::from_resolution_parts(
                metadata,
                build_unit,
                selected,
                toolchain,
                target,
                profile,
                Some(universe),
                capture.allocation_mut(),
            )?;
            let (document, invocations) = capture.into_parts(limits)?;
            materialize_document(stored, document, invocations, policy, &context, limits)
        },
    )
    .map_err(|source| Error::Node {
        operation: "API from current resolution owner",
        source: Box::new(source),
    })?;
    // Staged API promotion validates the complete closure while its parent
    // retention pin is held, before this input capability is published.
    let proof = input.publish(owner, context.graph_budget())?;
    let successor = retention
        .extend_current(&product)
        .map_err(|source| Error::Node {
            operation: "API successor retention",
            source: Box::new(source),
        })?;
    let chain = crate::node::Chain::validated_retained(context.clone(), &product, successor)
        .map_err(|source| Error::Node {
            operation: "API retained chain",
            source: Box::new(source),
        })?;
    graph.publish_with_chain(proof, Some(chain))
}

/// Rebuild the captured graph from the complete owner observations, replaying
/// the stored API product when one is supplied.
#[cfg(feature = "rustc-driver")]
#[allow(
    clippy::too_many_arguments,
    reason = "materialization consumes the complete owner observations"
)]
pub(crate) fn materialize_owner(
    stored: Option<&api::cached::Product>,
    metadata: &cargo::unit::Metadata,
    build_unit: &cargo::unit::BuildGraph,
    selected: &[cargo::Selection],
    toolchain: &cargo::toolchain::Proof,
    target: &str,
    profile: &str,
    universe: cargo::universe::Proof,
    mut capture: plan::Capture,
    policy: &Policy,
    limits: &cargo::Limits,
) -> Result<api::Graph<api::Captured>> {
    let context = api::Context::from_resolution_parts(
        metadata,
        build_unit,
        selected,
        toolchain,
        target,
        profile,
        Some(universe),
        capture.allocation_mut(),
    )?;
    let (document, invocations) = capture.into_parts(limits)?;
    materialize_document(stored, document, invocations, policy, &context, limits)
}

/// Retain the API from the owner; a current API hit defers `materialize`.
#[cfg(feature = "rustc-driver")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn retain_from_owner_parts(
    context: &crate::context::Context,
    request: &[u8; 32],
    owner: crate::node::Validated<cargo::resolution::owner::Product>,
    retention: &crate::node::read_set::Retention,
    input: crate::input::stored::Completed,
    policy: &Policy,
    memo: impl FnOnce(&[u8; 32]) -> Option<[u8; 32]>,
    materialize: impl FnOnce(Option<&api::cached::Product>) -> Result<api::Graph<api::Captured>>
        + Send
        + 'static,
) -> Result<api::Continuation> {
    let policy_identity = policy.cache_identity()?;
    api::cached::retain_owner(
        context,
        request,
        &policy_identity,
        owner,
        retention,
        || memo(&policy_identity),
        materialize,
        |owner, product, retained, graph| {
            api::retained::Continuation::capture(context, owner, input, product, retained, graph)
        },
    )
    .map_err(|source| Error::Node {
        operation: "retained API from resolution owner",
        source: Box::new(source),
    })
}

/// The API graph of a run that replayed shared bindings: the sidecar
/// `document` it gave, with the external crates bound through this
/// project's unit graph instead of a build (`crate::bindings`).
#[cfg(feature = "rustc-driver")]
pub(crate) fn materialize_shared(
    stored: Option<&api::cached::Product>,
    document: &[u8],
    external: Vec<ExternalBinding>,
    context: &api::Context,
    policy: &Policy,
    limits: &cargo::Limits,
) -> Result<api::Graph<api::Captured>> {
    let mut allocation = Allocation::new(limits);
    let document = protocol::decode_accounted(document, limits, &mut allocation)?;
    let invocations = Invocations::seal_document_with_allocation(&document, allocation)?
        .bind_external(external)?;
    materialize_document(stored, document, invocations, policy, context, limits)
}

/// The API context of a run that replayed shared bindings, from its
/// resolution. It is plain data, so a later run keeps it (`owner::memo`).
#[cfg(feature = "rustc-driver")]
#[allow(
    clippy::too_many_arguments,
    reason = "the context joins the resolution, the toolchain and the request"
)]
pub(crate) fn shared_context(
    metadata: &cargo::unit::Metadata,
    build_unit: &cargo::unit::BuildGraph,
    selected: &[cargo::Selection],
    toolchain: &cargo::toolchain::Proof,
    target: &str,
    profile: &str,
    limits: &cargo::Limits,
) -> Result<api::Context> {
    let mut allocation = Allocation::new(limits);
    api::Context::from_resolution_parts(
        metadata,
        build_unit,
        selected,
        toolchain,
        target,
        profile,
        None,
        &mut allocation,
    )
}

/// Retain the API of a run that replayed shared bindings, under the
/// toolchain attestation; a current API hit defers `materialize`. An API
/// that is not current under this attestation, as after an install that
/// replaced the driver, is staged from its memo when the tables gave it
/// before.
#[cfg(feature = "rustc-driver")]
pub(crate) fn retain_shared(
    context: &crate::context::Context,
    request: &[u8; 32],
    parent: std::sync::Arc<crate::node::Validated<cargo::toolchain::Attestation>>,
    proof: crate::input::proof::Proof,
    policy: &Policy,
    limits: &cargo::Limits,
    materialize: impl FnOnce(Option<&api::cached::Product>) -> Result<api::Graph<api::Captured>>
        + Send
        + 'static,
) -> Result<api::Continuation> {
    let node_error = |operation: &'static str| {
        move |source| Error::Node {
            operation,
            source: Box::new(source),
        }
    };
    let policy_identity = policy.cache_identity()?;
    let retention = crate::node::read_set::Retention::from_validated(parent.as_ref(), None)
        .map_err(node_error("shared bindings retention"))?;
    api::cached::retain_under(
        context,
        request,
        &policy_identity,
        crate::node::Parent::validated(parent.as_ref()),
        &retention,
        || {
            Some(api::memo::shared_key(
                request,
                &policy_identity,
                &context.identity(),
                limits,
            ))
        },
        materialize,
        |product, retained, graph| {
            api::retained::Continuation::capture_shared(
                context, parent, proof, product, retained, graph,
            )
        },
    )
    .map_err(node_error("retained API from shared bindings"))
}

/// Measure one complete canonicalization without granting publication authority.
#[cfg(feature = "rustc-driver")]
pub fn measure_canonicalization(
    request: cargo::Request,
    native: &crate::process::Native,
    limits: &cargo::Limits,
) -> Result<CanonicalizationEvidence> {
    let session = crate::process::EvidenceSession::new();
    let resolution = request.resolve_evidence(native, limits, &session)?;
    resolution.publication(native, limits, |resolution| {
        let publication = resolution.into_extraction_uncached(limits)?;
        EvidenceCapability {
            publication,
            session,
        }
        .measure(limits)
    })
}

/// Run one scalar-only classification measurement after exact normal extraction.
///
/// This test-fixture-only entry point performs input proof revalidation without
/// cache publication, consumes the Graph and Contract inside the private
/// capability, prints only fixed scalars, and returns no product authority.
#[cfg(all(feature = "rustc-driver", feature = "test-fixtures"))]
#[doc(hidden)]
pub fn measure_classification(
    request: cargo::Request,
    native: &crate::process::Native,
    limits: &cargo::Limits,
) -> Result<()> {
    let resolution = request.resolve(native, limits)?;
    resolution.publication(native, limits, |resolution| {
        let publication = resolution.into_extraction_uncached(limits)?;
        ClassificationEvidenceCapability { publication }.measure(limits)
    })
}

/// Verify one exact classified facade surface without cache publication.
///
/// This test-fixture-only transform uses the governed evidence sampler, consumes
/// the extraction and classification authorities internally, and returns no
/// graph, contract, facade plan, or publication capability.
#[cfg(all(feature = "rustc-driver", feature = "test-fixtures"))]
#[doc(hidden)]
pub fn verify_facade_surface(
    request: cargo::Request,
    native: &crate::process::Native,
    limits: &cargo::Limits,
    root: &str,
    expected_types: &[&str],
    expected_items: &[&str],
    rejected_paths: &[&str],
) -> Result<()> {
    let session = crate::process::EvidenceSession::new();
    let resolution = request.resolve_evidence(native, limits, &session)?;
    resolution.publication(native, limits, |resolution| {
        let publication = resolution.into_extraction_uncached(limits)?;
        FacadeSurfaceEvidenceCapability { publication }.verify(
            limits,
            root,
            expected_types,
            expected_items,
            rejected_paths,
        )
    })
}

#[cfg(all(feature = "rustc-driver", feature = "test-fixtures"))]
impl FacadeSurfaceEvidenceCapability {
    fn verify(
        self,
        limits: &cargo::Limits,
        root: &str,
        expected_types: &[&str],
        expected_items: &[&str],
        rejected_paths: &[&str],
    ) -> Result<()> {
        let Self { publication } = self;
        let DirectPublication {
            context,
            document,
            invocations,
            input,
            policy,
        } = publication.into_direct()?;
        let graph = canonicalize_document(document, invocations, policy, &context, limits)?;
        let proof = input.prove(limits)?;
        let graph = graph.publish(proof)?;
        let contract = graph
            .classify(&crate::classify::Policy::standard(), limits)
            .map_err(|error| extraction("bindings-surface-classification", &error.to_string()))?;
        let bytes = crate::surface::facade::Plan::encode(&contract)
            .map_err(|error| extraction("bindings-surface-encoding", &error.to_string()))?;
        let plan = crate::surface::facade::Plan::decode(&bytes)
            .map_err(|error| extraction("bindings-surface-decoding", &error.to_string()))?;
        let mut facades = plan.facades().filter(|facade| facade.root() == root);
        let facade = facades.next().ok_or_else(|| {
            extraction("bindings-surface-root", "expected bindings root is absent")
        })?;
        if facades.next().is_some() {
            return Err(extraction(
                "bindings-surface-root",
                "expected bindings root is duplicated",
            ));
        }
        for path in expected_types {
            if !facade.types().any(|ty| ty.path() == *path) {
                return Err(extraction("bindings-surface-type", path));
            }
        }
        for path in expected_items {
            if !facade.items().any(|item| item.path() == *path) {
                return Err(extraction("bindings-surface-item", path));
            }
        }
        for path in rejected_paths {
            if facade.types().any(|ty| ty.path() == *path)
                || facade.items().any(|item| item.path() == *path)
            {
                return Err(extraction("bindings-surface-rejection", path));
            }
        }
        eprintln!(
            "BINDINGS_SURFACE root={} types={} items={} expected_types={} expected_items={} rejected={}",
            root,
            facade.types().count(),
            facade.items().count(),
            expected_types.len(),
            expected_items.len(),
            rejected_paths.len(),
        );
        Ok(())
    }
}

#[cfg(all(feature = "rustc-driver", feature = "test-fixtures"))]
impl ClassificationEvidenceCapability {
    fn measure(self, limits: &cargo::Limits) -> Result<()> {
        let DirectPublication {
            context,
            document,
            invocations,
            input,
            policy,
        } = self.publication.into_direct()?;
        let graph = canonicalize_document(document, invocations, policy, &context, limits)?;
        let proof = input.prove(limits)?;
        let graph = graph.publish(proof)?;
        let evidence = graph
            .into_classification_evidence()
            .measure(&crate::classify::Policy::standard())
            .map_err(|error| extraction("classification-evidence", &error.to_string()))?;
        let retained = evidence.graph_retained;
        eprintln!(
            "GRAPH_RETAINED context_items={} context_bytes={} invocation_items={} invocation_bytes={} definition_items={} definition_bytes={} kind_items={} kind_bytes={} type_items={} type_bytes={} predicate_items={} predicate_bytes={} generic_items={} generic_bytes={} instantiation_items={} instantiation_bytes={} export_items={} export_bytes={} implementation_items={} implementation_bytes={} support_items={} support_bytes={} anonymous_items={} anonymous_bytes={} canonical_items={} canonical_bytes={} total_items={} total_bytes={}",
            retained.context.items,
            retained.context.bytes,
            retained.invocation.items,
            retained.invocation.bytes,
            retained.definition.items,
            retained.definition.bytes,
            retained.kind.items,
            retained.kind.bytes,
            retained.ty.items,
            retained.ty.bytes,
            retained.predicate.items,
            retained.predicate.bytes,
            retained.generic.items,
            retained.generic.bytes,
            retained.instantiation.items,
            retained.instantiation.bytes,
            retained.export.items,
            retained.export.bytes,
            retained.implementation.items,
            retained.implementation.bytes,
            retained.support.items,
            retained.support.bytes,
            retained.anonymous.items,
            retained.anonymous.bytes,
            retained.canonical.items,
            retained.canonical.bytes,
            retained.total.items,
            retained.total.bytes,
        );
        eprintln!(
            "CLASSIFICATION_COUNTS transaction_items={} transaction_bytes={} graph_items={} graph_bytes={} definitions={} implementations={} exports={} support_groups={} support_edges={} support_rejections={} implementation_ledgers={} associated={} classifier_new_items={} final_items={} item_limit={} production_item_limit={}",
            evidence.transaction_items,
            evidence.transaction_bytes,
            evidence.graph_items,
            evidence.graph_bytes,
            evidence.definitions,
            evidence.implementations,
            evidence.exports,
            evidence.support_groups,
            evidence.support_edges,
            evidence.support_rejections,
            evidence.implementation_ledgers,
            evidence.associated,
            evidence.classifier_new_items,
            evidence.final_items,
            crate::classify::EVIDENCE_RETAINED_ITEMS,
            limits.item_limit(),
        );
        eprintln!(
            "CLASSIFICATION_BYTES graph={} contract_inline={} definition_plans={} outcomes={} support={} canonical={} final={} scratch_high_water={} byte_limit={} production_byte_limit={}",
            evidence.bytes.graph,
            evidence.bytes.contract_inline,
            evidence.bytes.definition_plans,
            evidence.bytes.outcomes,
            evidence.bytes.support,
            evidence.bytes.canonical,
            evidence.final_bytes,
            evidence.scratch_high_water_bytes,
            crate::classify::EVIDENCE_RETAINED_BYTES,
            limits.byte_limit(),
        );
        eprintln!(
            "CLASSIFICATION_STAGE elapsed_ns={}",
            evidence.elapsed.as_nanos(),
        );
        Ok(())
    }
}

impl CanonicalizationEvidence {
    /// Return the exact cumulative owned-item charge at completed canonicalization.
    #[must_use]
    pub const fn items(self) -> usize {
        self.items
    }

    /// Return the exact cumulative owned-byte charge at completed canonicalization.
    #[must_use]
    pub const fn bytes(self) -> usize {
        self.bytes
    }
}

#[cfg(feature = "rustc-driver")]
impl Publication {
    pub(crate) fn inventory(
        self,
        limits: &cargo::Limits,
    ) -> Result<crate::applicability::inventory::Acquired> {
        use crate::applicability::inventory;

        let DirectPublication {
            context,
            document,
            invocations,
            input,
            policy,
        } = self.into_direct()?;
        let Invocations {
            authority,
            mut allocation,
        } = invocations;
        allocation.retain(policy.usage)?;
        validate_resolution_context(&document.fragments, &context, &mut allocation)?;
        let catalog = Catalog::from_fragments(&document.fragments, &authority, &mut allocation)?;
        let mut partitions = Vec::new();
        allocation.reserve(&mut partitions, document.fragments.len())?;
        // The crates this capture holds whole. A definition of any other,
        // as the standard library's, is in no crate's own facts: each
        // crate's entry keeps the ones its own walk recorded (`foreign`).
        let captured = document
            .fragments
            .iter()
            .map(|fragment| {
                (
                    fragment.handshake.crate_name.as_str(),
                    fragment.handshake.artifact.stable_crate_id.as_str(),
                )
            })
            .collect::<BTreeSet<_>>();
        let mut foreign = BTreeMap::new();
        for fragment in &document.fragments {
            // An inventory capture asks the driver for each crate's whole
            // domain, which it gives for the crate it compiled.
            let manifest = fragment
                .inventory
                .as_ref()
                .expect("an inventory capture's fragment carries its crate's inventory");
            assert!(
                manifest.schema == 1
                    && manifest.owner == fragment.handshake.artifact.stable_crate_id,
                "crate `{}`'s inventory is another crate's",
                fragment.handshake.crate_name
            );
            let key = CompilerKey {
                crate_name: fragment.handshake.crate_name.as_str().into(),
                stable_crate_id: manifest.owner.as_str().into(),
            };
            let unit = catalog
                .units
                .get(&key)
                .expect("the catalog holds each fragment's own crate");
            allocation.unit(unit)?;
            let raw_foreign = fragment
                .definitions
                .iter()
                .filter(|raw| {
                    let protocol::Authority::Compiler {
                        crate_name,
                        stable_crate_id,
                    } = &raw.id.authority;
                    !captured.contains(&(crate_name.as_str(), stable_crate_id.as_str()))
                })
                .collect::<Vec<_>>();
            let mut recorded = Vec::new();
            allocation.reserve(&mut recorded, raw_foreign.len())?;
            for raw in raw_foreign {
                if catalog
                    .unavailable_identity(&raw.id, &mut allocation)?
                    .is_some()
                {
                    continue;
                }
                allocation.protocol_id(&raw.id)?;
                recorded.push(catalog.id_bounded(raw.id.clone(), &mut allocation)?);
            }
            recorded.sort_unstable();
            recorded.dedup();
            allocation.unit(unit)?;
            foreign.insert(unit.clone(), recorded.into_boxed_slice());
            let mut inherent = Vec::new();
            allocation.reserve(&mut inherent, manifest.inherent.len())?;
            for domain in &manifest.inherent {
                inherent.push(inventory::Inherent {
                    owner: inventory_id(&domain.owner, &catalog, &mut allocation)?,
                    implementations: inventory_ids(
                        &domain.implementations,
                        &catalog,
                        &mut allocation,
                    )?,
                    associated: inventory_associations(
                        &domain.associated,
                        &catalog,
                        &mut allocation,
                    )?,
                });
            }
            let mut dependencies = Vec::new();
            let mut missing_dependencies = Vec::new();
            allocation.reserve(&mut dependencies, fragment.handshake.dependencies.len())?;
            allocation.reserve(
                &mut missing_dependencies,
                fragment.handshake.dependencies.len(),
            )?;
            for key in &fragment.handshake.dependencies {
                match document
                    .fragments
                    .iter()
                    .find(|fragment| &fragment.handshake.unit_key == key)
                {
                    Some(fragment) => {
                        let key = CompilerKey {
                            crate_name: fragment.handshake.crate_name.as_str().into(),
                            stable_crate_id: fragment
                                .handshake
                                .artifact
                                .stable_crate_id
                                .as_str()
                                .into(),
                        };
                        let dependency = catalog
                            .units
                            .get(&key)
                            .expect("the catalog holds each fragment's own crate");
                        allocation.unit(dependency)?;
                        dependencies.push(dependency.clone());
                    }
                    None => missing_dependencies.push(allocation.owned(key)?),
                }
            }
            dependencies.sort_unstable();
            missing_dependencies.sort_unstable();
            let mut preludes = Vec::new();
            allocation.reserve(&mut preludes, manifest.preludes.len())?;
            for prelude in &manifest.preludes {
                let mut traits = Vec::new();
                allocation.reserve(&mut traits, prelude.traits.len())?;
                for trait_ in &prelude.traits {
                    let mut methods = Vec::new();
                    allocation.reserve(&mut methods, trait_.methods.len())?;
                    for method in &trait_.methods {
                        methods.push(allocation.owned(method)?);
                    }
                    traits.push(inventory::TraitNames {
                        id: inventory_id(&trait_.id, &catalog, &mut allocation)?,
                        methods: methods.into_boxed_slice(),
                    });
                }
                preludes.push(inventory::Prelude {
                    module: inventory_id(&prelude.module, &catalog, &mut allocation)?,
                    edition: allocation.owned(&prelude.edition)?,
                    traits: traits.into_boxed_slice(),
                });
            }
            preludes.sort_unstable();
            let deref = manifest
                .deref
                .as_ref()
                .map(|lookup| {
                    let mut domains = Vec::new();
                    allocation.reserve(&mut domains, lookup.domains.len())?;
                    for domain in &lookup.domains {
                        let mut implementations = Vec::new();
                        allocation.reserve(&mut implementations, domain.implementations.len())?;
                        for header in &domain.implementations {
                            implementations.push(inventory::LookupHeader {
                                implementation: inventory_id(
                                    &header.implementation,
                                    &catalog,
                                    &mut allocation,
                                )?,
                                receiver: match &header.receiver {
                                    engine::model::Receiver::Nominal(id) => {
                                        inventory::Receiver::Nominal(inventory_id(
                                            id,
                                            &catalog,
                                            &mut allocation,
                                        )?)
                                    }
                                    engine::model::Receiver::Closed => inventory::Receiver::Closed,
                                    engine::model::Receiver::Unresolved => {
                                        inventory::Receiver::Unresolved
                                    }
                                },
                            });
                        }
                        domains.push(inventory::LookupDomain {
                            root: inventory_id(&domain.root, &catalog, &mut allocation)?,
                            implementations: implementations.into_boxed_slice(),
                        });
                    }
                    Ok::<_, crate::Error>(inventory::Lookup {
                        trait_id: inventory_id(&lookup.trait_id, &catalog, &mut allocation)?,
                        domains: domains.into_boxed_slice(),
                    })
                })
                .transpose()?;
            partitions.push(inventory::Partition {
                unit: unit.clone(),
                native_unit: allocation.owned(&fragment.handshake.unit_key)?,
                native_inputs: input
                    .partition(&fragment.handshake.unit_key)
                    .expect("the input baseline witnesses each crate it captured"),
                compiler: allocation.owned(&fragment.handshake.compiler_commit)?,
                target: allocation.owned(&fragment.handshake.selected_target)?,
                crate_hash: allocation.owned(&fragment.handshake.artifact.crate_hash)?,
                preludes: preludes.into_boxed_slice(),
                deref,
                dependencies: dependencies.into_boxed_slice(),
                missing_dependencies: missing_dependencies.into_boxed_slice(),
                implementations: inventory_ids(
                    &manifest.implementations,
                    &catalog,
                    &mut allocation,
                )?,
                associated: inventory_associations(
                    &manifest.associated,
                    &catalog,
                    &mut allocation,
                )?,
                unsupported: inventory_ids(&manifest.unsupported, &catalog, &mut allocation)?,
                inherent: inherent.into_boxed_slice(),
            });
        }
        partitions.sort_unstable_by(|left, right| left.unit.cmp(&right.unit));
        let candidate =
            Candidate::from_fragments(document.fragments, &catalog, limits, &mut allocation)?;
        for partition in &mut partitions {
            partition.mark_unsupported(&candidate.unmodeled);
        }
        let (items, bytes) =
            api::intrinsic_usage(candidate.definitions.values(), &candidate.implementations)
                .expect("facts held in memory have a size that fits in memory");
        allocation.items(items)?;
        allocation.bytes(bytes)?;
        // An inventory is its crates' facts and each export's target, which
        // is all its entries keep. A program's own demand decides what its
        // API admits; no API of the whole closure is built: iced's closure
        // alone passed a gigabyte and minutes of closure work.
        for export in candidate.exports.values() {
            validate_alias_chain(export, &candidate.definitions, &candidate.spellings)?;
        }
        let mut exports = Vec::new();
        allocation.reserve(&mut exports, candidate.exports.len())?;
        for export in candidate.exports.into_values() {
            if is_excluded(&policy, &export) {
                continue;
            }
            if let ExportTarget::Available(target) = export.target {
                exports.push((export.key, target));
            }
        }
        exports.sort_unstable();
        let mut implementations = candidate.implementations;
        implementations.sort_unstable_by(|left, right| left.id().cmp(right.id()));
        let facts = inventory::Facts {
            definitions: candidate.definitions.into_values().collect(),
            implementations: implementations.into_boxed_slice(),
            partitions: partitions.into_boxed_slice(),
        };
        // The inputs the capture read are the ones the build still sees.
        input.prove(limits)?;
        Ok(inventory::Acquired::authenticate(
            exports.into_boxed_slice(),
            facts,
            foreign,
        ))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "publication transforms every retained resolution authority as one proof bundle"
    )]
    pub(crate) fn from_resolution_parts(
        metadata: &cargo::unit::Metadata,
        build_unit: &cargo::unit::BuildGraph,
        selected: &[cargo::Selection],
        toolchain: &cargo::toolchain::Proof,
        target: &str,
        profile: &str,
        universe: Option<cargo::universe::Proof>,
        capture: plan::Capture,
        input: crate::input::Baseline,
        policy: Policy,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        let mut capture = capture;
        let context = api::Context::from_resolution_parts(
            metadata,
            build_unit,
            selected,
            toolchain,
            target,
            profile,
            universe,
            capture.allocation_mut(),
        )?;
        let (document, invocations) = capture.into_parts(limits)?;
        Ok(Self::Direct(DirectPublication {
            context,
            document,
            invocations,
            input,
            policy,
        }))
    }

    pub(crate) fn cache(self, dag: cargo::DagResolution, limits: &cargo::Limits) -> Result<Self> {
        let DirectPublication {
            context,
            document,
            invocations,
            input,
            policy,
        } = self.into_direct()?;
        let (product, graph) = api::cached::evaluate(
            &dag.context,
            dag.request.as_bytes(),
            &policy.cache_identity()?,
            &dag.build,
            &crate::metrics::Current,
            |stored| materialize_document(stored, document, invocations, &policy, &context, limits),
        )
        .map_err(|source| Error::Node {
            operation: "captured API graph",
            source: Box::new(source),
        })?;
        let chain = crate::node::Chain::validated(dag.context, &product);
        Ok(Self::Cached {
            graph,
            input,
            chain,
        })
    }

    fn publish(self, limits: &cargo::Limits) -> Result<api::Graph<api::Extracted>> {
        match self {
            Self::Direct(DirectPublication {
                context,
                document,
                invocations,
                input,
                policy,
            }) => {
                let graph = canonicalize_document(document, invocations, policy, &context, limits)?;
                crate::cache::publish(graph, input, limits)
            }
            Self::Cached {
                graph,
                input,
                chain,
            } => {
                let proof = input.prove(limits)?;
                graph.publish_with_chain(proof, Some(chain))
            }
        }
    }

    fn into_direct(self) -> Result<DirectPublication> {
        match self {
            Self::Direct(publication) => Ok(publication),
            Self::Cached { .. } => Err(extraction(
                "cached-publication-evidence",
                "a private evidence path cannot consume a cached publication",
            )),
        }
    }
}

#[cfg(feature = "rustc-driver")]
impl EvidenceCapability {
    fn measure(self, limits: &cargo::Limits) -> Result<CanonicalizationEvidence> {
        let Self {
            publication,
            session,
        } = self;
        let DirectPublication {
            context,
            document,
            invocations,
            input,
            policy,
        } = publication.into_direct()?;
        let evidence = canonicalize_evidence_document(
            document,
            invocations,
            policy,
            &context,
            limits,
            &session,
        )?;
        drop(input);
        Ok(evidence)
    }
}

/// Exact expected selected-unit handshakes sealed before sidecars are decoded.
pub struct Invocations {
    authority: InvocationAuthority,
    allocation: Allocation,
}

struct InvocationAuthority {
    #[cfg(test)]
    expected: BTreeMap<Box<str>, protocol::Handshake>,
    external_units: BTreeMap<CompilerKey, ExternalAuthority>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExternalUnit {
    unit_key: Box<str>,
    package_id: Box<str>,
    package_name: Box<str>,
    package_version: Box<str>,
    package_source: Box<str>,
    target: Box<str>,
    target_kinds: Box<[Box<str>]>,
    crate_types: Box<[Box<str>]>,
    crate_name: Box<str>,
    features: Box<[Box<str>]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ExternalAuthority {
    Available(ExternalUnit),
    Unavailable { artifacts: Box<[Box<str>]> },
}

#[cfg(feature = "rustc-driver")]
#[derive(serde::Deserialize, serde::Serialize)]
pub(super) struct ExternalBinding {
    crate_name: Box<str>,
    stable_crate_id: Box<str>,
    artifacts: Box<[Box<str>]>,
    authority: Option<crate::input::ArtifactAuthority>,
    package: Option<(Box<str>, Box<str>, Box<str>)>,
}

/// Fingerprinted Rawr-side exclusions, kept separate from compiler fragments.
type ExclusionPath = Box<[Box<str>]>;
type ExclusionsByPath = BTreeMap<ExclusionPath, Exclusion>;
type ManifestExclusions = BTreeMap<Box<str>, ExclusionsByPath>;

pub struct Policy {
    exclusions: ManifestExclusions,
    usage: PolicyUsage,
}

/// Transformation from manifest exclusions into one sealed policy.
pub struct ManifestPolicyBuilder {
    exclusions: ManifestExclusions,
    items: usize,
    bytes: usize,
}

/// One exact source range retained from the validated Rawr manifest.
pub struct ManifestSpan<'manifest> {
    file: &'manifest str,
    byte_start: u32,
    byte_end: u32,
    line_start: u32,
    column_start: u32,
    line_end: u32,
    column_end: u32,
}

#[derive(Clone, Copy, Default)]
struct PolicyUsage {
    items: usize,
    bytes: usize,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Key {
    alias: Box<str>,
    path: Box<[Box<str>]>,
    namespace: identity::Namespace,
}

struct Exclusion {
    reason: Box<str>,
    span: model::Span,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Catalog {
    units: BTreeMap<CompilerKey, identity::Unit>,
    sysroots: BTreeMap<CompilerKey, identity::Unit>,
    /// The crate names of each loaded crate with no authority, by stable
    /// crate id: each id an API names is looked up, never compared against
    /// every such crate, which a large closure has hundreds of.
    unavailable: BTreeMap<Box<str>, BTreeSet<Box<str>>>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CompilerKey {
    crate_name: Box<str>,
    stable_crate_id: Box<str>,
}

impl CompilerKey {
    /// The key a protocol id's authority names, taking its strings.
    fn from_raw(crate_name: String, stable_crate_id: String) -> Self {
        Self {
            crate_name: crate_name.into_boxed_str(),
            stable_crate_id: stable_crate_id.into_boxed_str(),
        }
    }
}

struct Candidate {
    definitions: BTreeMap<identity::Definition, model::Definition>,
    rejected: BTreeMap<identity::Definition, Rejected>,
    implementations: Vec<model::Implementation>,
    /// Implementations whose shape the extractor cannot model, such as one
    /// with a constant of a type it does not read. Each is unavailable by
    /// itself, and its crate's inventory lists it as unsupported.
    unmodeled: BTreeSet<identity::Definition>,
    scalar_predicate_proofs: Vec<model::ScalarPredicateProof>,
    implementation_index: BTreeMap<identity::Definition, Vec<usize>>,
    exports: BTreeMap<Key, Export>,
    anonymous: BTreeSet<ledger::AnonymousImport>,
    invocations: Vec<api::Invocation>,
    /// The other fragment's spelling of each definition merged from two
    /// selected crates; exports keep their own fragment's spelling.
    spellings: BTreeMap<identity::Definition, Box<[Box<str>]>>,
    /// Each demanded dependency's name index, per root alias.
    index: Vec<ledger::Index>,
}

#[derive(Clone, Serialize)]
struct Export {
    key: identity::Export,
    target: ExportTarget,
    aliases: Box<[ledger::Alias]>,
    reexports: Box<[ledger::Reexport]>,
    span: model::Span,
    variant: Option<Box<str>>,
}

#[derive(Clone, Serialize)]
enum ExportTarget {
    Available(identity::Definition),
    ExternalAuthorityUnavailable {
        crate_name: Box<str>,
        stable_crate_id: Box<str>,
    },
    UnsupportedPublicPath {
        compiler_kind: Box<str>,
    },
}

#[derive(Clone, Eq, PartialEq, Serialize)]
struct Rejected {
    reason: ledger::Reason,
    span: model::Span,
}

impl Rejected {
    /// Whether `self` and `other` reject one definition alike, their spans
    /// compared by place (`model::Span::same_place`).
    fn same(&self, other: &Self) -> bool {
        self.reason == other.reason && self.span.same_place(&other.span)
    }
}

enum ClosureOutcome {
    Complete,
    Rejected(Box<Rejected>),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct FixedNode {
    first_edge: usize,
    edge_count: usize,
    reachable: bool,
    retained: bool,
    depth: usize,
    rejection_distance: Option<usize>,
    rejection_terminal: Option<usize>,
    rejection_edge: Option<usize>,
}

#[derive(Clone, Copy)]
struct FixedEdge<'candidate> {
    source: usize,
    target: usize,
    span: &'candidate model::Span,
    kind: FixedEdgeKind<'candidate>,
}

#[derive(Clone, Copy)]
enum FixedEdgeKind<'candidate> {
    /// An associated item to the implementation that owns it.
    Admission,
    /// A definition to an implementation that names it (its self type,
    /// its trait or a type in either). A rejected implementation does not
    /// reject the definitions it names: it is unavailable by itself.
    Relevance,
    Support {
        provenance: &'candidate identity::Definition,
        role: ledger::SupportRole,
    },
}

struct FixedClosure {
    definitions: Box<[bool]>,
    implementations: Box<[bool]>,
    outcomes: BTreeMap<identity::Definition, ClosureOutcome>,
    support: BTreeMap<SupportKey, ledger::Support>,
}

#[derive(Clone, Copy)]
enum SealMode<'evidence> {
    Direct,
    #[cfg(feature = "rustc-driver")]
    Evidence(&'evidence crate::process::EvidenceSession),
    #[cfg(not(feature = "rustc-driver"))]
    Marker(std::marker::PhantomData<&'evidence ()>),
}

/// The pre-policy graph after its context and invocations: the part the raw and
/// semantic encodings share, encoded once.
#[derive(Serialize)]
struct RawBody<'a> {
    definitions: &'a [&'a model::Definition],
    rejected: &'a [(&'a identity::Definition, &'a Rejected)],
    exports: &'a [&'a Export],
    anonymous: &'a [&'a ledger::AnonymousImport],
    implementations: &'a [model::Implementation],
    scalar_predicate_proofs: &'a [model::ScalarPredicateProof],
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SupportKey {
    id: identity::Definition,
    provenance: identity::Definition,
    role: ledger::SupportRole,
}

#[expect(
    clippy::large_enum_variant,
    reason = "support closure carries one inline canonical rejection proof; boxing its reason would add allocation and weaken proof locality"
)]
#[cfg(test)]
enum PendingSupport {
    Present {
        id: identity::Definition,
        provenance: identity::Definition,
        role: ledger::SupportRole,
    },
    Rejected {
        id: identity::Definition,
        provenance: identity::Definition,
        role: ledger::SupportRole,
        reason: ledger::Reason,
        span: model::Span,
    },
}

#[cfg(test)]
impl PendingSupport {
    const fn id(&self) -> &identity::Definition {
        match self {
            Self::Present { id, .. } | Self::Rejected { id, .. } => id,
        }
    }

    const fn provenance(&self) -> &identity::Definition {
        match self {
            Self::Present { provenance, .. } | Self::Rejected { provenance, .. } => provenance,
        }
    }
}

pub(crate) struct Allocation {
    items: usize,
    bytes: usize,
    item_limit: usize,
    byte_limit: usize,
    string_limit: usize,
    decoded_documents: usize,
    moved_documents: usize,
    captured_document_nodes: usize,
    evidence: Option<EvidenceCounters>,
    evidence_category: EvidenceCategory,
    /// Set only by genuine allocation failure or checked-arithmetic overflow;
    /// a latched account does no further work.
    refused: bool,
}

const EVIDENCE_CATEGORY_COUNT: usize = 6;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum EvidenceCategory {
    #[default]
    Entry,
    Candidate,
    Raw,
    Closure,
    Retained,
    Graph,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct EvidenceCounters {
    captured_items: usize,
    captured_bytes: usize,
    document_nodes: usize,
    population: protocol::Population,
    candidate_definitions: usize,
    candidate_rejected: usize,
    candidate_implementations: usize,
    candidate_exports: usize,
    candidate_anonymous: usize,
    support_edges: usize,
    queue_entries: usize,
    dedup_entries: usize,
    index_entries: usize,
    sort_keys: usize,
    temporary_conversions: usize,
    decoded_documents: usize,
    moved_documents: usize,
    cloned_ownership: usize,
    closure_computations: usize,
    closure_cache_hits: usize,
    fixed_nodes: usize,
    fixed_edges: usize,
    fixed_targets: usize,
    forward_queue_entries: usize,
    reverse_queue_entries: usize,
    retained_queue_entries: usize,
    final_invocations: usize,
    final_definitions: usize,
    final_exports: usize,
    final_implementations: usize,
    final_support: usize,
    final_anonymous: usize,
    final_graph_items: usize,
    raw_canonical_bytes: usize,
    item_charges: [usize; EVIDENCE_CATEGORY_COUNT],
    byte_charges: [usize; EVIDENCE_CATEGORY_COUNT],
}

impl EvidenceCategory {
    const fn index(self) -> usize {
        self as usize
    }
}

impl Allocation {
    pub(crate) fn new(limits: &cargo::Limits) -> Self {
        Self {
            items: 0,
            bytes: 0,
            item_limit: limits.canonical_items,
            byte_limit: limits.canonical_bytes,
            string_limit: limits.string_bytes,
            decoded_documents: 0,
            moved_documents: 0,
            captured_document_nodes: 0,
            evidence: None,
            evidence_category: EvidenceCategory::Entry,
            refused: false,
        }
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn with_limits(item_limit: usize, byte_limit: usize) -> Self {
        Self {
            items: 0,
            bytes: 0,
            item_limit,
            byte_limit,
            string_limit: byte_limit,
            decoded_documents: 0,
            moved_documents: 0,
            captured_document_nodes: 0,
            evidence: None,
            evidence_category: EvidenceCategory::Entry,
            refused: false,
        }
    }

    pub(crate) const fn usage(&self) -> (usize, usize) {
        (self.items, self.bytes)
    }

    /// Whether a genuine failure has latched this account.
    #[cfg_attr(not(test), allow(dead_code, reason = "latch inspection"))]
    pub(crate) const fn is_refused(&self) -> bool {
        self.refused
    }

    /// A latched account does no further work.
    fn check(&self) -> Result<()> {
        if self.refused {
            Err(refused_account())
        } else {
            Ok(())
        }
    }

    /// Latch this account on a genuine failure (test-only cause injection).
    #[cfg(any(test, feature = "test-fixtures"))]
    #[cfg_attr(not(test), allow(dead_code, reason = "test cause injection"))]
    pub(crate) fn refuse(&mut self) {
        self.refused = true;
    }

    /// A latched account for ordering tests: every charge fails first.
    #[cfg(any(test, feature = "test-fixtures"))]
    #[cfg_attr(not(test), allow(dead_code, reason = "test cause injection"))]
    pub(crate) fn refused() -> Self {
        let mut allocation = Self::with_limits(usize::MAX, usize::MAX);
        allocation.refuse();
        allocation
    }

    /// Checked-arithmetic overflow is a genuine failure: it latches the
    /// account and stays terminal.
    fn overflow(&mut self, operation: &'static str) -> Error {
        self.refused = true;
        self.failure(Error::Node {
            operation,
            source: Box::new(crate::node::Error::Allocation),
        })
    }

    /// Genuine allocator failure latches the account and stays terminal.
    fn allocation_failure(
        &mut self,
        operation: &'static str,
        source: std::collections::TryReserveError,
    ) -> Error {
        self.refused = true;
        self.failure(Error::Allocation { operation, source })
    }

    /// Admit decoder peak work against this same transaction without charging
    /// the peak as exact retained ownership. The caller debits decode work
    /// before entering the allocating decoder, including on malformed input.
    pub(crate) fn admit_decode(&mut self) -> Result<()> {
        self.check()?;
        Ok(())
    }

    /// A latched account refuses the retained graph.
    pub(crate) fn validate_retained(&self) -> Result<()> {
        self.check()?;
        Ok(())
    }

    pub(crate) fn failure(&self, error: Error) -> Error {
        self.failure_at(
            crate::metrics::accounting::SemanticStage::ApiTransaction,
            error,
        )
    }

    /// Attribute a retained-stage failure (a genuine or correctness failure)
    /// to this account's retained stage.
    pub(crate) fn retained_failure(&self, error: Error) -> Error {
        self.failure_at(
            crate::metrics::accounting::SemanticStage::ApiRetained,
            error,
        )
    }

    fn failure_at(&self, _stage: crate::metrics::accounting::SemanticStage, error: Error) -> Error {
        error
    }

    fn into_evidence(
        mut self,
        limits: &cargo::Limits,
        population: protocol::Population,
    ) -> Result<Self> {
        if self.item_limit != limits.canonical_items
            || self.byte_limit != limits.canonical_bytes
            || self.string_limit != limits.string_bytes
        {
            return Err(extraction(
                "canonical-evidence-authority-mismatch",
                "canonical evidence must descend from the normal capture allocation",
            ));
        }
        self.evidence = Some(EvidenceCounters {
            captured_items: self.items,
            captured_bytes: self.bytes,
            document_nodes: self.captured_document_nodes,
            population,
            decoded_documents: self.decoded_documents,
            moved_documents: self.moved_documents,
            ..EvidenceCounters::default()
        });
        self.item_limit = CANONICAL_EVIDENCE_ITEMS;
        self.byte_limit = CANONICAL_EVIDENCE_BYTES;
        Ok(self)
    }

    pub(crate) fn captured_document(&mut self, nodes: usize) {
        self.decoded_documents = self.decoded_documents.saturating_add(1);
        self.captured_document_nodes = self.captured_document_nodes.saturating_add(nodes);
    }

    pub(crate) fn moved_document(&mut self) {
        self.moved_documents = self.moved_documents.saturating_add(1);
    }

    fn category(&mut self, category: EvidenceCategory) {
        self.evidence_category = category;
    }

    fn observe(&mut self, observe: impl FnOnce(&mut EvidenceCounters)) {
        if let Some(evidence) = &mut self.evidence {
            observe(evidence);
        }
    }

    pub(crate) fn items(&mut self, amount: usize) -> Result<()> {
        self.check()?;
        let Some(actual) = self.items.checked_add(amount) else {
            return Err(self.overflow("canonical allocation items"));
        };
        self.items = actual;
        let category = self.evidence_category.index();
        self.observe(|evidence| {
            evidence.item_charges[category] =
                evidence.item_charges[category].saturating_add(amount);
        });
        Ok(())
    }

    pub(crate) fn bytes(&mut self, amount: usize) -> Result<()> {
        self.check()?;
        let Some(actual) = self.bytes.checked_add(amount) else {
            return Err(self.overflow("canonical allocation bytes"));
        };
        self.bytes = actual;
        let category = self.evidence_category.index();
        self.observe(|evidence| {
            evidence.byte_charges[category] =
                evidence.byte_charges[category].saturating_add(amount);
        });
        Ok(())
    }

    fn retain(&mut self, usage: PolicyUsage) -> Result<()> {
        self.check()?;
        let Some(items) = self.items.checked_add(usage.items) else {
            return Err(self.overflow("canonical allocation items"));
        };
        let Some(bytes) = self.bytes.checked_add(usage.bytes) else {
            return Err(self.overflow("canonical allocation bytes"));
        };
        self.items = items;
        self.bytes = bytes;
        let category = self.evidence_category.index();
        self.observe(|evidence| {
            evidence.item_charges[category] =
                evidence.item_charges[category].saturating_add(usage.items);
            evidence.byte_charges[category] =
                evidence.byte_charges[category].saturating_add(usage.bytes);
        });
        Ok(())
    }

    pub(crate) fn string(&mut self, value: &str) -> Result<()> {
        self.check()?;
        self.items(1)?;
        self.bytes(value.len())
    }

    pub(crate) fn owned(&mut self, value: &str) -> Result<Box<str>> {
        self.string(value)?;
        let mut owned = String::new();
        if let Err(error) = owned.try_reserve_exact(value.len()) {
            return Err(self.allocation_failure("owned string allocation", error));
        }
        owned.push_str(value);
        Ok(owned.into_boxed_str())
    }

    pub(crate) fn hex(&mut self, bytes: impl AsRef<[u8]>) -> Result<Box<str>> {
        let bytes = bytes.as_ref();
        let length = bytes.len().saturating_mul(2);
        self.check()?;
        self.items(1)?;
        self.bytes(length)?;
        let mut encoded = String::new();
        if let Err(error) = encoded.try_reserve_exact(length) {
            return Err(self.allocation_failure("hexadecimal allocation", error));
        }
        for byte in bytes {
            use std::fmt::Write as _;
            let _ = write!(encoded, "{byte:02x}");
        }
        Ok(encoded.into_boxed_str())
    }

    pub(crate) fn reserve<T>(&mut self, values: &mut Vec<T>, amount: usize) -> Result<()> {
        self.items(amount)?;
        match values.try_reserve_exact(amount) {
            Ok(()) => Ok(()),
            Err(error) => Err(self.allocation_failure("owned container allocation", error)),
        }
    }

    pub(crate) fn path(&mut self, value: &Path) -> Result<()> {
        self.check()?;
        self.items(1)?;
        self.bytes(value.as_os_str().len())
    }

    pub(crate) fn clone_path(&mut self, value: &Path) -> Result<PathBuf> {
        self.path(value)?;
        self.observe(|evidence| {
            evidence.cloned_ownership = evidence.cloned_ownership.saturating_add(1);
        });
        Ok(value.to_path_buf())
    }

    pub(crate) fn path_capacity(&mut self, maximum: usize) -> Result<()> {
        self.check()?;
        self.items(1)?;
        self.bytes(maximum)
    }

    /// `maximum` is the path capacity this account charged; a longer
    /// path is recorded against it, never refused, and the bytes beyond the
    /// precharge are charged so the account measures the full path length.
    pub(crate) fn validate_path(&mut self, value: &Path, maximum: usize) -> Result<()> {
        self.check()?;
        let length = value.as_os_str().len();
        if let Some(extra) = length.checked_sub(maximum).filter(|extra| *extra != 0) {
            self.bytes(extra)?;
        }
        Ok(())
    }

    fn identity(&mut self, value: &identity::Definition) -> Result<()> {
        self.items(1)?;
        self.unit(value.unit())?;
        self.string(value.def_path_hash())
    }

    fn unit(&mut self, unit: &identity::Unit) -> Result<()> {
        self.items(2)?;
        for text in [
            unit.target(),
            unit.target_kind(),
            unit.crate_name(),
            unit.stable_crate_id(),
        ] {
            self.string(text)?;
        }
        self.items(1)?;
        for feature in unit.features() {
            self.string(feature)?;
        }
        match unit.authority() {
            identity::Authority::Cargo(package) => {
                self.items(1)?;
                for text in [
                    package.cargo_id(),
                    package.name(),
                    package.version(),
                    package.source(),
                ] {
                    self.string(text)?;
                }
            }
            identity::Authority::Sysroot(sysroot) => {
                self.items(1)?;
                for text in [
                    sysroot.compiler_commit(),
                    sysroot.selected_target(),
                    sysroot.crate_name(),
                    sysroot.stable_crate_id(),
                ] {
                    self.string(text)?;
                }
            }
        }
        Ok(())
    }

    fn clone_identity(&mut self, value: &identity::Definition) -> Result<identity::Definition> {
        self.identity(value)?;
        self.observe(|evidence| {
            evidence.cloned_ownership = evidence.cloned_ownership.saturating_add(1);
        });
        Ok(value.clone())
    }

    fn span(&mut self, value: &model::Span) -> Result<()> {
        self.items(1)?;
        self.string(value.file())
    }

    fn clone_span(&mut self, value: &model::Span) -> Result<model::Span> {
        self.span(value)?;
        self.observe(|evidence| {
            evidence.cloned_ownership = evidence.cloned_ownership.saturating_add(1);
        });
        Ok(value.clone())
    }

    fn reason(&mut self, value: &ledger::Reason) -> Result<()> {
        self.items(1)?;
        match value {
            ledger::Reason::ExternalAuthorityUnavailable {
                crate_name,
                stable_crate_id,
            } => {
                self.string(crate_name)?;
                self.string(stable_crate_id)
            }
            ledger::Reason::UnnameablePrivate(id) | ledger::Reason::UnresolvedDefinition(id) => {
                self.identity(id)
            }
            ledger::Reason::UnknownCompilerShape {
                definition,
                compiler_kind,
            } => {
                self.identity(definition)?;
                self.string(compiler_kind)
            }
            ledger::Reason::UnsupportedPublicPath { compiler_kind } => self.string(compiler_kind),
            ledger::Reason::InvalidAliasChain | ledger::Reason::ConflictingIdentity => Ok(()),
        }
    }

    fn clone_reason(&mut self, value: &ledger::Reason) -> Result<ledger::Reason> {
        self.reason(value)?;
        self.observe(|evidence| {
            evidence.cloned_ownership = evidence.cloned_ownership.saturating_add(1);
        });
        Ok(value.clone())
    }

    fn clone_rejected(&mut self, value: &Rejected) -> Result<Rejected> {
        self.items(1)?;
        Ok(Rejected {
            reason: self.clone_reason(&value.reason)?,
            span: self.clone_span(&value.span)?,
        })
    }

    fn protocol_id(&mut self, value: &protocol::Id) -> Result<()> {
        self.items(2)?;
        let protocol::Authority::Compiler {
            crate_name,
            stable_crate_id,
        } = &value.authority;
        self.string(crate_name)?;
        self.string(stable_crate_id)?;
        self.string(&value.def_path_hash)
    }

    fn protocol_span(&mut self, value: &protocol::Span) -> Result<()> {
        self.items(1)?;
        self.string(&value.file)
    }

    fn candidate_population(
        &mut self,
        definitions: usize,
        rejected: usize,
        implementations: usize,
        exports: usize,
        anonymous: usize,
    ) {
        self.observe(|evidence| {
            evidence.candidate_definitions = definitions;
            evidence.candidate_rejected = rejected;
            evidence.candidate_implementations = implementations;
            evidence.candidate_exports = exports;
            evidence.candidate_anonymous = anonymous;
        });
    }

    fn queue_entries(&mut self, amount: usize) {
        self.observe(|evidence| {
            evidence.queue_entries = evidence.queue_entries.saturating_add(amount);
        });
    }

    fn dedup_entries(&mut self, amount: usize) {
        self.observe(|evidence| {
            evidence.dedup_entries = evidence.dedup_entries.saturating_add(amount);
        });
    }

    fn index_entries(&mut self, amount: usize) {
        self.observe(|evidence| {
            evidence.index_entries = evidence.index_entries.saturating_add(amount);
        });
    }

    pub(crate) fn sort_keys(&mut self, amount: usize) {
        self.observe(|evidence| {
            evidence.sort_keys = evidence.sort_keys.saturating_add(amount);
        });
    }

    pub(crate) fn temporary_conversions(&mut self, amount: usize) {
        self.observe(|evidence| {
            evidence.temporary_conversions = evidence.temporary_conversions.saturating_add(amount);
        });
    }

    fn closure_computation(&mut self) {
        self.observe(|evidence| {
            evidence.closure_computations = evidence.closure_computations.saturating_add(1);
        });
    }

    fn closure_cache_hit(&mut self) {
        self.observe(|evidence| {
            evidence.closure_cache_hits = evidence.closure_cache_hits.saturating_add(1);
        });
    }

    fn fixed_population(&mut self, nodes: usize, edges: usize, targets: usize) {
        self.observe(|evidence| {
            evidence.fixed_nodes = nodes;
            evidence.fixed_edges = edges;
            evidence.fixed_targets = targets;
        });
    }

    fn fixed_queue(&mut self, forward: usize, reverse: usize, retained: usize) {
        self.observe(|evidence| {
            evidence.forward_queue_entries = forward;
            evidence.reverse_queue_entries = reverse;
            evidence.retained_queue_entries = retained;
        });
    }

    fn cloned_ownership(&mut self, amount: usize) {
        self.observe(|evidence| {
            evidence.cloned_ownership = evidence.cloned_ownership.saturating_add(amount);
        });
    }

    fn support_edges(&mut self, amount: usize) {
        self.observe(|evidence| evidence.support_edges = amount);
    }

    fn raw_canonical_bytes(&mut self, amount: usize) {
        self.observe(|evidence| evidence.raw_canonical_bytes = amount);
    }

    pub(crate) fn final_graph(
        &mut self,
        invocations: usize,
        definitions: usize,
        exports: usize,
        implementations: usize,
        support: usize,
        anonymous: usize,
    ) {
        self.observe(|evidence| {
            evidence.final_invocations = invocations;
            evidence.final_definitions = definitions;
            evidence.final_exports = exports;
            evidence.final_implementations = implementations;
            evidence.final_support = support;
            evidence.final_anonymous = anonymous;
            evidence.final_graph_items = invocations
                .saturating_add(definitions)
                .saturating_add(exports)
                .saturating_add(implementations)
                .saturating_add(support)
                .saturating_add(anonymous);
        });
    }

    #[cfg(feature = "rustc-driver")]
    fn report_evidence(&self, stages: crate::process::EvidenceStages) {
        let Some(evidence) = self.evidence else {
            return;
        };
        eprintln!(
            "CANONICAL_DECOMPOSITION captured_items={} captured_bytes={} document_nodes={} fragments={} document_definitions={} document_implementations={} document_exports={} document_anonymous={} owned_type_nodes={} owned_predicate_nodes={} owned_generic_nodes={} instantiation_nodes={} unique_complete_definitions={} unique_rejected_definitions={} unique_implementations={} unique_exports={} unique_anonymous_imports={} unique_support_edges={} queue_entries={} dedup_entries={} index_entries={} sort_keys={} temp_conversion_entries={} decoded_documents={} moved_documents={} accounted_clones={} closure_computations={} closure_cache_hits={} fixed_nodes={} fixed_edges={} fixed_targets={} forward_queue={} reverse_queue={} retained_queue={} final_invocations={} final_definitions={} final_exports={} final_implementations={} final_support={} final_anonymous={} final_graph_items={} canonical_encoded_bytes={} total_items={} total_bytes={}",
            evidence.captured_items,
            evidence.captured_bytes,
            evidence.document_nodes,
            evidence.population.fragments,
            evidence.population.definitions,
            evidence.population.implementations,
            evidence.population.exports,
            evidence.population.anonymous,
            evidence.population.types,
            evidence.population.predicates,
            evidence.population.generics,
            evidence.population.instantiations,
            evidence.candidate_definitions,
            evidence.candidate_rejected,
            evidence.candidate_implementations,
            evidence.candidate_exports,
            evidence.candidate_anonymous,
            evidence.support_edges,
            evidence.queue_entries,
            evidence.dedup_entries,
            evidence.index_entries,
            evidence.sort_keys,
            evidence.temporary_conversions,
            evidence.decoded_documents,
            evidence.moved_documents,
            evidence.cloned_ownership,
            evidence.closure_computations,
            evidence.closure_cache_hits,
            evidence.fixed_nodes,
            evidence.fixed_edges,
            evidence.fixed_targets,
            evidence.forward_queue_entries,
            evidence.reverse_queue_entries,
            evidence.retained_queue_entries,
            evidence.final_invocations,
            evidence.final_definitions,
            evidence.final_exports,
            evidence.final_implementations,
            evidence.final_support,
            evidence.final_anonymous,
            evidence.final_graph_items,
            evidence.raw_canonical_bytes,
            self.items,
            self.bytes,
        );
        eprintln!(
            "CANONICAL_CHARGES entry_items={} candidate_items={} raw_items={} closure_items={} retained_items={} graph_items={} entry_bytes={} candidate_bytes={} raw_bytes={} closure_bytes={} retained_bytes={} graph_bytes={}",
            evidence.item_charges[EvidenceCategory::Entry.index()],
            evidence.item_charges[EvidenceCategory::Candidate.index()],
            evidence.item_charges[EvidenceCategory::Raw.index()],
            evidence.item_charges[EvidenceCategory::Closure.index()],
            evidence.item_charges[EvidenceCategory::Retained.index()],
            evidence.item_charges[EvidenceCategory::Graph.index()],
            evidence.byte_charges[EvidenceCategory::Entry.index()],
            evidence.byte_charges[EvidenceCategory::Candidate.index()],
            evidence.byte_charges[EvidenceCategory::Raw.index()],
            evidence.byte_charges[EvidenceCategory::Closure.index()],
            evidence.byte_charges[EvidenceCategory::Retained.index()],
            evidence.byte_charges[EvidenceCategory::Graph.index()],
        );
        for (name, stage) in [
            ("build", stages.build),
            ("probe_traversal", stages.probe),
            ("decode", stages.decode),
            ("canonicalization", stages.canonicalization),
            ("graph_sealing", stages.graph_sealing),
        ] {
            eprintln!(
                "CANONICAL_STAGE stage={} invocations={} elapsed_ms={}",
                name,
                stage.invocations(),
                stage.elapsed().as_millis(),
            );
        }
    }

    #[cfg(test)]
    fn evidence_counters(&self) -> Option<EvidenceCounters> {
        self.evidence
    }
}

enum ShapeFailure {
    Unknown(Box<str>),
    UnnameablePrivate,
    ExternalAuthorityUnavailable {
        crate_name: Box<str>,
        stable_crate_id: Box<str>,
    },
    Fatal(Error),
}

impl From<Error> for ShapeFailure {
    fn from(error: Error) -> Self {
        Self::Fatal(error)
    }
}

impl Invocations {
    fn seal_document_with_allocation(
        document: &protocol::Document,
        mut allocation: Allocation,
    ) -> Result<Self> {
        let mut correlations = BTreeSet::<&str>::new();
        let mut nonces = BTreeSet::<&str>::new();
        let mut aliases = BTreeMap::<&str, &str>::new();
        for fragment in &document.fragments {
            let handshake = &fragment.handshake;
            validate_handshake(handshake)?;
            if correlations.contains(handshake.unit_correlation.as_str()) {
                return Err(extraction(
                    "duplicate-unit-correlation",
                    &handshake.unit_correlation,
                ));
            }
            allocation.items(1)?;
            correlations.insert(&handshake.unit_correlation);
            if nonces.contains(handshake.invocation_nonce.as_str()) {
                return Err(extraction(
                    "duplicate-invocation-nonce",
                    &handshake.invocation_nonce,
                ));
            }
            allocation.items(1)?;
            nonces.insert(&handshake.invocation_nonce);
            let protocol::Root::Selected { aliases: roots } = &handshake.root;
            for alias in roots {
                if aliases.contains_key(alias.as_str()) {
                    return Err(extraction("ambiguous-selected-alias", alias));
                }
                allocation.items(1)?;
                aliases.insert(alias, &handshake.unit_correlation);
            }
        }
        if correlations.is_empty() {
            return Err(extraction(
                "missing-unit-correlation",
                "no selected normal library units were expected",
            ));
        }
        if aliases.is_empty() {
            return Err(extraction(
                "missing-selected-root",
                "the sidecar set contains support units but no selected root",
            ));
        }
        Ok(Self {
            authority: InvocationAuthority {
                #[cfg(test)]
                expected: BTreeMap::new(),
                external_units: BTreeMap::new(),
            },
            allocation,
        })
    }

    #[cfg(test)]
    pub(crate) fn seal(expected: Vec<protocol::Handshake>, limits: &cargo::Limits) -> Result<Self> {
        Self::seal_with_allocation(expected, Allocation::new(limits))
    }

    #[cfg(test)]
    fn seal_with_allocation(
        expected: Vec<protocol::Handshake>,
        mut allocation: Allocation,
    ) -> Result<Self> {
        let mut sealed = BTreeMap::new();
        let mut nonces = BTreeSet::<Box<str>>::new();
        let mut aliases = BTreeMap::<Box<str>, Box<str>>::new();
        for handshake in expected {
            validate_handshake(&handshake)?;
            if sealed.contains_key(handshake.unit_correlation.as_str()) {
                return Err(extraction(
                    "duplicate-unit-correlation",
                    &handshake.unit_correlation,
                ));
            }
            if nonces.contains(handshake.invocation_nonce.as_str()) {
                return Err(extraction(
                    "duplicate-invocation-nonce",
                    &handshake.invocation_nonce,
                ));
            }
            let correlation = allocation.owned(&handshake.unit_correlation)?;
            allocation.items(1)?;
            nonces.insert(allocation.owned(&handshake.invocation_nonce)?);
            let protocol::Root::Selected { aliases: roots } = &handshake.root;
            for alias in roots {
                if aliases.contains_key(alias.as_str()) {
                    return Err(extraction("ambiguous-selected-alias", alias));
                }
                allocation.items(1)?;
                aliases.insert(allocation.owned(alias)?, allocation.owned(&correlation)?);
            }
            allocation.items(1)?;
            sealed.insert(correlation, handshake);
        }
        if sealed.is_empty() {
            return Err(extraction(
                "missing-unit-correlation",
                "no selected normal library units were expected",
            ));
        }
        if aliases.is_empty() {
            return Err(extraction(
                "missing-selected-root",
                "the sidecar set contains support units but no selected root",
            ));
        }
        Ok(Self {
            authority: InvocationAuthority {
                expected: sealed,
                external_units: BTreeMap::new(),
            },
            allocation,
        })
    }

    /// Each external crate the probe named, with the unit that compiled it.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn external_records(&self) -> Vec<crate::bindings::External> {
        self.authority
            .external_units
            .iter()
            .map(|(key, authority)| crate::bindings::External {
                crate_name: key.crate_name.to_string(),
                stable_crate_id: key.stable_crate_id.to_string(),
                unit_key: match authority {
                    ExternalAuthority::Available(unit) => Some(unit.unit_key.to_string()),
                    ExternalAuthority::Unavailable { .. } => None,
                },
            })
            .collect()
    }

    #[cfg(feature = "rustc-driver")]
    fn bind_external(mut self, values: Vec<ExternalBinding>) -> Result<Self> {
        for value in values {
            let ExternalBinding {
                crate_name,
                stable_crate_id,
                artifacts,
                authority,
                package,
            } = value;
            let key = CompilerKey {
                crate_name,
                stable_crate_id,
            };
            let authority = match authority {
                // A proc-macro crate (a crate re-exporting derives, such as
                // palette's) has no items Rawr can call: its macros expand
                // in Rust. Its exports stay unavailable.
                Some(authority)
                    if authority.crate_name.as_ref() == key.crate_name.as_ref()
                        && authority
                            .target_kinds
                            .iter()
                            .any(|kind| kind.as_ref() == "proc-macro") =>
                {
                    ExternalAuthority::Unavailable { artifacts }
                }
                Some(authority) => {
                    if authority.crate_name.as_ref() != key.crate_name.as_ref() {
                        return Err(extraction(
                            "external-authority-crate-mismatch",
                            &key.crate_name,
                        ));
                    } else {
                        let target =
                            self.allocation.owned(authority.target.to_str().ok_or_else(
                                || extraction("non-utf8-external-target", &authority.package_id),
                            )?)?;
                        if authority.crate_types.is_empty()
                            || !authority
                                .target_kinds
                                .iter()
                                .any(|kind| matches!(kind.as_ref(), "lib" | "rlib"))
                        {
                            return Err(extraction(
                                "external-artifact-not-library",
                                &authority.package_id,
                            ));
                        }
                        let (package_name, package_version, package_source) =
                            package.ok_or_else(|| {
                                extraction("missing-external-package", &key.crate_name)
                            })?;
                        self.allocation.items(1)?;
                        for value in [
                            authority.semantic_key.as_ref(),
                            authority.package_id.as_ref(),
                            package_name.as_ref(),
                            package_version.as_ref(),
                            package_source.as_ref(),
                            authority.crate_name.as_ref(),
                        ] {
                            self.allocation.string(value)?;
                        }
                        self.allocation.items(authority.target_kinds.len())?;
                        for value in &authority.target_kinds {
                            self.allocation.string(value)?;
                        }
                        self.allocation.items(authority.crate_types.len())?;
                        for value in &authority.crate_types {
                            self.allocation.string(value)?;
                        }
                        self.allocation.items(authority.features.len())?;
                        for value in &authority.features {
                            self.allocation.string(value)?;
                        }
                        ExternalAuthority::Available(ExternalUnit {
                            unit_key: authority.semantic_key,
                            package_id: authority.package_id,
                            package_name,
                            package_version,
                            package_source,
                            target,
                            target_kinds: authority.target_kinds,
                            crate_types: authority.crate_types,
                            crate_name: authority.crate_name,
                            features: authority.features,
                        })
                    }
                }
                None => ExternalAuthority::Unavailable { artifacts },
            };
            match self.authority.external_units.get(&key) {
                Some(previous) if previous != &authority => {
                    return Err(extraction(
                        "split-external-compiler-authority",
                        &key.crate_name,
                    ));
                }
                Some(_) => {}
                None => {
                    self.allocation.items(1)?;
                    self.authority.external_units.insert(key, authority);
                }
            }
        }
        Ok(self)
    }
}

impl Policy {
    /// API policy is independent of the Rust owner, but controls the extracted
    /// surface and its diagnostics. Frame every field without retaining a copy.
    pub(crate) fn cache_identity(&self) -> Result<[u8; 32]> {
        use sha2::{Digest as _, Sha256};
        let mut digest = Sha256::new();
        let mut field = |bytes: &[u8]| -> Result<()> {
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
            Ok(())
        };
        field(b"rawr.api.policy/v1")?;
        field(&(self.exclusions.len() as u64).to_le_bytes())?;
        for (alias, exclusions) in &self.exclusions {
            field(alias.as_bytes())?;
            field(&(exclusions.len() as u64).to_le_bytes())?;
            for (path, exclusion) in exclusions {
                field(&(path.len() as u64).to_le_bytes())?;
                for component in path {
                    field(component.as_bytes())?;
                }
                field(exclusion.reason.as_bytes())?;
                field(exclusion.span.file().as_bytes())?;
                let range = exclusion.span.byte_range();
                let (start, end) = exclusion.span.positions();
                for position in [range.start, range.end, start.0, start.1, end.0, end.1] {
                    field(&position.to_le_bytes())?;
                }
            }
        }
        Ok(digest.finalize().into())
    }

    /// Begin the manifest-policy transformation.
    pub fn manifest() -> ManifestPolicyBuilder {
        ManifestPolicyBuilder {
            exclusions: BTreeMap::new(),
            items: 0,
            bytes: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            exclusions: BTreeMap::new(),
            usage: PolicyUsage::default(),
        }
    }

    #[cfg(test)]
    fn excluding(key: Key, reason: &str, span: model::Span) -> Self {
        Self {
            exclusions: BTreeMap::from([(
                key.alias,
                BTreeMap::from([(
                    key.path,
                    Exclusion {
                        reason: reason.into(),
                        span,
                    },
                )]),
            )]),
            usage: PolicyUsage::default(),
        }
    }
}

impl ManifestPolicyBuilder {
    /// One manifest string: nonempty is correctness; its size is a target.
    fn string(&mut self, field: &'static str, value: &str) -> Result<()> {
        present(field, value)
    }

    /// Measure the prospective peak of retained plus transient ownership
    /// against the manifest policy targets; checked overflow is terminal.
    fn peak(
        &mut self,
        retained_items: usize,
        retained_bytes: usize,
        transient_items: usize,
        transient_bytes: usize,
    ) -> Result<()> {
        retained_items
            .checked_add(transient_items)
            .ok_or_else(|| arithmetic("manifest policy peak items"))?;
        retained_bytes
            .checked_add(transient_bytes)
            .ok_or_else(|| arithmetic("manifest policy peak bytes"))?;
        Ok(())
    }

    /// Validate and retain one borrowed manifest exclusion under the bound limits.
    pub fn exclusion<'manifest, I>(
        mut self,
        alias: &str,
        path: I,
        reason: &str,
        span: ManifestSpan<'manifest>,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = &'manifest str>,
    {
        self.string("exclusion alias", alias)?;
        self.string("exclusion reason", reason)?;
        self.string("manifest span file", span.file)?;
        let mut borrowed = Vec::new();
        for part in path {
            self.string("exclusion path component", part)?;
            let next = borrowed.len().saturating_add(1);
            let next_capacity = if next > borrowed.capacity() {
                next
            } else {
                borrowed.capacity()
            };
            let pointer_bytes = next_capacity
                .checked_mul(std::mem::size_of::<&str>())
                .ok_or_else(|| arithmetic("manifest exclusion borrowed path bytes"))?;
            self.peak(self.items, self.bytes, next, pointer_bytes)?;
            if next > borrowed.capacity() {
                borrowed
                    .try_reserve_exact(next - borrowed.len())
                    .map_err(|error| Error::Allocation {
                        operation: "manifest exclusion borrow allocation",
                        source: error,
                    })?;
                let actual_pointer_bytes = borrowed
                    .capacity()
                    .checked_mul(std::mem::size_of::<&str>())
                    .ok_or_else(|| arithmetic("manifest exclusion borrowed path bytes"))?;
                self.peak(self.items, self.bytes, next, actual_pointer_bytes)?;
            }
            borrowed.push(part);
            #[cfg(test)]
            POLICY_BORROWED_PATH_OWNERSHIPS.with(|count| count.set(count.get().saturating_add(1)));
        }
        if borrowed.is_empty() {
            return Err(extraction(
                "empty-exclusion-path",
                "an exclusion requires a path after its dependency alias",
            ));
        }
        if self.exclusions.get(alias).is_some_and(|paths| {
            paths.keys().any(|retained| {
                retained.len() == borrowed.len()
                    && retained
                        .iter()
                        .zip(&borrowed)
                        .all(|(left, right)| left.as_ref() == *right)
            })
        }) {
            return Err(extraction(
                "duplicate-exclusion-path",
                "the manifest repeats one exclusion path",
            ));
        }
        let new_alias = !self.exclusions.contains_key(alias);
        let owned_items = borrowed
            .len()
            .checked_add(4)
            .and_then(|items| items.checked_add(usize::from(new_alias)))
            .ok_or_else(|| arithmetic("manifest policy items"))?;
        let path_bytes = borrowed.iter().try_fold(0_usize, |bytes, part| {
            bytes
                .checked_add(part.len())
                .ok_or_else(|| arithmetic("manifest policy bytes"))
        })?;
        let owned_bytes = path_bytes
            .checked_add(reason.len())
            .and_then(|bytes| bytes.checked_add(span.file.len()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Exclusion>()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Box<str>>() * borrowed.len()))
            .and_then(|bytes| bytes.checked_add(if new_alias { alias.len() } else { 0 }))
            .ok_or_else(|| arithmetic("manifest policy bytes"))?;
        let next_items = self
            .items
            .checked_add(owned_items)
            .ok_or_else(|| arithmetic("manifest policy items"))?;
        let next_bytes = self
            .bytes
            .checked_add(owned_bytes)
            .ok_or_else(|| arithmetic("manifest policy bytes"))?;
        let borrowed_pointer_bytes = borrowed
            .capacity()
            .checked_mul(std::mem::size_of::<&str>())
            .ok_or_else(|| arithmetic("manifest exclusion borrowed path bytes"))?;
        // The final retained node/string/path usage is measured while the full
        // borrowed path Vec is still live.  This single prospective peak is
        // stronger than every partial conversion state below; the transient
        // portion is released rather than transferred into `PolicyUsage`.
        self.peak(
            next_items,
            next_bytes,
            borrowed.len(),
            borrowed_pointer_bytes,
        )?;
        let mut owned_path = Vec::new();
        owned_path
            .try_reserve_exact(borrowed.len())
            .map_err(|error| Error::Allocation {
                operation: "manifest exclusion allocation",
                source: error,
            })?;
        #[cfg(test)]
        POLICY_OWNED_PATH_RESERVES.with(|count| count.set(count.get().saturating_add(1)));
        for part in borrowed {
            owned_path.push(own_policy_string(part)?);
        }
        self.items = next_items;
        self.bytes = next_bytes;
        let path = owned_path.into_boxed_slice();
        let exclusion = Exclusion {
            reason: own_policy_string(reason)?,
            span: span.owned()?,
        };
        match self.exclusions.get_mut(alias) {
            Some(paths) => {
                paths.insert(path, exclusion);
            }
            None => {
                self.exclusions.insert(
                    own_policy_string(alias)?,
                    BTreeMap::from([(path, exclusion)]),
                );
            }
        }
        Ok(self)
    }

    /// Seal the completed policy after every ownership charge has succeeded.
    pub fn seal(self) -> Policy {
        Policy {
            exclusions: self.exclusions,
            usage: PolicyUsage {
                items: self.items,
                bytes: self.bytes,
            },
        }
    }
}

impl<'manifest> ManifestSpan<'manifest> {
    /// Seal one manifest byte and line/column range.
    pub fn new(
        file: &'manifest str,
        byte_start: usize,
        byte_end: usize,
        line_start: usize,
        column_start: usize,
        line_end: usize,
        column_end: usize,
    ) -> Result<Self> {
        present("manifest span file", file)?;
        if byte_start > byte_end || line_start == 0 || line_end == 0 {
            return Err(extraction(
                "invalid-manifest-span",
                "manifest exclusion span is malformed",
            ));
        }
        let convert = |value: usize| {
            u32::try_from(value).map_err(|_| {
                extraction(
                    "manifest-span-overflow",
                    "manifest exclusion span exceeds the canonical range",
                )
            })
        };
        Ok(Self {
            file,
            byte_start: convert(byte_start)?,
            byte_end: convert(byte_end)?,
            line_start: convert(line_start)?,
            column_start: convert(column_start)?,
            line_end: convert(line_end)?,
            column_end: convert(column_end)?,
        })
    }

    fn owned(self) -> Result<model::Span> {
        Ok(model::Span::new(
            own_policy_string(self.file)?,
            self.byte_start,
            self.byte_end,
            self.line_start,
            self.column_start,
            self.line_end,
            self.column_end,
        ))
    }
}

fn own_policy_string(value: &str) -> Result<Box<str>> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|error| Error::Allocation {
            operation: "manifest policy string allocation",
            source: error,
        })?;
    owned.push_str(value);
    #[cfg(test)]
    POLICY_STRING_OWNERSHIPS.with(|count| count.set(count.get().saturating_add(1)));
    Ok(owned.into_boxed_str())
}

#[cfg(test)]
thread_local! {
    static POLICY_STRING_OWNERSHIPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static POLICY_BORROWED_PATH_OWNERSHIPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static POLICY_OWNED_PATH_RESERVES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn canonicalize(
    bytes: &[u8],
    invocations: Invocations,
    policy: Policy,
    context: &api::Context,
    limits: &cargo::Limits,
) -> Result<api::Graph<api::Captured>> {
    let Invocations {
        authority,
        mut allocation,
    } = invocations;
    allocation.retain(policy.usage)?;
    let document = protocol::decode_accounted(bytes, limits, &mut allocation)?;
    let fragments = correlate(document.fragments, &authority, &mut allocation)?;
    canonicalize_fragments(
        fragments,
        authority,
        &policy,
        context,
        limits,
        &mut allocation,
        SealMode::Direct,
    )
}

/// Both paths consume the same verified document and original transaction.
/// Stored bodies carry no authority for the exact current compiler header.
#[cfg(feature = "rustc-driver")]
fn materialize_document(
    stored: Option<&api::cached::Product>,
    document: protocol::Document,
    invocations: Invocations,
    policy: &Policy,
    context: &api::Context,
    limits: &cargo::Limits,
) -> Result<api::Graph<api::Captured>> {
    let Some(stored) = stored else {
        return canonicalize_document_borrowed(document, invocations, policy, context, limits);
    };
    let Invocations {
        authority: _,
        mut allocation,
    } = invocations;
    allocation.retain(policy.usage)?;
    let mut current = Vec::new();
    allocation.reserve(&mut current, document.fragments.len())?;
    for fragment in &document.fragments {
        current.push(invocation(fragment, &mut allocation)?);
    }
    allocation.sort_keys(current.len());
    current.sort_unstable();
    stored.replay(context, current, limits, &mut allocation)
}

#[cfg(feature = "rustc-driver")]
fn inventory_id(
    raw: &engine::model::Id,
    catalog: &Catalog,
    allocation: &mut Allocation,
) -> Result<identity::Definition> {
    let engine::model::Authority::Compiler {
        crate_name,
        stable_crate_id,
    } = &raw.authority;
    let raw = protocol::Id {
        authority: protocol::Authority::Compiler {
            crate_name: crate_name.clone(),
            stable_crate_id: stable_crate_id.clone(),
        },
        def_path_hash: raw.def_path_hash.clone(),
        namespace: match raw.namespace {
            engine::model::Namespace::Type => protocol::Namespace::Type,
            engine::model::Namespace::Value => protocol::Namespace::Value,
            engine::model::Namespace::Macro => protocol::Namespace::Macro,
        },
    };
    allocation.protocol_id(&raw)?;
    catalog.id_bounded(raw, allocation)
}

#[cfg(feature = "rustc-driver")]
fn inventory_ids(
    raw: &[engine::model::Id],
    catalog: &Catalog,
    allocation: &mut Allocation,
) -> Result<Box<[identity::Definition]>> {
    let mut ids = Vec::new();
    allocation.reserve(&mut ids, raw.len())?;
    for raw in raw {
        ids.push(inventory_id(raw, catalog, allocation)?);
    }
    Ok(ids.into_boxed_slice())
}

#[cfg(feature = "rustc-driver")]
fn inventory_associations(
    raw: &[engine::model::Association],
    catalog: &Catalog,
    allocation: &mut Allocation,
) -> Result<Box<[crate::applicability::inventory::Association]>> {
    let mut links = Vec::new();
    allocation.reserve(&mut links, raw.len())?;
    for raw in raw {
        links.push(crate::applicability::inventory::Association {
            implementation: inventory_id(&raw.implementation, catalog, allocation)?,
            item: inventory_id(&raw.item, catalog, allocation)?,
            declaration: raw
                .declaration
                .as_ref()
                .map(|id| inventory_id(id, catalog, allocation))
                .transpose()?,
        });
    }
    Ok(links.into_boxed_slice())
}

fn canonicalize_document(
    document: protocol::Document,
    invocations: Invocations,
    policy: Policy,
    context: &api::Context,
    limits: &cargo::Limits,
) -> Result<api::Graph<api::Captured>> {
    canonicalize_document_borrowed(document, invocations, &policy, context, limits)
}

fn canonicalize_document_borrowed(
    document: protocol::Document,
    invocations: Invocations,
    policy: &Policy,
    context: &api::Context,
    limits: &cargo::Limits,
) -> Result<api::Graph<api::Captured>> {
    let Invocations {
        authority,
        mut allocation,
    } = invocations;
    allocation.retain(policy.usage)?;
    canonicalize_fragments(
        document.fragments,
        authority,
        policy,
        context,
        limits,
        &mut allocation,
        SealMode::Direct,
    )
}

#[cfg(feature = "rustc-driver")]
fn canonicalize_evidence_document(
    document: protocol::Document,
    invocations: Invocations,
    policy: Policy,
    context: &api::Context,
    limits: &cargo::Limits,
    session: &crate::process::EvidenceSession,
) -> Result<CanonicalizationEvidence> {
    let Invocations {
        authority,
        allocation,
    } = invocations;
    let population = document.population();
    let mut allocation = allocation.into_evidence(limits, population)?;
    allocation.retain(policy.usage)?;
    let result = session.observe_inline(
        crate::process::EvidenceInlineStage::Canonicalization,
        || {
            canonicalize_fragments(
                document.fragments,
                authority,
                &policy,
                context,
                limits,
                &mut allocation,
                SealMode::Evidence(session),
            )
        },
    );
    allocation.report_evidence(session.stages());
    let graph = result?;
    let (items, bytes) = allocation.usage();
    #[cfg(test)]
    let counters = allocation
        .evidence_counters()
        .ok_or_else(|| extraction("missing-evidence-counters", "private evidence allocation"))?;
    #[cfg(test)]
    let stages = session.stages();
    drop(graph);
    Ok(CanonicalizationEvidence {
        items,
        bytes,
        #[cfg(test)]
        counters,
        #[cfg(test)]
        stages,
    })
}

fn canonicalize_fragments(
    fragments: Vec<protocol::Fragment>,
    authority: InvocationAuthority,
    policy: &Policy,
    context: &api::Context,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
    seal: SealMode<'_>,
) -> Result<api::Graph<api::Captured>> {
    crate::progress::canonicalized(fragments.len());
    let result = (|| {
        allocation.category(EvidenceCategory::Candidate);
        validate_resolution_context(&fragments, context, allocation)?;
        let catalog = Catalog::from_fragments(&fragments, &authority, allocation)?;
        let candidate = Candidate::from_fragments(fragments, &catalog, limits, allocation)?;
        candidate.finish(policy, context, limits, allocation, seal)
    })();
    result.map_err(|error| allocation.failure(error))
}

fn validate_resolution_context(
    fragments: &[protocol::Fragment],
    context: &api::Context,
    allocation: &mut Allocation,
) -> Result<()> {
    if context.probe_schema() != protocol::SCHEMA {
        return Err(extraction(
            "probe-schema-mismatch",
            "resolution and metadata-probe protocol versions differ",
        ));
    }
    for fragment in fragments {
        let handshake = &fragment.handshake;
        if handshake.compiler_commit != context.compiler_commit() {
            return Err(extraction(
                "compiler-commit-mismatch",
                &handshake.unit_correlation,
            ));
        }
        if handshake.host != context.host() || handshake.selected_target != context.target() {
            return Err(extraction(
                "host-target-mismatch",
                &handshake.unit_correlation,
            ));
        }
        let protocol::Root::Selected { aliases } = &handshake.root;
        for alias in aliases {
            if context.has_selections()
                && context.selected_package(alias) != Some(handshake.package.cargo_id.as_str())
            {
                return Err(extraction(
                    "selected-package-mismatch",
                    &handshake.unit_correlation,
                ));
            }
        }
        let mut names = BTreeMap::<&str, &str>::new();
        let mut stable_ids = BTreeMap::<&str, &str>::new();
        for sysroot in &handshake.sysroot {
            if sysroot.compiler_commit != context.compiler_commit()
                || sysroot.selected_target != context.target()
            {
                return Err(extraction("sysroot-context-mismatch", &sysroot.crate_name));
            }
            match names.get(sysroot.crate_name.as_str()) {
                Some(previous) if **previous != sysroot.stable_crate_id => {
                    return Err(extraction("conflicting-sysroot-name", &sysroot.crate_name));
                }
                Some(_) => {}
                None => {
                    allocation.items(1)?;
                    names.insert(&sysroot.crate_name, &sysroot.stable_crate_id);
                }
            }
            match stable_ids.get(sysroot.stable_crate_id.as_str()) {
                Some(previous) if **previous != sysroot.crate_name => {
                    return Err(extraction(
                        "conflicting-stable-crate-id",
                        &sysroot.stable_crate_id,
                    ));
                }
                Some(_) => {}
                None => {
                    allocation.items(1)?;
                    stable_ids.insert(&sysroot.stable_crate_id, &sysroot.crate_name);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
fn correlate(
    fragments: Vec<protocol::Fragment>,
    invocations: &InvocationAuthority,
    allocation: &mut Allocation,
) -> Result<Vec<protocol::Fragment>> {
    if fragments.len() > invocations.expected.len() {
        return Err(extraction(
            "extra-fragment",
            "more sidecars arrived than selected compiler units",
        ));
    }
    if fragments.len() < invocations.expected.len() {
        return Err(extraction(
            "missing-fragment",
            "one or more selected compiler units published no sidecar",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut nonces = BTreeSet::new();
    for fragment in &fragments {
        validate_handshake(&fragment.handshake)?;
        let correlation = fragment.handshake.unit_correlation.as_str();
        if seen.contains(correlation) {
            return Err(extraction("duplicate-unit-correlation", correlation));
        }
        allocation.items(1)?;
        seen.insert(correlation);
        if nonces.contains(fragment.handshake.invocation_nonce.as_str()) {
            return Err(extraction(
                "duplicate-invocation-nonce",
                &fragment.handshake.invocation_nonce,
            ));
        }
        allocation.items(1)?;
        nonces.insert(fragment.handshake.invocation_nonce.as_str());
        let expected = invocations
            .expected
            .get(correlation)
            .ok_or_else(|| extraction("extra-unit-correlation", correlation))?;
        if expected != &fragment.handshake {
            return Err(extraction("mismatched-unit-correlation", correlation));
        }
    }
    if seen.len() != invocations.expected.len() {
        return Err(extraction(
            "missing-unit-correlation",
            "the sidecar set does not cover every expected unit",
        ));
    }
    Ok(fragments)
}

fn validate_handshake(handshake: &protocol::Handshake) -> Result<()> {
    if handshake.schema != protocol::SCHEMA {
        return Err(extraction(
            "unknown-handshake-schema",
            &format!("expected {}, found {}", protocol::SCHEMA, handshake.schema),
        ));
    }
    for (field, value) in [
        ("Cargo PackageId", handshake.package.cargo_id.as_str()),
        ("package name", handshake.package.name.as_str()),
        ("package version", handshake.package.version.as_str()),
        ("package source", handshake.package.source.as_str()),
        ("target identity", handshake.target.as_str()),
        ("target kind", handshake.target_kind.as_str()),
        ("Cargo profile", handshake.profile.as_str()),
        ("crate name", handshake.crate_name.as_str()),
        ("host", handshake.host.as_str()),
        ("selected target", handshake.selected_target.as_str()),
        ("compiler commit", handshake.compiler_commit.as_str()),
        ("invocation nonce", handshake.invocation_nonce.as_str()),
        ("unit correlation", handshake.unit_correlation.as_str()),
        ("unit key", handshake.unit_key.as_str()),
    ] {
        nonempty(field, value)?;
    }
    sorted_unique("target kinds", &handshake.target_kinds)?;
    if !handshake
        .target_kinds
        .iter()
        .any(|kind| matches!(kind.as_str(), "lib" | "rlib"))
    {
        return Err(extraction(
            "compiler-cargo-unit-mismatch",
            &handshake.unit_correlation,
        ));
    }
    exact_hex(
        "probe argument digest",
        &handshake.probe_arguments_digest,
        64,
    )?;
    exact_hex("invocation nonce", &handshake.invocation_nonce, 64)?;
    exact_hex("unit correlation", &handshake.unit_correlation, 64)?;
    exact_hex("compiler commit", &handshake.compiler_commit, 40)?;
    exact_hex(
        "selected StableCrateId",
        &handshake.artifact.stable_crate_id,
        16,
    )?;
    exact_hex("selected artifact digest", &handshake.artifact.digest, 64)?;
    for (field, value) in [
        ("selected artifact path", handshake.artifact.path.as_str()),
        (
            "selected artifact profile",
            handshake.artifact.profile.as_str(),
        ),
        (
            "selected artifact crate hash",
            handshake.artifact.crate_hash.as_str(),
        ),
        (
            "metadata probe alias",
            handshake.artifact.probe_alias.as_str(),
        ),
    ] {
        nonempty(field, value)?;
    }
    exact_hex("unit key", &handshake.unit_key, 64)?;
    sorted_unique("enabled features", &handshake.features)?;
    for feature in &handshake.features {
        nonempty("enabled feature", feature)?;
    }
    sorted_unique("normal-library dependency keys", &handshake.dependencies)?;
    for dependency in &handshake.dependencies {
        exact_hex("normal-library dependency key", dependency, 64)?;
    }
    if handshake.sysroot.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(extraction("noncanonical-handshake-set", "sysroot crates"));
    }
    for sysroot in &handshake.sysroot {
        for (field, value) in [
            ("sysroot compiler commit", sysroot.compiler_commit.as_str()),
            ("sysroot target", sysroot.selected_target.as_str()),
            ("sysroot crate name", sysroot.crate_name.as_str()),
        ] {
            nonempty(field, value)?;
        }
        exact_hex("sysroot compiler commit", &sysroot.compiler_commit, 40)?;
        exact_hex("StableCrateId", &sysroot.stable_crate_id, 16)?;
    }
    validate_external_crates("loaded crates", &handshake.loaded_crates)?;
    validate_external_crates("external crates", &handshake.external_crates)?;
    if handshake
        .external_crates
        .iter()
        .any(|external| handshake.loaded_crates.binary_search(external).is_err())
    {
        return Err(extraction(
            "external-crate-not-loaded",
            &handshake.unit_correlation,
        ));
    }
    if !handshake.loaded_crates.iter().any(|loaded| {
        loaded.crate_name == handshake.crate_name
            && loaded.stable_crate_id == handshake.artifact.stable_crate_id
            && loaded
                .artifacts
                .binary_search(&handshake.artifact.path)
                .is_ok()
    }) {
        return Err(extraction(
            "selected-artifact-not-loaded",
            &handshake.unit_correlation,
        ));
    }
    let protocol::Root::Selected { aliases } = &handshake.root;
    if aliases.is_empty() {
        return Err(extraction(
            "missing-selected-root",
            &handshake.unit_correlation,
        ));
    }
    sorted_unique("selected aliases", aliases)?;
    for alias in aliases {
        nonempty("selected alias", alias)?;
    }
    Ok(())
}

fn validate_external_crates(label: &'static str, crates: &[protocol::ExternalCrate]) -> Result<()> {
    if crates.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(extraction("noncanonical-handshake-set", label));
    }
    for external in crates {
        nonempty("external crate name", &external.crate_name)?;
        exact_hex("external StableCrateId", &external.stable_crate_id, 16)?;
        if external.artifacts.is_empty()
            || external.artifacts.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(extraction(
                "noncanonical-handshake-set",
                "external crate artifacts",
            ));
        }
        for artifact in &external.artifacts {
            nonempty("external crate artifact", artifact)?;
        }
    }
    Ok(())
}

impl Catalog {
    /// The captured and the sysroot unit `key` names, each looked up once
    /// rather than compared against every unit.
    fn lookup(&self, key: &CompilerKey) -> (Option<&identity::Unit>, Option<&identity::Unit>) {
        (self.units.get(key), self.sysroots.get(key))
    }

    fn from_fragments(
        fragments: &[protocol::Fragment],
        invocations: &InvocationAuthority,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        let mut packages = BTreeMap::<&str, (&str, &str, &str)>::new();
        let mut units = BTreeMap::new();
        let mut sysroots = BTreeMap::new();
        let mut unavailable = BTreeMap::<Box<str>, BTreeSet<Box<str>>>::new();
        let mut unit_keys = BTreeSet::<&str>::new();
        let mut captured_ids = BTreeMap::<&str, &str>::new();
        let mut sysroot_ids = BTreeMap::<&str, &str>::new();
        for fragment in fragments {
            let raw = &fragment.handshake.package;
            let package_fields = (raw.name.as_str(), raw.version.as_str(), raw.source.as_str());
            match packages.get(raw.cargo_id.as_str()) {
                Some(previous) if previous != &package_fields => {
                    return Err(extraction("conflicting-package-identity", &raw.cargo_id));
                }
                Some(_) => {}
                None => {
                    allocation.items(1)?;
                    packages.insert(&raw.cargo_id, package_fields);
                }
            }
            allocation.items(1)?;
            let package = identity::Package::new(
                boxed("Cargo PackageId", &raw.cargo_id, allocation)?,
                boxed("package name", &raw.name, allocation)?,
                boxed("package version", &raw.version, allocation)?,
                boxed("package source", &raw.source, allocation)?,
            );
            let compilation = compilation(&fragment.handshake.compilation);
            let key = CompilerKey {
                crate_name: boxed("crate name", &fragment.handshake.crate_name, allocation)?,
                stable_crate_id: boxed(
                    "selected StableCrateId",
                    &fragment.handshake.artifact.stable_crate_id,
                    allocation,
                )?,
            };
            let mut features = Vec::new();
            allocation.reserve(&mut features, fragment.handshake.features.len())?;
            for feature in &fragment.handshake.features {
                features.push(allocation.owned(feature)?);
            }
            allocation.items(1)?;
            let unit = identity::Unit::new(
                identity::Authority::Cargo(package),
                boxed("target identity", &fragment.handshake.target, allocation)?,
                boxed("target kind", &fragment.handshake.target_kind, allocation)?,
                allocation.owned(&key.crate_name)?,
                compilation,
                features.into_boxed_slice(),
                allocation.owned(&key.stable_crate_id)?,
            );
            match captured_ids.get(fragment.handshake.artifact.stable_crate_id.as_str()) {
                Some(previous) if **previous != fragment.handshake.crate_name => {
                    return Err(extraction(
                        "ambiguous-stable-crate-id",
                        &fragment.handshake.artifact.stable_crate_id,
                    ));
                }
                Some(_) => {}
                None => {
                    allocation.items(1)?;
                    captured_ids.insert(
                        &fragment.handshake.artifact.stable_crate_id,
                        &fragment.handshake.crate_name,
                    );
                }
            }
            if units.contains_key(&key) {
                return Err(extraction(
                    "ambiguous-package-unit-correlation",
                    &fragment.handshake.unit_correlation,
                ));
            }
            allocation.items(1)?;
            units.insert(key, unit);
            nonempty("unit key", &fragment.handshake.unit_key)?;
            if unit_keys.contains(fragment.handshake.unit_key.as_str()) {
                return Err(extraction(
                    "duplicate-unit-key",
                    &fragment.handshake.unit_key,
                ));
            }
            allocation.items(1)?;
            unit_keys.insert(&fragment.handshake.unit_key);
            for raw in &fragment.handshake.sysroot {
                let key = CompilerKey {
                    crate_name: boxed("sysroot crate name", &raw.crate_name, allocation)?,
                    stable_crate_id: boxed("StableCrateId", &raw.stable_crate_id, allocation)?,
                };
                allocation.items(2)?;
                let authority = identity::Authority::Sysroot(identity::Sysroot::new(
                    boxed("sysroot compiler commit", &raw.compiler_commit, allocation)?,
                    boxed("sysroot selected target", &raw.selected_target, allocation)?,
                    allocation.owned(&key.crate_name)?,
                    allocation.owned(&key.stable_crate_id)?,
                ));
                match sysroot_ids.get(raw.stable_crate_id.as_str()) {
                    Some(previous) if **previous != raw.crate_name => {
                        return Err(extraction("conflicting-sysroot-identity", &raw.crate_name));
                    }
                    Some(_) => {}
                    None => {
                        allocation.items(1)?;
                        sysroot_ids.insert(&raw.stable_crate_id, &raw.crate_name);
                    }
                }
                allocation.items(1)?;
                let unit = identity::Unit::new(
                    authority,
                    allocation.owned(&key.crate_name)?,
                    allocation.owned("sysroot")?,
                    allocation.owned(&key.crate_name)?,
                    identity::Compilation::Target,
                    Box::default(),
                    allocation.owned(&key.stable_crate_id)?,
                );
                match sysroots.get(&key) {
                    Some(previous) if previous != &unit => {
                        return Err(extraction("conflicting-sysroot-identity", &raw.crate_name));
                    }
                    Some(_) => {}
                    None => {
                        allocation.items(1)?;
                        sysroots.insert(key, unit);
                    }
                }
            }
        }
        for (key, authority) in &invocations.external_units {
            let ExternalAuthority::Available(external) = authority else {
                allocation.items(1)?;
                let crate_name = allocation.owned(&key.crate_name)?;
                unavailable
                    .entry(allocation.owned(&key.stable_crate_id)?)
                    .or_default()
                    .insert(crate_name);
                continue;
            };
            allocation.items(1)?;
            let package = identity::Package::new(
                allocation.owned(&external.package_id)?,
                allocation.owned(&external.package_name)?,
                allocation.owned(&external.package_version)?,
                allocation.owned(&external.package_source)?,
            );
            let target_kind = external
                .target_kinds
                .first()
                .ok_or_else(|| extraction("missing-external-target-kind", &external.package_id))?;
            let mut features = Vec::new();
            allocation.reserve(&mut features, external.features.len())?;
            for feature in &external.features {
                features.push(allocation.owned(feature)?);
            }
            allocation.items(1)?;
            let unit = identity::Unit::new(
                identity::Authority::Cargo(package),
                allocation.owned(&external.target)?,
                allocation.owned(target_kind)?,
                allocation.owned(&external.crate_name)?,
                identity::Compilation::Target,
                features.into_boxed_slice(),
                allocation.owned(&key.stable_crate_id)?,
            );
            match units.get(key) {
                Some(previous) if previous != &unit => {
                    return Err(extraction(
                        "conflicting-external-unit",
                        &external.package_id,
                    ));
                }
                Some(_) => {}
                None => {
                    allocation.items(1)?;
                    units.insert(
                        CompilerKey {
                            crate_name: allocation.owned(&key.crate_name)?,
                            stable_crate_id: allocation.owned(&key.stable_crate_id)?,
                        },
                        unit,
                    );
                }
            }
        }
        if units.keys().any(|key| sysroots.contains_key(key)) {
            return Err(extraction(
                "forged-sysroot-unit",
                "a captured Cargo unit collides with compiler authority",
            ));
        }
        Ok(Self {
            units,
            sysroots,
            unavailable,
        })
    }

    fn unavailable_identity(
        &self,
        raw: &protocol::Id,
        allocation: &mut Allocation,
    ) -> Result<Option<CompilerKey>> {
        let protocol::Authority::Compiler {
            crate_name,
            stable_crate_id,
        } = &raw.authority;
        nonempty("compiler crate name", crate_name)?;
        exact_hex("StableCrateId", stable_crate_id, 16)?;
        exact_hex("DefPathHash", &raw.def_path_hash, 32)?;
        let unavailable = self
            .unavailable
            .get(stable_crate_id.as_str())
            .is_some_and(|names| names.contains(crate_name.as_str()));
        if unavailable {
            Ok(Some(CompilerKey {
                crate_name: allocation.owned(crate_name)?,
                stable_crate_id: allocation.owned(stable_crate_id)?,
            }))
        } else {
            Ok(None)
        }
    }

    fn validate_raw_id(&self, raw: &protocol::Id) -> Result<()> {
        let protocol::Authority::Compiler {
            crate_name,
            stable_crate_id,
        } = &raw.authority;
        nonempty("compiler crate name", crate_name)?;
        exact_hex("StableCrateId", stable_crate_id, 16)?;
        exact_hex("DefPathHash", &raw.def_path_hash, 32)
    }

    fn id(&self, raw: protocol::Id, allocation: &mut Allocation) -> Result<identity::Definition> {
        let unit = match raw.authority {
            protocol::Authority::Compiler {
                crate_name,
                stable_crate_id,
            } => {
                nonempty("compiler crate name", &crate_name)?;
                exact_hex("StableCrateId", &stable_crate_id, 16)?;
                let key = CompilerKey::from_raw(crate_name, stable_crate_id);
                match self.lookup(&key) {
                    (Some(unit), None) | (None, Some(unit)) => {
                        allocation.unit(unit)?;
                        allocation.cloned_ownership(1);
                        unit.clone()
                    }
                    (Some(_), Some(_)) => {
                        return Err(extraction("ambiguous-compiler-authority", &key.crate_name));
                    }
                    (None, None) => {
                        return Err(extraction("unresolved-compiler-unit", &key.crate_name));
                    }
                }
            }
        };
        exact_hex("DefPathHash", &raw.def_path_hash, 32)?;
        allocation.items(1)?;
        Ok(identity::Definition::new(
            unit,
            {
                allocation.string(&raw.def_path_hash)?;
                raw.def_path_hash.into_boxed_str()
            },
            namespace(raw.namespace),
        ))
    }

    fn id_bounded(
        &self,
        raw: protocol::Id,
        allocation: &mut Allocation,
    ) -> Result<identity::Definition> {
        self.id(raw, allocation)
    }

    fn reexport(
        &self,
        raw: protocol::ReexportId,
        allocation: &mut Allocation,
    ) -> Result<identity::Reexport> {
        nonempty("re-export compiler crate name", &raw.crate_name)?;
        exact_hex("re-export StableCrateId", &raw.stable_crate_id, 16)?;
        exact_hex("re-export DefPathHash", &raw.def_path_hash, 32)?;
        let protocol::ReexportId {
            crate_name,
            stable_crate_id,
            def_path_hash,
        } = raw;
        let key = CompilerKey::from_raw(crate_name, stable_crate_id);
        let unit = match self.lookup(&key) {
            (Some(unit), None) | (None, Some(unit)) => {
                allocation.unit(unit)?;
                allocation.cloned_ownership(1);
                unit.clone()
            }
            (Some(_), Some(_)) => {
                return Err(extraction("ambiguous-re-export-authority", &key.crate_name));
            }
            (None, None) => {
                return Err(extraction(
                    "unresolved-re-export-authority",
                    &key.crate_name,
                ));
            }
        };
        allocation.items(1)?;
        Ok(identity::Reexport::new(unit, {
            allocation.string(&def_path_hash)?;
            def_path_hash.into_boxed_str()
        }))
    }

    fn is_sysroot_id(&self, raw: &protocol::Id) -> bool {
        match &raw.authority {
            protocol::Authority::Compiler {
                crate_name,
                stable_crate_id,
            } => self.sysroots.keys().any(|key| {
                key.crate_name.as_ref() == crate_name
                    && key.stable_crate_id.as_ref() == stable_crate_id
            }),
        }
    }
}

impl Candidate {
    fn from_fragments(
        fragments: Vec<protocol::Fragment>,
        catalog: &Catalog,
        limits: &cargo::Limits,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        let mut definitions =
            BTreeMap::<identity::Definition, conflict::Sourced<model::Definition>>::new();
        let mut spellings = BTreeMap::new();
        let mut rejected = BTreeMap::<identity::Definition, conflict::Sourced<Rejected>>::new();
        let mut sources = Vec::new();
        let mut implementations = Vec::new();
        let mut unmodeled = BTreeSet::new();
        let mut scalar_predicate_proofs = Vec::new();
        let mut exports = BTreeMap::new();
        let mut anonymous = BTreeSet::new();
        let mut imports = Vec::new();
        let mut invocations = Vec::new();
        let mut index = Vec::new();
        let mut definition_count = 0_usize;
        let mut edge_count = 0_usize;
        for fragment in fragments {
            let protocol::Root::Selected { aliases } = &fragment.handshake.root;
            let mut root_aliases = BTreeSet::new();
            for alias in aliases {
                allocation.items(1)?;
                root_aliases.insert(alias.as_str());
            }
            if !fragment.index.is_empty() {
                for alias in &root_aliases {
                    allocation.items(fragment.index.len())?;
                    index.push(ledger::Index::new(
                        (*alias).into(),
                        fragment.index.clone().into_boxed_slice(),
                    ));
                }
            }
            {
                let mut fragment_definitions = BTreeSet::new();
                for raw in &fragment.definitions {
                    allocation.items(1)?;
                    if !fragment_definitions.insert(&raw.id) {
                        return Err(extraction(
                            "duplicate-definition",
                            &fragment.handshake.unit_correlation,
                        ));
                    }
                }
            }
            allocation.reserve(&mut invocations, 1)?;
            invocations.push(invocation(&fragment, allocation)?);
            let source = sources.len();
            allocation.reserve(&mut sources, 1)?;
            sources.push(conflict::Source::new(&fragment.handshake));
            for raw in fragment.definitions {
                let local = is_local_id(&raw.id, &fragment.handshake);
                let sysroot_shape = matches!(&raw.shape, protocol::Shape::Sysroot { .. });
                if sysroot_shape
                    && (local
                        || !catalog.is_sysroot_id(&raw.id)
                        || !sysroot_owner_matches_kind(&raw.owner, &raw.shape)
                        || !matches!(&raw.visibility, protocol::Visibility::Public))
                {
                    return Err(extraction(
                        "mismatched-local-definition",
                        &fragment.handshake.unit_correlation,
                    ));
                }
                definition_count = definition_count.saturating_add(1);
                if catalog.unavailable_identity(&raw.id, allocation)?.is_some() {
                    validate_unavailable_definition(&raw, catalog)?;
                    continue;
                }
                allocation.protocol_id(&raw.id)?;
                allocation.cloned_ownership(1);
                let id = catalog.id_bounded(raw.id.clone(), allocation)?;
                allocation.protocol_span(&raw.span)?;
                allocation.cloned_ownership(1);
                let span = span(raw.span.clone(), allocation)?;
                match definition(raw, catalog, limits, allocation) {
                    Ok(definition) => {
                        if let Some(previous) = rejected.get(&id) {
                            return Err(extraction(
                                "duplicate-definition",
                                &conflict::mixed(
                                    (&definition, &sources[source]),
                                    &sources[previous.fragment],
                                ),
                            ));
                        }
                        match definitions.get_mut(&id) {
                            Some(previous) if previous.value == definition => {}
                            // One selected crate extracts the definition for
                            // shape only and its own crate as a selected root; they
                            // are one definition under its own crate's spelling.
                            Some(previous)
                                if previous.value.same_except_display_path(&definition) =>
                            {
                                let other = if local {
                                    previous.fragment = source;
                                    std::mem::replace(&mut previous.value, definition)
                                } else {
                                    definition
                                };
                                let key = allocation.clone_identity(&id)?;
                                allocation.items(1)?;
                                spellings
                                    .insert(key, other.display_path().map(Box::from).collect());
                            }
                            Some(previous) => {
                                return Err(extraction(
                                    "conflicting-definition",
                                    &conflict::definition(
                                        (&previous.value, &sources[previous.fragment]),
                                        (&definition, &sources[source]),
                                        conflict::Detail::from_env(),
                                    ),
                                ));
                            }
                            None => {
                                let key = allocation.clone_identity(&id)?;
                                allocation.items(1)?;
                                definitions.insert(
                                    key,
                                    conflict::Sourced {
                                        value: definition,
                                        fragment: source,
                                    },
                                );
                            }
                        }
                    }
                    Err(ShapeFailure::Unknown(kind)) => {
                        let outcome = Rejected {
                            reason: ledger::Reason::UnknownCompilerShape {
                                definition: allocation.clone_identity(&id)?,
                                compiler_kind: kind,
                            },
                            span,
                        };
                        reject(
                            &id,
                            outcome,
                            source,
                            &sources,
                            &definitions,
                            &mut rejected,
                            allocation,
                        )?;
                    }
                    Err(ShapeFailure::UnnameablePrivate) => {
                        let outcome = Rejected {
                            reason: ledger::Reason::UnnameablePrivate(
                                allocation.clone_identity(&id)?,
                            ),
                            span,
                        };
                        reject(
                            &id,
                            outcome,
                            source,
                            &sources,
                            &definitions,
                            &mut rejected,
                            allocation,
                        )?;
                    }
                    Err(ShapeFailure::ExternalAuthorityUnavailable {
                        crate_name,
                        stable_crate_id,
                    }) => {
                        let outcome = Rejected {
                            reason: ledger::Reason::ExternalAuthorityUnavailable {
                                crate_name,
                                stable_crate_id,
                            },
                            span,
                        };
                        reject(
                            &id,
                            outcome,
                            source,
                            &sources,
                            &definitions,
                            &mut rejected,
                            allocation,
                        )?;
                    }
                    Err(ShapeFailure::Fatal(error)) => return Err(error),
                }
            }
            for raw in fragment.reachable {
                edge_count = edge_count.saturating_add(1);
                if !root_aliases.contains(raw.root_alias.as_str()) {
                    return Err(extraction("mismatched-root-alias", &raw.root_alias));
                }
                let binding = raw.binding;
                let export = export(raw, catalog, allocation)?;
                match binding {
                    protocol::Binding::Nameable => {
                        let key = key(&export.key, allocation)?;
                        if exports.contains_key(&key) {
                            return Err(extraction("path-namespace-collision", &key.alias));
                        }
                        allocation.items(1)?;
                        exports.insert(key, export);
                    }
                    // Checked once every fragment is in: a trait another
                    // selected crate spells its own way is spelled so
                    // only when that crate's fragment is.
                    protocol::Binding::AnonymousImport => {
                        allocation.items(1)?;
                        imports.push(export);
                    }
                }
            }
            for raw in fragment.implementations {
                edge_count = edge_count.saturating_add(1);
                // An implementation whose crate Cargo cannot join is
                // unavailable by itself: its types and trait stay.
                if catalog.unavailable_identity(&raw.id, allocation)?.is_some() {
                    validate_unavailable_implementation(&raw, catalog)?;
                    allocation.protocol_span(&raw.span)?;
                    continue;
                }
                let mut relevance = Vec::new();
                allocation.temporary_conversions(raw.relevance.len());
                for anchor in &raw.relevance {
                    if catalog.unavailable_identity(anchor, allocation)?.is_none() {
                        allocation.protocol_id(anchor)?;
                        allocation.cloned_ownership(1);
                        let id = catalog.id_bounded(anchor.clone(), allocation)?;
                        allocation.reserve(&mut relevance, 1)?;
                        relevance.push(id);
                    }
                }
                allocation.protocol_span(&raw.span)?;
                allocation.cloned_ownership(1);
                let implementation_span = span(raw.span.clone(), allocation)?;
                // An implementation the extractor cannot model is unavailable
                // by itself. Its self type, its trait and every other
                // implementation of either stay available: one `impl Debug
                // for f16` with an unknown constant must not take `Debug`
                // and every type that implements it with it.
                // Its crate's inventory lists it as unsupported, so no proof
                // reads its trait's domain as complete.
                let _ = (&relevance, &implementation_span);
                let id = raw.id.clone();
                match implementation(raw, catalog, limits, allocation) {
                    Ok(implementation) => {
                        allocation.reserve(&mut implementations, 1)?;
                        implementations.push(implementation);
                    }
                    Err(
                        ShapeFailure::ExternalAuthorityUnavailable { .. }
                        | ShapeFailure::Unknown(_)
                        | ShapeFailure::UnnameablePrivate,
                    ) => {
                        allocation.items(1)?;
                        unmodeled.insert(catalog.id_bounded(id, allocation)?);
                    }
                    Err(ShapeFailure::Fatal(error)) => return Err(error),
                }
            }
            for raw in fragment.scalar_predicate_proofs {
                edge_count = edge_count.saturating_add(1);
                allocation.reserve(&mut scalar_predicate_proofs, 1)?;
                scalar_predicate_proofs.push(scalar_predicate_proof(raw, catalog, allocation)?);
            }
        }
        let definitions: BTreeMap<_, _> = definitions
            .into_iter()
            .map(|(id, sourced)| (id, sourced.value))
            .collect();
        let rejected: BTreeMap<_, _> = rejected
            .into_iter()
            .map(|(id, sourced)| (id, sourced.value))
            .collect();
        for export in imports {
            // Only a trait's `use Trait as _` brings anything into scope.
            let Some(import) = anonymous_import(export, &definitions, &spellings)? else {
                continue;
            };
            if !anonymous.contains(&import) {
                anonymous.insert(import);
            }
        }
        allocation.sort_keys(invocations.len());
        invocations.sort_unstable();
        reconcile_implementations(&mut implementations, allocation)?;
        reconcile_scalar_predicate_proofs(&mut scalar_predicate_proofs, allocation)?;
        let mut semantic_edges = edge_count;
        for definition in definitions.values() {
            let mut references = 0_usize;
            definition.for_each_reference(&mut |_| {
                references = references.saturating_add(1);
            });
            semantic_edges = semantic_edges.saturating_add(references);
        }
        let mut implementation_index = BTreeMap::<identity::Definition, Vec<usize>>::new();
        for (index, implementation) in implementations.iter().enumerate() {
            for anchor in implementation.relevance() {
                semantic_edges = semantic_edges.saturating_add(1);
                if let Some(indices) = implementation_index.get_mut(anchor) {
                    allocation.reserve(indices, 1)?;
                    indices.push(index);
                } else {
                    allocation.items(1)?;
                    let mut indices = Vec::new();
                    allocation.reserve(&mut indices, 1)?;
                    indices.push(index);
                    implementation_index.insert(allocation.clone_identity(anchor)?, indices);
                }
            }
        }
        let mut index_entries = 0_usize;
        for indices in implementation_index.values() {
            index_entries = index_entries.saturating_add(indices.len());
        }
        allocation.index_entries(index_entries);
        allocation.dedup_entries(
            definitions
                .len()
                .saturating_add(rejected.len())
                .saturating_add(implementations.len())
                .saturating_add(scalar_predicate_proofs.len())
                .saturating_add(exports.len())
                .saturating_add(anonymous.len()),
        );
        allocation.candidate_population(
            definitions.len(),
            rejected.len(),
            implementations.len(),
            exports.len(),
            anonymous.len(),
        );
        Ok(Self {
            definitions,
            rejected,
            implementations,
            unmodeled,
            scalar_predicate_proofs,
            implementation_index,
            exports,
            anonymous,
            invocations,
            spellings,
            index: {
                index.sort_unstable();
                index
            },
        })
    }

    fn finish(
        self,
        policy: &Policy,
        context: &api::Context,
        limits: &cargo::Limits,
        allocation: &mut Allocation,
        seal: SealMode<'_>,
    ) -> Result<api::Graph<api::Captured>> {
        // Extraction reads only demanded items, so an exclusion for an
        // item that was never read has nothing to exclude.
        allocation.category(EvidenceCategory::Raw);
        let raw = raw_canonical(&self, context, allocation)?;
        for export in self.exports.values() {
            validate_alias_chain(export, &self.definitions, &self.spellings)?;
        }
        allocation.category(EvidenceCategory::Closure);
        let FixedClosure {
            definitions: retained_definitions,
            implementations: retained_implementations,
            outcomes,
            support,
        } = self.fixed_closure(policy, limits, allocation)?;
        allocation.category(EvidenceCategory::Retained);
        let mut paths = Vec::new();
        for export in self.exports.into_values() {
            let path_key = key(&export.key, allocation)?;
            let rejection = match &export.target {
                ExportTarget::Available(target) => match outcomes.get(target) {
                    Some(ClosureOutcome::Complete) => None,
                    Some(ClosureOutcome::Rejected(rejection)) => {
                        Some(allocation.clone_rejected(rejection)?)
                    }
                    None => {
                        return Err(extraction("internal-fixed-outcome", target.def_path_hash()))
                    }
                },
                ExportTarget::ExternalAuthorityUnavailable {
                    crate_name,
                    stable_crate_id,
                } => {
                    allocation.cloned_ownership(2);
                    Some(Rejected {
                        reason: ledger::Reason::ExternalAuthorityUnavailable {
                            crate_name: {
                                allocation.string(crate_name)?;
                                crate_name.clone()
                            },
                            stable_crate_id: {
                                allocation.string(stable_crate_id)?;
                                stable_crate_id.clone()
                            },
                        },
                        span: allocation.clone_span(&export.span)?,
                    })
                }
                ExportTarget::UnsupportedPublicPath { compiler_kind } => {
                    allocation.cloned_ownership(1);
                    Some(Rejected {
                        reason: ledger::Reason::UnsupportedPublicPath {
                            compiler_kind: {
                                allocation.string(compiler_kind)?;
                                compiler_kind.clone()
                            },
                        },
                        span: allocation.clone_span(&export.span)?,
                    })
                }
            };
            if let Some(exclusion) = policy
                .exclusions
                .get(path_key.alias.as_ref())
                .and_then(|paths| paths.get(path_key.path.as_ref()))
            {
                allocation.string(&exclusion.reason)?;
                allocation.span(&exclusion.span)?;
                allocation.cloned_ownership(2);
                allocation.reserve(&mut paths, 1)?;
                paths.push(ledger::Path::Excluded {
                    key: export.key,
                    reason: exclusion.reason.clone(),
                    policy_span: exclusion.span.clone(),
                    source_span: export.span,
                });
                continue;
            }
            if let Some(rejection) = rejection {
                allocation.reserve(&mut paths, 1)?;
                paths.push(ledger::Path::Rejected {
                    key: export.key,
                    reason: rejection.reason,
                    span: rejection.span,
                });
            } else {
                let ExportTarget::Available(target) = export.target else {
                    return Err(extraction(
                        "missing-external-rejection",
                        "unavailable external export was not rejected",
                    ));
                };
                allocation.reserve(&mut paths, 1)?;
                paths.push(ledger::Path::Imported(
                    ledger::Export::new(
                        export.key,
                        target,
                        export.aliases,
                        export.reexports,
                        export.span,
                    )
                    .with_variant(export.variant),
                ));
            }
        }
        paths.sort_unstable_by(|left, right| left.key().cmp(right.key()));
        allocation.sort_keys(paths.len());
        let expected_definitions = retained_definitions
            .iter()
            .filter(|retained| **retained)
            .count();
        let mut definitions = Vec::new();
        allocation.reserve(&mut definitions, expected_definitions)?;
        for ((_, definition), retained) in self
            .definitions
            .into_iter()
            .zip(retained_definitions.into_vec())
        {
            if retained {
                definitions.push(definition);
            }
        }
        if definitions.len() != expected_definitions {
            return Err(extraction(
                "internal-definition-ledger",
                "retained definition identity was absent",
            ));
        }
        let expected_implementations = retained_implementations
            .iter()
            .filter(|retained| **retained)
            .count();
        let mut implementations = Vec::new();
        allocation.reserve(&mut implementations, expected_implementations)?;
        for (implementation, retained) in self
            .implementations
            .into_iter()
            .zip(retained_implementations.into_vec())
        {
            if retained {
                implementations.push(implementation);
            }
        }
        if implementations.len() != expected_implementations {
            return Err(extraction(
                "internal-implementation-ledger",
                "retained implementation index was absent",
            ));
        }
        let mut support_values = Vec::new();
        allocation.support_edges(support.len());
        allocation.reserve(&mut support_values, support.len())?;
        support_values.extend(support.into_values());
        let mut anonymous = Vec::new();
        allocation.reserve(&mut anonymous, self.anonymous.len())?;
        anonymous.extend(self.anonymous);
        allocation.category(EvidenceCategory::Graph);
        allocation.final_graph(
            self.invocations.len(),
            definitions.len(),
            paths.len(),
            implementations.len(),
            support_values.len(),
            anonymous.len(),
        );
        let seal_graph = || {
            api::Graph::seal_with_raw(
                self.invocations,
                definitions,
                paths,
                implementations,
                self.scalar_predicate_proofs,
                support_values,
                anonymous,
                self.index,
                raw,
                allocation,
            )
        };
        match seal {
            SealMode::Direct => seal_graph(),
            #[cfg(feature = "rustc-driver")]
            SealMode::Evidence(session) => session.observe_inline(
                crate::process::EvidenceInlineStage::GraphSealing,
                seal_graph,
            ),
            #[cfg(not(feature = "rustc-driver"))]
            SealMode::Marker(_) => unreachable!("the marker carries no sealing authority"),
        }
    }

    fn fixed_closure(
        &self,
        policy: &Policy,
        limits: &cargo::Limits,
        allocation: &mut Allocation,
    ) -> Result<FixedClosure> {
        let mut node_ids = BTreeSet::<identity::Definition>::new();
        for id in self.definitions.keys().chain(self.rejected.keys()) {
            insert_fixed_node(&mut node_ids, id, allocation)?;
        }
        for export in self.exports.values() {
            if let ExportTarget::Available(target) = &export.target {
                insert_fixed_node(&mut node_ids, target, allocation)?;
            }
        }
        for definition in self.definitions.values() {
            let mut result = Ok(());
            definition.for_each_reference(&mut |reference| {
                if result.is_ok() {
                    result = insert_fixed_node(&mut node_ids, reference, allocation);
                }
            });
            result?;
        }
        for implementation in &self.implementations {
            insert_fixed_node(&mut node_ids, implementation.id(), allocation)?;
            let mut result = Ok(());
            implementation.for_each_reference(&mut |reference| {
                if result.is_ok() {
                    result = insert_fixed_node(&mut node_ids, reference, allocation);
                }
            });
            result?;
        }

        let callables = callable_lang_items(&self.definitions);
        let mut nodes = Vec::new();
        allocation.temporary_conversions(node_ids.len());
        allocation.reserve(&mut nodes, node_ids.len())?;
        nodes.extend(node_ids);
        let mut states = Vec::new();
        allocation.reserve(&mut states, nodes.len())?;
        states.resize(nodes.len(), FixedNode::default());
        let mut edges = Vec::new();
        for (source, id) in nodes.iter().enumerate() {
            states[source].first_edge = edges.len();
            let definition = self.definitions.get(id);
            let implementation = fixed_implementation(&self.implementations, id);
            if definition.is_some() && implementation.is_some() {
                return Err(extraction("fixed-node-kind-collision", id.def_path_hash()));
            }
            if let Some(definition) = definition {
                let mut result = Ok(());
                definition.for_each_reference(&mut |reference| {
                    if result.is_ok() {
                        result = push_fixed_edge(
                            &mut edges,
                            source,
                            reference,
                            definition.span(),
                            FixedEdgeKind::Support {
                                provenance: id,
                                role: ledger::SupportRole::Definition,
                            },
                            &nodes,
                            allocation,
                        );
                    }
                });
                result?;
                // A function pointer an item takes is read as its `Fn(..) ->
                // R` bound, so `Fn`, `FnOnce` and its `Output` support it.
                if takes_function_pointer(definition) {
                    for callable in &callables {
                        push_fixed_edge(
                            &mut edges,
                            source,
                            callable,
                            definition.span(),
                            FixedEdgeKind::Support {
                                provenance: id,
                                role: ledger::SupportRole::Definition,
                            },
                            &nodes,
                            allocation,
                        )?;
                    }
                }

                let mut owners = Vec::new();
                if let model::Owner::Implementation(owner) = definition.owner() {
                    let index =
                        fixed_implementation_index(&self.implementations, owner, definition)?;
                    let implementation = &self.implementations[index];
                    if implementation
                        .associated_slice()
                        .binary_search(definition.id())
                        .is_err()
                    {
                        return Err(extraction(
                            "implementation-owner-associated",
                            definition.id().def_path_hash(),
                        ));
                    }
                    allocation.reserve(&mut owners, 1)?;
                    owners.push(index);
                }
                let mut admissions = Vec::new();
                if let Some(indices) = self.implementation_index.get(id) {
                    allocation.reserve(&mut admissions, indices.len())?;
                    admissions.extend(
                        indices
                            .iter()
                            .copied()
                            .filter(|index| !owners.contains(index)),
                    );
                }
                allocation.sort_keys(admissions.len());
                admissions.sort_unstable();
                admissions.dedup();
                for (index, kind) in owners
                    .into_iter()
                    .map(|index| (index, FixedEdgeKind::Admission))
                    .chain(
                        admissions
                            .into_iter()
                            .map(|index| (index, FixedEdgeKind::Relevance)),
                    )
                {
                    let implementation = &self.implementations[index];
                    push_fixed_edge(
                        &mut edges,
                        source,
                        implementation.id(),
                        definition.span(),
                        kind,
                        &nodes,
                        allocation,
                    )?;
                }
            } else if let Some((_, implementation)) = implementation {
                validate_fixed_implementation(implementation, self)?;
                let mut result = Ok(());
                implementation.for_each_reference(&mut |reference| {
                    if result.is_ok() {
                        result = push_fixed_edge(
                            &mut edges,
                            source,
                            reference,
                            implementation.span(),
                            FixedEdgeKind::Support {
                                provenance: implementation.id(),
                                role: ledger::SupportRole::Implementation,
                            },
                            &nodes,
                            allocation,
                        );
                    }
                });
                result?;
            }
            states[source].edge_count = edges.len().saturating_sub(states[source].first_edge);
        }

        let mut forward = VecDeque::new();
        let mut target_count = 0_usize;
        for export in self.exports.values() {
            let ExportTarget::Available(target) = &export.target else {
                continue;
            };
            let node = fixed_node_index(&nodes, target)?;
            if !states[node].reachable {
                states[node].reachable = true;
                states[node].depth = 0;
                enqueue_fixed(&mut forward, node, allocation)?;
                target_count = target_count.saturating_add(1);
            }
        }
        let mut forward_entries = forward.len();
        while let Some(source) = forward.pop_front() {
            let depth = states[source].depth;
            let first = states[source].first_edge;
            let end = first.saturating_add(states[source].edge_count);
            for edge in &edges[first..end] {
                if !states[edge.target].reachable {
                    let next_depth = depth.saturating_add(1);
                    limited("support closure depth", limits.depth, next_depth)?;
                    states[edge.target].reachable = true;
                    states[edge.target].depth = next_depth;
                    enqueue_fixed(&mut forward, edge.target, allocation)?;
                    forward_entries = forward_entries.saturating_add(1);
                }
            }
        }

        let mut incoming = Vec::new();
        allocation.temporary_conversions(edges.len());
        allocation.reserve(&mut incoming, edges.len())?;
        for (index, edge) in edges.iter().enumerate() {
            if states[edge.source].reachable && states[edge.target].reachable {
                incoming.push(index);
            }
        }
        allocation.sort_keys(incoming.len());
        incoming.sort_unstable_by_key(|index| (edges[*index].target, *index));
        let mut reverse = VecDeque::new();
        for (node, id) in nodes.iter().enumerate() {
            if states[node].reachable
                && fixed_implementation(&self.implementations, id).is_none()
                && (self.rejected.contains_key(id) || !self.definitions.contains_key(id))
            {
                states[node].rejection_distance = Some(0);
                states[node].rejection_terminal = Some(node);
                enqueue_fixed(&mut reverse, node, allocation)?;
            }
        }
        let mut reverse_entries = reverse.len();
        while let Some(target) = reverse.pop_front() {
            let distance = states[target]
                .rejection_distance
                .ok_or_else(|| extraction("fixed-rejection-distance", "queued target"))?;
            let start = incoming.partition_point(|index| edges[*index].target < target);
            let end = incoming.partition_point(|index| edges[*index].target <= target);
            for index in &incoming[start..end] {
                if matches!(edges[*index].kind, FixedEdgeKind::Relevance) {
                    continue;
                }
                let source = edges[*index].source;
                if states[source].rejection_distance.is_none() {
                    let next_distance = distance.saturating_add(1);
                    limited("support closure depth", limits.depth, next_distance)?;
                    states[source].rejection_distance = Some(next_distance);
                    enqueue_fixed(&mut reverse, source, allocation)?;
                    reverse_entries = reverse_entries.saturating_add(1);
                }
            }
        }
        let mut rejected_nodes = Vec::new();
        allocation.temporary_conversions(states.len());
        allocation.reserve(&mut rejected_nodes, states.len())?;
        rejected_nodes.extend(
            states.iter().enumerate().filter_map(|(node, state)| {
                state.rejection_distance.map(|distance| (distance, node))
            }),
        );
        allocation.sort_keys(rejected_nodes.len());
        rejected_nodes.sort_unstable();
        for (distance, source) in rejected_nodes {
            if distance == 0 {
                continue;
            }
            let first = states[source].first_edge;
            let end = first.saturating_add(states[source].edge_count);
            let edge = (first..end)
                .find(|index| {
                    !matches!(edges[*index].kind, FixedEdgeKind::Relevance)
                        && states[edges[*index].target].rejection_distance
                            == Some(distance.saturating_sub(1))
                })
                .ok_or_else(|| {
                    extraction("fixed-rejection-witness", nodes[source].def_path_hash())
                })?;
            let target = edges[edge].target;
            states[source].rejection_terminal = states[target].rejection_terminal;
            states[source].rejection_edge = states[target].rejection_edge.or(Some(edge));
        }

        let mut retained = VecDeque::new();
        for export in self.exports.values() {
            let ExportTarget::Available(target) = &export.target else {
                continue;
            };
            if is_excluded(policy, export) {
                continue;
            }
            let node = fixed_node_index(&nodes, target)?;
            if !states[node].retained {
                states[node].retained = true;
                enqueue_fixed(&mut retained, node, allocation)?;
            }
        }
        let mut retained_entries = retained.len();
        let mut support = BTreeMap::new();
        while let Some(source) = retained.pop_front() {
            let first = states[source].first_edge;
            let end = first.saturating_add(states[source].edge_count);
            for (edge_index, edge) in edges[first..end].iter().enumerate() {
                if matches!(edge.kind, FixedEdgeKind::Support { .. }) {
                    insert_fixed_support(
                        &mut support,
                        first.saturating_add(edge_index),
                        edge,
                        &nodes,
                        &states,
                        &edges,
                        self,
                        allocation,
                    )?;
                }
                if !states[edge.target].retained {
                    states[edge.target].retained = true;
                    enqueue_fixed(&mut retained, edge.target, allocation)?;
                    retained_entries = retained_entries.saturating_add(1);
                }
            }
        }

        let mut outcomes = BTreeMap::new();
        allocation.closure_computation();
        for export in self.exports.values() {
            let ExportTarget::Available(target) = &export.target else {
                continue;
            };
            if outcomes.contains_key(target) {
                allocation.closure_cache_hit();
                continue;
            }
            let node = fixed_node_index(&nodes, target)?;
            let outcome = match states[node].rejection_terminal {
                Some(terminal) => {
                    let rejection = fixed_rejection(
                        terminal,
                        states[node].rejection_edge,
                        &nodes,
                        &edges,
                        self,
                        allocation,
                    )?;
                    allocation.items(1)?;
                    ClosureOutcome::Rejected(Box::new(rejection))
                }
                None => ClosureOutcome::Complete,
            };
            allocation.items(1)?;
            outcomes.insert(allocation.clone_identity(target)?, outcome);
        }

        let mut definitions = Vec::new();
        allocation.reserve(&mut definitions, self.definitions.len())?;
        for id in self.definitions.keys() {
            let node = fixed_node_index(&nodes, id)?;
            definitions.push(states[node].retained);
        }
        let mut implementations = Vec::new();
        allocation.reserve(&mut implementations, self.implementations.len())?;
        for implementation in &self.implementations {
            let node = fixed_node_index(&nodes, implementation.id())?;
            implementations.push(states[node].retained);
        }
        allocation.fixed_population(nodes.len(), edges.len(), outcomes.len());
        allocation.fixed_queue(forward_entries, reverse_entries, retained_entries);
        Ok(FixedClosure {
            definitions: definitions.into_boxed_slice(),
            implementations: implementations.into_boxed_slice(),
            outcomes,
            support,
        })
    }
}

fn reconcile_implementations(
    implementations: &mut Vec<model::Implementation>,
    allocation: &mut Allocation,
) -> Result<()> {
    allocation.sort_keys(implementations.len());
    implementations
        .sort_unstable_by(|left, right| left.id().cmp(right.id()).then_with(|| left.cmp(right)));
    if implementations.len() < 2 {
        return Ok(());
    }

    let mut retained = 1_usize;
    for candidate in 1..implementations.len() {
        if implementations[retained - 1].id() != implementations[candidate].id() {
            implementations.swap(retained, candidate);
            retained += 1;
            continue;
        }
        let (prefix, suffix) = implementations.split_at_mut(candidate);
        let owner = &mut prefix[retained - 1];
        let observation = &mut suffix[0];
        if !owner.same_structure(observation) {
            return Err(extraction(
                "conflicting-implementation",
                owner.id().def_path_hash(),
            ));
        }
        allocation.sort_keys(
            owner
                .relevance()
                .len()
                .saturating_add(observation.relevance().len()),
        );
        allocation.sort_keys(
            owner
                .associated()
                .len()
                .saturating_add(observation.associated().len()),
        );
        owner.join_evidence(observation, |values, amount| {
            allocation.reserve(values, amount)
        })?;
    }
    implementations.truncate(retained);
    Ok(())
}

fn reconcile_scalar_predicate_proofs(
    proofs: &mut Vec<model::ScalarPredicateProof>,
    allocation: &mut Allocation,
) -> Result<()> {
    allocation.sort_keys(proofs.len());
    proofs.sort_unstable();
    for pair in proofs.windows(2) {
        if pair[0].owner() == pair[1].owner()
            && pair[0].parameter() == pair[1].parameter()
            && pair[0].trait_id() == pair[1].trait_id()
            && pair[0].scalar() == pair[1].scalar()
            && pair[0] != pair[1]
        {
            return Err(extraction(
                "conflicting-scalar-predicate-proof",
                pair[0].owner().def_path_hash(),
            ));
        }
    }
    proofs.dedup();
    Ok(())
}

#[cfg(test)]
mod implementation_join_tests {
    use super::{reconcile_implementations, Allocation};
    use crate::api::identity::{Authority, Compilation, Definition, Namespace, Package, Unit};
    use crate::api::model;
    use crate::Error;

    fn id(hash: &str) -> Definition {
        let package = Package::new(
            "path+file:///fixture#fixture@0.1.0".into(),
            "fixture".into(),
            "0.1.0".into(),
            "path+file:///fixture".into(),
        );
        let unit = Unit::new(
            Authority::Cargo(package),
            "fixture".into(),
            "lib".into(),
            "fixture".into(),
            Compilation::Target,
            Box::default(),
            "0000000000000001".into(),
        );
        Definition::new(unit, hash.into(), Namespace::Type)
    }

    fn implementation(
        relevance: &[&str],
        associated: &[&str],
        self_type: &str,
    ) -> model::Implementation {
        model::Implementation::new(
            "implementation".into(),
            id("implementation"),
            relevance
                .iter()
                .map(|hash| id(hash))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            model::Trait::Inherent,
            model::Type::Primitive(self_type.into()),
            model::Safety::Safe,
            model::Polarity::Positive,
            false,
            false,
            model::Generics::new(Box::default(), Box::default()),
            associated
                .iter()
                .map(|hash| id(hash))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            model::Span::new("fixture.rs".into(), 0, 1, 1, 0, 1, 1),
            model::Origin::Source,
        )
    }

    fn reconcile(mut rows: Vec<model::Implementation>) -> Vec<model::Implementation> {
        reconcile_implementations(
            &mut rows,
            &mut Allocation::with_limits(usize::MAX, usize::MAX),
        )
        .expect("structurally identical implementation evidence joins");
        rows
    }

    #[test]
    fn split_sets_join_idempotently_and_ignore_fragment_order() {
        let first = implementation(&["relevance-a"], &["associated-a"], "u8");
        let second = implementation(&["relevance-b"], &["associated-b"], "u8");
        let duplicate = implementation(&["relevance-a"], &["associated-a"], "u8");
        let forward = reconcile(vec![first, second, duplicate]);
        let reverse = reconcile(
            vec![
                implementation(&["relevance-a"], &["associated-a"], "u8"),
                implementation(&["relevance-b"], &["associated-b"], "u8"),
                implementation(&["relevance-a"], &["associated-a"], "u8"),
            ]
            .into_iter()
            .rev()
            .collect(),
        );
        assert_eq!(forward, reverse);
        assert_eq!(forward.len(), 1);
        assert_eq!(
            forward[0]
                .relevance()
                .map(Definition::def_path_hash)
                .collect::<Vec<_>>(),
            ["relevance-a", "relevance-b"],
        );
        assert_eq!(
            forward[0]
                .associated()
                .map(Definition::def_path_hash)
                .collect::<Vec<_>>(),
            ["associated-a", "associated-b"],
        );
    }

    #[test]
    fn same_identity_with_different_structure_rejects() {
        let mut rows = vec![
            implementation(&["relevance-a"], &[], "u8"),
            implementation(&["relevance-b"], &[], "u16"),
        ];
        assert!(matches!(
            reconcile_implementations(
                &mut rows,
                &mut Allocation::with_limits(usize::MAX, usize::MAX),
            ),
            Err(Error::Extraction {
                code: "conflicting-implementation",
                ..
            })
        ));
    }

    /// The joined capacity is charged before growth; only a latched
    /// account (a genuine failure) refuses before any growth.
    #[test]
    fn joined_capacity_is_charged_before_growth() {
        let mut rows = vec![
            implementation(&["relevance-a"], &[], "u8"),
            implementation(&["relevance-b"], &[], "u8"),
        ];
        let mut allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        reconcile_implementations(&mut rows, &mut allocation)
            .expect("the join is charged and completes");
        assert!(allocation.usage().0 >= 1);
        let mut rows = vec![
            implementation(&["relevance-a"], &[], "u8"),
            implementation(&["relevance-b"], &[], "u8"),
        ];
        assert!(matches!(
            reconcile_implementations(&mut rows, &mut Allocation::refused()),
            Err(Error::Node { operation, .. }) if operation == super::REFUSED_ACCOUNT_OPERATION
        ));
        assert_eq!(rows.len(), 2, "a refused account grows nothing");
    }
}

fn insert_fixed_node(
    nodes: &mut BTreeSet<identity::Definition>,
    id: &identity::Definition,
    allocation: &mut Allocation,
) -> Result<()> {
    if !nodes.contains(id) {
        allocation.items(1)?;
        nodes.insert(allocation.clone_identity(id)?);
    }
    Ok(())
}

fn fixed_node_index(nodes: &[identity::Definition], id: &identity::Definition) -> Result<usize> {
    nodes
        .binary_search(id)
        .map_err(|_| extraction("fixed-node-index", id.def_path_hash()))
}

fn fixed_implementation<'candidate>(
    implementations: &'candidate [model::Implementation],
    id: &identity::Definition,
) -> Option<(usize, &'candidate model::Implementation)> {
    implementations
        .binary_search_by(|implementation| implementation.id().cmp(id))
        .ok()
        .map(|index| (index, &implementations[index]))
}

/// The index of the impl `id` that owns `member`, named by the member's
/// path when it is missing, as its hash alone names nothing a reader can
/// find.
fn fixed_implementation_index(
    implementations: &[model::Implementation],
    id: &identity::Definition,
    member: &model::Definition,
) -> Result<usize> {
    fixed_implementation(implementations, id)
        .map(|(index, _)| index)
        .ok_or_else(|| {
            let path = member.display_path().collect::<Vec<_>>().join("::");
            extraction(
                "implementation-owner-cardinality",
                &format!(
                    "{} (the impl of `{path}` in crate `{}`)",
                    id.def_path_hash(),
                    id.unit().crate_name()
                ),
            )
        })
}

fn validate_fixed_implementation(
    implementation: &model::Implementation,
    candidate: &Candidate,
) -> Result<()> {
    for associated in implementation.associated() {
        let definition = candidate.definitions.get(associated).ok_or_else(|| {
            extraction(
                "implementation-associated-cardinality",
                associated.def_path_hash(),
            )
        })?;
        if !implementation.retains_associated_owner(definition) {
            return Err(extraction(
                "implementation-associated-owner",
                associated.def_path_hash(),
            ));
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "one fixed edge carries its canonical endpoints, exact support/admission kind, span, and bounded ownership authorities"
)]
/// `core`'s `Fn`, `FnOnce` and `FnOnce::Output`, those the extraction
/// holds: the driver adds them wherever an item names a function pointer.
fn callable_lang_items(
    definitions: &BTreeMap<identity::Definition, model::Definition>,
) -> Vec<identity::Definition> {
    let core = |definition: &model::Definition| {
        let unit = definition.id().unit();
        unit.authority().sysroot().is_some() && unit.crate_name() == "core"
    };
    let once = definitions
        .values()
        .find(|definition| core(definition) && definition.display_path().last() == Some("FnOnce"));
    definitions
        .values()
        .filter(|definition| core(definition))
        .filter(|definition| {
            let path = definition.display_path().collect::<Vec<_>>();
            matches!(path.last(), Some(&("Fn" | "FnOnce")))
                || (path.last() == Some(&"Output")
                    && (path.iter().rev().nth(1) == Some(&"FnOnce")
                        || once.is_some_and(|once| {
                            matches!(definition.owner(), model::Owner::Definition(owner)
                                if owner == once.id())
                        })))
        })
        .map(|definition| definition.id().clone())
        .collect()
}

/// Whether a function takes a function pointer.
fn takes_function_pointer(definition: &model::Definition) -> bool {
    matches!(definition.kind(), model::Kind::Function { signature, .. }
        if signature
            .inputs()
            .any(|input| matches!(input, model::Type::FunctionPointer(_))))
}

fn push_fixed_edge<'candidate>(
    edges: &mut Vec<FixedEdge<'candidate>>,
    source: usize,
    target: &identity::Definition,
    span: &'candidate model::Span,
    kind: FixedEdgeKind<'candidate>,
    nodes: &[identity::Definition],
    allocation: &mut Allocation,
) -> Result<()> {
    let target = fixed_node_index(nodes, target)?;
    allocation.reserve(edges, 1)?;
    edges.push(FixedEdge {
        source,
        target,
        span,
        kind,
    });
    Ok(())
}

fn enqueue_fixed(
    queue: &mut VecDeque<usize>,
    node: usize,
    allocation: &mut Allocation,
) -> Result<()> {
    allocation.items(1)?;
    queue.try_reserve(1).map_err(|error| Error::Allocation {
        operation: "fixed queue allocation",
        source: error,
    })?;
    queue.push_back(node);
    allocation.queue_entries(1);
    Ok(())
}

fn fixed_rejection(
    terminal: usize,
    rejection_edge: Option<usize>,
    nodes: &[identity::Definition],
    edges: &[FixedEdge<'_>],
    candidate: &Candidate,
    allocation: &mut Allocation,
) -> Result<Rejected> {
    let id = &nodes[terminal];
    let reason = match candidate.rejected.get(id) {
        Some(rejected) => allocation.clone_reason(&rejected.reason)?,
        None => ledger::Reason::UnresolvedDefinition(allocation.clone_identity(id)?),
    };
    let span = match rejection_edge {
        Some(edge) => allocation.clone_span(edges[edge].span)?,
        None => match candidate.rejected.get(id) {
            Some(rejected) => allocation.clone_span(&rejected.span)?,
            None => fallback_span(allocation)?,
        },
    };
    Ok(Rejected { reason, span })
}

#[allow(
    clippy::too_many_arguments,
    reason = "one support outcome consumes the exact fixed edge, global witness state, and canonical ownership authorities"
)]
fn insert_fixed_support(
    support: &mut BTreeMap<SupportKey, ledger::Support>,
    edge_index: usize,
    edge: &FixedEdge<'_>,
    nodes: &[identity::Definition],
    states: &[FixedNode],
    edges: &[FixedEdge<'_>],
    candidate: &Candidate,
    allocation: &mut Allocation,
) -> Result<()> {
    let FixedEdgeKind::Support { provenance, role } = edge.kind else {
        return Err(extraction(
            "fixed-support-admission",
            nodes[edge.target].def_path_hash(),
        ));
    };
    let id = &nodes[edge.target];
    let key = SupportKey {
        id: allocation.clone_identity(id)?,
        provenance: allocation.clone_identity(provenance)?,
        role,
    };
    if support.contains_key(&key) {
        return Ok(());
    }
    let outcome = match states[edge.target].rejection_terminal {
        Some(terminal) => {
            let rejected = fixed_rejection(
                terminal,
                states[edge.target].rejection_edge.or(Some(edge_index)),
                nodes,
                edges,
                candidate,
                allocation,
            )?;
            ledger::Support::Rejected {
                id: allocation.clone_identity(id)?,
                provenance: allocation.clone_identity(provenance)?,
                role,
                reason: rejected.reason,
                span: rejected.span,
            }
        }
        None => ledger::Support::Complete {
            id: allocation.clone_identity(id)?,
            provenance: allocation.clone_identity(provenance)?,
            role,
        },
    };
    allocation.items(1)?;
    support.insert(key, outcome);
    Ok(())
}

fn raw_canonical(
    candidate: &Candidate,
    context: &api::Context,
    allocation: &mut Allocation,
) -> Result<api::Raw> {
    let mut definitions = Vec::new();
    allocation.temporary_conversions(candidate.definitions.len());
    allocation.reserve(&mut definitions, candidate.definitions.len())?;
    definitions.extend(candidate.definitions.values());
    let mut rejected = Vec::new();
    allocation.temporary_conversions(candidate.rejected.len());
    allocation.reserve(&mut rejected, candidate.rejected.len())?;
    rejected.extend(candidate.rejected.iter());
    let mut exports = Vec::new();
    allocation.temporary_conversions(candidate.exports.len());
    allocation.reserve(&mut exports, candidate.exports.len())?;
    exports.extend(candidate.exports.values());
    let mut anonymous = Vec::new();
    allocation.temporary_conversions(candidate.anonymous.len());
    allocation.reserve(&mut anonymous, candidate.anonymous.len())?;
    anonymous.extend(candidate.anonymous.iter());
    let body = RawBody {
        definitions: &definitions,
        rejected: &rejected,
        exports: &exports,
        anonymous: &anonymous,
        implementations: &candidate.implementations,
        scalar_predicate_proofs: &candidate.scalar_predicate_proofs,
    };
    let (canonical, semantic) = raw_pair(context, &candidate.invocations, &body, allocation)?;
    Ok(api::Graph::raw_pair(
        canonical,
        semantic,
        context.resolution_universe(),
    ))
}

/// The raw and semantic encodings of one pre-policy graph. They differ only in
/// their context and invocations, so the body is encoded once and each header
/// is spliced around it; the bytes equal encoding each whole document, which
/// took four passes over the graph (measure and write, twice).
fn raw_pair(
    context: &api::Context,
    invocations: &[api::Invocation],
    body: &RawBody<'_>,
    allocation: &mut Allocation,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let encoding = |error: serde_json::Error| Error::Extraction {
        code: "raw-canonical-encoding",
        reason: error.to_string().into_boxed_str(),
    };
    let body = api::semantic::encode_once(body, allocation)?;
    let header = |context: Vec<u8>, invocations: Vec<u8>| {
        const SCHEMA: &[u8] = br#"{"schema":3,"context":"#;
        const INVOCATIONS: &[u8] = br#","invocations":"#;
        let length = [
            SCHEMA.len(),
            context.len(),
            INVOCATIONS.len(),
            invocations.len(),
            body.len(),
        ]
        .into_iter()
        .try_fold(0_usize, usize::checked_add)
        .ok_or_else(|| arithmetic("canonical raw API bytes"))?;
        let mut document = Vec::new();
        document
            .try_reserve_exact(length)
            .map_err(|source| Error::Allocation {
                operation: "raw canonical allocation",
                source,
            })?;
        document.extend_from_slice(SCHEMA);
        document.extend_from_slice(&context);
        document.extend_from_slice(INVOCATIONS);
        document.extend_from_slice(&invocations);
        document.push(b',');
        document.extend_from_slice(&body[1..]);
        Ok::<_, Error>(document)
    };
    let canonical = header(
        serde_json::to_vec(context).map_err(encoding)?,
        serde_json::to_vec(invocations).map_err(encoding)?,
    )?;
    allocation.items(1)?;
    allocation.bytes(canonical.len())?;
    allocation.raw_canonical_bytes(canonical.len());
    let semantic = header(
        serde_json::to_vec(&api::semantic::Context(context)).map_err(encoding)?,
        serde_json::to_vec(&api::semantic::Invocations(invocations)).map_err(encoding)?,
    )?;
    api::semantic::admit(semantic.len(), allocation)?;
    Ok((canonical, semantic))
}

#[cfg(test)]
fn finalize_support(
    pending: BTreeMap<SupportKey, PendingSupport>,
    allocation: &mut Allocation,
) -> Result<BTreeMap<SupportKey, ledger::Support>> {
    let mut invalid = BTreeMap::<identity::Definition, Rejected>::new();
    for outcome in pending.values() {
        if let PendingSupport::Rejected {
            id,
            provenance,
            reason,
            span,
            ..
        } = outcome
        {
            let evidence = Rejected {
                reason: allocation.clone_reason(reason)?,
                span: allocation.clone_span(span)?,
            };
            if !invalid.contains_key(id) {
                allocation.items(1)?;
                invalid.insert(allocation.clone_identity(id)?, evidence);
            }
            if !invalid.contains_key(provenance) {
                let evidence = Rejected {
                    reason: allocation.clone_reason(reason)?,
                    span: allocation.clone_span(span)?,
                };
                allocation.items(1)?;
                invalid.insert(allocation.clone_identity(provenance)?, evidence);
            }
        }
    }
    loop {
        let mut changed = false;
        for outcome in pending.values() {
            let Some(evidence) = invalid.get(outcome.id()) else {
                continue;
            };
            if !invalid.contains_key(outcome.provenance()) {
                let evidence = Rejected {
                    reason: allocation.clone_reason(&evidence.reason)?,
                    span: allocation.clone_span(&evidence.span)?,
                };
                allocation.items(1)?;
                invalid.insert(allocation.clone_identity(outcome.provenance())?, evidence);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    allocation.items(pending.len())?;
    let mut support = BTreeMap::new();
    for (key, outcome) in pending {
        let outcome = match outcome {
            PendingSupport::Present {
                id,
                provenance,
                role,
            } => match invalid.get(&id) {
                Some(evidence) => ledger::Support::Rejected {
                    id,
                    provenance,
                    role,
                    reason: allocation.clone_reason(&evidence.reason)?,
                    span: allocation.clone_span(&evidence.span)?,
                },
                None => ledger::Support::Complete {
                    id,
                    provenance,
                    role,
                },
            },
            PendingSupport::Rejected {
                id,
                provenance,
                role,
                reason,
                span,
            } => ledger::Support::Rejected {
                id,
                provenance,
                role,
                reason,
                span,
            },
        };
        support.insert(key, outcome);
    }
    Ok(support)
}

fn is_local_id(id: &protocol::Id, handshake: &protocol::Handshake) -> bool {
    match &id.authority {
        protocol::Authority::Compiler {
            crate_name,
            stable_crate_id,
        } => {
            crate_name == &handshake.crate_name
                && stable_crate_id == &handshake.artifact.stable_crate_id
        }
    }
}

fn validate_alias_chain(
    export: &Export,
    definitions: &BTreeMap<identity::Definition, model::Definition>,
    spellings: &BTreeMap<identity::Definition, Box<[Box<str>]>>,
) -> Result<()> {
    let ExportTarget::Available(target) = &export.target else {
        for pair in export.aliases.windows(2) {
            if pair[0].to_parts() != pair[1].from_parts() {
                return Err(extraction(
                    "invalid-alias-chain",
                    "unavailable external alias chain is discontinuous",
                ));
            }
        }
        if export.aliases.last().is_some_and(|alias| {
            !export
                .key
                .path()
                .eq(alias.to_parts().iter().map(AsRef::as_ref))
        }) {
            return Err(extraction(
                "invalid-alias-chain",
                "unavailable external alias chain does not reach its export",
            ));
        }
        return Ok(());
    };
    let Some(definition) = definitions.get(target) else {
        return Ok(());
    };
    // Each fragment spells a definition relative to its own selected crate; a
    // definition merged from two fragments carries both spellings.
    let alternate = spellings.get(target).map(AsRef::as_ref);
    let canonical = |parts: &[Box<str>]| {
        definition
            .display_path()
            .eq(parts.iter().map(AsRef::as_ref))
    };
    let own = match export.aliases.first() {
        Some(first) if first.to_parts().is_empty() => {
            return Err(extraction("invalid-alias-chain", target.def_path_hash()));
        }
        Some(first) if canonical(first.from_parts()) => None,
        Some(first) if alternate == Some(first.from_parts()) => alternate,
        Some(_) => return Err(extraction("invalid-alias-chain", target.def_path_hash())),
        None => None,
    };
    let is_own = |parts: &[Box<str>]| own.map_or_else(|| canonical(parts), |own| own == parts);
    for (index, alias) in export.aliases.iter().enumerate() {
        if alias.to_parts().is_empty()
            || index
                .checked_sub(1)
                .is_some_and(|previous| export.aliases[previous].to_parts() != alias.from_parts())
            || is_own(alias.to_parts())
            || export.aliases[..index]
                .iter()
                .any(|previous| previous.to_parts() == alias.to_parts())
        {
            return Err(extraction("invalid-alias-chain", target.def_path_hash()));
        }
    }
    let reaches_export = export.aliases.last().map_or_else(
        || {
            definition.display_path().eq(export.key.path())
                || alternate.is_some_and(|alternate| {
                    alternate.iter().map(AsRef::as_ref).eq(export.key.path())
                })
        },
        |alias| {
            export
                .key
                .path()
                .eq(alias.to_parts().iter().map(AsRef::as_ref))
        },
    );
    if !reaches_export {
        return Err(extraction("invalid-alias-chain", target.def_path_hash()));
    }
    Ok(())
}

fn shape_id(
    catalog: &Catalog,
    raw: protocol::Id,
    allocation: &mut Allocation,
) -> std::result::Result<identity::Definition, ShapeFailure> {
    if let Some(key) = catalog.unavailable_identity(&raw, allocation)? {
        return Err(ShapeFailure::ExternalAuthorityUnavailable {
            crate_name: key.crate_name,
            stable_crate_id: key.stable_crate_id,
        });
    }
    catalog.id(raw, allocation).map_err(ShapeFailure::Fatal)
}

fn sysroot_owner_matches_kind(owner: &protocol::Owner, shape: &protocol::Shape) -> bool {
    let protocol::Shape::Sysroot { compiler_kind } = shape else {
        return false;
    };
    let associated = matches!(
        compiler_kind.as_str(),
        "associated-function" | "associated-constant" | "associated-type"
    );
    match owner {
        protocol::Owner::Free => !associated,
        protocol::Owner::Definition { id } | protocol::Owner::Implementation { id } => {
            associated && id.namespace == protocol::Namespace::Type
        }
    }
}

fn validate_unavailable_definition(raw: &protocol::Definition, catalog: &Catalog) -> Result<()> {
    catalog.validate_raw_id(&raw.id)?;
    if raw.display_path.is_empty() {
        return Err(extraction(
            "empty-definition-path",
            "unavailable external definition",
        ));
    }
    for part in &raw.display_path {
        nonempty("definition display path", part)?;
    }
    match &raw.owner {
        protocol::Owner::Free => {}
        protocol::Owner::Definition { id } => catalog.validate_raw_id(id)?,
        protocol::Owner::Implementation { id } => catalog.validate_raw_id(id)?,
    }
    if let protocol::Visibility::Restricted { scope } = &raw.visibility {
        catalog.validate_raw_id(scope)?;
    }
    validate_span(&raw.span)?;
    validate_origin(&raw.origin)?;
    Ok(())
}

fn definition(
    raw: protocol::Definition,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Definition, ShapeFailure> {
    validate_type_constant_evidence(&raw)?;
    let rejected_class = rejected_class(&raw.owner, &raw.shape);
    let id = shape_id(catalog, raw.id, allocation)?;
    let display_path = strings("definition display path", raw.display_path, allocation)?;
    if display_path.is_empty() {
        return Err(extraction("empty-definition-path", id.def_path_hash()).into());
    }
    let owner = match raw.owner {
        protocol::Owner::Free => model::Owner::Free,
        protocol::Owner::Definition { id } => {
            model::Owner::Definition(shape_id(catalog, id, allocation)?)
        }
        protocol::Owner::Implementation { id } => {
            model::Owner::Implementation(shape_id(catalog, id, allocation)?)
        }
    };
    let visibility = visibility(raw.visibility, catalog, allocation)?;
    let kind = match shape(raw.shape, catalog, limits, allocation) {
        Ok(kind) => kind,
        Err(ShapeFailure::Unknown(compiler_kind)) => model::Kind::Rejected {
            class: rejected_class,
            reason: ledger::Reason::UnknownCompilerShape {
                definition: allocation.clone_identity(&id)?,
                compiler_kind,
            },
        },
        Err(ShapeFailure::UnnameablePrivate) => model::Kind::Rejected {
            class: rejected_class,
            reason: ledger::Reason::UnnameablePrivate(allocation.clone_identity(&id)?),
        },
        Err(ShapeFailure::ExternalAuthorityUnavailable {
            crate_name,
            stable_crate_id,
        }) => model::Kind::Rejected {
            class: rejected_class,
            reason: ledger::Reason::ExternalAuthorityUnavailable {
                crate_name,
                stable_crate_id,
            },
        },
        Err(ShapeFailure::Fatal(error)) => return Err(ShapeFailure::Fatal(error)),
    };
    let span = span(raw.span, allocation)?;
    let origin = origin(raw.origin, allocation)?;
    allocation.items(1)?;
    Ok(model::Definition::new(
        id,
        display_path,
        owner,
        visibility,
        kind,
        span,
        origin,
    ))
}

fn validate_type_constant_evidence(
    raw: &protocol::Definition,
) -> std::result::Result<(), ShapeFailure> {
    let protocol::Shape::TypeConst { rhs, .. } = &raw.shape else {
        return Ok(());
    };
    if raw.id.namespace != protocol::Namespace::Value {
        return Err(extraction("type-constant-namespace", raw.id.def_path_hash.as_str()).into());
    }
    let owner_is_type = match &raw.owner {
        protocol::Owner::Free => true,
        protocol::Owner::Definition { id } | protocol::Owner::Implementation { id } => {
            id.namespace == protocol::Namespace::Type
        }
    };
    if !owner_is_type {
        return Err(extraction(
            "type-constant-owner-namespace",
            raw.id.def_path_hash.as_str(),
        )
        .into());
    }
    let value_presence_is_valid = match (&raw.owner, rhs) {
        (protocol::Owner::Definition { .. }, _) => true,
        (_, protocol::Default::Present { .. }) => true,
        (_, protocol::Default::Absent) => false,
    };
    if !value_presence_is_valid {
        return Err(extraction("type-constant-missing-rhs", raw.id.def_path_hash.as_str()).into());
    }
    Ok(())
}

fn rejected_class(owner: &protocol::Owner, shape: &protocol::Shape) -> model::RejectedClass {
    use model::{MethodReceiver, RejectedClass};

    match shape {
        protocol::Shape::Struct { .. } | protocol::Shape::Enum { .. } => RejectedClass::Declaration,
        protocol::Shape::Const { .. } => match owner {
            protocol::Owner::Free => RejectedClass::Constant,
            protocol::Owner::Definition { .. } | protocol::Owner::Implementation { .. } => {
                RejectedClass::AssociatedConstant
            }
        },
        protocol::Shape::TypeConst { .. } => RejectedClass::Unavailable,
        protocol::Shape::Function { signature, .. } => match &signature.receiver {
            protocol::Receiver::Value { .. } => RejectedClass::Method(MethodReceiver::Owned),
            protocol::Receiver::Reference { mutability, .. } => {
                RejectedClass::Method(match mutability {
                    protocol::Mutability::Shared => MethodReceiver::Shared,
                    protocol::Mutability::Mutable => MethodReceiver::Mutable,
                })
            }
            protocol::Receiver::None => match owner {
                protocol::Owner::Free => RejectedClass::Function,
                protocol::Owner::Definition { .. } | protocol::Owner::Implementation { .. } => {
                    RejectedClass::AssociatedFunction
                }
            },
            protocol::Receiver::Explicit { .. } => RejectedClass::Unavailable,
        },
        protocol::Shape::Module
        | protocol::Shape::Union { .. }
        | protocol::Shape::Static { .. }
        | protocol::Shape::TypeAlias { .. }
        | protocol::Shape::Trait { .. }
        | protocol::Shape::AssociatedType { .. }
        | protocol::Shape::Sysroot { .. }
        | protocol::Shape::MacroRules { .. }
        | protocol::Shape::UnnameablePrivate
        | protocol::Shape::Unknown { .. } => RejectedClass::Unavailable,
    }
}

fn shape(
    raw: protocol::Shape,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Kind, ShapeFailure> {
    Ok(match raw {
        protocol::Shape::UnnameablePrivate => {
            return Err(ShapeFailure::UnnameablePrivate);
        }
        protocol::Shape::Module => model::Kind::Module,
        protocol::Shape::Struct {
            field_form,
            fields,
            fields_complete,
            non_exhaustive,
            generics,
            layout,
        } => model::Kind::Struct {
            fields: field_set(
                field_form,
                fields,
                fields_complete,
                non_exhaustive,
                catalog,
                limits,
                allocation,
            )?
            .with_layout(layout_of(layout, allocation)?),
            generics: generics_of(generics, catalog, limits, allocation)?,
        },
        protocol::Shape::Union {
            field_form,
            fields,
            fields_complete,
            non_exhaustive,
            generics,
        } => model::Kind::Union {
            fields: field_set(
                field_form,
                fields,
                fields_complete,
                non_exhaustive,
                catalog,
                limits,
                allocation,
            )?,
            generics: generics_of(generics, catalog, limits, allocation)?,
        },
        protocol::Shape::Enum { variants, generics } => model::Kind::Enum {
            variants: {
                let mut converted = Vec::new();
                allocation.temporary_conversions(variants.len());
                allocation.reserve(&mut converted, variants.len())?;
                for variant in variants {
                    converted.push(variant_of(variant, catalog, limits, allocation)?);
                }
                converted.into_boxed_slice()
            },
            generics: generics_of(generics, catalog, limits, allocation)?,
        },
        protocol::Shape::Const { ty, value } => model::Kind::Const {
            ty: type_of(ty, catalog, limits, allocation)?,
            value: default_const(value, catalog, limits, allocation)?,
        },
        protocol::Shape::TypeConst { ty, rhs } => model::Kind::TypeConst {
            ty: type_of(ty, catalog, limits, allocation)?,
            rhs: default_const(rhs, catalog, limits, allocation)?,
        },
        protocol::Shape::Static {
            ty,
            mutable,
            thread_local,
        } => model::Kind::Static {
            ty: type_of(ty, catalog, limits, allocation)?,
            mutable,
            thread_local,
        },
        protocol::Shape::TypeAlias { ty, generics } => model::Kind::TypeAlias {
            ty: type_of(ty, catalog, limits, allocation)?,
            generics: generics_of(generics, catalog, limits, allocation)?,
        },
        protocol::Shape::Function {
            signature,
            generics,
        } => model::Kind::Function {
            signature: signature_of(signature, catalog, limits, allocation)?,
            generics: generics_of(generics, catalog, limits, allocation)?,
        },
        protocol::Shape::Trait {
            unsafe_trait,
            auto_trait,
            supertraits,
            associated,
            generics,
        } => model::Kind::Trait {
            unsafe_trait,
            auto_trait,
            supertraits: predicates(supertraits, catalog, limits, allocation)?,
            associated: ids(associated, catalog, allocation)?,
            generics: generics_of(generics, catalog, limits, allocation)?,
        },
        protocol::Shape::AssociatedType {
            bounds: raw_bounds,
            default,
            generics,
        } => model::Kind::AssociatedType {
            bounds: predicates(raw_bounds, catalog, limits, allocation)?,
            default: default_type(default, catalog, limits, allocation)?,
            generics: generics_of(generics, catalog, limits, allocation)?,
        },
        protocol::Shape::Sysroot { compiler_kind } => model::Kind::Sysroot {
            compiler_kind: boxed("sysroot compiler kind", &compiler_kind, allocation)?,
        },
        protocol::Shape::MacroRules { source, krate } => model::Kind::MacroRules {
            source: boxed("macro_rules source", &source, allocation)?,
            krate: boxed("macro_rules crate", &krate, allocation)?,
        },
        protocol::Shape::Unknown { compiler_kind } => {
            allocation.string(&compiler_kind)?;
            return Err(ShapeFailure::Unknown(compiler_kind.into_boxed_str()));
        }
    })
}

fn fields_of(
    fields: Vec<protocol::Field>,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<Box<[model::Field]>, ShapeFailure> {
    let mut converted = Vec::new();
    allocation.temporary_conversions(fields.len());
    allocation.reserve(&mut converted, fields.len())?;
    for field in fields {
        allocation.items(1)?;
        converted.push(model::Field::new(
            boxed("field name", &field.name, allocation)?,
            field.index,
            visibility(field.visibility, catalog, allocation)?,
            type_of(field.ty, catalog, limits, allocation)?,
            span(field.span, allocation)?,
        ));
    }
    Ok(converted.into_boxed_slice())
}

fn field_set(
    form: protocol::FieldForm,
    fields: Vec<protocol::Field>,
    complete: bool,
    non_exhaustive: bool,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::FieldSet, ShapeFailure> {
    let fields = fields_of(fields, catalog, limits, allocation)?;
    if (matches!(form, protocol::FieldForm::Unit) && !fields.is_empty())
        || fields
            .iter()
            .any(|field| !matches!(field.visibility(), model::Visibility::Public))
    {
        return Err(ShapeFailure::Unknown("malformed public field proof".into()));
    }
    let form = match form {
        protocol::FieldForm::Record => model::FieldForm::Record,
        protocol::FieldForm::Tuple => model::FieldForm::Tuple,
        protocol::FieldForm::Unit => model::FieldForm::Unit,
    };
    allocation.items(1)?;
    Ok(model::FieldSet::new(form, fields, complete, non_exhaustive))
}

fn layout_of(
    layout: protocol::Default<protocol::Layout>,
    allocation: &mut Allocation,
) -> std::result::Result<Option<model::Layout>, ShapeFailure> {
    let protocol::Default::Present { value } = layout else {
        return Ok(None);
    };
    allocation.items(1)?;
    let mut pointers = Vec::new();
    allocation.reserve(&mut pointers, value.pointers.len())?;
    pointers.extend(value.pointers);
    Ok(Some(model::Layout::new(
        value.size,
        pointers.into_boxed_slice(),
        value.varied,
    )))
}

fn variant_of(
    variant: protocol::Variant,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Variant, ShapeFailure> {
    allocation.items(1)?;
    Ok(model::Variant::new(
        boxed("variant name", &variant.name, allocation)?,
        field_set(
            variant.field_form,
            variant.fields,
            variant.fields_complete,
            variant.non_exhaustive,
            catalog,
            limits,
            allocation,
        )?,
        default_const(variant.discriminant, catalog, limits, allocation)?,
        span(variant.span, allocation)?,
    ))
}

fn generics_of(
    generics: protocol::Generics,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Generics, ShapeFailure> {
    let mut parameters = Vec::new();
    allocation.temporary_conversions(generics.parameters.len());
    allocation.reserve(&mut parameters, generics.parameters.len())?;
    for parameter in generics.parameters {
        let mut covariant = false;
        let mut given = false;
        let kind = match parameter.parameter_kind {
            protocol::ParameterKind::Lifetime {
                bounds: raw,
                covariant: declared,
            } => {
                covariant = declared;
                model::ParameterKind::Lifetime {
                    bounds: bounds(raw, catalog, limits, allocation)?,
                }
            }
            protocol::ParameterKind::Type {
                bounds: raw,
                default,
                given: declared,
            } => {
                given = declared;
                model::ParameterKind::Type {
                    bounds: bounds(raw, catalog, limits, allocation)?,
                    default: default_type(default, catalog, limits, allocation)?,
                }
            }
            protocol::ParameterKind::Const { ty, default } => model::ParameterKind::Const {
                ty: type_of(ty, catalog, limits, allocation)?,
                default: default_const(default, catalog, limits, allocation)?,
            },
        };
        allocation.items(1)?;
        parameters.push(
            model::Parameter::new(
                shape_id(catalog, parameter.owner, allocation)?,
                parameter.index,
                boxed("generic parameter name", &parameter.name, allocation)?,
                kind,
            )
            .with_covariant(covariant)
            .with_given(given),
        );
    }
    let mut predicates = Vec::new();
    allocation.temporary_conversions(generics.predicates.len());
    allocation.reserve(&mut predicates, generics.predicates.len())?;
    for predicate in generics.predicates {
        predicates.push(predicate_of(predicate, catalog, limits, allocation)?);
    }
    allocation.sort_keys(predicates.len());
    predicates.sort_unstable();
    predicates.dedup();
    allocation.items(1)?;
    Ok(model::Generics::new(
        parameters.into_boxed_slice(),
        predicates.into_boxed_slice(),
    ))
}

fn predicate_of(
    predicate: protocol::Predicate,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Predicate, ShapeFailure> {
    Ok(match predicate {
        protocol::Predicate::Bound { ty, bound } => model::Predicate::Bound {
            ty: type_of(ty, catalog, limits, allocation)?,
            bound: bound_of(bound, catalog, limits, allocation)?,
        },
        protocol::Predicate::ProjectionEquality { projection, ty } => {
            model::Predicate::ProjectionEquality {
                projection: type_of(projection, catalog, limits, allocation)?,
                ty: type_of(ty, catalog, limits, allocation)?,
            }
        }
        protocol::Predicate::LifetimeOutlives { longer, shorter } => {
            model::Predicate::LifetimeOutlives {
                longer: lifetime(longer, catalog, allocation)?,
                shorter: lifetime(shorter, catalog, allocation)?,
            }
        }
        protocol::Predicate::TypeOutlives { ty, lifetime: raw } => model::Predicate::TypeOutlives {
            ty: type_of(ty, catalog, limits, allocation)?,
            lifetime: lifetime(raw, catalog, allocation)?,
        },
        protocol::Predicate::ConstEvaluatable { value } => {
            model::Predicate::ConstEvaluatable(const_of(value, catalog, limits, allocation)?)
        }
        protocol::Predicate::Unknown { compiler_kind } => {
            allocation.string(&compiler_kind)?;
            return Err(ShapeFailure::Unknown(compiler_kind.into_boxed_str()));
        }
    })
}

fn type_of(
    ty: protocol::Type,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Type, ShapeFailure> {
    Ok(match ty {
        protocol::Type::Primitive { name } => {
            if !matches!(
                name.as_str(),
                "bool"
                    | "char"
                    | "str"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "f16"
                    | "f32"
                    | "f64"
                    | "f128"
            ) {
                return Err(ShapeFailure::Unknown(prefixed(
                    "primitive:",
                    &name,
                    allocation,
                )?));
            }
            model::Type::Primitive(boxed("primitive type", &name, allocation)?)
        }
        protocol::Type::Definition { id, arguments } => model::Type::Definition {
            id: shape_id(catalog, id, allocation)?,
            arguments: arguments_of(arguments, catalog, limits, allocation)?,
        },
        protocol::Type::Parameter { owner, index } => model::Type::Parameter {
            owner: shape_id(catalog, owner, allocation)?,
            index,
        },
        protocol::Type::SelfType { owner } => {
            model::Type::SelfType(shape_id(catalog, owner, allocation)?)
        }
        protocol::Type::Tuple { elements } => {
            let mut converted = Vec::new();
            allocation.temporary_conversions(elements.len());
            allocation.reserve(&mut converted, elements.len())?;
            for element in elements {
                converted.push(type_of(element, catalog, limits, allocation)?);
            }
            model::Type::Tuple(converted.into_boxed_slice())
        }
        protocol::Type::Slice { element } => {
            allocation.items(1)?;
            model::Type::Slice(Box::new(type_of(*element, catalog, limits, allocation)?))
        }
        protocol::Type::Array { element, length } => {
            allocation.items(1)?;
            model::Type::Array {
                element: Box::new(type_of(*element, catalog, limits, allocation)?),
                length: const_of(length, catalog, limits, allocation)?,
            }
        }
        protocol::Type::Reference {
            lifetime: raw,
            mutability,
            ty,
        } => model::Type::Reference {
            lifetime: lifetime(raw, catalog, allocation)?,
            mutability: mutability_of(mutability),
            ty: {
                allocation.items(1)?;
                Box::new(type_of(*ty, catalog, limits, allocation)?)
            },
        },
        protocol::Type::RawPointer { mutability, ty } => model::Type::RawPointer {
            mutability: mutability_of(mutability),
            ty: {
                allocation.items(1)?;
                Box::new(type_of(*ty, catalog, limits, allocation)?)
            },
        },
        protocol::Type::FunctionPointer { signature } => {
            allocation.items(1)?;
            model::Type::FunctionPointer(Box::new(signature_of(
                *signature, catalog, limits, allocation,
            )?))
        }
        protocol::Type::TraitObject {
            bounds: raw,
            lifetime: lt,
        } => model::Type::TraitObject {
            bounds: object_bounds(raw, catalog, limits, allocation)?,
            lifetime: lifetime(lt, catalog, allocation)?,
        },
        protocol::Type::ImplTrait {
            id,
            bounds: raw,
            captures,
        } => model::Type::ImplTrait {
            id: shape_id(catalog, id, allocation)?,
            bounds: predicates(raw, catalog, limits, allocation)?,
            captures: arguments_of(captures, catalog, limits, allocation)?,
        },
        protocol::Type::Projection {
            trait_id,
            associated,
            arguments,
        } => model::Type::Projection {
            trait_id: shape_id(catalog, trait_id, allocation)?,
            associated: shape_id(catalog, associated, allocation)?,
            arguments: arguments_of(arguments, catalog, limits, allocation)?,
        },
        protocol::Type::Never => model::Type::Never,
        protocol::Type::Unknown { compiler_kind } => {
            allocation.string(&compiler_kind)?;
            return Err(ShapeFailure::Unknown(compiler_kind.into_boxed_str()));
        }
    })
}

fn signature_of(
    signature: protocol::Signature,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Signature, ShapeFailure> {
    let receiver = match signature.receiver {
        protocol::Receiver::None => model::Receiver::None,
        protocol::Receiver::Value { ty } => model::Receiver::Value {
            ty: type_of(ty, catalog, limits, allocation)?,
        },
        protocol::Receiver::Reference {
            lifetime: raw,
            mutability,
            ty,
        } => model::Receiver::Reference {
            lifetime: lifetime(raw, catalog, allocation)?,
            mutability: mutability_of(mutability),
            ty: type_of(ty, catalog, limits, allocation)?,
        },
        protocol::Receiver::Explicit { ty } => model::Receiver::Explicit {
            ty: type_of(ty, catalog, limits, allocation)?,
        },
    };
    let guard = match signature.guard {
        Some(guard) => {
            allocation.items(1)?;
            Some(model::Guard::new(
                type_of(guard.target, catalog, limits, allocation)?,
                guard.mutable,
            ))
        }
        None => None,
    };
    allocation.items(1)?;
    Ok(model::Signature::new(
        safety(signature.safety),
        signature.const_function,
        signature.async_function,
        boxed("function ABI", &signature.abi, allocation)?,
        signature.variadic,
        binders_of(signature.binders, catalog, allocation)?,
        receiver,
        types(signature.inputs, catalog, limits, allocation)?,
        type_of(signature.output, catalog, limits, allocation)?,
    )
    .with_guard(guard))
}

fn bound_of(
    bound: protocol::Bound,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Bound, ShapeFailure> {
    Ok(match bound {
        protocol::Bound::Trait {
            trait_id,
            arguments,
            polarity,
            binders,
        } => model::Bound::Trait {
            trait_id: shape_id(catalog, trait_id, allocation)?,
            arguments: arguments_of(arguments, catalog, limits, allocation)?,
            polarity: polarity_of(polarity),
            binders: binders_of(binders, catalog, allocation)?,
        },
        protocol::Bound::MaybeSized => model::Bound::MaybeSized,
    })
}

fn object_bounds(
    bounds: Vec<protocol::ObjectBound>,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<Box<[model::ObjectBound]>, ShapeFailure> {
    let mut converted = Vec::new();
    allocation.temporary_conversions(bounds.len());
    allocation.reserve(&mut converted, bounds.len())?;
    for bound in bounds {
        converted.push(match bound {
            protocol::ObjectBound::Trait { bound } => {
                model::ObjectBound::Trait(bound_of(bound, catalog, limits, allocation)?)
            }
            protocol::ObjectBound::ProjectionEquality {
                associated,
                arguments,
                ty,
            } => model::ObjectBound::ProjectionEquality {
                associated: shape_id(catalog, associated, allocation)?,
                arguments: arguments_of(arguments, catalog, limits, allocation)?,
                ty: type_of(ty, catalog, limits, allocation)?,
            },
        });
    }
    allocation.sort_keys(converted.len());
    converted.sort_unstable();
    converted.dedup();
    Ok(converted.into_boxed_slice())
}

fn const_of(
    value: protocol::Const,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Const, ShapeFailure> {
    Ok(match value {
        protocol::Const::Literal { ty, value } => model::Const::Literal {
            ty: boxed("const type", &ty, allocation)?,
            value: boxed("const literal", &value, allocation)?,
        },
        protocol::Const::Parameter { owner, index } => model::Const::Parameter {
            owner: shape_id(catalog, owner, allocation)?,
            index,
        },
        protocol::Const::Unevaluated {
            definition,
            arguments,
        } => model::Const::Unevaluated {
            definition: shape_id(catalog, definition, allocation)?,
            arguments: arguments_of(arguments, catalog, limits, allocation)?,
        },
        protocol::Const::Unknown { compiler_kind } => {
            allocation.string(&compiler_kind)?;
            return Err(ShapeFailure::Unknown(compiler_kind.into_boxed_str()));
        }
    })
}

fn implementation(
    raw: protocol::Implementation,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Implementation, ShapeFailure> {
    exact_hex("implementation DefPathHash", &raw.key, 32)?;
    let id = shape_id(catalog, raw.id, allocation)?;
    if id.namespace() != identity::Namespace::Type || id.def_path_hash() != raw.key {
        return Err(extraction("implementation-identity-mismatch", &raw.key).into());
    }
    if raw.relevance.is_empty() || raw.relevance.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(extraction("implementation-relevance-order", &raw.key).into());
    }
    let relevance = ids(raw.relevance, catalog, allocation)?;
    let trait_ref = match raw.trait_ref {
        protocol::Trait::Inherent => model::Trait::Inherent,
        protocol::Trait::Trait { id, arguments } => model::Trait::Trait {
            id: shape_id(catalog, id, allocation)?,
            arguments: arguments_of(arguments, catalog, limits, allocation)?,
        },
    };
    allocation.items(1)?;
    Ok(model::Implementation::new(
        boxed("implementation key", &raw.key, allocation)?,
        id,
        relevance,
        trait_ref,
        type_of(raw.self_type, catalog, limits, allocation)?,
        safety(raw.safety),
        polarity_of(raw.polarity),
        raw.const_impl,
        raw.default_impl,
        generics_of(raw.generics, catalog, limits, allocation)?,
        ids(raw.associated, catalog, allocation)?,
        span(raw.span, allocation)?,
        origin(raw.origin, allocation)?,
    ))
}

fn scalar_predicate_proof(
    raw: protocol::ScalarPredicateProof,
    catalog: &Catalog,
    allocation: &mut Allocation,
) -> Result<model::ScalarPredicateProof> {
    let owner = catalog.id_bounded(raw.owner, allocation)?;
    let trait_id = catalog.id_bounded(raw.trait_id, allocation)?;
    if trait_id.namespace() != identity::Namespace::Type {
        return Err(extraction(
            "scalar-predicate-trait-namespace",
            trait_id.def_path_hash(),
        ));
    }
    let scalar = match raw.scalar {
        protocol::Scalar::Unit => model::Scalar::Unit,
        protocol::Scalar::Bool => model::Scalar::Bool,
        protocol::Scalar::Char => model::Scalar::Char,
        protocol::Scalar::F64 => model::Scalar::F64,
        protocol::Scalar::I64 => model::Scalar::I64,
    };
    let outcome = match raw.outcome {
        protocol::ScalarPredicateOutcome::Absent => model::ScalarPredicateOutcome::Absent,
        protocol::ScalarPredicateOutcome::Positive { implementation } => {
            let implementation = catalog.id_bounded(implementation, allocation)?;
            if implementation.namespace() != identity::Namespace::Type {
                return Err(extraction(
                    "scalar-predicate-implementation-namespace",
                    implementation.def_path_hash(),
                ));
            }
            model::ScalarPredicateOutcome::Positive { implementation }
        }
        protocol::ScalarPredicateOutcome::Unresolved => model::ScalarPredicateOutcome::Unresolved,
    };
    allocation.items(1)?;
    Ok(model::ScalarPredicateProof::new(
        owner,
        raw.parameter,
        trait_id,
        scalar,
        outcome,
    ))
}

fn validate_unavailable_implementation(
    raw: &protocol::Implementation,
    catalog: &Catalog,
) -> Result<()> {
    exact_hex("implementation DefPathHash", &raw.key, 32)?;
    catalog.validate_raw_id(&raw.id)?;
    if raw.id.def_path_hash != raw.key || raw.id.namespace != protocol::Namespace::Type {
        return Err(extraction("implementation-identity-mismatch", &raw.key));
    }
    if raw.relevance.is_empty() || raw.relevance.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(extraction("implementation-relevance-order", &raw.key));
    }
    for anchor in &raw.relevance {
        catalog.validate_raw_id(anchor)?;
    }
    if let protocol::Trait::Trait { id, .. } = &raw.trait_ref {
        catalog.validate_raw_id(id)?;
    }
    for associated in &raw.associated {
        catalog.validate_raw_id(associated)?;
    }
    validate_span(&raw.span)?;
    validate_origin(&raw.origin)?;
    Ok(())
}

fn export(raw: protocol::Export, catalog: &Catalog, allocation: &mut Allocation) -> Result<Export> {
    allocation.string(&raw.root_alias)?;
    allocation.items(1)?;
    let key = identity::Export::new(
        boxed("selected root alias", &raw.root_alias, allocation)?,
        strings("public export path", raw.path, allocation)?,
        namespace(raw.namespace),
    );
    if key.path().len() == 0 {
        return Err(extraction("empty-export-path", key.root_alias()));
    }
    let mut aliases = Vec::new();
    allocation.reserve(&mut aliases, raw.aliases.len())?;
    for alias in raw.aliases {
        allocation.items(1)?;
        aliases.push(ledger::Alias::new(
            strings("alias source path", alias.from, allocation)?,
            strings("alias target path", alias.to, allocation)?,
            span(alias.span, allocation)?,
        ));
    }
    let mut reexports = Vec::new();
    allocation.reserve(&mut reexports, raw.reexports.len())?;
    for reexport in raw.reexports {
        allocation.items(1)?;
        let kind = match reexport.kind {
            protocol::ReexportKind::Single => ledger::ReexportKind::Single,
            protocol::ReexportKind::Glob => ledger::ReexportKind::Glob,
            protocol::ReexportKind::ExternCrate => ledger::ReexportKind::ExternCrate,
            protocol::ReexportKind::MacroUse => ledger::ReexportKind::MacroUse,
            protocol::ReexportKind::MacroExport => ledger::ReexportKind::MacroExport,
        };
        match reexport.evidence {
            protocol::ReexportEvidence::Identified { item, span: source } => {
                if matches!(
                    kind,
                    ledger::ReexportKind::MacroUse | ledger::ReexportKind::MacroExport
                ) {
                    return Err(extraction("identified-macro-re-export", key.root_alias()));
                }
                reexports.push(ledger::Reexport::identified(
                    kind,
                    catalog.reexport(item, allocation)?,
                    span(source, allocation)?,
                ));
            }
            protocol::ReexportEvidence::IdentitylessMacro => {
                if !matches!(
                    kind,
                    ledger::ReexportKind::MacroUse | ledger::ReexportKind::MacroExport
                ) {
                    return Err(extraction("identityless-item-re-export", key.root_alias()));
                }
                reexports.push(ledger::Reexport::identityless_macro(kind));
            }
        }
    }
    let target = match raw.target {
        protocol::ExportTarget::Definition { id } => {
            match catalog.unavailable_identity(&id, allocation)? {
                Some(key) => ExportTarget::ExternalAuthorityUnavailable {
                    crate_name: key.crate_name,
                    stable_crate_id: key.stable_crate_id,
                },
                None => ExportTarget::Available(catalog.id(id, allocation)?),
            }
        }
        protocol::ExportTarget::Unsupported { compiler_kind } => {
            ExportTarget::UnsupportedPublicPath {
                compiler_kind: boxed("unsupported public path kind", &compiler_kind, allocation)?,
            }
        }
    };
    let variant = raw
        .variant
        .map(|variant| boxed("re-exported variant name", &variant, allocation))
        .transpose()?;
    allocation.items(1)?;
    Ok(Export {
        key,
        target,
        aliases: aliases.into_boxed_slice(),
        reexports: reexports.into_boxed_slice(),
        span: span(raw.span, allocation)?,
        variant,
    })
}

fn anonymous_import(
    export: Export,
    definitions: &BTreeMap<identity::Definition, model::Definition>,
    spellings: &BTreeMap<identity::Definition, Box<[Box<str>]>>,
) -> Result<Option<ledger::AnonymousImport>> {
    if export.key.namespace() != identity::Namespace::Type {
        return Err(extraction(
            "anonymous-import-namespace",
            export.key.root_alias(),
        ));
    }
    let ExportTarget::Available(target) = &export.target else {
        return Err(extraction(
            "anonymous-import-target",
            export.key.root_alias(),
        ));
    };
    let Some(definition) = definitions.get(target) else {
        return Err(extraction(
            "anonymous-import-definition",
            target.def_path_hash(),
        ));
    };
    match definition.kind() {
        model::Kind::Trait { .. } => {}
        // A declaration whose shape failed closed may be a trait, whose
        // methods an underscore import brings into scope: refuse it, naming
        // the item, rather than drop the methods.
        model::Kind::Rejected { .. } => {
            let named = spellings
                .get(target)
                .map(|path| path.join("::"))
                .unwrap_or_else(|| target.def_path_hash().to_string());
            return Err(extraction("anonymous-import-kind", &named));
        }
        // `use Item as _` brings only a trait's methods into scope, so any
        // other item adds nothing, as in rustc.
        _ => return Ok(None),
    }
    if export.aliases.is_empty() {
        return Err(extraction("anonymous-import-alias", target.def_path_hash()));
    }
    validate_alias_chain(&export, definitions, spellings)?;
    if !matches!(
        export.reexports.last(),
        Some(ledger::Reexport::Identified {
            kind: ledger::ReexportKind::Single,
            ..
        })
    ) {
        return Err(extraction(
            "anonymous-import-provenance",
            target.def_path_hash(),
        ));
    }
    let Export {
        key,
        target: ExportTarget::Available(target),
        aliases,
        reexports,
        span,
        variant: None,
    } = export
    else {
        return Err(extraction(
            "anonymous-import-target",
            "validated target changed before transfer",
        ));
    };
    let (root_alias, path, _) = key.into_parts();
    Ok(Some(ledger::AnonymousImport::new(
        root_alias, path, target, aliases, reexports, span,
    )))
}

fn visibility(
    raw: protocol::Visibility,
    catalog: &Catalog,
    allocation: &mut Allocation,
) -> std::result::Result<model::Visibility, ShapeFailure> {
    Ok(match raw {
        protocol::Visibility::Public => model::Visibility::Public,
        protocol::Visibility::Restricted { scope } => {
            model::Visibility::Restricted(shape_id(catalog, scope, allocation)?)
        }
    })
}

fn default_type(
    default: protocol::Default<protocol::Type>,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Default<model::Type>, ShapeFailure> {
    Ok(match default {
        protocol::Default::Absent => model::Default::Absent,
        protocol::Default::Present { value } => {
            model::Default::Present(type_of(value, catalog, limits, allocation)?)
        }
    })
}

fn default_const(
    default: protocol::Default<protocol::Const>,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<model::Default<model::Const>, ShapeFailure> {
    Ok(match default {
        protocol::Default::Absent => model::Default::Absent,
        protocol::Default::Present { value } => {
            model::Default::Present(const_of(value, catalog, limits, allocation)?)
        }
    })
}

fn arguments_of(
    arguments: Vec<protocol::Argument>,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<Box<[model::Argument]>, ShapeFailure> {
    let mut converted = Vec::new();
    allocation.temporary_conversions(arguments.len());
    allocation.reserve(&mut converted, arguments.len())?;
    for argument in arguments {
        converted.push(match argument {
            protocol::Argument::Lifetime { lifetime: raw } => {
                model::Argument::Lifetime(lifetime(raw, catalog, allocation)?)
            }
            protocol::Argument::Type { ty } => {
                model::Argument::Type(type_of(ty, catalog, limits, allocation)?)
            }
            protocol::Argument::Const { value } => {
                model::Argument::Const(const_of(value, catalog, limits, allocation)?)
            }
        });
    }
    Ok(converted.into_boxed_slice())
}

fn lifetime(
    lifetime: protocol::Lifetime,
    catalog: &Catalog,
    allocation: &mut Allocation,
) -> std::result::Result<model::Lifetime, ShapeFailure> {
    Ok(match lifetime {
        protocol::Lifetime::Static => model::Lifetime::Static,
        protocol::Lifetime::Early { owner, index } => model::Lifetime::Early {
            owner: shape_id(catalog, owner, allocation)?,
            index,
        },
        protocol::Lifetime::Late { binder, index } => model::Lifetime::Late {
            binder: binder_of(binder, catalog, allocation)?,
            index,
        },
        protocol::Lifetime::Erased => model::Lifetime::Erased,
    })
}

fn binder_of(
    binder: protocol::Binder,
    catalog: &Catalog,
    allocation: &mut Allocation,
) -> std::result::Result<model::Binder, ShapeFailure> {
    allocation.items(1)?;
    Ok(model::Binder::new(
        shape_id(catalog, binder.owner, allocation)?,
        binder.depth,
        binder.index,
        boxed("binder name", &binder.name, allocation)?,
    ))
}

fn binders_of(
    binders: Vec<protocol::Binder>,
    catalog: &Catalog,
    allocation: &mut Allocation,
) -> std::result::Result<Box<[model::Binder]>, ShapeFailure> {
    let mut converted = Vec::new();
    allocation.temporary_conversions(binders.len());
    allocation.reserve(&mut converted, binders.len())?;
    for binder in binders {
        converted.push(binder_of(binder, catalog, allocation)?);
    }
    Ok(converted.into_boxed_slice())
}

fn types(
    types: Vec<protocol::Type>,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<Box<[model::Type]>, ShapeFailure> {
    let mut converted = Vec::new();
    allocation.temporary_conversions(types.len());
    allocation.reserve(&mut converted, types.len())?;
    for ty in types {
        converted.push(type_of(ty, catalog, limits, allocation)?);
    }
    Ok(converted.into_boxed_slice())
}

fn bounds(
    bounds: Vec<protocol::Bound>,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<Box<[model::Bound]>, ShapeFailure> {
    let mut converted = Vec::new();
    allocation.temporary_conversions(bounds.len());
    allocation.reserve(&mut converted, bounds.len())?;
    for bound in bounds {
        converted.push(bound_of(bound, catalog, limits, allocation)?);
    }
    allocation.sort_keys(converted.len());
    converted.sort_unstable();
    converted.dedup();
    Ok(converted.into_boxed_slice())
}

fn predicates(
    predicates: Vec<protocol::Predicate>,
    catalog: &Catalog,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> std::result::Result<Box<[model::Predicate]>, ShapeFailure> {
    let mut converted = Vec::new();
    allocation.temporary_conversions(predicates.len());
    allocation.reserve(&mut converted, predicates.len())?;
    for predicate in predicates {
        converted.push(predicate_of(predicate, catalog, limits, allocation)?);
    }
    allocation.sort_keys(converted.len());
    converted.sort_unstable();
    converted.dedup();
    Ok(converted.into_boxed_slice())
}

fn ids(
    ids: Vec<protocol::Id>,
    catalog: &Catalog,
    allocation: &mut Allocation,
) -> std::result::Result<Box<[identity::Definition]>, ShapeFailure> {
    let mut converted = Vec::new();
    allocation.temporary_conversions(ids.len());
    allocation.reserve(&mut converted, ids.len())?;
    for id in ids {
        converted.push(shape_id(catalog, id, allocation)?);
    }
    allocation.sort_keys(converted.len());
    converted.sort_unstable();
    if converted.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(extraction(
            "duplicate-associated-definition",
            "associated definition identity",
        )
        .into());
    }
    Ok(converted.into_boxed_slice())
}

fn origin(raw: protocol::Origin, allocation: &mut Allocation) -> Result<model::Origin> {
    Ok(match raw {
        protocol::Origin::Source => model::Origin::Source,
        protocol::Origin::Expansion {
            call_site,
            definition_site,
        } => model::Origin::Expansion {
            call_site: span(call_site, allocation)?,
            definition_site: span(definition_site, allocation)?,
        },
        protocol::Origin::Synthesized { mechanism } => {
            model::Origin::Synthesized(boxed("synthesis mechanism", &mechanism, allocation)?)
        }
    })
}

fn span(raw: protocol::Span, allocation: &mut Allocation) -> Result<model::Span> {
    validate_span(&raw)?;
    allocation.items(1)?;
    allocation.string(&raw.file)?;
    Ok(model::Span::new(
        raw.file.into_boxed_str(),
        raw.byte_start,
        raw.byte_end,
        raw.line_start,
        raw.column_start,
        raw.line_end,
        raw.column_end,
    ))
}

fn validate_span(raw: &protocol::Span) -> Result<()> {
    nonempty("source span file", &raw.file)?;
    if raw.byte_start > raw.byte_end
        || raw.line_start == 0
        || raw.line_end == 0
        || (raw.line_start, raw.column_start) > (raw.line_end, raw.column_end)
    {
        return Err(extraction("invalid-source-span", &raw.file));
    }
    Ok(())
}

fn validate_origin(raw: &protocol::Origin) -> Result<()> {
    match raw {
        protocol::Origin::Source => Ok(()),
        protocol::Origin::Expansion {
            call_site,
            definition_site,
        } => {
            validate_span(call_site)?;
            validate_span(definition_site)
        }
        protocol::Origin::Synthesized { mechanism } => nonempty("synthesis mechanism", mechanism),
    }
}

/// Record `outcome` for `id`, which `sources[source]`'s fragment rejected;
/// another fragment that extracted or rejected it otherwise is a conflict.
fn reject(
    id: &identity::Definition,
    outcome: Rejected,
    source: usize,
    sources: &[conflict::Source],
    definitions: &BTreeMap<identity::Definition, conflict::Sourced<model::Definition>>,
    rejected: &mut BTreeMap<identity::Definition, conflict::Sourced<Rejected>>,
    allocation: &mut Allocation,
) -> Result<()> {
    if let Some(previous) = definitions.get(id) {
        return Err(extraction(
            "duplicate-definition",
            &conflict::mixed(
                (&previous.value, &sources[previous.fragment]),
                &sources[source],
            ),
        ));
    }
    match rejected.get(id) {
        Some(previous) if !previous.value.same(&outcome) => Err(extraction(
            "conflicting-definition",
            &conflict::rejection(
                id,
                (
                    &previous.value.reason,
                    &previous.value.span,
                    &sources[previous.fragment],
                ),
                (&outcome.reason, &outcome.span, &sources[source]),
                conflict::Detail::from_env(),
            ),
        )),
        Some(_) => Ok(()),
        None => {
            let key = allocation.clone_identity(id)?;
            allocation.items(1)?;
            rejected.insert(
                key,
                conflict::Sourced {
                    value: outcome,
                    fragment: source,
                },
            );
            Ok(())
        }
    }
}

fn invocation(
    fragment: &protocol::Fragment,
    allocation: &mut Allocation,
) -> Result<api::Invocation> {
    let handshake = &fragment.handshake;
    let protocol::Root::Selected { aliases } = &handshake.root;
    let root_aliases = canonical_strings(aliases, allocation)?;
    for value in [
        handshake.package.cargo_id.as_str(),
        handshake.target.as_str(),
        handshake.target_kind.as_str(),
        handshake.crate_name.as_str(),
        handshake.edition.as_str(),
        "target",
        handshake.host.as_str(),
        handshake.selected_target.as_str(),
        handshake.artifact.digest.as_str(),
        handshake.artifact.profile.as_str(),
        handshake.artifact.crate_hash.as_str(),
        handshake.artifact.stable_crate_id.as_str(),
        handshake.compiler_commit.as_str(),
        handshake.unit_correlation.as_str(),
        handshake.unit_key.as_str(),
    ] {
        allocation.string(value)?;
    }
    let crate_types = canonical_strings(&handshake.crate_types, allocation)?;
    let features = canonical_strings(&handshake.features, allocation)?;
    let dependencies = canonical_strings(&handshake.dependencies, allocation)?;
    let mut sysroot = Vec::new();
    allocation.reserve(&mut sysroot, handshake.sysroot.len())?;
    for crate_ in &handshake.sysroot {
        allocation.items(1)?;
        for value in [
            crate_.compiler_commit.as_str(),
            crate_.selected_target.as_str(),
            crate_.crate_name.as_str(),
            crate_.stable_crate_id.as_str(),
        ] {
            allocation.string(value)?;
        }
        sysroot.push(identity::Sysroot::new(
            crate_.compiler_commit.as_str().into(),
            crate_.selected_target.as_str().into(),
            crate_.crate_name.as_str().into(),
            crate_.stable_crate_id.as_str().into(),
        ));
    }
    Ok(api::Invocation::new(
        handshake.package.cargo_id.as_str().into(),
        root_aliases,
        handshake.target.as_str().into(),
        handshake.target_kind.as_str().into(),
        crate_types,
        handshake.crate_name.as_str().into(),
        handshake.edition.as_str().into(),
        "target".into(),
        handshake.host.as_str().into(),
        handshake.selected_target.as_str().into(),
        features,
        api::Artifact {
            length: handshake.artifact.length,
            digest: handshake.artifact.digest.as_str().into(),
            profile: handshake.artifact.profile.as_str().into(),
            crate_hash: handshake.artifact.crate_hash.as_str().into(),
            stable_crate_id: handshake.artifact.stable_crate_id.as_str().into(),
        },
        handshake.compiler_commit.as_str().into(),
        handshake.unit_correlation.as_str().into(),
        handshake.unit_key.as_str().into(),
        dependencies,
        sysroot.into_boxed_slice(),
    ))
}

fn canonical_strings(values: &[String], allocation: &mut Allocation) -> Result<Box<[Box<str>]>> {
    let mut canonical = Vec::new();
    allocation.reserve(&mut canonical, values.len())?;
    for value in values {
        allocation.string(value)?;
        canonical.push(value.as_str().into());
    }
    Ok(canonical.into_boxed_slice())
}

fn key(export: &identity::Export, allocation: &mut Allocation) -> Result<Key> {
    allocation.items(1)?;
    allocation.string(export.root_alias())?;
    let mut path = Vec::new();
    allocation.reserve(&mut path, export.path().len())?;
    for part in export.path() {
        allocation.string(part)?;
        path.push(Box::from(part));
    }
    Ok(Key {
        alias: export.root_alias().into(),
        path: path.into_boxed_slice(),
        namespace: export.namespace(),
    })
}

fn is_excluded(policy: &Policy, export: &Export) -> bool {
    let Some(paths) = policy.exclusions.get(export.key.root_alias()) else {
        return false;
    };
    paths.keys().any(|excluded| {
        excluded.len() == export.key.path().len()
            && excluded.iter().map(AsRef::as_ref).eq(export.key.path())
    })
}

fn namespace(namespace: protocol::Namespace) -> identity::Namespace {
    match namespace {
        protocol::Namespace::Type => identity::Namespace::Type,
        protocol::Namespace::Value => identity::Namespace::Value,
        protocol::Namespace::Macro => identity::Namespace::Macro,
    }
}

fn compilation(raw: &protocol::Compilation) -> identity::Compilation {
    match raw {
        protocol::Compilation::Target => identity::Compilation::Target,
    }
}

fn safety(raw: protocol::Safety) -> model::Safety {
    match raw {
        protocol::Safety::Safe => model::Safety::Safe,
        protocol::Safety::Unsafe => model::Safety::Unsafe,
    }
}

fn mutability_of(raw: protocol::Mutability) -> model::Mutability {
    match raw {
        protocol::Mutability::Shared => model::Mutability::Shared,
        protocol::Mutability::Mutable => model::Mutability::Mutable,
    }
}

fn polarity_of(raw: protocol::Polarity) -> model::Polarity {
    match raw {
        protocol::Polarity::Positive => model::Polarity::Positive,
        protocol::Polarity::Negative => model::Polarity::Negative,
    }
}

fn strings(
    field: &'static str,
    values: Vec<String>,
    allocation: &mut Allocation,
) -> Result<Box<[Box<str>]>> {
    for value in &values {
        allocation.string(value)?;
    }
    let mut boxed = Vec::new();
    allocation.reserve(&mut boxed, values.len())?;
    for value in values {
        nonempty(field, &value)?;
        boxed.push(value.into_boxed_str());
    }
    Ok(boxed.into_boxed_slice())
}

fn boxed(field: &'static str, value: &str, allocation: &mut Allocation) -> Result<Box<str>> {
    nonempty(field, value)?;
    allocation.owned(value)
}

fn prefixed(prefix: &str, value: &str, allocation: &mut Allocation) -> Result<Box<str>> {
    let length = prefix.len().saturating_add(value.len());
    allocation.items(1)?;
    allocation.bytes(length)?;
    let mut owned = String::new();
    owned
        .try_reserve_exact(length)
        .map_err(|error| Error::Allocation {
            operation: "owned string allocation",
            source: error,
        })?;
    owned.push_str(prefix);
    owned.push_str(value);
    Ok(owned.into_boxed_str())
}

/// An empty protocol string is a correctness failure.
fn nonempty(field: &'static str, value: &str) -> Result<()> {
    present(field, value)
}

fn present(field: &'static str, value: &str) -> Result<()> {
    if value.is_empty() {
        Err(extraction("empty-protocol-field", field))
    } else {
        Ok(())
    }
}

/// Checked-arithmetic overflow is a genuine failure and stays terminal.
fn arithmetic(operation: &'static str) -> Error {
    Error::Node {
        operation,
        source: Box::new(crate::node::Error::Allocation),
    }
}

fn sorted_unique(field: &'static str, values: &[String]) -> Result<()> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        Err(extraction("noncanonical-handshake-set", field))
    } else {
        Ok(())
    }
}

fn exact_hex(field: &'static str, value: &str, length: usize) -> Result<()> {
    if value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(extraction("invalid-protocol-digest", field))
    }
}

fn fallback_span(allocation: &mut Allocation) -> Result<model::Span> {
    allocation.items(1)?;
    Ok(model::Span::new(
        allocation.owned("<compiler>")?,
        0,
        0,
        1,
        0,
        1,
        0,
    ))
}

/// The terminal error of a canonical account latched by a genuine failure.
pub(crate) const REFUSED_ACCOUNT_OPERATION: &str = "canonical allocation account";

fn refused_account() -> Error {
    Error::Node {
        operation: REFUSED_ACCOUNT_OPERATION,
        source: Box::new(crate::node::Error::Allocation),
    }
}

/// Fixed (non-accounting) bounds keep their terminal `Error::Limit`.
fn limited(resource: &'static str, limit: usize, actual: usize) -> Result<()> {
    if actual <= limit {
        Ok(())
    } else {
        Err(Error::Limit {
            resource,
            limit,
            actual,
        })
    }
}

fn extraction(code: &'static str, reason: &str) -> Error {
    Error::Extraction {
        code,
        reason: reason.into(),
    }
}

#[cfg(all(test, feature = "rustc-driver"))]
mod api_protocol;

#[cfg(test)]
mod policy_bounds {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::{
        ManifestSpan, Policy, POLICY_BORROWED_PATH_OWNERSHIPS, POLICY_OWNED_PATH_RESERVES,
        POLICY_STRING_OWNERSHIPS,
    };

    fn span() -> ManifestSpan<'static> {
        ManifestSpan::new("Cargo.toml", 0, 1, 1, 0, 1, 1).expect("fixture span seals")
    }

    fn ownerships() -> usize {
        POLICY_STRING_OWNERSHIPS.with(Cell::get)
    }

    fn borrowed_ownerships() -> usize {
        POLICY_BORROWED_PATH_OWNERSHIPS.with(Cell::get)
    }

    fn owned_path_reserves() -> usize {
        POLICY_OWNED_PATH_RESERVES.with(Cell::get)
    }

    /// The retained exclusion owns each of its strings once and reserves its
    /// owned path once.
    #[test]
    fn exclusion_owns_each_string_once() {
        let before = ownerships();
        let reserves = owned_path_reserves();
        let _policy = Policy::manifest()
            .exclusion("dep", ["one", "two", "three"], "reason", span())
            .expect("the exclusion seals")
            .seal();
        assert_eq!(
            ownerships() - before,
            6,
            "alias, three components, reason and span file"
        );
        assert_eq!(owned_path_reserves() - reserves, 1);
    }

    /// The builder drains a finite path, borrowing and retaining every
    /// component.
    #[test]
    fn policy_builder_retains_every_path_component() {
        let generated = Rc::new(Cell::new(0_usize));
        let counter = Rc::clone(&generated);
        let iterator = std::iter::from_fn(move || {
            let next = counter.get().saturating_add(1);
            counter.set(next);
            (next <= 5).then_some("item")
        });
        let before = ownerships();
        let borrowed_before = borrowed_ownerships();
        let reserves = owned_path_reserves();
        let _policy = Policy::manifest()
            .exclusion("dep", iterator, "reason", span())
            .expect("the path is retained")
            .seal();
        assert_eq!(generated.get(), 6, "the builder drains the finite path");
        assert_eq!(borrowed_ownerships() - borrowed_before, 5);
        assert_eq!(
            ownerships() - before,
            8,
            "alias, reason, span file and five components"
        );
        assert_eq!(owned_path_reserves() - reserves, 1);
    }

    #[test]
    fn an_empty_span_file_is_refused() {
        assert!(ManifestSpan::new("", 0, 1, 1, 0, 1, 1).is_err());
    }
}

#[cfg(test)]
mod allocation {
    use std::collections::VecDeque;

    use serde::Serialize;

    use super::{
        enqueue_fixed, raw_pair, Allocation, PolicyUsage, RawBody, REFUSED_ACCOUNT_OPERATION,
    };
    use crate::api;
    use crate::Error;

    fn is_refused_account(error: &Error) -> bool {
        matches!(error, Error::Node { operation, .. } if *operation == REFUSED_ACCOUNT_OPERATION)
    }

    /// Charges accumulate, an owned string charges one item and its length,
    /// and decode admission charges nothing.
    #[test]
    fn charges_accumulate_and_decode_admission_charges_nothing() {
        let mut allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        allocation.items(3).unwrap();
        allocation.items(4).unwrap();
        allocation.bytes(5).unwrap();
        assert_eq!(allocation.usage(), (7, 5));
        let owned = allocation.owned("hello").unwrap();
        assert_eq!(&*owned, "hello");
        assert_eq!(allocation.usage(), (8, 10));
        allocation.admit_decode().unwrap();
        allocation.validate_retained().unwrap();
        assert_eq!(allocation.usage(), (8, 10));
        assert!(!allocation.is_refused());
    }

    /// Checked-arithmetic overflow is a genuine failure: it latches and stays
    /// terminal while usage keeps its last truthful value.
    /// A genuine failure latches the account: every later charge, decode
    /// admission, retained validation and reservation refuses, and the usage
    /// spent before the latch is kept. Charges past any figure never latch.
    #[test]
    fn genuine_failure_latches_and_large_charges_never_do() {
        let mut allocation = Allocation::with_limits(0, 0);
        allocation.items(1).expect("a charge never latches");
        allocation.bytes(1).expect("a charge never latches");
        assert!(!allocation.is_refused());
        allocation.refuse();
        assert!(allocation.is_refused());
        assert!(is_refused_account(&allocation.items(1).unwrap_err()));
        assert!(is_refused_account(&allocation.bytes(1).unwrap_err()));
        assert!(is_refused_account(&allocation.string("x").unwrap_err()));
        assert!(is_refused_account(&allocation.owned("x").unwrap_err()));
        assert!(is_refused_account(&allocation.admit_decode().unwrap_err()));
        assert!(is_refused_account(
            &allocation.validate_retained().unwrap_err()
        ));
        assert!(is_refused_account(
            &allocation.reserve(&mut Vec::<u8>::new(), 1).unwrap_err()
        ));
        assert_eq!(allocation.usage(), (1, 1));

        assert!(Allocation::refused().is_refused());
    }

    #[test]
    fn checked_overflow_latches_the_canonical_account() {
        let mut allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        allocation.items(3).unwrap();
        allocation.bytes(5).unwrap();
        let error = allocation
            .retain(PolicyUsage {
                items: usize::MAX,
                bytes: 0,
            })
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Node {
                operation: "canonical allocation items",
                ..
            }
        ));
        assert!(allocation.is_refused());
        assert_eq!(allocation.usage(), (3, 5));
        assert!(is_refused_account(&allocation.items(1).unwrap_err()));
    }

    /// `items` and `bytes` use checked arithmetic: a genuine overflow latches
    /// the account instead of saturating, and the latched account refuses
    /// decoder admission.
    #[test]
    fn item_and_byte_overflow_latches_and_refuses_decode_admission() {
        let mut items = Allocation::with_limits(usize::MAX, usize::MAX);
        items
            .items(usize::MAX)
            .expect("the exact maximum is measurable");
        let error = items.items(1).expect_err("one more overflows");
        assert!(matches!(
            error,
            Error::Node {
                operation: "canonical allocation items",
                ..
            }
        ));
        assert!(items.is_refused());
        assert_eq!(items.usage(), (usize::MAX, 0));
        assert!(is_refused_account(&items.items(0).unwrap_err()));

        let mut bytes = Allocation::with_limits(usize::MAX, usize::MAX);
        bytes
            .bytes(usize::MAX)
            .expect("the exact maximum is measurable");
        let error = bytes.bytes(1).expect_err("one more overflows");
        assert!(matches!(
            error,
            Error::Node {
                operation: "canonical allocation bytes",
                ..
            }
        ));
        assert!(bytes.is_refused());
        assert_eq!(bytes.usage(), (0, usize::MAX));
        assert!(is_refused_account(&bytes.bytes(0).unwrap_err()));

        assert!(is_refused_account(&bytes.admit_decode().unwrap_err()));
    }

    /// A path longer than its precharged capacity is charged in full: the
    /// account measures every byte and never refuses.
    #[test]
    fn long_paths_are_charged_their_full_length() {
        let path = std::path::Path::new("/abcdefgh");
        let mut exact = Allocation::with_limits(usize::MAX, usize::MAX);
        exact.path_capacity(9).unwrap();
        exact.validate_path(path, 9).unwrap();
        assert_eq!(exact.usage(), (1, 9));

        let mut long = Allocation::with_limits(usize::MAX, usize::MAX);
        long.path_capacity(4).unwrap();
        long.validate_path(path, 4)
            .expect("a long path is not refused");
        assert_eq!(long.usage(), (1, 9), "the full path length is charged");
        assert!(!long.is_refused());
    }

    #[test]
    fn raw_pair_splices_equal_encoding_each_whole_document() {
        #[derive(Serialize)]
        struct Whole<'a, C: Serialize, I: Serialize> {
            schema: u32,
            context: C,
            invocations: I,
            #[serde(flatten)]
            body: &'a RawBody<'a>,
        }
        let invocation: api::Invocation = serde_json::from_value(serde_json::json!({
            "package_id": "p", "root_aliases": ["a"], "target": "t", "target_kind": "lib",
            "crate_types": ["lib"], "crate_name": "m", "edition": "2021",
            "compilation": "c", "host": "h", "selected_target": "s", "features": [],
            "artifact": {"length": 1, "digest": "d", "profile": "dev",
                "crate_hash": "h1", "stable_crate_id": "i"},
            "compiler_commit": "x", "unit_correlation": "u1", "unit_key": "u",
            "dependencies": [], "sysroot": [],
        }))
        .unwrap();
        let invocations = [invocation];
        let context = api::Context::fixture();
        let implementations = [];
        let proofs = [];
        let body = RawBody {
            definitions: &[],
            rejected: &[],
            exports: &[],
            anonymous: &[],
            implementations: &implementations,
            scalar_predicate_proofs: &proofs,
        };
        let mut allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        let (raw, semantic) = raw_pair(&context, &invocations, &body, &mut allocation).unwrap();
        let whole = serde_json::to_vec(&Whole {
            schema: 3,
            context: &context,
            invocations: &invocations,
            body: &body,
        })
        .unwrap();
        assert_eq!(raw, whole);
        let whole = serde_json::to_vec(&Whole {
            schema: 3,
            context: api::semantic::Context(&context),
            invocations: api::semantic::Invocations(&invocations),
            body: &body,
        })
        .unwrap();
        assert_eq!(semantic, whole);
        assert_ne!(raw, semantic);
    }

    #[test]
    fn protocol_string_fields_are_owned_and_stay_nonempty_proofs() {
        let mut allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        let owned = super::boxed("package name", "abcdef", &mut allocation).unwrap();
        assert_eq!(&*owned, "abcdef");
        assert!(matches!(
            super::boxed("package name", "", &mut allocation),
            Err(Error::Extraction {
                code: "empty-protocol-field",
                ..
            })
        ));
    }

    #[test]
    fn fixed_queue_charges_its_entries_and_refuses_only_when_latched() {
        let mut queue = VecDeque::new();
        let mut allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        enqueue_fixed(&mut queue, 7, &mut allocation).unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(allocation.usage().0, 1);
        let mut queue = VecDeque::new();
        let mut refused = Allocation::refused();
        let before = refused.usage();
        assert!(is_refused_account(
            &enqueue_fixed(&mut queue, 7, &mut refused).unwrap_err()
        ));
        assert!(queue.is_empty());
        assert_eq!(refused.usage(), before);
    }
}

#[cfg(test)]
mod support_closure_tests {
    use std::collections::{BTreeMap, VecDeque};

    use super::{enqueue_fixed, finalize_support, Allocation, PendingSupport, SupportKey};
    use crate::api::identity::{Authority, Compilation, Definition, Namespace, Package, Unit};
    use crate::api::{ledger, model};

    fn id(hash: &str) -> Definition {
        let package = Package::new(
            "path+file:///fixture#fixture@0.1.0".into(),
            "fixture".into(),
            "0.1.0".into(),
            "path+file:///fixture".into(),
        );
        let unit = Unit::new(
            Authority::Cargo(package),
            "fixture".into(),
            "lib".into(),
            "fixture".into(),
            Compilation::Target,
            Box::default(),
            "0000000000000001".into(),
        );
        Definition::new(unit, hash.into(), Namespace::Type)
    }

    fn edge(id: Definition, provenance: Definition) -> SupportKey {
        SupportKey {
            id,
            provenance,
            role: ledger::SupportRole::Definition,
        }
    }

    #[test]
    fn rejected_descendant_propagates_to_every_ancestor_edge() {
        let a = id("a");
        let b = id("b");
        let c = id("c");
        let span = model::Span::new("fixture.rs".into(), 0, 1, 1, 0, 1, 1);
        let pending = BTreeMap::from([
            (
                edge(b.clone(), a.clone()),
                PendingSupport::Present {
                    id: b.clone(),
                    provenance: a,
                    role: ledger::SupportRole::Definition,
                },
            ),
            (
                edge(c.clone(), b.clone()),
                PendingSupport::Rejected {
                    id: c.clone(),
                    provenance: b,
                    role: ledger::SupportRole::Definition,
                    reason: ledger::Reason::UnresolvedDefinition(c),
                    span,
                },
            ),
        ]);
        let support = finalize_support(
            pending,
            &mut Allocation::with_limits(usize::MAX, usize::MAX),
        )
        .expect("bounded reverse propagation succeeds");
        assert!(support
            .values()
            .all(|outcome| matches!(outcome, ledger::Support::Rejected { .. })));
    }

    #[test]
    fn complete_cycle_without_a_rejection_seed_remains_complete() {
        let a = id("a");
        let b = id("b");
        let pending = BTreeMap::from([
            (
                edge(b.clone(), a.clone()),
                PendingSupport::Present {
                    id: b.clone(),
                    provenance: a.clone(),
                    role: ledger::SupportRole::Definition,
                },
            ),
            (
                edge(a.clone(), b.clone()),
                PendingSupport::Present {
                    id: a,
                    provenance: b,
                    role: ledger::SupportRole::Definition,
                },
            ),
        ]);
        let support = finalize_support(
            pending,
            &mut Allocation::with_limits(usize::MAX, usize::MAX),
        )
        .expect("bounded cycle finalization succeeds");
        assert!(support
            .values()
            .all(|outcome| matches!(outcome, ledger::Support::Complete { .. })));
    }

    #[test]
    fn fixed_queue_charges_before_owning_the_boundary_entry() {
        let mut queue = VecDeque::new();
        let mut allocation = Allocation::refused();
        let before = allocation.usage();
        assert!(matches!(
            enqueue_fixed(&mut queue, 7, &mut allocation),
            Err(crate::Error::Node { operation, .. })
                if operation == super::REFUSED_ACCOUNT_OPERATION
        ));
        assert!(queue.is_empty());
        assert_eq!(allocation.usage(), before);
    }
}
