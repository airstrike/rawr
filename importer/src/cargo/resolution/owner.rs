//! Complete resolution bundles admitted through current semantic replay.

use crate::node;

/// Historical bundle association and its completed exact input evidence. Decoding
/// these bytes grants no freshness; the owner validator must replay the bundle.
pub(crate) struct Product {
    bundle: [u8; 156],
    complete: [u8; 32],
}

const BYTES: usize = 188;

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::Resolution;
    const SCHEMA: u32 = 7;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        if BYTES > limit {
            return Err(node::Error::Limit {
                resource: "resolution owner bytes",
                limit,
                actual: BYTES,
            });
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(BYTES)
            .map_err(|_| node::Error::Allocation)?;
        bytes.extend_from_slice(&self.wire());
        Ok(bytes.into_boxed_slice())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "resolution owner bytes",
                limit,
                actual: bytes.len(),
            });
        }
        let bytes: &[u8; BYTES] = bytes
            .try_into()
            .map_err(|_| node::Error::Codec("resolution owner extent differs"))?;
        Ok(Self {
            bundle: bytes[..156]
                .try_into()
                .map_err(|_| node::Error::Codec("resolution owner bundle truncated"))?,
            complete: bytes[156..]
                .try_into()
                .map_err(|_| node::Error::Codec("resolution owner identity truncated"))?,
        })
    }
}

use std::path::Path;
use std::sync::{Arc, Mutex};

use super::watched::Recorded;
use super::{bundle, cold, layout, replay};
use crate::cargo::{self, Limits, Request};
use crate::input::{self, stored};
use crate::{metrics, store};

#[cfg(feature = "import-baseline")]
pub(in crate::cargo) mod census;

/// The exact admitted node and the successful replay that admitted its bytes.
pub(crate) struct Ready {
    node: node::Validated<Product>,
    replay: replay::Replay,
    evaluation: Arc<Evaluation>,
}

impl Ready {
    #[cfg(feature = "import-baseline")]
    pub(in crate::cargo) fn observed_toolchain(&self) -> [u8; 32] {
        self.evaluation
            .cached
            .attestation
            .value()
            .proof()
            .observational_digest()
    }

    #[cfg(feature = "import-baseline")]
    pub(in crate::cargo) fn observed_identity(&self) -> [u8; 32] {
        self.replay.observation
    }

    #[cfg(test)]
    pub(crate) fn node(&self) -> &node::Validated<Product> {
        &self.node
    }
    pub(crate) fn publish(
        self,
        limits: &Limits,
    ) -> crate::Result<crate::api::Graph<crate::api::Extracted>> {
        let Self {
            node,
            replay,
            evaluation,
        } = self;
        evaluation.request.require_limits(limits)?;
        let commitment = evaluation
            .bundle_commitment(node.value(), evaluation.cached.context.graph_budget())
            .map_err(|source| crate::Error::Node {
                operation: "resolution owner retention commitment",
                source: Box::new(source),
            })?;
        let retention = node::read_set::Retention::from_validated(&node, Some(commitment))
            .map_err(|source| crate::Error::Node {
                operation: "resolution owner retention closure",
                source: Box::new(source),
            })?;
        #[cfg(feature = "import-baseline")]
        metrics::record_target(
            metrics::pipeline::target::Kind::Resolution,
            metrics::pipeline::target::Target::observe(commitment.reference()),
        );
        crate::extract::from_owner_parts(
            &evaluation.cached.context,
            evaluation.identity.as_bytes(),
            &node,
            &retention,
            &replay.decoded.metadata,
            &replay.decoded.graph,
            &replay.decoded.selected,
            evaluation.cached.attestation.value().proof(),
            &evaluation.request.target,
            &evaluation.request.profile,
            replay.completion.proof(),
            replay.capture,
            replay.input,
            &evaluation.request.policy,
            limits,
        )
    }

    pub(crate) fn retain_api(self, limits: &Limits) -> crate::Result<crate::api::Continuation> {
        let Self {
            node,
            replay,
            evaluation,
        } = self;
        evaluation.request.require_limits(limits)?;
        let commitment = evaluation
            .bundle_commitment(node.value(), evaluation.cached.context.graph_budget())
            .map_err(|source| crate::Error::Node {
                operation: "resolution owner retention commitment",
                source: Box::new(source),
            })?;
        let retention = node::read_set::Retention::from_validated(&node, Some(commitment))
            .map_err(|source| crate::Error::Node {
                operation: "resolution owner retention closure",
                source: Box::new(source),
            })?;
        #[cfg(feature = "import-baseline")]
        metrics::record_target(
            metrics::pipeline::target::Kind::Resolution,
            metrics::pipeline::target::Target::observe(commitment.reference()),
        );
        // The graph rebuild owns its observations so a resolved warm launch
        // can skip it entirely (see `api::retained::Lazy`).
        let decoded = Arc::clone(&replay.decoded);
        let source = Arc::clone(&evaluation);
        let universe = replay.completion.proof();
        let capture = replay.capture;
        let owned_limits = *limits;
        // What the API is a function of, read before the capture moves.
        let semantic = capture.semantic_sidecars().and_then(|sidecars| {
            let mut allocation = crate::extract::Allocation::new(limits);
            let context = crate::api::Context::from_resolution_parts(
                &decoded.metadata,
                &decoded.graph,
                &decoded.selected,
                evaluation.cached.attestation.value().proof(),
                &evaluation.request.target,
                &evaluation.request.profile,
                Some(universe),
                &mut allocation,
            )
            .ok()?;
            Some((context, sidecars))
        });
        let store_context = evaluation.cached.context.identity();
        crate::extract::retain_from_owner_parts(
            &evaluation.cached.context,
            evaluation.identity.as_bytes(),
            node,
            &retention,
            replay.input,
            &evaluation.request.policy,
            |policy| {
                let (context, sidecars) = semantic.as_ref()?;
                crate::api::memo::key(
                    context,
                    sidecars,
                    evaluation.identity.as_bytes(),
                    policy,
                    &store_context,
                    limits,
                )
            },
            move |stored| {
                crate::extract::materialize_owner(
                    stored,
                    &decoded.metadata,
                    &decoded.graph,
                    &decoded.selected,
                    source.cached.attestation.value().proof(),
                    &source.request.target,
                    &source.request.profile,
                    universe,
                    capture,
                    &source.request.policy,
                    &owned_limits,
                )
            },
        )
    }

