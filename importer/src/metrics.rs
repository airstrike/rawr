//! Bounded observational evidence for importer work and child processes.
//!
//! These counters never participate in requests, recipes, cache identities, or
//! scheduling. With no scoped evidence sink installed, every observation is an
//! allocation-free call through [`Noop`].

#[cfg(any(test, feature = "test-fixtures"))]
use std::cell::RefCell;
#[cfg(any(test, feature = "test-fixtures"))]
use std::rc::Rc;
#[cfg(any(test, feature = "test-fixtures"))]
use std::sync::Arc;

pub mod accounting;
#[cfg(any(test, feature = "test-fixtures"))]
pub mod recorder;
pub mod runner;

/// One closed transform in the generated-owner dependency pipeline.
#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum NodeKind {
    Toolchain,
    Resolution,
    Api,
    Generic,
    Profile,
    Facade,
    Checked,
    ProfileProof,
    NativeProduct,
    Overlay,
    Runner,
}

/// One observational result for a dependency node.
#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum NodeEvent {
    ValidatedHit,
    ExecutedMiss,
    RejectedHit,
    Published,
}

/// One closed child-process role.
#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ProcessKind {
    RustupWhich,
    RustcVersion,
    RustdocVersion,
    CargoVersion,
    DriverVersion,
    ComponentList,
    TargetList,
    CargoResolution,
    PackageList,
    MetadataProbe,
    CargoRunner,
    Runner,
}

/// One lifecycle boundary measured from the beginning of an evidence scope.
#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Milestone {
    RunnerSpawned,
    Prepared,
    Activated,
    Admitted,
    Started,
    Visible,
}

/// A write-only observational sink with no execution or publication authority.
pub trait Sink: Send + Sync {
    fn node(&self, kind: NodeKind, event: NodeEvent);
    fn process(&self, kind: ProcessKind);
    fn milestone(&self, milestone: Milestone, observed_ns: Option<u64>);
    fn runner_owned(&self, owned: &runner::Owned);
    /// Observe a produced scalar without influencing the producer's result.
    fn scalar(&self, _scalar: accounting::Scalar, _value: u64) {}
    /// Retain conversion failure as evidence without changing the product result.
    fn scalar_overflow(&self, _scalar: accounting::Scalar) {}
    /// Observe the same original graph account, including any refused charge.
    fn graph(&self, _graph: accounting::Graph) {}
    /// Observe fixed actual native-account state from the same authenticated runner.
    fn native(
        &self,
        _runner_pid: u32,
        _epoch: u64,
        _observation: crate::protocol::account::Observation,
    ) {
    }
    /// Record incomplete native telemetry without replacing the product result.
    fn native_fault(&self, _runner_pid: u32, _epoch: u64, _cause: accounting::NativeFault) {}
    fn scope_opened(&self);
    fn scope_joined(&self);
}

/// Allocation-free production observation when no evidence scope is active.
#[derive(Clone, Copy, Debug, Default)]
pub struct Noop;

impl Sink for Noop {
    fn node(&self, _kind: NodeKind, _event: NodeEvent) {}

    fn process(&self, _kind: ProcessKind) {}

    fn milestone(&self, _milestone: Milestone, _observed_ns: Option<u64>) {}

    fn runner_owned(&self, _owned: &runner::Owned) {}

    fn scope_opened(&self) {}

    fn scope_joined(&self) {}
}

/// Forward node observations into the launch's currently installed evidence
/// sink without retaining any scheduling or cache authority.
#[derive(Clone, Copy, Debug, Default)]
pub struct Current;

impl Sink for Current {
    fn native(
        &self,
        runner_pid: u32,
        epoch: u64,
        observation: crate::protocol::account::Observation,
    ) {
        record_native(runner_pid, epoch, observation);
    }

    fn native_fault(&self, runner_pid: u32, epoch: u64, cause: accounting::NativeFault) {
        record_native_fault(runner_pid, epoch, cause);
    }
    fn node(&self, kind: NodeKind, event: NodeEvent) {
        record_node(kind, event);
    }

    fn process(&self, kind: ProcessKind) {
        record_process(kind);
    }

    fn milestone(&self, milestone: Milestone, _observed_ns: Option<u64>) {
        record_milestone(milestone);
    }

    fn runner_owned(&self, owned: &runner::Owned) {
        record_runner_owned(owned);
    }

    fn scalar(&self, scalar: accounting::Scalar, value: u64) {
        record_scalar(scalar, value);
    }

    fn scalar_overflow(&self, scalar: accounting::Scalar) {
        with_sink(|sink, _| sink.scalar_overflow(scalar));
    }

    fn graph(&self, graph: accounting::Graph) {
        record_graph(graph);
    }

    fn scope_opened(&self) {}

    fn scope_joined(&self) {}
}

#[cfg(any(test, feature = "test-fixtures"))]
struct Active {
    sink: Arc<dyn Sink>,
    origin_ns: u64,
}

