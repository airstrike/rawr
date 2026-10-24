//! Capability-free stored API graph replay.

use std::path::PathBuf;

use crate::cargo::resolution::{BuildProduct, Locked, UnitGraph};
use crate::node::{self, Parent};
use crate::{context::Context, metrics};

use super::{
    header, Captured, Graph, StoredGraph, Usage, CANONICALIZER_VERSION, HEADER_BYTES, MAGIC,
};

/// One canonical captured API graph without a current input-proof capability.
///
/// The store keeps the manifest of its content-defined parts; the graph
/// bytes are read back from the parts only when a consumer replays them.
pub(crate) struct Product {
    form: Form,
}

enum Form {
    /// Captured by this run, with the directory its parts are kept in.
    Captured { stored: StoredGraph, parts: PathBuf },
    /// Read from the store: the manifest of the parts.
    Manifest(Box<[u8]>),
    /// The parts a manifest names, read back for replay.
    Resolved {
        stored: StoredGraph,
        manifest: Box<[u8]>,
    },
}

#[allow(
    clippy::too_many_arguments,
    reason = "the API node binds its complete three-parent Cargo closure and request authority"
)]
pub(crate) fn evaluate(
    context: &Context,
    request: &[u8; 32],
    policy: &[u8; 32],
    build: &node::Validated<BuildProduct>,
    post_unit: &node::Validated<UnitGraph>,
    post_metadata: &node::Validated<Locked>,
    sink: &dyn metrics::Sink,
    materialize: impl FnOnce(Option<&Product>) -> crate::Result<Graph<Captured>>,
) -> Result<(node::Validated<Product>, Graph<Captured>), node::Error> {
    let parents = [
        Parent::validated(build),
        Parent::validated(post_unit),
        Parent::validated(post_metadata),
    ];
    evaluate_with_parents(context, request, policy, &parents, sink, materialize)
}

/// Look up the API before materializing extraction, under the complete current
/// resolution owner rather than separately restored legacy Cargo products.
pub(crate) fn evaluate_owner(
    context: &Context,
    request: &[u8; 32],
    policy: &[u8; 32],
    owner: &node::Validated<crate::cargo::resolution::owner::Product>,
    retention: &node::read_set::Retention,
    sink: &dyn metrics::Sink,
    materialize: impl FnOnce(Option<&Product>) -> crate::Result<Graph<Captured>>,
) -> Result<(node::Validated<Product>, Graph<Captured>), node::Error> {
    let recipe = recipe(context, request, policy, &[Parent::validated(owner)])?;
    let mut materialize = Some(materialize);
    let mut current = None;
    let product = node::Evaluator::new(&context.open_store()?, sink).staged_direct(
        &recipe,
        retention,
        || {
            capture(
                context,
                &mut materialize,
                &mut current,
                context.limits().node_canonical_bytes(),
            )
        },
    )?;
    let graph = current_graph(context, &mut materialize, current, product.value())?;
    Ok((product, graph))
}

