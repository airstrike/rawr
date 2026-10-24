//! Scoped checked recorder used by tests.

use super::{Milestone, NodeEvent, NodeKind, ProcessKind, Scope, ScopeError, Sink};
use std::sync::{Arc, Mutex, MutexGuard};

pub(crate) const NODE_KINDS: [NodeKind; 11] = [
    NodeKind::Toolchain,
    NodeKind::Resolution,
    NodeKind::Api,
    NodeKind::Generic,
    NodeKind::Profile,
    NodeKind::Facade,
    NodeKind::Checked,
    NodeKind::ProfileProof,
    NodeKind::NativeProduct,
    NodeKind::Overlay,
    NodeKind::Runner,
];
pub(crate) const NODE_EVENTS: [NodeEvent; 4] = [
    NodeEvent::ValidatedHit,
    NodeEvent::ExecutedMiss,
    NodeEvent::RejectedHit,
    NodeEvent::Published,
];
pub(crate) const PROCESS_KINDS: [ProcessKind; 12] = [
    ProcessKind::RustupWhich,
    ProcessKind::RustcVersion,
    ProcessKind::RustdocVersion,
    ProcessKind::CargoVersion,
    ProcessKind::DriverVersion,
    ProcessKind::ComponentList,
    ProcessKind::TargetList,
    ProcessKind::CargoResolution,
    ProcessKind::PackageList,
    ProcessKind::MetadataProbe,
    ProcessKind::CargoRunner,
    ProcessKind::Runner,
];

pub(crate) const MILESTONES: [Milestone; 6] = [
    Milestone::RunnerSpawned,
    Milestone::Prepared,
    Milestone::Activated,
    Milestone::Admitted,
    Milestone::Started,
    Milestone::Visible,
];

pub mod milestone;
pub mod node;
pub mod process;

/// A fixed-order copy of all bounded evidence counters.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Snapshot {
    pub nodes: Box<[node::Count]>,
    pub processes: Box<[process::Count]>,
    pub milestones: Box<[milestone::Count]>,
    pub runner: Option<super::runner::Owned>,
    /// One fixed allocation keeps nested report construction off the call stack.
    pub accounting: Box<super::accounting::Evidence>,
}

/// Failure to create, scope, or read a checked recorder.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("metrics counter limit must be nonzero")]
    ZeroLimit,
    #[error(transparent)]
    Scope(#[from] ScopeError),
    #[error("metrics counter overflowed at {counter}")]
    Overflow { counter: Counter },
    #[error("a milestone did not retain a valid process-shared clock value")]
    Clock,
    #[error("{scopes} metrics scope(s) remain unjoined")]
    Unjoined { scopes: u64 },
    #[error("metrics scope accounting is imbalanced")]
    ScopeBalance,
    #[error("runner evidence is duplicated or invalid")]
    Runner,
}

/// Stable identity of one bounded counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Counter {
    Node(NodeKind, NodeEvent),
    Process(ProcessKind),
    Milestone(Milestone),
    Scope,
}

impl std::fmt::Display for Counter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Node(kind, event) => write!(formatter, "node {kind:?}/{event:?}"),
            Self::Process(kind) => write!(formatter, "process {kind:?}"),
            Self::Milestone(milestone) => write!(formatter, "milestone {milestone:?}"),
            Self::Scope => formatter.write_str("scope"),
        }
    }
}

/// Shareable evidence sink whose state is readable only through checked snapshots.
#[derive(Clone)]
pub struct Recorder {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<State>,
    limit: u64,
    origin_ns: u64,
}

struct State {
    nodes: [[u64; NODE_EVENTS.len()]; NODE_KINDS.len()],
    processes: [u64; PROCESS_KINDS.len()],
    milestones: [(u64, Option<u64>); MILESTONES.len()],
    scopes: u64,
    overflow: Option<Counter>,
    clock_failure: bool,
    scope_imbalance: bool,
    runner: Option<super::runner::Owned>,
    runner_ambiguous: bool,
    accounting: super::accounting::Evidence,
}

impl Recorder {
    /// Transform a positive per-counter bound into an empty recorder.
    pub fn transform(limit: u64) -> Result<Self, Error> {
        let origin_ns = super::monotonic_ns().ok_or(Error::Clock)?;
        Self::transform_at(limit, origin_ns)
    }

