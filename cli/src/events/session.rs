//! The debugger session a run's owner hosts (D1053.2): one per `rawr run`,
//! which any number of shells attach to, `rawr mcp` among them.
//!
//! The owner listens on a socket at its project's [`Place`], where a shell
//! finds it from the project's root. A shell that starts a run binds that
//! socket itself and hands its listener to the run ([`handover`]), so it
//! can connect before the run is ready. The program's in-process inspector
//! ([`rawr::inspect`]) stays the agent that reads values: the session
//! forwards a shell's request to it, and hears from it where it paused.
//!
//! The protocol is JSON-RPC 2.0, one message per line each way. A shell
//! asks:
//! - `hello`: `{"protocol", "capabilities": [..], "root", "pid", "events":
//!   [..], "output": [{"stream", "text"}], "breakpoints"}`, the events so
//!   far each with its `seq`, the output kept so far and the breakpoints
//!   now, as their event tells them. From then on the connection hears
//!   each event as an `event` notification, numbered on, and each piece of
//!   output as an `output` notification: each connection has its own cursor
//!   through them. A pause, a resume and a reload reach every connection
//!   this way. A change to the breakpoints, from any shell, is told as a
//!   `breakpoints` event with no number and no place in the history:
//!   `{"event": "breakpoints", "revision", "breakpoints": [..]}`, the list
//!   the program's inspector keeps (#2278), its revision counting the
//!   changes, so a shell takes only a newer list than it has.
//! - `program`: the inspector's request (see [`rawr::inspect`]), answered
//!   with the inspector's answer once the program answers it, or the error
//!   [`NOT_RUNNING`] with the session's `state` when no program will. A
//!   request made while the program starts or restarts goes to the program
//!   that comes of it, and one whose program ended while it held it goes to
//!   the next. Control requests (`break`, `clear`, `step`, `eval` and
//!   input) reach the program one at a time, in turn; `pause` and
//!   `continue` reach it at once, so they can end a step that runs long or
//!   never pauses.
//! - `cancel` (a notification, `{"id"}`): that request ends with the error
//!   [`CANCELLED`]. A connection that closes cancels its requests.
//! - `output` (a notification, `{"stream", "text"}`): from the shell that
//!   started the run and holds its output, for the other connections.
//!
//! The inspector tells `serve` (`{"socket"}`) once it listens, which holds
//! while its connection stays open, and `emit` (an event) when the program
//! pauses and goes on and when its breakpoints change.
//!
//! In a run no shell started, the program's output reaches the session
//! another way (#2288). An owner that copies the program's output to the
//! terminal tells each piece itself ([`output()`]). A program that writes
//! to the terminal itself tells it on a connection of its own
//! ([`rawr::teller`]): it says `tell`, keeps the last of its output until the
//! first `hello`, at which the session says `listen` and waits, then tells
//! what it kept as `output`, says `told`, and tells each piece from then
//! on.
//!
//! A run no shell started registers no debugger. Its program serves its
//! inspector all the same ([`prepare`]), and a shell's first request that
//! needs the debugger has the program register one, keeping its state:
//! the engine that runs it takes it at its next step, a base program's
//! (#2297) and a generated runner's (#2308) alike. Until a
//! shell connects, the session is one thread blocked in `accept`.

mod handover;
mod host;
mod place;
mod state;

pub use handover::{adopt, handed, Handed, LISTENER};
pub use host::{open, output, prepare, record, takes_stderr, Host, CANCELLED, NOT_RUNNING};
pub use place::{root, Place};
pub use state::{Phase, State};

pub(crate) use handover::{hand, pass_on};
