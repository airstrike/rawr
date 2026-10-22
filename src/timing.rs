//! Read-only timing observations for host-requested compiler and runtime work.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ATTEMPT_LIMIT: usize = 256;

/// One semantic operation measured by an explicit host observation session.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Phase {
    /// Source parsing and Rawr item discovery.
    ParseItems,
    /// Item, type, method, and application-root resolution.
    Resolve,
    /// Lowering one resolved program into its executable representation.
    Lower,
    /// Validation and normalization of the complete application contract.
    ApplicationContract,
    /// Compatibility, binding, owner preparation, and source revalidation for one reload candidate.
    CandidateValidation,
    /// Structural latest-revision host and input publication admission.
    Installation,
    /// Evaluation of one Rawr application view callback.
    ViewEvaluation,
    /// Lowering one durable widget descriptor into a transient iced element.
    DescriptorLowering,
    /// One authenticated scripted-widget lifecycle hook dispatch.
    ///
    /// This phase begins after the binding has proved the hook exists and its
    /// hook-specific input preconditions hold. It includes the complete
    /// authority invocation: retained-item and arity verification, dispatch
    /// preparation, evaluator-state setup, and evaluator execution. Omitted
    /// hooks and earlier binding validation are not attempts. Adapter carrier,
    /// structural, and commit validation after a successful authority return
    /// remains outside the measured dispatch.
    ScriptedHookDispatch,
    /// Post-admission release of previous host state and its effect handles.
    ///
    /// This measures synchronous retirement initiation and host-owned drops,
    /// not later asynchronous disposal performed by iced's native trackers.
    EffectRetirement,
}

/// The semantic result of one measured operation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Outcome {
    /// The operation returned successfully.
    Success,
    /// The operation returned an error or resumed an observed unwind.
    Failure,
    /// The operation was truthfully superseded before it could become current.
    Obsolete,
}

/// One ordered timing attempt retained by a host observation session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attempt {
    sequence: u64,
    phase: Phase,
    outcome: Outcome,
    duration: Duration,
}

/// Honest aggregation of every attempt for one semantic phase.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Aggregate {
    count: u64,
    latest: Option<Duration>,
    total: Duration,
}

/// An immutable read-only timing snapshot.
///
/// It contains bounded recent attempt details, exact lifetime aggregates, and
/// an explicit count of attempts omitted from the bounded history.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    attempts: Box<[Attempt]>,
    phases: BTreeMap<Phase, Aggregate>,
    outcomes: BTreeMap<(Phase, Outcome), Aggregate>,
    omitted: u64,
}

/// A host-owned timing session shared only beside semantic proof objects.
///
/// This type is an inter-crate host seam. Application APIs expose only cloned
/// [`Snapshot`] values; scripts and runtime values never receive a session.
#[doc(hidden)]
#[derive(Clone)]
pub struct Session {
    inner: Arc<Inner>,
}

struct Inner {
    clock: Box<dyn Clock>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    next_sequence: u64,
    sequence_exhausted: bool,
    attempts: BTreeMap<u64, Attempt>,
    phases: BTreeMap<Phase, Aggregate>,
    outcomes: BTreeMap<(Phase, Outcome), Aggregate>,
    omitted: u64,
}

trait Clock: Send + Sync {
    fn now(&self) -> Duration;
}

struct Monotonic {
    origin: Instant,
}

impl Attempt {
    /// Return the measured semantic phase.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Return whether the operation succeeded, failed, or became obsolete.
    #[must_use]
    pub const fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// Return the elapsed monotonic duration for this attempt.
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.duration
    }
}

impl Aggregate {
    /// Return the number of attempts, saturated at [`u64::MAX`].
    #[must_use]
    pub const fn count(&self) -> u64 {
        self.count
    }

    /// Return the most recently completed duration, if this phase ran.
    #[must_use]
    pub const fn latest(&self) -> Option<Duration> {
        self.latest
    }

    /// Return the saturated sum of all durations for this phase.
    #[must_use]
    pub const fn total(&self) -> Duration {
        self.total
    }

    fn include(&mut self, duration: Duration) {
        self.count = self.count.saturating_add(1);
        self.latest = Some(duration);
        self.total = self.total.saturating_add(duration);
    }
}

impl Snapshot {
    /// Return retained bounded recent attempts in the order their operations began.
    #[must_use]
    pub fn attempts(&self) -> &[Attempt] {
        &self.attempts
    }

    /// Aggregate all attempts for one semantic phase.
    #[must_use]
    pub fn aggregate(&self, phase: Phase) -> Aggregate {
        self.phases.get(&phase).copied().unwrap_or_default()
    }

