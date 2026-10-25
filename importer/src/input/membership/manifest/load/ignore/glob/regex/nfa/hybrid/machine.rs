// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! hybrid::dfa::Builder::build_from_nfa, with its exact semantic capacity floor.
use super::{dfa, finalize, info, Admission, Arc, Compiler, Error, Session, Step};

#[cfg(test)]
pub(super) mod tests;

pub(super) struct Machine {
    config: Config,
    nfa: finalize::Nfa,
    stride2: usize,
    start_map: [dfa::Start; 256],
    classes: [u8; 256],
    quitset: [u128; 2],
    cache_capacity: usize,
}

// Retain the pinned fixed Config shape. These fields are only populated by
// the closed Core wrapper; no general configuration surface is introduced.
// The pinned hybrid Config keeps every field; it is admitted at its full size.
#[allow(dead_code)]
struct Config {
    match_kind: Option<info::Kind>,
    pre: Option<Option<info::Prefilter>>,
    starts_for_each_pattern: Option<bool>,
    byte_classes: Option<bool>,
    unicode_word_boundary: Option<bool>,
    quitset: Option<[u128; 2]>,
    specialize_start_states: Option<bool>,
    cache_capacity: Option<usize>,
    skip_cache_capacity_check: Option<bool>,
    minimum_cache_clear_count: Option<Option<usize>>,
    minimum_bytes_per_state: Option<Option<usize>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    CacheCapacity { minimum: usize, given: usize },
    StateIdCapacity { attempted: usize },
}

impl Machine {
    pub(super) fn nfa(&self) -> &finalize::Nfa {
        &self.nfa
    }
    pub(super) fn kind(&self) -> info::Kind {
        self.config.match_kind.unwrap_or(info::Kind::LeftmostFirst)
    }
    pub(super) fn start(&self, behind: Option<u8>) -> dfa::Start {
        behind.map_or(dfa::Start::Text, |byte| self.start_map[usize::from(byte)])
    }

    pub(super) fn state_len(&self) -> usize {
        self.nfa.0.states.len()
    }
    pub(super) fn pattern_len(&self) -> usize {
        self.nfa.0.starts.len()
    }
    pub(super) fn stride2(&self) -> usize {
        self.stride2
    }
    pub(super) fn stride(&self) -> usize {
        1 << self.stride2
    }
    pub(super) fn classes(&self) -> &[u8; 256] {
        &self.classes
    }
    pub(super) fn capacity(&self) -> usize {
        self.cache_capacity
    }
    pub(super) fn minimum_clears(&self) -> Option<usize> {
        self.config.minimum_cache_clear_count.unwrap_or(None)
    }
    pub(super) fn minimum_bytes(&self) -> Option<usize> {
        self.config.minimum_bytes_per_state.unwrap_or(None)
    }

    pub(super) fn build<A: Admission>(
        nfa: &finalize::Nfa,
        kind: info::Kind,
        capacity: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        let config = Config {
            match_kind: Some(kind),
            pre: Some(None),
            starts_for_each_pattern: Some(true),
            byte_classes: Some(true),
            unicode_word_boundary: Some(true),
            quitset: None,
            specialize_start_states: Some(false),
            cache_capacity: Some(capacity),
            skip_cache_capacity_check: Some(false),
            minimum_cache_clear_count: Some(Some(3)),
            minimum_bytes_per_state: Some(Some(10)),
        };
        // Source builder initialization precedes the NFA argument clone.
        let compiler = Compiler::new(false, session)?;
        session.items(1)?;
        let nfa = finalize::Nfa(Arc::clone(&nfa.0));
        let result = Self::from_nfa(nfa, config, session);
        drop(compiler);
        result
    }

    fn from_nfa<A: Admission>(
        nfa: finalize::Nfa,
        config: Config,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        session.items(1)?;
        // The emitted grammar contains only Start/End assertions and no
        // configured quit bytes, so the source quit set remains empty.
        if nfa.0.looks & !3 != 0 {
            return session.fail(Error::State);
        }
        let quitset = [0; 2];
        let classes = nfa.0.byte_classes(session)?;
        let minimum = minimum_capacity(&nfa, &classes, session)?;
        let capacity = config.cache_capacity.unwrap_or(2 * (1 << 20));
        session.items(1)?;
        if capacity < minimum {
            return Ok(Err(Failure::CacheCapacity {
                minimum,
                given: capacity,
            }));
        }
        // This check follows the cache floor, including its temporary dead
        // State ownership. With the pinned byte alphabet the attempted ID is
        // at most 2048 on the supported 32/64-bit targets.
        let stride = (usize::from(classes[255]) + 2).next_power_of_two();
        session.items(1)?;
        let attempted = 4 * stride;
        let max_id = if cfg!(target_pointer_width = "16") {
            (1usize << 11) - 1
        } else {
            (1usize << 27) - 1
        };
        if attempted > max_id {
            return Ok(Err(Failure::StateIdCapacity { attempted }));
        }
        let stride2 = stride.trailing_zeros() as usize;
        let start_map = dfa::Start::byte_map(nfa.0.line_terminator, session)?;
        Ok(Ok(Self {
            config,
            nfa,
            stride2,
            start_map,
            classes,
            quitset,
            cache_capacity: capacity,
        }))
    }
}

fn minimum_capacity<A: Admission>(
    nfa: &finalize::Nfa,
    classes: &[u8; 256],
    session: &mut Session<'_, A>,
) -> Step<usize> {
    session.items(1)?;
    let stride = (usize::from(classes[255]) + 2).next_power_of_two();
    let states_len = nfa.0.states.len();
    let patterns = nfa.0.starts.len();
    // Preserve the source's heuristic arithmetic, including one base start
    // group and two state-ID arrays here (the actual cache owns more).
    let sparses = states_len.checked_mul(8);
    let trans = 5 * stride * 4;
    let starts = patterns
        .checked_mul(6 * 4)
        .and_then(|n| n.checked_add(6 * 4));
    // Upstream computes this constant by actually constructing State::dead,
    // so its temporary byte Vec and Arc must both reach the original account.
    let (state_size, dead_heap) = dfa::dead_state_memory(session)?;
    let max_state = patterns
        .checked_mul(4)
        .and_then(|n| {
            states_len
                .checked_mul(5)
                .and_then(|states| n.checked_add(states))
        })
        .and_then(|n| n.checked_add(9));
    let Some((sparses, starts, max_state)) = sparses
        .zip(starts)
        .zip(max_state)
        .map(|((a, b), c)| (a, b, c))
    else {
        return session.fail(Error::Overflow);
    };
    let states = (state_size + dead_heap)
        .checked_mul(3)
        .and_then(|sentinels| {
            (state_size + max_state)
                .checked_mul(2)
                .and_then(|other| sentinels.checked_add(other))
        });
    let states_to_id = 5 * state_size + 5 * 4;
    let stack = states_len.checked_mul(4);
    let result = starts
        .checked_add(trans)
        .and_then(|n| states.and_then(|states| n.checked_add(states)))
        .and_then(|n| n.checked_add(states_to_id))
        .and_then(|n| n.checked_add(sparses))
        .and_then(|n| stack.and_then(|stack| n.checked_add(stack)))
        .and_then(|n| n.checked_add(max_state));
    match result {
        Some(size) => Ok(size),
        None => session.fail(Error::Overflow),
    }
}
