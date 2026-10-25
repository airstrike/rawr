// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! util::determinize's ordered closure for the emitted Start/End-look grammar.
use super::super::{key, Admission, Error, Session, Step};
use super::{finalize, info, Array, Unit};

pub(super) struct Sets {
    pub(super) first: Set,
    second: Set,
}

pub(super) struct Set {
    length: usize,
    dense: Array<u32>,
    sparse: Array<u32>,
}

impl Sets {
    pub(super) fn new<A: Admission>(length: usize, session: &mut Session<'_, A>) -> Step<Self> {
        Ok(Self {
            first: Set::new(length, session)?,
            second: Set::new(length, session)?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn next<A: Admission>(
        &mut self,
        nfa: &finalize::Nfa,
        kind: info::Kind,
        stack: &mut Array<u32>,
        state: &key::State,
        unit: Unit,
        empty: key::Empty,
        session: &mut Session<'_, A>,
    ) -> Step<key::Nfa> {
        self.first.clear(session)?;
        self.second.clear(session)?;
        state.nfa_ids(session, |id, session| {
            self.first.insert(id, session).map(|_| ())
        })?;
        session.items(1)?;
        let have = state.have() | if matches!(unit, Unit::End(_)) { 2 } else { 0 };
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
                    if unit
                        .byte()
                        .is_some_and(|byte| trans.start <= byte && byte <= trans.end)
                    {
                        Some(trans.next.0)
                    } else {
                        None
                    }
                }
                finalize::State::Sparse { transitions } => {
                    let mut next = None;
                    if let Some(byte) = unit.byte() {
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
}

impl Set {
    fn new<A: Admission>(length: usize, session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        if length > i32::MAX as usize {
            return session.fail(Error::State);
        }
        let mut dense = Array::new();
        dense.resize(length, 0, session)?;
        let mut sparse = Array::new();
        sparse.resize(length, 0, session)?;
        Ok(Self {
            length: 0,
            dense,
            sparse,
        })
    }

    pub(super) fn clear<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
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

    pub(super) fn closure<A: Admission>(
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

    pub(super) fn encode<A: Admission>(
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
            // Union/BinaryUnion and Fail remain in the key, just as upstream;
            // omitting unions can change preferences when redoing a closure.
            builder.id(id, session)?;
        }
        builder.need(need, session)
    }
}
