//! Bounded scheduling hints. The admission gate remains authoritative.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::Notice;

/// Coalesced watcher notices: the newest change or the first terminal failure.
/// Receiving a notice does not grant candidate admission authority.
pub struct Notices {
    state: Arc<Mutex<State>>,
    wake: Receiver<()>,
}

#[derive(Clone)]
pub(crate) struct Publisher {
    state: Arc<Mutex<State>>,
    wake: SyncSender<()>,
}

#[derive(Default)]
struct State {
    pending: Option<Notice>,
    terminal: bool,
}

pub(crate) fn channel() -> (Publisher, Notices) {
    let state = Arc::new(Mutex::new(State::default()));
    let (sender, receiver) = mpsc::sync_channel(1);
    (
        Publisher {
            state: Arc::clone(&state),
            wake: sender,
        },
        Notices {
            state,
            wake: receiver,
        },
    )
}

impl Publisher {
    pub(crate) fn publish(&self, notice: Notice) {
        self.publish_with(|| Some(notice));
    }

    // Creating and replacing a revision notice share one ordering. No consumer
    // holds this lock while waiting, and waking never waits for the consumer.
    // Callback lock order is notice -> gate -> publication. Normalization is
    // outside this lock; consumers only take the slot and never acquire a gate.
    // Separate publisher calls must occur after releasing any gate lock.
    pub(crate) fn publish_with(&self, create: impl FnOnce() -> Option<Notice>) {
        let mut state = lock(&self.state);
        let Some(notice) = create() else {
            return;
        };
        if state.terminal {
            return;
        }
        state.terminal = !matches!(notice, Notice::Changed(_));
        let retired = state.pending.replace(notice);
        // Full means an existing wake already covers the newest payload.
        // Disconnected means the owner has stopped receiving hints.
        let _ = self.wake.try_send(());
        drop(state);
        drop(retired);
    }
}

impl Notices {
    /// Take the newest available scheduling hint without waiting.
    pub fn try_recv(&self) -> Result<Notice, TryRecvError> {
        loop {
            self.wake.try_recv()?;
            if let Some(notice) = lock(&self.state).pending.take() {
                return Ok(notice);
            }
            // A receiver may consume the old wake, then take a newer payload
            // before consuming its wake. That extra wake carries no authority.
        }
    }

    /// Wait at most `timeout` for a scheduling hint. Stale wakes do not reset
    /// the deadline; the standard channel timeout/disconnect errors are kept.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Notice, RecvTimeoutError> {
        let started = Instant::now();
        loop {
            self.wake
                .recv_timeout(timeout.saturating_sub(started.elapsed()))?;
            if let Some(notice) = lock(&self.state).pending.take() {
                return Ok(notice);
            }
            if started.elapsed() >= timeout {
                return Err(RecvTimeoutError::Timeout);
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(state) => state,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawr::reload::scheduler::{Admission, Gate};

    #[test]
    fn notice_burst_retains_only_the_latest_revision_and_rearms() {
        let (sender, receiver) = channel();
        let gate = Gate::new(Duration::ZERO);
        let now = Instant::now();
        for _ in 0..10_000 {
            sender.publish(Notice::Changed(gate.observe(now).unwrap()));
        }
        assert_eq!(receiver.wake.try_iter().count(), 1);
        // Restore the single wake removed only to inspect its boundedness.
        sender.wake.try_send(()).unwrap();
        let Notice::Changed(observed) = receiver.try_recv().unwrap() else {
            panic!("expected latest change");
        };
        let mailbox = rawr::reload::scheduler::mailbox::Slot::<
            rawr::reload::scheduler::mailbox::Revision<(), ()>,
            (),
        >::new();
        assert!(matches!(
            gate.publish_notice(observed, (), &mailbox),
            Admission::Current(Ok(()))
        ));
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
        sender.publish(Notice::Changed(gate.observe(now).unwrap()));
        assert!(matches!(receiver.try_recv(), Ok(Notice::Changed(_))));
    }

    #[test]
    fn notice_terminal_replaces_unread_change_and_stays_first() {
        let (sender, receiver) = channel();
        let gate = Gate::new(Duration::ZERO);
        let now = Instant::now();
        sender.publish(Notice::Changed(gate.observe(now).unwrap()));
        sender.publish(Notice::Backend(notify::Error::generic("first")));
        for _ in 0..1_000 {
            sender.publish(Notice::Changed(gate.observe(now).unwrap()));
            sender.publish(Notice::Backend(notify::Error::generic("later")));
        }
        let Notice::Backend(error) = receiver.try_recv().unwrap() else {
            panic!("terminal must win");
        };
        assert!(error.to_string().contains("first"));
        sender.publish(Notice::Changed(gate.observe(now).unwrap()));
        sender.publish(Notice::Schedule(rawr::reload::scheduler::Error::Suspended));
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn notice_schedule_is_terminal_and_disconnect_drains_payload() {
        let (sender, receiver) = channel();
        sender.publish(Notice::Schedule(rawr::reload::scheduler::Error::Suspended));
        sender.publish(Notice::Backend(notify::Error::generic("later")));
        drop(sender);
        assert!(matches!(receiver.try_recv(), Ok(Notice::Schedule(_))));
        assert!(matches!(
            receiver.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
        assert!(matches!(
            receiver.recv_timeout(Duration::ZERO),
            Err(RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn notice_stale_wake_race_is_drained_without_extending_timeout() {
        let (sender, receiver) = channel();
        let gate = Gate::new(Duration::ZERO);
        let now = Instant::now();
        sender.publish(Notice::Changed(gate.observe(now).unwrap()));
        // Reproduce the receiver taking a wake, then a producer replacing the
        // payload and rearming before the receiver takes that newer payload.
        receiver.wake.try_recv().unwrap();
        sender.publish(Notice::Changed(gate.observe(now).unwrap()));
        assert!(lock(&receiver.state).pending.take().is_some());
        assert!(matches!(
            receiver.recv_timeout(Duration::ZERO),
            Err(RecvTimeoutError::Timeout)
        ));
        sender.publish(Notice::Changed(gate.observe(now).unwrap()));
        assert!(matches!(
            receiver.recv_timeout(Duration::ZERO),
            Ok(Notice::Changed(_))
        ));
        // The nonblocking path must also swallow an empty wake.
        sender.wake.try_send(()).unwrap();
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
        drop(receiver);
        sender.publish(Notice::Changed(gate.observe(now).unwrap()));
    }
}