    /// Transform a checked limit and externally captured clock origin.
    #[doc(hidden)]
    pub fn transform_at(limit: u64, origin_ns: u64) -> Result<Self, Error> {
        if limit == 0 {
            return Err(Error::ZeroLimit);
        }
        Ok(Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    nodes: [[0; NODE_EVENTS.len()]; NODE_KINDS.len()],
                    processes: [0; PROCESS_KINDS.len()],
                    milestones: [(0, None); MILESTONES.len()],
                    scopes: 0,
                    overflow: None,
                    clock_failure: false,
                    scope_imbalance: false,
                    runner: None,
                    runner_ambiguous: false,
                    accounting: super::accounting::Evidence::default(),
                }),
                limit,
                origin_ns,
            }),
        })
    }

    /// Install this recorder until the returned opaque same-thread proof drops.
    pub fn scope(&self) -> Result<Scope, Error> {
        let sink: Arc<dyn Sink> = self.inner.clone();
        Scope::transform(sink).map_err(Error::from)
    }

    /// Copy every counter in canonical enum order, rejecting any overflow.
    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        let state = lock(&self.inner.state);
        if let Some(counter) = state.overflow {
            return Err(Error::Overflow { counter });
        }
        if state.clock_failure {
            return Err(Error::Clock);
        }
        if state.scope_imbalance {
            return Err(Error::ScopeBalance);
        }
        if state.scopes != 0 {
            return Err(Error::Unjoined {
                scopes: state.scopes,
            });
        }
        if state.runner_ambiguous || state.runner.as_ref().is_some_and(|runner| !runner.valid()) {
            return Err(Error::Runner);
        }
        let state = &*state;
        let nodes = NODE_KINDS
            .iter()
            .enumerate()
            .flat_map(|(kind_index, kind)| {
                NODE_EVENTS
                    .iter()
                    .enumerate()
                    .map(move |(event_index, event)| node::Count {
                        kind: *kind,
                        event: *event,
                        count: state.nodes[kind_index][event_index],
                    })
            })
            .collect();
        let processes = PROCESS_KINDS
            .iter()
            .enumerate()
            .map(|(index, kind)| process::Count {
                kind: *kind,
                count: state.processes[index],
            })
            .collect();
        let milestones = MILESTONES
            .iter()
            .enumerate()
            .map(|(index, milestone)| milestone::Count {
                milestone: *milestone,
                count: state.milestones[index].0,
                last_observed_ns: state.milestones[index].1.unwrap_or(0),
                last_elapsed_ns: state.milestones[index]
                    .1
                    .unwrap_or(self.inner.origin_ns)
                    .saturating_sub(self.inner.origin_ns),
            })
            .collect();
        Ok(Snapshot {
            nodes,
            processes,
            milestones,
            runner: state.runner.clone(),
            accounting: Box::new(state.accounting),
        })
    }
}

impl Sink for Inner {
    fn native(
        &self,
        runner_pid: u32,
        epoch: u64,
        observation: crate::protocol::account::Observation,
    ) {
        lock(&self.state)
            .accounting
            .record_native(runner_pid, epoch, observation, self.limit);
    }

    fn native_fault(&self, runner_pid: u32, epoch: u64, cause: super::accounting::NativeFault) {
        lock(&self.state)
            .accounting
            .record_native_fault(runner_pid, epoch, cause);
    }
    fn scalar_overflow(&self, scalar: super::accounting::Scalar) {
        lock(&self.state).accounting.scalar_overflow(scalar);
    }

    fn scalar(&self, scalar: super::accounting::Scalar, value: u64) {
        lock(&self.state)
            .accounting
            .record_scalar(scalar, value, self.limit);
    }

    fn graph(&self, graph: super::accounting::Graph) {
        lock(&self.state).accounting.record_graph(graph, self.limit);
    }

    fn node(&self, kind: NodeKind, event: NodeEvent) {
        let kind_index = node_index(kind);
        let event_index = event_index(event);
        let counter = Counter::Node(kind, event);
        let mut state = lock(&self.state);
        let current = state.nodes[kind_index][event_index];
        match increment(current, self.limit) {
            Some(next) => state.nodes[kind_index][event_index] = next,
            None => record_overflow(&mut state, counter),
        }
    }

    fn process(&self, kind: ProcessKind) {
        let process_index = process_index(kind);
        let counter = Counter::Process(kind);
        let mut state = lock(&self.state);
        let current = state.processes[process_index];
        match increment(current, self.limit) {
            Some(next) => state.processes[process_index] = next,
            None => record_overflow(&mut state, counter),
        }
    }

