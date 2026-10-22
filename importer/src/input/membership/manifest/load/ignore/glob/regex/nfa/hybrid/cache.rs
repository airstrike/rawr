// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Private hybrid::dfa cache ownership, initialization and retained epochs.
use super::super::storage::Array;
use super::{dfa::key, intern, machine::Machine, Admission, Error, Session, Step};

mod search;
mod sparse;
mod states;
#[cfg(test)]
mod tests;
mod transition;

#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
const HIGH_BIT: u32 = 31;
#[cfg(target_pointer_width = "16")]
const HIGH_BIT: u32 = 15;
const UNKNOWN: u32 = 1 << HIGH_BIT;
const DEAD: u32 = 1 << (HIGH_BIT - 1);
const QUIT: u32 = 1 << (HIGH_BIT - 2);
const START: u32 = 1 << (HIGH_BIT - 3);
const MATCH: u32 = 1 << (HIGH_BIT - 4);
const MAX_ID: u32 = MATCH - 1;

pub(super) struct Cache {
    trans: Array<u32>,
    starts: Array<u32>,
    states: states::States,
    interner: intern::Interner,
    sparses: sparse::Sets,
    stack: Array<u32>,
    builder: key::Empty,
    saver: Saver,
    memory_states: usize,
    clear_count: usize,
    bytes_searched: usize,
    progress: Option<Progress>,
}

enum Saver {
    None,
    ToSave { id: u32, state: key::State },
    Saved(u32),
}
struct Progress {
    start: usize,
    at: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    BadEfficiency,
    TooManyClears,
}

#[derive(Clone, Copy)]
pub(super) enum Tag {
    None,
    Unknown,
    Dead,
    Quit,
    Start,
}
impl Tag {
    fn bits(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Unknown => UNKNOWN,
            Self::Dead => DEAD,
            Self::Quit => QUIT,
            Self::Start => START,
        }
    }
}

impl Cache {
    pub(super) fn new<A: Admission>(dfa: &Machine, session: &mut Session<'_, A>) -> Step<Self> {
        session.check()?;
        let mut cache = Self {
            trans: Array::new(),
            starts: Array::new(),
            states: states::States::new(),
            interner: intern::Interner::new(),
            sparses: sparse::Sets::new(dfa.state_len(), session)?,
            stack: Array::new(),
            builder: key::Empty::new(),
            saver: Saver::None,
            memory_states: 0,
            clear_count: 0,
            bytes_searched: 0,
            progress: None,
        };
        cache.init(dfa, session)?;
        Ok(cache)
    }

    fn init<A: Admission>(&mut self, dfa: &Machine, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        let Some(length) = dfa
            .pattern_len()
            .checked_mul(6)
            .and_then(|n| n.checked_add(12))
        else {
            return session.fail(Error::Overflow);
        };
        self.extend_starts(length, session)?;
        let dead = key::State::dead(session)?;
        let unknown = self.add_state(dfa, dead.share(session)?, Tag::Unknown, session)?;
        let Ok(unknown) = unknown else {
            return session.fail(Error::State);
        };
        let dead_id = self.add_state(dfa, dead.share(session)?, Tag::Dead, session)?;
        let Ok(dead_id) = dead_id else {
            return session.fail(Error::State);
        };
        let quit = self.add_state(dfa, dead.share(session)?, Tag::Quit, session)?;
        let Ok(quit) = quit else {
            return session.fail(Error::State);
        };
        if unknown != UNKNOWN
            || dead_id != DEAD | dfa.stride() as u32
            || quit != QUIT | (2 * dfa.stride()) as u32
        {
            return session.fail(Error::State);
        }
        self.self_loop(dfa, unknown, session)?;
        self.self_loop(dfa, dead_id, session)?;
        self.self_loop(dfa, quit, session)?;
        self.interner.insert(dead, dead_id, session)
    }

    fn extend_starts<A: Admission>(
        &mut self,
        length: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.starts.reserve(length, session)?;
        session.items(length)?;
        self.starts
            .values
            .extend(std::iter::repeat_n(UNKNOWN, length));
        Ok(())
    }

