// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned forward c_unicode_class dispatch; full HIR driver remains separate.
mod cache;
mod compiler;
mod ranges;
mod reverse;
use super::super::super::translate::unicode::Class;
use super::{fragment::Fragment, Buffer, Builder, Failure, Session, State, Step, Transition};
pub(in super::super) use compiler::Pending;
use rawr_toml::document::Admission;
pub(in super::super) use reverse::Suffix;

impl Builder {
    pub(in super::super) fn unicode_forward<A: Admission>(
        &mut self,
        class: &Class,
        pending: &mut Pending,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        session.items(1)?;
        if class.is_ascii() {
            return self.unicode_ascii(class, session);
        }
        let mut compiler = match compiler::Compiler::new(self, pending, session)? {
            Ok(compiler) => compiler,
            Err(failure) => return Ok(Err(failure)),
        };
        let mut ranges = class.ranges();
        loop {
            session.items(1)?;
            let Some((start, end)) = ranges.next() else {
                break;
            };
            let mut sequences = ranges::Ranges::new(start, end, session)?;
            while let Some(sequence) = sequences.next(session)? {
                if let Err(failure) = compiler.add(sequence.as_slice(), session)? {
                    return Ok(Err(failure));
                }
            }
        }
        compiler.finish(session)
    }
    fn unicode_ascii<A: Admission>(
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
                    start: start as u8,
                    end: stop as u8,
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
    fn class<A: Admission>(chars: &[char], session: &mut Session<'_, A>) -> Step<Class> {
        let mut input = Buffer::new();
        for &c in chars {
            input.push(c, session)?;
        }
        Class::singletons(input, session)
    }
    #[test]
    fn ascii_dispatch_preserves_sparse_end_order_including_empty() {
        for chars in [&[][..], &['a', 'b', 'd']] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let class = class(chars, &mut session).unwrap();
            let mut builder = Builder::new();
            let fragment = builder
                .unicode_forward(&class, &mut Pending::new(), &mut session)
                .unwrap()
                .unwrap();
            assert_eq!(fragment.end.0, 0);
            assert_eq!(fragment.start.0, 1);
            assert_eq!(builder.states.len(), 2);
            let State::Sparse { transitions } = &builder.states.as_slice()[1] else {
                panic!("sparse ASCII");
            };
            assert_eq!(transitions.len(), if chars.is_empty() { 0 } else { 2 });
        }
    }
    fn accepts(builder: &Builder, fragment: &Fragment, c: char) -> bool {
        let mut bytes = [0; 4];
        let mut id = fragment.start;
        for byte in c.encode_utf8(&mut bytes).as_bytes() {
            let State::Sparse { transitions } = &builder.states.as_slice()[id.0 as usize] else {
                return false;
            };
            let Some(edge) = transitions
                .as_slice()
                .iter()
                .find(|t| t.start <= *byte && *byte <= t.end)
            else {
                return false;
            };
            id = edge.next;
        }
        id == fragment.end
    }
    #[test]
    fn canonical_classes_feed_sorted_sequences_and_reuse_pending_state() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut builder = Builder::new();
        let mut pending = Pending::new();
        for chars in [&['é', 'a', '😀', 'ê'][..], &['中', 'b']] {
            let class = class(chars, &mut session).unwrap();
            let fragment = builder
                .unicode_forward(&class, &mut pending, &mut session)
                .unwrap()
                .unwrap();
            for c in ['a', 'b', 'é', 'ê', 'ë', '中', '😀', '😁'] {
                assert_eq!(accepts(&builder, &fragment, c), chars.contains(&c));
            }
        }
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
    fn dispatch_refusal_stays_terminal_for_ascii_and_unicode() {
        let mut account = Account;
        let mut setup = Session::new(&mut account);
        for chars in [&['a'][..], &['é', '中']] {
            let class = class(chars, &mut setup).unwrap();
            let mut pending = Pending::new();
            Builder::new()
                .unicode_forward(&class, &mut pending, &mut setup)
                .unwrap()
                .unwrap();
            let mut count = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            Builder::new()
                .unicode_forward(&class, &mut pending, &mut Session::new(&mut count))
                .unwrap()
                .unwrap();
            for refuse in 0..count.calls {
                let mut count = Count { calls: 0, refuse };
                let mut session = Session::new(&mut count);
                assert!(matches!(
                    Builder::new().unicode_forward(&class, &mut pending, &mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                assert_eq!(count.calls, refuse + 1);
            }
        }
    }
}
