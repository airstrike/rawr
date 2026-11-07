//! Native: a library module that calls the marked one.
use crate::score::{label, Game};

/// The line the binary prints for `game`.
pub fn line(game: &Game) -> String {
    format!("score: {}", label(game))
}
