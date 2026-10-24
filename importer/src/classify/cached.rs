//! Content-addressed generic Rust contract classification.

use crate::node;

use super::{api, Authority, Classified, ClassifiedParts, Contract, Error, Policy};

const DOMAIN: &[u8] = b"rawr.generic.classification/v2";

/// Keep owned generic/facade results private until their complete union passes.
pub(super) fn classify_facade(
    graph: api::Graph<api::Extracted>,
    policy: &Policy,
    limits: &crate::cargo::Limits,
) -> Result<(Contract<Classified>, crate::surface::facade::Cached), Error> {
    let parent = graph.node_chain().cloned().ok_or(Error::Canonical)?;
    if parent.output_limit() != limits.node_output_bytes() {
        return Err(Error::Canonical);
    }
    let Some(retention) = parent.retention() else {
        // Existing callers without carried historical bundle commitments keep
        // their current-validation path; no staged retention is inferred.
        let contract = classify(graph, policy, limits)?;
        let facade =
            crate::surface::facade::Plan::encode_cached(&contract, limits).map_err(|error| {
                Error::Node(Box::new(node::Error::Transform(
                    error.to_string().into_boxed_str(),
                )))
            })?;
        return Ok((contract, facade));
    };
    let context = parent.context();
    let store = context
        .open_store()
        .map_err(|error| Error::Node(Box::new(error)))?;
    let recipe = node::staging::Recipe::<Product>::from_chain(
        &parent,
        &authority(policy)?,
        node::MAX_CONTENT_BYTES,
    )
    .map_err(|error| Error::Node(Box::new(error)))?;
    store
        .with_staged_batch(retention, &crate::metrics::Current, |batch| {
            let generic = recipe.evaluate(batch, || {
                Product::capture(&graph, policy, limits)
                    .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))
            })?;
            let parts = generic
                .value()
                .replay(&graph, policy, limits)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
            let authority =
                crate::surface::facade::cached::authority(parts.fingerprint.as_bytes())?;
            let facade_recipe =
                node::staging::Recipe::<crate::surface::facade::cached::Product>::child(
                    context,
                    &generic,
                    &authority,
                    limits.node_output_bytes(),
                )?;
            let facade = facade_recipe.evaluate(batch, || {
                crate::surface::facade::cached::Product::capture_parts(&graph, &parts, limits)
            })?;
            let (generic, facade) = node::staging::promote_pair(generic, facade, &store)?;
            let chain = node::Chain::validated(context.clone(), &generic);
            let contract = parts.into_contract(graph, Some(chain));
            let facade = crate::surface::facade::Cached::from_validated(context.clone(), facade);
            Ok((contract, facade))
        })
        .map_err(|error| Error::Node(Box::new(error)))
}

pub(super) fn classify(
    graph: api::Graph<api::Extracted>,
    policy: &Policy,
    limits: &crate::cargo::Limits,
) -> Result<Contract<Classified>, Error> {
    let parent = graph.node_chain().cloned().ok_or(Error::Canonical)?;
    if parent.output_limit() != limits.node_output_bytes() {
        return Err(Error::Canonical);
    }
    let authority = authority(policy)?;
    let product = parent
        .direct_artifact::<Product>(&authority, || {
            Product::capture(&graph, policy, limits)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))
        })
        .map_err(|error| Error::Node(Box::new(error)))?;
    let chain = parent.child(&product);
    let parts = product.value().replay(&graph, policy, limits)?;
    Ok(parts.into_contract(graph, Some(chain)))
}

pub(crate) fn authority(policy: &Policy) -> Result<Vec<u8>, Error> {
    let mut authority = Vec::new();
    authority
        .try_reserve_exact(DOMAIN.len().saturating_add(12))
        .map_err(|_| Error::Allocation)?;
    authority.extend_from_slice(DOMAIN);
    authority.extend_from_slice(&policy.schema.to_le_bytes());
    authority.extend_from_slice(&policy.classifier.to_le_bytes());
    authority.extend_from_slice(&policy.compiler_abi.to_le_bytes());
    Ok(authority)
}

/// Owned canonical classification, without a current contract or node chain.
pub(crate) struct Product {
    canonical: Box<[u8]>,
}

