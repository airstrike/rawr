//! Sealed resolved demand: the demand plans and the contract/profile identities
//! a launch needs, reused when nothing changed without the API graph or the
//! classification replay (spreadsheet principle).
//!
//! The product is a direct child of the API node. Its key binds that node
//! (owner, Cargo inputs, toolchain, extraction policy), the classification
//! policy, the profile adapter identity/version, the Rawr source snapshot and
//! the surface schema. A hit still requires current hits for the profile,
//! generic, facade and checked nodes under the recorded identities, and the
//! checked demand bytes must be the ones its plans were resolved for; anything
//! else is a miss and the full path runs unchanged.

use serde::{Deserialize, Serialize};

use super::{Facade, Generic, Profile};
use crate::node::{self, staging::Outcome, staging::Owned, staging::Recipe};
use crate::{checked, surface};

const DOMAIN: &[u8] = b"rawr.surface.resolved/v1";

/// Everything a warm launch needs that classification would otherwise derive.
#[derive(Deserialize, Serialize)]
struct Header {
    contract: [u8; 32],
    api: [u8; 32],
    semantic: [u8; 32],
    items: u64,
    bytes: u64,
    demand: [u8; 32],
}

#[derive(Serialize)]
struct Encoding<'a> {
    header: &'a Header,
    plans: &'a [surface::adapter::Plan],
}

#[derive(Deserialize)]
struct Decoding<P> {
    header: Header,
    plans: P,
}

pub(crate) struct Product {
    header: Header,
    bytes: Box<[u8]>,
}

impl Product {
    fn capture(header: Header, plans: &[surface::adapter::Plan]) -> Result<Self, node::Error> {
        let bytes = serde_json::to_vec(&Encoding {
            header: &header,
            plans,
        })
        .map_err(|_| node::Error::Codec("resolved demand is not encodable"))?;
        Ok(Self {
            header,
            bytes: bytes.into_boxed_slice(),
        })
    }

    fn plans(&self) -> Result<Vec<surface::adapter::Plan>, node::Error> {
        serde_json::from_slice::<Decoding<Vec<surface::adapter::Plan>>>(&self.bytes)
            .map(|decoded| decoded.plans)
            .map_err(|_| node::Error::Codec("resolved demand plans changed"))
    }
}

impl node::Value for Product {
    // Same transform family as the checked demand it resolves; its own schema
    // and authority domain keep the action keys distinct.
    const KIND: node::Kind = node::Kind::Checked;
    const SCHEMA: u32 = 0x5245_0001;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        if self.bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "resolved demand bytes",
                limit,
                actual: self.bytes.len(),
            });
        }
        Ok(self.bytes.clone())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "resolved demand bytes",
                limit,
                actual: bytes.len(),
            });
        }
        let decoded: Decoding<serde::de::IgnoredAny> = serde_json::from_slice(bytes)
            .map_err(|_| node::Error::Codec("resolved demand bytes changed"))?;
        Ok(Self {
            header: decoded.header,
            bytes: Box::from(bytes),
        })
    }
}

fn authority(
    policy: &crate::classify::Policy,
    profile: (crate::profile::Identity, crate::profile::Version),
    snapshot: surface::Snapshot,
) -> Result<Vec<u8>, node::Error> {
    let failure = |error: &dyn std::fmt::Display| node::Error::Transform(error.to_string().into());
    let generic = crate::classify::cached::authority(policy).map_err(|error| failure(&error))?;
    let profile = crate::profile::cached::authority(profile.0, profile.1, None)
        .map_err(|error| failure(&error))?;
    let mut authority = Vec::new();
    authority
        .try_reserve_exact(DOMAIN.len() + 4 + generic.len() + profile.len() + 32)
        .map_err(|_| node::Error::Allocation)?;
    authority.extend_from_slice(DOMAIN);
    authority.extend_from_slice(&surface::SURFACE_SCHEMA.to_le_bytes());
    authority.extend_from_slice(&generic);
    authority.extend_from_slice(&profile);
    authority.extend_from_slice(snapshot.as_bytes());
    Ok(authority)
}

