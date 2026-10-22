// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! dfa::determinize::Runner and its scratch lifetime across shuffle.
use super::super::{finalize, info, storage::Array};
use super::{initial, intern, key, Admission, Error, Session, Step};
use crate::input::membership::manifest::storage::Buffer;

mod closure;
#[cfg(test)]
mod tests;

pub(super) struct Complete {
    // IDs are strictly ascending, matching BTreeMap's later traversal. The
    // admitted flat ordered-map precedent is convert/dependencies/sources.rs.
    pub(super) matches: Buffer<(u32, Array<u32>)>,
    // Runner and run-local scratch remain alive across upstream's shuffle
    // call. Keep that ownership at this unfinished constructor boundary.
    scratch: Scratch,
}

impl Complete {
    pub(super) fn shuffle<A: Admission>(
        self,
        dense: &mut initial::Initial,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), initial::Failure>> {
        let Self { matches, scratch } = self;
        let result = super::finish::shuffle(dense, matches, session);
        drop(scratch);
        result
    }
}

struct Scratch {
    cache: intern::Interner,
    sets: closure::Sets,
    stack: Array<u32>,
    builder: key::Empty,
    representatives: Array<Unit>,
    uncompiled: Array<u32>,
}

struct Runner<'a> {
    nfa: &'a finalize::Nfa,
    dense: &'a mut initial::Initial,
    kind: info::Kind,
    dfa_limit: Option<usize>,
    auxiliary_limit: Option<usize>,
    states: States,
    memory_usage_state: usize,
    scratch: Scratch,
}

struct States {
    values: Vec<key::State>,
    admitted: usize,
}

// Matches alphabet::Unit's payload/alignment, including EOI's u16 value.
#[derive(Clone, Copy)]
enum Unit {
    Byte(u8),
    End(u16),
}

impl Unit {
    fn byte(self) -> Option<u8> {
        match self {
            Self::Byte(byte) => Some(byte),
            Self::End(_) => None,
        }
    }

    fn class(self, classes: &[u8; 256]) -> usize {
        match self {
            Self::Byte(byte) => usize::from(classes[usize::from(byte)]),
            Self::End(class) => usize::from(class),
        }
    }
}

