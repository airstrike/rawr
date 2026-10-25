//! rawr-import end to end: a plain program with a Cargo dependency runs in
//! a runner with no iced, and that runner builds the project's own units; a
//! native `main` calls marked code that a save swaps; an async `main` runs on
//! the project's own tokio, and a save restarts it.

mod erasure;
mod islands;
mod plain;
mod support;
mod tokio;
mod unit_graph;
