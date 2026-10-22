// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Pinned Core reverse NFA and private forward/reverse dense construction.
//! Only completed construction exposes the closed globset query boundary.
use super::super::translate::Expression;
use super::{builder, compile::Compiler, finalize, info};
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::Admission;

mod finish;
mod initial;
pub(super) mod intern;
pub(super) mod key;
mod search;
mod subset;
#[cfg(test)]
mod tests;

pub(super) use initial::Start;
pub(super) use search::Failure;

/// hybrid::minimum_cache_capacity actually constructs this temporary State.
/// Keep its admitted encoding/Arc ownership shared with dense construction.
pub(super) fn dead_state_memory<A: Admission>(
    session: &mut Session<'_, A>,
) -> Step<(usize, usize)> {
    let dead = key::State::dead(session)?;
    Ok((std::mem::size_of::<key::State>(), dead.bytes().len()))
}

pub(super) struct Prepared<'n> {
    reverse: finalize::Nfa,
    forward: Forward<'n>,
}

enum Forward<'n> {
    // Only disabled configuration, the existing 30-state heuristic or a source
    // semantic construction failure can decline. Pending never means decline.
    Skipped,
    Rejected(initial::Failure),
    Pending(Pending<'n>),
    Unshuffled(Unshuffled<'n>),
    Built(Built),
}

struct Built {
    forward: initial::Initial,
    reverse: initial::Initial,
}

/// Completed dense construction, with semantic decline distinct from pending.
pub(super) struct Dfa {
    completed: Option<Built>,
}

impl Dfa {
    pub(super) fn none() -> Self {
        Self { completed: None }
    }

    pub(super) fn has_engine(&self) -> bool {
        self.completed.is_some()
    }
}

struct Unshuffled<'n> {
    pending: Pending<'n>,
    subsets: subset::Complete,
}

struct Pending<'n> {
    nfa: &'n finalize::Nfa,
    config: Config,
    dense: initial::Initial,
    // The dense Builder temporary lives through build_from_nfa, including
    // determinization/finishing. Retain its eager compiler until completion.
    _compiler: Compiler,
}

struct Config {
    kind: info::Kind,
    anchored_only: bool,
    size_limit: Option<usize>,
}

impl<'n> Prepared<'n> {
    /// Consume every construction continuation before returning Core's parts.
    pub(super) fn complete<A: Admission>(
        self,
        session: &mut Session<'_, A>,
    ) -> Step<(finalize::Nfa, Dfa)> {
        let complete = self.determinize(session)?.finish(session)?;
        session.items(1)?;
        let completed = match complete.forward {
            Forward::Built(pair) => Some(pair),
            Forward::Skipped | Forward::Rejected(_) => None,
            Forward::Pending(_) | Forward::Unshuffled(_) => {
                return session.fail(Error::State);
            }
        };
        Ok((complete.reverse, Dfa { completed }))
    }

    /// Advance the existing forward attempt to its complete subset graph.
    /// This is still a private unfinished constructor, with no search/fallback
    /// API. The next source operation is dense::DFA::shuffle.
    fn determinize<A: Admission>(mut self, session: &mut Session<'_, A>) -> Step<Self> {
        session.check()?;
        self.forward = match self.forward {
            Forward::Pending(mut pending) => {
                match subset::run(
                    pending.nfa,
                    &mut pending.dense,
                    pending.config.kind,
                    pending.config.size_limit,
                    pending.config.size_limit,
                    session,
                )? {
                    Ok(subsets) => Forward::Unshuffled(Unshuffled { pending, subsets }),
                    Err(error) => Forward::Rejected(error),
                }
            }
            Forward::Unshuffled(_) | Forward::Built(_) => return session.fail(Error::State),
            state => state,
        };
        Ok(self)
    }

    /// Finish both dense builders in the source's forward-then-reverse order.
    /// A semantic construction failure retains the reverse NFA for the later
    /// Core continuation; an admission refusal terminates the original Session.
    fn finish<A: Admission>(mut self, session: &mut Session<'_, A>) -> Step<Self> {
        session.check()?;
        self.forward = match self.forward {
            Forward::Unshuffled(stage) => match finish_pair(stage, &self.reverse, session)? {
                Ok(pair) => Forward::Built(pair),
                Err(error) => Forward::Rejected(error),
            },
            Forward::Pending(_) | Forward::Built(_) => return session.fail(Error::State),
            state => state,
        };
        Ok(self)
    }