    #[cfg(test)]
    pub(in crate::cargo) fn publish_input(
        self,
        graph: &node::GraphBudget,
    ) -> crate::Result<input::proof::Proof> {
        self.replay.input.publish(&self.node, graph)
    }
}

impl Product {
    pub(crate) fn bundle_commitment(
        &self,
        budget: &mut stored::Budget<'_>,
    ) -> stored::Result<store::bundle::BundleCommitment> {
        bundle::Stored::decode(&self.bundle, budget).map(|stored| stored.bundle_commitment())
    }

    /// Exact owner/input association, including path-bound replayed evidence.
    pub(crate) fn completed_identity(&self) -> &[u8; 32] {
        &self.complete
    }

    // The schema-versioned owner wire binds fixed placement, sealed evidence,
    // and exact completion. Native semantic identity is a separate concern.
    fn completed(&self, graph: &node::GraphBudget) -> Result<Box<[u8]>, node::Error> {
        graph.decode(BYTES)?;
        <Self as node::Value>::encode(self, BYTES)
    }

    fn wire(&self) -> [u8; BYTES] {
        let mut bytes = [0; BYTES];
        bytes[..156].copy_from_slice(&self.bundle);
        bytes[156..].copy_from_slice(&self.complete);
        bytes
    }
}

// Pure owner-replay interpretation staged for one graph evaluation.
// The first replay charges its authenticated parse; later consumers still run
// every current check and reuse a value only when this observation's sealed
// commitment names the same exact bytes. Never a Context, GraphBudget, or Replay.
#[derive(Default)]
struct Stage {
    lock: Option<(Limits, cargo::universe::Candidate)>,
    receipt: Option<StagedReceipt>,
    record: Option<StagedRecord>,
}

struct StagedReceipt {
    limits: Limits,
    universe: [u8; 32],
    commitment: (u64, [u8; 32]),
    receipt: cargo::universe::Receipt,
}

/// Everything the decoded record interpretation was a pure function of, besides
/// the bundle root, target and profile compared alongside it.
#[derive(PartialEq)]
struct RecordKey {
    limits: Limits,
    universe: [u8; 32],
    request: cargo::Digest,
    lock: (u64, [u8; 32]),
    receipt: (u64, [u8; 32]),
    record: (u64, [u8; 32]),
    context: cargo::universe::ContextProof,
}

struct StagedRecord {
    key: RecordKey,
    root: Box<Path>,
    target: Box<str>,
    profile: Box<str>,
    decoded: Arc<replay::Decoded>,
}

impl StagedRecord {
    fn names(&self, key: &RecordKey, root: &Path, target: &str, profile: &str) -> bool {
        self.key == *key
            && *self.root == *root
            && *self.target == *target
            && *self.profile == *profile
    }
}

#[cfg(test)]
thread_local! {
    /// Staged lock/receipt reuses observed on this test thread.
    pub(crate) static STAGE_HITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Per-validate graph decode growth on this test thread: total and by file.
    pub(crate) static REPLAY_TRACE: std::cell::RefCell<
        Vec<(u64, std::collections::BTreeMap<String, u64>)>,
    > = const { std::cell::RefCell::new(Vec::new()) };
}

fn stage_hit() {
    #[cfg(test)]
    STAGE_HITS.with(|hits| hits.set(hits.get() + 1));
}

// Counter ownership only. Never put a Context, GraphBudget, or Replay here:
// graph-owned cells must not retain their owning graph.
struct Accounts {
    input: input::Allocation,
    request: cargo::RequestAllocation,
    cargo: cargo::unit::Allocation,
    refused: bool,
}

#[cfg(feature = "import-baseline")]
impl Accounts {
    fn observe<T>(&self, stage: metrics::accounting::OwnerStage, result: &Result<T, node::Error>) {
        use metrics::accounting::Scalar;

        self.request.observe();
        let (items, bytes) = self.input.usage.observation();
        metrics::record_scalar(Scalar::OwnerInputItems, items as u64);
        metrics::record_scalar(Scalar::OwnerInputBytes, bytes as u64);
        self.cargo.observe();
        if let Err(error) = result {
            observe_refusal(stage, error);
        }
    }
}

#[cfg(feature = "import-baseline")]
fn observe_refusal(stage: metrics::accounting::OwnerStage, mut error: &node::Error) {
    use metrics::accounting::Scalar;

    metrics::record_scalar(Scalar::OwnerRefusals(stage), 1);
    let (actual, limit) = loop {
        match error {
            node::Error::Limit { actual, limit, .. } => break (*actual, *limit),
            node::Error::Importer(importer) => match importer.as_ref() {
                crate::Error::Limit { actual, limit, .. } => break (*actual, *limit),
                crate::Error::Node { source, .. } => error = source,
                _ => return,
            },
            _ => return,
        }
    };
    metrics::record_scalar(Scalar::OwnerRefusedRequired(stage), actual as u64);
    metrics::record_scalar(Scalar::OwnerRefusedLimit(stage), limit as u64);
}

struct Captured {
    enabled: bool,
    replay: Option<([u8; BYTES], replay::Replay)>,
    /// The last successful replay of this evaluation: while every path
    /// it read keeps its stamp, a later consumer reuses its result.
    current: Option<Current>,
}

struct Current {
    wire: [u8; BYTES],
    watched: super::watched::Watched,
}

