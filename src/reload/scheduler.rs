//! I/O-free debounce and latest-revision admission shared by reload transports.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub mod publication;

/// One debounce and latest-revision admission boundary, owned by the one
/// thread that observes events and admits work.
///
/// An observation revokes older work at once. Candidate preparation happens
/// outside this gate; final admission runs one direct current-slot
/// replacement between two observations.
pub struct Gate {
    stamp: Arc<Stamp>,
    scheduler: Scheduler,
}

/// A compile request carrying authority for one exact observed revision.
#[must_use = "a reload request must be completed or dropped during shutdown"]
pub struct Request {
    stamp: Arc<Stamp>,
    revision: Revision,
}

/// One watcher-registration checkpoint that can authorize exactly one
/// non-blocking current-slot admission while no newer event has been observed.
#[must_use = "a watcher checkpoint must be admitted or dropped"]
pub struct Checkpoint {
    stamp: Arc<Stamp>,
    sequence: u64,
}

/// A transformed value retaining one exact request revision.
#[must_use = "a reload candidate must pass through final admission"]
pub struct Candidate<T> {
    stamp: Arc<Stamp>,
    revision: Revision,
    value: T,
}

/// The final latest-revision decision without in-gate value destruction.
#[must_use = "only a current candidate may affect live state"]
#[derive(Debug, Eq, PartialEq)]
pub enum Admission<T, U> {
    /// This current revision completed the non-dropping slot admission.
    Current(U),
    /// A newer event revoked this value; its owner must retire it outside the gate.
    Obsolete(T),
}

/// Failure while observing or scheduling one reload revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The session has entered its terminal state and cannot issue more work.
    Suspended,
    /// The monotonic request sequence cannot issue another revision.
    RevisionExhausted,
    /// The debounce deadline cannot be represented after the observed time.
    DeadlineOverflow,
}

struct Scheduler {
    debounce: Duration,
    next: u64,
    latest: Option<Revision>,
    pending: Option<Pending>,
    exhausted: bool,
    suspended: bool,
}

/// One unforgeable live-session identity.
struct Stamp;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Pending {
    revision: Revision,
    deadline: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Revision(u64);

impl Gate {
    /// Create a gate with one fixed debounce duration.
    #[must_use]
    pub fn new(debounce: Duration) -> Self {
        Self {
            stamp: Arc::new(Stamp),
            scheduler: Scheduler::new(debounce),
        }
    }

    /// Observe one relevant event and revoke older work at once.
    pub fn observe(&mut self, now: Instant) -> Result<(), Error> {
        self.scheduler.observe(now).map(|_| ())
    }

    /// Return the newest queued revision's debounce deadline.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.scheduler.deadline()
    }

    /// Start the newest queued request after its debounce deadline.
    #[must_use]
    pub fn start(&mut self, now: Instant) -> Option<Request> {
        self.scheduler.start(now).map(|revision| Request {
            stamp: Arc::clone(&self.stamp),
            revision,
        })
    }

    /// Observe and start one request in one step. Nonzero debounce still
    /// returns `None` and leaves the observed request queued for its normal
    /// deadline.
    pub fn observe_and_start(&mut self, now: Instant) -> Result<Option<Request>, Error> {
        self.scheduler.observe(now)?;
        Ok(self.start(now))
    }

    /// Capture the event sequence after watcher registration and before
    /// preparation or process work.
    pub fn checkpoint(&self) -> Result<Checkpoint, Error> {
        if self.scheduler.suspended {
            return Err(Error::Suspended);
        }
        if self.scheduler.exhausted {
            return Err(Error::RevisionExhausted);
        }
        Ok(Checkpoint {
            stamp: Arc::clone(&self.stamp),
            sequence: self.scheduler.next,
        })
    }