struct Lookup<'a> {
    context: &'a crate::context::Context,
    evaluator: &'a node::Evaluator<'a>,
}

impl Lookup<'_> {
    fn at<K: node::Value>(
        &self,
        parents: &[node::staging::Input<'_>],
        authority: &[u8],
        limit: usize,
    ) -> Result<Outcome<K>, node::Error> {
        Recipe::from_inputs(self.context, parents, authority, limit)?.lookup_owned(self.evaluator)
    }
}

/// One resolved warm launch: the same values the full path hands its consumer.
pub struct Warm<B> {
    /// Sealed demand with the trusted runner inputs selected.
    pub demand: crate::Demand<crate::Sealed, B>,
    /// Framework profile contract under its current node.
    pub profile: crate::profile::Contract,
    /// Canonical compiler facade.
    pub facade: surface::facade::Cached,
    /// Retained checked output for the profile continuation.
    pub checked: checked::Retained,
    /// Native-input proof published from the revalidated owner.
    pub native: crate::input::proof::Proof,
    /// Raw API identity recorded with the resolved product.
    pub api: crate::api::Fingerprint,
}

/// Reuse the sealed resolved demand when every input is unchanged. A miss
/// returns the continuation untouched for the full path.
#[cfg(feature = "rustc-driver")]
pub fn resolve_api_semantic<B, E, T>(
    api: crate::api::Continuation,
    adapter: &crate::profile::trusted::Adapter<T>,
    policy: &crate::classify::Policy,
    snapshot: surface::Snapshot,
    limits: &crate::cargo::Limits,
    brand: impl FnOnce(&surface::facade::Cached) -> Result<B, E>,
) -> Result<Result<Warm<B>, crate::api::Continuation>, checked::Evaluation<E>>
where
    T: crate::profile::trusted::Operations,
{
    let cache =
        |error: node::Error| checked::Evaluation::Cache(checked::Error::Node(Box::new(error)));
    let context = api.publication.context.clone();
    if !adapter.uses_semantic_api()
        || context.limits().node_output_bytes() != limits.node_output_bytes()
    {
        return Ok(Err(api));
    }
    let store = context.open_store().map_err(cache)?;
    // An API staged under a new resolution with the content its consumers
    // already resolved is promoted here, so its unchanged dependents
    // are current hits and the graph is never rebuilt.
    let crate::api::Continuation {
        product,
        graph,
        publication,
    } = api;
    let product = match product {
        Owned::Staged(staged) => Owned::Current(staged.validate_current(&store).map_err(cache)?),
        current => current,
    };
    let api = crate::api::Continuation {
        product,
        graph,
        publication,
    };
    let Owned::Current(api_node) = &api.product else {
        return Ok(Err(api));
    };
    let evaluator = node::Evaluator::new(&store, &crate::metrics::Current);
    let input = node::Parent::validated(api_node);
    let input = [node::staging::Input::current(&input)];
    let lookup = Lookup {
        context: &context,
        evaluator: &evaluator,
    };
    let profile_identity = (adapter.identity(), adapter.version());
    let resolved = authority(policy, profile_identity, snapshot)
        .and_then(|authority| lookup.at::<Product>(&input, &authority, limits.node_output_bytes()))
        .map_err(cache)?;
    let Outcome::Hit(Owned::Current(resolved)) = resolved else {
        return Ok(Err(api));
    };
    let header = &resolved.value().header;
    let parts = (|| {
        let failure =
            |error: &dyn std::fmt::Display| node::Error::Transform(error.to_string().into());
        let profile =
            crate::profile::cached::authority(profile_identity.0, profile_identity.1, None)
                .map_err(|error| failure(&error))?;
        let Outcome::Hit(Owned::Current(profile)) =
            lookup.at::<Profile>(&input, &profile, limits.node_output_bytes())?
        else {
            return Ok(None);
        };
        let generic =
            crate::classify::cached::authority(policy).map_err(|error| failure(&error))?;
        let Outcome::Hit(Owned::Current(generic)) =
            lookup.at::<Generic>(&input, &generic, node::MAX_CONTENT_BYTES)?
        else {
            return Ok(None);
        };
        let facade = surface::facade::cached::authority(&header.contract)?;
        let parent = node::Parent::validated(&generic);
        let Outcome::Hit(Owned::Current(facade)) = lookup.at::<Facade>(
            &[node::staging::Input::current(&parent)],
            &facade,
            limits.node_output_bytes(),
        )?
        else {
            return Ok(None);
        };
        let api_fingerprint = crate::api::Fingerprint::from_bytes(header.api);
        let profile_parts = profile
            .value()
            .owned_resolved(
                profile_identity.0,
                profile_identity.1,
                (
                    api_fingerprint,
                    crate::api::Fingerprint::from_bytes(header.semantic),
                ),
                limits,
            )
            .map_err(|error| failure(&error))?;
        let authority = checked::authority(
            crate::contract::fingerprint::Fingerprint(header.contract),
            api_fingerprint,
            profile_parts.identity(),
            profile_parts.version(),
            profile_parts.rules_digest(),
            profile_parts.graph_fingerprint(),
            snapshot,
            None,
        )
        .map_err(|error| failure(&error))?;
        let parents = [
            node::Parent::validated(&facade),
            node::Parent::validated(&profile),
        ];
        let Outcome::Hit(Owned::Current(checked)) = lookup.at::<checked::Product>(
            &[
                node::staging::Input::current(&parents[0]),
                node::staging::Input::current(&parents[1]),
            ],
            &authority,
            limits.node_output_bytes(),
        )?
        else {
            return Ok(None);
        };
        if checked.value().demand_digest() != header.demand {
            return Ok(None);
        }
        Ok(Some((profile, generic, facade, checked, profile_parts)))
    })()
    .map_err(cache)?;
    let Some((profile, generic, facade, checked, profile_parts)) = parts else {
        return Ok(Err(api));
    };
    // The same lineage and outputs as the full path's all-hit branch.
    let retained = (|| {
        api.publication
            .retention
            .extend_current(&profile)?
            .extend_current(&generic)?
            .extend_current(&facade)?
            .extend_current(&checked)
    })()
    .map_err(cache)?;
    let profile = profile_parts.into_current(node::Chain::validated(context.clone(), &profile));
    let generic = node::Chain::validated(context.clone(), &generic);
    let facade = surface::facade::Cached::from_validated(context.clone(), facade);
    let plans = resolved.value().plans().map_err(cache)?;
    let contract = crate::contract::fingerprint::Fingerprint(header.contract);
    let allocation = crate::contract::Allocation::new(
        contract,
        crate::contract::Usage {
            items: usize::try_from(header.items).unwrap_or(usize::MAX),
            bytes: usize::try_from(header.bytes).unwrap_or(usize::MAX),
        },
        limits,
    )
    .map_err(|error| checked::Evaluation::Cache(checked::Error::Surface(error)))?;
    let demand = checked
        .value()
        .replay_resolved(
            brand(&facade).map_err(checked::Evaluation::Transform)?,
            (contract, &generic),
            allocation,
            snapshot,
            limits,
            plans,
        )
        .map_err(checked::Evaluation::Cache)?
        .with_trusted_runner_parts(true, &facade, contract, &generic)
        .map_err(|error| checked::Evaluation::Cache(checked::Error::Surface(error)))?;
    let checked = <checked::Retained as checked::retained::Output>::complete(
        context,
        Owned::Current(checked),
        retained,
        &store,
    )
    .map_err(checked::Evaluation::Cache)?;
    let api_fingerprint = crate::api::Fingerprint::from_bytes(header.api);
    let native = api.into_native_proof().map_err(cache)?;
    Ok(Ok(Warm {
        demand,
        profile,
        facade,
        checked,
        native,
        api: api_fingerprint,
    }))
}

