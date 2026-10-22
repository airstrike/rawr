//! One-slot latest completion publication owned by the revision gate.

use std::fmt;
use std::sync::{Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

/// One bounded completion slot with explicit terminal shutdown.
///
/// Revision-bearing values can only enter through
/// [`super::Gate::publish_latest`]. This keeps the final revision check and
/// publication replacement under one linearized scheduler operation.
pub struct Slot<T, E> {
    state: Mutex<State<T, E>>,
}

/// One value removed from the bounded completion slot.
#[must_use = "a mailbox entry must be handled or retired"]
pub enum Entry<T, E> {
    /// The newest admitted revision.
    Revision(T),
    /// The explicit terminal session failure.
    Terminal(E),
}

/// One revision-bound candidate or non-install progress notice.
#[must_use = "a revision value must be handled or retired"]
pub enum Revision<T, N> {
    /// A completed value retaining compile and final-install authority.
    Candidate(super::Candidate<T>),
    /// Authority-free progress for an exact observed revision.
    Notice(super::notice::Value<N>),
}

struct State<T, E> {
    pending: Option<Entry<T, E>>,
    edge: bool,
    waker: Option<Waker>,
    closed: bool,
    terminal: bool,
}

pub(super) struct Transition<T, E> {
    retired: Option<Entry<T, E>>,
    waker: Option<Waker>,
}

impl<T, E> Slot<T, E> {
    /// Create an empty open slot.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                pending: None,
                edge: false,
                waker: None,
                closed: false,
                terminal: false,
            }),
        }
    }

    /// Replace the pending value with an explicit terminal failure.
    ///
    /// Replaced values and their destructors run only after the slot lock has
    /// been released.
    pub fn terminate(&self, error: E) -> Result<(), E> {
        let transition = {
            let mut state = lock(&self.state);
            if state.closed {
                return Err(error);
            }
            let was_empty = state.pending.is_none();
            let retired = state.pending.replace(Entry::Terminal(error));
            state.closed = true;
            state.terminal = true;
            if was_empty {
                state.edge = true;
            }
            Transition {
                retired,
                waker: was_empty.then(|| state.waker.take()).flatten(),
            }
        };
        transition.finish();
        Ok(())
    }

    /// Close producers and retire an unpolled non-terminal revision.
    ///
    /// A terminal failure remains available to the sole consumer.
    pub fn shutdown(&self) {
        let transition = {
            let mut state = lock(&self.state);
            if state.terminal {
                return;
            }
            state.closed = true;
            state.edge = false;
            Transition {
                retired: state.pending.take(),
                waker: state.waker.take(),
            }
        };
        transition.finish();
    }

    /// Close the consumer and retire every pending entry.
    pub fn close(&self) {
        let transition = {
            let mut state = lock(&self.state);
            state.closed = true;
            state.terminal = false;
            state.edge = false;
            Transition {
                retired: state.pending.take(),
                waker: state.waker.take(),
            }
        };
        drop(transition.retired);
        drop(transition.waker);
    }

    /// Poll one authority-free empty-to-full wake edge.
    ///
    /// The pending entry remains in this slot until the event-loop owner calls
    /// [`Self::take_latest`]. Replacements while the slot is full therefore do
    /// not enqueue payloads or emit another edge.
    pub fn poll_wake(&self, context: &mut Context<'_>) -> Poll<Option<()>> {
        let mut state = lock(&self.state);
        if state.edge {
            state.edge = false;
            return Poll::Ready(Some(()));
        }
        if state.closed {
            return Poll::Ready(None);
        }
        if state
            .waker
            .as_ref()
            .is_none_or(|waker| !waker.will_wake(context.waker()))
        {
            state.waker = Some(context.waker().clone());
        }
        Poll::Pending
    }

    /// Remove the newest entry after its edge reaches the sole event-loop
    /// owner.
    #[must_use]
    pub fn take_latest(&self) -> Option<Entry<T, E>> {
        let mut state = lock(&self.state);
        state.edge = false;
        state.pending.take()
    }

    /// Return the bounded resident count for transport verification.
    #[doc(hidden)]
    #[must_use]
    pub fn pending(&self) -> usize {
        usize::from(lock(&self.state).pending.is_some())
    }
}

impl<T, N, E> Slot<Revision<T, N>, E> {
    pub(super) fn publish_candidate(
        &self,
        candidate: super::Candidate<T>,
    ) -> Result<Transition<Revision<T, N>, E>, super::Candidate<T>> {
        let mut state = lock(&self.state);
        if state.closed {
            return Err(candidate);
        }
        let was_empty = state.pending.is_none();
        let retired = state
            .pending
            .replace(Entry::Revision(Revision::Candidate(candidate)));
        if was_empty {
            state.edge = true;
        }
        let waker = was_empty.then(|| state.waker.take()).flatten();
        Ok(Transition { retired, waker })
    }

    pub(super) fn publish_notice(
        &self,
        notice: super::notice::Value<N>,
    ) -> Result<Transition<Revision<T, N>, E>, super::notice::Value<N>> {
        let mut state = lock(&self.state);
        if state.closed {
            return Err(notice);
        }
        let was_empty = state.pending.is_none();
        let retired = state
            .pending
            .replace(Entry::Revision(Revision::Notice(notice)));
        if was_empty {
            state.edge = true;
        }
        let waker = was_empty.then(|| state.waker.take()).flatten();
        Ok(Transition { retired, waker })
    }
}

impl<T, E> Default for Slot<T, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, E> Transition<T, E> {
    pub(super) fn finish(self) {
        drop(self.retired);
        if let Some(waker) = self.waker {
            waker.wake();
        }
    }
}

impl<T, E> fmt::Debug for Slot<T, E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Slot(<private>)")
    }
}

impl<T, E> fmt::Debug for Entry<T, E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Revision(_) => formatter.write_str("Revision(<private>)"),
            Self::Terminal(_) => formatter.write_str("Terminal(<private>)"),
        }
    }
}

impl<T, N> fmt::Debug for Revision<T, N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Candidate(_) => formatter.write_str("Candidate(<private>)"),
            Self::Notice(_) => formatter.write_str("Notice(<private>)"),
        }
    }
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    match value.lock() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}
