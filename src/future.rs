//! Async code at run time (D511.3, D513.3).
//!
//! An `async` block, an `async fn` call or a native async function gives a
//! [`Future`]: a value Rawr code holds and awaits. It is lazy, as Rust's
//! futures are, and a copy shares it, so it runs once.
//!
//! [`Script`] runs one as a [`std::future::Future`] that any runtime can
//! poll. A script step runs only on the thread that owns the program, the
//! runner's island server (D479, D510). Polled there, a step runs inline.
//! Polled from any other thread, the step goes to the owner as a [`Job`]
//! and the poll returns `Pending`; the owner runs it and wakes the poller.
//! A native future runs on whichever thread polls, and its output becomes a
//! Rawr value on the owner, where imported values live.
//!
//! A step that finds its code changed since it suspended fails with the
//! machine's own message (`eval::machine`), and the [`Script`] polling it
//! panics with it, as a Rust task that panics.

pub use crate::eval::machine::{Frame, Poll};

#[cfg(feature = "std")]
pub mod facade;
#[cfg(feature = "std")]
mod runtime;
#[cfg(feature = "std")]
pub use runtime::*;

#[cfg(all(test, feature = "std"))]
mod tests;
