// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! hybrid::dfa::{DFA, Lazy}: start groups and lazy byte/EOI transitions.
use super::super::dfa::Start;
use super::{Admission, Cache, Error, Failure, Machine, Session, Step, Tag, DEAD, MAX_ID, UNKNOWN};

#[derive(Clone, Copy)]
pub(super) enum Anchored {
    No,
    Yes,
    Pattern(u32),
}

impl Cache {
    pub(super) fn start<A: Admission>(
        &mut self,
        dfa: &Machine,
        anchored: Anchored,
        behind: Option<u8>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<u32, Failure>> {
        session.items(1)?;
        let start = dfa.start(behind);
        let group = match anchored {
            Anchored::No => 0,
            Anchored::Yes => 6,
            Anchored::Pattern(pattern) => {
                if pattern as usize >= dfa.pattern_len() {
                    return Ok(Ok(DEAD | dfa.stride() as u32));
                }
                12 + 6 * pattern as usize
            }
        };
        let slot = group + start as usize;
        let id = self.starts.values[slot];
        if id & UNKNOWN == 0 {
            return Ok(Ok(id));
        }
        let nfa = dfa.nfa();
        let id = match anchored {
            Anchored::No => nfa.0.unanchored.0,
            Anchored::Yes => nfa.0.anchored.0,
            Anchored::Pattern(pattern) => nfa.0.starts[pattern as usize].0,
        };
        let empty = self.take_builder(session)?;
        let have = u32::from(start == Start::Text && nfa.0.looks & 3 != 0);
        let builder = self
            .sparses
            .start(nfa, id, have, &mut self.stack, empty, session)?;
        // The fixed source configuration does not specialize/tag starts.
        let id = match self.insert_builder(dfa, builder, Tag::None, session)? {
            Ok(id) => id,
            Err(error) => return Ok(Err(error)),
        };
        session.items(1)?;
        self.starts.values[slot] = id;
        Ok(Ok(id))
    }

    /// The source's untagged hot loop reads the table without constructing.
    pub(super) fn cached<A: Admission>(
        &self,
        dfa: &Machine,
        current: u32,
        byte: Option<u8>,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        session.items(1)?;
        let class = class(dfa, byte);
        let offset = (current & MAX_ID) as usize + class;
        match self.trans.values.get(offset) {
            Some(&id) => Ok(id),
            None => session.fail(Error::State),
        }
    }

    pub(super) fn next<A: Admission>(
        &mut self,
        dfa: &Machine,
        mut current: u32,
        byte: Option<u8>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<u32, Failure>> {
        let next = self.cached(dfa, current, byte, session)?;
        if next & UNKNOWN == 0 {
            return Ok(Ok(next));
        }
        let empty = self.take_builder(session)?;
        let state = &self.states.values[(current & MAX_ID) as usize >> dfa.stride2()];
        let builder = self.sparses.next(
            dfa.nfa(),
            dfa.kind(),
            &mut self.stack,
            state,
            byte,
            empty,
            session,
        )?;
        // This predicate deliberately precedes the borrowed interner lookup.
        // A hit may avoid clearing, in which case saved_id returns the old ID.
        let save = !self.fits(dfa, builder.bytes().len(), session)?
            || self.trans.values.len() >= MAX_ID as usize;
        if save {
            self.save(dfa, current, session)?;
        }
        let next = match self.insert_builder(dfa, builder, Tag::None, session)? {
            Ok(next) => next,
            Err(error) => return Ok(Err(error)),
        };
        if save {
            current = self.saved_id(session)?;
        }
        session.items(1)?;
        let offset = (current & MAX_ID) as usize + class(dfa, byte);
        self.trans.values[offset] = next;
        Ok(Ok(next))
    }

    pub(super) fn match_len<A: Admission>(
        &self,
        dfa: &Machine,
        id: u32,
        session: &mut Session<'_, A>,
    ) -> Step<usize> {
        session.items(1)?;
        Ok(self.states.values[(id & MAX_ID) as usize >> dfa.stride2()].match_len())
    }

    pub(super) fn match_pattern<A: Admission>(
        &self,
        dfa: &Machine,
        id: u32,
        index: usize,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        session.items(1)?;
        let state = &self.states.values[(id & MAX_ID) as usize >> dfa.stride2()];
        match state.match_pattern(index) {
            Some(pattern) => Ok(pattern),
            None => session.fail(Error::State),
        }
    }
}

fn class(dfa: &Machine, byte: Option<u8>) -> usize {
    byte.map_or(usize::from(dfa.classes()[255]) + 1, |byte| {
        usize::from(dfa.classes()[usize::from(byte)])
    })
}
