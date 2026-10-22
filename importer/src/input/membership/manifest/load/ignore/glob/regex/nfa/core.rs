// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Complete private meta::strategy::Core construction, cache and closed queries.
use super::super::translate::Expression;
use super::{backtrack, builder, compile::Compiler, dfa, finalize, hybrid, info, pike};
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::Admission;

pub(super) mod cache;
mod search;
#[cfg(test)]
mod tests;

/// Completed engine ownership. Unfinished dense attempts cannot inhabit Core.
pub(super) struct Core {
    info: info::Info,
    pre: Option<info::Prefilter>,
    nfa: finalize::Nfa,
    reverse: Option<finalize::Nfa>,
    pike: pike::Pike,
    backtrack: backtrack::Backtrack,
    onepass: Onepass,
    hybrid: hybrid::Hybrid,
    dfa: dfa::Dfa,
}

/// The actual source wrapper outcomes for the closed emitted grammar.
#[derive(Debug, PartialEq, Eq)]
enum Onepass {
    Disabled,
    NotUseful,
}

impl Core {
    pub(super) fn create_cache<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<cache::Cache> {
        cache::Cache::new(self, session)
    }

    /// Consume the RegexInfo collected from these same admitted expressions.
    pub(super) fn new<A: Admission>(
        info: info::Info,
        expressions: &[Expression],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, builder::Failure>> {
        session.check()?;
        let mut compiler = Compiler::new(false, session)?;
        let nfa = match compiler.build(expressions, info.nfa_size_limit(), session)? {
            Ok(nfa) => nfa,
            Err(error) => return Ok(Err(error)),
        };
        // The source's forward Compiler temporary ends before Pike construction.
        drop(compiler);
        let pike = pike::Pike::new(&info, &nfa, session)?;
        let backtrack = backtrack::Backtrack::new(&info, &nfa, session)?;
        let onepass = Onepass::new(&info, session)?;
        let (reverse, hybrid, dfa) = if !info.hybrid() && !info.dfa() {
            (None, hybrid::Hybrid::none(), dfa::Dfa::none())
        } else {
            // Core builds the reverse NFA before the dense wrapper's heuristic,
            // even when dense construction will immediately decline.
            let pending = match dfa::Prepared::begin(&info, &nfa, expressions, session)? {
                Ok(pending) => pending,
                Err(error) => return Ok(Err(error)),
            };
            let (reverse, dfa) = pending.complete(session)?;
            let hybrid = if !info.hybrid() || dfa.has_engine() {
                hybrid::Hybrid::none()
            } else {
                hybrid::Hybrid::new(&info, &nfa, &reverse, session)?
            };
            (Some(reverse), hybrid, dfa)
        };
        Ok(Ok(Self {
            info,
            pre: None,
            nfa,
            reverse,
            pike,
            backtrack,
            onepass,
            hybrid,
            dfa,
        }))
    }
}

impl Onepass {
    fn new<A: Admission>(info: &info::Info, session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        if !info.onepass() {
            return Ok(Self::Disabled);
        }
        if !info.onepass_candidate() {
            return Ok(Self::NotUseful);
        }
        // Only noncapturing groups and Start/End assertions can be emitted.
        // A different producer requires the actual one-pass builder; it cannot
        // silently become a semantic decline or a partial accepted matcher.
        session.fail(Error::State)
    }
}
