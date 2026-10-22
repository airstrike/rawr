// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! regex-automata 0.4.14 meta::wrappers::BoundedBacktracker construction/cache.
//! Private partial Core construction and Boolean search; no membership authority.
use super::{compile::Compiler, finalize, info};
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::Admission;
use std::sync::Arc;

pub(super) mod cache;
mod search;
#[cfg(test)]
mod tests;

pub(super) struct Backtrack(Option<Engine>);
pub(super) struct Engine {
    config: Config,
    nfa: finalize::Nfa,
}

// Retain the pinned fixed Config shape. Globset supplies prefilter(None) and
// leaves visited_capacity unset; the source default is a semantic engine limit,
// distinct from and subordinate to the original cumulative Session admission.
struct Config {
    pre: Option<Option<info::Prefilter>>,
    visited_capacity: Option<usize>,
}

impl Backtrack {
    pub(super) fn new<A: Admission>(
        info: &info::Info,
        nfa: &finalize::Nfa,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        if info.kind() != info::Kind::LeftmostFirst {
            return Ok(Self(None));
        }
        let config = Config {
            pre: Some(None),
            visited_capacity: None,
        };
        // Builder::new constructs its compiler before build_from_nfa clones
        // the supplied NFA. Even this otherwise unused RangeTrie is reached.
        let builder = Compiler::new(false, session)?;
        session.items(1)?;
        let nfa = finalize::Nfa(Arc::clone(&nfa.0));
        session.items(1)?;
        // The closed emitted grammar has only Start/End assertions. A changed
        // producer must extend the contained availability check before use.
        if nfa.0.looks & !3 != 0 || info.pattern_len() != nfa.0.starts.len() {
            return session.fail(Error::State);
        }
        let engine = Engine { config, nfa };
        drop(builder);
        Ok(Self(Some(engine)))
    }

    pub(super) fn create_cache<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<cache::Lazy> {
        session.items(1)?;
        Ok(cache::Lazy::none())
    }

    /// A skipped engine remains distinct from a completed non-match. Core must
    /// still select full/lazy DFA first, and its own fallback after a skip.
    pub(super) fn is_match<A: Admission>(
        &self,
        cache: &mut cache::Lazy,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Option<bool>> {
        let Some(engine) = self.get(bytes.len(), bytes.len(), true, session)? else {
            return Ok(None);
        };
        match engine.search(cache, bytes, session)? {
            Ok(hit) => Ok(Some(hit.is_some())),
            // The wrapper's span guard guarantees this source failure cannot
            // occur; it must never turn an internal defect into a non-match.
            Err(_) => session.fail(Error::State),
        }
    }

    /// The wrapper reads total haystack length for earliest, then span length
    /// for capacity. These scalars must come from the same borrowed Input.
    pub(super) fn get<A: Admission>(
        &self,
        haystack_len: usize,
        span_len: usize,
        earliest: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Option<&Engine>> {
        session.items(1)?;
        let Some(engine) = self.0.as_ref() else {
            return Ok(None);
        };
        if earliest && haystack_len > 128 {
            return Ok(None);
        }
        session.items(1)?;
        if span_len > engine.max_haystack_len(session)? {
            return Ok(None);
        }
        Ok(Some(engine))
    }
}

impl Engine {
    fn visited_capacity(&self) -> usize {
        self.config.visited_capacity.unwrap_or(256 * (1 << 10))
    }

    fn max_haystack_len<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<usize> {
        session.items(1)?;
        let Some(capacity) = self.visited_capacity().checked_mul(8) else {
            return session.fail(Error::Overflow);
        };
        let blocks = capacity.div_ceil(cache::Visited::BLOCK_SIZE);
        let capacity = blocks.saturating_mul(cache::Visited::BLOCK_SIZE);
        let states = self.nfa.0.states.len();
        if states == 0 {
            return session.fail(Error::State);
        }
        Ok((capacity / states).saturating_sub(1))
    }
}
