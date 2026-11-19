//! A terminal debugger shell on a run's shared debugger session (D1053):
//! the run's phase and output, the source where the program paused with
//! its breakpoints, the paused calls and their bindings, and an eval
//! prompt. It attaches to the session as any shell does, so it keeps
//! working beside other shells that drive the same run.
//!
//! Keys: `b` sets or clears a breakpoint at the cursor's line and `B`
//! asks where to break; `p` pauses a running program; `s` steps over,
//! `i` into, `o` out, and `c` continues; `e` evaluates Rust where the
//! program is paused; Tab moves between the source, the frames and the
//! bindings, where the arrows move and Enter opens a value; `q` quits.

#![cfg(unix)]

pub mod client;
pub mod phase;
pub mod shell;
mod terminal;
pub mod value;

pub use shell::{Flow, Input, Shell, Stream};
pub use terminal::{run, Session};