    pub(super) fn begin<A: Admission>(
        info: &info::Info,
        nfa: &'n finalize::Nfa,
        expressions: &[Expression],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, builder::Failure>> {
        Self::begin_with_nfa_limit(info, nfa, expressions, info.nfa_size_limit(), session)
    }

    fn begin_with_nfa_limit<A: Admission>(
        info: &info::Info,
        nfa: &'n finalize::Nfa,
        expressions: &[Expression],
        size_limit: Option<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, builder::Failure>> {
        // Core builds the reverse NFA before the DFA wrapper examines the
        // forward state count, even when that wrapper will decline immediately.
        let mut compiler = Compiler::new(true, session)?;
        let reverse = match compiler.build(expressions, size_limit, session)? {
            Ok(reverse) => reverse,
            Err(error) => return Ok(Err(error)),
        };
        drop(compiler);
        // Core examines its enabled-DFA gate after the reverse NFA exists.
        // A disabled engine never enters the wrapper's forward-NFA checks.
        session.items(1)?;
        if !info.dfa() {
            return Ok(Ok(Self {
                reverse,
                forward: Forward::Skipped,
            }));
        }
        if nfa.0.reverse || info.pattern_len() != nfa.0.starts.len() {
            return session.fail(Error::State);
        }
        let forward = if info
            .dfa_state_limit()
            .is_some_and(|limit| nfa.0.states.len() > limit)
        {
            Forward::Skipped
        } else {
            let config = Config {
                kind: info.kind(),
                anchored_only: false,
                size_limit: info.dfa_size_limit().map(|limit| limit / 4),
            };
            match Pending::begin(nfa, config, session)? {
                Ok(pending) => Forward::Pending(pending),
                Err(error) => Forward::Rejected(error),
            }
        };
        Ok(Ok(Self { reverse, forward }))
    }
}

impl<'n> Pending<'n> {
    fn finish<A: Admission>(
        mut self,
        subsets: subset::Complete,
        session: &mut Session<'_, A>,
    ) -> Step<Result<initial::Initial, initial::Failure>> {
        // Runner, including its retained cache, survives shuffle, but drops
        // before acceleration. The eager dense Builder compiler survives both.
        if let Err(error) = subsets.shuffle(&mut self.dense, session)? {
            return Ok(Err(error));
        }
        finish::rest(&mut self.dense, session)?;
        Ok(Ok(self.dense))
    }

    fn begin<A: Admission>(
        nfa: &'n finalize::Nfa,
        config: Config,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, initial::Failure>> {
        // dense::Builder::new reaches Compiler::new before build_from_nfa.
        let compiler = Compiler::new(false, session)?;
        let dense = match initial::Initial::new(nfa, config.anchored_only, session)? {
            Ok(dense) => dense,
            Err(error) => return Ok(Err(error)),
        };
        Ok(Ok(Self {
            nfa,
            config,
            dense,
            _compiler: compiler,
        }))
    }
}

fn finish_pair<A: Admission>(
    stage: Unshuffled<'_>,
    reverse_nfa: &finalize::Nfa,
    session: &mut Session<'_, A>,
) -> Step<Result<Built, initial::Failure>> {
    let size_limit = stage.pending.config.size_limit;
    let forward = match stage.pending.finish(stage.subsets, session)? {
        Ok(forward) => forward,
        Err(error) => return Ok(Err(error)),
    };
    let mut pending = match Pending::begin(
        reverse_nfa,
        Config {
            kind: info::Kind::All,
            anchored_only: true,
            size_limit,
        },
        session,
    )? {
        Ok(pending) => pending,
        Err(error) => return Ok(Err(error)),
    };
    let subsets = match subset::run(
        pending.nfa,
        &mut pending.dense,
        pending.config.kind,
        size_limit,
        size_limit,
        session,
    )? {
        Ok(subsets) => subsets,
        Err(error) => return Ok(Err(error)),
    };
    let reverse = match pending.finish(subsets, session)? {
        Ok(reverse) => reverse,
        Err(error) => return Ok(Err(error)),
    };
    // regex::Builder::new eagerly makes another dense::Builder/Compiler even
    // when build_from_dfas only moves the already-built pair into Regex.
    let compiler = Compiler::new(false, session)?;
    let pair = Built { forward, reverse };
    drop(compiler);
    Ok(Ok(pair))
}