#[cfg(any(test, feature = "test-fixtures"))]
thread_local! {
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

/// Opaque same-thread proof that one evidence sink is currently installed.
#[cfg(any(test, feature = "test-fixtures"))]
pub struct Scope {
    _same_thread: std::marker::PhantomData<Rc<()>>,
}

/// Failure to install an observational evidence scope.
#[cfg(any(test, feature = "test-fixtures"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ScopeError {
    #[error("an importer metrics scope is already active on this thread")]
    Nested,
    #[error("the process-shared monotonic clock is unavailable")]
    Clock,
}

#[cfg(any(test, feature = "test-fixtures"))]
impl Scope {
    pub(crate) fn transform(sink: Arc<dyn Sink>) -> Result<Self, ScopeError> {
        let origin_ns = monotonic_ns().ok_or(ScopeError::Clock)?;
        Self::transform_at(sink, origin_ns)
    }

    /// Install a sink with an externally captured process-shared clock origin.
    #[doc(hidden)]
    pub fn transform_at(sink: Arc<dyn Sink>, origin_ns: u64) -> Result<Self, ScopeError> {
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            if active.is_some() {
                return Err(ScopeError::Nested);
            }
            sink.scope_opened();
            *active = Some(Active { sink, origin_ns });
            Ok(Self {
                _same_thread: std::marker::PhantomData,
            })
        })
    }
}

#[cfg(any(test, feature = "test-fixtures"))]
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            if let Some(active) = active.borrow_mut().take() {
                active.sink.scope_joined();
            }
        });
    }
}

/// Record one node result without granting transform or cache authority.
#[doc(hidden)]
pub fn record_node(kind: NodeKind, event: NodeEvent) {
    with_sink(|sink, _| sink.node(kind, event));
}

/// Record one child after the closed launcher has spawned it successfully.
#[doc(hidden)]
pub fn record_process(kind: ProcessKind) {
    with_sink(|sink, _| sink.process(kind));
}

/// Record one lifecycle boundary from the current evidence-scope origin.
#[doc(hidden)]
pub fn record_milestone(milestone: Milestone) {
    with_sink(|sink, _| sink.milestone(milestone, monotonic_ns()));
}

/// Record the authenticated runner identity at its truthful ownership seam.
#[doc(hidden)]
pub fn record_runner_owned(owned: &runner::Owned) {
    with_sink(|sink, _| sink.runner_owned(owned));
}

/// Record a scalar from an existing encoding or retained transformation.
#[doc(hidden)]
pub fn record_scalar(scalar: accounting::Scalar, value: u64) {
    with_sink(|sink, _| sink.scalar(scalar, value));
}

/// Check a wider producer quantity without saturating or inventing a measurement.
#[doc(hidden)]
pub fn record_scalar_wide(scalar: accounting::Scalar, value: u128) {
    with_sink(|sink, _| match u64::try_from(value) {
        Ok(value) => sink.scalar(scalar, value),
        Err(_) => sink.scalar_overflow(scalar),
    });
}

/// Record cumulative usage/refusal on the original graph without changing it.
#[doc(hidden)]
pub fn record_graph(graph: accounting::Graph) {
    with_sink(|sink, _| sink.graph(graph));
}

/// Record actual fixed native-account facts from one authenticated runner lifetime.
#[doc(hidden)]
pub fn record_native(
    runner_pid: u32,
    epoch: u64,
    observation: crate::protocol::account::Observation,
) {
    with_sink(|sink, _| sink.native(runner_pid, epoch, observation));
}

/// Latch missing or invalid required telemetry as incomplete evidence only.
#[doc(hidden)]
pub fn record_native_fault(runner_pid: u32, epoch: u64, cause: accounting::NativeFault) {
    with_sink(|sink, _| sink.native_fault(runner_pid, epoch, cause));
}

/// Observe one current linear transform as a miss and, on success, publication.
#[doc(hidden)]
pub fn executed<T, E>(kind: NodeKind, transform: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    record_node(kind, NodeEvent::ExecutedMiss);
    let value = transform()?;
    record_node(kind, NodeEvent::Published);
    Ok(value)
}

#[cfg(any(test, feature = "test-fixtures"))]
fn with_sink(observe: impl FnOnce(&dyn Sink, u64)) {
    let active = ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .map(|active| (Arc::clone(&active.sink), active.origin_ns))
    });
    if let Some((sink, origin_ns)) = active {
        observe(sink.as_ref(), origin_ns)
    }
}

#[cfg(not(any(test, feature = "test-fixtures")))]
#[inline(always)]
fn with_sink(_observe: impl FnOnce(&dyn Sink, u64)) {}

/// Read the process-shared monotonic clock without granting scheduling authority.
#[doc(hidden)]
pub fn monotonic_ns() -> Option<u64> {
    use rustix::time::{clock_gettime, ClockId};

    let value = clock_gettime(ClockId::Monotonic);
    let seconds = u64::try_from(value.tv_sec).ok()?;
    let nanoseconds = u64::try_from(value.tv_nsec).ok()?;
    seconds
        .checked_mul(1_000_000_000)
        .and_then(|base| base.checked_add(nanoseconds))
}
