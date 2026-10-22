// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned c_empty/c_literal/c_range/c_byte_class fragments before finalization.
use super::super::super::translate::bytes::Class;
use super::{Buffer, Builder, Failure, Id, Session, State, Step, Transition};
use rawr_toml::document::Admission;

#[derive(Debug, PartialEq, Eq)]
pub(in super::super) struct Fragment {
    pub(in super::super) start: Id,
    pub(in super::super) end: Id,
}
impl Builder {
    pub(in super::super) fn empty<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        Ok(self
            .add(State::Empty { next: Id::ZERO }, session)?
            .map(|id| Fragment { start: id, end: id }))
    }
    fn range<A: Admission>(
        &mut self,
        start: u8,
        end: u8,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        Ok(self
            .add(
                State::ByteRange {
                    trans: Transition {
                        start,
                        end,
                        next: Id::ZERO,
                    },
                },
                session,
            )?
            .map(|id| Fragment { start: id, end: id }))
    }
    pub(in super::super) fn literal<A: Admission>(
        &mut self,
        bytes: &[u8],
        reverse: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        session.check()?;
        let mut bytes = bytes.iter().copied();
        session.items(1)?;
        let first = if reverse {
            bytes.next_back()
        } else {
            bytes.next()
        };
        let Some(first) = first else {
            return self.empty(session);
        };
        let mut fragment = match self.range(first, first, session)? {
            Ok(fragment) => fragment,
            Err(failure) => return Ok(Err(failure)),
        };
        loop {
            session.items(1)?;
            let byte = if reverse {
                bytes.next_back()
            } else {
                bytes.next()
            };
            let Some(byte) = byte else {
                return Ok(Ok(fragment));
            };
            let next = match self.range(byte, byte, session)? {
                Ok(fragment) => fragment,
                Err(failure) => return Ok(Err(failure)),
            };
            if let Err(failure) = self.patch(fragment.end, next.start, session)? {
                return Ok(Err(failure));
            }
            fragment.end = next.end;
        }
    }
    pub(in super::super) fn byte_class<A: Admission>(
        &mut self,
        class: &Class,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        let end = match self.empty(session)? {
            Ok(fragment) => fragment.end,
            Err(failure) => return Ok(Err(failure)),
        };
        let mut transitions = Buffer::new();
        transitions.reserve_more(class.ranges().len(), session)?;
        let mut ranges = class.ranges();
        loop {
            session.items(1)?;
            let Some((start, stop)) = ranges.next() else {
                break;
            };
            transitions.push(
                Transition {
                    start,
                    end: stop,
                    next: end,
                },
                session,
            )?;
        }
        Ok(self
            .add(State::Sparse { transitions }, session)?
            .map(|start| Fragment { start, end }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn literals_link_bytes_in_compilation_direction() {
        for reverse in [false, true] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let mut builder = Builder::new();
            let fragment = builder
                .literal(b"abc", reverse, &mut session)
                .unwrap()
                .unwrap();
            let mut current = fragment.start;
            let mut actual = Vec::new();
            loop {
                let State::ByteRange { trans } = &builder.states.as_slice()[current.0 as usize]
                else {
                    panic!("byte range");
                };
                assert_eq!(trans.start, trans.end);
                actual.push(trans.start);
                if current == fragment.end {
                    break;
                }
                current = trans.next;
            }
            assert_eq!(actual, if reverse { b"cba" } else { b"abc" });
            let empty = builder
                .literal(b"", reverse, &mut session)
                .unwrap()
                .unwrap();
            assert_eq!(empty.start, empty.end);
            assert!(matches!(
                builder.states.as_slice()[empty.start.0 as usize],
                State::Empty { .. }
            ));
        }
    }
    #[test]
    fn empty_byte_class_retains_sparse_failure_until_finalization() {
        // A canonical empty class is the HIR failure expression. The source
        // still emits its empty target and zero-transition sparse state.
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        let fragment = builder
            .byte_class(&Class::empty(), &mut session)
            .unwrap()
            .unwrap();
        assert_eq!(builder.states.len(), 2);
        assert_eq!(fragment.end, Id::ZERO);
        let State::Sparse { transitions } = &builder.states.as_slice()[fragment.start.0 as usize]
        else {
            panic!("sparse");
        };
        assert_eq!(transitions.len(), 0);
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    fn class() -> Class {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut bytes = Buffer::new();
        for byte in *b"abd" {
            bytes.push(byte, &mut session).unwrap();
        }
        Class::singletons(bytes, &mut session).unwrap()
    }
    #[test]
    fn byte_class_targets_one_empty_end_in_canonical_order() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        let fragment = builder.byte_class(&class(), &mut session).unwrap().unwrap();
        let State::Sparse { transitions } = &builder.states.as_slice()[fragment.start.0 as usize]
        else {
            panic!("sparse");
        };
        let ranges: Vec<_> = transitions
            .as_slice()
            .iter()
            .map(|t| (t.start, t.end, t.next))
            .collect();
        assert_eq!(
            ranges,
            [(b'a', b'b', fragment.end), (b'd', b'd', fragment.end)]
        );
        let class = class();
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        Builder::new()
            .byte_class(&class, &mut Session::new(&mut count))
            .unwrap()
            .unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                Builder::new().byte_class(&class, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
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
    #[test]
    fn literal_refusal_prevents_later_state_growth() {
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        Builder::new()
            .literal(b"abcdef", false, &mut Session::new(&mut count))
            .unwrap()
            .unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                Builder::new().literal(b"abcdef", false, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