    fn milestone(&self, milestone: Milestone, observed_ns: Option<u64>) {
        let milestone_index = milestone_index(milestone);
        let counter = Counter::Milestone(milestone);
        let mut state = lock(&self.state);
        let current = state.milestones[milestone_index].0;
        match increment(current, self.limit) {
            Some(next) => state.milestones[milestone_index] = (next, observed_ns),
            None => record_overflow(&mut state, counter),
        }
        if observed_ns.is_none_or(|observed_ns| observed_ns < self.origin_ns) {
            state.clock_failure = true;
        }
    }

    fn runner_owned(&self, owned: &super::runner::Owned) {
        let mut state = lock(&self.state);
        if state.runner.replace(owned.clone()).is_some() {
            state.runner_ambiguous = true;
        }
    }

    fn scope_opened(&self) {
        let mut state = lock(&self.state);
        match increment(state.scopes, self.limit) {
            Some(next) => state.scopes = next,
            None => record_overflow(&mut state, Counter::Scope),
        }
    }

    fn scope_joined(&self) {
        let mut state = lock(&self.state);
        match state.scopes.checked_sub(1) {
            Some(next) => state.scopes = next,
            None => state.scope_imbalance = true,
        }
    }
}

impl Sink for Recorder {
    fn native(
        &self,
        runner_pid: u32,
        epoch: u64,
        observation: crate::protocol::account::Observation,
    ) {
        self.inner.native(runner_pid, epoch, observation);
    }

    fn native_fault(&self, runner_pid: u32, epoch: u64, cause: super::accounting::NativeFault) {
        self.inner.native_fault(runner_pid, epoch, cause);
    }
    fn scalar_overflow(&self, scalar: super::accounting::Scalar) {
        self.inner.scalar_overflow(scalar);
    }

    fn scalar(&self, scalar: super::accounting::Scalar, value: u64) {
        self.inner.scalar(scalar, value);
    }

    fn graph(&self, graph: super::accounting::Graph) {
        self.inner.graph(graph);
    }

    fn node(&self, kind: NodeKind, event: NodeEvent) {
        self.inner.node(kind, event);
    }

    fn process(&self, kind: ProcessKind) {
        self.inner.process(kind);
    }

    fn milestone(&self, milestone: Milestone, observed_ns: Option<u64>) {
        self.inner.milestone(milestone, observed_ns);
    }

    fn runner_owned(&self, owned: &super::runner::Owned) {
        self.inner.runner_owned(owned);
    }

    fn scope_opened(&self) {
        self.inner.scope_opened();
    }

    fn scope_joined(&self) {
        self.inner.scope_joined();
    }
}

fn increment(current: u64, limit: u64) -> Option<u64> {
    current.checked_add(1).filter(|next| *next <= limit)
}

fn record_overflow(state: &mut State, counter: Counter) {
    if state.overflow.is_none() {
        state.overflow = Some(counter);
    }
}

const fn node_index(kind: NodeKind) -> usize {
    match kind {
        NodeKind::Toolchain => 0,
        NodeKind::Resolution => 1,
        NodeKind::Api => 2,
        NodeKind::Generic => 3,
        NodeKind::Profile => 4,
        NodeKind::Facade => 5,
        NodeKind::Checked => 6,
        NodeKind::ProfileProof => 7,
        NodeKind::NativeProduct => 8,
        NodeKind::Overlay => 9,
        NodeKind::Runner => 10,
    }
}

const fn event_index(event: NodeEvent) -> usize {
    match event {
        NodeEvent::ValidatedHit => 0,
        NodeEvent::ExecutedMiss => 1,
        NodeEvent::RejectedHit => 2,
        NodeEvent::Published => 3,
    }
}

const fn process_index(kind: ProcessKind) -> usize {
    match kind {
        ProcessKind::RustupWhich => 0,
        ProcessKind::RustcVersion => 1,
        ProcessKind::RustdocVersion => 2,
        ProcessKind::CargoVersion => 3,
        ProcessKind::DriverVersion => 4,
        ProcessKind::ComponentList => 5,
        ProcessKind::TargetList => 6,
        ProcessKind::CargoResolution => 7,
        ProcessKind::PackageList => 8,
        ProcessKind::MetadataProbe => 9,
        ProcessKind::CargoRunner => 10,
        ProcessKind::Runner => 11,
    }
}

