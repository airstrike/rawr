//! How the shell's lines reach the session and the session's reach the
//! shell. A transport sends; what it hears arrives as [`Event`]s on the
//! stream that connected it, so the shell's state machine never blocks on
//! one and never knows which carries it: a local socket here, a page's
//! session in a browser.

use std::fmt;
use std::sync::Arc;

/// Something that carries the shell's lines to the session.
pub trait Transport: Send + Sync {
    /// Send `line`, which ends with a newline. A transport whose session
    /// went away drops it: the stream that connected it then ends with
    /// [`Event::Closed`].
    fn send(&self, line: &str);
}

/// A connected transport, as the shell holds it.
#[derive(Clone)]
pub struct Link(Arc<dyn Transport>);

/// What a connection's stream tells the shell.
#[derive(Clone, Debug)]
pub enum Event {
    /// The connection is up: send on this link.
    Connected(Link),
    /// One line from the session, without its newline.
    Line(String),
    /// The connection closed or never opened, with why.
    Closed(String),
}

impl Link {
    /// The link that sends through `transport`.
    #[must_use]
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self(transport)
    }

    /// Send `line` to the session.
    pub fn send(&self, line: &str) {
        self.0.send(line);
    }
}

impl fmt::Debug for Link {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Link")
    }
}
