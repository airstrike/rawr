//! `cargo check` results for the live window. The command that runs cargo
//! sends each finished check through a [`Sender`]. The window shows the
//! newest one beside the reload status. It never blocks or fails a reload.

use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use iced::futures::task::AtomicWaker;
use iced::futures::Stream;

/// What the newest `cargo check` found, for the window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    /// One line: `cargo check found 2 errors`.
    pub heading: String,
    /// The errors as rustc rendered them.
    pub details: String,
}

/// A finished check: `None` when cargo found nothing wrong.
#[derive(Clone, Debug)]
pub struct Checked(pub Option<Report>);

/// The sending end, for the thread that runs cargo.
#[derive(Clone)]
pub struct Sender(Arc<Inner>);

#[derive(Clone)]
pub(crate) struct Inbox(Arc<Inner>);

struct Inner {
    newest: Mutex<Option<Checked>>,
    waker: AtomicWaker,
}

pub(crate) fn channel() -> (Sender, Inbox) {
    let inner = Arc::new(Inner {
        newest: Mutex::new(None),
        waker: AtomicWaker::new(),
    });
    (Sender(Arc::clone(&inner)), Inbox(inner))
}

impl Sender {
    /// Show `report` in place of the previous check's.
    pub fn send(&self, report: Option<Report>) {
        *self
            .0
            .newest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Checked(report));
        self.0.waker.wake();
    }
}

impl Hash for Inbox {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

pub(crate) struct Messages(Inbox);

impl Stream for Messages {
    type Item = Checked;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Checked>> {
        let inner = &self.0 .0;
        inner.waker.register(context.waker());
        let newest = inner
            .newest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        match newest {
            Some(checked) => Poll::Ready(Some(checked)),
            None => Poll::Pending,
        }
    }
}

pub(crate) fn messages(inbox: &Inbox) -> Messages {
    Messages(inbox.clone())
}

#[cfg(test)]
mod tests {
    use iced::futures::Stream as _;

    #[test]
    fn only_the_newest_check_reaches_the_window() {
        let (sender, inbox) = super::channel();
        let mut messages = super::messages(&inbox);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::pin::Pin::new(&mut messages)
            .poll_next(&mut context)
            .is_pending());
        sender.send(Some(super::Report {
            heading: "old".into(),
            details: String::new(),
        }));
        sender.send(None);
        match std::pin::Pin::new(&mut messages).poll_next(&mut context) {
            std::task::Poll::Ready(Some(super::Checked(None))) => {}
            other => panic!("{other:?}"),
        }
        assert!(std::pin::Pin::new(&mut messages)
            .poll_next(&mut context)
            .is_pending());
    }
}