const fn milestone_index(milestone: Milestone) -> usize {
    match milestone {
        Milestone::RunnerSpawned => 0,
        Milestone::Prepared => 1,
        Milestone::Activated => 2,
        Milestone::Admitted => 3,
        Milestone::Started => 4,
        Milestone::Visible => 5,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Counter, Error, Recorder};
    use crate::metrics::{self, Milestone, NodeEvent, NodeKind, ProcessKind};

    #[test]
    fn snapshots_keep_fixed_accounting_storage_out_of_nested_report_stack_frames() {
        assert!(std::mem::size_of::<super::Snapshot>() <= 1024);
        let recorder = Recorder::transform_at(1, 1).expect("positive recorder bound");
        let snapshot = recorder.snapshot().expect("fixed accounting snapshots");
        let encoded = serde_json::to_vec(&snapshot).expect("snapshot serializes");
        let decoded: super::Snapshot = serde_json::from_slice(&encoded).expect("snapshot decodes");
        assert_eq!(decoded, snapshot);
        assert_eq!(
            *decoded.accounting,
            metrics::accounting::Evidence::default()
        );
    }

    #[test]
    fn scope_records_every_closed_evidence_family() {
        let recorder = Recorder::transform(2).expect("positive limit seals");
        {
            let _scope = recorder.scope().expect("scope installs");
            metrics::record_node(NodeKind::Api, NodeEvent::ExecutedMiss);
            metrics::record_process(ProcessKind::MetadataProbe);
            metrics::record_milestone(Milestone::Prepared);
        }
        let snapshot = recorder.snapshot().expect("bounded evidence snapshots");
        assert!(snapshot.nodes.iter().any(|row| {
            row.kind == NodeKind::Api && row.event == NodeEvent::ExecutedMiss && row.count == 1
        }));
        assert!(snapshot
            .processes
            .iter()
            .any(|row| row.kind == ProcessKind::MetadataProbe && row.count == 1));
        assert!(snapshot
            .milestones
            .iter()
            .any(|row| row.milestone == Milestone::Prepared && row.count == 1));
    }

    #[test]
    fn nested_scope_rejects_without_replacing_the_outer_sink() {
        let recorder = Recorder::transform(1).expect("positive limit seals");
        let scope = recorder.scope().expect("outer scope installs");
        assert!(matches!(recorder.scope(), Err(Error::Scope(_))));
        metrics::record_process(ProcessKind::Runner);
        drop(scope);
        assert_eq!(
            recorder
                .snapshot()
                .expect("outer evidence remains")
                .processes
                .iter()
                .find(|row| row.kind == ProcessKind::Runner)
                .map(|row| row.count),
            Some(1)
        );
    }

    #[test]
    fn overflow_fails_the_evidence_snapshot() {
        let recorder = Recorder::transform(1).expect("positive limit seals");
        let _scope = recorder.scope().expect("scope installs");
        metrics::record_process(ProcessKind::CargoRunner);
        metrics::record_process(ProcessKind::CargoRunner);
        assert_eq!(
            recorder.snapshot(),
            Err(Error::Overflow {
                counter: Counter::Process(ProcessKind::CargoRunner),
            })
        );
    }

    #[test]
    fn snapshot_rejects_an_unjoined_scope() {
        let recorder = Recorder::transform(2).expect("positive limit seals");
        let scope = recorder.scope().expect("scope installs");
        assert_eq!(recorder.snapshot(), Err(Error::Unjoined { scopes: 1 }));
        drop(scope);
        assert!(recorder.snapshot().is_ok());
    }

    #[test]
    fn panic_drop_cleans_the_scope() {
        let recorder = Recorder::transform(1).expect("positive limit seals");
        let _ = std::panic::catch_unwind(|| {
            let _scope = recorder.scope().expect("scope installs");
            panic!("controlled panic");
        });
        let _scope = recorder.scope().expect("panic drop removed the old scope");
    }

    #[test]
    fn no_scope_uses_noop_without_mutating_a_recorder() {
        metrics::record_node(NodeKind::Toolchain, NodeEvent::Published);
        metrics::record_process(ProcessKind::Runner);
        metrics::record_milestone(Milestone::Visible);
        let recorder = Recorder::transform(1).expect("positive limit seals");
        let snapshot = recorder.snapshot().expect("empty evidence snapshots");
        assert!(snapshot.nodes.iter().all(|row| row.count == 0));
        assert!(snapshot.processes.iter().all(|row| row.count == 0));
        assert!(snapshot.milestones.iter().all(|row| row.count == 0));
    }
}
