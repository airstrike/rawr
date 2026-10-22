// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! Pinned alternation normalization and iterative common-prefix factoring.
use super::super::super::{bytes, unicode};
use super::{singleton, Arena, Buffer, Error, Expr, Kind, Properties, Session, Step};
use rawr_toml::document::Admission;

impl Arena {
    pub(in super::super) fn alternation<A: Admission>(
        &mut self,
        mut input: Buffer<Expr>,
        session: &mut Session<'_, A>,
    ) -> Step<Expr> {
        let mut prefixes: Buffer<Buffer<Expr>> = Buffer::new();
        let mut result = loop {
            let mut children = Buffer::new();
            // Pinned Vec::with_capacity precedes consuming any child.
            children.reserve_more(input.len(), session)?;
            for child in input.into_iter() {
                session.items(1)?;
                let (kind, properties) = child.into_parts(session)?;
                match kind {
                    Kind::Alternation(nested) => {
                        children.reserve_more(nested.len(), session)?;
                        for id in nested.into_iter() {
                            children.push(self.take(id, session)?, session)?;
                        }
                    }
                    kind => {
                        children.push(Expr { kind, properties }, session)?;
                    }
                }
            }
            if children.len() == 0 {
                break Expr::fail(session)?;
            }
            if children.len() == 1 {
                break match children.pop() {
                    Some(child) => child,
                    None => return session.fail(Error::State),
                };
            }
            if let Some(values) = singleton::chars(children.as_slice(), session)? {
                break Expr::unicode(unicode::Class::singletons(values, session)?, session)?;
            }
            if let Some(values) = singleton::bytes(children.as_slice(), session)? {
                break Expr::bytes(bytes::Class::singletons(values, session)?, session)?;
            }
            if let Some(class) = singleton::class_chars(children.as_slice(), session)? {
                break Expr::unicode(class, session)?;
            }
            if let Some(class) = singleton::class_bytes(children.as_slice(), session)? {
                break Expr::bytes(class, session)?;
            }
            let length = self.common_prefix(children.as_slice(), session)?;
            if length == 0 {
                let properties = Properties::union_iter(
                    children.as_slice().iter().map(|child| &child.properties),
                    session,
                )?;
                let mut ids = Buffer::new();
                for child in children.into_iter() {
                    ids.push(self.retain(child, session)?, session)?;
                }
                break Expr {
                    kind: Kind::Alternation(ids),
                    properties,
                };
            }
            let mut prefix = Buffer::new();
            let mut suffixes = Buffer::new();
            for child in children.into_iter() {
                let kind = child.into_kind();
                let Kind::Concat(mut ids) = kind else {
                    return session.fail(Error::State);
                };
                // split_off allocates the suffix while the original prefix
                // allocation remains retained.
                let mut suffix_ids = Buffer::new();
                suffix_ids.reserve_more(ids.len() - length, session)?;
                for &id in &ids.as_slice()[length..] {
                    suffix_ids.push(id, session)?;
                }
                ids.truncate(length);
                let mut suffix = Buffer::new();
                for id in suffix_ids.into_iter() {
                    suffix.push(self.take(id, session)?, session)?;
                }
                suffixes.push(self.concat(suffix, session)?, session)?;
                if prefix.len() == 0 {
                    for id in ids.into_iter() {
                        prefix.push(self.take(id, session)?, session)?;
                    }
                } else {
                    for id in ids.into_iter() {
                        self.discard(id, session)?;
                    }
                }
            }
            prefixes.push(prefix, session)?;
            input = suffixes;
        };
        while let Some(mut prefix) = prefixes.pop() {
            prefix.push(result, session)?;
            result = self.concat(prefix, session)?;
        }
        Ok(result)
    }