    fn self_loop<A: Admission>(
        &mut self,
        dfa: &Machine,
        id: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let offset = (id & MAX_ID) as usize;
        let mut previous = None;
        // Match ByteClassRepresentatives' byte-by-byte traversal and EOI.
        for byte in 0..256 {
            session.items(1)?;
            let class = dfa.classes()[byte];
            if previous != Some(class) {
                session.items(1)?;
                self.trans.values[offset + usize::from(class)] = id;
                previous = Some(class);
            }
        }
        session.items(1)?;
        self.trans.values[offset + usize::from(dfa.classes()[255]) + 1] = id;
        Ok(())
    }

    pub(super) fn take_builder<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<key::Empty> {
        session.items(1)?;
        Ok(std::mem::replace(&mut self.builder, key::Empty::new()))
    }

    pub(super) fn insert_builder<A: Admission>(
        &mut self,
        dfa: &Machine,
        builder: key::Nfa,
        tag: Tag,
        session: &mut Session<'_, A>,
    ) -> Step<Result<u32, Failure>> {
        if let Some(id) = self.interner.get(builder.bytes(), session)? {
            self.builder = builder.clear(session)?;
            return Ok(Ok(id));
        }
        let state = builder.shared(session)?;
        let result = self.add_state(dfa, state, tag, session)?;
        self.builder = builder.clear(session)?;
        Ok(result)
    }

    pub(super) fn add_state<A: Admission>(
        &mut self,
        dfa: &Machine,
        state: key::State,
        tag: Tag,
        session: &mut Session<'_, A>,
    ) -> Step<Result<u32, Failure>> {
        if !self.fits(dfa, state.bytes().len(), session)?
            && let Err(error) = self.try_clear(dfa, session)?
        {
            return Ok(Err(error));
        }
        // Source ID capacity handling follows the semantic fit/clear attempt.
        session.items(1)?;
        if self.trans.values.len() > MAX_ID as usize {
            if let Err(error) = self.try_clear(dfa, session)? {
                return Ok(Err(error));
            }
            if self.trans.values.len() > MAX_ID as usize {
                return session.fail(Error::State);
            }
        }
        let mut id = self.trans.values.len() as u32 | tag.bits();
        if state.bytes()[0] & 1 != 0 {
            id |= MATCH;
        }
        self.trans.reserve(dfa.stride(), session)?;
        session.items(dfa.stride())?;
        self.trans
            .values
            .extend(std::iter::repeat_n(UNKNOWN, dfa.stride()));
        // The closed grammar has no quit bytes; no quit transitions are set.
        let Some(memory) = self.memory_states.checked_add(state.bytes().len()) else {
            return session.fail(Error::Overflow);
        };
        self.memory_states = memory;
        let shared = state.share(session)?;
        self.states.push(shared, session)?;
        self.interner.insert(state, id, session)?;
        Ok(Ok(id))
    }

    pub(super) fn memory_usage<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<usize> {
        session.items(1)?;
        let state_size = std::mem::size_of::<key::State>();
        let terms = [
            self.trans.values.len().checked_mul(4),
            self.starts.values.len().checked_mul(4),
            self.states.values.len().checked_mul(state_size),
            self.interner.len().checked_mul(state_size + 4),
            Some(self.sparses.memory_usage()),
            self.stack.admitted.checked_mul(4),
            Some(self.builder.capacity()),
            Some(self.memory_states),
        ];
        let mut total = 0usize;
        for term in terms {
            let Some(next) = term.and_then(|term| total.checked_add(term)) else {
                return session.fail(Error::Overflow);
            };
            total = next;
        }
        Ok(total)
    }

    pub(super) fn fits<A: Admission>(
        &self,
        dfa: &Machine,
        state_bytes: usize,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        let current = self.memory_usage(session)?;
        let state_size = std::mem::size_of::<key::State>();
        let Some(needed) = current
            .checked_add(dfa.stride() * 4 + 2 * state_size + 4)
            .and_then(|n| n.checked_add(state_bytes))
        else {
            return session.fail(Error::Overflow);
        };
        Ok(needed <= dfa.capacity())
    }

