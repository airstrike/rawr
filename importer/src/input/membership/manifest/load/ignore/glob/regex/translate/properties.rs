// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Pinned base HIR properties, computed before publishing each expression.
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::{Admission, Request};
use std::alloc::Layout;

/// Private one-record allocation: fallible allocation replaces Box::new without
/// introducing a shrinking conversion or relying on allocator bonus capacity.
pub(super) struct Properties(Vec<Record>);
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Record {
    pub(super) minimum_len: Option<usize>,
    pub(super) maximum_len: Option<usize>,
    pub(super) look_set: u32,
    pub(super) look_set_prefix: u32,
    pub(super) look_set_suffix: u32,
    pub(super) look_set_prefix_any: u32,
    pub(super) look_set_suffix_any: u32,
    pub(super) utf8: bool,
    pub(super) explicit_captures_len: usize,
    pub(super) static_explicit_captures_len: Option<usize>,
    pub(super) literal: bool,
    pub(super) alternation_literal: bool,
}
#[derive(Clone, Copy)]
pub(super) enum Look {
    Start,
    End,
}
impl Properties {
    pub(super) fn copy<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        Self::retain(self.get().clone(), session)
    }
    pub(super) fn unicode<A: Admission>(
        class: &super::unicode::Class,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let mut record = Record::base();
        record.minimum_len = class.minimum_len();
        record.maximum_len = class.maximum_len();
        Self::retain(record, session)
    }
    pub(super) fn union<A: Admission>(
        children: &[&Self],
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        Self::union_iter(children.iter().copied(), session)
    }
    pub(super) fn union_iter<'a, A: Admission>(
        children: impl IntoIterator<Item = &'a Self>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let mut children = children.into_iter();
        let first = children.next();
        let fix = if first.is_none() { 0 } else { u32::MAX };
        let mut record = Record {
            minimum_len: None,
            maximum_len: None,
            look_set: 0,
            look_set_prefix: fix,
            look_set_suffix: fix,
            look_set_prefix_any: 0,
            look_set_suffix_any: 0,
            utf8: true,
            explicit_captures_len: 0,
            static_explicit_captures_len: first.and_then(|p| p.get().static_explicit_captures_len),
            literal: false,
            alternation_literal: true,
        };
        let (mut min_poisoned, mut max_poisoned) = (false, false);
        for child in first.into_iter().chain(children) {
            session.items(1)?;
            let p = child.get();
            record.look_set |= p.look_set;
            record.look_set_prefix &= p.look_set_prefix;
            record.look_set_suffix &= p.look_set_suffix;
            record.look_set_prefix_any |= p.look_set_prefix_any;
            record.look_set_suffix_any |= p.look_set_suffix_any;
            record.utf8 &= p.utf8;
            record.explicit_captures_len = record
                .explicit_captures_len
                .saturating_add(p.explicit_captures_len);
            if record.static_explicit_captures_len != p.static_explicit_captures_len {
                record.static_explicit_captures_len = None;
            }
            record.alternation_literal &= p.literal;
            if !min_poisoned {
                match p.minimum_len {
                    Some(value) => {
                        if record.minimum_len.is_none_or(|previous| value < previous) {
                            record.minimum_len = Some(value);
                        }
                    }
                    None => {
                        record.minimum_len = None;
                        min_poisoned = true;
                    }
                }
            }
            if !max_poisoned {
                match p.maximum_len {
                    Some(value) => {
                        if record.maximum_len.is_none_or(|previous| value > previous) {
                            record.maximum_len = Some(value);
                        }
                    }
                    None => {
                        record.maximum_len = None;
                        max_poisoned = true;
                    }
                }
            }
        }
        Self::retain(record, session)
    }
    /// Properties for the normalized sequence, after adjacent literals merge.
    pub(super) fn concat<A: Admission>(
        children: &[super::hir::Expr],
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut record = Record::base();
        record.literal = true;
        record.alternation_literal = true;
        for child in children {
            session.items(1)?;
            let p = child.properties.get();
            record.look_set |= p.look_set;
            record.utf8 &= p.utf8;
            record.explicit_captures_len = record
                .explicit_captures_len
                .saturating_add(p.explicit_captures_len);
            record.static_explicit_captures_len = p.static_explicit_captures_len.and_then(|n| {
                record
                    .static_explicit_captures_len
                    .map(|m| n.saturating_add(m))
            });
            record.literal &= p.literal;
            record.alternation_literal &= p.alternation_literal;
            if let Some(minimum) = record.minimum_len {
                record.minimum_len = p.minimum_len.map(|n| minimum.saturating_add(n));
            }
            if let Some(maximum) = record.maximum_len {
                record.maximum_len = p.maximum_len.and_then(|n| maximum.checked_add(n));
            }
        }
        for child in children {
            session.items(1)?;
            let p = child.properties.get();
            record.look_set_prefix |= p.look_set_prefix;
            record.look_set_prefix_any |= p.look_set_prefix_any;
            if p.maximum_len.is_none_or(|n| n > 0) {
                break;
            }
        }
        for child in children.iter().rev() {
            session.items(1)?;
            let p = child.properties.get();
            record.look_set_suffix |= p.look_set_suffix;
            record.look_set_suffix_any |= p.look_set_suffix_any;
            if p.maximum_len.is_none_or(|n| n > 0) {
                break;
            }
        }
        Self::retain(record, session)
    }
    /// Called after repetition normalization, matching the pinned constructor.
    pub(super) fn repetition<A: Admission>(
        child: &Self,
        min: u32,
        max: Option<u32>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let p = child.get();
        let minimum_len = p
            .minimum_len
            .map(|length| length.saturating_mul(usize::try_from(min).unwrap_or(usize::MAX)));
        let maximum_len = max.and_then(|max| {
            let count = usize::try_from(max).ok()?;
            p.maximum_len?.checked_mul(count)
        });
        let mut record = Record {
            minimum_len,
            maximum_len,
            look_set: p.look_set,
            look_set_prefix: 0,
            look_set_suffix: 0,
            look_set_prefix_any: p.look_set_prefix_any,
            look_set_suffix_any: p.look_set_suffix_any,
            utf8: p.utf8,
            explicit_captures_len: p.explicit_captures_len,
            static_explicit_captures_len: p.static_explicit_captures_len,
            literal: false,
            alternation_literal: false,
        };
        if min > 0 {
            record.look_set_prefix = p.look_set_prefix;
            record.look_set_suffix = p.look_set_suffix;
        }
        if min == 0 && record.static_explicit_captures_len.is_some_and(|n| n > 0) {
            record.static_explicit_captures_len = if max == Some(0) { Some(0) } else { None };
        }
        Self::retain(record, session)
    }
    pub(super) fn bytes<A: Admission>(
        class: &super::bytes::Class,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let mut record = Record::base();
        record.minimum_len = if class.is_empty() { None } else { Some(1) };
        record.maximum_len = record.minimum_len;
        record.utf8 = class.is_ascii();
        Self::retain(record, session)
    }
    fn retain<A: Admission>(record: Record, session: &mut Session<'_, A>) -> Step<Self> {
        let layout = Layout::new::<Record>();
        session.admit(Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        let mut values = Vec::new();
        if let Err(error) = values.try_reserve_exact(1) {
            return session.fail(Error::Allocation(error));
        }
        values.push(record);
        Ok(Self(values))
    }
    pub(super) fn get(&self) -> &Record {
        &self.0[0]
    }
    pub(super) fn empty<A: Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        Self::retain(Record::base(), session)
    }
    pub(super) fn literal<A: Admission>(bytes: &[u8], session: &mut Session<'_, A>) -> Step<Self> {
        session.items(bytes.len())?;
        let mut record = Record::base();
        record.minimum_len = Some(bytes.len());
        record.maximum_len = Some(bytes.len());
        record.utf8 = std::str::from_utf8(bytes).is_ok();
        record.literal = true;
        record.alternation_literal = true;
        Self::retain(record, session)
    }
    pub(super) fn look<A: Admission>(look: Look, session: &mut Session<'_, A>) -> Step<Self> {
        let bits = match look {
            Look::Start => 1 << 0,
            Look::End => 1 << 1,
        };
        let mut record = Record::base();
        record.look_set = bits;
        record.look_set_prefix = bits;
        record.look_set_suffix = bits;
        record.look_set_prefix_any = bits;
        record.look_set_suffix_any = bits;
        Self::retain(record, session)
    }
}
impl Record {
    fn base() -> Self {
        Self {
            minimum_len: Some(0),
            maximum_len: Some(0),
            look_set: 0,
            look_set_prefix: 0,
            look_set_suffix: 0,
            look_set_prefix_any: 0,
            look_set_suffix_any: 0,
            utf8: true,
            explicit_captures_len: 0,
            static_explicit_captures_len: Some(0),
            literal: false,
            alternation_literal: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};

    #[test]
    fn alternative_union_keeps_unknown_lengths_poisoned() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let short = Properties::literal(b"a", &mut session).unwrap();
        let long = Properties::literal(b"abc", &mut session).unwrap();
        let unbounded = Properties::repetition(&short, 1, None, &mut session).unwrap();
        let fail = Properties::bytes(&super::super::bytes::Class::empty(), &mut session).unwrap();
        let p = Properties::union(&[&short, &long], &mut session).unwrap();
        assert_eq!(
            (p.get().minimum_len, p.get().maximum_len),
            (Some(1), Some(3))
        );
        assert!(!p.get().literal && p.get().alternation_literal);
        let p = Properties::union(&[&short, &unbounded, &long], &mut session).unwrap();
        assert_eq!((p.get().minimum_len, p.get().maximum_len), (Some(1), None));
        assert!(!p.get().alternation_literal);
        let p = Properties::union(&[&short, &fail, &long], &mut session).unwrap();
        assert_eq!((p.get().minimum_len, p.get().maximum_len), (None, None));
        let p = Properties::union(&[], &mut session).unwrap();
        assert_eq!(p.get().static_explicit_captures_len, None);
        assert_eq!((p.get().look_set_prefix, p.get().look_set_suffix), (0, 0));
    }

