//! The sessions the server reaches, one per project root. The server's
//! loop owns them. A call sends the loop a [`Request`] and waits on its
//! own channel, which its cancel also wakes; what blocks (a greeting, an
//! exit) the call waits for itself, then tells the loop the session it
//! attached to or started.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::call::{Call, Wake, Waker};
use super::session::{Entry, Session};
use super::{Heard, Server};

/// A call's ask of the loop's sessions.
pub(super) enum Request {
    /// The session of `root`, if the loop holds one.
    Find { root: PathBuf, reply: Waker },
    /// The session of `root`, which the loop stops and lets go, for a call
    /// that replaces or stops it.
    Take { root: PathBuf, reply: Waker },
    /// Every session.
    All { reply: Waker },
    /// A session a call attached to or started, in place of the one its
    /// root had.
    Keep(Arc<Session>),
}

/// The sessions, one per project root. The server's loop owns them.
#[derive(Debug, Default)]
pub(super) struct Sessions(HashMap<PathBuf, Arc<Session>>);

impl Sessions {
    /// Answer `request` on the asking call's channel.
    pub(super) fn heard(&mut self, request: Request) {
        match request {
            Request::Find { root, reply } => {
                let found = self.0.get(&root).cloned();
                reply.wake(Wake::Sessions(found.into_iter().collect()));
            }
            Request::Take { root, reply } => {
                let taken = self.0.remove(&root);
                // Stopped here, so a call cancelled before it hears of it
                // leaves nothing running.
                if let Some(taken) = &taken {
                    taken.stop();
                }
                reply.wake(Wake::Sessions(taken.into_iter().collect()));
            }
            Request::All { reply } => {
                let mut all: Vec<_> = self.0.values().cloned().collect();
                all.sort_by(|a, b| a.root.cmp(&b.root));
                reply.wake(Wake::Sessions(all));
            }
            Request::Keep(session) => {
                self.0.insert(session.root.clone(), session);
            }
        }
    }

    /// End every session.
    pub(super) fn stop_all(&mut self) {
        for (_, session) in self.0.drain() {
            session.stop();
        }
    }
}

/// The loop's answer to the request `ask` makes for `call`: empty once
/// the call is cancelled.
fn ask(server: &Server, call: &Call, ask: impl FnOnce(Waker) -> Request) -> Vec<Arc<Session>> {
    if call.is_cancelled() {
        return Vec::new();
    }
    server.ask(Heard::Sessions(ask(call.waker())));
    call.wait(|wake| match wake {
        Wake::Sessions(sessions) => Some(sessions),
        _ => None,
    })
    .unwrap_or_default()
}

/// The session for `root`: the server's, or the session of a run started
/// elsewhere, which the server attaches to. One that is over gives way to
/// a run there now.
pub(super) fn get(server: &Server, root: &Path, call: &Call) -> Option<Arc<Session>> {
    let known = ask(server, call, |reply| Request::Find {
        root: root.to_path_buf(),
        reply,
    })
    .pop();
    if known
        .as_ref()
        .is_some_and(|session| !session.phase().is_over())
    {
        return known;
    }
    let Some(attached) = Session::attach(root, call) else {
        return known;
    };
    server.ask(Heard::Sessions(Request::Keep(Arc::clone(&attached))));
    Some(attached)
}

/// Every session, by root.
pub(super) fn all(server: &Server, call: &Call) -> Vec<Arc<Session>> {
    ask(server, call, |reply| Request::All { reply })
}

/// Start `rawr run <path> -- arguments` for `root`, replacing the session
/// the server started there once its run exited, its program reading
/// `stdin`. A run started elsewhere that serves `root` is attached to
/// instead.
pub(super) fn start(
    server: &Server,
    root: &Path,
    path: &Path,
    arguments: &[String],
    stdin: Option<&str>,
    call: &Call,
) -> std::io::Result<Arc<Session>> {
    if let Some(old) = stop(server, root, call) {
        old.wait_exit(call);
    }
    let cancelled = || std::io::Error::other("cancelled");
    if call.is_cancelled() {
        return Err(cancelled());
    }
    let session = match Session::attach(root, call) {
        Some(attached) => attached,
        None if call.is_cancelled() => return Err(cancelled()),
        None => Session::spawn(root, path, arguments, stdin, Entry::Run)?,
    };
    server.ask(Heard::Sessions(Request::Keep(Arc::clone(&session))));
    Ok(session)
}

/// End the session for `root`, if any.
pub(super) fn stop(server: &Server, root: &Path, call: &Call) -> Option<Arc<Session>> {
    ask(server, call, |reply| Request::Take {
        root: root.to_path_buf(),
        reply,
    })
    .pop()
}
