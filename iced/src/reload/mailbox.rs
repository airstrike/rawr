//! Iced stream adapter for the neutral one-slot revision mailbox.

use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use iced::futures::Stream;
use rawr::reload::scheduler::{self, Admission};

use super::{status, Completion, Input, Outcome, Wake};

type Core = scheduler::mailbox::Slot<scheduler::mailbox::Revision<Outcome, status::Phase>, Input>;

/// Cloneable producer for the sole live-host completion slot.
pub struct Sender {
    producer: Arc<Producer>,
}

#[derive(Clone)]
pub(super) struct Inbox(Arc<Core>);

pub(super) struct Messages(Inbox);

struct Producer {
    mailbox: Arc<Core>,
}

pub(super) fn channel() -> (Sender, Inbox) {
    let mailbox = Arc::new(Core::new());
    (
        Sender {
            producer: Arc::new(Producer {
                mailbox: Arc::clone(&mailbox),
            }),
        },
        Inbox(mailbox),
    )
}

impl Sender {
    pub(super) fn publish(
        &self,
        gate: &scheduler::Gate,
        candidate: scheduler::Candidate<Outcome>,
    ) -> Result<bool, scheduler::Candidate<Outcome>> {
        match gate.publish_latest(candidate, &self.producer.mailbox) {
            Admission::Current(Ok(())) => Ok(true),
            Admission::Current(Err(candidate)) => Err(candidate),
            Admission::Obsolete(outcome) => {
                drop(outcome);
                Ok(false)
            }
        }
    }

    pub(super) fn publish_notice(
        &self,
        gate: &scheduler::Gate,
        observed: scheduler::notice::Observed,
        phase: status::Phase,
    ) -> bool {
        match gate.publish_notice(observed, phase, &self.producer.mailbox) {
            Admission::Current(Ok(())) => true,
            Admission::Current(Err(notice)) => {
                drop(notice);
                false
            }
            Admission::Obsolete(_phase) => false,
        }
    }

    pub(super) fn publish_started(
        &self,
        gate: &scheduler::Gate,
        request: &scheduler::Request,
        phase: status::Phase,
    ) -> bool {
        match gate.publish_started(request, phase, &self.producer.mailbox) {
            Admission::Current(Ok(())) => true,
            Admission::Current(Err(notice)) => {
                drop(notice);
                false
            }
            Admission::Obsolete(_phase) => false,
        }
    }

    pub(super) fn publish_terminal_input(&self, input: Input) -> Result<(), Input> {
        self.producer.mailbox.terminate(input)
    }

    /// Close this producer and retire any unpolled non-terminal completion.
    pub fn shutdown(&self) {
        self.producer.mailbox.shutdown();
    }

    #[cfg(test)]
    pub(super) fn pending(&self) -> usize {
        self.producer.mailbox.pending()
    }
}

impl Clone for Sender {
    fn clone(&self) -> Self {
        Self {
            producer: Arc::clone(&self.producer),
        }
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        self.mailbox.shutdown();
    }
}

impl Inbox {
    pub(super) fn close(&self) {
        self.0.close();
    }

    pub(super) fn take_latest(&self, _wake: Wake) -> Option<Completion> {
        self.0.take_latest().map(|entry| match entry {
            scheduler::mailbox::Entry::Revision(revision) => Completion(Input::Revision(revision)),
            scheduler::mailbox::Entry::Terminal(input) => Completion(input),
        })
    }
}

impl Hash for Inbox {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl Stream for Messages {
    type Item = Wake;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0
             .0
            .poll_wake(context)
            .map(|edge| edge.map(|()| Wake(())))
    }
}

pub(super) fn messages(inbox: &Inbox) -> Messages {
    Messages(inbox.clone())
}
