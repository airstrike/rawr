// Copyright the aho-corasick contributors. MIT OR Unlicense.
//! The pinned default engine choice and its actual shared allocation.
use std::{
    alloc::Layout,
    sync::{atomic::AtomicUsize, Arc},
};

use rawr_toml::document::{Admission, Request};

use super::{contiguous, dfa, noncontiguous, prefilter::Prefilter, Abort, Failure};
use crate::input::membership::manifest::{session::Session, Error, Step};

// The enum closes the source trait object's three possible concrete owners;
// each selected payload still has its own source Arc allocation.
pub(in super::super) enum Engine {
    Dfa(Arc<dfa::Dfa>),
    Contiguous(Arc<contiguous::Nfa>),
    Noncontiguous(Arc<noncontiguous::Nfa>),
}

impl Engine {
    pub(in super::super) fn build<A: Admission>(
        patterns: &[String],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        let nfa = match super::prepare(patterns, session)? {
            Ok(nfa) => nfa,
            Err(failure) => return Ok(Err(failure)),
        };
        // Default StartKind is Unanchored. A conversion's semantic size error
        // permits fallback; a refusal from the original account never does.
        if nfa.pattern_lens().len() <= 100 {
            match dfa::Dfa::build(&nfa, session) {
                Ok(dfa) => return Ok(Ok(Self::Dfa(shared(dfa, session)?))),
                Err(Abort::Stop(stop)) => return Err(stop),
                Err(Abort::Semantic(_)) => {}
            }
        }
        match contiguous::Nfa::build(&nfa, session) {
            Ok(contiguous) => return Ok(Ok(Self::Contiguous(shared(contiguous, session)?))),
            Err(Abort::Stop(stop)) => return Err(stop),
            Err(Abort::Semantic(_)) => {}
        }
        Ok(Ok(Self::Noncontiguous(shared(nfa, session)?)))
    }

    pub(super) fn start(&self) -> u32 {
        match self {
            Self::Dfa(dfa) => dfa.start(),
            Self::Contiguous(nfa) => nfa.start(),
            Self::Noncontiguous(nfa) => nfa.special().start,
        }
    }

    pub(super) fn is_special(&self, id: u32) -> bool {
        match self {
            Self::Dfa(dfa) => dfa.is_special(id),
            Self::Contiguous(nfa) => nfa.is_special(id),
            Self::Noncontiguous(nfa) => id <= nfa.special().max_special,
        }
    }

    pub(super) fn is_match(&self, id: u32) -> bool {
        match self {
            Self::Dfa(dfa) => dfa.is_match(id),
            Self::Contiguous(nfa) => nfa.is_match(id),
            Self::Noncontiguous(nfa) => id != 0 && id <= nfa.special().max_match,
        }
    }

    pub(super) fn next_state<A: Admission>(
        &self,
        id: u32,
        byte: u8,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        match self {
            Self::Dfa(dfa) => dfa.next_state(id, byte, session),
            Self::Contiguous(nfa) => nfa.next_state(id, byte, session),
            Self::Noncontiguous(nfa) => nfa.next_state(id, byte, session),
        }
    }

    pub(super) fn pattern_len(&self, pattern: u32) -> usize {
        match self {
            Self::Dfa(dfa) => dfa.pattern_len(pattern),
            Self::Contiguous(nfa) => nfa.pattern_len(pattern),
            Self::Noncontiguous(nfa) => nfa.pattern_lens()[pattern as usize] as usize,
        }
    }

    pub(super) fn match_len<A: Admission>(
        &self,
        id: u32,
        session: &mut Session<'_, A>,
    ) -> Step<usize> {
        match self {
            Self::Dfa(dfa) => Ok(dfa.match_len(id)),
            Self::Contiguous(nfa) => Ok(nfa.match_len(id)),
            Self::Noncontiguous(nfa) => {
                let mut matches = nfa.matches(id);
                let mut length = 0;
                while matches.next(session)?.is_some() {
                    length += 1;
                }
                Ok(length)
            }
        }
    }

    pub(super) fn match_pattern<A: Admission>(
        &self,
        id: u32,
        index: usize,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        match self {
            Self::Dfa(dfa) => Ok(dfa.match_pattern(id, index)),
            Self::Contiguous(nfa) => Ok(nfa.match_pattern(id, index)),
            Self::Noncontiguous(nfa) => {
                let mut matches = nfa.matches(id);
                for position in 0..=index {
                    let Some(pattern) = matches.next(session)? else {
                        return session.fail(Error::State);
                    };
                    if position == index {
                        return Ok(pattern);
                    }
                }
                session.fail(Error::State)
            }
        }
    }

    pub(super) fn prefilter(&self) -> Option<&Prefilter> {
        match self {
            Self::Dfa(dfa) => dfa.prefilter(),
            Self::Contiguous(nfa) => nfa.prefilter(),
            Self::Noncontiguous(nfa) => nfa.prefilter(),
        }
    }
}

fn shared<T, A: Admission>(value: T, session: &mut Session<'_, A>) -> Step<Arc<T>> {
    let Ok((layout, _)) = Layout::new::<[AtomicUsize; 2]>().extend(Layout::new::<T>()) else {
        return session.fail(Error::Overflow);
    };
    let layout = layout.pad_to_align();
    session.admit(Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    Ok(Arc::new(value))
}