    /// Aggregate every lifetime attempt for one semantic phase and outcome.
    #[must_use]
    pub fn aggregate_outcome(&self, phase: Phase, outcome: Outcome) -> Aggregate {
        self.outcomes
            .get(&(phase, outcome))
            .copied()
            .unwrap_or_default()
    }

    /// Return the saturated count of older attempts omitted from recent history.
    #[must_use]
    pub const fn omitted_attempts(&self) -> u64 {
        self.omitted
    }
}

impl Session {
    /// Start one production observation session with a monotonic clock.
    #[doc(hidden)]
    #[must_use]
    pub fn start() -> Self {
        Self::with_clock(Box::new(Monotonic {
            origin: Instant::now(),
        }))
    }

    /// Measure one infallible host operation as a successful phase attempt.
    #[doc(hidden)]
    pub fn observe<T>(&self, phase: Phase, operation: impl FnOnce() -> T) -> T {
        let sequence = self.reserve_sequence();
        let started = self.inner.clock.now();
        let value = operation();
        let duration = self.inner.clock.now().saturating_sub(started);
        self.record(sequence, phase, Outcome::Success, duration);
        value
    }

    /// Measure one fallible host operation and retain its returned outcome.
    #[doc(hidden)]
    pub fn observe_result<T, E>(
        &self,
        phase: Phase,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let sequence = self.reserve_sequence();
        let started = self.inner.clock.now();
        let result = operation();
        let duration = self.inner.clock.now().saturating_sub(started);
        let outcome = if result.is_ok() {
            Outcome::Success
        } else {
            Outcome::Failure
        };
        self.record(sequence, phase, outcome, duration);
        result
    }

    /// Measure one host operation whose semantic outcome is part of its result.
    #[doc(hidden)]
    pub fn observe_outcome<T>(&self, phase: Phase, operation: impl FnOnce() -> (T, Outcome)) -> T {
        let sequence = self.reserve_sequence();
        let started = self.inner.clock.now();
        let (value, outcome) = operation();
        let duration = self.inner.clock.now().saturating_sub(started);
        self.record(sequence, phase, outcome, duration);
        value
    }

    /// Measure one semantic-outcome operation and record any resumed unwind as failure.
    #[doc(hidden)]
    pub fn observe_outcome_unwind<T>(
        &self,
        phase: Phase,
        operation: impl FnOnce() -> (T, Outcome),
    ) -> T {
        let sequence = self.reserve_sequence();
        let started = self.inner.clock.now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
        let duration = self.inner.clock.now().saturating_sub(started);
        match result {
            Ok((value, outcome)) => {
                self.record(sequence, phase, outcome, duration);
                value
            }
            Err(payload) => {
                self.record(sequence, phase, Outcome::Failure, duration);
                std::panic::resume_unwind(payload)
            }
        }
    }

    /// Measure one operation whose panic must be recorded before it resumes unwinding.
    #[doc(hidden)]
    pub fn observe_unwind<T>(&self, phase: Phase, operation: impl FnOnce() -> T) -> T {
        let sequence = self.reserve_sequence();
        let started = self.inner.clock.now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
        let duration = self.inner.clock.now().saturating_sub(started);
        let outcome = if result.is_ok() {
            Outcome::Success
        } else {
            Outcome::Failure
        };
        self.record(sequence, phase, outcome, duration);
        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Clone the immutable observations recorded so far.
    #[doc(hidden)]
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let attempts = state.attempts.values().cloned().collect::<Vec<_>>();
        Snapshot {
            attempts: attempts.into_boxed_slice(),
            phases: state.phases.clone(),
            outcomes: state.outcomes.clone(),
            omitted: state.omitted,
        }
    }

    fn with_clock(clock: Box<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(Inner {
                clock,
                state: Mutex::new(State::default()),
            }),
        }
    }

    fn reserve_sequence(&self) -> Option<u64> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.sequence_exhausted {
            return None;
        }
        let sequence = state.next_sequence;
        match sequence.checked_add(1) {
            Some(next) => state.next_sequence = next,
            None => state.sequence_exhausted = true,
        }
        Some(sequence)
    }

    fn record(&self, sequence: Option<u64>, phase: Phase, outcome: Outcome, duration: Duration) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.phases.entry(phase).or_default().include(duration);
        state
            .outcomes
            .entry((phase, outcome))
            .or_default()
            .include(duration);
        let Some(sequence) = sequence else {
            state.omitted = state.omitted.saturating_add(1);
            return;
        };
        if state.attempts.len() == ATTEMPT_LIMIT {
            let oldest = state
                .attempts
                .first_key_value()
                .map(|(&sequence, _)| sequence);
            let Some(oldest) = oldest else {
                state.omitted = state.omitted.saturating_add(1);
                return;
            };
            if sequence <= oldest {
                state.omitted = state.omitted.saturating_add(1);
                return;
            }
            state.attempts.remove(&oldest);
            state.omitted = state.omitted.saturating_add(1);
        }
        state.attempts.insert(
            sequence,
            Attempt {
                sequence,
                phase,
                outcome,
                duration,
            },
        );
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::start()
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Session")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl Clock for Monotonic {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

