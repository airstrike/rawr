// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! util::sparse_set and determinize's ordered epsilon/subset traversal.
use super::super::{finalize, info};
use super::{key, Admission, Array, Error, Session, Step};

pub(super) struct Sets {
    first: Set,
    second: Set,
}
struct Set {
    length: usize,
    dense: Array<u32>,
    sparse: Array<u32>,
}

impl Sets {
    pub(super) fn start<A: Admission>(
        &mut self,
        nfa: &finalize::Nfa,
        id: u32,
        have: u32,
        stack: &mut Array<u32>,
        empty: key::Empty,
        session: &mut Session<'_, A>,
    ) -> Step<key::Nfa> {
        let mut builder = empty.matches(session)?;
        if have != 0 {
            builder.have(have, session)?;
        }
        self.first.clear(session)?;
        self.first.closure(nfa, id, have, stack, session)?;
        let mut builder = builder.nfa(session)?;
        self.first.encode(nfa, &mut builder, session)?;
        Ok(builder)
    }

    pub(super) fn next<A: Admission>(
        &mut self,
        nfa: &finalize::Nfa,
        kind: info::Kind,
        stack: &mut Array<u32>,
        state: &key::State,
        byte: Option<u8>,
        empty: key::Empty,
        session: &mut Session<'_, A>,
    ) -> Step<key::Nfa> {
        self.first.clear(session)?;
        self.second.clear(session)?;
        state.nfa_ids(session, |id, session| {
            self.first.insert(id, session).map(|_| ())
        })?;
        session.items(1)?;
        // Machine's producer permits only Start/End assertions. End becomes
        // true only for EOI; Start is established only in the start builder.
        let have = state.have() | if byte.is_none() { 2 } else { 0 };
        if have & !state.have() & state.need() != 0 {
            for &id in &self.first.dense.values[..self.first.length] {
                session.items(1)?;
                self.second.closure(nfa, id, have, stack, session)?;
            }
            std::mem::swap(&mut self.first, &mut self.second);
            self.second.clear(session)?;
        }
        let mut builder = empty.matches(session)?;
        for &id in &self.first.dense.values[..self.first.length] {
            session.items(1)?;
            let next = match &nfa.0.states[id as usize] {
                finalize::State::Match { pattern } => {
                    builder.pattern(*pattern, session)?;
                    if kind == info::Kind::LeftmostFirst {
                        break;
                    }
                    None
                }
                finalize::State::ByteRange { trans } => {
                    if byte.is_some_and(|byte| trans.start <= byte && byte <= trans.end) {
                        Some(trans.next.0)
                    } else {
                        None
                    }
                }
                finalize::State::Sparse { transitions } => {
                    let mut next = None;
                    if let Some(byte) = byte {
                        for trans in transitions {
                            session.items(1)?;
                            if trans.start > byte {
                                break;
                            }
                            if byte <= trans.end {
                                next = Some(trans.next.0);
                                break;
                            }
                        }
                    }
                    next
                }
                _ => None,
            };
            if let Some(next) = next {
                self.second.closure(nfa, next, 0, stack, session)?;
            }
        }
        let mut builder = builder.nfa(session)?;
        self.second.encode(nfa, &mut builder, session)?;
        Ok(builder)
    }

    pub(super) fn new<A: Admission>(length: usize, session: &mut Session<'_, A>) -> Step<Self> {
        Ok(Self {
            first: Set::new(length, session)?,
            second: Set::new(length, session)?,
        })
    }

    pub(super) fn resize<A: Admission>(
        &mut self,
        length: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.first.resize(length, session)?;
        self.second.resize(length, session)
    }

    pub(super) fn memory_usage(&self) -> usize {
        (self.first.dense.values.len()
            + self.first.sparse.values.len()
            + self.second.dense.values.len()
            + self.second.sparse.values.len())
            * 4
    }
}

impl Set {
    fn clear<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        self.length = 0;
        Ok(())
    }

    fn insert<A: Admission>(&mut self, id: u32, session: &mut Session<'_, A>) -> Step<bool> {
        session.items(1)?;
        let index = self.sparse.values[id as usize] as usize;
        if index < self.length && self.dense.values[index] == id {
            return Ok(false);
        }
        self.dense.values[self.length] = id;
        self.sparse.values[id as usize] = self.length as u32;
        self.length += 1;
        Ok(true)
    }

    fn closure<A: Admission>(
        &mut self,
        nfa: &finalize::Nfa,
        start: u32,
        have: u32,
        stack: &mut Array<u32>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if !stack.values.is_empty() {
            return session.fail(Error::State);
        }
        if matches!(
            nfa.0.states[start as usize],
            finalize::State::ByteRange { .. }
                | finalize::State::Sparse { .. }
                | finalize::State::Fail
                | finalize::State::Match { .. }
        ) {
            self.insert(start, session)?;
            return Ok(());
        }
        stack.push(start, session)?;
        while let Some(mut id) = stack.pop(session)? {
            loop {
                if !self.insert(id, session)? {
                    break;
                }
                session.items(1)?;
                match &nfa.0.states[id as usize] {
                    finalize::State::Look { look, next } => {
                        if have & look == 0 {
                            break;
                        }
                        id = next.0;
                    }
                    finalize::State::Union { alternates } => {
                        let Some(first) = alternates.first() else {
                            break;
                        };
                        id = first.0;
                        stack.reserve(alternates.len() - 1, session)?;
                        session.items(alternates.len() - 1)?;
                        stack
                            .values
                            .extend(alternates[1..].iter().rev().map(|id| id.0));
                    }
                    finalize::State::BinaryUnion { alt1, alt2 } => {
                        id = alt1.0;
                        stack.push(alt2.0, session)?;
                    }
                    finalize::State::Capture { next, .. } => id = next.0,
                    _ => break,
                }
            }
        }
        Ok(())
    }

    fn encode<A: Admission>(
        &self,
        nfa: &finalize::Nfa,
        builder: &mut key::Nfa,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let mut need = 0;
        for &id in &self.dense.values[..self.length] {
            session.items(1)?;
            match &nfa.0.states[id as usize] {
                finalize::State::Capture { .. } => continue,
                finalize::State::Look { look, .. } => need |= look,
                _ => {}
            }
            // Pinned add_nfa_states keeps Union/BinaryUnion/Fail, preserving
            // branch preference when a newly true assertion redoes closure.
            builder.id(id, session)?;
        }
        builder.need(need, session)
    }

    fn new<A: Admission>(length: usize, session: &mut Session<'_, A>) -> Step<Self> {
        let mut set = Self {
            length: 0,
            dense: Array::new(),
            sparse: Array::new(),
        };
        set.resize(length, session)?;
        Ok(set)
    }

    fn resize<A: Admission>(&mut self, length: usize, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        if length > i32::MAX as usize {
            return session.fail(Error::State);
        }
        self.length = 0;
        self.dense.resize(length, 0, session)?;
        self.sparse.resize(length, 0, session)
    }
}
