//! The generations as a stream, for an async framework's subscription.

use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures_channel::mpsc;

use crate::runtime;

/// The number of each new generation the program takes, from when the
/// stream is made. Under cargo it never yields.
#[must_use = "a stream yields nothing unless polled"]
pub struct Generations {
    receiver: Option<mpsc::UnboundedReceiver<u64>>,
}

/// Every generation the program takes from now, as a stream: under `rawr
/// run` it yields each one's number, and under cargo it stays pending.
pub fn generations() -> Generations {
    let (sender, receiver) = mpsc::unbounded();
    let listening = runtime::listen(Box::new(move |generation| {
        // A dropped stream's receiver is gone, and its number goes nowhere.
        let _ = sender.unbounded_send(generation);
    }));
    Generations {
        receiver: listening.then_some(receiver),
    }
}

impl futures_core::Stream for Generations {
    type Item = u64;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<u64>> {
        match &mut self.receiver {
            Some(receiver) => futures_core::Stream::poll_next(Pin::new(receiver), context),
            None => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::on_generation;

    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::task::Waker;

    #[test]
    fn without_rawr_a_stream_stays_pending_and_a_callback_never_runs() {
        let called = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&called);
        on_generation(move |_| seen.store(true, Ordering::Relaxed));
        let mut stream = generations();
        let mut context = Context::from_waker(Waker::noop());
        let polled = futures_core::Stream::poll_next(Pin::new(&mut stream), &mut context);
        assert!(polled.is_pending(), "no runner told this process anything");
        assert!(!called.load(Ordering::Relaxed));
    }
}