/// The recorded input files of the bundles this process last replayed,
/// newest first. A later lookup of one of them refuses it without a full replay
/// when a recorded file's bytes provably changed.
static RECORDED: Mutex<Vec<([u8; BYTES], Arc<Recorded>)>> = Mutex::new(Vec::new());

/// Bundles remembered: the current one and the few an undo may return to.
const RECORDED_BUNDLES: usize = 4;

fn recorded(wire: &[u8; BYTES]) -> Option<Arc<Recorded>> {
    let recorded = RECORDED.lock().ok()?;
    recorded
        .iter()
        .find(|(known, _)| known == wire)
        .map(|(_, files)| Arc::clone(files))
}

fn remember(wire: [u8; BYTES], replay: &replay::Replay) {
    if recorded(&wire).is_some() {
        return;
    }
    let files = Arc::new(Recorded::of(
        replay.decoded.entries(),
        replay.watched.bundle(),
    ));
    if let Ok(mut recorded) = RECORDED.lock() {
        recorded.insert(0, (wire, files));
        recorded.truncate(RECORDED_BUNDLES);
    }
}

#[cfg(test)]
pub(in crate::cargo) type ScriptFactory =
    fn(&Request, &cargo::CachedToolchain, &layout::Layout) -> crate::process::test_launch::Script;

struct Evaluation {
    request: Request,
    cached: cargo::CachedToolchain,
    limits: Limits,
    store: store::Store,
    identity: cargo::Digest,
    accounts: node::account::Key<Accounts>,
    stage: node::account::Key<Stage>,
    captured: Mutex<Captured>,
    #[cfg(test)]
    script: Option<ScriptFactory>,
}

struct Preflight {
    request: Request,
    context: crate::context::Context,
    identity: cargo::Digest,
    allocation: cargo::RequestAllocation,
}

#[cfg(test)]
fn capture_context(
    request: &Request,
    limits: &Limits,
) -> Result<crate::context::Context, node::Error> {
    // A fresh graph account is measured against the captured profile's
    // graph targets, never against the standard ones unconditionally.
    capture_context_in(
        request,
        limits,
        &node::GraphBudget::with_targets(limits.graph_targets()),
    )
}

/// Capture the owner route's context in the original attempt scope: its graph
/// account, bound to the captured profile's graph targets.
fn scoped_context(
    request: &Request,
    limits: &Limits,
    scope: &crate::supervisor::snapshot::attempt::Scope,
) -> Result<crate::context::Context, node::Error> {
    capture_context_in(request, limits, scope.budget_for(limits)?)
}

/// Test entry to the owner route's scoped context capture.
#[cfg(test)]
pub(in crate::cargo) fn scoped_context_for_test(
    request: &Request,
    limits: &Limits,
    scope: &crate::supervisor::snapshot::attempt::Scope,
) -> Result<crate::context::Context, node::Error> {
    scoped_context(request, limits, scope)
}

fn capture_context_in(
    request: &Request,
    limits: &Limits,
    graph_budget: &node::GraphBudget,
) -> Result<crate::context::Context, node::Error> {
    request.require_limits(limits)?;
    crate::context::Context::capture_in(
        &request.project_root,
        request.config.environment().clone(),
        &request.target,
        limits,
        graph_budget,
    )
    .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))
}

fn preflight(
    request: Request,
    context: crate::context::Context,
    limits: &Limits,
) -> Result<Preflight, node::Error> {
    let mut allocation = request.allocation;
    let result = (|| {
        let catalog = request.universe.as_ref().ok_or(node::Error::Codec(
            "resolution owner requires versioned universe",
        ))?;
        let manifest = cargo::render_manifest_accounted(&request, None, limits, &mut allocation)?;
        cargo::select_placement_accounted(
            &request,
            catalog,
            &manifest,
            limits,
            &mut allocation,
            context.graph_budget(),
        )
        .map_err(node::Error::from)
    })();
    #[cfg(feature = "import-baseline")]
    {
        allocation.observe();
        if let Err(error) = &result {
            observe_refusal(metrics::accounting::OwnerStage::Preflight, error);
        }
    }
    let placement = result?;
    Ok(Preflight {
        request,
        context,
        identity: placement.request,
        allocation,
    })
}

pub(in crate::cargo) fn prepare_cached_in(
    request: Request,
    limits: &Limits,
    scope: &crate::supervisor::snapshot::attempt::Scope,
    sink: &dyn metrics::Sink,
) -> Result<Ready, node::Error> {
    let context = scoped_context(&request, limits, scope)?;
    let preflight = preflight(request, context, limits)?;
    let attestation = cargo::toolchain::cached(&preflight.context, sink)?;
    evaluate_preflight(
        preflight,
        attestation,
        limits,
        sink,
        #[cfg(test)]
        None,
    )
}

#[cfg(test)]
pub(in crate::cargo) fn evaluate_scripted(
    request: Request,
    cached: cargo::CachedToolchain,
    limits: &Limits,
    sink: &dyn metrics::Sink,
    script: ScriptFactory,
) -> Result<Ready, node::Error> {
    let expected_context = capture_context(&request, limits)?;
    if expected_context.identity() != cached.context.identity() {
        return Err(node::Error::Codec(
            "resolution owner context differs from sealed request",
        ));
    }
    let preflight = preflight(request, cached.context, limits)?;
    evaluate_preflight(preflight, cached.attestation, limits, sink, Some(script))
}

