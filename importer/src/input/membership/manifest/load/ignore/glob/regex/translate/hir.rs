// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Base consuming HIR constructors from regex-syntax 0.8.11 hir/mod.rs.
use super::{
    bytes,
    properties::{Look, Properties},
    unicode,
};
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::{Admission, Request};
use std::alloc::Layout;
mod arena;
mod tree;
pub(in super::super) mod view;
pub(super) use tree::Translated;

pub(super) struct Expr {
    pub(super) kind: Kind,
    pub(super) properties: Properties,
}
pub(super) enum Kind {
    Empty,
    Literal(Literal),
    Bytes(bytes::Class),
    Unicode(unicode::Class),
    Look(Look),
    Concat(crate::input::membership::manifest::storage::Buffer<arena::Id>),
    Alternation(crate::input::membership::manifest::storage::Buffer<arena::Id>),
    Repetition {
        min: u32,
        max: Option<u32>,
        greedy: bool,
        child: arena::Id,
    },
}
/// Owned literal bytes with no public mutation or unadmitted constructor.
pub(super) struct Literal(Vec<u8>);
impl Literal {
    pub(super) fn copy<A: Admission>(bytes: &[u8], session: &mut Session<'_, A>) -> Step<Self> {
        session.items(bytes.len())?;
        let mut owned = Vec::new();
        if !bytes.is_empty() {
            let Ok(layout) = Layout::array::<u8>(bytes.len()) else {
                return session.fail(Error::Overflow);
            };
            session.admit(Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            })?;
            if let Err(error) = owned.try_reserve_exact(bytes.len()) {
                return session.fail(Error::Allocation(error));
            }
            owned.extend_from_slice(bytes);
        }
        Ok(Self(owned))
    }
    pub(super) fn bytes(&self) -> &[u8] {
        &self.0
    }
}
impl Expr {
    fn into_kind(self) -> Kind {
        // Pinned into_kind replaces only the kind with Empty before Drop;
        // unlike into_parts, it does not allocate replacement properties.
        let Self { kind, properties } = self;
        drop(properties);
        kind
    }
    pub(super) fn unicode<A: Admission>(
        class: unicode::Class,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        if class.is_empty() {
            return Self::fail(session);
        }
        if let Some(c) = class.singleton() {
            let mut encoded = [0; 4];
            let bytes = c.encode_utf8(&mut encoded).as_bytes();
            // Pinned ClassUnicode::literal owns a String before into_bytes.
            session.string(bytes.len())?;
            let literal = Literal::copy(bytes, session)?;
            return Self::literal(literal, session);
        }
        let properties = Properties::unicode(&class, session)?;
        Ok(Self {
            kind: Kind::Unicode(class),
            properties,
        })
    }
    fn into_parts<A: Admission>(self, session: &mut Session<'_, A>) -> Step<(Kind, Properties)> {
        // Preserve the pinned into_parts replacement-property construction
        // before moving the original kind and properties out.
        let replacement = Properties::empty(session)?;
        let Self { kind, properties } = self;
        drop(replacement);
        Ok((kind, properties))
    }
    pub(super) fn empty<A: Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        let properties = Properties::empty(session)?;
        Ok(Self {
            kind: Kind::Empty,
            properties,
        })
    }
    pub(super) fn literal<A: Admission>(
        literal: Literal,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        if literal.bytes().is_empty() {
            return Self::empty(session);
        }
        let properties = Properties::literal(literal.bytes(), session)?;
        Ok(Self {
            kind: Kind::Literal(literal),
            properties,
        })
    }
    pub(super) fn look<A: Admission>(look: Look, session: &mut Session<'_, A>) -> Step<Self> {
        let properties = Properties::look(look, session)?;
        Ok(Self {
            kind: Kind::Look(look),
            properties,
        })
    }
    pub(super) fn fail<A: Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        let class = bytes::Class::empty();
        let properties = Properties::bytes(&class, session)?;
        Ok(Self {
            kind: Kind::Bytes(class),
            properties,
        })
    }
    pub(super) fn bytes<A: Admission>(
        class: bytes::Class,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        if class.is_empty() {
            return Self::fail(session);
        }
        if let Some(byte) = class.singleton() {
            // Class::literal owns its byte vector before Hir::literal computes
            // properties. Keep the input class alive through this conversion.
            let literal = Literal::copy(&[byte], session)?;
            return Self::literal(literal, session);
        }
        let properties = Properties::bytes(&class, session)?;
        Ok(Self {
            kind: Kind::Bytes(class),
            properties,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};

    fn unicode(values: &[char]) -> unicode::Class {
        let mut chars = crate::input::membership::manifest::storage::Buffer::new();
        for &c in values {
            chars.push(c, &mut Session::new(&mut Account)).unwrap();
        }
        unicode::Class::singletons(chars, &mut Session::new(&mut Account)).unwrap()
    }
    #[test]
    fn unicode_class_normalization_preserves_utf8_lengths() {
        let empty = Expr::unicode(unicode(&[]), &mut Session::new(&mut Account)).unwrap();
        assert!(matches!(empty.kind, Kind::Bytes(_)));
        assert_eq!(empty.properties.get().minimum_len, None);
        let singleton =
            Expr::unicode(unicode(&['é', 'é']), &mut Session::new(&mut Account)).unwrap();
        assert_eq!(
            (
                singleton.properties.get().minimum_len,
                singleton.properties.get().maximum_len
            ),
            (Some(2), Some(2))
        );
        let Kind::Literal(literal) = singleton.kind else {
            panic!("Unicode singleton literal")
        };
        assert_eq!(literal.bytes(), "é".as_bytes());
        let class = Expr::unicode(
            unicode(&['\u{10000}', 'é', '\u{800}']),
            &mut Session::new(&mut Account),
        )
        .unwrap();
        assert!(matches!(class.kind, Kind::Unicode(_)));
        assert_eq!(
            (
                class.properties.get().minimum_len,
                class.properties.get().maximum_len
            ),
            (Some(2), Some(4))
        );
        assert!(class.properties.get().utf8 && !class.properties.get().literal);
    }
    #[test]
    fn unicode_normalization_refusal_covers_string_and_property_ownership() {
        for values in [&['é'][..], &['a', 'z'][..]] {
            let mut count = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            Expr::unicode(unicode(values), &mut Session::new(&mut count)).unwrap();
            for refuse in 0..count.calls {
                let mut count = Count { calls: 0, refuse };
                let mut session = Session::new(&mut count);
                assert!(matches!(
                    Expr::unicode(unicode(values), &mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                assert_eq!(count.calls, refuse + 1);
            }
        }
    }
    #[test]
    fn class_normalization_distinguishes_failure_literal_and_range() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let failed = Expr::bytes(bytes::Class::empty(), &mut session).unwrap();
        assert!(matches!(failed.kind, Kind::Bytes(_)));
        assert_eq!(failed.properties.get().minimum_len, None);
        assert!(failed.properties.get().utf8);
        for byte in [b'a', 0xff] {
            let mut class = bytes::Class::empty();
            class
                .push(bytes::Range::new(byte, byte), &mut session)
                .unwrap();
            let expr = Expr::bytes(class, &mut session).unwrap();
            let Kind::Literal(literal) = expr.kind else {
                panic!("singleton literal")
            };
            assert_eq!(literal.bytes(), &[byte]);
            assert_eq!(expr.properties.get().utf8, byte.is_ascii());
            assert!(expr.properties.get().literal);
        }
        let mut class = bytes::Class::empty();
        class.push(bytes::Range::new(0, 255), &mut session).unwrap();
        let expr = Expr::bytes(class, &mut session).unwrap();
        assert!(matches!(expr.kind, Kind::Bytes(_)));
        assert_eq!(expr.properties.get().minimum_len, Some(1));
        assert_eq!(expr.properties.get().maximum_len, Some(1));
        assert!(!expr.properties.get().utf8 && !expr.properties.get().literal);
        let empty = Expr::literal(Literal::copy(&[], &mut session).unwrap(), &mut session).unwrap();
        assert!(matches!(empty.kind, Kind::Empty));
        assert_eq!(empty.properties.get().minimum_len, Some(0));
        assert!(!empty.properties.get().literal);
        let anchor = Expr::look(Look::End, &mut session).unwrap();
        assert!(matches!(anchor.kind, Kind::Look(Look::End)));
        assert_eq!(anchor.properties.get().look_set, 2);
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: Request) -> Result<(), ()> {
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
    fn singleton_conversion_refuses_before_literal_and_property_ownership() {
        fn run<A: Admission>(session: &mut Session<'_, A>) -> Step<Expr> {
            let mut class = bytes::Class::empty();
            class.push(bytes::Range::new(0xff, 0xff), session)?;
            Expr::bytes(class, session)
        }
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        run(&mut Session::new(&mut count)).unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(run(&mut session), Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