pub(crate) fn retain_owner<O>(
    context: &Context,
    request: &[u8; 32],
    policy: &[u8; 32],
    owner: node::Validated<crate::cargo::resolution::owner::Product>,
    retention: &node::read_set::Retention,
    memo: Option<[u8; 32]>,
    materialize: impl FnOnce(Option<&Product>) -> crate::Result<Graph<Captured>> + 'static,
    finish: impl FnOnce(
        node::Validated<crate::cargo::resolution::owner::Product>,
        node::staging::Owned<Product>,
        node::read_set::Retention,
        super::retained::Lazy,
    ) -> Result<O, node::Error>,
) -> Result<O, node::Error> {
    use node::staging::{Outcome, Owned, Recipe};
    let direct = recipe(context, request, policy, &[Parent::validated(&owner)])?;
    let store = context.open_store()?;
    let evaluator = node::Evaluator::new(&store, &metrics::Current);
    let recipe = Recipe::from_current_direct(direct);
    let mut materialize = Some(materialize);
    let mut current = None;
    match recipe.lookup_owned(&evaluator)? {
        Outcome::Hit(Owned::Current(product)) => {
            // A current hit defers its graph rebuild to the consumer that needs it.
            let retained = retention.extend_current(&product)?;
            let materialize = materialize
                .take()
                .ok_or(node::Error::Codec("API capture was already consumed"))?;
            let graph = super::retained::Lazy::Pending(Box::new(materialize));
            finish(owner, Owned::Current(product), retained, graph)
        }
        outcome => store.with_staged_batch(retention, &metrics::Current, |batch| {
            // Inputs whose content is remembered stage that content;
            // its graph is rebuilt only for a consumer that needs it.
            let remembered = match &outcome {
                Outcome::Miss(_) => memo.and_then(|key| super::memo::load(context, &key)),
                Outcome::Hit(_) => None,
            };
            let product = match (outcome, remembered) {
                (Outcome::Hit(value), _) => batch.carry_owned(value)?,
                (Outcome::Miss(miss), Some(remembered)) => {
                    let staged = recipe.resume(batch, miss, || Ok(remembered))?;
                    let materialize = materialize
                        .take()
                        .ok_or(node::Error::Codec("API capture was already consumed"))?;
                    let graph = super::retained::Lazy::Pending(Box::new(materialize));
                    return finish(
                        owner,
                        Owned::Staged(staged),
                        batch.retention().try_clone()?,
                        graph,
                    );
                }
                (Outcome::Miss(miss), None) => {
                    let staged = recipe.resume(batch, miss, || {
                        capture(
                            context,
                            &mut materialize,
                            &mut current,
                            context.limits().node_canonical_bytes(),
                        )
                    })?;
                    if let Some(key) = memo {
                        super::memo::remember(context, &key, staged.content().digest());
                    }
                    Owned::Staged(staged)
                }
            };
            let graph = current_graph(context, &mut materialize, current, product.value())?;
            let graph = super::retained::Lazy::Ready(graph);
            finish(owner, product, batch.retention().try_clone()?, graph)
        }),
    }
}

fn evaluate_with_parents(
    context: &Context,
    request: &[u8; 32],
    policy: &[u8; 32],
    parents: &[Parent],
    sink: &dyn metrics::Sink,
    materialize: impl FnOnce(Option<&Product>) -> crate::Result<Graph<Captured>>,
) -> Result<(node::Validated<Product>, Graph<Captured>), node::Error> {
    let recipe = recipe(context, request, policy, parents)?;
    let mut materialize = Some(materialize);
    let mut current = None;
    let product = node::Evaluator::new(&context.open_store()?, sink).direct(&recipe, || {
        capture(
            context,
            &mut materialize,
            &mut current,
            context.limits().node_canonical_bytes(),
        )
    })?;
    let graph = current_graph(context, &mut materialize, current, product.value())?;
    Ok((product, graph))
}

// Consume the already authenticated capture once, on either path. A miss keeps
// its exact graph while moving only the semantic storage into the node value.
fn capture<F>(
    context: &Context,
    materialize: &mut Option<F>,
    current: &mut Option<Graph<Captured>>,
    limit: usize,
) -> Result<Product, node::Error>
where
    F: FnOnce(Option<&Product>) -> crate::Result<Graph<Captured>>,
{
    let materialize = materialize
        .take()
        .ok_or(node::Error::Codec("API capture was already consumed"))?;
    let mut graph = materialize(None)?;
    let stored = graph.take_stored()?;
    super::limited_graph_usage(
        "stored API graph bytes",
        limit,
        stored.canonical.len().saturating_add(stored.selected.len()),
    )?;
    *current = Some(graph);
    Ok(Product {
        form: Form::Captured {
            stored,
            parts: super::parts::directory(context),
        },
    })
}

