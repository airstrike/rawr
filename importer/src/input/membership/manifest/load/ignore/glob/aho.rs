// Copyright the aho-corasick contributors. MIT OR Unlicense.
//! Closed default Aho-Corasick construction and Standard overlapping queries.
use rawr_toml::document::Admission;

use crate::input::membership::manifest::{session::Session, Step, Stop};

mod contiguous;
mod dfa;
mod engine;
mod noncontiguous;
mod prefilter;
pub(super) mod search;
mod storage;

pub(super) use engine::Engine;

#[cfg(test)]
mod tests;

// aho-corasick 1.1.4 SmallIndex leaves one positive value for one_more().
const MAX: usize = if usize::BITS >= 32 {
    i32::MAX as usize - 1
} else {
    isize::MAX as usize - 1
};

#[derive(Debug, Eq, PartialEq)]
pub(in super::super::super) enum Failure {
    StateId { attempted: usize },
    PatternId { attempted: usize },
    PatternLength { pattern: u32, length: usize },
}

enum Abort {
    Stop(Stop),
    Semantic(Failure),
}
impl From<Stop> for Abort {
    fn from(stop: Stop) -> Self {
        Self::Stop(stop)
    }
}
type Trial<T> = Result<T, Abort>;

/// Keep semantic build failure distinct from the original terminal refusal.
fn prepare<A: Admission>(
    patterns: &[String],
    session: &mut Session<'_, A>,
) -> Step<Result<noncontiguous::Nfa, Failure>> {
    match noncontiguous::Nfa::build(patterns, session) {
        Ok(nfa) => Ok(Ok(nfa)),
        Err(Abort::Semantic(failure)) => Ok(Err(failure)),
        Err(Abort::Stop(stop)) => Err(stop),
    }
}

fn state_id(index: usize) -> Trial<u32> {
    if index > MAX {
        return Err(Abort::Semantic(Failure::StateId { attempted: index }));
    }
    Ok(index as u32)
}
