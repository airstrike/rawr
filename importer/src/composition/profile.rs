//! Retained audited demand and generation up to the runner consumer boundary.

use crate::node;

pub(crate) mod pair;
use crate::node::staging::{Outcome, Owned, Recipe};
use crate::profile::demand::cached::{self, GeneratedProduct, Product};
use crate::profile::{Demand, Error, Proof};
pub use pair::Pair;

#[cfg(test)]
mod tests;

/// Reuse or compose the audited profile outputs, making them current before
/// the caller enters runner request construction and its actual consumers.
pub fn evaluate<T: crate::profile::trusted::Operations>(
    adapter: &crate::profile::trusted::Adapter<T>,
    contract: &crate::profile::Contract,
    checked: &crate::checked::Cached,
    limits: &crate::cargo::Limits,
) -> Result<(Demand, Proof), Error> {
    let parent = checked.node_chain();
    if parent.output_limit() != limits.node_output_bytes() {
        return Err(Error::DefinitionDrift);
    }
    evaluate_parts(
        adapter,
        contract,
        parent.context(),
        parent.retention().ok_or(Error::ForeignEvidence)?,
        node::staging::Input::current(parent.current_parent()),
        checked.checked_fingerprint(),
        checked.checked_payload(),
        limits,
    )
}

/// Continue from authenticated historical checked output under its original account.
pub fn evaluate_retained<T: crate::profile::trusted::Operations>(
    adapter: &crate::profile::trusted::Adapter<T>,
    contract: &crate::profile::Contract,
    checked: &crate::checked::Retained,
    limits: &crate::cargo::Limits,
) -> Result<(Demand, Proof), Error> {
    if checked.context.limits().node_output_bytes() != limits.node_output_bytes() {
        return Err(Error::DefinitionDrift);
    }
    evaluate_parts(
        adapter,
        contract,
        &checked.context,
        &checked.retention,
        checked.product.input(),
        checked.checked_fingerprint(),
        checked.checked_payload(),
        limits,
    )
}

