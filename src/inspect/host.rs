//! The inspector's host seams (D1057.2): how a request's answer reaches
//! whoever asked, how a paused program waits for its next request, and
//! where a breakpoint reads its line's text.
//!
//! A socket's inspector answers each connection on a thread of its own,
//! which blocks until the answer; its pause waits on a `Condvar`, and line
//! text comes from disk. A hosted one, as a browser page's, has one thread:
//! a request hands over a callback that takes its answer, which may come
//! at a later step of the program; a pause waits through the host, which
//! suspends the program's stack and lets the host run until the next
//! request wakes it; line text comes from the files the program compiled
//! from.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use serde_json::Value;

use crate::package::snapshot::Snapshot;

/// How a hosted inspector's paused program waits: the host suspends the
/// stack the program runs on until [`Host::wake`], while the host goes on
/// with its own events, which may hand the inspector requests.
pub trait Host: Send + Sync {
    /// Whether the step the program is at can wait: a host that suspends
    /// one stack of its own can't wait on any other, and the program goes
    /// on there without pausing.
    fn can_wait(&self) -> bool;

    /// Wait until the next [`Host::wake`]. `Err` says why this host can't
    /// suspend the program after all; the pause then ends.
    ///
    /// # Errors
    ///
    /// The host's reason it couldn't suspend the program.
    fn wait(&self) -> Result<(), String>;

    /// End the current [`Host::wait`], if one is under way. A wake with no
    /// wait under way does nothing: the program looks at what changed
    /// before it waits again.
    fn wake(&self);
}

/// Where one request's answer goes, once: the thread of the connection
/// waiting for it, or the callback a host handed over with the request.
pub(super) struct Reply(Box<dyn FnOnce(Value) + Send>);

impl Reply {
    pub(super) fn new(reply: impl FnOnce(Value) + Send + 'static) -> Self {
        Self(Box::new(reply))
    }

    pub(super) fn send(self, answer: Value) {
        (self.0)(answer);
    }
}

/// How a pause waits for what it waits on.
pub(super) enum Waits {
    /// On the inspector's `Condvar`: each request runs on a thread of its
    /// own.
    #[cfg_attr(not(unix), allow(dead_code))]
    Thread,
    /// Through the host, which runs the requests itself.
    Host(Box<dyn Host>),
}

/// Where a breakpoint on a line reads the line's text.
pub(super) enum Files {
    /// The file on disk, as a save leaves it.
    #[cfg_attr(not(unix), allow(dead_code))]
    Disk,
    /// The files the program that runs next compiled from, once it did.
    Compiled(Mutex<Option<Snapshot>>),
}

impl Files {
    /// The text of `file`, or why it has none.
    pub(super) fn read(&self, file: &str) -> Result<String, String> {
        match self {
            Self::Disk => std::fs::read_to_string(file).map_err(|error| error.to_string()),
            Self::Compiled(compiled) => compiled
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .and_then(|snapshot| snapshot.read(Path::new(file)))
                .map(str::to_owned)
                .ok_or_else(|| "it isn't one of the program's files".to_owned()),
        }
    }

    /// Read from `snapshot` from now on, when these are the compiled files.
    pub(super) fn compiled_from(&self, snapshot: Option<&Snapshot>) {
        if let Self::Compiled(compiled) = self {
            *compiled.lock().unwrap_or_else(PoisonError::into_inner) = snapshot.cloned();
        }
    }
}