/// Seal the resolved demand after the full path, under the API node of the
/// contract that resolved it. An existing product is kept as is.
pub fn record<B, T>(
    contract: &crate::Contract<crate::Classified>,
    demand: &crate::Demand<crate::Sealed, B>,
    adapter: &crate::profile::trusted::Adapter<T>,
    policy: &crate::classify::Policy,
) -> Result<(), checked::Error>
where
    T: crate::profile::trusted::Operations,
{
    let Some(api) = contract.raw_graph().node_chain() else {
        return Ok(());
    };
    if !adapter.uses_semantic_api() {
        return Ok(());
    }
    let node = |error: node::Error| checked::Error::Node(Box::new(error));
    let authority = authority(
        policy,
        (adapter.identity(), adapter.version()),
        demand.snapshot(),
    )
    .map_err(node)?;
    api.direct_with_parents::<Product>(&authority, &[], || {
        use sha2::Digest as _;
        let usage = contract.usage();
        let header = Header {
            contract: *contract.fingerprint().as_bytes(),
            api: *contract.raw_fingerprint().as_bytes(),
            semantic: *contract.raw_graph().semantic_fingerprint().as_bytes(),
            items: u64::try_from(usage.items).unwrap_or(u64::MAX),
            bytes: u64::try_from(usage.bytes).unwrap_or(u64::MAX),
            demand: sha2::Sha256::digest(demand.canonical_bytes()).into(),
        };
        Product::capture(header, demand.adapter_plans())
    })
    .map_err(node)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    use super::super::tests::{demand, graph, snapshot, Root};
    use super::{Header, Product};
    use crate::node::Value as _;

    #[test]
    fn resolved_plans_replay_like_the_contract() {
        let root = Root::new();
        let limits = crate::cargo::Limits::standard();
        let contract = graph(&root.0, &Arc::new(AtomicUsize::new(0)), &limits)
            .classify(&crate::classify::Policy::standard(), &limits)
            .expect("fixture contract");
        let source = snapshot(&limits);
        let sealed = demand(&contract, source, &limits);
        let (bytes, retained) = (sealed.canonical_bytes(), sealed.cached_retained_usage());
        let cached = crate::Demand::replay_cached(bytes, retained, (), &contract, source, &limits)
            .expect("contract replay");
        assert!(!cached.adapter_plans().is_empty());
        let header = Header {
            contract: [1; 32],
            api: [2; 32],
            semantic: [3; 32],
            items: 4,
            bytes: 5,
            demand: [6; 32],
        };
        let product = Product::capture(header, cached.adapter_plans()).expect("capture");
        let stored = product.encode(usize::MAX).expect("encode");
        let product = Product::decode(&stored, usize::MAX).expect("decode");
        assert_eq!(product.header.demand, [6; 32]);
        let resolved = crate::Demand::replay_resolved(
            bytes,
            retained,
            (),
            (
                contract.fingerprint(),
                contract.node_chain().expect("generic node"),
            ),
            contract.allocation(&limits).expect("allocation"),
            source,
            &limits,
            product.plans().expect("plans"),
        )
        .expect("resolved replay");
        let plans = |demand: &crate::Demand<crate::Sealed, ()>| {
            serde_json::to_vec(demand.adapter_plans()).expect("plans encode")
        };
        assert_eq!(plans(&resolved), plans(&cached));
        assert_eq!(resolved.canonical_bytes(), cached.canonical_bytes());
        assert_eq!(resolved.fingerprint(), cached.fingerprint());
        assert_eq!(
            resolved.cached_retained_usage(),
            cached.cached_retained_usage()
        );
        assert!(Product::decode(&stored[1..], usize::MAX).is_err());
    }
}