fn current_graph<F>(
    context: &Context,
    materialize: &mut Option<F>,
    current: Option<Graph<Captured>>,
    product: &Product,
) -> Result<Graph<Captured>, node::Error>
where
    F: FnOnce(Option<&Product>) -> crate::Result<Graph<Captured>>,
{
    match current {
        Some(graph) => Ok(graph),
        None => {
            let resolved = product.resolve(context)?;
            Ok(materialize.take().ok_or(node::Error::Codec(
                "API capture was already consumed",
            ))?(Some(resolved.as_ref().unwrap_or(product)))?)
        }
    }
}

fn recipe(
    context: &Context,
    request: &[u8; 32],
    policy: &[u8; 32],
    parents: &[Parent],
) -> Result<node::Direct<Product>, node::Error> {
    let mut authority = Vec::new();
    authority
        .try_reserve_exact(32 + request.len() + policy.len() + 8)
        .map_err(|_| node::Error::Allocation)?;
    authority.extend_from_slice(b"rawr.api.captured/v5");
    authority.extend_from_slice(&CANONICALIZER_VERSION.to_le_bytes());
    authority.extend_from_slice(&crate::extract::protocol::SCHEMA.to_le_bytes());
    authority.extend_from_slice(request);
    authority.extend_from_slice(policy);
    context.direct_with_parents::<Product>(
        &authority,
        context.limits().node_canonical_bytes(),
        parents,
    )
}

impl Product {
    #[cfg(test)]
    pub(crate) fn capture(
        graph: Graph<Captured>,
        limit: usize,
        parts: PathBuf,
    ) -> crate::Result<Self> {
        Ok(Self {
            form: Form::Captured {
                stored: graph.into_stored(limit)?,
                parts,
            },
        })
    }

    /// A product read from the store, with its graph bytes read back from its
    /// parts; `None` when this product already holds them.
    pub(crate) fn resolve(&self, context: &Context) -> Result<Option<Self>, node::Error> {
        self.resolve_in(
            &super::parts::directory(context),
            context.limits().node_canonical_bytes(),
        )
    }

    fn resolve_in(
        &self,
        parts: &std::path::Path,
        limit: usize,
    ) -> Result<Option<Self>, node::Error> {
        let Form::Manifest(manifest) = &self.form else {
            return Ok(None);
        };
        let bytes = super::parts::read(parts, manifest, limit)?;
        Ok(Some(Self {
            form: Form::Resolved {
                stored: graph_bytes(bytes, limit)?,
                manifest: manifest.clone(),
            },
        }))
    }

    #[cfg(test)]
    fn stored_mut(&mut self) -> &mut StoredGraph {
        match &mut self.form {
            Form::Captured { stored, .. } | Form::Resolved { stored, .. } => stored,
            Form::Manifest(_) => panic!("the fixture product holds its graph"),
        }
    }

    fn stored(&self) -> Result<&StoredGraph, node::Error> {
        match &self.form {
            Form::Captured { stored, .. } | Form::Resolved { stored, .. } => Ok(stored),
            Form::Manifest(_) => Err(node::Error::Codec("API parts were not read back")),
        }
    }

    pub(crate) fn replay(
        &self,
        context: &super::Context,
        invocations: Vec<super::Invocation>,
        limits: &crate::cargo::Limits,
        allocation: &mut crate::extract::Allocation,
    ) -> crate::Result<Graph<Captured>> {
        let stored = self.stored().map_err(|source| crate::Error::Node {
            operation: "replay stored API",
            source: Box::new(source),
        })?;
        Graph::from_stored(stored, context, invocations, limits, allocation)
    }
}

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::Api;
    const SCHEMA: u32 = 4;

    /// The manifest of the graph's parts; a captured graph keeps any part not
    /// already kept.
    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        match &self.form {
            Form::Captured { stored, parts } => {
                super::parts::write(parts, &graph_payload(stored, limit)?)
            }
            Form::Manifest(manifest) | Form::Resolved { manifest, .. } => Ok(manifest.clone()),
        }
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        Self::decode_owned(bytes.into(), limit)
    }

    /// The manifest is checked here; its parts are read on replay.
    fn decode_owned(bytes: Box<[u8]>, limit: usize) -> Result<Self, node::Error> {
        super::parts::check(&bytes, limit)?;
        Ok(Self {
            form: Form::Manifest(bytes),
        })
    }
}

