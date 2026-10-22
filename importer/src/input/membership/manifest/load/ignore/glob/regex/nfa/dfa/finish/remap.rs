// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! dfa::remapper with exact collected and cloned map ownership.
use super::{initial, storage, Admission, Error, Session, Step};

pub(super) struct Remap {
    map: Vec<u32>,
}

impl Remap {
    pub(super) fn new<A: Admission>(
        dense: &initial::Initial,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let length = dense.transitions.values.len() >> dense.stride2;
        let mut map = storage::vector(length, session)?;
        session.items(length)?;
        map.extend((0..length).map(|index| (index << dense.stride2) as u32));
        Ok(Self { map })
    }

    pub(super) fn swap<A: Admission>(
        &mut self,
        dense: &mut initial::Initial,
        left: u32,
        right: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if left == right {
            return Ok(());
        }
        if left as usize >= dense.transitions.values.len()
            || right as usize >= dense.transitions.values.len()
        {
            return session.fail(Error::State);
        }
        // TransitionTable::swap visits alphabet slots only, not row padding.
        for class in 0..usize::from(dense.classes[255]) + 2 {
            session.items(1)?;
            dense
                .transitions
                .values
                .swap(left as usize + class, right as usize + class);
        }
        session.items(1)?;
        self.map.swap(
            left as usize >> dense.stride2,
            right as usize >> dense.stride2,
        );
        Ok(())
    }

    pub(super) fn apply<A: Admission>(
        mut self,
        dense: &mut initial::Initial,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let old = storage::copy(&self.map, session)?;
        for index in 0..self.map.len() {
            session.items(1)?;
            let current = (index << dense.stride2) as u32;
            let mut next = old[index];
            if current == next {
                continue;
            }
            loop {
                session.items(1)?;
                let id = old[next as usize >> dense.stride2];
                if current == id {
                    self.map[index] = next;
                    break;
                }
                next = id;
            }
        }
        // DFA::remap intentionally includes transition padding and every start.
        for id in &mut dense.transitions.values {
            session.items(1)?;
            *id = self.map[*id as usize >> dense.stride2];
        }
        for id in &mut dense.starts.table.values {
            session.items(1)?;
            *id = self.map[*id as usize >> dense.stride2];
        }
        Ok(())
    }
}
