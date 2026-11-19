//! A run's debugger session as the shell reaches it: the JSON-RPC client,
//! the phase its events move, and the transport that carries its lines.

mod client;
mod phase;
mod transport;
#[cfg(unix)]
mod unix;

pub use client::{Client, Error, Incoming, NOT_RUNNING};
pub use phase::{Phase, Stop};
pub use transport::{Event, Link, Transport};
#[cfg(unix)]
pub use unix::connect;
