//! Content-addressed normalized runner-source generation.

use crate::node;

// v3: the generated source never reads the Rawr snapshot, so a Rawr-only edit
// reuses the overlay instead of regenerating identical source.
const DOMAIN: &[u8] = b"rawr.generated.overlay/v3";
const MAGIC: &[u8; 8] = b"RAWROVL1";
const HEADER_BYTES: usize = 112;

#[cfg(all(test, feature = "test-fixtures"))]
#[path = "cached/tests.rs"]
mod retained_tests;

pub(crate) fn materialize<B: crate::surface::Runner>(
    key: super::super::GenerationRequestKey<B>,
    limits: &crate::cargo::Limits,
) -> Result<crate::runner::Manifest<crate::runner::Generated, B>, super::request::Error> {
    let chain = key.profile.node_chain().cloned().ok_or_else(|| {
        super::request::Error::Node(Box::new(node::Error::State(
            "cached overlay lost its generated-profile parent",
        )))
    })?;
    let authority = authority(&key)?;
    let retained = key.request.runner.is_some() && chain.retention().is_some();
    let (product, overlay) = if retained {
        evaluate_retained(&chain, &authority, || Product::render(&key))
            .map_err(|error| super::request::Error::Node(Box::new(error)))?
    } else {
        let product = evaluate(&chain, &authority, || Product::render(&key))?;
        let overlay = if let Some(initial) = chain.retention() {
            let retention = initial
                .extend_current(&product)
                .map_err(|error| super::request::Error::Node(Box::new(error)))?;
            node::Chain::validated_retained(chain.context().clone(), &product, retention)
                .map_err(|error| super::request::Error::Node(Box::new(error)))?
        } else {
            chain.child(&product)
        };
        let overlay = crate::runner::overlay::Overlay::from_current(overlay)
            .map_err(|error| super::request::Error::Node(Box::new(error)))?;
        (node::staging::Owned::Current(product), overlay)
    };
    let mut manifest = product.value().assemble(key, chain, limits)?;
    manifest.overlay = Some(overlay);
    Ok(manifest)
}

/// Carry owned profile generation to the fresh harness-render consumer.
pub(crate) fn materialize_historical<B: crate::surface::Runner>(
    key: super::super::GenerationRequestKey<B>,
    limits: &crate::cargo::Limits,
) -> Result<crate::runner::Manifest<crate::runner::Generated, B>, super::request::Error> {
    use node::staging::{Outcome, Owned, Recipe};
    let super::super::profile::Profile::Historical(pair) = &key.profile else {
        return materialize(key, limits);
    };
    let context = pair.context.clone();
    let initial = pair.retention.try_clone().map_err(node_error)?;
    let recipe = Recipe::<Product>::from_inputs(
        &context,
        &[pair.generated.input()],
        &authority(&key)?,
        limits.node_output_bytes(),
    )
    .map_err(node_error)?;
    let store = context.open_store().map_err(node_error)?;
    let evaluator = node::Evaluator::new(&store, &crate::metrics::Current);
    let outcome = recipe.lookup_owned(&evaluator).map_err(node_error)?;
    let (product, overlay, key, chain) = store
        .with_staged_batch(&initial, &crate::metrics::Current, |batch| {
            let mut pending = Some(key);
            let mut consumed = None;
            let product = match outcome {
                Outcome::Hit(value) => batch.carry_owned(value)?,
                Outcome::Miss(miss) => Owned::Staged(recipe.resume(batch, miss, || {
                    let key = pending
                        .take()
                        .ok_or(node::Error::State("historical profile request disappeared"))?
                        .promote_profile(true, &store)?;
                    // Promotion is after ownership/winner waits, immediately before
                    // the harness read. It replaces the separate parent refresh.
                    let rendered = Product::render(&key);
                    consumed = Some(key);
                    rendered
                })?),
            };
            let key = match consumed {
                Some(key) => key,
                None => pending
                    .take()
                    .ok_or(node::Error::State("historical profile winner disappeared"))?
                    .promote_profile(false, &store)?,
            };
            let chain = key
                .profile
                .node_chain()
                .cloned()
                .ok_or(node::Error::State("promoted profile has no current chain"))?;
            let overlay = crate::runner::overlay::Overlay::retain(&chain, &product)?;
            Ok((product, overlay, key, chain))
        })
        .map_err(node_error)?;
    let mut manifest = product.value().assemble(key, chain, limits)?;
    manifest.overlay = Some(overlay);
    Ok(manifest)
}