fn evaluate_preflight(
    preflight: Preflight,
    attestation: Arc<node::Validated<cargo::toolchain::Attestation>>,
    limits: &Limits,
    sink: &dyn metrics::Sink,
    #[cfg(test)] script: Option<ScriptFactory>,
) -> Result<Ready, node::Error> {
    let Preflight {
        request,
        context,
        identity,
        allocation: original_request,
    } = preflight;
    let cached = cargo::CachedToolchain {
        context,
        attestation,
    };
    let store = cached.context.open_store()?;
    let recipe = cached.context.discover_with_parents::<Product>(
        identity.as_bytes(),
        BYTES,
        &[node::Parent::validated(cached.attestation.as_ref())],
    )?;
    let accounts = node::account::Key::new()?;
    accounts.seed(
        store.graph_budget(),
        Accounts {
            input: input::Allocation::for_replay(limits),
            request: original_request,
            cargo: cargo::unit::Allocation::begin(limits),
            refused: false,
        },
    )?;
    let state = Arc::new(Evaluation {
        request,
        cached,
        limits: *limits,
        store: store.clone(),
        identity,
        accounts,
        stage: node::account::Key::new()?,
        #[cfg(test)]
        script,
        captured: Mutex::new(Captured {
            enabled: true,
            replay: None,
            current: None,
        }),
    });
    let validator = Arc::clone(&state);
    let executor = Arc::clone(&state);
    let node = node::Evaluator::new(&store, sink).discover_bundled_budgeted(
        &recipe,
        move |value, graph| validator.validate(value, graph),
        move |attempt| executor.execute(attempt),
    )?;
    let mut captured = state
        .captured
        .lock()
        .map_err(|_| node::Error::State("resolution replay capture was poisoned"))?;
    captured.enabled = false;
    let (wire, replay) = captured.replay.take().ok_or(node::Error::State(
        "admitted resolution lacks its successful replay",
    ))?;
    if wire != node.value().wire() {
        return Err(node::Error::State(
            "resolution replay belongs to a different candidate",
        ));
    }
    replay
        .input
        .require_account(store.graph_budget())
        .map_err(error)?;
    drop(captured);
    Ok(Ready {
        node,
        replay,
        evaluation: state,
    })
}

impl Evaluation {
    fn initial_accounts(&self) -> Accounts {
        Accounts {
            input: input::Allocation::for_replay(&self.limits),
            request: self.request.allocation,
            cargo: cargo::unit::Allocation::begin(&self.limits),
            refused: false,
        }
    }

    fn bundle_commitment(
        &self,
        value: &Product,
        graph: &node::GraphBudget,
    ) -> Result<store::bundle::BundleCommitment, node::Error> {
        self.accounts.with(
            graph,
            || Ok(self.initial_accounts()),
            |accounts| {
                if accounts.refused {
                    return Err(node::Error::State("resolution owner account refused"));
                }
                let result = value
                    .bundle_commitment(&mut stored::Budget::borrow(
                        &self.limits,
                        &mut accounts.input,
                        graph,
                    ))
                    .map_err(error);
                if result.as_ref().is_err_and(refusal) {
                    accounts.refused = true;
                }
                #[cfg(feature = "import-baseline")]
                accounts.observe(metrics::accounting::OwnerStage::Commitment, &result);
                result
            },
        )
    }

    /// Every replay checks the complete scope, the strongest any consumer asks
    /// for, so its stamps answer every later consumer of this evaluation.
    fn validate(
        &self,
        value: &Product,
        graph: &node::GraphBudget,
    ) -> Result<Box<[u8]>, node::Error> {
        {
            let mut captured = self
                .captured
                .lock()
                .map_err(|_| node::Error::State("resolution replay capture was poisoned"))?;
            if !captured.enabled
                && captured.current.as_ref().is_some_and(|current| {
                    current.wire == value.wire() && current.watched.unchanged()
                })
            {
                return value.completed(graph);
            }
            captured.replay = None;
            captured.current = None;
        }
        if recorded(&value.wire()).is_some_and(|recorded| recorded.changed()) {
            return Err(node::Error::Transform(
                "complete current input entries differ".into(),
            ));
        }
        #[cfg(test)]
        let trace_before = (
            graph.census_usage()?.0,
            node::DECODE_SITES.with(|sites| sites.borrow().clone()),
        );
        let outcome = self.accounts.with(
            graph,
            || Ok(self.initial_accounts()),
            |accounts| {
                if accounts.refused {
                    return Err(node::Error::State("resolution owner account refused"));
                }
                let result = (|| {
                    let mut budget =
                        stored::Budget::borrow(&self.limits, &mut accounts.input, graph);
                    let stored =
                        bundle::Stored::decode(&value.bundle, &mut budget).map_err(error)?;
                    graph.hash(b"rawr.node.content/v1".len() + 1 + 4 + 8 + BYTES)?;
                    let observed = stored
                        .read_published_scoped_for_replay(
                            &self.store,
                            <Product as node::Value>::KIND,
                            <Product as node::Value>::SCHEMA,
                            node::content_digest::<Product>(&value.wire()),
                            &mut budget,
                            node::read_set::Scope::Complete,
                        )
                        .map_err(error)?;
                    let replay =
                        self.replay(observed, graph, &mut accounts.request, &mut accounts.cargo)?;
                    if replay.completion.proof().complete().as_bytes() != &value.complete {
                        return Err(node::Error::Codec(
                            "resolution owner completion identity differs",
                        ));
                    }
                    Ok((replay, value.completed(graph)?))
                })();
                if result.as_ref().is_err_and(refusal) {
                    accounts.refused = true;
                }
                #[cfg(feature = "import-baseline")]
                accounts.observe(metrics::accounting::OwnerStage::Replay, &result);
                result
            },
        );
        #[cfg(test)]
        {
            let decoded = graph.census_usage()?.0 - trace_before.0;
            let mut files = std::collections::BTreeMap::<String, u64>::new();
            node::DECODE_SITES.with(|sites| {
                for (site, (_, bytes)) in sites.borrow().iter() {
                    let before = trace_before.1.get(site).map_or(0, |(_, bytes)| *bytes);
                    let file = site
                        .rsplit_once(':')
                        .map_or(site.as_str(), |(file, _)| file);
                    *files.entry(file.to_owned()).or_default() += bytes - before;
                }
            });
            REPLAY_TRACE.with(|trace| trace.borrow_mut().push((decoded, files)));
        }
        let (mut replay, completed) = outcome?;
        let mut captured = self
            .captured
            .lock()
            .map_err(|_| node::Error::State("resolution replay capture was poisoned"))?;
        remember(value.wire(), &replay);
        captured.current = Some(Current {
            wire: value.wire(),
            watched: std::mem::take(&mut replay.watched),
        });
        if captured.enabled {
            captured.replay = Some((value.wire(), replay));
        }
        Ok(completed)
    }