    /// Install one already-prepared value only if no watcher event was
    /// observed after its checkpoint.
    ///
    /// `admit` runs before the gate can observe anything else. It performs
    /// only the direct in-memory slot transition.
    #[must_use = "the checkpoint admission result must be handled"]
    pub fn admit_checkpoint<T, U>(
        &self,
        checkpoint: Checkpoint,
        value: T,
        admit: impl FnOnce(T) -> U,
    ) -> Admission<T, U> {
        let Checkpoint { stamp, sequence } = checkpoint;
        if !Arc::ptr_eq(&self.stamp, &stamp)
            || self.scheduler.suspended
            || self.scheduler.exhausted
            || self.scheduler.next != sequence
        {
            return Admission::Obsolete(value);
        }
        Admission::Current(admit(value))
    }

    /// Inspect the original request without consuming it or refreshing its revision.
    /// Final installation must still pass through the normal admission gate.
    #[must_use]
    pub fn request_is_current(&self, request: &Request) -> bool {
        self.is_current(&request.stamp, request.revision)
    }

    /// Admit one already-prepared value against its exact request revision.
    ///
    /// `admit` runs before the gate can observe anything else. It may
    /// perform bounded admission validation and a commit handshake, but must
    /// not build or transform.
    #[must_use = "the current admission result must be handled"]
    pub fn admit_current<T, U>(
        &self,
        candidate: Candidate<T>,
        admit: impl FnOnce(T) -> U,
    ) -> Admission<T, U> {
        let Candidate {
            stamp,
            revision,
            value,
        } = candidate;
        if !self.is_current(&stamp, revision) {
            return Admission::Obsolete(value);
        }
        Admission::Current(admit(value))
    }

    fn is_current(&self, stamp: &Arc<Stamp>, revision: Revision) -> bool {
        Arc::ptr_eq(&self.stamp, stamp) && self.scheduler.is_current(revision)
    }

    /// Revoke all work after an unrecoverable transport failure.
    pub fn suspend(&mut self) {
        self.scheduler.suspend();
    }
}

impl Request {
    /// Pair this exact request with its finished compilation result.
    #[must_use = "the completed candidate retains revision authority"]
    pub fn complete<T>(self, value: T) -> Candidate<T> {
        Candidate {
            stamp: self.stamp,
            revision: self.revision,
            value,
        }
    }
}

impl<T> Candidate<T> {
    /// Borrow the prepared value without losing final revision authority.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Transform the value while retaining its exact request revision.
    #[must_use = "the transformed candidate retains revision authority"]
    pub fn map<U>(self, transform: impl FnOnce(T) -> U) -> Candidate<U> {
        Candidate {
            stamp: self.stamp,
            revision: self.revision,
            value: transform(self.value),
        }
    }

    /// Transform the value while retaining its exact request revision.
    pub fn try_map<U, E>(
        self,
        transform: impl FnOnce(T) -> Result<U, E>,
    ) -> Result<Candidate<U>, E> {
        transform(self.value).map(|value| Candidate {
            stamp: self.stamp,
            revision: self.revision,
            value,
        })
    }

    /// Partition a transformed value while retaining the exact revision on
    /// either branch.
    #[must_use = "both result branches retain revision authority"]
    pub fn map_result<U, E>(
        self,
        transform: impl FnOnce(T) -> Result<U, E>,
    ) -> Result<Candidate<U>, Candidate<E>> {
        match transform(self.value) {
            Ok(value) => Ok(Candidate {
                stamp: self.stamp,
                revision: self.revision,
                value,
            }),
            Err(value) => Err(Candidate {
                stamp: self.stamp,
                revision: self.revision,
                value,
            }),
        }
    }
}

impl<T, U> Admission<T, U> {
    /// Borrow the slot-admission result only when the revision was current.
    #[must_use]
    pub const fn current(&self) -> Option<&U> {
        match self {
            Self::Current(value) => Some(value),
            Self::Obsolete(_) => None,
        }
    }

    /// Borrow the preserved value only when a newer revision revoked it.
    #[must_use]
    pub const fn obsolete(&self) -> Option<&T> {
        match self {
            Self::Current(_) => None,
            Self::Obsolete(value) => Some(value),
        }
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Request(<private>)")
    }
}

impl fmt::Debug for Checkpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Checkpoint(<private>)")
    }
}