    pub(super) fn try_clear<A: Admission>(
        &mut self,
        dfa: &Machine,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        session.items(1)?;
        if dfa
            .minimum_clears()
            .is_some_and(|minimum| self.clear_count >= minimum)
        {
            if let Some(minimum) = dfa.minimum_bytes() {
                let searched = self.total_searched(session)?;
                if searched < minimum.saturating_mul(self.states.values.len()) {
                    return Ok(Err(Failure::BadEfficiency));
                }
            } else {
                return Ok(Err(Failure::TooManyClears));
            }
        }
        self.clear(dfa, session)?;
        Ok(Ok(()))
    }

    fn clear<A: Admission>(&mut self, dfa: &Machine, session: &mut Session<'_, A>) -> Step<()> {
        self.trans.resize(0, 0, session)?;
        self.starts.resize(0, 0, session)?;
        self.states.clear(session)?;
        self.interner.clear(session)?;
        session.items(1)?;
        self.memory_states = 0;
        let Some(count) = self.clear_count.checked_add(1) else {
            return session.fail(Error::Overflow);
        };
        self.clear_count = count;
        self.bytes_searched = 0;
        if let Some(progress) = &mut self.progress {
            progress.start = progress.at;
        }
        self.init(dfa, session)?;
        session.items(1)?;
        if let Saver::ToSave { id, state } = std::mem::replace(&mut self.saver, Saver::None) {
            if id == UNKNOWN
                || id == DEAD | dfa.stride() as u32
                || id == QUIT | (2 * dfa.stride()) as u32
            {
                return session.fail(Error::State);
            }
            let tag = if id & START != 0 {
                Tag::Start
            } else {
                Tag::None
            };
            let result = self.add_state(dfa, state, tag, session)?;
            let Ok(id) = result else {
                return session.fail(Error::State);
            };
            self.saver = Saver::Saved(id);
        }
        Ok(())
    }

    pub(super) fn reset<A: Admission>(
        &mut self,
        dfa: &Machine,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        self.saver = Saver::None;
        self.clear(dfa, session)?;
        self.sparses.resize(dfa.state_len(), session)?;
        self.clear_count = 0;
        self.progress = None;
        Ok(())
    }

    pub(super) fn save<A: Admission>(
        &mut self,
        dfa: &Machine,
        id: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let Some(state) = self
            .states
            .values
            .get((id & MAX_ID) as usize >> dfa.stride2())
        else {
            return session.fail(Error::State);
        };
        self.saver = Saver::ToSave {
            id,
            state: state.share(session)?,
        };
        Ok(())
    }

    pub(super) fn saved_id<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<u32> {
        session.items(1)?;
        match std::mem::replace(&mut self.saver, Saver::None) {
            Saver::Saved(id) | Saver::ToSave { id, .. } => Ok(id),
            Saver::None => session.fail(Error::State),
        }
    }

    pub(super) fn search_start<A: Admission>(
        &mut self,
        at: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if let Some(progress) = self.progress.take() {
            self.add_searched(progress.start.abs_diff(progress.at), session)?;
        }
        self.progress = Some(Progress { start: at, at });
        Ok(())
    }

    pub(super) fn search_update<A: Admission>(
        &mut self,
        at: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let Some(progress) = &mut self.progress else {
            return session.fail(Error::State);
        };
        progress.at = at;
        Ok(())
    }

    pub(super) fn search_finish<A: Admission>(
        &mut self,
        at: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let Some(progress) = self.progress.take() else {
            return session.fail(Error::State);
        };
        self.add_searched(progress.start.abs_diff(at), session)
    }

    fn add_searched<A: Admission>(
        &mut self,
        length: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let Some(total) = self.bytes_searched.checked_add(length) else {
            return session.fail(Error::Overflow);
        };
        self.bytes_searched = total;
        Ok(())
    }

    pub(super) fn total_searched<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<usize> {
        session.items(1)?;
        let current = self.progress.as_ref().map_or(0, |p| p.start.abs_diff(p.at));
        match self.bytes_searched.checked_add(current) {
            Some(total) => Ok(total),
            None => session.fail(Error::Overflow),
        }
    }
}