    fn replay(
        &self,
        observed: bundle::PendingReplay<'_, '_, '_>,
        graph: &node::GraphBudget,
        request_account: &mut cargo::RequestAllocation,
        cargo_account: &mut cargo::unit::Allocation,
    ) -> Result<replay::Replay, node::Error> {
        let definition = self
            .request
            .universe
            .as_ref()
            .ok_or(node::Error::Codec("resolution owner universe absent"))?
            .current();
        let paths = observed.paths();
        // Observation already hashed the current control files against these
        // sealed commitments; staged values are exact owned bytes they name.
        let lock_commitment = observed.commitment(paths.final_form().lock())?;
        let receipt_commitment = observed.commitment(paths.receipt())?;
        let record_commitment = observed.commitment(paths.record())?;
        let (lock, receipt) = self.stage.with(
            graph,
            || Ok(Stage::default()),
            |stage| {
                let lock = match stage.lock.as_ref().filter(|(limits, lock)| {
                    *limits == self.limits && lock.names(definition, lock_commitment)
                }) {
                    Some((_, lock)) => {
                        stage_hit();
                        lock.clone()
                    }
                    None => {
                        // Lock reads are I/O retention bounded by the
                        // physical capture constant, never by the JSON target.
                        let lock_bytes = read(
                            paths.final_form().lock(),
                            crate::process::CAPTURE_BYTES,
                            request_account,
                            graph,
                        )?;
                        observed.require_bytes(paths.final_form().lock(), &lock_bytes)?;
                        let lock = definition.candidate_accounted(
                            lock_bytes.into_boxed_slice(),
                            &self.limits,
                            request_account,
                            graph,
                        )?;
                        stage.lock = Some((self.limits, lock.clone()));
                        lock
                    }
                };
                let receipt = match &stage.receipt {
                    Some(staged)
                        if staged.limits == self.limits
                            && staged.universe == *definition.identity()
                            && staged.commitment == receipt_commitment =>
                    {
                        stage_hit();
                        staged.receipt.clone()
                    }
                    _ => {
                        let receipt_bytes = read(
                            paths.receipt(),
                            crate::process::CAPTURE_BYTES,
                            request_account,
                            graph,
                        )?;
                        observed.require_bytes(paths.receipt(), &receipt_bytes)?;
                        cargo::ControlAccount::current(request_account, graph)
                            .parser(receipt_bytes.len())?;
                        let receipt = cargo::universe::Receipt::parse(&receipt_bytes, definition)?;
                        stage.receipt = Some(StagedReceipt {
                            limits: self.limits,
                            universe: *definition.identity(),
                            commitment: receipt_commitment,
                            receipt: receipt.clone(),
                        });
                        receipt
                    }
                };
                Ok((lock, receipt))
            },
        )?;
        if !receipt.matches_request(self.identity) {
            return Err(node::Error::Codec(
                "resolution bundle receipt belongs to another request",
            ));
        }
        let toolchain = self.cached.attestation.value().proof();
        let context = definition.validate_context(
            &self.request.target,
            &self.request.profile,
            toolchain.rustc_commit(),
            toolchain.cargo_commit(),
            self.request.config.projection_schema(),
            self.request.toolchain.schemas().graph(),
            self.request.toolchain.schemas().unit(),
            cold::cohort(&self.request, paths, request_account)?,
            self.request.config.universe_context(),
            self.request.config.digest(),
        )?;
        // The record interpretation is pure over the sealed bytes this
        // observation hashed and over these exact associations; a staged one is
        // reused only when every component agrees, else decoded afresh.
        let key = RecordKey {
            limits: self.limits,
            universe: *definition.identity(),
            request: self.identity,
            lock: lock_commitment,
            receipt: receipt_commitment,
            record: record_commitment,
            context,
        };
        let staged = self.stage.with(
            graph,
            || Ok(Stage::default()),
            |stage| match stage.record.as_ref().filter(|staged| {
                staged.names(
                    &key,
                    paths.root(),
                    &self.request.target,
                    &self.request.profile,
                )
            }) {
                Some(staged) => {
                    // A hit allocates nothing but the holder this replay retains.
                    cargo::ControlAccount::current(request_account, graph)
                        .holder(std::mem::size_of::<Arc<replay::Decoded>>())?;
                    stage_hit();
                    Ok(Some(Arc::clone(&staged.decoded)))
                }
                None => Ok(None),
            },
        )?;
        let fresh = match &staged {
            Some(_) => None,
            None => {
                request_account.path(paths.root())?;
                request_account.string(&self.request.target)?;
                request_account.string(&self.request.profile)?;
                Some((
                    Box::<Path>::from(paths.root()),
                    Box::<str>::from(&*self.request.target),
                    Box::<str>::from(&*self.request.profile),
                ))
            }
        };
        let replay = self
            .request
            .replay_observed_in(
                &self.cached,
                observed,
                Some(self.cached.context.cargo_home()),
                definition,
                lock,
                self.identity,
                context,
                Some(receipt),
                staged,
                &self.limits,
                &self.limits,
                request_account,
                cargo_account,
            )
            .map_err(error)?;
        if let Some((root, target, profile)) = fresh {
            self.stage.with(
                graph,
                || Ok(Stage::default()),
                |stage| {
                    stage.record = Some(StagedRecord {
                        key,
                        root,
                        target,
                        profile,
                        decoded: Arc::clone(&replay.decoded),
                    });
                    Ok(())
                },
            )?;
        }
        Ok(replay)
    }