impl PartialEq for Request {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.stamp, &other.stamp) && self.revision == other.revision
    }
}

impl Eq for Request {}

impl<T> fmt::Debug for Candidate<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Candidate(<private>)")
    }
}

impl<T: PartialEq> PartialEq for Candidate<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.stamp, &other.stamp)
            && self.revision == other.revision
            && self.value == other.value
    }
}

impl<T: Eq> Eq for Candidate<T> {}

impl Scheduler {
    const fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            next: 0,
            latest: None,
            pending: None,
            exhausted: false,
            suspended: false,
        }
    }

    fn observe(&mut self, now: Instant) -> Result<Revision, Error> {
        if self.suspended {
            return Err(Error::Suspended);
        }
        if self.exhausted {
            return Err(Error::RevisionExhausted);
        }
        let Some(next) = self.next.checked_add(1) else {
            self.exhausted = true;
            self.latest = None;
            self.pending = None;
            return Err(Error::RevisionExhausted);
        };
        let revision = Revision(next);
        self.next = next;
        self.latest = Some(revision);
        let Some(deadline) = now.checked_add(self.debounce) else {
            self.pending = None;
            return Err(Error::DeadlineOverflow);
        };
        self.pending = Some(Pending { revision, deadline });
        Ok(revision)
    }

    fn deadline(&self) -> Option<Instant> {
        self.pending.map(|pending| pending.deadline)
    }

    fn start(&mut self, now: Instant) -> Option<Revision> {
        if self.suspended {
            return None;
        }
        let pending = self.pending?;
        if now < pending.deadline {
            return None;
        }
        self.pending = None;
        (self.latest == Some(pending.revision)).then_some(pending.revision)
    }

    fn is_current(&self, revision: Revision) -> bool {
        !self.suspended && !self.exhausted && self.latest == Some(revision)
    }

    fn suspend(&mut self) {
        self.suspended = true;
        self.latest = None;
        self.pending = None;
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Suspended => formatter.write_str("reload session is suspended"),
            Self::RevisionExhausted => formatter.write_str("reload request sequence is exhausted"),
            Self::DeadlineOverflow => {
                formatter.write_str("reload debounce deadline is outside the clock range")
            }
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::Gate;
    use std::time::{Duration, Instant};

    #[test]
    fn borrowed_request_current_check_preserves_the_original_revision() {
        let mut gate = Gate::new(Duration::ZERO);
        let foreign = Gate::new(Duration::ZERO);
        let request = gate.observe_and_start(Instant::now()).unwrap().unwrap();
        assert!(gate.request_is_current(&request));
        assert!(gate.request_is_current(&request));
        assert!(!foreign.request_is_current(&request));
        assert!(gate.start(Instant::now()).is_none());
        gate.observe(Instant::now()).unwrap();
        assert!(!gate.request_is_current(&request));
        assert!(matches!(
            gate.admit_current(request.complete(()), |()| ()),
            super::Admission::Obsolete(())
        ));
        let current = gate.start(Instant::now()).unwrap();
        assert!(gate.request_is_current(&current));
        gate.suspend();
        assert!(!gate.request_is_current(&current));
    }

    #[test]
    fn observe_and_start_preserves_revision_and_debounce() {
        let now = Instant::now();
        let mut gate = Gate::new(Duration::ZERO);
        let request = gate.observe_and_start(now).unwrap().unwrap();
        assert!(gate.start(now).is_none());
        gate.observe(now + Duration::from_millis(1)).unwrap();
        assert!(!gate.request_is_current(&request));
        assert!(gate.start(now + Duration::from_millis(1)).is_some());

        let mut gate = Gate::new(Duration::from_millis(75));
        assert!(gate.observe_and_start(now).unwrap().is_none());
        assert_eq!(gate.deadline(), Some(now + Duration::from_millis(75)));
        assert!(gate.start(now).is_none());
        assert!(gate.start(now + Duration::from_millis(75)).is_some());
    }

    #[test]
    fn observe_and_start_preserves_terminal_scheduler_errors() {
        let now = Instant::now();
        let mut gate = Gate::new(Duration::ZERO);
        gate.suspend();
        assert!(matches!(
            gate.observe_and_start(now),
            Err(super::Error::Suspended)
        ));
        let mut gate = Gate::new(Duration::ZERO);
        gate.scheduler.next = u64::MAX;
        assert!(matches!(
            gate.observe_and_start(now),
            Err(super::Error::RevisionExhausted)
        ));
        assert!(gate.start(now).is_none());
    }

    #[test]
    fn event_during_preparation_revokes_the_candidate_before_commit() {
        let now = Instant::now();
        let mut gate = Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let candidate = gate
            .start(now)
            .unwrap()
            .complete("compiled")
            .try_map(|value| Ok::<_, ()>((value, "prepared")))
            .unwrap();

        gate.observe(now).unwrap();

        assert!(!gate.scheduler.is_current(candidate.revision));
        assert_eq!(candidate.value(), &("compiled", "prepared"));
    }

    #[test]
    fn transformations_retain_the_exact_current_revision() {
        let now = Instant::now();
        let mut gate = Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let candidate = gate.start(now).unwrap().complete(7).map(|value| value + 1);

        assert!(gate.scheduler.is_current(candidate.revision));
        assert_eq!(candidate.value(), &8);
    }

    #[test]
    fn suspension_is_sticky_and_rejects_post_stop_observation() {
        let now = Instant::now();
        let mut gate = Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let candidate = gate.start(now).unwrap().complete("running");

        gate.suspend();

        assert!(matches!(gate.observe(now), Err(super::Error::Suspended)));
        assert!(gate.start(now).is_none());
        assert!(matches!(
            gate.admit_current(candidate, |_| panic!("a suspended gate admits nothing")),
            super::Admission::Obsolete("running")
        ));
    }

    #[test]
    fn watcher_checkpoint_admits_only_without_an_intervening_event() {
        let now = Instant::now();
        let mut gate = Gate::new(Duration::ZERO);
        let current = gate.checkpoint().unwrap();
        assert_eq!(
            gate.admit_checkpoint(current, "current", |value| (value, "installed")),
            super::Admission::Current(("current", "installed"))
        );

        let revoked = gate.checkpoint().unwrap();
        gate.observe(now).unwrap();
        assert_eq!(
            gate.admit_checkpoint(revoked, "obsolete", |value| (value, "installed")),
            super::Admission::Obsolete("obsolete")
        );
    }

    #[test]
    fn watcher_checkpoint_is_bound_to_its_gate() {
        let origin = Gate::new(Duration::ZERO);
        let foreign = Gate::new(Duration::ZERO);
        let checkpoint = origin.checkpoint().unwrap();
        assert_eq!(
            foreign.admit_checkpoint(checkpoint, "value", |_| "installed"),
            super::Admission::Obsolete("value"),
            "a foreign watcher checkpoint must reject"
        );
    }

    #[test]
    fn request_admission_admits_only_the_current_revision() {
        let now = Instant::now();
        let mut gate = Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let request = gate.start(now).unwrap();
        assert_eq!(
            gate.admit_current(request.complete("current"), |value| (value, "installed")),
            super::Admission::Current(("current", "installed"))
        );

        gate.observe(now).unwrap();
        let revoked = gate.start(now).unwrap().complete("revoked");
        gate.observe(now).unwrap();
        assert_eq!(
            gate.admit_current(revoked, |_| panic!(
                "revocation must abort before the commit callback"
            )),
            super::Admission::Obsolete("revoked")
        );
    }

    #[test]
    fn revision_authority_debug_output_is_redacted() {
        let now = Instant::now();
        let mut gate = Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let request = gate.start(now).unwrap();
        assert_eq!(format!("{request:?}"), "Request(<private>)");
        assert_eq!(
            format!("{:?}", request.complete("secret")),
            "Candidate(<private>)"
        );
        assert_eq!(
            format!("{:?}", Gate::new(Duration::ZERO).checkpoint().unwrap()),
            "Checkpoint(<private>)"
        );
    }
}