    #[test]
    fn alternative_union_intersects_required_and_unions_possible_anchors() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let start = Properties::look(Look::Start, &mut session).unwrap();
        let end = Properties::look(Look::End, &mut session).unwrap();
        let p = Properties::union(&[&start, &end], &mut session).unwrap();
        assert_eq!(p.get().look_set, 3);
        assert_eq!((p.get().look_set_prefix, p.get().look_set_suffix), (0, 0));
        assert_eq!(
            (p.get().look_set_prefix_any, p.get().look_set_suffix_any),
            (3, 3)
        );
    }

    #[test]
    fn alternative_union_refusal_is_terminal() {
        let start = Properties::look(Look::Start, &mut Session::new(&mut Account)).unwrap();
        let end = Properties::look(Look::End, &mut Session::new(&mut Account)).unwrap();
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        Properties::union(&[&start, &end], &mut Session::new(&mut count)).unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                Properties::union(&[&start, &end], &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }

    #[test]
    fn concat_scans_anchors_only_through_zero_width_children() {
        use super::super::hir::{Expr, Literal};
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let children = [
            Expr::look(Look::Start, &mut session).unwrap(),
            Expr::literal(Literal::copy(b"ab", &mut session).unwrap(), &mut session).unwrap(),
            Expr::look(Look::End, &mut session).unwrap(),
        ];
        let p = Properties::concat(&children, &mut session).unwrap();
        assert_eq!(
            (p.get().minimum_len, p.get().maximum_len),
            (Some(2), Some(2))
        );
        assert_eq!(p.get().look_set, 3);
        assert_eq!(
            (p.get().look_set_prefix, p.get().look_set_prefix_any),
            (1, 1)
        );
        assert_eq!(
            (p.get().look_set_suffix, p.get().look_set_suffix_any),
            (2, 2)
        );
        assert!(!p.get().literal && !p.get().alternation_literal && p.get().utf8);
        let children = [
            Expr::look(Look::Start, &mut session).unwrap(),
            Expr::look(Look::End, &mut session).unwrap(),
        ];
        let p = Properties::concat(&children, &mut session).unwrap();
        assert_eq!(p.get().maximum_len, Some(0));
        assert_eq!((p.get().look_set_prefix, p.get().look_set_suffix), (3, 3));
    }

    #[test]
    fn concat_failure_stops_prefix_scan_and_propagates_impossible_lengths() {
        use super::super::hir::Expr;
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let children = [
            Expr::fail(&mut session).unwrap(),
            Expr::look(Look::End, &mut session).unwrap(),
        ];
        let p = Properties::concat(&children, &mut session).unwrap();
        assert_eq!((p.get().minimum_len, p.get().maximum_len), (None, None));
        assert_eq!(p.get().look_set_prefix, 0);
        assert_eq!(p.get().look_set_suffix, 2);
    }

    #[test]
    fn concat_refusal_precedes_each_child_scan_and_final_allocation() {
        use super::super::hir::Expr;
        let children = [
            Expr::look(Look::Start, &mut Session::new(&mut Account)).unwrap(),
            Expr::look(Look::End, &mut Session::new(&mut Account)).unwrap(),
        ];
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        Properties::concat(&children, &mut Session::new(&mut count)).unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                Properties::concat(&children, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }

    #[test]
    fn repetition_preserves_lengths_and_optional_anchor_semantics() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let literal = Properties::literal(b"ab", &mut session).unwrap();
        for (min, max, lengths) in [
            (0, None, (Some(0), None)),
            (1, None, (Some(2), None)),
            (0, Some(1), (Some(0), Some(2))),
        ] {
            let repeated = Properties::repetition(&literal, min, max, &mut session).unwrap();
            let p = repeated.get();
            assert_eq!((p.minimum_len, p.maximum_len), lengths);
            assert!(p.utf8 && !p.literal && !p.alternation_literal);
            assert_eq!(p.static_explicit_captures_len, Some(0));
        }
        let anchor = Properties::look(Look::Start, &mut session).unwrap();
        for min in [0, 1] {
            let p = Properties::repetition(&anchor, min, Some(1), &mut session).unwrap();
            assert_eq!(p.get().look_set, 1);
            assert_eq!(p.get().look_set_prefix_any, 1);
            assert_eq!(p.get().look_set_suffix_any, 1);
            assert_eq!(p.get().look_set_prefix, min);
            assert_eq!(p.get().look_set_suffix, min);
        }
        let fail = Properties::bytes(&super::super::bytes::Class::empty(), &mut session).unwrap();
        let p = Properties::repetition(&fail, 0, None, &mut session).unwrap();
        assert_eq!((p.get().minimum_len, p.get().maximum_len), (None, None));
    }

    #[test]
    fn repetition_length_overflow_saturates_minimum_and_unbounds_maximum() {
        let mut record = Record::base();
        record.minimum_len = Some(usize::MAX);
        record.maximum_len = Some(usize::MAX);
        let child = Properties::retain(record, &mut Session::new(&mut Account)).unwrap();
        let repeated =
            Properties::repetition(&child, 2, Some(2), &mut Session::new(&mut Account)).unwrap();
        assert_eq!(repeated.get().minimum_len, Some(usize::MAX));
        assert_eq!(repeated.get().maximum_len, None);
    }
    #[test]
    fn literal_properties_recompute_utf8_after_byte_concatenation() {
        for bytes in [&[0xc3][..], &[0xa9][..]] {
            assert!(
                !Properties::literal(bytes, &mut Session::new(&mut Account))
                    .unwrap()
                    .get()
                    .utf8
            );
        }
        let joined = Properties::literal(&[0xc3, 0xa9], &mut Session::new(&mut Account)).unwrap();
        let p = joined.get();
        assert!(p.utf8 && p.literal && p.alternation_literal);
        assert_eq!((p.minimum_len, p.maximum_len), (Some(2), Some(2)));
        assert_eq!(
            (p.explicit_captures_len, p.static_explicit_captures_len),
            (0, Some(0))
        );
        assert_eq!(p.look_set, 0);
    }
    #[test]
    fn empty_and_anchor_properties_preserve_zero_width_semantics() {
        let empty = Properties::empty(&mut Session::new(&mut Account)).unwrap();
        assert_eq!(empty.get(), &Record::base());
        for (look, bit) in [(Look::Start, 1), (Look::End, 2)] {
            let p = Properties::look(look, &mut Session::new(&mut Account)).unwrap();
            let p = p.get();
            assert_eq!(
                [
                    p.look_set,
                    p.look_set_prefix,
                    p.look_set_suffix,
                    p.look_set_prefix_any,
                    p.look_set_suffix_any
                ],
                [bit; 5]
            );
            assert!(p.utf8 && !p.literal && !p.alternation_literal);
            assert_eq!((p.minimum_len, p.maximum_len), (Some(0), Some(0)));
        }
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
    fn literal_scan_and_property_allocation_are_admitted() {
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        Properties::literal(b"abc", &mut Session::new(&mut count)).unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                Properties::literal(b"abc", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                Properties::empty(&mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
