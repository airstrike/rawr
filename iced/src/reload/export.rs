//! State export requests from the protocol worker to the iced owner.
//!
//! A `.rs` edit starts a new runner process. Before it starts, the worker
//! asks the iced owner of the old one for its State as carried bytes; the
//! owner answers on its own thread, between messages.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use iced::futures::Stream;

#[derive(Default)]
struct Queue {
    requests: VecDeque<mpsc::SyncSender<Option<Vec<u8>>>>,
    waker: Option<Waker>,
}

/// The worker's end: asks for the State and waits for the owner's answer.
#[derive(Clone, Default)]
pub struct Exporter(Arc<Mutex<Queue>>);

/// The owner's end, a subscription of pending requests.
#[derive(Clone)]
pub(crate) struct Requests(Arc<Mutex<Queue>>);

/// One pending request; the owner answers it once.
#[derive(Clone)]
pub(crate) struct Request(Arc<Mutex<Option<mpsc::SyncSender<Option<Vec<u8>>>>>>);

impl Exporter {
    pub(crate) fn requests(&self) -> Requests {
        Requests(Arc::clone(&self.0))
    }

    /// The owner's State as carried bytes; `None` when it holds anything but
    /// plain script data or the owner does not answer within `timeout`.
    #[must_use]
    pub fn export(&self, timeout: Duration) -> Option<Vec<u8>> {
        let (answer, answered) = mpsc::sync_channel(1);
        let waker = {
            let mut queue = self.0.lock().ok()?;
            queue.requests.push_back(answer);
            queue.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        answered.recv_timeout(timeout).ok().flatten()
    }
}

impl Request {
    pub(crate) fn answer(&self, state: Option<Vec<u8>>) {
        if let Some(answer) = self.0.lock().ok().and_then(|mut answer| answer.take()) {
            let _ = answer.send(state);
        }
    }
}

impl std::fmt::Debug for Request {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("export::Request")
    }
}

impl Hash for Requests {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl Requests {
    pub(crate) fn subscription(&self) -> iced::Subscription<Request> {
        iced::Subscription::run_with(self.clone(), |requests| Pending(requests.clone()))
    }
}

struct Pending(Requests);

impl Stream for Pending {
    type Item = Request;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Request>> {
        let Ok(mut queue) = (self.0).0.lock() else {
            return Poll::Ready(None);
        };
        match queue.requests.pop_front() {
            Some(answer) => Poll::Ready(Some(Request(Arc::new(Mutex::new(Some(answer)))))),
            None => {
                queue.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use iced::futures::StreamExt;
    use std::time::Duration;

    #[test]
    fn a_worker_request_reaches_the_owner_stream_and_gets_its_answer() {
        let exporter = super::Exporter::default();
        let mut pending = super::Pending(exporter.requests());
        let worker = {
            let exporter = exporter.clone();
            std::thread::spawn(move || exporter.export(Duration::from_secs(5)))
        };
        let request = iced::futures::executor::block_on(pending.next()).unwrap();
        request.answer(Some(vec![1, 2, 3]));
        assert_eq!(worker.join().unwrap(), Some(vec![1, 2, 3]));
    }
}
