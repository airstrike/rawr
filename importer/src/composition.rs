//! Connected authenticated-hit reuse and retained checked-source consumption.

use crate::node::staging::{Outcome, Owned, Recipe};
use crate::{checked, node};

pub mod profile;
#[cfg(feature = "rustc-driver")]
mod resolved;
mod source;

#[cfg(feature = "rustc-driver")]
pub use resolved::{record as record_resolved, resolve_api_semantic, Warm};

#[cfg(test)]
mod tests;

type Profile = crate::profile::cached::Product;
type Generic = crate::classify::cached::Product;
type Facade = crate::surface::facade::cached::Product;

/// Current contract, profile and facade with the selected checked-output authority.
pub struct Completed<C = checked::Cached> {
    /// Generic Rust contract used by the checked program.
    pub contract: crate::Contract<crate::Classified>,
    /// Framework profile selected for this graph.
    pub profile: crate::profile::Contract,
    /// Canonical compiler facade.
    pub facade: crate::surface::facade::Cached,
    /// Checked program and symbolic demand, current or historical as typed by `C`.
    pub checked: C,
}

/// Reuse exact authenticated prefix hits, or carry missing pure work to the
/// genuine checked-source consumer under one retained ownership lifetime.
pub fn evaluate<B, C, E, T>(
    graph: crate::api::Graph<crate::api::Extracted>,
    adapter: &crate::profile::trusted::Adapter<T>,
    policy: &crate::classify::Policy,
    snapshot: crate::surface::Snapshot,
    limits: &crate::cargo::Limits,
    execute: impl FnOnce(
        &crate::Contract<crate::Classified>,
        &crate::profile::Contract,
        &crate::surface::facade::Cached,
    ) -> Result<(crate::Demand<crate::Sealed, B>, C), E>,
) -> Result<Completed, checked::Evaluation<E>>
where
    C: crate::profile::Checked,
    T: crate::profile::trusted::Operations,
{
    evaluate_with::<B, C, E, T, checked::Cached, _>(
        source::Current::capture(graph).map_err(cache)?,
        adapter,
        policy,
        snapshot,
        limits,
        false,
        execute,
    )
}

/// Carry checked output into audited profile generation without promoting it.
pub fn evaluate_retained<B, C, E, T>(
    graph: crate::api::Graph<crate::api::Extracted>,
    adapter: &crate::profile::trusted::Adapter<T>,
    policy: &crate::classify::Policy,
    snapshot: crate::surface::Snapshot,
    limits: &crate::cargo::Limits,
    execute: impl FnOnce(
        &crate::Contract<crate::Classified>,
        &crate::profile::Contract,
        &crate::surface::facade::Cached,
    ) -> Result<(crate::Demand<crate::Sealed, B>, C), E>,
) -> Result<Completed<checked::Retained>, checked::Evaluation<E>>
where
    C: crate::profile::Checked,
    T: crate::profile::trusted::Operations,
{
    evaluate_with::<B, C, E, T, checked::Retained, _>(
        source::Current::capture(graph).map_err(cache)?,
        adapter,
        policy,
        snapshot,
        limits,
        false,
        execute,
    )
}

/// Continue owned API capture through the actual source-consumer union.
#[cfg(feature = "rustc-driver")]
pub fn evaluate_api<B, C, E, T>(
    graph: crate::api::Continuation,
    adapter: &crate::profile::trusted::Adapter<T>,
    policy: &crate::classify::Policy,
    snapshot: crate::surface::Snapshot,
    limits: &crate::cargo::Limits,
    execute: impl FnOnce(
        &crate::Contract<crate::Classified>,
        &crate::profile::Contract,
        &crate::surface::facade::Cached,
    ) -> Result<(crate::Demand<crate::Sealed, B>, C), E>,
) -> Result<Completed<checked::Retained>, checked::Evaluation<E>>
where
    C: crate::profile::Checked,
    T: crate::profile::trusted::Operations,
{
    evaluate_with::<B, C, E, T, checked::Retained, _>(
        graph.materialize().map_err(cache)?,
        adapter,
        policy,
        snapshot,
        limits,
        false,
        execute,
    )
}

