// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Pinned Thompson Builder::add/patch and semantic size-limit ordering.
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;
mod control;
mod literal;
pub(super) use literal::Trie;
mod fragment;
pub(super) use control::Repeat;
pub(super) use fragment::Fragment;
mod finalize;
mod pattern;
mod utf8;
pub(super) use utf8::{Pending, Suffix};

// Pinned builder.rs:state_has_small_size asserts 32/16 on 64/32-bit targets.
// This is the source's semantic estimate, not our larger admitted State layout.
const SOURCE_STATE_BYTES: usize = 4 * std::mem::size_of::<usize>();
const INDEX_LIMIT: usize = i32::MAX as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Id(pub(super) u32);
impl Id {
    pub(super) fn index(self) -> usize {
        self.0 as usize
    }
    pub(super) const ZERO: Self = Self(0);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Transition {
    pub(super) start: u8,
    pub(super) end: u8,
    pub(super) next: Id,
}
pub(super) enum State {
    Empty { next: Id },
    ByteRange { trans: Transition },
    Sparse { transitions: Buffer<Transition> },
    Look { look: u32, next: Id },
    CaptureStart { pattern: u32, group: u32, next: Id },
    CaptureEnd { pattern: u32, group: u32, next: Id },
    Union { alternates: Buffer<Id> },
    UnionReverse { alternates: Buffer<Id> },
    Fail,
    Match { pattern: u32 },
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    TooManyStates(usize),
    TooManyPatterns(usize),
    SizeLimit(usize),
    Captures(super::finalize::captures::Failure),
}
pub(super) struct Builder {
    pattern: Option<u32>,
    starts: Buffer<Id>,
    captures: Buffer<Buffer<Option<std::sync::Arc<str>>>>,
    states: Buffer<State>,
    memory_states: usize,
    size_limit: Option<usize>,
}
impl Builder {
    #[cfg(test)]
    pub(super) fn states(&self) -> &[State] {
        self.states.as_slice()
    }
    pub(super) fn new() -> Self {
        Self {
            pattern: None,
            starts: Buffer::new(),
            captures: Buffer::new(),
            states: Buffer::new(),
            memory_states: 0,
            size_limit: None,
        }
    }
    pub(super) fn set_size_limit<A: Admission>(
        &mut self,
        limit: Option<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        session.check()?;
        self.size_limit = limit;
        self.check_size(session)
    }
    pub(super) fn add<A: Admission>(
        &mut self,
        state: State,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Id, Failure>> {
        session.items(1)?;
        if self.states.len() >= INDEX_LIMIT {
            return Ok(Err(Failure::TooManyStates(self.states.len())));
        }
        let id = Id(self.states.len() as u32);
        let (length, width) = match &state {
            State::Sparse { transitions } => (transitions.len(), 8),
            State::Union { alternates } | State::UnionReverse { alternates } => {
                (alternates.len(), 4)
            }
            _ => (0, 0),
        };
        let Some(memory) = length
            .checked_mul(width)
            .and_then(|n| self.memory_states.checked_add(n))
        else {
            return session.fail(Error::Overflow);
        };
        self.memory_states = memory;
        self.states.push(state, session)?;
        // The source limit is checked after insertion, while prospective
        // cumulative admission already guarded the actual allocation above.
        Ok(self.check_size(session)?.map(|()| id))
    }
    pub(super) fn patch<A: Admission>(
        &mut self,
        from: Id,
        to: Id,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        session.items(1)?;
        let Some(state) = self.states.as_mut_slice().get_mut(from.0 as usize) else {
            return session.fail(Error::State);
        };
        let grew = match state {
            State::Empty { next }
            | State::Look { next, .. }
            | State::CaptureStart { next, .. }
            | State::CaptureEnd { next, .. } => {
                *next = to;
                false
            }
            State::ByteRange { trans } => {
                trans.next = to;
                false
            }
            State::Union { alternates } | State::UnionReverse { alternates } => {
                alternates.push(to, session)?;
                self.memory_states = match self.memory_states.checked_add(4) {
                    Some(memory) => memory,
                    None => return session.fail(Error::Overflow),
                };
                true
            }
            State::Sparse { .. } => return session.fail(Error::State),
            State::Fail | State::Match { .. } => false,
        };
        if grew {
            self.check_size(session)
        } else {
            Ok(Ok(()))
        }
    }
    fn check_size<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Result<(), Failure>> {
        session.check()?;
        let Some(memory) = self
            .states
            .len()
            .checked_mul(SOURCE_STATE_BYTES)
            .and_then(|n| n.checked_add(self.memory_states))
        else {
            return session.fail(Error::Overflow);
        };
        Ok(match self.size_limit {
            Some(limit) if memory > limit => Err(Failure::SizeLimit(limit)),
            _ => Ok(()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn semantic_limit_checks_follow_insert_and_union_patch() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        builder
            .set_size_limit(Some(SOURCE_STATE_BYTES), &mut session)
            .unwrap()
            .unwrap();
        let union = builder
            .add(
                State::Union {
                    alternates: Buffer::new(),
                },
                &mut session,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            builder.patch(union, union, &mut session).unwrap(),
            Err(Failure::SizeLimit(SOURCE_STATE_BYTES))
        );
        let State::Union { alternates } = &builder.states.as_slice()[0] else {
            panic!("union");
        };
        assert_eq!(alternates.as_slice(), &[union]);
        let result = builder.add(State::Fail, &mut session).unwrap();
        assert_eq!(result, Err(Failure::SizeLimit(SOURCE_STATE_BYTES)));
        assert_eq!(builder.states.len(), 2);
        // Failure is the pinned build result, not an admission refusal.
        session.check().unwrap();
    }
    #[test]
    fn patch_keeps_union_order_and_non_growing_state_behavior() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        let empty = builder
            .add(State::Empty { next: Id::ZERO }, &mut session)
            .unwrap()
            .unwrap();
        let range = builder
            .add(
                State::ByteRange {
                    trans: Transition {
                        start: b'a',
                        end: b'z',
                        next: Id::ZERO,
                    },
                },
                &mut session,
            )
            .unwrap()
            .unwrap();
        let union = builder
            .add(
                State::UnionReverse {
                    alternates: Buffer::new(),
                },
                &mut session,
            )
            .unwrap()
            .unwrap();
        for target in [range, empty] {
            builder.patch(union, target, &mut session).unwrap().unwrap();
        }
        builder.patch(empty, range, &mut session).unwrap().unwrap();
        let State::UnionReverse { alternates } = &builder.states.as_slice()[2] else {
            panic!("union");
        };
        assert_eq!(alternates.as_slice(), &[range, empty]);
        assert!(matches!(builder.states.as_slice()[0], State::Empty { next } if next == range));
        assert_eq!(std::mem::size_of::<Transition>(), 8);
        assert_eq!(std::mem::size_of::<Id>(), 4);
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let call = self.calls;
            self.calls += 1;
            if call == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    fn exercise<A: Admission>(session: &mut Session<'_, A>) -> Step<()> {
        let mut builder = Builder::new();
        let id = builder
            .add(
                State::Union {
                    alternates: Buffer::new(),
                },
                session,
            )?
            .unwrap();
        for _ in 0..9 {
            builder.patch(id, id, session)?.unwrap();
        }
        Ok(())
    }
    #[test]
    fn refusal_precedes_state_and_transition_growth() {
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        exercise(&mut Session::new(&mut count)).unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(exercise(&mut session), Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