/// The graph's bytes: the header, the semantic canonical, the selected ledger.
fn graph_payload(
    stored: &StoredGraph,
    limit: usize,
) -> Result<std::borrow::Cow<'_, [u8]>, node::Error> {
    let length = HEADER_BYTES
        .checked_add(stored.canonical.len())
        .and_then(|length| length.checked_add(stored.selected.len()))
        .unwrap_or(usize::MAX);
    if length > limit {
        return Err(node::Error::Limit {
            resource: "cached API graph bytes",
            limit,
            actual: length,
        });
    }
    let StoredGraph {
        canonical,
        selected,
        ..
    } = stored;
    // A captured graph is laid out contiguously by `stored`: its parts are cut
    // from that buffer without a copy.
    if std::sync::Arc::ptr_eq(&canonical.buffer, &selected.buffer)
        && canonical.range.start == HEADER_BYTES
        && canonical.range.end == selected.range.start
        && selected.range.end == selected.buffer.len()
    {
        return Ok(std::borrow::Cow::Borrowed(&selected.buffer[..]));
    }
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(length)
        .map_err(|_| node::Error::Allocation)?;
    encoded.extend_from_slice(&header(
        stored.selected_usage,
        canonical.len(),
        selected.len(),
    ));
    encoded.extend_from_slice(canonical);
    encoded.extend_from_slice(selected);
    Ok(std::borrow::Cow::Owned(encoded))
}

/// The canonical and selected bytes stay views of the read buffer.
fn graph_bytes(bytes: Box<[u8]>, limit: usize) -> Result<StoredGraph, node::Error> {
    if bytes.len() > limit {
        return Err(node::Error::Limit {
            resource: "cached API graph bytes",
            limit,
            actual: bytes.len(),
        });
    }
    if bytes.get(..MAGIC.len()) != Some(MAGIC) {
        return Err(node::Error::Codec("cached API graph magic changed"));
    }
    let items = scalar(&bytes, 8, "API selected ownership item count")?;
    let selected_bytes = scalar(&bytes, 16, "API selected ownership byte count")?;
    let canonical_length = scalar(&bytes, 24, "API canonical length")?;
    let canonical_length = usize::try_from(canonical_length)
        .map_err(|_| node::Error::Codec("API canonical length is invalid"))?;
    let selected_length = scalar(&bytes, 32, "API selected length")?;
    let selected_length = usize::try_from(selected_length)
        .map_err(|_| node::Error::Codec("API selected length is invalid"))?;
    let canonical_end = HEADER_BYTES
        .checked_add(canonical_length)
        .ok_or(node::Error::Codec("API canonical length is invalid"))?;
    let payload_end = canonical_end
        .checked_add(selected_length)
        .ok_or(node::Error::Codec("API selected length is invalid"))?;
    if payload_end != bytes.len() {
        return Err(node::Error::Codec("cached API graph payload is truncated"));
    }
    if canonical_length == 0 || selected_length == 0 {
        return Err(node::Error::Codec("cached API graph payload is empty"));
    }
    let [canonical, selected] = crate::api::Shared::split(
        bytes,
        [HEADER_BYTES..canonical_end, canonical_end..payload_end],
    );
    Ok(StoredGraph {
        canonical,
        selected,
        selected_usage: Usage {
            items: usize::try_from(items)
                .map_err(|_| node::Error::Codec("API selected ownership item count is invalid"))?,
            bytes: usize::try_from(selected_bytes)
                .map_err(|_| node::Error::Codec("API selected ownership byte count is invalid"))?,
        },
    })
}

