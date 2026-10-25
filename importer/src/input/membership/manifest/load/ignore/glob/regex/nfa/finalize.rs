// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! regex-automata 0.4.14 nfa::Inner final states and epsilon-prefix properties.
use super::builder::{Id, Transition};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;
use std::sync::Arc;

pub(super) mod captures;
pub(super) mod storage;

/// Shared immutable data only. No search engine or package authority is exposed.
pub(super) struct Nfa(pub(super) Arc<Graph>);

pub(super) enum State {
    ByteRange {
        trans: Transition,
    },
    Sparse {
        transitions: Vec<Transition>,
    },
    Look {
        look: u32,
        next: Id,
    },
    // regex-automata's State::Capture; admitted state storage keeps its size.
    #[allow(dead_code)]
    Capture {
        next: Id,
        pattern: u32,
        group: u32,
        slot: u32,
    },
    Union {
        alternates: Vec<Id>,
    },
    BinaryUnion {
        alt1: Id,
        alt2: Id,
    },
    Fail,
    Match {
        pattern: u32,
    },
}

pub(super) struct Graph {
    pub(super) states: Vec<State>,
    pub(super) anchored: Id,
    pub(super) unanchored: Id,
    pub(super) starts: Vec<Id>,
    pub(super) groups: Arc<captures::Groups>,
    boundaries: [u128; 2],
    pub(super) classes: [u8; 256],
    pub(super) has_capture: bool,
    pub(super) has_empty: bool,
    pub(super) utf8: bool,
    pub(super) reverse: bool,
    pub(super) line_terminator: u8,
    pub(super) looks: u32,
    pub(super) prefix_looks: u32,
    pub(super) memory_extra: usize,
}

pub(super) struct Building {
    pub(super) graph: Graph,
    states: Buffer<State>,
}

impl Graph {
    /// Rebuild the source byte-class map from its retained boundary set. Dense
    /// construction reaches this again even though finalization already did.
    pub(super) fn byte_classes<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<[u8; 256]> {
        let mut classes = [0; 256];
        let mut class = 0_u8;
        for (byte, slot) in classes.iter_mut().enumerate() {
            session.items(1)?;
            *slot = class;
            if byte < 255 && self.boundaries[byte / 128] & (1_u128 << (byte % 128)) != 0 {
                class += 1;
            }
        }
        Ok(classes)
    }
}

impl Building {
    pub(super) fn new<A: Admission>(reverse: bool, session: &mut Session<'_, A>) -> Step<Self> {
        // Inner::default allocates its empty GroupInfo before remap storage.
        let groups = captures::Groups::empty(session)?;
        Ok(Self {
            graph: Graph {
                states: Vec::new(),
                anchored: Id::ZERO,
                unanchored: Id::ZERO,
                starts: Vec::new(),
                groups,
                boundaries: [0; 2],
                classes: [0; 256],
                has_capture: false,
                has_empty: false,
                utf8: false,
                reverse,
                line_terminator: b'\n',
                looks: 0,
                prefix_looks: 0,
                memory_extra: 0,
            },
            states: Buffer::new(),
        })
    }

    pub(super) fn add<A: Admission>(
        &mut self,
        state: State,
        session: &mut Session<'_, A>,
    ) -> Step<Id> {
        session.items(1)?;
        let extra = match &state {
            State::ByteRange { trans } => {
                self.range(trans.start, trans.end, session)?;
                0
            }
            State::Sparse { transitions } => {
                for trans in transitions {
                    self.range(trans.start, trans.end, session)?;
                }
                match transitions
                    .len()
                    .checked_mul(std::mem::size_of::<Transition>())
                {
                    Some(bytes) => bytes,
                    None => return session.fail(Error::Overflow),
                }
            }
            State::Look { look, .. } => {
                // The glob emitter's grammar reaches Start and End only. They
                // add no byte boundaries, even in reverse/LF configuration.
                if !matches!(*look, 1 | 2) {
                    return session.fail(Error::State);
                }
                self.graph.looks |= look;
                0
            }
            State::Capture { .. } => {
                self.graph.has_capture = true;
                0
            }
            State::Union { alternates } => match alternates.len().checked_mul(4) {
                Some(bytes) => bytes,
                None => return session.fail(Error::Overflow),
            },
            _ => 0,
        };
        self.graph.memory_extra = match self.graph.memory_extra.checked_add(extra) {
            Some(bytes) => bytes,
            None => return session.fail(Error::Overflow),
        };
        if self.states.len() >= i32::MAX as usize {
            return session.fail(Error::State);
        }
        let id = Id(self.states.len() as u32);
        self.states.push(state, session)?;
        Ok(id)
    }