pub(crate) fn observe_optional<T, E>(
    session: Option<&Session>,
    phase: Phase,
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    match session {
        Some(session) => session.observe_result(phase, operation),
        None => operation(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Barrier};

    use super::{Aggregate, Clock, Outcome, Phase, Session, ATTEMPT_LIMIT};
    use std::time::Duration;

    struct StepClock {
        nanos: AtomicU64,
        step: u64,
    }

    impl Clock for StepClock {
        fn now(&self) -> Duration {
            Duration::from_nanos(self.nanos.fetch_add(self.step, Ordering::Relaxed))
        }
    }

    #[test]
    fn injected_monotonic_clock_makes_attempt_order_and_aggregation_exact() {
        let session = Session::with_clock(Box::new(StepClock {
            nanos: AtomicU64::new(0),
            step: 7,
        }));

        let _: Result<(), ()> = session.observe_result(Phase::ParseItems, || Ok(()));
        let _: Result<(), ()> = session.observe_result(Phase::Resolve, || Err(()));
        session.observe(Phase::ParseItems, || ());

        let snapshot = session.snapshot();
        assert_eq!(snapshot.attempts().len(), 3);
        assert_eq!(snapshot.attempts()[0].phase(), Phase::ParseItems);
        assert_eq!(snapshot.attempts()[0].outcome(), Outcome::Success);
        assert_eq!(snapshot.attempts()[1].phase(), Phase::Resolve);
        assert_eq!(snapshot.attempts()[1].outcome(), Outcome::Failure);
        assert_eq!(snapshot.attempts()[2].phase(), Phase::ParseItems);

        let parse = snapshot.aggregate(Phase::ParseItems);
        assert_eq!(parse.count(), 2);
        assert_eq!(parse.latest(), Some(Duration::from_nanos(7)));
        assert_eq!(parse.total(), Duration::from_nanos(14));
    }

    #[test]
    fn aggregation_saturates_count_and_duration() {
        let mut aggregate = Aggregate {
            count: u64::MAX,
            latest: Some(Duration::MAX),
            total: Duration::MAX,
        };
        aggregate.include(Duration::from_nanos(1));

        assert_eq!(aggregate.count(), u64::MAX);
        assert_eq!(aggregate.latest(), Some(Duration::from_nanos(1)));
        assert_eq!(aggregate.total(), Duration::MAX);
    }

    #[test]
    fn reload_phases_record_returned_and_unwinding_failures_in_order() {
        let session = Session::with_clock(Box::new(StepClock {
            nanos: AtomicU64::new(0),
            step: 11,
        }));

        let _: Result<(), ()> = session.observe_result(Phase::CandidateValidation, || Err(()));
        let _: Result<(), ()> = session.observe_result(Phase::Installation, || Ok(()));
        let marker = Arc::new(());
        let panic_marker = Arc::clone(&marker);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.observe_unwind(Phase::EffectRetirement, || {
                std::panic::panic_any(panic_marker)
            });
        }));
        let returned = panic
            .expect_err("retirement must resume the original panic")
            .downcast::<Arc<()>>()
            .expect("panic payload must preserve its concrete type");
        assert!(Arc::ptr_eq(&marker, returned.as_ref()));

        let snapshot = session.snapshot();
        assert_eq!(snapshot.attempts().len(), 3);
        assert_eq!(snapshot.attempts()[0].phase(), Phase::CandidateValidation);
        assert_eq!(snapshot.attempts()[0].outcome(), Outcome::Failure);
        assert_eq!(snapshot.attempts()[1].phase(), Phase::Installation);
        assert_eq!(snapshot.attempts()[1].outcome(), Outcome::Success);
        assert_eq!(snapshot.attempts()[2].phase(), Phase::EffectRetirement);
        assert_eq!(snapshot.attempts()[2].outcome(), Outcome::Failure);
        assert!(snapshot
            .attempts()
            .iter()
            .all(|attempt| attempt.duration() == Duration::from_nanos(11)));
    }

    #[test]
    fn recent_history_is_bounded_without_losing_lifetime_aggregates() {
        let session = Session::with_clock(Box::new(StepClock {
            nanos: AtomicU64::new(0),
            step: 3,
        }));
        let total = ATTEMPT_LIMIT + 37;

        for index in 0..total {
            let _result: Result<(), ()> = session.observe_result(Phase::Installation, || {
                if index % 2 == 0 {
                    Ok(())
                } else {
                    Err(())
                }
            });
        }

        let snapshot = session.snapshot();
        assert_eq!(snapshot.attempts().len(), ATTEMPT_LIMIT);
        assert_eq!(snapshot.omitted_attempts(), 37);
        assert_eq!(
            snapshot.aggregate(Phase::Installation).count(),
            total as u64
        );
        assert_eq!(
            snapshot
                .aggregate_outcome(Phase::Installation, Outcome::Success)
                .count(),
            total.div_ceil(2) as u64
        );
        assert_eq!(
            snapshot
                .aggregate_outcome(Phase::Installation, Outcome::Failure)
                .count(),
            (total / 2) as u64
        );
        assert_eq!(
            snapshot.aggregate(Phase::Installation).total(),
            Duration::from_nanos((total * 3) as u64)
        );
        assert!(snapshot
            .attempts()
            .windows(2)
            .all(|attempts| attempts[0].sequence < attempts[1].sequence));
    }

    #[test]
    fn concurrent_slow_old_completion_cannot_evict_newer_started_history() {
        let session = Session::with_clock(Box::new(StepClock {
            nanos: AtomicU64::new(0),
            step: 5,
        }));
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let old_session = session.clone();
        let old_entered = Arc::clone(&entered);
        let old_release = Arc::clone(&release);
        let old = std::thread::spawn(move || {
            old_session.observe(Phase::CandidateValidation, || {
                old_entered.wait();
                old_release.wait();
            });
        });
        entered.wait();

        for _ in 0..ATTEMPT_LIMIT {
            session.observe(Phase::Installation, || ());
        }
        release.wait();
        old.join()
            .expect("the controlled old observation must finish");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.attempts().len(), ATTEMPT_LIMIT);
        assert_eq!(snapshot.attempts()[0].sequence, 1);
        assert_eq!(snapshot.attempts()[ATTEMPT_LIMIT - 1].sequence, 256);
        assert_eq!(snapshot.omitted_attempts(), 1);
        assert_eq!(snapshot.aggregate(Phase::CandidateValidation).count(), 1);
    }

    #[test]
    fn exhausted_sequence_is_issued_once_then_only_aggregated() {
        let session = Session::with_clock(Box::new(StepClock {
            nanos: AtomicU64::new(0),
            step: 13,
        }));
        {
            let mut state = session
                .inner
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.next_sequence = u64::MAX;
            state.sequence_exhausted = false;
        }

        session.observe(Phase::Installation, || ());
        session.observe(Phase::Installation, || ());

        let snapshot = session.snapshot();
        assert_eq!(snapshot.attempts().len(), 1);
        assert_eq!(snapshot.attempts()[0].sequence, u64::MAX);
        assert_eq!(snapshot.omitted_attempts(), 1);
        assert_eq!(snapshot.aggregate(Phase::Installation).count(), 2);
    }

    #[test]
    fn obsolete_outcome_is_aggregated_separately_from_failure() {
        let session = Session::with_clock(Box::new(StepClock {
            nanos: AtomicU64::new(0),
            step: 17,
        }));

        let value = session.observe_outcome(Phase::Installation, || (42, Outcome::Obsolete));

        assert_eq!(value, 42);
        let snapshot = session.snapshot();
        assert_eq!(snapshot.attempts()[0].outcome(), Outcome::Obsolete);
        assert_eq!(
            snapshot
                .aggregate_outcome(Phase::Installation, Outcome::Obsolete)
                .count(),
            1
        );
        assert_eq!(
            snapshot
                .aggregate_outcome(Phase::Installation, Outcome::Failure)
                .count(),
            0
        );
    }

    #[test]
    fn outcome_observer_records_panics_and_resumes_original_payload() {
        let session = Session::with_clock(Box::new(StepClock {
            nanos: AtomicU64::new(0),
            step: 19,
        }));
        let marker = Arc::new(());
        let panic_marker = Arc::clone(&marker);

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.observe_outcome_unwind::<()>(Phase::Installation, || {
                std::panic::panic_any(panic_marker)
            });
        }));
        let returned = panic
            .expect_err("outcome observation must resume the original panic")
            .downcast::<Arc<()>>()
            .expect("panic payload must preserve its concrete type");
        assert!(Arc::ptr_eq(&marker, returned.as_ref()));

        let snapshot = session.snapshot();
        assert_eq!(snapshot.attempts().len(), 1);
        assert_eq!(snapshot.attempts()[0].phase(), Phase::Installation);
        assert_eq!(snapshot.attempts()[0].outcome(), Outcome::Failure);
        assert_eq!(snapshot.attempts()[0].duration(), Duration::from_nanos(19));
        assert_eq!(snapshot.aggregate(Phase::Installation).count(), 1);
        assert_eq!(
            snapshot
                .aggregate_outcome(Phase::Installation, Outcome::Failure)
                .count(),
            1
        );
    }
}