fn scalar(bytes: &[u8], start: usize, field: &'static str) -> Result<u64, node::Error> {
    bytes
        .get(start..start.saturating_add(8))
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(node::Error::Codec(field))
}

#[cfg(test)]
mod tests {
    use crate::node::Value as _;

    use super::{Form, Product};
    use crate::api::{Context, Graph};
    use crate::cargo::Limits;
    use crate::extract::Allocation;

    fn graph(context: &Context, allocation: &mut Allocation) -> Graph<crate::api::Captured> {
        Graph::seal(
            context,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Limits::standard().node_canonical_bytes(),
            allocation,
        )
        .expect("the fixture graph seals")
    }

    fn parts() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("rawr-api-cached-{}", std::process::id()))
    }

    fn capture(graph: Graph<crate::api::Captured>, limit: usize) -> Product {
        Product::capture(graph, limit, parts()).expect("the fixture product captures")
    }

    fn product() -> Product {
        let limits = Limits::standard();
        let graph = graph(&Context::fixture(), &mut Allocation::new(&limits));
        capture(graph, limits.node_canonical_bytes())
    }

    /// Decode a stored manifest and read its parts back.
    fn read(bytes: &[u8], limit: usize) -> Product {
        Product::decode(bytes, limit)
            .expect("the manifest decodes")
            .resolve_in(&parts(), limit)
            .expect("the parts read back")
            .expect("a decoded product reads its parts")
    }

    fn replay(product: &Product, limits: &Limits) -> crate::Result<Graph<crate::api::Captured>> {
        product.replay(
            &Context::fixture(),
            Vec::new(),
            limits,
            &mut Allocation::new(limits),
        )
    }

    #[test]
    fn captured_storage_is_published_as_the_manifest_of_its_parts() {
        let limit = Limits::standard().node_canonical_bytes();
        let product = product();
        let expected = product.encode(limit).expect("the product encodes");
        assert!(super::super::parts::check(&expected, limit).is_ok());
        let encoded = product.into_encoded(limit).expect("the product hands over");
        assert_eq!(encoded, expected);
        let decoded = Product::decode(&encoded, limit).expect("the manifest decodes");
        assert!(matches!(decoded.form, Form::Manifest(_)));
        assert_eq!(
            decoded.encode(limit).expect("a manifest re-encodes"),
            expected
        );
    }

    #[test]
    fn stored_graph_roundtrips_and_replays_the_same_semantic_graph() {
        let limits = Limits::standard();
        let product = product();
        let expected = replay(&product, &limits).expect("the source product replays");
        let bytes = product
            .encode(limits.node_canonical_bytes())
            .expect("the product encodes");
        let decoded = read(&bytes, limits.node_canonical_bytes());
        let actual = replay(&decoded, &limits).expect("the stored graph replays");

        assert_eq!(actual.fingerprint(), expected.fingerprint());
        assert_eq!(actual.canonical_bytes(), expected.canonical_bytes());
        assert_eq!(actual.transaction(), expected.transaction());
        assert_eq!(actual.allocation(), expected.allocation());
    }

    /// A read-back API is viewed in place, not copied, and re-encodes
    /// to the same manifest.
    #[test]
    fn read_parts_are_viewed_without_copying() {
        let limit = Limits::standard().node_canonical_bytes();
        let bytes = product().encode(limit).expect("the product encodes");
        let decoded = read(&bytes, limit);
        let stored = decoded.stored().expect("the parts were read");
        assert!(std::sync::Arc::ptr_eq(
            &stored.canonical.buffer,
            &stored.selected.buffer
        ));
        assert_eq!(
            decoded.encode(limit).expect("the product re-encodes"),
            bytes
        );
    }

    #[test]
    fn stored_graph_rejects_framing_and_canonical_tamper() {
        let limits = Limits::standard();
        let limit = limits.node_canonical_bytes();
        let product = product();
        let bytes = super::graph_payload(product.stored().unwrap(), limit)
            .expect("the graph lays out")
            .into_owned();

        let mut magic = bytes.clone();
        magic[0] ^= 1;
        assert!(super::graph_bytes(magic.into(), limit).is_err());

        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(super::graph_bytes(trailing.into(), limit).is_err());

        let mut canonical = bytes;
        let last = canonical.last_mut().expect("canonical bytes are present");
        *last = b' ';
        let decoded = Product {
            form: Form::Captured {
                stored: super::graph_bytes(canonical.into(), limit)
                    .expect("the frame remains structurally complete"),
                parts: parts(),
            },
        };
        assert!(replay(&decoded, &limits).is_err());

        let manifest = product.encode(limit).expect("the product encodes");
        let mut tampered = manifest.to_vec();
        tampered[0] ^= 1;
        assert!(Product::decode(&tampered, limit).is_err());
        let mut trailing = manifest.to_vec();
        trailing.push(0);
        assert!(Product::decode(&trailing, limit).is_err());
    }

    #[test]
    fn raw_pre_policy_canonical_roundtrips_with_selected_replay_state() {
        let limits = Limits::standard();
        let universe = [7; 32];
        let context = Context::fixture_with_universe(Some(universe));
        let mut allocation = Allocation::new(&limits);
        let mut source = graph(&context, &mut allocation);
        let stored = source.take_stored().expect("semantic source is present");
        // Retain a pre-policy rejection absent from the selected typed graph.
        let add_rejected = |bytes: &[u8]| {
            String::from_utf8(bytes.to_vec())
                .expect("canonical JSON is UTF-8")
                .replacen(
                    "\"exports\":",
                    "\"rejected\":[\"pre-policy rejection\"],\"exports\":",
                    1,
                )
                .into_bytes()
        };
        let raw = add_rejected(source.canonical_bytes());
        let semantic = add_rejected(&stored.canonical);
        allocation.items(2).expect("raw buffers fit");
        allocation
            .bytes(raw.len() + semantic.len())
            .expect("raw bytes fit");
        let graph = Graph::seal_with_raw(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Graph::raw_pair(raw.clone(), semantic, Some(universe)),
            &mut allocation,
        )
        .expect("the pre-policy raw graph seals");
        let expected_fingerprint = graph.fingerprint();
        let product = capture(graph, limits.node_canonical_bytes());
        let bytes = product
            .encode(limits.node_canonical_bytes())
            .expect("the raw graph product encodes");
        let decoded = read(&bytes, limits.node_canonical_bytes());
        let replayed = decoded
            .replay(&context, Vec::new(), &limits, &mut Allocation::new(&limits))
            .expect("selected state replays");

        assert_eq!(replayed.canonical_bytes(), raw);
        assert_eq!(replayed.fingerprint(), expected_fingerprint);
        assert_eq!(replayed.resolution_universe(), Some(&universe));
        assert_eq!(replayed.invocation_count(), 0);
        assert_eq!(replayed.definitions().count(), 0);
    }
    #[test]
    fn physical_context_rebinds_exact_bytes_with_identical_stored_content() {
        let limits = Limits::standard();
        let first_context = Context::fixture_with_universe(Some([7; 32]));
        let mut current_context = Context::fixture_with_universe(Some([7; 32]));
        current_context.metadata = [21; 32];
        current_context.build_unit = [22; 32];
        current_context.resolution_proof = [23; 32];
        let first = graph(&first_context, &mut Allocation::new(&limits));
        let mut current = graph(&current_context, &mut Allocation::new(&limits));
        assert_ne!(first.fingerprint(), current.fingerprint());
        let expected_exact = current.canonical_bytes().to_vec();
        let expected_fingerprint = current.fingerprint();
        let first = capture(first, limits.node_canonical_bytes());
        let current = Product {
            form: Form::Captured {
                stored: current.take_stored().expect("current captures"),
                parts: parts(),
            },
        };
        assert_eq!(
            first
                .encode(limits.node_canonical_bytes())
                .expect("first encodes"),
            current
                .encode(limits.node_canonical_bytes())
                .expect("current encodes")
        );
        let mut allocation = Allocation::new(&limits);
        allocation.items(17).expect("current history fits");
        allocation.bytes(19).expect("current history fits");
        let restored = first
            .replay(&current_context, Vec::new(), &limits, &mut allocation)
            .expect("semantic graph rebinds to the current exact context");
        assert_eq!(restored.canonical_bytes(), expected_exact);
        assert_eq!(restored.fingerprint(), expected_fingerprint);
        assert_eq!(restored.driver_transaction_usage(), allocation.usage());
        let plain = first
            .replay(
                &current_context,
                Vec::new(),
                &limits,
                &mut Allocation::new(&limits),
            )
            .expect("same content with a different transaction history replays");
        assert_eq!(restored.transaction().items, plain.transaction().items + 17);
        assert_eq!(restored.transaction().bytes, plain.transaction().bytes + 19);
        current_context.final_lock = Some([8; 32]);
        assert!(first
            .replay(
                &current_context,
                Vec::new(),
                &limits,
                &mut Allocation::new(&limits)
            )
            .is_err());
    }

    #[test]
    fn replay_preflight_ignores_understated_ownership_and_honors_prior_spending() {
        let limits = Limits::standard();
        let mut product = capture(
            crate::test_fixture::captured_support_outcomes(),
            limits.node_canonical_bytes(),
        );
        let decode = crate::extract::protocol::selected_preflight(
            &product.stored().unwrap().selected,
            &limits,
        )
        .unwrap();
        assert!(decode.items >= product.stored().unwrap().selected_usage.items);
        assert!(decode.bytes >= product.stored().unwrap().selected_usage.bytes);
        assert!(decode.admission_bytes >= decode.bytes);
        eprintln!("API_DECODE_FIXTURE support retained_items={} retained_bytes={} decode_items={} decode_bytes={} admission_bytes={}",
            product.stored().unwrap().selected_usage.items, product.stored().unwrap().selected_usage.bytes,
            decode.items, decode.bytes, decode.admission_bytes);
        product.stored_mut().selected_usage = crate::api::Usage { items: 0, bytes: 0 };
        let mut short = Allocation::with_limits(usize::MAX, decode.admission_bytes + 18);
        short.bytes(19).unwrap();
        let error = product
            .replay(&Context::fixture(), Vec::new(), &limits, &mut short)
            .err()
            .unwrap();
        // The decoder admission one over the byte target is recorded
        // (honoring the prior 19 bytes, ignoring the zeroed ownership header)
        // and replay continues to the ownership audit, which refuses.
        assert!(matches!(
            error,
            crate::Error::Extraction {
                code: "stored-api-ownership",
                ..
            }
        ));
        let record = short.exceedances();
        let axis = record
            .axis("canonical allocation bytes")
            .expect("the admission exceedance is recorded");
        assert_eq!(axis.target, (decode.admission_bytes + 18) as u64);
        assert_eq!(axis.first_requested, (decode.admission_bytes + 19) as u64);
        assert!(axis.high_water >= axis.first_requested);
        assert!(!short.is_refused());
        let mut exact = Allocation::with_limits(decode.items + 17, decode.admission_bytes + 19);
        exact.items(17).unwrap();
        exact.bytes(19).unwrap();
        exact
            .admit_decode(decode.items, decode.admission_bytes)
            .unwrap();
        assert_eq!(
            exact.usage(),
            (17, 19),
            "prospective admission is not a second charge"
        );
        assert!(!exact.exceedances().is_exceeded());
        let mut admitted = Allocation::new(&limits);
        let error = product
            .replay(&Context::fixture(), Vec::new(), &limits, &mut admitted)
            .err()
            .unwrap();
        assert!(matches!(
            error,
            crate::Error::Extraction {
                code: "stored-api-ownership",
                ..
            }
        ));
        assert!(admitted.usage().0 >= decode.items && admitted.usage().1 >= decode.bytes);
        let Form::Captured { stored, .. } = product.form else {
            panic!("the fixture product is captured")
        };
        let mut malformed = Product {
            form: Form::Captured {
                stored,
                parts: parts(),
            },
        };
        // A syntactically valid but ill-typed ledger reaches serde only after
        // the independent debit, irrespective of its zero ownership header.
        malformed.stored_mut().selected = br#"{"universe":null,"definitions":[false],"exports":[],"implementations":[],"scalar_predicate_proofs":[],"support":[],"anonymous":[]}"#.to_vec().into();
        let decode = crate::extract::protocol::selected_preflight(
            &malformed.stored().unwrap().selected,
            &limits,
        )
        .unwrap();
        let mut allocation = Allocation::new(&limits);
        let error = malformed
            .replay(&Context::fixture(), Vec::new(), &limits, &mut allocation)
            .err()
            .unwrap();
        assert!(matches!(
            error,
            crate::Error::Extraction {
                code: "stored-selected-schema",
                ..
            }
        ));
        assert_eq!(allocation.usage(), (decode.items, decode.bytes));
    }

    #[test]
    fn selected_preflight_covers_nested_boxes_and_concrete_array_storage() {
        use crate::api::model;

        let limits = Limits::standard();
        let fixture = crate::test_fixture::captured_support_outcomes();
        let first = fixture.definitions().next().unwrap();
        let nested = model::Type::FunctionPointer(Box::new(model::Signature::new(
            model::Safety::Safe,
            false,
            false,
            "\"Rust\"".into(),
            false,
            Box::default(),
            model::Receiver::None,
            Box::from([model::Type::Slice(Box::new(model::Type::Primitive(
                "i64".into(),
            )))]),
            model::Type::Reference {
                lifetime: model::Lifetime::Erased,
                mutability: model::Mutability::Shared,
                ty: Box::new(model::Type::Tuple(Box::from([
                    model::Type::Primitive("bool".into()),
                    model::Type::Primitive("char".into()),
                ]))),
            },
        )));
        let definition = model::Definition::new(
            first.id().clone(),
            Box::from([Box::<str>::from("nested")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::TypeAlias {
                ty: nested,
                generics: model::Generics::new(Box::default(), Box::default()),
            },
            first.span().clone(),
            model::Origin::Source,
        );
        let graph = Graph::seal(
            &Context::fixture(),
            Vec::new(),
            vec![definition],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            limits.node_canonical_bytes(),
            &mut Allocation::new(&limits),
        )
        .unwrap();
        let product = capture(graph, limits.node_canonical_bytes());
        let decode = crate::extract::protocol::selected_preflight(
            &product.stored().unwrap().selected,
            &limits,
        )
        .unwrap();
        assert!(decode.items >= product.stored().unwrap().selected_usage.items);
        assert!(decode.bytes >= product.stored().unwrap().selected_usage.bytes);
        eprintln!("API_DECODE_FIXTURE nested retained_items={} retained_bytes={} decode_items={} decode_bytes={} admission_bytes={}",
            product.stored().unwrap().selected_usage.items, product.stored().unwrap().selected_usage.bytes,
            decode.items, decode.bytes, decode.admission_bytes);
        replay(&product, &limits).unwrap();
        let unknown =
            crate::extract::protocol::selected_preflight(br#"{"unclassified":[null]}"#, &limits)
                .unwrap();
        assert!(unknown.bytes >= crate::api::decode::Shape::widest());
        assert!(unknown.admission_bytes >= 4 * crate::api::decode::Shape::widest());
    }
}
