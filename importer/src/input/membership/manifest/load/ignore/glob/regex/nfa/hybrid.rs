// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Private meta::wrappers::HybridEngine, retained cache and closed forward queries.
use super::{compile::Compiler, dfa, finalize, info, pike};
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::Admission;
use std::sync::Arc;

mod cache;
mod intern;
mod machine;

pub(super) struct Hybrid(Attempt);

// Built once per regex and matched per search; boxing would add a hop to each search.
#[allow(clippy::large_enum_variant)]
enum Attempt {
    Skipped,
    Rejected(machine::Failure),
    Built(Pair),
}

struct Pair {
    forward: machine::Machine,
    reverse: machine::Machine,
}

pub(super) struct Cache(Option<Caches>);
struct Caches {
    forward: cache::Cache,
    reverse: cache::Cache,
}

/// A source semantic failure permits Core's prescribed fallback. A Session
/// refusal is always the outer Step error and cannot enter this type.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    GaveUp { offset: usize },
}

impl Hybrid {
    pub(super) fn none() -> Self {
        Self(Attempt::Skipped)
    }

    pub(super) fn has_engine(&self) -> bool {
        matches!(self.0, Attempt::Built(_))
    }

    #[cfg(test)]
    pub(super) fn test_minimum_capacity(
        info: &info::Info,
        forward: &finalize::Nfa,
        reverse: &finalize::Nfa,
    ) -> usize {
        use crate::input::membership::manifest::reference::Account;
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut minimum = 0;
        for (nfa, kind) in [(forward, info.kind()), (reverse, info::Kind::All)] {
            match machine::Machine::build(nfa, kind, 0, &mut session).unwrap() {
                Err(machine::Failure::CacheCapacity {
                    minimum: required, ..
                }) => {
                    minimum = minimum.max(required);
                }
                _ => panic!("zero capacity must report the pinned source floor"),
            }
        }
        minimum
    }

    pub(super) fn create_cache<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Cache> {
        session.items(1)?;
        let Attempt::Built(pair) = &self.0 else {
            return Ok(Cache(None));
        };
        let forward = cache::Cache::new(&pair.forward, session)?;
        let reverse = cache::Cache::new(&pair.reverse, session)?;
        Ok(Cache(Some(Caches { forward, reverse })))
    }

    pub(super) fn is_match<A: Admission>(
        &self,
        cache: &mut Cache,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Result<bool, Failure>> {
        session.items(1)?;
        let (Attempt::Built(pair), Some(caches)) = (&self.0, &mut cache.0) else {
            return session.fail(Error::State);
        };
        caches.forward.is_match(&pair.forward, bytes, session)
    }

    pub(super) fn overlapping<A: Admission>(
        &self,
        cache: &mut Cache,
        bytes: &[u8],
        set: &mut pike::set::Set,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        session.items(1)?;
        let (Attempt::Built(pair), Some(caches)) = (&self.0, &mut cache.0) else {
            return session.fail(Error::State);
        };
        caches
            .forward
            .overlapping(&pair.forward, bytes, set, session)
    }

    pub(super) fn new<A: Admission>(
        info: &info::Info,
        nfa: &finalize::Nfa,
        reverse: &finalize::Nfa,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        Self::with_capacity(info, nfa, reverse, info.hybrid_cache_capacity(), session)
    }

    fn with_capacity<A: Admission>(
        info: &info::Info,
        nfa: &finalize::Nfa,
        reverse: &finalize::Nfa,
        capacity: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        if !info.hybrid() {
            return Ok(Self(Attempt::Skipped));
        }
        if nfa.0.reverse
            || !reverse.0.reverse
            || info.pattern_len() != nfa.0.starts.len()
            || info.pattern_len() != reverse.0.starts.len()
        {
            return session.fail(Error::State);
        }
        let forward = match machine::Machine::build(nfa, info.kind(), capacity, session)? {
            Ok(forward) => forward,
            Err(error) => return Ok(Self(Attempt::Rejected(error))),
        };
        let reverse = match machine::Machine::build(reverse, info::Kind::All, capacity, session)? {
            Ok(reverse) => reverse,
            Err(error) => return Ok(Self(Attempt::Rejected(error))),
        };
        // hybrid::regex::Builder::new eagerly creates a hybrid DFA builder,
        // even though build_from_dfas only transfers these two machines.
        let compiler = Compiler::new(false, session)?;
        let pair = Pair { forward, reverse };
        drop(compiler);
        Ok(Self(Attempt::Built(pair)))
    }
}

impl Cache {
    pub(super) fn reset<A: Admission>(
        &mut self,
        hybrid: &Hybrid,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if let Attempt::Built(pair) = &hybrid.0 {
            let Some(caches) = &mut self.0 else {
                return session.fail(Error::State);
            };
            caches.forward.reset(&pair.forward, session)?;
            caches.reverse.reset(&pair.reverse, session)?;
        }
        Ok(())
    }
}
