//! I/O-free debounce and latest-revision admission shared by reload transports.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub mod mailbox;
pub mod notice;
pub mod publication;

/// One synchronized debounce and latest-revision admission boundary.
///
/// Observers revoke old work immediately. Candidate preparation happens
/// outside this gate; final admission holds the lock only through one direct
/// current-slot replacement.
pub struct Gate {
    stamp: Arc<Stamp>,
    scheduler: Mutex<Scheduler>,
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

type SlotAdmission<T> = Admission<
    super::installation::current::Prepared<T>,
    Result<
        (
            Box<[super::compatibility::Evidence]>,
            super::installation::current::Retired<T>,
        ),
        super::installation::current::Rejected<T>,
    >,
>;

type PublishedSlotAdmission<T> = Admission<
    (
        super::installation::current::Prepared<T>,
        publication::Validated,
    ),
    Result<
        (
            Box<[super::compatibility::Evidence]>,
            super::installation::current::Retired<T>,
            publication::Retired,
        ),
        publication::Rejected<T>,
    >,
>;

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
            scheduler: Mutex::new(Scheduler::new(debounce)),
        }
    }

    /// Observe one relevant event and immediately revoke older work.
    pub fn observe(&self, now: Instant) -> Result<notice::Observed, Error> {
        lock(&self.scheduler)
            .observe(now)
            .map(|revision| notice::Observed::new(Arc::clone(&self.stamp), revision))
    }

    /// Evaluate pure relevance and observe it under the same gate lock.
    ///
    /// This lets transports test the current exact input filter and issue a
    /// revoking revision as one linearized observation.
    pub fn observe_if(&self, now: Instant, relevant: impl FnOnce() -> bool) -> Result<bool, Error> {
        let mut scheduler = lock(&self.scheduler);
        if !relevant() {
            return Ok(false);
        }
        scheduler.observe(now)?;
        Ok(true)
    }

    /// Evaluate pure relevance and return a non-install observation proof.
    ///
    /// The returned value can publish progress for this revision, but cannot
    /// be transformed into compile or installation authority.
    pub fn observe_if_notice(
        &self,
        now: Instant,
        relevant: impl FnOnce() -> bool,
    ) -> Result<Option<notice::Observed>, Error> {
        let mut scheduler = lock(&self.scheduler);
        if !relevant() {
            return Ok(None);
        }
        scheduler
            .observe(now)
            .map(|revision| notice::Observed::new(Arc::clone(&self.stamp), revision))
            .map(Some)
    }

    /// Return the newest queued revision's debounce deadline.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        lock(&self.scheduler).deadline()
    }

    /// Start the newest queued request after its debounce deadline.
    #[must_use]
    pub fn start(&self, now: Instant) -> Option<Request> {
        lock(&self.scheduler).start(now).map(|revision| Request {
            stamp: Arc::clone(&self.stamp),
            revision,
        })
    }

    /// Observe and start one request without allowing an intervening event to
    /// replace its revision. Nonzero debounce still returns `None` and leaves
    /// the observed request queued for its normal deadline.
    pub fn observe_and_start(&self, now: Instant) -> Result<Option<Request>, Error> {
        let mut scheduler = lock(&self.scheduler);
        scheduler.observe(now)?;
        Ok(scheduler.start(now).map(|revision| Request {
            stamp: Arc::clone(&self.stamp),
            revision,
        }))
    }

    /// Capture the event sequence after watcher registration and before
    /// off-lock preparation or process work.
    pub fn checkpoint(&self) -> Result<Checkpoint, Error> {
        let scheduler = lock(&self.scheduler);
        if scheduler.suspended {
            return Err(Error::Suspended);
        }
        if scheduler.exhausted {
            return Err(Error::RevisionExhausted);
        }
        Ok(Checkpoint {
            stamp: Arc::clone(&self.stamp),
            sequence: scheduler.next,
        })
    }

    /// Install one already-prepared value only if no watcher event occurred
    /// after its checkpoint.
    ///
    /// `admit` runs under the scheduler lock and must perform only the direct,
    /// non-blocking in-memory slot transition. Expensive work and destructors
    /// remain outside this boundary.
    #[must_use = "the checkpoint admission result must be handled"]
    pub fn admit_checkpoint<T, U>(
        &self,
        checkpoint: Checkpoint,
        value: T,
        admit: impl FnOnce(T) -> U,
    ) -> Admission<T, U> {
        let Checkpoint { stamp, sequence } = checkpoint;
        let scheduler = lock(&self.scheduler);
        if !Arc::ptr_eq(&self.stamp, &stamp)
            || scheduler.suspended
            || scheduler.exhausted
            || scheduler.next != sequence
        {
            drop(scheduler);
            return Admission::Obsolete(value);
        }
        Admission::Current(admit(value))
    }

    /// Preserve one finished candidate only while its revision remains current.
    ///
    /// This structural check lets bounded transports reject inverted worker
    /// completions before publishing them. Obsolete payloads and their
    /// destructors are returned after the scheduler lock has been released.
    #[must_use = "the admission result must be handled"]
    pub fn retain_current<T>(&self, candidate: Candidate<T>) -> Admission<T, Candidate<T>> {
        let Candidate {
            stamp,
            revision,
            value,
        } = candidate;
        let scheduler = lock(&self.scheduler);
        if self.is_current(&stamp, revision, &scheduler) {
            drop(scheduler);
            Admission::Current(Candidate {
                stamp,
                revision,
                value,
            })
        } else {
            drop(scheduler);
            Admission::Obsolete(value)
        }
    }

    /// Inspect the original request without consuming it or refreshing its revision.
    /// Final installation must still pass through the normal admission gate.
    #[must_use]
    pub fn request_is_current(&self, request: &Request) -> bool {
        self.is_current(&request.stamp, request.revision, &lock(&self.scheduler))
    }

    /// Linearize one already-prepared value against its exact request revision.
    ///
    /// `admit` runs while the scheduler is locked. It may perform bounded
    /// admission validation and a commit handshake, but must not build,
    /// transform, or re-enter this gate. Obsolete values are returned after
    /// the lock is dropped.
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
        let scheduler = lock(&self.scheduler);
        if !self.is_current(&stamp, revision, &scheduler) {
            drop(scheduler);
            return Admission::Obsolete(value);
        }
        Admission::Current(admit(value))
    }

    /// Publish one finished value only while its revision remains current.
    ///
    /// The revision check and one-slot replacement are linearized under the
    /// scheduler lock. Replaced values and wakes are released only after both
    /// scheduler and mailbox locks have been released.
    #[must_use = "the publication admission result must be handled"]
    pub fn publish_latest<T, N, E>(
        &self,
        candidate: Candidate<T>,
        mailbox: &mailbox::Slot<mailbox::Revision<T, N>, E>,
    ) -> Admission<T, Result<(), Candidate<T>>> {
        let Candidate {
            stamp,
            revision,
            value,
        } = candidate;
        let scheduler = lock(&self.scheduler);
        if !self.is_current(&stamp, revision, &scheduler) {
            drop(scheduler);
            return Admission::Obsolete(value);
        }
        let candidate = Candidate {
            stamp,
            revision,
            value,
        };
        match mailbox.publish_candidate(candidate) {
            Ok(transition) => {
                drop(scheduler);
                transition.finish();
                Admission::Current(Ok(()))
            }
            Err(candidate) => {
                drop(scheduler);
                Admission::Current(Err(candidate))
            }
        }
    }

    /// Publish progress for one exact observed revision without install authority.
    #[must_use = "the notice admission result must be handled"]
    pub fn publish_notice<T, N, E>(
        &self,
        observed: notice::Observed,
        value: N,
        mailbox: &mailbox::Slot<mailbox::Revision<T, N>, E>,
    ) -> Admission<N, Result<(), notice::Value<N>>> {
        let (stamp, revision) = observed.into_parts();
        self.publish_notice_revision(stamp, revision, value, mailbox)
    }

    /// Publish progress for one exact started request without consuming it.
    #[must_use = "the started-notice admission result must be handled"]
    pub fn publish_started<T, N, E>(
        &self,
        request: &Request,
        value: N,
        mailbox: &mailbox::Slot<mailbox::Revision<T, N>, E>,
    ) -> Admission<N, Result<(), notice::Value<N>>> {
        self.publish_notice_revision(Arc::clone(&request.stamp), request.revision, value, mailbox)
    }

    fn publish_notice_revision<T, N, E>(
        &self,
        stamp: Arc<Stamp>,
        revision: Revision,
        value: N,
        mailbox: &mailbox::Slot<mailbox::Revision<T, N>, E>,
    ) -> Admission<N, Result<(), notice::Value<N>>> {
        let scheduler = lock(&self.scheduler);
        if !self.is_current(&stamp, revision, &scheduler) {
            drop(scheduler);
            return Admission::Obsolete(value);
        }
        let notice = notice::Value::new(revision, value);
        match mailbox.publish_notice(notice) {
            Ok(transition) => {
                drop(scheduler);
                transition.finish();
                Admission::Current(Ok(()))
            }
            Err(notice) => {
                drop(scheduler);
                Admission::Current(Err(notice))
            }
        }
    }

    /// Admit one complete prepared slot while its revision remains current.
    ///
    /// The lock spans only the revision check and
    /// [`crate::reload::installation::current::Slot::admit`].
    /// Obsolete, rejected, and retired values are returned without running any
    /// of their destructors under the gate.
    #[must_use = "the slot admission result must be handled"]
    pub fn admit_slot<T>(
        &self,
        candidate: Candidate<super::installation::current::Prepared<T>>,
        slot: &mut super::installation::current::Slot<T>,
    ) -> SlotAdmission<T> {
        let Candidate {
            stamp,
            revision,
            value,
        } = candidate;
        let scheduler = lock(&self.scheduler);
        if self.is_current(&stamp, revision, &scheduler) {
            let admitted = slot.admit(value);
            drop(scheduler);
            Admission::Current(admitted)
        } else {
            drop(scheduler);
            Admission::Obsolete(value)
        }
    }

    /// Admit one prepared slot and publish its exact authority-free filter.
    ///
    /// The host bundle and filter change are linearized under the same revision
    /// lock. Every replaced, rejected, or obsolete value is returned for
    /// retirement after the lock has been released. Admission invokes no
    /// caller callback and drops no payload while locked, so ordinary gate
    /// admission exposes no user-code panic seam.
    #[must_use = "the atomic installation result must be handled"]
    pub fn admit_slot_and_publish<T>(
        &self,
        candidate: Candidate<(
            super::installation::current::Prepared<T>,
            publication::Validated,
        )>,
        slot: &mut super::installation::current::Slot<T>,
        publication: &publication::Slot,
    ) -> PublishedSlotAdmission<T> {
        let Candidate {
            stamp,
            revision,
            value,
        } = candidate;
        let scheduler = lock(&self.scheduler);
        if !self.is_current(&stamp, revision, &scheduler) {
            drop(scheduler);
            return Admission::Obsolete(value);
        }
        let (prepared, validated) = value;
        let Some(reservation) = publication.reserve(&validated) else {
            let rejected = publication::Rejected::new(
                super::installation::current::Slot::reject(prepared),
                validated,
            );
            drop(scheduler);
            return Admission::Current(Err(rejected));
        };
        match slot.admit(prepared) {
            Ok((evidence, retired)) => {
                let published = reservation.publish(validated);
                drop(scheduler);
                Admission::Current(Ok((evidence, retired, published)))
            }
            Err(rejected) => {
                drop(reservation);
                let rejected = publication::Rejected::new(rejected, validated);
                drop(scheduler);
                Admission::Current(Err(rejected))
            }
        }
    }

    fn is_current(&self, stamp: &Arc<Stamp>, revision: Revision, scheduler: &Scheduler) -> bool {
        Arc::ptr_eq(&self.stamp, stamp) && scheduler.is_current(revision)
    }

    /// Revoke all work after an unrecoverable transport failure.
    pub fn suspend(&self) {
        lock(&self.scheduler).suspend();
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

fn lock(scheduler: &Mutex<Scheduler>) -> MutexGuard<'_, Scheduler> {
    match scheduler.lock() {
        Ok(scheduler) => scheduler,
        Err(poisoned) => poisoned.into_inner(),
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
    use super::{lock, Gate};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn borrowed_request_current_check_preserves_the_original_revision() {
        let gate = Gate::new(Duration::ZERO);
        let foreign = Gate::new(Duration::ZERO);
        let request = gate.observe_and_start(Instant::now()).unwrap().unwrap();
        assert!(gate.request_is_current(&request));
        assert!(gate.request_is_current(&request));
        assert!(!foreign.request_is_current(&request));
        assert!(gate.start(Instant::now()).is_none());
        gate.observe(Instant::now()).unwrap();
        assert!(!gate.request_is_current(&request));
        assert!(matches!(
            gate.retain_current(request.complete(())),
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
        let gate = Gate::new(Duration::ZERO);
        let request = gate.observe_and_start(now).unwrap().unwrap();
        assert!(gate.start(now).is_none());
        gate.observe(now + Duration::from_millis(1)).unwrap();
        assert!(matches!(
            gate.retain_current(request.complete(())),
            super::Admission::Obsolete(())
        ));
        assert!(gate.start(now + Duration::from_millis(1)).is_some());

        let gate = Gate::new(Duration::from_millis(75));
        assert!(gate.observe_and_start(now).unwrap().is_none());
        assert_eq!(gate.deadline(), Some(now + Duration::from_millis(75)));
        assert!(gate.start(now).is_none());
        assert!(gate.start(now + Duration::from_millis(75)).is_some());
    }

    #[test]
    fn observe_and_start_preserves_terminal_scheduler_errors() {
        let now = Instant::now();
        let gate = Gate::new(Duration::ZERO);
        gate.suspend();
        assert!(matches!(
            gate.observe_and_start(now),
            Err(super::Error::Suspended)
        ));
        let gate = Gate::new(Duration::ZERO);
        lock(&gate.scheduler).next = u64::MAX;
        assert!(matches!(
            gate.observe_and_start(now),
            Err(super::Error::RevisionExhausted)
        ));
        assert!(gate.start(now).is_none());
    }

    #[test]
    fn event_during_preparation_revokes_the_candidate_before_commit() {
        let now = Instant::now();
        let gate = Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let candidate = gate
            .start(now)
            .unwrap()
            .complete("compiled")
            .try_map(|value| Ok::<_, ()>((value, "prepared")))
            .unwrap();

        gate.observe(now).unwrap();

        assert!(!lock(&gate.scheduler).is_current(candidate.revision));
        assert_eq!(candidate.value(), &("compiled", "prepared"));
    }

    #[test]
    fn transformations_retain_the_exact_current_revision() {
        let now = Instant::now();
        let gate = Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let candidate = gate.start(now).unwrap().complete(7).map(|value| value + 1);

        assert!(lock(&gate.scheduler).is_current(candidate.revision));
        assert_eq!(candidate.value(), &8);
    }

    #[test]
    fn relevance_check_and_revision_observation_are_linearized() {
        let now = Instant::now();
        let gate = Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let candidate = gate.start(now).unwrap().complete(());

        assert!(!gate.observe_if(now, || false).unwrap());
        assert!(lock(&gate.scheduler).is_current(candidate.revision));
        assert!(gate.observe_if(now, || true).unwrap());
        assert!(!lock(&gate.scheduler).is_current(candidate.revision));
    }

    #[test]
    fn suspension_is_sticky_and_rejects_post_stop_observation() {
        let now = Instant::now();
        let gate = Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let candidate = gate.start(now).unwrap().complete("running");

        gate.suspend();

        assert!(matches!(gate.observe(now), Err(super::Error::Suspended)));
        assert_eq!(gate.observe_if(now, || true), Err(super::Error::Suspended));
        assert!(gate.start(now).is_none());
        assert!(matches!(
            gate.retain_current(candidate),
            super::Admission::Obsolete("running")
        ));
    }

    #[test]
    fn replaced_completion_destructor_runs_after_gate_and_mailbox_unlock() {
        let now = Instant::now();
        let gate = Arc::new(Gate::new(Duration::ZERO));
        let mailbox = super::mailbox::Slot::<super::mailbox::Revision<Revokes, ()>, ()>::new();

        gate.observe(now).unwrap();
        let old = gate.start(now).unwrap().complete(Revokes {
            gate: Arc::clone(&gate),
            now,
        });
        assert!(matches!(
            gate.publish_latest(old, &mailbox),
            super::Admission::Current(Ok(()))
        ));

        gate.observe(now).unwrap();
        let current = gate.start(now).unwrap().complete(Revokes {
            gate: Arc::clone(&gate),
            now,
        });
        assert!(matches!(
            gate.publish_latest(current, &mailbox),
            super::Admission::Current(Ok(()))
        ));

        assert!(gate.start(now).is_some());
        mailbox.close();
    }

    #[test]
    fn notices_are_revision_bound_without_minting_candidate_authority() {
        let now = Instant::now();
        let gate = Gate::new(Duration::ZERO);
        let mailbox = super::mailbox::Slot::<super::mailbox::Revision<(), &'static str>, ()>::new();

        let old = gate.observe(now).unwrap();
        let current = gate.observe(now).unwrap();
        assert!(matches!(
            gate.publish_notice(old, "old checking", &mailbox),
            super::Admission::Obsolete("old checking")
        ));
        assert!(matches!(
            gate.publish_notice(current, "checking", &mailbox),
            super::Admission::Current(Ok(()))
        ));

        let request = gate.start(now).unwrap();
        assert!(matches!(
            gate.publish_started(&request, "reloading", &mailbox),
            super::Admission::Current(Ok(()))
        ));
        let Some(super::mailbox::Entry::Revision(super::mailbox::Revision::Notice(notice))) =
            mailbox.take_latest()
        else {
            panic!("latest progress must remain a non-install notice");
        };
        assert_eq!(notice.into_inner(), "reloading");
        drop(request);
    }

    #[test]
    fn revision_proofs_are_bound_to_their_originating_gate() {
        let now = Instant::now();
        let origin = Gate::new(Duration::ZERO);
        let foreign = Gate::new(Duration::ZERO);
        let mailbox =
            super::mailbox::Slot::<super::mailbox::Revision<&'static str, &'static str>, ()>::new();

        let observed = origin.observe(now).unwrap();
        let request = origin.start(now).unwrap();
        foreign.observe(now).unwrap();

        assert!(matches!(
            foreign.publish_notice(observed, "checking", &mailbox),
            super::Admission::Obsolete("checking")
        ));
        assert!(matches!(
            foreign.publish_started(&request, "reloading", &mailbox),
            super::Admission::Obsolete("reloading")
        ));
        assert!(matches!(
            foreign.publish_latest(request.complete("finished"), &mailbox),
            super::Admission::Obsolete("finished")
        ));
        assert!(mailbox.take_latest().is_none());
    }

    #[test]
    fn watcher_checkpoint_admits_only_without_an_intervening_event() {
        let now = Instant::now();
        let gate = Gate::new(Duration::ZERO);
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
    fn watcher_checkpoint_is_bound_to_its_gate_and_releases_obsolete_values_off_lock() {
        let now = Instant::now();
        let origin = Gate::new(Duration::ZERO);
        let foreign = Arc::new(Gate::new(Duration::ZERO));
        let checkpoint = origin.checkpoint().unwrap();
        let value = Revokes {
            gate: Arc::clone(&foreign),
            now,
        };
        let super::Admission::Obsolete(value) =
            foreign.admit_checkpoint(checkpoint, value, |_| "installed")
        else {
            panic!("a foreign watcher checkpoint must reject");
        };
        drop(value);
        assert!(foreign.start(now).is_some());
    }

    #[test]
    fn request_admission_linearizes_only_the_current_direct_transition() {
        let now = Instant::now();
        let gate = Gate::new(Duration::ZERO);
        gate.observe(now).unwrap();
        let request = gate.start(now).unwrap();
        assert_eq!(
            gate.admit_current(request.complete("current"), |value| {
                assert!(matches!(
                    gate.scheduler.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                (value, "installed")
            }),
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
    fn cross_gate_rejection_releases_payload_after_both_locks() {
        let now = Instant::now();
        let origin = Gate::new(Duration::ZERO);
        let foreign = Arc::new(Gate::new(Duration::ZERO));
        let mailbox = super::mailbox::Slot::<super::mailbox::Revision<Revokes, ()>, ()>::new();

        origin.observe(now).unwrap();
        let candidate = origin.start(now).unwrap().complete(Revokes {
            gate: Arc::clone(&foreign),
            now,
        });
        foreign.observe(now).unwrap();

        let super::Admission::Obsolete(value) = foreign.publish_latest(candidate, &mailbox) else {
            panic!("another gate must reject the candidate");
        };
        drop(value);

        assert!(foreign.start(now).is_some());
        assert!(mailbox.take_latest().is_none());
    }

    #[test]
    fn revision_authority_debug_output_is_redacted() {
        let now = Instant::now();
        let gate = Gate::new(Duration::ZERO);
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

    struct Revokes {
        gate: Arc<Gate>,
        now: Instant,
    }

    impl Drop for Revokes {
        fn drop(&mut self) {
            self.gate.observe(self.now).unwrap();
        }
    }
}
