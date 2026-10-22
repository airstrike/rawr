// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! util::determinize::state's encoded, shared ordered NFA subsets.
use std::{alloc::Layout, borrow::Borrow, sync::Arc};

use rawr_toml::document::Request;

use super::super::storage::Array;
use super::{Admission, Error, Session, Step};

#[derive(Debug, PartialEq, Eq, Hash)]
pub(in super::super) struct State(Arc<[u8]>);

pub(in super::super) struct Empty(Array<u8>);
pub(in super::super) struct Matches(Array<u8>);
pub(in super::super) struct Nfa {
    bytes: Array<u8>,
    previous: i32,
}

impl Borrow<[u8]> for State {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl State {
    #[cfg(test)]
    pub(in super::super) fn weak(&self) -> std::sync::Weak<[u8]> {
        Arc::downgrade(&self.0)
    }

    pub(in super::super) fn dead<A: Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        Empty::new().matches(session)?.nfa(session)?.shared(session)
    }

    pub(in super::super) fn share<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        Ok(Self(Arc::clone(&self.0)))
    }

    pub(in super::super) fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub(in super::super) fn have(&self) -> u32 {
        read(&self.0[1..5])
    }

    pub(in super::super) fn need(&self) -> u32 {
        read(&self.0[5..9])
    }

    pub(in super::super) fn nfa_ids<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
        mut visit: impl FnMut(u32, &mut Session<'_, A>) -> Step<()>,
    ) -> Step<()> {
        let offset = if self.0[0] & 2 != 0 {
            13 + read(&self.0[9..13]) as usize * 4
        } else {
            9
        };
        let mut cursor = offset;
        let mut previous = 0i32;
        while cursor < self.0.len() {
            let mut value = 0u32;
            let mut shift = 0;
            loop {
                session.items(1)?;
                let byte = self.0[cursor];
                cursor += 1;
                value |= u32::from(byte & 0x7F) << shift;
                if byte & 0x80 == 0 {
                    break;
                }
                shift += 7;
            }
            let delta = ((value >> 1) as i32) ^ -((value & 1) as i32);
            previous += delta;
            visit(previous as u32, session)?;
        }
        Ok(())
    }

    /// Direct encoded match access, as used by the lazy DFA searcher.
    pub(in super::super) fn match_len(&self) -> usize {
        if self.0[0] & 1 == 0 {
            0
        } else if self.0[0] & 2 == 0 {
            1
        } else {
            read(&self.0[9..13]) as usize
        }
    }

    pub(in super::super) fn match_pattern(&self, index: usize) -> Option<u32> {
        if index >= self.match_len() {
            None
        } else if self.0[0] & 2 == 0 {
            Some(0)
        } else {
            let offset = 13 + index * 4;
            Some(read(&self.0[offset..offset + 4]))
        }
    }

    pub(super) fn patterns<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Array<u32>>> {
        session.items(1)?;
        if self.0[0] & 1 == 0 {
            return Ok(None);
        }
        let mut patterns = Array::new();
        if self.0[0] & 2 == 0 {
            patterns.push(0, session)?;
        } else {
            for bytes in self.0[13..13 + read(&self.0[9..13]) as usize * 4]
                .as_chunks::<4>()
                .0
                .iter()
            {
                session.items(1)?;
                patterns.push(read(bytes), session)?;
            }
        }
        Ok(Some(patterns))
    }
}

impl Empty {
    pub(in super::super) fn new() -> Self {
        Self(Array::new())
    }

    pub(in super::super) fn capacity(&self) -> usize {
        self.0.admitted
    }

    pub(in super::super) fn matches<A: Admission>(
        mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Matches> {
        // The first extend is nine bytes, above Vec<u8>'s eight-byte minimum.
        // Subsequent growth therefore follows Array's same doubling rule.
        self.0.resize(9, 0, session)?;
        Ok(Matches(self.0))
    }
}

impl Matches {
    pub(in super::super) fn have<A: Admission>(
        &mut self,
        have: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(4)?;
        self.0.values[1..5].copy_from_slice(&have.to_ne_bytes());
        Ok(())
    }

    pub(in super::super) fn pattern<A: Admission>(
        &mut self,
        pattern: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if self.0.values[0] & 2 == 0 {
            if pattern == 0 {
                self.0.values[0] |= 1;
                return Ok(());
            }
            word(&mut self.0, 0, session)?;
            self.0.values[0] |= 2;
            if self.0.values[0] & 1 != 0 {
                word(&mut self.0, 0, session)?;
            } else {
                self.0.values[0] |= 1;
            }
        }
        word(&mut self.0, pattern, session)
    }

    pub(in super::super) fn nfa<A: Admission>(mut self, session: &mut Session<'_, A>) -> Step<Nfa> {
        session.items(1)?;
        if self.0.values[0] & 2 != 0 {
            let length = (self.0.values.len() - 13) / 4;
            let Ok(length) = u32::try_from(length) else {
                return session.fail(Error::Overflow);
            };
            session.items(4)?;
            self.0.values[9..13].copy_from_slice(&length.to_ne_bytes());
        }
        Ok(Nfa {
            bytes: self.0,
            previous: 0,
        })
    }
}

impl Nfa {
    pub(in super::super) fn bytes(&self) -> &[u8] {
        &self.bytes.values
    }