pub(super) fn run<A: Admission>(
    nfa: &finalize::Nfa,
    dense: &mut initial::Initial,
    kind: info::Kind,
    dfa_limit: Option<usize>,
    auxiliary_limit: Option<usize>,
    session: &mut Session<'_, A>,
) -> Step<Result<Complete, initial::Failure>> {
    let dead = key::State::dead(session)?;
    let quit = key::State::dead(session)?;
    let mut cache = intern::Interner::new();
    cache.insert(dead.share(session)?, 0, session)?;
    let mut values = finalize::storage::vector(2, session)?;
    session.items(2)?;
    values.push(dead);
    values.push(quit);
    let states = States {
        values,
        admitted: 2,
    };
    let sets = closure::Sets::new(nfa.0.states.len(), session)?;
    let mut runner = Runner {
        nfa,
        dense,
        kind,
        dfa_limit,
        auxiliary_limit,
        states,
        // Upstream deliberately excludes dead/quit encoded bytes here.
        memory_usage_state: 0,
        scratch: Scratch {
            cache,
            sets,
            stack: Array::new(),
            builder: key::Empty::new(),
            representatives: Array::new(),
            uncompiled: Array::new(),
        },
    };
    session.items(1)?;
    if nfa.0.looks & !3 != 0 {
        return session.fail(Error::State);
    }
    let mut previous = None;
    for byte in u8::MIN..=u8::MAX {
        session.items(1)?;
        let class = runner.dense.classes[usize::from(byte)];
        if previous != Some(class) {
            previous = Some(class);
            runner
                .scratch
                .representatives
                .push(Unit::Byte(byte), session)?;
        }
    }
    runner
        .scratch
        .representatives
        .push(Unit::End(u16::from(runner.dense.classes[255]) + 1), session)?;
    if let Err(error) = runner.starts(session)? {
        return Ok(Err(error));
    }
    while let Some(id) = runner.scratch.uncompiled.pop(session)? {
        for index in 0..runner.scratch.representatives.values.len() {
            session.items(1)?;
            let unit = runner.scratch.representatives.values[index];
            let empty = std::mem::replace(&mut runner.scratch.builder, key::Empty::new());
            let builder = runner.scratch.sets.next(
                nfa,
                kind,
                &mut runner.scratch.stack,
                &runner.states.values[id as usize >> runner.dense.stride2],
                unit,
                empty,
                session,
            )?;
            let (next, new) = match runner.intern(builder, session)? {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            session.items(1)?;
            let offset = id as usize + unit.class(&runner.dense.classes);
            runner.dense.transitions.values[offset] = next;
            if new {
                runner.scratch.uncompiled.push(next, session)?;
            }
        }
    }
    let mut matches = Buffer::new();
    runner.scratch.cache.clear(session)?;
    for (index, state) in runner.states.values.into_iter().enumerate() {
        session.items(1)?;
        if let Some(patterns) = state.patterns(session)? {
            let id = (index << runner.dense.stride2) as u32;
            // Enumeration proves this key follows the last key; no sort or
            // replay traversal is introduced when replacing the scratch tree.
            matches.push((id, patterns), session)?;
        }
    }
    Ok(Ok(Complete {
        matches,
        scratch: runner.scratch,
    }))
}

impl Runner<'_> {
    fn starts<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), initial::Failure>> {
        if !self.dense.starts.anchored_only
            && let Err(error) = self.group(0, self.nfa.0.unanchored.0, session)?
        {
            return Ok(Err(error));
        }
        if let Err(error) = self.group(1, self.nfa.0.anchored.0, session)? {
            return Ok(Err(error));
        }
        for pattern in 0..self.nfa.0.starts.len() {
            session.items(1)?;
            if let Err(error) = self.group(2 + pattern, self.nfa.0.starts[pattern].0, session)? {
                return Ok(Err(error));
            }
        }
        Ok(Ok(()))
    }

    fn group<A: Admission>(
        &mut self,
        group: usize,
        start: u32,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), initial::Failure>> {
        let (id, new) = match self.start(start, initial::Start::NonWordByte, session)? {
            Ok(value) => value,
            Err(error) => return Ok(Err(error)),
        };
        self.set_start(group, initial::Start::NonWordByte, id, session)?;
        if new {
            self.scratch.uncompiled.push(id, session)?;
        }
        self.set_start(group, initial::Start::WordByte, id, session)?;
        for context in [
            initial::Start::Text,
            initial::Start::LineLF,
            initial::Start::LineCR,
            initial::Start::CustomLineTerminator,
        ] {
            session.items(1)?;
            if self.nfa.0.prefix_looks & 3 == 0 {
                self.set_start(group, context, id, session)?;
            } else {
                let (id, new) = match self.start(start, context, session)? {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
                self.set_start(group, context, id, session)?;
                if new {
                    self.scratch.uncompiled.push(id, session)?;
                }
            }
        }
        Ok(Ok(()))
    }

    fn set_start<A: Admission>(
        &mut self,
        group: usize,
        context: initial::Start,
        id: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        self.dense.starts.table.values[group * 6 + context as usize] = id;
        Ok(())
    }

    fn start<A: Admission>(
        &mut self,
        start: u32,
        context: initial::Start,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(u32, bool), initial::Failure>> {
        let empty = std::mem::replace(&mut self.scratch.builder, key::Empty::new());
        let mut builder = empty.matches(session)?;
        let have = if context == initial::Start::Text && self.nfa.0.looks & 3 != 0 {
            1
        } else {
            0
        };
        builder.have(have, session)?;
        self.scratch.sets.first.clear(session)?;
        self.scratch
            .sets
            .first
            .closure(self.nfa, start, have, &mut self.scratch.stack, session)?;
        let mut builder = builder.nfa(session)?;
        self.scratch
            .sets
            .first
            .encode(self.nfa, &mut builder, session)?;
        self.intern(builder, session)
    }

    fn intern<A: Admission>(
        &mut self,
        builder: key::Nfa,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(u32, bool), initial::Failure>> {
        if let Some(id) = self.scratch.cache.get(builder.bytes(), session)? {
            self.scratch.builder = builder.clear(session)?;
            return Ok(Ok((id, false)));
        }
        // TransitionTable::add_empty_state rejects the premultiplied ID before
        // extending storage. Semantic limits are checked only after publishing
        // the shared key to BOTH canonical states and the cache.
        session.items(1)?;
        let id = self.dense.transitions.values.len();
        if id > i32::MAX as usize {
            return Ok(Err(initial::Failure::States));
        }
        let Some(length) = id.checked_add(self.dense.stride()) else {
            return session.fail(Error::Overflow);
        };
        self.dense.transitions.resize(length, 0, session)?;
        let state = builder.shared(session)?;
        let Some(memory) = self.memory_usage_state.checked_add(state.bytes().len()) else {
            return session.fail(Error::Overflow);
        };
        self.memory_usage_state = memory;
        self.states.push(state.share(session)?, session)?;
        self.scratch.cache.insert(state, id as u32, session)?;
        self.scratch.builder = builder.clear(session)?;
        session.items(1)?;
        if let Some(limit) = self.dfa_limit
            && self.dense.memory_usage() > limit
        {
            return Ok(Err(initial::Failure::DfaSize(limit)));
        }
        session.items(1)?;
        if let Some(limit) = self.auxiliary_limit
            && self.memory_usage(session)? > limit
        {
            return Ok(Err(initial::Failure::DeterminizeSize(limit)));
        }
        Ok(Ok((id as u32, true)))
    }

    fn memory_usage<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<usize> {
        // Preserve upstream's semantic formula literally. Its cache projection
        // excludes padding/buckets; actual ownership is separately admitted.
        let Some(memory) = self
            .states
            .values
            .len()
            .checked_mul(std::mem::size_of::<key::State>())
            .and_then(|memory| {
                self.scratch
                    .cache
                    .len()
                    .checked_mul(std::mem::size_of::<key::State>() + 4)
                    .and_then(|cache| memory.checked_add(cache))
            })
            .and_then(|memory| memory.checked_add(self.memory_usage_state))
            .and_then(|memory| {
                self.scratch
                    .stack
                    .admitted
                    .checked_mul(4)
                    .and_then(|stack| memory.checked_add(stack))
            })
            .and_then(|memory| memory.checked_add(self.scratch.builder.capacity()))
        else {
            return session.fail(Error::Overflow);
        };
        Ok(memory)
    }
}

impl States {
    fn push<A: Admission>(&mut self, state: key::State, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        if self.values.len() == self.admitted {
            let Some(admitted) = self.admitted.max(2).checked_mul(2) else {
                return session.fail(Error::Overflow);
            };
            let mut replacement = finalize::storage::vector(admitted, session)?;
            session.items(self.values.len())?;
            replacement.append(&mut self.values);
            self.values = replacement;
            self.admitted = admitted;
        }
        self.values.push(state);
        Ok(())
    }
}