/// Select an audited source callback whose output is independent of exact API
/// placement, raw evidence and accounting observations. The current contracts
/// are still supplied unchanged. Profile classification requires its separate
/// `Adapter::semantic_api` selection; this does not opt arbitrary callbacks in.
pub fn evaluate_semantic<B, C, E, T>(
    graph: crate::api::Graph<crate::api::Extracted>,
    adapter: &crate::profile::trusted::Adapter<T>,
    policy: &crate::classify::Policy,
    snapshot: crate::surface::Snapshot,
    limits: &crate::cargo::Limits,
    execute: impl FnOnce(
        &crate::Contract<crate::Classified>,
        &crate::profile::Contract,
        &crate::surface::facade::Cached,
    ) -> Result<(crate::Demand<crate::Sealed, B>, C), E>,
) -> Result<Completed, checked::Evaluation<E>>
where
    C: crate::profile::Checked,
    T: crate::profile::trusted::Operations,
{
    evaluate_with::<B, C, E, T, checked::Cached, _>(
        source::Current::capture(graph).map_err(cache)?,
        adapter,
        policy,
        snapshot,
        limits,
        true,
        execute,
    )
}

/// Select an audited source callback whose output is independent of exact API
/// placement, raw evidence and accounting observations. The current contracts
/// are still supplied unchanged. Profile classification requires its separate
/// `Adapter::semantic_api` selection; this does not opt arbitrary callbacks in.
pub fn evaluate_retained_semantic<B, C, E, T>(
    graph: crate::api::Graph<crate::api::Extracted>,
    adapter: &crate::profile::trusted::Adapter<T>,
    policy: &crate::classify::Policy,
    snapshot: crate::surface::Snapshot,
    limits: &crate::cargo::Limits,
    execute: impl FnOnce(
        &crate::Contract<crate::Classified>,
        &crate::profile::Contract,
        &crate::surface::facade::Cached,
    ) -> Result<(crate::Demand<crate::Sealed, B>, C), E>,
) -> Result<Completed<checked::Retained>, checked::Evaluation<E>>
where
    C: crate::profile::Checked,
    T: crate::profile::trusted::Operations,
{
    evaluate_with::<B, C, E, T, checked::Retained, _>(
        source::Current::capture(graph).map_err(cache)?,
        adapter,
        policy,
        snapshot,
        limits,
        true,
        execute,
    )
}

/// Select an audited source callback whose output is independent of exact API
/// placement, raw evidence and accounting observations. The current contracts
/// are still supplied unchanged. Profile classification requires its separate
/// `Adapter::semantic_api` selection; this does not opt arbitrary callbacks in.
#[cfg(feature = "rustc-driver")]
pub fn evaluate_api_semantic<B, C, E, T>(
    graph: crate::api::Continuation,
    adapter: &crate::profile::trusted::Adapter<T>,
    policy: &crate::classify::Policy,
    snapshot: crate::surface::Snapshot,
    limits: &crate::cargo::Limits,
    execute: impl FnOnce(
        &crate::Contract<crate::Classified>,
        &crate::profile::Contract,
        &crate::surface::facade::Cached,
    ) -> Result<(crate::Demand<crate::Sealed, B>, C), E>,
) -> Result<Completed<checked::Retained>, checked::Evaluation<E>>
where
    C: crate::profile::Checked,
    T: crate::profile::trusted::Operations,
{
    evaluate_with::<B, C, E, T, checked::Retained, _>(
        graph.materialize().map_err(cache)?,
        adapter,
        policy,
        snapshot,
        limits,
        true,
        execute,
    )
}