    fn execute(
        &self,
        attempt: store::bundle::Attempt,
    ) -> Result<node::Bundled<Product>, node::Error> {
        let graph = self.store.graph_budget();
        self.accounts.with(
            graph,
            || Ok(self.initial_accounts()),
            |accounts| {
                if accounts.refused {
                    return Err(node::Error::State("resolution owner account refused"));
                }
                let result = (|| {
                    // Taking the Cargo account transfers actual spent ownership into the
                    // cold harness. Any failed tail remains terminal in this graph.
                    accounts.refused = true;
                    let cargo_account = std::mem::replace(
                        &mut accounts.cargo,
                        cargo::unit::Allocation::begin(&self.limits),
                    );
                    let layout = layout::Layout::bind(attempt, graph)?;
                    #[cfg(test)]
                    let script = self
                        .script
                        .map(|factory| factory(&self.request, &self.cached, &layout));
                    let run = || self.execute_in(layout, cargo_account, accounts, graph);
                    #[cfg(test)]
                    if let Some(script) = script {
                        return script.run_result(run).map_err(|error| {
                            node::Error::Transform(error.to_string().into_boxed_str())
                        })?;
                    }
                    run()
                })();
                #[cfg(feature = "import-baseline")]
                accounts.observe(metrics::accounting::OwnerStage::Execute, &result);
                result
            },
        )
    }

    fn execute_in(
        &self,
        layout: layout::Layout,
        cargo_account: cargo::unit::Allocation,
        accounts: &mut Accounts,
        graph: &node::GraphBudget,
    ) -> Result<node::Bundled<Product>, node::Error> {
        #[cfg(feature = "import-baseline")]
        census::checkpoint("before-prepare-harness", graph)?;
        let harness = self.request.prepare_harness_in(
            &layout,
            &self.cached,
            &self.limits,
            &self.limits,
            &mut accounts.request,
            cargo_account,
        )?;
        #[cfg(feature = "import-baseline")]
        census::checkpoint("after-prepare-harness", graph)?;
        let built = self.request.build_harness_in(
            &self.cached,
            &layout,
            harness,
            &mut accounts.request,
            &self.limits,
            &self.limits,
        )?;
        #[cfg(feature = "import-baseline")]
        census::checkpoint("after-build-harness", graph)?;
        let completed = self.request.complete_parts_in(
            &self.cached,
            &layout,
            built,
            &self.limits,
            &self.limits,
        )?;
        // The build's own completion names the product; the one replay after
        // publication proves the bundle reaches it, so the sealed bundle is
        // not replayed here as well.
        let complete = *completed
            .universe
            .ok_or(node::Error::Codec(
                "complete resolution did not retain its universe proof",
            ))?
            .complete()
            .as_bytes();
        let sealed = self.seal_completed(completed, layout, accounts, graph)?;
        let (stored, pending) = sealed.into_parts();
        let product = Product {
            bundle: stored.encode(),
            complete,
        };
        let bundled = node::Bundled::validate(product, pending, |value, _| value.completed(graph))?;
        accounts.refused = false;
        Ok(bundled)
    }

    fn seal_completed(
        &self,
        completed: cargo::CompletedHarness,
        layout: layout::Layout,
        accounts: &mut Accounts,
        graph: &node::GraphBudget,
    ) -> Result<bundle::Sealed, node::Error> {
        // Consuming the cold result here releases its semantic data and the
        // encoding buffers before current replay builds the validated owner.
        accounts.cargo = completed.cargo_allocation;
        let retained = completed.observations.ok_or(node::Error::Codec(
            "complete resolution did not retain its observations",
        ))?;
        let (stored, original) =
            stored::Stored::capture(completed.input, &self.limits, graph).into_parts();
        accounts.input.absorb(original)?;
        let stored = stored.map_err(error)?;
        let scope = super::content::Scope::from_observations(
            layout.root(),
            &stored,
            &retained.build,
            &completed.capture,
            &completed.toolchain,
            &self.request.target,
            graph,
        )?;
        #[cfg(feature = "import-baseline")]
        if option_env!("RAWR_SCOPE_CENSUS") == Some("1") {
            let query = super::content::Scope::from_query_observations(
                layout.root(),
                &stored,
                &retained.build,
                &completed.capture,
                &completed.toolchain,
                &self.request.target,
                graph,
            )?;
            census::report(layout.root(), &scope, &query, &completed.capture, graph)?;
        }
        let mut budget = stored::Budget::borrow(&self.limits, &mut accounts.input, graph);
        let bytes = match completed.capture.namespace() {
            Some(namespace) => stored::Record::encode_with_namespace(
                retained.documents.metadata.bytes(),
                retained.documents.units.bytes(),
                &retained.build,
                &stored,
                namespace,
                &mut budget,
            ),
            None => stored::Record::encode(
                retained.documents.metadata.bytes(),
                retained.documents.units.bytes(),
                &retained.build,
                &stored,
                &mut budget,
            ),
        }
        .map_err(error)?;
        let record = stored::Record::decode(&bytes, &mut budget).map_err(error)?;
        bundle::Stored::capture(layout, &record, &scope, &mut budget).map_err(error)
    }
}

fn read(
    path: &Path,
    limit: usize,
    allocation: &mut cargo::RequestAllocation,
    graph: &node::GraphBudget,
) -> Result<Vec<u8>, node::Error> {
    cargo::read_bounded_in("resolution replay control", path, limit, allocation, graph)
        .map_err(Into::into)
}