    fn range<A: Admission>(
        &mut self,
        start: u8,
        end: u8,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if start > end {
            return session.fail(Error::State);
        }
        if start > 0 {
            self.boundary(start - 1);
        }
        self.boundary(end);
        Ok(())
    }

    fn boundary(&mut self, byte: u8) {
        self.graph.boundaries[usize::from(byte) / 128] |= 1_u128 << (byte % 128);
    }

    pub(super) fn remap<A: Admission>(
        &mut self,
        remap: &[Id],
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        for state in self.states.as_mut_slice() {
            session.items(1)?;
            match state {
                State::ByteRange { trans } => mapped(&mut trans.next, remap, session)?,
                State::Sparse { transitions } => {
                    for trans in transitions {
                        mapped(&mut trans.next, remap, session)?;
                    }
                }
                State::Look { next, .. } | State::Capture { next, .. } => {
                    mapped(next, remap, session)?
                }
                State::Union { alternates } => {
                    for next in alternates {
                        mapped(next, remap, session)?;
                    }
                }
                State::BinaryUnion { alt1, alt2 } => {
                    mapped(alt1, remap, session)?;
                    mapped(alt2, remap, session)?;
                }
                State::Fail | State::Match { .. } => {}
            }
        }
        mapped(&mut self.graph.anchored, remap, session)?;
        mapped(&mut self.graph.unanchored, remap, session)?;
        for start in &mut self.graph.starts {
            mapped(start, remap, session)?;
        }
        Ok(())
    }

    pub(super) fn finish<A: Admission>(mut self, session: &mut Session<'_, A>) -> Step<Nfa> {
        self.graph.classes = self.graph.byte_classes(session)?;
        let mut stack = Buffer::new();
        let mut seen = Seen::new(self.states.len(), session)?;
        for &start in &self.graph.starts {
            stack.push(start, session)?;
            seen.length = 0;
            let mut prefix = 0;
            while let Some(id) = stack.pop() {
                session.items(1)?;
                if !seen.insert(id, session)? {
                    continue;
                }
                let Some(state) = self.states.as_slice().get(id.index()) else {
                    return session.fail(Error::State);
                };
                match state {
                    State::ByteRange { .. } | State::Sparse { .. } | State::Fail => {}
                    State::Match { .. } => self.graph.has_empty = true,
                    State::Look { look, next } => {
                        prefix |= look;
                        stack.push(*next, session)?;
                    }
                    State::Union { alternates } => {
                        for &next in alternates {
                            stack.push(next, session)?;
                        }
                    }
                    State::BinaryUnion { alt1, alt2 } => {
                        stack.push(*alt2, session)?;
                        stack.push(*alt1, session)?;
                    }
                    State::Capture { next, .. } => {
                        stack.push(*next, session)?;
                    }
                }
            }
            self.graph.prefix_looks |= prefix;
        }
        self.graph.states = storage::compact(self.states, session)?;
        // starts was allocated at exact length by set_starts, so the source's
        // shrink_to_fit has no capacity change here.
        storage::shared(self.graph, session).map(Nfa)
    }
}

fn mapped<A: Admission>(id: &mut Id, remap: &[Id], session: &mut Session<'_, A>) -> Step<()> {
    session.items(1)?;
    let Some(&next) = remap.get(id.index()) else {
        return session.fail(Error::State);
    };
    *id = next;
    Ok(())
}

struct Seen {
    length: usize,
    dense: Vec<Id>,
    sparse: Vec<Id>,
}
impl Seen {
    fn new<A: Admission>(length: usize, session: &mut Session<'_, A>) -> Step<Self> {
        let dense = storage::repeat(Id::ZERO, length, session)?;
        let sparse = storage::repeat(Id::ZERO, length, session)?;
        Ok(Self {
            length: 0,
            dense,
            sparse,
        })
    }
    fn insert<A: Admission>(&mut self, id: Id, session: &mut Session<'_, A>) -> Step<bool> {
        session.items(1)?;
        let Some(&index) = self.sparse.get(id.index()) else {
            return session.fail(Error::State);
        };
        if index.index() < self.length && self.dense[index.index()] == id {
            return Ok(false);
        }
        if self.length >= self.dense.len() {
            return session.fail(Error::State);
        }
        self.dense[self.length] = id;
        self.sparse[id.index()] = Id(self.length as u32);
        self.length += 1;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