fn node_error(error: node::Error) -> super::request::Error {
    super::request::Error::Node(Box::new(error))
}

fn authority<B>(
    key: &super::super::GenerationRequestKey<B>,
) -> Result<Box<[u8]>, super::request::Error> {
    let mut authority = Vec::new();
    authority
        .try_reserve_exact(DOMAIN.len().saturating_add(40))
        .map_err(|_| super::request::Error::Allocation)?;
    authority.extend_from_slice(DOMAIN);
    authority.extend_from_slice(&super::super::generator_version().to_le_bytes());
    authority.extend_from_slice(key.build.as_bytes());
    Ok(authority.into_boxed_slice())
}

fn evaluate(
    chain: &node::Chain,
    authority: &[u8],
    execute: impl FnOnce() -> Result<Product, node::Error>,
) -> Result<node::Validated<Product>, super::request::Error> {
    chain
        .direct::<Product>(authority, execute)
        .map_err(|error| super::request::Error::Node(Box::new(error)))
}

fn evaluate_retained(
    chain: &node::Chain,
    authority: &[u8],
    execute: impl FnOnce() -> Result<Product, node::Error>,
) -> Result<
    (
        node::staging::Owned<Product>,
        crate::runner::overlay::Overlay,
    ),
    node::Error,
> {
    let initial = chain.retention().ok_or(node::Error::State(
        "retained overlay requires exact parent ownership",
    ))?;
    let recipe =
        node::staging::Recipe::<Product>::from_chain(chain, authority, chain.output_limit())?;
    let store = chain.context().open_store()?;
    let evaluator = node::Evaluator::new(&store, &crate::metrics::Current);
    match recipe.lookup(&evaluator)? {
        node::Lookup::Hit(product) => {
            let product = node::staging::Owned::Current(product);
            let overlay = crate::runner::overlay::Overlay::retain(chain, &product)?;
            Ok((product, overlay))
        }
        node::Lookup::Miss(miss) => {
            store.with_staged_batch(initial, &crate::metrics::Current, |batch| {
                let product = recipe.resume(batch, miss, || {
                    // Run after action/project acquisition and winner recheck.
                    // Rendering still reads the harness; retention is not its
                    // current input authority.
                    chain.current_parent().read_set().revalidate(&store)?;
                    execute()
                })?;
                // The exact bytes may travel as historical evidence. The runner
                // evaluation union must complete current validation before use.
                let product = node::staging::Owned::Staged(product);
                let overlay = crate::runner::overlay::Overlay::retain(chain, &product)?;
                Ok((product, overlay))
            })
        }
    }
}

struct Product {
    aggregate: [u8; 32],
    entries: [u8; 32],
    lock: Box<[u8]>,
    manifest: Box<[u8]>,
    source: Box<[u8]>,
    source_map: Box<[u8]>,
}

impl Product {
    fn render<B: crate::surface::Runner>(
        key: &super::super::GenerationRequestKey<B>,
    ) -> Result<Self, node::Error> {
        let rendered = super::render(&key.request, &key.native, key.profile.value())
            .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
        let expected = super::expected(&key.request, &rendered)
            .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
        Ok(Self {
            aggregate: expected.aggregate,
            entries: expected.entries,
            lock: rendered.lock.into_boxed_slice(),
            manifest: rendered.manifest.into_boxed_slice(),
            source: rendered.source.into_boxed_slice(),
            source_map: rendered.source_map.into_boxed_slice(),
        })
    }