fn error(error: stored::Error) -> node::Error {
    match error {
        stored::Error::Graph(error) | stored::Error::Bundle(error) => error,
        stored::Error::Input(crate::Error::Node { source, .. }) => *source,
        stored::Error::Input(error) => error.into(),
        stored::Error::Allocation(source) => crate::Error::Allocation {
            operation: "stored input replay",
            source,
        }
        .into(),
        stored::Error::Overflow => node::Error::Allocation,
        stored::Error::Observation(stored::observe::Error::Allocation) => node::Error::Allocation,
        stored::Error::Observation(stored::observe::Error::Io { operation, source }) => {
            node::Error::ObservationIo { operation, source }
        }
        stored::Error::Refused => {
            node::Error::Importer(Box::new(crate::Error::InputAccountRefused))
        }
        error => node::Error::Transform(error.to_string().into_boxed_str()),
    }
}

fn refusal(mut error: &node::Error) -> bool {
    loop {
        match error {
            node::Error::Limit { .. } | node::Error::Allocation => return true,
            node::Error::Importer(importer) => match importer.as_ref() {
                crate::Error::InputAccountRefused
                | crate::Error::Limit { .. }
                | crate::Error::Allocation { .. } => return true,
                crate::Error::Node { source, .. } => error = source,
                _ => return false,
            },
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_facts_use_the_canonical_wire_and_original_graph_account() {
        let product = Product {
            bundle: [1; 156],
            complete: [2; 32],
        };
        let graph = node::GraphBudget::new();
        let facts = product.completed(&graph).unwrap();
        assert_eq!(facts.as_ref(), product.wire());
        assert_eq!(graph.census_usage().unwrap(), (BYTES as u64, 0, 0));
        let decoded = <Product as node::Value>::decode(&facts, BYTES).unwrap();
        assert_eq!(decoded.completed(&graph).unwrap(), facts);
        assert_eq!(graph.census_usage().unwrap(), (2 * BYTES as u64, 0, 0));

        graph
            .decode((node::MAX_DECODED_BYTES_PER_GRAPH - 2 * BYTES as u64) as usize)
            .unwrap();
        assert!(!graph.exceedances().is_exceeded());
        // The decode account is a target. Completion past it still
        // produces the canonical wire, charges the original graph in full and
        // records the crossing by exactly the wire's length.
        assert_eq!(product.completed(&graph).unwrap(), facts);
        assert_eq!(
            graph.census_usage().unwrap(),
            (node::MAX_DECODED_BYTES_PER_GRAPH + BYTES as u64, 0, 0)
        );
        let record = graph.exceedances();
        let axis = record
            .axis(node::DECODED_RESOURCE)
            .expect("completion records the decode crossing");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                node::MAX_DECODED_BYTES_PER_GRAPH,
                node::MAX_DECODED_BYTES_PER_GRAPH + BYTES as u64,
                node::MAX_DECODED_BYTES_PER_GRAPH + BYTES as u64,
            )
        );
        assert!(axis.site.is_some());
        // The node content bound is physical and still refuses.
        assert!(matches!(
            <Product as node::Value>::encode(&product, BYTES - 1),
            Err(node::Error::Limit { actual: BYTES, .. })
        ));
    }

    #[test]
    fn legacy_completion_schema_has_no_current_recipe_authority() {
        struct Legacy(Product);
        impl node::Value for Legacy {
            const KIND: node::Kind = node::Kind::Resolution;
            const SCHEMA: u32 = 6;

            fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
                <Product as node::Value>::encode(&self.0, limit)
            }

            fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
                <Product as node::Value>::decode(bytes, limit).map(Self)
            }
        }

        let old = node::Discoverable::<Legacy>::capture([1; 32], &[2; 32], BYTES, &[], 0).unwrap();
        let current =
            node::Discoverable::<Product>::capture([1; 32], &[2; 32], BYTES, &[], 0).unwrap();
        assert_ne!(old.lookup(), current.lookup());
        assert_ne!(old.execution(), current.execution());
        let old = node::Direct::<Legacy>::capture([1; 32], &[2; 32], BYTES, &[], 0).unwrap();
        let current = node::Direct::<Product>::capture([1; 32], &[2; 32], BYTES, &[], 0).unwrap();
        let legacy = node::codec::Envelope::direct(&old, node::Digest::from_bytes([3; 32]), BYTES)
            .unwrap()
            .encode()
            .unwrap();
        let historical = node::codec::Envelope::decode(&legacy).unwrap();
        assert!(historical.matches(&old));
        assert!(!historical.matches(&current));
        assert_eq!(historical.encode().unwrap(), legacy);
    }

    #[test]
    fn replay_allocation_failures_stay_typed_and_latch_original_input_account() {
        fn allocation() -> crate::Error {
            crate::Error::Allocation {
                operation: "fixture retained replay storage",
                source: Vec::<u8>::new().try_reserve(usize::MAX).unwrap_err(),
            }
        }
        let failures: [fn() -> stored::Error; 3] = [
            || stored::Error::Observation(stored::observe::Error::Allocation),
            || stored::Error::Input(allocation()),
            || {
                stored::Error::Input(crate::Error::Node {
                    operation: "fixture nested replay",
                    source: Box::new(node::Error::Importer(Box::new(allocation()))),
                })
            },
        ];
        let limits = Limits::standard();
        for failure in failures {
            let graph = node::GraphBudget::new();
            let mut account = input::Allocation::for_replay(&limits);
            let mut budget = stored::Budget::borrow(&limits, &mut account, &graph);
            let original = budget.failure(failure());
            assert!(matches!(budget.check(), Err(stored::Error::Refused)));
            let mapped = error(original);
            assert!(refusal(&mapped));
            match mapped {
                node::Error::Allocation => {}
                node::Error::Importer(inner) => assert!(matches!(
                    inner.as_ref(),
                    crate::Error::Allocation {
                        operation: "fixture retained replay storage",
                        ..
                    }
                )),
                other => panic!("allocation lost its typed identity: {other:?}"),
            }
        }
    }

    #[test]
    fn replay_observation_io_keeps_source_while_stale_evidence_remains_a_miss() {
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::NotFound,
        ] {
            let mapped = error(stored::Error::Observation(stored::observe::Error::Io {
                operation: "fixture current observation",
                source: std::io::Error::from(kind),
            }));
            assert!(matches!(mapped, node::Error::ObservationIo {
                operation: "fixture current observation",
                source,
            } if source.kind() == kind));
        }
        let limits = Limits::standard();
        for stale in [
            stored::Error::Encoding("fixture stale encoding"),
            stored::Error::Observation(stored::observe::Error::Policy("fixture stale shape")),
            stored::Error::Observation(stored::observe::Error::Drift("fixture source changed")),
        ] {
            let graph = node::GraphBudget::new();
            let mut account = input::Allocation::for_replay(&limits);
            let mut budget = stored::Budget::borrow(&limits, &mut account, &graph);
            let mapped = error(budget.failure(stale));
            assert!(budget.check().is_ok());
            assert!(matches!(mapped, node::Error::Transform(_)));
            assert!(!refusal(&mapped));
        }
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn allocation_observation_preserves_original_usage_and_refusal_amounts() {
        use metrics::accounting::{OwnerStage, Scalar};

        let limits = Limits::standard();
        let mut accounts = Accounts {
            input: input::Allocation::for_replay(&limits),
            request: cargo::RequestAllocation::new(&limits),
            cargo: cargo::unit::Allocation::begin(&limits),
            refused: false,
        };
        accounts.request.retained(2, 7).unwrap();
        accounts.input.items(3).unwrap();
        accounts.input.bytes(11).unwrap();
        let result: Result<(), node::Error> = Err(crate::Error::Limit {
            resource: "request retained bytes",
            limit: 8,
            actual: 9,
        }
        .into());
        let recorder = metrics::recorder::Recorder::transform_at(10, 1).unwrap();
        {
            let _scope = recorder.scope().unwrap();
            accounts.observe(OwnerStage::Execute, &result);
            accounts.observe::<()>(
                OwnerStage::Replay,
                &Err(node::Error::Codec("ordinary stale input")),
            );
        }
        assert!(!accounts.refused);
        assert!(matches!(&result, Err(node::Error::Importer(inner))
        if matches!(inner.as_ref(), crate::Error::Limit {
            resource: "request retained bytes", limit: 8, actual: 9,
        })));
        let evidence = recorder.snapshot().unwrap().accounting;
        for (scalar, value) in [
            (Scalar::CargoRequestItems, 2),
            (Scalar::CargoRequestBytes, 7),
            (Scalar::OwnerInputItems, 3),
            (Scalar::OwnerInputBytes, 11),
            (Scalar::OwnerRefusedRequired(OwnerStage::Execute), 9),
            (Scalar::OwnerRefusedLimit(OwnerStage::Execute), 8),
        ] {
            assert_eq!(evidence.scalar(scalar).unwrap().max, value);
        }
        assert_eq!(
            evidence
                .scalar(Scalar::OwnerRefusals(OwnerStage::Replay))
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            evidence.scalar(Scalar::OwnerRefusedRequired(OwnerStage::Replay)),
            None
        );
    }

    #[test]
    fn wrapped_control_graph_refusal_remains_terminal() {
        // Crossing a graph accounting target is not an error at all. The
        // control read admits its seven path checks in full on the spent
        // account, records the crossing, and fails only for its own reason.
        let graph = node::GraphBudget::new();
        graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH - 1).unwrap();
        let mut request = cargo::RequestAllocation::new(&Limits::standard());
        let error = read(
            Path::new("/nonexistent-rawr-owner-control"),
            1024,
            &mut request,
            &graph,
        )
        .unwrap_err();
        assert!(
            !refusal(&error),
            "a missing control is not a refusal: {error:?}"
        );
        let record = graph.exceedances();
        let axis = record
            .axis(node::PATHS_RESOURCE)
            .expect("the spent graph records the control read's path checks");
        assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
        assert_eq!(axis.first_requested, axis.target + 6);
        // A real control past the target still reads; only the physical byte
        // bound refuses, and that refusal stays terminal.
        let root = std::env::temp_dir().join(format!(
            "rawr-owner-control-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir(&root).unwrap();
        let root = std::fs::canonicalize(&root).unwrap();
        let control = root.join("control");
        std::fs::write(&control, b"sixteen bytes!!!").unwrap();
        assert_eq!(
            read(&control, 1024, &mut request, &graph).unwrap(),
            b"sixteen bytes!!!"
        );
        assert!(
            graph
                .exceedances()
                .axis(node::PATHS_RESOURCE)
                .unwrap()
                .high_water
                > axis.high_water
        );
        let physical = read(&control, 8, &mut request, &graph).unwrap_err();
        assert!(matches!(&physical, node::Error::Importer(inner)
            if matches!(inner.as_ref(), crate::Error::Limit { limit: 8, actual: 16, .. })));
        assert!(refusal(&physical));
        std::fs::remove_dir_all(&root).unwrap();
        // Wrapped genuine failures remain terminal regardless of the graph's
        // numeric state: a smaller graph charge still fits after each of them.
        let failures = [
            crate::Error::Node {
                operation: "fixture control",
                source: Box::new(node::Error::Allocation),
            },
            crate::Error::InputAccountRefused,
            crate::Error::Allocation {
                operation: "fixture control",
                source: Vec::<u8>::new().try_reserve(usize::MAX).unwrap_err(),
            },
        ];
        for wrapped in failures {
            let error = node::Error::Importer(Box::new(wrapped));
            graph.paths(1).unwrap();
            assert!(refusal(&error), "{error:?}");
        }
        assert!(refusal(&node::Error::Allocation));
        assert!(!refusal(&node::Error::Codec("ordinary stale control")));
    }
}
