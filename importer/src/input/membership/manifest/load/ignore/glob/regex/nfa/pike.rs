// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! regex-automata 0.4.14 meta::wrappers::PikeVM construction and cache lifecycle.
//! Private construction, Boolean and overlapping search; no membership authority.
use super::{compile::Compiler, finalize, info, storage};
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::Admission;
use std::sync::Arc;

pub(super) mod cache;
mod search;
pub(super) mod set;

pub(super) struct Pike {
    kind: info::Kind,
    nfa: finalize::Nfa,
}

#[cfg(test)]
mod tests;

impl Pike {
    pub(super) fn new<A: Admission>(
        info: &info::Info,
        nfa: &finalize::Nfa,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let kind = info.kind();
        // PikeVM::Builder::new constructs this even for build_from_nfa. In
        // particular, the RangeTrie allocates before the supplied NFA clone.
        let builder = Compiler::new(false, session)?;
        session.items(1)?;
        let nfa = finalize::Nfa(Arc::clone(&nfa.0));
        session.items(1)?;
        // Only Start/End looks are emitted. The source availability check has
        // no Unicode-word-data failure in this closed grammar. A different
        // producer is an internal contract error, never a rejected Cargo glob.
        if nfa.0.looks & !3 != 0 || info.pattern_len() != nfa.0.starts.len() {
            return session.fail(Error::State);
        }
        let engine = Self { kind, nfa };
        drop(builder);
        Ok(engine)
    }

    pub(super) fn create_cache<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<cache::Lazy> {
        session.items(1)?;
        Ok(cache::Lazy::none())
    }

    /// Search the whole borrowed input with the source's earliest semantics.
    pub(super) fn is_match<A: Admission>(
        &self,
        lazy: &mut cache::Lazy,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        self.earliest(lazy, bytes, session)
            .map(|pattern| pattern.is_some())
    }
}