impl Product {
    pub(crate) fn capture(
        graph: &api::Graph<impl api::Phase>,
        policy: &Policy,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        let (parts, _) = graph.classify_parts(policy, Authority::production(limits))?;
        Ok(Self {
            canonical: parts.canonical,
        })
    }

    pub(crate) fn replay(
        &self,
        graph: &api::Graph<impl api::Phase>,
        policy: &Policy,
        limits: &crate::cargo::Limits,
    ) -> Result<ClassifiedParts, Error> {
        ClassifiedParts::decode(&self.canonical, graph, policy, limits)
    }
}

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::Generic;
    const SCHEMA: u32 = 4;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        if self.canonical.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached generic contract bytes",
                limit,
                actual: self.canonical.len(),
            });
        }
        if self.canonical.is_empty() {
            return Err(node::Error::Codec("cached generic contract is empty"));
        }
        Ok(self.canonical.clone())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached generic contract bytes",
                limit,
                actual: bytes.len(),
            });
        }
        if bytes.is_empty() {
            return Err(node::Error::Codec("cached generic contract is empty"));
        }
        Ok(Self {
            canonical: bytes.to_vec().into_boxed_slice(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::node::Value as _;
    use sha2::Digest as _;

    use super::{Authority, ClassifiedParts, Policy, Product};

    static TEST_ID: AtomicUsize = AtomicUsize::new(0);

    #[cfg(feature = "test-fixtures")]
    #[test]
    fn generic_facade_batch_reuses_owned_outputs_and_returns_current_receipts() {
        let root = std::env::temp_dir().join(format!(
            "rawr-generic-facade-batch-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let project = root.join("project");
        let cargo_home = root.join("cargo-home");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&cargo_home).unwrap();
        let cargo_home = fs::canonicalize(cargo_home).unwrap();
        let limits = crate::cargo::Limits::standard();
        let policy = Policy::standard();
        let context = crate::context::Context::capture(
            &project,
            crate::cargo::config::Environment::from_pairs([(
                "CARGO_HOME",
                cargo_home.to_str().unwrap(),
            )])
            .unwrap(),
            "generic-facade-test-target",
            &limits,
        )
        .unwrap();
        let store = context.open_store().unwrap();
        let seed_recipe = context
            .direct::<Product>(b"owned-api-fixture", crate::node::MAX_CONTENT_BYTES)
            .unwrap();
        let seed = crate::node::Evaluator::new(&store, &crate::metrics::Current)
            .direct(&seed_recipe, || {
                Product::capture(&crate::test_fixture::support_outcomes(), &policy, &limits)
                    .map_err(|error| {
                        crate::node::Error::Transform(error.to_string().into_boxed_str())
                    })
            })
            .unwrap();
        let retained = crate::node::read_set::Retention::from_validated(&seed, None).unwrap();
        let chain = crate::node::Chain::validated_retained(context, &seed, retained).unwrap();
        let counts = Arc::new(Counts::default());
        let scope = crate::metrics::Scope::transform_at(
            Arc::clone(&counts) as Arc<dyn crate::metrics::Sink>,
            1,
        )
        .unwrap();
        let (cold, cold_facade) = crate::test_fixture::support_outcomes()
            .with_node_chain_for_test(chain.clone())
            .classify_with_facade(&policy, &limits)
            .unwrap();
        let (warm, warm_facade) = crate::test_fixture::support_outcomes()
            .with_node_chain_for_test(chain)
            .classify_with_facade(&policy, &limits)
            .unwrap();
        assert_eq!(cold.canonical_bytes(), warm.canonical_bytes());
        assert_eq!(cold_facade.bytes(), warm_facade.bytes());
        assert!(cold.node_chain().is_some());
        assert!(warm.node_chain().is_some());
        assert!(
            warm_facade.node_chain().parent().read_set().len()
                > warm.node_chain().unwrap().parent().read_set().len()
        );
        drop(scope);
        assert_eq!(counts.misses.load(Ordering::Relaxed), 1);
        assert_eq!(counts.hits.load(Ordering::Relaxed), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn captured_graph_roundtrips_owned_classification_without_current_publication() {
        let limits = crate::cargo::Limits::standard();
        let policy = Policy::standard();
        let captured = crate::test_fixture::captured_support_outcomes();
        let extracted = crate::test_fixture::support_outcomes();
        let captured_product = Product::capture(&captured, &policy, &limits).unwrap();
        let current_product = Product::capture(&extracted, &policy, &limits).unwrap();
        assert_eq!(captured_product.canonical, current_product.canonical);
        let parts = captured_product
            .replay(&captured, &policy, &limits)
            .unwrap();
        let owned_facade =
            crate::surface::facade::Plan::encode_parts(&captured, &parts, &limits).unwrap();
        let current = extracted.classify(&policy, &limits).unwrap();
        assert_eq!(parts.fingerprint, current.fingerprint());
        assert_eq!(
            owned_facade,
            crate::surface::facade::Plan::encode(&current, &limits).unwrap()
        );
        // The captured branch only returns ClassifiedParts and facade bytes;
        // no input proof is supplied and no current contract/Chain is created.
    }

    #[test]
    fn owned_classification_parts_generate_the_same_facade_before_contract_publication() {
        let limits = crate::cargo::Limits::standard();
        let policy = Policy::standard();
        let graph = crate::test_fixture::support_outcomes();
        let product = Product::capture(&graph, &policy, &limits).expect("classify owned graph");
        let parts = product
            .replay(&graph, &policy, &limits)
            .expect("replay owned classification");
        let staged_bytes = crate::surface::facade::Plan::encode_parts(&graph, &parts, &limits)
            .expect("generate facade without a current contract");
        let contract = parts.into_contract(graph, None);
        let current_bytes = crate::surface::facade::Plan::encode(&contract, &limits)
            .expect("generate facade from the public contract");
        assert_eq!(staged_bytes, current_bytes);
        assert!(contract.node_chain().is_none());
    }

    #[derive(Default)]
    struct Counts {
        hits: AtomicUsize,
        misses: AtomicUsize,
        published: AtomicUsize,
    }

    impl crate::metrics::Sink for Counts {
        fn node(&self, kind: crate::metrics::NodeKind, event: crate::metrics::NodeEvent) {
            if kind != crate::metrics::NodeKind::Generic {
                return;
            }
            match event {
                crate::metrics::NodeEvent::ValidatedHit => &self.hits,
                crate::metrics::NodeEvent::ExecutedMiss => &self.misses,
                crate::metrics::NodeEvent::Published => &self.published,
                crate::metrics::NodeEvent::RejectedHit => return,
            }
            .fetch_add(1, Ordering::Relaxed);
        }

        fn process(&self, _kind: crate::metrics::ProcessKind) {}
        fn milestone(&self, _milestone: crate::metrics::Milestone, _observed_ns: Option<u64>) {}
        fn runner_owned(&self, _owned: &crate::metrics::runner::Owned) {}
        fn scope_opened(&self) {}
        fn scope_joined(&self) {}
    }

    #[test]
    fn generic_product_replays_the_exact_contract_and_rejects_noncanonical_bytes() {
        let limits = crate::cargo::Limits::standard();
        let policy = Policy::standard();
        let graph = crate::test_fixture::support_outcomes();
        let (parts, _) = graph
            .classify_parts(&policy, Authority::production(&limits))
            .expect("the source classification completes");
        let expected = parts.canonical.clone();
        let product = Product {
            canonical: expected.clone(),
        };
        let bytes = product
            .encode(crate::node::MAX_CONTENT_BYTES)
            .expect("the generic product encodes");
        let decoded = Product::decode(&bytes, crate::node::MAX_CONTENT_BYTES)
            .expect("the generic product decodes");
        let replayed = ClassifiedParts::decode(&decoded.canonical, &graph, &policy, &limits)
            .expect("the generic product replays");
        let contract = replayed.into_contract(graph, None);
        assert_eq!(contract.canonical_bytes(), expected.as_ref());
        let expected_fingerprint: [u8; 32] = sha2::Sha256::digest(expected).into();
        assert_eq!(contract.fingerprint().as_bytes(), &expected_fingerprint);

        let graph = crate::test_fixture::support_outcomes();
        let mut trailing = bytes.into_vec();
        trailing.push(b' ');
        assert!(ClassifiedParts::decode(&trailing, &graph, &policy, &limits).is_err());
    }

    #[cfg(feature = "test-fixtures")]
    #[test]
    fn identical_generic_contract_validates_from_the_shared_node_without_reclassification() {
        let root = std::env::temp_dir().join(format!(
            "rawr-generic-node-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("the generic fixture root creates");
        let project = root.join("project");
        let cargo_home = root.join("cargo-home");
        fs::create_dir(&project).expect("the generic fixture project creates");
        fs::create_dir(&cargo_home).expect("the generic fixture Cargo home creates");
        let cargo_home =
            fs::canonicalize(cargo_home).expect("the generic fixture Cargo home canonicalizes");
        let environment = crate::cargo::config::Environment::from_pairs([(
            "CARGO_HOME",
            cargo_home
                .to_str()
                .expect("the generic fixture Cargo home is Unicode"),
        )])
        .expect("the generic fixture environment seals");
        let limits = crate::cargo::Limits::standard();
        let context =
            crate::context::Context::capture(&project, environment, "generic-test-target", &limits)
                .expect("the generic fixture context seals");
        let parent =
            crate::node::Chain::fixture(context).expect("the generic fixture parent validates");
        let counts = Arc::new(Counts::default());
        let scope = crate::metrics::Scope::transform_at(
            Arc::clone(&counts) as Arc<dyn crate::metrics::Sink>,
            1,
        )
        .expect("the generic fixture metric scope opens");

        let first = crate::test_fixture::support_outcomes()
            .with_node_chain_for_test(parent.clone())
            .classify(&Policy::standard(), &limits)
            .expect("the first generic node publishes");
        let expected = first.canonical_bytes().to_vec();
        assert!(first.node_chain().is_some());
        drop(first);
        let second = crate::test_fixture::support_outcomes()
            .with_node_chain_for_test(parent)
            .classify(&Policy::standard(), &limits)
            .expect("the second generic node replays");
        assert_eq!(second.canonical_bytes(), expected.as_slice());
        assert!(second.node_chain().is_some());
        drop(second);
        drop(scope);

        assert_eq!(counts.misses.load(Ordering::Relaxed), 1);
        assert_eq!(counts.published.load(Ordering::Relaxed), 1);
        assert_eq!(counts.hits.load(Ordering::Relaxed), 1);
        fs::remove_dir_all(root).expect("the generic fixture retires");
    }

    #[cfg(feature = "test-fixtures")]
    #[test]
    fn canonical_edge_order_may_revisit_a_support_provenance() {
        let root = std::env::temp_dir().join(format!(
            "rawr-generic-interleaved-node-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("the interleaved fixture root creates");
        let project = root.join("project");
        let cargo_home = root.join("cargo-home");
        fs::create_dir(&project).expect("the interleaved fixture project creates");
        fs::create_dir(&cargo_home).expect("the interleaved fixture Cargo home creates");
        let cargo_home =
            fs::canonicalize(cargo_home).expect("the interleaved fixture Cargo home canonicalizes");
        let environment = crate::cargo::config::Environment::from_pairs([(
            "CARGO_HOME",
            cargo_home
                .to_str()
                .expect("the interleaved fixture Cargo home is Unicode"),
        )])
        .expect("the interleaved fixture environment seals");
        let limits = crate::cargo::Limits::standard();
        let context = crate::context::Context::capture(
            &project,
            environment,
            "interleaved-generic-test-target",
            &limits,
        )
        .expect("the interleaved fixture context seals");
        let parent =
            crate::node::Chain::fixture(context).expect("the interleaved parent validates");
        let graph = crate::test_fixture::interleaved_support_outcomes();
        let provenances = graph
            .support()
            .map(|outcome| outcome.provenance().clone())
            .collect::<Vec<_>>();
        assert_eq!(provenances.len(), 3);
        assert_eq!(provenances[0], provenances[2]);
        assert_ne!(provenances[0], provenances[1]);

        graph
            .with_node_chain_for_test(parent)
            .classify(&Policy::standard(), &limits)
            .expect("canonical interleaved support replays through the generic node");
        fs::remove_dir_all(root).expect("the interleaved fixture retires");
    }
}