/// Retain generated outputs until the trusted runner consumes its harness.
pub fn prepare_retained<T: crate::profile::trusted::Operations>(
    adapter: &crate::profile::trusted::Adapter<T>,
    contract: &crate::profile::Contract,
    checked: &crate::checked::Retained,
    limits: &crate::cargo::Limits,
) -> Result<Pair, Error> {
    if checked.context.limits().node_output_bytes() != limits.node_output_bytes() {
        return Err(Error::DefinitionDrift);
    }
    evaluate_parts(
        adapter,
        contract,
        &checked.context,
        &checked.retention,
        checked.product.input(),
        checked.checked_fingerprint(),
        checked.checked_payload(),
        limits,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "exact historical checked data and original ancestry travel together"
)]
fn evaluate_parts<T: crate::profile::trusted::Operations, O: pair::Output>(
    adapter: &crate::profile::trusted::Adapter<T>,
    contract: &crate::profile::Contract,
    context: &crate::context::Context,
    initial: &node::read_set::Retention,
    input: node::staging::Input<'_>,
    fingerprint: [u8; 32],
    payload: &[u8],
    limits: &crate::cargo::Limits,
) -> Result<O, Error> {
    if adapter.identity() != contract.identity() || adapter.version() != contract.version() {
        return Err(Error::DefinitionDrift);
    }
    let profile_parent = contract.node_chain().ok_or(Error::ForeignEvidence)?;
    initial
        .covers_parents(std::slice::from_ref(profile_parent.current_parent()))
        .map_err(node_error)?;

    let store = context.open_store().map_err(node_error)?;
    let evaluator = node::Evaluator::new(&store, &crate::metrics::Current);
    let lifecycle = adapter.owns_product_lifecycle();
    let recipe = Recipe::<Product>::from_inputs(
        context,
        &[input],
        &cached::demand_authority(
            adapter.identity(),
            adapter.version(),
            contract.rules_digest(),
            &fingerprint,
            payload,
            limits,
            (!adapter.uses_semantic_api()).then_some(&contract.graph_fingerprint()),
        )?,
        limits.node_output_bytes(),
    )
    .map_err(node_error)?;
    let demand = recipe.lookup_owned(&evaluator).map_err(node_error)?;
    let generated = match &demand {
        Outcome::Hit(demand) => {
            let parts = demand
                .value()
                .owned(contract, fingerprint, payload, limits)?;
            let recipe = generated_recipe(context, adapter, demand, parts.value(), limits)?;
            let generated = recipe.lookup_owned(&evaluator).map_err(node_error)?;
            Some((recipe, generated, parts))
        }
        Outcome::Miss(_) => None,
    };
    match (demand, generated) {
        (
            Outcome::Hit(Owned::Current(demand)),
            Some((_, Outcome::Hit(Owned::Current(generated)), parts)),
        ) => {
            let proof = generated.value().owned(parts.value(), lifecycle, limits)?;
            let retained_demand = initial.extend_current(&demand).map_err(node_error)?;
            let retained_generated = retained_demand
                .extend_current(&generated)
                .map_err(node_error)?;
            O::complete(
                Pair {
                    context: context.clone(),
                    demand: Owned::Current(demand),
                    generated: Owned::Current(generated),
                    parts,
                    proof,
                    demand_retention: retained_demand,
                    retention: retained_generated,
                },
                &store,
            )
        }
        (demand, generated) => store
            .with_staged_batch(initial, &crate::metrics::Current, |batch| {
                let demand = match demand {
                    Outcome::Hit(value) => batch.carry_owned(value)?,
                    Outcome::Miss(miss) => Owned::Staged(recipe.resume(batch, miss, || {
                        let value = adapter
                            .demand_owned(contract, fingerprint, payload, limits)
                            .map_err(failure)?;
                        Product::capture(&value, limits.node_output_bytes()).map_err(failure)
                    })?),
                };
                let retained_demand = batch.retention().try_clone()?;
                let (generated_recipe, generated, parts) = match generated {
                    Some(prepared) => prepared,
                    None => {
                        let parts = demand
                            .value()
                            .owned(contract, fingerprint, payload, limits)
                            .map_err(failure)?;
                        let recipe =
                            generated_recipe(context, adapter, &demand, parts.value(), limits)
                                .map_err(failure)?;
                        let outcome = recipe.lookup_owned(&evaluator)?;
                        (recipe, outcome, parts)
                    }
                };
                let generated = match generated {
                    Outcome::Hit(value) => batch.carry_owned(value)?,
                    Outcome::Miss(miss) => {
                        Owned::Staged(generated_recipe.resume(batch, miss, || {
                            let proof = adapter
                                .generate_owned(parts.value(), limits)
                                .map_err(failure)?;
                            GeneratedProduct::capture(&proof, limits.node_output_bytes())
                                .map_err(failure)
                        })?)
                    }
                };
                let proof = generated
                    .value()
                    .owned(parts.value(), lifecycle, limits)
                    .map_err(failure)?;
                let retained_generated = batch.retention().try_clone()?;
                O::complete(
                    Pair {
                        context: context.clone(),
                        demand,
                        generated,
                        parts,
                        proof,
                        demand_retention: retained_demand,
                        retention: retained_generated,
                    },
                    &store,
                )
                .map_err(failure)
            })
            .map_err(node_error),
    }
}

fn generated_recipe<T: crate::profile::trusted::Operations>(
    context: &crate::context::Context,
    adapter: &crate::profile::trusted::Adapter<T>,
    parent: &Owned<Product>,
    demand: &Demand,
    limits: &crate::cargo::Limits,
) -> Result<Recipe<GeneratedProduct>, Error> {
    Recipe::from_inputs(
        context,
        &[parent.input()],
        &cached::generated_authority(
            adapter.identity(),
            adapter.version(),
            demand.rules_digest(),
            demand.checked_fingerprint(),
            demand.digest(),
            adapter.owns_product_lifecycle(),
        )?,
        limits.node_output_bytes(),
    )
    .map_err(node_error)
}

fn finish(
    context: &crate::context::Context,
    demand: node::Validated<Product>,
    generated: node::Validated<GeneratedProduct>,
    parts: crate::profile::demand::Parts,
    proof: crate::profile::Parts,
    retained_demand: node::read_set::Retention,
    retained_generated: node::read_set::Retention,
) -> Result<(Demand, Proof), Error> {
    let demand_chain = node::Chain::validated_retained(context.clone(), &demand, retained_demand)
        .map_err(node_error)?;
    let generated_chain =
        node::Chain::validated_retained(context.clone(), &generated, retained_generated)
            .map_err(node_error)?;
    Ok((
        parts.into_current(demand_chain),
        proof.into_current(generated_chain),
    ))
}

fn failure(error: impl std::fmt::Display) -> node::Error {
    node::Error::Transform(error.to_string().into_boxed_str())
}
fn node_error(error: node::Error) -> Error {
    Error::Node(Box::new(error))
}