    pub(in super::super) fn id<A: Admission>(
        &mut self,
        id: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if id > i32::MAX as u32 {
            return session.fail(Error::State);
        }
        let delta = id as i32 - self.previous;
        let mut value = (delta as u32) << 1;
        if delta < 0 {
            value = !value;
        }
        while value >= 0x80 {
            self.bytes.push(value as u8 | 0x80, session)?;
            value >>= 7;
        }
        self.bytes.push(value as u8, session)?;
        self.previous = id as i32;
        Ok(())
    }

    pub(in super::super) fn need<A: Admission>(
        &mut self,
        need: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(4)?;
        self.bytes.values[5..9].copy_from_slice(&need.to_ne_bytes());
        if need == 0 {
            session.items(4)?;
            self.bytes.values[1..5].fill(0);
        }
        Ok(())
    }

    pub(in super::super) fn shared<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<State> {
        let Ok(value) = Layout::array::<u8>(self.bytes.values.len()) else {
            return session.fail(Error::Overflow);
        };
        let Ok((layout, _)) = Layout::new::<[usize; 2]>().extend(value) else {
            return session.fail(Error::Overflow);
        };
        let layout = layout.pad_to_align();
        session.admit(Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        session.items(self.bytes.values.len())?;
        Ok(State(Arc::from(self.bytes.values.as_slice())))
    }

    pub(in super::super) fn clear<A: Admission>(
        mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Empty> {
        self.bytes.resize(0, 0, session)?;
        Ok(Empty(self.bytes))
    }
}

fn read(bytes: &[u8]) -> u32 {
    u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn word<A: Admission>(bytes: &mut Array<u8>, word: u32, session: &mut Session<'_, A>) -> Step<()> {
    bytes.reserve(4, session)?;
    session.items(4)?;
    bytes.values.extend_from_slice(&word.to_ne_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::reference::Account;

    #[test]
    fn implicit_zero_and_explicit_pattern_order_keep_native_endian_source_encoding() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        for patterns in [vec![], vec![0], vec![7], vec![0, 7, 3], vec![7, 0, 3]] {
            let mut matches = Empty::new().matches(&mut session).unwrap();
            for &id in &patterns {
                matches.pattern(id, &mut session).unwrap();
            }
            let builder = matches.nfa(&mut session).unwrap();
            let mut expected = vec![0; 9];
            if !patterns.is_empty() {
                expected[0] = 1;
                if patterns != [0] {
                    expected[0] |= 2;
                    expected.extend_from_slice(&(patterns.len() as u32).to_ne_bytes());
                    for &id in &patterns {
                        expected.extend_from_slice(&id.to_ne_bytes());
                    }
                }
            }
            assert_eq!(builder.bytes(), expected);
            let state = builder.shared(&mut session).unwrap();
            let actual = state.patterns(&mut session).unwrap().map(|ids| ids.values);
            assert_eq!(actual, (!patterns.is_empty()).then_some(patterns));
        }
    }

    #[test]
    fn ordered_ids_use_signed_delta_varints_including_backward_and_maximum_jumps() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Empty::new()
            .matches(&mut session)
            .unwrap()
            .nfa(&mut session)
            .unwrap();
        let ids = [1, 3, 2, i32::MAX as u32, 0];
        for id in ids {
            builder.id(id, &mut session).unwrap();
        }
        assert_eq!(&builder.bytes()[9..12], &[2, 4, 1]);
        let state = builder.shared(&mut session).unwrap();
        let mut actual = Vec::new();
        state
            .nfa_ids(&mut session, |id, _| {
                actual.push(id);
                Ok(())
            })
            .unwrap();
        assert_eq!(actual, ids);
        let mut other = Empty::new()
            .matches(&mut session)
            .unwrap()
            .nfa(&mut session)
            .unwrap();
        for id in ids.into_iter().rev() {
            other.id(id, &mut session).unwrap();
        }
        assert_ne!(state, other.shared(&mut session).unwrap());
    }

    #[test]
    fn arc_copy_and_scratch_reuse_keep_separate_ownership_and_exact_header_layout() {
        use super::super::tests::Count;
        let mut count = Count::default();
        let mut session = Session::new(&mut count);
        let builder = Empty::new()
            .matches(&mut session)
            .unwrap()
            .nfa(&mut session)
            .unwrap();
        let state = builder.shared(&mut session).unwrap();
        let copy = state.share(&mut session).unwrap();
        assert!(Arc::ptr_eq(&state.0, &copy.0));
        assert_eq!(Arc::strong_count(&state.0), 2);
        let empty = builder.clear(&mut session).unwrap();
        assert_eq!(empty.capacity(), 9);
        let _reused = empty.matches(&mut session).unwrap();
        let arc = Layout::new::<[usize; 2]>()
            .extend(Layout::array::<u8>(9).unwrap())
            .unwrap()
            .0
            .pad_to_align();
        assert_eq!(count.allocations, [(9, 1), (arc.size(), arc.align())]);
    }
}