    fn assemble<B: crate::surface::Runner>(
        &self,
        key: super::super::GenerationRequestKey<B>,
        chain: node::Chain,
        limits: &crate::cargo::Limits,
    ) -> Result<crate::runner::Manifest<crate::runner::Generated, B>, super::request::Error> {
        let expected = super::expected_parts(
            &key.request,
            &self.lock,
            &self.manifest,
            &self.source,
            &self.source_map,
        )?;
        if expected.aggregate != self.aggregate || expected.entries != self.entries {
            return Err(super::request::Error::Drift {
                field: "cached normalized generated product",
            });
        }
        let rendered = super::Product {
            manifest: clone_bytes(&self.manifest)?,
            lock: clone_bytes(&self.lock)?,
            source: clone_bytes(&self.source)?,
            source_map: clone_bytes(&self.source_map)?,
            spans: Vec::new(),
        };
        super::assemble(key, rendered, expected, Some(chain), limits)
    }

    fn encoded_len(&self) -> usize {
        HEADER_BYTES
            .saturating_add(self.lock.len())
            .saturating_add(self.manifest.len())
            .saturating_add(self.source.len())
            .saturating_add(self.source_map.len())
    }
}

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::Overlay;
    const SCHEMA: u32 = 1;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        let length = self.encoded_len();
        if length > limit {
            return Err(node::Error::Limit {
                resource: "cached generated overlay bytes",
                limit,
                actual: length,
            });
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| node::Error::Allocation)?;
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.aggregate);
        bytes.extend_from_slice(&self.entries);
        for part in [
            self.lock.as_ref(),
            self.manifest.as_ref(),
            self.source.as_ref(),
            self.source_map.as_ref(),
        ] {
            bytes.extend_from_slice(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&self.lock);
        bytes.extend_from_slice(&self.manifest);
        bytes.extend_from_slice(&self.source);
        bytes.extend_from_slice(&self.source_map);
        if bytes.len() != length {
            return Err(node::Error::State("overlay encoder length drifted"));
        }
        Ok(bytes.into_boxed_slice())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit || bytes.len() < HEADER_BYTES {
            return Err(node::Error::Limit {
                resource: "cached generated overlay bytes",
                limit,
                actual: bytes.len(),
            });
        }
        if bytes.get(..8) != Some(MAGIC) || bytes[104..112] != [0; 8] {
            return Err(node::Error::Codec("cached overlay header changed"));
        }
        let aggregate = array_at(bytes, 8, "cached overlay aggregate is truncated")?;
        let entries = array_at(bytes, 40, "cached overlay entries are truncated")?;
        let lengths = [
            usize_at(bytes, 72, "cached overlay lock length is truncated")?,
            usize_at(bytes, 80, "cached overlay manifest length is truncated")?,
            usize_at(bytes, 88, "cached overlay source length is truncated")?,
            usize_at(bytes, 96, "cached overlay source-map length is truncated")?,
        ];
        let expected = lengths
            .iter()
            .try_fold(HEADER_BYTES, |total, length| total.checked_add(*length))
            .ok_or(node::Error::Codec(
                "cached overlay length is not addressable",
            ))?;
        if expected != bytes.len() || lengths[..3].contains(&0) {
            return Err(node::Error::Codec("cached overlay framing changed"));
        }
        let mut offset = HEADER_BYTES;
        let lock = take_part(bytes, &mut offset, lengths[0])?;
        let manifest = take_part(bytes, &mut offset, lengths[1])?;
        let source = take_part(bytes, &mut offset, lengths[2])?;
        let source_map = take_part(bytes, &mut offset, lengths[3])?;
        Ok(Self {
            aggregate,
            entries,
            lock,
            manifest,
            source,
            source_map,
        })
    }
}

fn clone_bytes(bytes: &[u8]) -> Result<Vec<u8>, super::request::Error> {
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(bytes.len())
        .map_err(|_| super::request::Error::Allocation)?;
    retained.extend_from_slice(bytes);
    Ok(retained)
}

fn take_part(bytes: &[u8], offset: &mut usize, length: usize) -> Result<Box<[u8]>, node::Error> {
    let end = offset
        .checked_add(length)
        .ok_or(node::Error::Codec("cached overlay part is not addressable"))?;
    let part = bytes
        .get(*offset..end)
        .ok_or(node::Error::Codec("cached overlay part is truncated"))?;
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(part.len())
        .map_err(|_| node::Error::Allocation)?;
    retained.extend_from_slice(part);
    *offset = end;
    Ok(retained.into_boxed_slice())
}