fn evaluate_with<B, C, E, T, O: checked::retained::Output, S: source::Source>(
    graph: S,
    adapter: &crate::profile::trusted::Adapter<T>,
    policy: &crate::classify::Policy,
    snapshot: crate::surface::Snapshot,
    limits: &crate::cargo::Limits,
    semantic_source: bool,
    execute: impl FnOnce(
        &crate::Contract<crate::Classified>,
        &crate::profile::Contract,
        &crate::surface::facade::Cached,
    ) -> Result<(crate::Demand<crate::Sealed, B>, C), E>,
) -> Result<Completed<O>, checked::Evaluation<E>>
where
    C: crate::profile::Checked,
    T: crate::profile::trusted::Operations,
{
    let owned_context = graph.context().clone();
    let context = &owned_context;
    if context.limits().node_output_bytes() != limits.node_output_bytes() {
        return Err(cache(node::Error::State(
            "connected classification limits changed",
        )));
    }
    let initial = graph
        .retention()
        .map_err(cache)?
        .try_clone()
        .map_err(cache)?;
    let retention = &initial;
    let store = context.open_store().map_err(cache)?;
    let evaluator = node::Evaluator::new(&store, &crate::metrics::Current);
    let observations = graph.callback_observations();
    let profile_exact = (!adapter.uses_semantic_api()).then_some(&observations);
    let source_exact = (!semantic_source).then_some(&observations);
    let profile_recipe = Recipe::<Profile>::from_inputs(
        context,
        &[graph.input()],
        &crate::profile::cached::authority(adapter.identity(), adapter.version(), profile_exact)
            .map_err(|error| cache(failure(error)))?,
        limits.node_output_bytes(),
    )
    .map_err(cache)?;
    let generic_recipe = Recipe::<Generic>::from_inputs(
        context,
        &[graph.input()],
        &crate::classify::cached::authority(policy).map_err(|error| cache(failure(error)))?,
        node::MAX_CONTENT_BYTES,
    )
    .map_err(cache)?;
    let profile = profile_recipe.lookup_owned(&evaluator).map_err(cache)?;
    let generic = generic_recipe.lookup_owned(&evaluator).map_err(cache)?;
    // Only the generic parent selects the facade. A missing profile must not
    // fabricate a sibling dependency or prevent an already usable facade hit.
    let facade = match &generic {
        Outcome::Hit(generic) => {
            let parts = generic
                .value()
                .replay(graph.graph(), policy, limits)
                .map_err(|error| cache(failure(error)))?;
            let recipe = Recipe::<Facade>::from_inputs(
                context,
                &[generic.input()],
                &crate::surface::facade::cached::authority(parts.fingerprint.as_bytes())
                    .map_err(cache)?,
                limits.node_output_bytes(),
            )
            .map_err(cache)?;
            let outcome = recipe.lookup_owned(&evaluator).map_err(cache)?;
            Some((recipe, outcome, parts))
        }
        Outcome::Miss(_) => None,
    };
    let prepared_checked = match (&profile, &facade) {
        (Outcome::Hit(profile), Some((_, Outcome::Hit(facade), parts))) => {
            let profile_parts = profile
                .value()
                .owned(adapter.identity(), adapter.version(), graph.graph(), limits)
                .map_err(|error| cache(failure(error)))?;
            let authority = checked::authority(
                parts.fingerprint,
                graph.graph().fingerprint(),
                profile_parts.identity(),
                profile_parts.version(),
                profile_parts.rules_digest(),
                profile_parts.graph_fingerprint(),
                snapshot,
                source_exact,
            )
            .map_err(|error| cache(failure(error)))?;
            let recipe = Recipe::<checked::Product>::from_inputs(
                context,
                &[facade.input(), profile.input()],
                &authority,
                limits.node_output_bytes(),
            )
            .map_err(cache)?;
            let outcome = recipe.lookup_owned(&evaluator).map_err(cache)?;
            Some((recipe, outcome, profile_parts))
        }
        _ => None,
    };
    match (profile, generic, facade, prepared_checked) {
        (
            Outcome::Hit(Owned::Current(profile)),
            Outcome::Hit(Owned::Current(generic)),
            Some((_, Outcome::Hit(Owned::Current(facade)), parts)),
            Some((_, Outcome::Hit(Owned::Current(checked)), profile_parts)),
        ) => {
            // All four usable receipts keep the established hit path. Carry
            // original lineage and bundle commitments without a seal or pin.
            let retained = retention
                .extend_current(&profile)
                .map_err(cache)?
                .extend_current(&generic)
                .map_err(cache)?
                .extend_current(&facade)
                .map_err(cache)?
                .extend_current(&checked)
                .map_err(cache)?;
            let (contract, profile, facade) = current(
                graph.current().map_err(cache)?,
                profile_parts,
                parts,
                context,
                profile,
                generic,
                facade,
            )
            .map_err(cache)?;
            let checked = O::complete(context.clone(), Owned::Current(checked), retained, &store)
                .map_err(checked::Evaluation::Cache)?;
            Ok(Completed {
                contract,
                profile,
                facade,
                checked,
            })
        }
        (profile, generic, facade, prepared_checked) => {
            let mut transform = checked::Transform::new();
            let result = store.with_staged_batch(retention, &crate::metrics::Current, |batch| {
                let profile = acquire(profile, &profile_recipe, batch, || {
                    let parts = adapter
                        .classify_parts(graph.graph(), limits)
                        .map_err(failure)?;
                    Profile::capture_parts(&parts, limits.node_output_bytes()).map_err(failure)
                })?;
                let generic = acquire(generic, &generic_recipe, batch, || {
                    Generic::capture(graph.graph(), policy, limits).map_err(failure)
                })?;
                let (facade_recipe, facade_outcome, parts) = match facade {
                    Some(prepared) => prepared,
                    None => {
                        let parts = generic
                            .value()
                            .replay(graph.graph(), policy, limits)
                            .map_err(failure)?;
                        let recipe = Recipe::<Facade>::from_inputs(
                            context,
                            &[generic.input()],
                            &crate::surface::facade::cached::authority(
                                parts.fingerprint.as_bytes(),
                            )?,
                            limits.node_output_bytes(),
                        )?;
                        let outcome = recipe.lookup_owned(&evaluator)?;
                        (recipe, outcome, parts)
                    }
                };
                let facade = acquire(facade_outcome, &facade_recipe, batch, || {
                    Facade::capture_parts(graph.graph(), &parts, limits)
                })?;
                let (checked_recipe, checked_outcome, profile_parts) = match prepared_checked {
                    Some(prepared) => prepared,
                    None => {
                        let profile_parts = profile
                            .value()
                            .owned(adapter.identity(), adapter.version(), graph.graph(), limits)
                            .map_err(failure)?;
                        let authority = checked::authority(
                            parts.fingerprint,
                            graph.graph().fingerprint(),
                            profile_parts.identity(),
                            profile_parts.version(),
                            profile_parts.rules_digest(),
                            profile_parts.graph_fingerprint(),
                            snapshot,
                            source_exact,
                        )
                        .map_err(failure)?;
                        let recipe = Recipe::<checked::Product>::from_inputs(
                            context,
                            &[facade.input(), profile.input()],
                            &authority,
                            limits.node_output_bytes(),
                        )?;
                        let outcome = recipe.lookup_owned(&evaluator)?;
                        (recipe, outcome, profile_parts)
                    }
                };
                match checked_outcome {
                    Outcome::Hit(checked) => {
                        let checked = batch.carry_owned(checked)?;
                        let (graph, profile, generic, facade, checked) =
                            graph.promote_checked(profile, generic, facade, checked, &store)?;
                        let retained = batch.retention().try_clone()?;
                        let (contract, profile, facade) = current(
                            graph,
                            profile_parts,
                            parts,
                            context,
                            profile,
                            generic,
                            facade,
                        )?;
                        let checked =
                            O::complete(context.clone(), Owned::Current(checked), retained, &store)
                                .map_err(output_failure)?;
                        Ok(Completed {
                            contract,
                            profile,
                            facade,
                            checked,
                        })
                    }
                    Outcome::Miss(miss) => {
                        let mut pending =
                            Some((profile, generic, facade, profile_parts, parts, graph));
                        let mut consumed = None;
                        let checked = checked_recipe.resume(batch, miss, || {
                            let (profile, generic, facade, profile_parts, parts, graph) = pending
                                .take()
                                .ok_or(node::Error::State("checked parent inputs disappeared"))?;
                            // The resume callback runs after action ownership and
                            // authoritative winner lookup. Validate immediately
                            // before the real source consumer, never before a wait.
                            let (graph, profile, generic, facade) =
                                graph.promote(profile, generic, facade, &store)?;
                            let (contract, profile, facade) = current(
                                graph,
                                profile_parts,
                                parts,
                                context,
                                profile,
                                generic,
                                facade,
                            )?;
                            let product =
                                transform.capture(contract.fingerprint(), snapshot, limits, || {
                                    execute(&contract, &profile, &facade)
                                });
                            consumed = Some((contract, profile, facade));
                            product
                        })?;
                        let (contract, profile, facade, checked) = match consumed {
                            Some((contract, profile, facade)) => {
                                (contract, profile, facade, Owned::Staged(checked))
                            }
                            None => {
                                // Another owner published while this continuation
                                // waited. No source callback ran; promote the exact
                                // retained branch with that winner only once.
                                let (profile, generic, facade, profile_parts, parts, graph) =
                                    pending.take().ok_or(node::Error::State(
                                        "checked winner inputs disappeared",
                                    ))?;
                                let (graph, profile, generic, facade, checked) = graph
                                    .promote_checked(
                                        profile,
                                        generic,
                                        facade,
                                        Owned::Staged(checked),
                                        &store,
                                    )?;
                                let (contract, profile, facade) = current(
                                    graph,
                                    profile_parts,
                                    parts,
                                    context,
                                    profile,
                                    generic,
                                    facade,
                                )?;
                                (contract, profile, facade, Owned::Current(checked))
                            }
                        };
                        let retained = batch.retention().try_clone()?;
                        let checked = O::complete(context.clone(), checked, retained, &store)
                            .map_err(output_failure)?;
                        Ok(Completed {
                            contract,
                            profile,
                            facade,
                            checked,
                        })
                    }
                }
            });
            transform.resolve(result)
        }
    }
}

