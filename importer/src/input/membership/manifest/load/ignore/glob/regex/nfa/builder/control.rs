// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Emitter-reachable Thompson control flow, split around child compilation.
use super::{fragment::Fragment, Buffer, Builder, Error, Failure, Id, Session, State, Step};
use rawr_toml::document::Admission;

pub(in super::super) struct Repeat(Continuation);
enum Continuation {
    Optional(Id),
    StarNonEmpty(Id),
    StarNullable(bool),
    Plus(bool),
}
impl Builder {
    fn union<A: Admission>(
        &mut self,
        greedy: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Id, Failure>> {
        let alternates = Buffer::new();
        self.add(
            if greedy {
                State::Union { alternates }
            } else {
                State::UnionReverse { alternates }
            },
            session,
        )
    }
    pub(in super::super) fn anchor<A: Admission>(
        &mut self,
        end: bool,
        reverse: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        let look = if end ^ reverse { 2 } else { 1 };
        Ok(self
            .add(
                State::Look {
                    look,
                    next: Id::ZERO,
                },
                session,
            )?
            .map(|id| Fragment { start: id, end: id }))
    }
    pub(in super::super) fn fail<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        Ok(self
            .add(State::Fail, session)?
            .map(|id| Fragment { start: id, end: id }))
    }
    /// First and second branches have already compiled, in source order.
    pub(in super::super) fn alternate_pair<A: Admission>(
        &mut self,
        first: Fragment,
        second: Fragment,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        let start = match self.union(true, session)? {
            Ok(id) => id,
            Err(e) => return Ok(Err(e)),
        };
        let end = match self.empty(session)? {
            Ok(f) => f.end,
            Err(e) => return Ok(Err(e)),
        };
        for (from, to) in [
            (start, first.start),
            (first.end, end),
            (start, second.start),
            (second.end, end),
        ] {
            if let Err(e) = self.patch(from, to, session)? {
                return Ok(Err(e));
            }
        }
        Ok(Ok(Fragment { start, end }))
    }
    pub(in super::super) fn alternate_next<A: Admission>(
        &mut self,
        alternatives: &Fragment,
        next: Fragment,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        if let Err(e) = self.patch(alternatives.start, next.start, session)? {
            return Ok(Err(e));
        }
        self.patch(next.end, alternatives.end, session)
    }
    /// Transform normalized emitter repetition into a continuation. Any union
    /// required before the child is allocated here, before driver descent.
    pub(in super::super) fn begin_repeat<A: Admission>(
        &mut self,
        min: u32,
        max: Option<u32>,
        greedy: bool,
        minimum_len: Option<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Repeat, Failure>> {
        session.items(1)?;
        let continuation = match (min, max) {
            (0, Some(1)) => Continuation::Optional(match self.union(greedy, session)? {
                Ok(id) => id,
                Err(e) => return Ok(Err(e)),
            }),
            (0, None) if minimum_len.is_some_and(|n| n > 0) => {
                Continuation::StarNonEmpty(match self.union(greedy, session)? {
                    Ok(id) => id,
                    Err(e) => return Ok(Err(e)),
                })
            }
            (0, None) => Continuation::StarNullable(greedy),
            (1, None) => Continuation::Plus(greedy),
            _ => return session.fail(Error::State),
        };
        Ok(Ok(Repeat(continuation)))
    }
    pub(in super::super) fn finish_repeat<A: Admission>(
        &mut self,
        repeat: Repeat,
        child: Fragment,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        session.items(1)?;
        match repeat.0 {
            Continuation::Optional(union) => {
                let empty = match self.empty(session)? {
                    Ok(f) => f.end,
                    Err(e) => return Ok(Err(e)),
                };
                for (from, to) in [(union, child.start), (union, empty), (child.end, empty)] {
                    if let Err(e) = self.patch(from, to, session)? {
                        return Ok(Err(e));
                    }
                }
                Ok(Ok(Fragment {
                    start: union,
                    end: empty,
                }))
            }
            Continuation::StarNonEmpty(union) => {
                if let Err(e) = self.patch(union, child.start, session)? {
                    return Ok(Err(e));
                }
                if let Err(e) = self.patch(child.end, union, session)? {
                    return Ok(Err(e));
                }
                Ok(Ok(Fragment {
                    start: union,
                    end: union,
                }))
            }
            Continuation::Plus(greedy) => {
                let union = match self.union(greedy, session)? {
                    Ok(id) => id,
                    Err(e) => return Ok(Err(e)),
                };
                if let Err(e) = self.patch(child.end, union, session)? {
                    return Ok(Err(e));
                }
                if let Err(e) = self.patch(union, child.start, session)? {
                    return Ok(Err(e));
                }
                Ok(Ok(Fragment {
                    start: child.start,
                    end: union,
                }))
            }
            Continuation::StarNullable(greedy) => {
                let plus = match self.union(greedy, session)? {
                    Ok(id) => id,
                    Err(e) => return Ok(Err(e)),
                };
                if let Err(e) = self.patch(child.end, plus, session)? {
                    return Ok(Err(e));
                }
                if let Err(e) = self.patch(plus, child.start, session)? {
                    return Ok(Err(e));
                }
                let question = match self.union(greedy, session)? {
                    Ok(id) => id,
                    Err(e) => return Ok(Err(e)),
                };
                let empty = match self.empty(session)? {
                    Ok(f) => f.end,
                    Err(e) => return Ok(Err(e)),
                };
                for (from, to) in [(question, child.start), (question, empty), (plus, empty)] {
                    if let Err(e) = self.patch(from, to, session)? {
                        return Ok(Err(e));
                    }
                }
                Ok(Ok(Fragment {
                    start: question,
                    end: empty,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn repeat<A: Admission>(
        builder: &mut Builder,
        min: u32,
        max: Option<u32>,
        greedy: bool,
        nullable: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        let pending =
            match builder.begin_repeat(min, max, greedy, Some(usize::from(!nullable)), session)? {
                Ok(r) => r,
                Err(e) => return Ok(Err(e)),
            };
        let child = match builder.literal(if nullable { b"" } else { b"a" }, false, session)? {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        builder.finish_repeat(pending, child, session)
    }
    fn accepts(builder: &Builder, fragment: &Fragment, bytes: &[u8]) -> bool {
        let mut pending = vec![(fragment.start, 0)];
        let mut seen = Vec::new();
        while let Some((id, pos)) = pending.pop() {
            if id == fragment.end && pos == bytes.len() {
                return true;
            }
            if seen.contains(&(id, pos)) {
                continue;
            }
            seen.push((id, pos));
            match &builder.states.as_slice()[id.0 as usize] {
                State::Empty { next } if id != fragment.end => pending.push((*next, pos)),
                State::Union { alternates } | State::UnionReverse { alternates } => {
                    pending.extend(alternates.as_slice().iter().map(|id| (*id, pos)))
                }
                State::ByteRange { trans }
                    if bytes
                        .get(pos)
                        .is_some_and(|b| trans.start <= *b && *b <= trans.end) =>
                {
                    pending.push((trans.next, pos + 1))
                }
                _ => {}
            }
        }
        false
    }
    #[test]
    fn repetition_languages_and_greediness_preserve_construction_order() {
        for greedy in [true, false] {
            for (min, max, expected) in [
                (0, Some(1), [true, true, false]),
                (0, None, [true, true, true]),
                (1, None, [false, true, true]),
            ] {
                let mut account = Account;
                let mut session = Session::new(&mut account);
                let mut builder = Builder::new();
                let fragment = repeat(&mut builder, min, max, greedy, false, &mut session)
                    .unwrap()
                    .unwrap();
                for (bytes, expected) in [b"".as_slice(), b"a", b"aa"].into_iter().zip(expected) {
                    assert_eq!(accepts(&builder, &fragment, bytes), expected);
                }
                let union = if min == 0 {
                    fragment.start
                } else {
                    fragment.end
                };
                assert_eq!(
                    matches!(
                        builder.states.as_slice()[union.0 as usize],
                        State::Union { .. }
                    ),
                    greedy
                );
                assert_eq!(union.0 == 0, min == 0);
            }
        }
    }
    #[test]
    fn nullable_star_uses_plus_then_question_and_anchors_reverse() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        let fragment = repeat(&mut builder, 0, None, true, true, &mut session)
            .unwrap()
            .unwrap();
        assert_eq!(fragment.start.0, 2);
        assert_eq!(fragment.end.0, 3);
        assert!(accepts(&builder, &fragment, b""));
        assert!(!accepts(&builder, &fragment, b"a"));
        for (end, reverse) in [(false, false), (false, true), (true, false), (true, true)] {
            let f = builder.anchor(end, reverse, &mut session).unwrap().unwrap();
            let State::Look { look, .. } = builder.states.as_slice()[f.start.0 as usize] else {
                panic!("anchor");
            };
            assert_eq!(look, if end ^ reverse { 2 } else { 1 });
        }
    }
    #[test]
    fn alternatives_keep_source_priority_and_accept_all_branches() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        let first = builder
            .literal(b"ab", false, &mut session)
            .unwrap()
            .unwrap();
        let first_start = first.start;
        let second = builder.literal(b"c", false, &mut session).unwrap().unwrap();
        let second_start = second.start;
        let fragment = builder
            .alternate_pair(first, second, &mut session)
            .unwrap()
            .unwrap();
        let third = builder.literal(b"d", false, &mut session).unwrap().unwrap();
        let third_start = third.start;
        builder
            .alternate_next(&fragment, third, &mut session)
            .unwrap()
            .unwrap();
        let State::Union { alternates } = &builder.states.as_slice()[fragment.start.0 as usize]
        else {
            panic!("union");
        };
        assert_eq!(
            alternates.as_slice(),
            &[first_start, second_start, third_start]
        );
        for bytes in [b"ab".as_slice(), b"c", b"d"] {
            assert!(accepts(&builder, &fragment, bytes));
        }
        assert!(!accepts(&builder, &fragment, b"a"));
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
    #[test]
    fn repetition_refusal_is_terminal_before_and_after_child() {
        for (min, max, nullable) in [
            (0, Some(1), false),
            (0, None, false),
            (0, None, true),
            (1, None, false),
        ] {
            let mut count = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            repeat(
                &mut Builder::new(),
                min,
                max,
                false,
                nullable,
                &mut Session::new(&mut count),
            )
            .unwrap()
            .unwrap();
            for refuse in 0..count.calls {
                let mut count = Count { calls: 0, refuse };
                let mut session = Session::new(&mut count);
                assert!(matches!(
                    repeat(&mut Builder::new(), min, max, false, nullable, &mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                assert_eq!(count.calls, refuse + 1);
            }
        }
    }
}