    fn common_prefix<A: Admission>(
        &self,
        children: &[Expr],
        session: &mut Session<'_, A>,
    ) -> Step<usize> {
        session.items(1)?;
        let Some(first) = children.first() else {
            return Ok(0);
        };
        let Kind::Concat(prefix) = &first.kind else {
            return Ok(0);
        };
        let mut length = prefix.len();
        if length == 0 {
            return Ok(0);
        }
        for child in &children[1..] {
            session.items(1)?;
            let Kind::Concat(other) = &child.kind else {
                return Ok(0);
            };
            let mut common = 0;
            for (&a, &b) in prefix.as_slice()[..length].iter().zip(other.as_slice()) {
                if !self.equal(a, b, session)? {
                    break;
                }
                common += 1;
            }
            length = common;
            if length == 0 {
                return Ok(0);
            }
        }
        Ok(length)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{Literal, Look};
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn literal<A: Admission>(value: &[u8], session: &mut Session<'_, A>) -> Step<Expr> {
        Expr::literal(Literal::copy(value, session)?, session)
    }
    fn input<A: Admission>(
        values: impl IntoIterator<Item = Expr>,
        session: &mut Session<'_, A>,
    ) -> Step<Buffer<Expr>> {
        let mut result = Buffer::new();
        for value in values {
            result.push(value, session)?;
        }
        Ok(result)
    }
    #[test]
    fn singleton_classes_preserve_byte_and_unicode_distinction() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        for (a, b, utf8) in [
            (&b"a"[..], &b"b"[..], true),
            (&[128][..], &[255][..], false),
        ] {
            let children = input(
                [
                    literal(a, &mut session).unwrap(),
                    literal(b, &mut session).unwrap(),
                ],
                &mut session,
            )
            .unwrap();
            let result = arena.alternation(children, &mut session).unwrap();
            assert_eq!(result.properties.get().utf8, utf8);
            assert!(matches!(result.kind, Kind::Unicode(_)) == utf8);
            assert!(matches!(result.kind, Kind::Bytes(_)) != utf8);
        }
    }
    fn factored<A: Admission>(arena: &mut Arena, session: &mut Session<'_, A>) -> Step<Expr> {
        let mut branches = Buffer::new();
        for suffix in [b"ab", b"cd"] {
            let children = input(
                [Expr::look(Look::Start, session)?, literal(suffix, session)?],
                session,
            )?;
            branches.push(arena.concat(children, session)?, session)?;
        }
        arena.alternation(branches, session)
    }
    #[test]
    fn common_prefix_factors_and_zero_repeat_releases_alternation() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let result = factored(&mut arena, &mut session).unwrap();
        let Kind::Concat(children) = &result.kind else {
            panic!("factored concat");
        };
        assert_eq!(children.len(), 2);
        assert!(matches!(
            arena
                .get(children.as_slice()[0], &mut session)
                .unwrap()
                .kind,
            Kind::Look(Look::Start)
        ));
        assert!(matches!(
            arena
                .get(children.as_slice()[1], &mut session)
                .unwrap()
                .kind,
            Kind::Alternation(_)
        ));
        arena
            .repetition(result, 0, Some(0), true, &mut session)
            .unwrap();
        assert!(arena.0.as_slice().iter().all(Option::is_none));
    }
    #[test]
    fn nested_alternatives_keep_order_and_full_prefix_keeps_empty_suffix() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let children = input(
            [
                literal(b"ab", &mut session).unwrap(),
                literal(b"cd", &mut session).unwrap(),
            ],
            &mut session,
        )
        .unwrap();
        let nested = arena.alternation(children, &mut session).unwrap();
        let children = input(
            [nested, literal(b"ef", &mut session).unwrap()],
            &mut session,
        )
        .unwrap();
        let result = arena.alternation(children, &mut session).unwrap();
        let Kind::Alternation(ids) = &result.kind else {
            panic!("alternation");
        };
        assert_eq!(ids.len(), 3);
        for (&id, expected) in ids.as_slice().iter().zip([b"ab", b"cd", b"ef"]) {
            let Kind::Literal(bytes) = &arena.get(id, &mut session).unwrap().kind else {
                panic!("literal");
            };
            assert_eq!(bytes.bytes(), expected);
        }
        arena
            .repetition(result, 0, Some(0), true, &mut session)
            .unwrap();
        let mut branches = Buffer::new();
        for end in [false, true] {
            let mut children = input(
                [
                    Expr::look(Look::Start, &mut session).unwrap(),
                    literal(b"ab", &mut session).unwrap(),
                ],
                &mut session,
            )
            .unwrap();
            if end {
                children
                    .push(Expr::look(Look::End, &mut session).unwrap(), &mut session)
                    .unwrap();
            }
            branches
                .push(arena.concat(children, &mut session).unwrap(), &mut session)
                .unwrap();
        }
        let result = arena.alternation(branches, &mut session).unwrap();
        let Kind::Concat(ids) = &result.kind else {
            panic!("factored concat");
        };
        assert_eq!(ids.len(), 3);
        let Kind::Alternation(suffix) = &arena.get(ids.as_slice()[2], &mut session).unwrap().kind
        else {
            panic!("suffix");
        };
        assert!(matches!(
            arena.get(suffix.as_slice()[0], &mut session).unwrap().kind,
            Kind::Empty
        ));
        assert!(matches!(
            arena.get(suffix.as_slice()[1], &mut session).unwrap().kind,
            Kind::Look(Look::End)
        ));
        arena
            .repetition(result, 0, Some(0), true, &mut session)
            .unwrap();
        assert!(arena.0.as_slice().iter().all(Option::is_none));
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
    fn factoring_refusal_stops_following_work() {
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        factored(&mut Arena::new(), &mut Session::new(&mut count)).unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                factored(&mut Arena::new(), &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