fn acquire<K: node::Value>(
    outcome: Outcome<K>,
    recipe: &Recipe<K>,
    batch: &mut crate::store::Batch<'_>,
    execute: impl FnOnce() -> Result<K, node::Error>,
) -> Result<Owned<K>, node::Error> {
    match outcome {
        Outcome::Hit(value) => batch.carry_owned(value),
        Outcome::Miss(miss) => recipe.resume(batch, miss, execute).map(Owned::Staged),
    }
}

fn current(
    graph: crate::api::Graph<crate::api::Extracted>,
    profile_parts: crate::profile::contract::Parts,
    generic_parts: crate::classify::ClassifiedParts,
    context: &crate::context::Context,
    profile: node::Validated<Profile>,
    generic: node::Validated<Generic>,
    facade: node::Validated<Facade>,
) -> Result<
    (
        crate::Contract<crate::Classified>,
        crate::profile::Contract,
        crate::surface::facade::Cached,
    ),
    node::Error,
> {
    let profile = profile_parts.into_current(node::Chain::validated(context.clone(), &profile));
    let contract = generic_parts.into_contract(
        graph,
        Some(node::Chain::validated(context.clone(), &generic)),
    );
    let facade = crate::surface::facade::Cached::from_validated(context.clone(), facade);
    Ok((contract, profile, facade))
}

fn failure(error: impl std::fmt::Display) -> node::Error {
    node::Error::Transform(error.to_string().into_boxed_str())
}

fn cache<E>(error: node::Error) -> checked::Evaluation<E> {
    checked::Evaluation::Cache(checked::Error::Node(Box::new(error)))
}

fn output_failure(error: checked::Error) -> node::Error {
    match error {
        checked::Error::Node(error) => *error,
        error => failure(error),
    }
}
