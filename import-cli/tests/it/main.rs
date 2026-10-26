//! rawr-import end to end: a plain program with a Cargo dependency runs in
//! a runner with no iced, and that runner builds the project's own units; a
//! native `main` calls marked code that a save swaps; an async `main` runs on
//! the project's own tokio, and a save restarts it; a marked `main` in a
//! native loop takes a body edit at its next call into script code.

mod erasure;
mod islands;
mod panes;
mod plain;
mod scope;
mod support;
mod swap;
mod tokio;
#[cfg(unix)]
mod tui;
mod unit_graph;