fn array_at(bytes: &[u8], offset: usize, reason: &'static str) -> Result<[u8; 32], node::Error> {
    bytes
        .get(offset..offset.saturating_add(32))
        .ok_or(node::Error::Codec(reason))?
        .try_into()
        .map_err(|_| node::Error::Codec(reason))
}

fn usize_at(bytes: &[u8], offset: usize, reason: &'static str) -> Result<usize, node::Error> {
    let encoded: [u8; 8] = bytes
        .get(offset..offset.saturating_add(8))
        .ok_or(node::Error::Codec(reason))?
        .try_into()
        .map_err(|_| node::Error::Codec(reason))?;
    usize::try_from(u64::from_le_bytes(encoded)).map_err(|_| node::Error::Codec(reason))
}

#[cfg(all(test, feature = "test-fixtures"))]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    use crate::node::Value as _;

    use super::Product;

    #[derive(Default)]
    struct Counts {
        overlay_hits: AtomicUsize,
        overlay_misses: AtomicUsize,
        runner_hits: AtomicUsize,
        runner_misses: AtomicUsize,
        cargo_runner: AtomicUsize,
    }

    impl crate::metrics::Sink for Counts {
        fn node(&self, kind: crate::metrics::NodeKind, event: crate::metrics::NodeEvent) {
            match (kind, event) {
                (crate::metrics::NodeKind::Overlay, crate::metrics::NodeEvent::ValidatedHit) => {
                    &self.overlay_hits
                }
                (crate::metrics::NodeKind::Overlay, crate::metrics::NodeEvent::ExecutedMiss) => {
                    &self.overlay_misses
                }
                (crate::metrics::NodeKind::Runner, crate::metrics::NodeEvent::ValidatedHit) => {
                    &self.runner_hits
                }
                (crate::metrics::NodeKind::Runner, crate::metrics::NodeEvent::ExecutedMiss) => {
                    &self.runner_misses
                }
                _ => return,
            }
            .fetch_add(1, Ordering::Relaxed);
        }

        fn process(&self, kind: crate::metrics::ProcessKind) {
            if kind == crate::metrics::ProcessKind::CargoRunner {
                self.cargo_runner.fetch_add(1, Ordering::Relaxed);
            }
        }

        fn milestone(&self, _milestone: crate::metrics::Milestone, _observed_ns: Option<u64>) {}

        fn runner_owned(&self, _owned: &crate::metrics::runner::Owned) {}

        #[cfg(feature = "import-baseline")]
        fn profile(&self, _evidence: &crate::metrics::profile::Evidence) {}

        #[cfg(feature = "import-baseline")]
        fn pipeline(&self, _evidence: &crate::metrics::pipeline::Evidence) {}

        fn scope_opened(&self) {}

        fn scope_joined(&self) {}
    }

    fn product() -> Product {
        Product {
            aggregate: [1; 32],
            entries: [2; 32],
            lock: Box::from(&b"lock"[..]),
            manifest: Box::from(&b"manifest"[..]),
            source: Box::from(&b"source"[..]),
            source_map: Box::from(&b"map"[..]),
        }
    }

    fn request(
        chain: crate::node::Chain,
        revision: crate::supervisor::Revision,
        limits: &crate::cargo::Limits,
    ) -> crate::bridge::GenerationRequestKey<()> {
        crate::bridge::GenerationRequestKey {
            request: crate::surface::Request::fixture((), limits),
            native: crate::input::proof::Proof::fixture(),
            profile: crate::profile::Proof::neutral_with_chain(chain).into(),
            revision,
            build: crate::bridge::BuildRequestKey::fixture([0x4f; 32]),
        }
    }

    fn runner_product(key: &crate::bridge::GenerationRequestKey<()>) -> Product {
        const LOCK: &[u8] = b"# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"rawr-generated-runner\"\nversion = \"0.0.0\"\n";
        const MANIFEST: &[u8] = b"[package]\nname = \"rawr-generated-runner\"\nversion = \"0.0.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[workspace]\n";
        const SOURCE: &[u8] = b"fn main() {}\n";
        let source =
            crate::bridge::generate::encode_sources(&[("src/main.rs".into(), SOURCE.to_vec())])
                .expect("the overlay runner sources frame");
        let expected =
            crate::bridge::generate::expected_parts(&key.request, LOCK, MANIFEST, &source, &[])
                .expect("the overlay runner product measures");
        Product {
            aggregate: expected.aggregate,
            entries: expected.entries,
            lock: Box::from(LOCK),
            manifest: Box::from(MANIFEST),
            source: source.into_boxed_slice(),
            source_map: Box::default(),
        }
    }

    fn materialize_fixture(
        chain: &crate::node::Chain,
        key: crate::bridge::GenerationRequestKey<()>,
        executions: &AtomicUsize,
        limits: &crate::cargo::Limits,
    ) -> crate::runner::Manifest<crate::runner::Generated, ()> {
        let authority = super::authority(&key).expect("the overlay authority seals");
        let product = super::evaluate(chain, &authority, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(runner_product(&key))
        })
        .expect("the overlay node evaluates");
        let child = chain.child(&product);
        let mut manifest = product
            .value()
            .assemble(key, chain.clone(), limits)
            .expect("the cached overlay materializes");
        manifest.overlay = Some(crate::runner::overlay::Overlay::from_current(child).unwrap());
        manifest
    }

    #[test]
    fn overlay_product_roundtrips_and_rejects_framing_drift() {
        let limit = crate::cargo::Limits::standard().node_output_bytes();
        let product = product();
        let bytes = product.encode(limit).expect("the overlay product encodes");
        let decoded = Product::decode(&bytes, limit).expect("the overlay product decodes");
        assert_eq!(decoded.aggregate, [1; 32]);
        assert_eq!(decoded.source.as_ref(), b"source");
        assert!(Product::decode(&bytes[..bytes.len() - 1], limit).is_err());
        let mut reserved = bytes.into_vec();
        reserved[104] = 1;
        assert!(Product::decode(&reserved, limit).is_err());
    }

    #[test]
    fn identical_overlay_validates_without_a_second_generation() {
        let project = crate::store::TestProject::new("rawr-overlay-dag");
        let root = project.path();
        let cargo_home = root.join("cargo-home");
        fs::create_dir(&cargo_home).expect("the overlay DAG Cargo home creates");
        let limits = crate::cargo::Limits::standard();
        let context = crate::context::Context::capture(
            root,
            crate::cargo::config::Environment::from_pairs([(
                "CARGO_HOME",
                cargo_home
                    .to_str()
                    .expect("the overlay DAG Cargo home is Unicode"),
            )])
            .expect("the overlay DAG environment seals"),
            "aarch64-apple-darwin",
            &limits,
        )
        .expect("the overlay DAG context seals");
        let chain =
            crate::node::Chain::fixture(context).expect("the overlay DAG fixture parent validates");
        let executions = AtomicUsize::new(0);
        let cold = super::evaluate(&chain, b"overlay-fixture", || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(product())
        })
        .expect("the cold overlay publishes");
        let warm = super::evaluate(&chain, b"overlay-fixture", || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(product())
        })
        .expect("the warm overlay validates");
        assert_eq!(executions.load(Ordering::Relaxed), 1);
        assert_eq!(cold.value().source, warm.value().source);
    }

    #[test]
    fn identical_overlay_and_runner_reuse_zero_transform_and_cargo_children() {
        let project = crate::store::TestProject::new("rawr-overlay-runner-dag");
        let root = project.path();
        let cargo_home = root.join("cargo-home");
        fs::create_dir(&cargo_home).expect("the overlay-runner Cargo home creates");
        let limits = crate::cargo::Limits::standard();
        let context = crate::context::Context::capture(
            root,
            crate::cargo::config::Environment::capture()
                .and_then(|environment| {
                    environment.setting(
                        "CARGO_HOME",
                        cargo_home
                            .to_str()
                            .expect("the overlay-runner Cargo home is Unicode"),
                    )
                })
                .expect("the overlay-runner environment seals"),
            "aarch64-apple-darwin",
            &limits,
        )
        .expect("the overlay-runner context seals");
        let chain = crate::node::Chain::fixture(context)
            .expect("the overlay-runner fixture parent validates");
        let cache = crate::cache::Writer::new(root).expect("the runner cache binds");
        let mut supervisor = crate::supervisor::Owner::new();
        let executions = AtomicUsize::new(0);

        let cold_counts = Arc::new(Counts::default());
        let cold_scope = crate::metrics::Scope::transform_at(
            Arc::clone(&cold_counts) as Arc<dyn crate::metrics::Sink>,
            1,
        )
        .expect("the cold overlay-runner scope opens");
        let cold_started = Instant::now();
        let cold_request = request(
            chain.clone(),
            supervisor.revision().expect("the cold revision seals"),
            &limits,
        );
        let key = cold_request.build_request_key();
        let cold = materialize_fixture(&chain, cold_request, &executions, &limits);
        let cargo = crate::cargo::Owner::acquire().expect("the cold Cargo lane acquires");
        let cold = cold
            .build(&cargo, &cache)
            .expect("the cold overlay-runner artifact seals");
        drop(cargo);
        cold.artifact()
            .revalidate()
            .expect("the cold overlay-runner artifact revalidates");
        let cold_elapsed = cold_started.elapsed();
        drop(cold);
        drop(cold_scope);
        assert_eq!(executions.load(Ordering::Relaxed), 1);
        assert_eq!(cold_counts.overlay_misses.load(Ordering::Relaxed), 1);
        assert_eq!(cold_counts.overlay_hits.load(Ordering::Relaxed), 0);
        assert_eq!(cold_counts.runner_misses.load(Ordering::Relaxed), 1);
        assert_eq!(cold_counts.runner_hits.load(Ordering::Relaxed), 0);
        assert_eq!(cold_counts.cargo_runner.load(Ordering::Relaxed), 1);

        let warm_counts = Arc::new(Counts::default());
        let warm_scope = crate::metrics::Scope::transform_at(
            Arc::clone(&warm_counts) as Arc<dyn crate::metrics::Sink>,
            1,
        )
        .expect("the warm overlay-runner scope opens");
        let warm_started = Instant::now();
        let warm_request = request(
            chain.clone(),
            supervisor.revision().expect("the warm revision seals"),
            &limits,
        );
        assert_eq!(warm_request.build_request_key(), key);
        let warm = materialize_fixture(&chain, warm_request, &executions, &limits);
        let cargo = crate::cargo::Owner::acquire().expect("the warm Cargo lane acquires");
        let warm = warm
            .build(&cargo, &cache)
            .expect("the warm runner restores from the shared DAG");
        drop(cargo);
        warm.artifact()
            .revalidate()
            .expect("the warm overlay-runner artifact revalidates");
        let warm_elapsed = warm_started.elapsed();
        drop(warm);
        drop(warm_scope);
        assert_eq!(executions.load(Ordering::Relaxed), 1);
        assert_eq!(warm_counts.overlay_hits.load(Ordering::Relaxed), 1);
        assert_eq!(warm_counts.overlay_misses.load(Ordering::Relaxed), 0);
        assert_eq!(warm_counts.runner_hits.load(Ordering::Relaxed), 1);
        assert_eq!(warm_counts.runner_misses.load(Ordering::Relaxed), 0);
        assert_eq!(warm_counts.cargo_runner.load(Ordering::Relaxed), 0);
        eprintln!(
            "OVERLAY_RUNNER_DAG cold_ms={} warm_ms={} saved_ms={} cold_generate=1 warm_generate=0 cold_cargo=1 warm_cargo=0",
            cold_elapsed.as_millis(),
            warm_elapsed.as_millis(),
            cold_elapsed.saturating_sub(warm_elapsed).as_millis(),
        );
    }
}
