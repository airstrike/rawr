//! A call's waits. What a call waits on answers on one channel of the
//! call's own: a child's exit, an extension's lines, the loop's checks and
//! sessions, and each session's answers. A cancel sets the call's flag and
//! is one more message on that channel, so it wakes whatever the call
//! waits on, and a wait the call starts after it reads the flag.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use serde_json::Value;

use super::session::{Session, Told};

/// What wakes a call.
#[derive(Debug)]
pub enum Wake {
    /// The client cancelled the call, or the server ends.
    Cancelled,
    /// The child that leads `group` exited. It stays a zombie for the call
    /// to reap, so its group can't pass to another process before the call
    /// ends it.
    Exited { group: u32 },
    /// A record an extension's child, leading `group`, wrote; `None` once
    /// its output closed.
    Line { group: u32, record: Option<Value> },
    /// The check the call asked the loop for.
    Checked(Value),
    /// The sessions the call asked the loop for.
    Sessions(Vec<Arc<Session>>),
    /// A session told the call's wait `id`.
    Session { id: u64, told: Told },
}

/// A call's channel, read by the call's own thread.
#[derive(Debug)]
pub struct Call {
    waker: Waker,
    woken: mpsc::Receiver<Wake>,
}

/// What reaches a call from other threads: its cancel flag, and its
/// channel.
#[derive(Clone, Debug)]
pub struct Waker {
    cancelled: Arc<AtomicBool>,
    to: mpsc::Sender<Wake>,
}

impl Default for Call {
    fn default() -> Self {
        let (to, woken) = mpsc::channel();
        Self {
            waker: Waker {
                cancelled: Arc::default(),
                to,
            },
            woken,
        }
    }
}

impl Call {
    /// What other threads wake this call with.
    #[must_use]
    pub fn waker(&self) -> Waker {
        self.waker.clone()
    }

    /// Whether the call was cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.waker.is_cancelled()
    }

    /// The first wake `pick` takes, or `None` once the call is cancelled.
    /// `pick` passes over what waits that already ended still send.
    pub fn wait<T>(&self, mut pick: impl FnMut(Wake) -> Option<T>) -> Option<T> {
        loop {
            // A wait after the one that took the cancel's message reads the
            // flag.
            if self.is_cancelled() {
                return None;
            }
            // The call holds a sender, so the channel never closes.
            match self.woken.recv() {
                Ok(Wake::Cancelled) | Err(mpsc::RecvError) => return None,
                Ok(wake) => {
                    if let Some(picked) = pick(wake) {
                        return Some(picked);
                    }
                }
            }
        }
    }
}

impl Waker {
    /// Cancel the call: its flag is set before its channel hears it.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _ = self.to.send(Wake::Cancelled);
    }

    /// Whether the call was cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Wake the call with `wake`: whether it went, which it doesn't once
    /// the call was cancelled or is over.
    pub fn wake(&self, wake: Wake) -> bool {
        !self.is_cancelled() && self.to.send(wake).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{Call, Wake};

    /// A cancel wakes the wait it comes during, and every wait after it
    /// returns at once.
    #[test]
    fn a_cancel_wakes_the_wait_and_ends_the_later_ones() {
        let call = Call::default();
        let waker = call.waker();
        let cancelling = std::thread::spawn(move || waker.cancel());
        assert!(call.wait(|_| Some(())).is_none());
        cancelling.join().unwrap();
        assert!(call.wait(|_| Some(())).is_none());
        assert!(
            !call.waker().wake(Wake::Checked(serde_json::json!(1))),
            "a cancelled call takes no more wakes"
        );
    }
}
