// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Reached byte IntervalSet insertion and complement, preserving retained
//! append-before-drain ownership during negation (regex-syntax 0.8.11).
use crate::input::membership::manifest::{
    session::Session,
    storage::{self, Buffer},
    Step,
};
use rawr_toml::document::Admission;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Range {
    lower: u8,
    upper: u8,
}
impl Range {
    pub(super) fn new(a: u8, b: u8) -> Self {
        Self {
            lower: a.min(b),
            upper: a.max(b),
        }
    }
    fn union(self, other: Self) -> Option<Self> {
        if u16::from(self.upper) + 1 < u16::from(other.lower)
            || u16::from(other.upper) + 1 < u16::from(self.lower)
        {
            return None;
        }
        Some(Self::new(
            self.lower.min(other.lower),
            self.upper.max(other.upper),
        ))
    }
}
pub(in super::super) struct Class {
    ranges: Buffer<Range>,
    folded: bool,
}
impl Class {
    pub(in super::super) fn ranges(&self) -> impl ExactSizeIterator<Item = (u8, u8)> + '_ {
        self.ranges
            .as_slice()
            .iter()
            .map(|range| (range.lower, range.upper))
    }
    pub(super) fn equal<A: Admission>(
        &self,
        other: &Self,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.check()?;
        if self.ranges.len() != other.ranges.len() {
            return Ok(false);
        }
        session.items(self.ranges.len())?;
        Ok(self.ranges.as_slice() == other.ranges.as_slice())
    }
    pub(super) fn from_ranges<A: Admission>(
        ranges: Buffer<Range>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let folded = ranges.len() == 0;
        let mut class = Self { ranges, folded };
        class.canonicalize(session)?;
        Ok(class)
    }
    pub(super) fn to_unicode<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<super::unicode::Class>> {
        session.check()?;
        if !self.is_ascii() {
            return Ok(None);
        }
        let mut ranges = Buffer::new();
        ranges.reserve_more(self.ranges.len(), session)?;
        for range in self.ranges.as_slice() {
            ranges.push(
                super::unicode::Range::new(char::from(range.lower), char::from(range.upper)),
                session,
            )?;
        }
        super::unicode::Class::from_ranges(ranges, session).map(Some)
    }
    pub(in super::super) fn singletons<A: Admission>(
        bytes: Buffer<u8>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let mut ranges = Buffer::new();
        ranges.reserve_more(bytes.len(), session)?;
        for byte in bytes.into_iter() {
            ranges.push(Range::new(byte, byte), session)?;
        }
        Self::from_ranges(ranges, session)
    }
    pub(super) fn union<A: Admission>(
        &mut self,
        other: &Self,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        if other.is_empty() {
            return Ok(());
        }
        if self.ranges.len() == other.ranges.len() {
            session.items(self.ranges.len())?;
            if self.ranges.as_slice() == other.ranges.as_slice() {
                return Ok(());
            }
        }
        self.ranges.reserve_more(other.ranges.len(), session)?;
        for &range in other.ranges.as_slice() {
            self.ranges.push(range, session)?;
        }
        self.canonicalize(session)?;
        self.folded &= other.folded;
        Ok(())
    }
    fn canonicalize<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.check()?;
        let mut canonical = true;
        for pair in self.ranges.as_slice().windows(2) {
            session.items(1)?;
            if pair[0] >= pair[1] || pair[0].union(pair[1]).is_some() {
                canonical = false;
                break;
            }
        }
        if canonical {
            return Ok(());
        }
        storage::try_sort(self.ranges.as_mut_slice(), |a, b| {
            session.items(1)?;
            Ok(a.cmp(b))
        })?;
        let original = self.ranges.len();
        for index in 0..original {
            session.items(1)?;
            let range = self.ranges.as_slice()[index];
            if self.ranges.len() > original {
                let last = self.ranges.len() - 1;
                if let Some(union) = self.ranges.as_slice()[last].union(range) {
                    self.ranges.as_mut_slice()[last] = union;
                    continue;
                }
            }
            self.ranges.push(range, session)?;
        }
        let remaining = self.ranges.len() - original;
        session.items(remaining)?;
        self.ranges.as_mut_slice().copy_within(original.., 0);
        self.ranges.truncate(remaining);
        Ok(())
    }
    pub(super) fn is_empty(&self) -> bool {
        self.ranges.len() == 0
    }
    pub(super) fn is_ascii(&self) -> bool {
        self.ranges
            .as_slice()
            .last()
            .is_none_or(|range| range.upper <= 0x7f)
    }
    pub(super) fn singleton(&self) -> Option<u8> {
        match self.ranges.as_slice() {
            [range] if range.lower == range.upper => Some(range.lower),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(super) fn contains(&self, byte: u8) -> bool {
        self.ranges
            .as_slice()
            .iter()
            .any(|range| range.lower <= byte && byte <= range.upper)
    }
    pub(in super::super) fn empty() -> Self {
        Self {
            ranges: Buffer::new(),
            folded: true,
        }
    }
    pub(super) fn push<A: Admission>(
        &mut self,
        mut range: Range,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        // Precharge a conservative bound for std's allocation-free binary search.
        session.items(self.ranges.len())?;
        let Err(index) = self.ranges.as_slice().binary_search(&range) else {
            return Ok(());
        };
        let mut start = index;
        if let Some(before) = index.checked_sub(1) {
            session.items(1)?;
            if let Some(union) = self.ranges.as_slice()[before].union(range) {
                range = union;
                start = before;
            }
        }
        let mut end = index;
        for after in index..self.ranges.len() {
            session.items(1)?;
            let Some(union) = range.union(self.ranges.as_slice()[after]) else {
                break;
            };
            range = union;
            end = after + 1;
        }
        let length = self.ranges.len();
        session.items(length - end)?;
        if start == end {
            // One-item splice grows only when no existing interval is removed.
            self.ranges.push(range, session)?;
            self.ranges
                .as_mut_slice()
                .copy_within(end..length, start + 1);
        } else {
            self.ranges
                .as_mut_slice()
                .copy_within(end..length, start + 1);
            self.ranges.truncate(length - (end - start) + 1);
        }
        self.ranges.as_mut_slice()[start] = range;
        self.folded = false;
        Ok(())
    }
    pub(super) fn negate<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.check()?;
        let length = self.ranges.len();
        if length == 0 {
            self.ranges.push(Range::new(0, 255), session)?;
            self.folded = true;
            return Ok(());
        }
        let first = self.ranges.as_slice()[0];
        if first.lower > 0 {
            self.ranges.push(Range::new(0, first.lower - 1), session)?;
        }
        for index in 1..length {
            session.items(1)?;
            let before = self.ranges.as_slice()[index - 1];
            let after = self.ranges.as_slice()[index];
            self.ranges
                .push(Range::new(before.upper + 1, after.lower - 1), session)?;
        }
        let last = self.ranges.as_slice()[length - 1];
        if last.upper < 255 {
            self.ranges.push(Range::new(last.upper + 1, 255), session)?;
        }
        let remaining = self.ranges.len() - length;
        session.items(remaining)?;
        self.ranges.as_mut_slice().copy_within(length.., 0);
        self.ranges.truncate(remaining);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};

    #[test]
    fn collected_singletons_and_union_preserve_canonical_membership() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut values = Buffer::new();
        for b in [5, 3, 4, 3] {
            values.push(b, &mut session).unwrap();
        }
        let mut class = Class::singletons(values, &mut session).unwrap();
        assert_eq!(class.ranges.as_slice(), &[Range::new(3, 5)]);
        let mut other = Class::empty();
        other.push(Range::new(0, 3), &mut session).unwrap();
        class.union(&other, &mut session).unwrap();
        assert_eq!(class.ranges.as_slice(), &[Range::new(0, 5)]);
        for b in 0..=255 {
            assert_eq!(class.contains(b), b <= 5);
        }
        let mut duplicate = Class::empty();
        duplicate.push(Range::new(0, 5), &mut session).unwrap();
        class.folded = true;
        class.union(&duplicate, &mut session).unwrap();
        assert!(class.folded); // equal-range shortcut does not recompute folding
    }
    #[test]
    fn ascii_class_conversion_round_trips_and_rejects_non_ascii_bytes() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut class = Class::empty();
        class.push(Range::new(0, 3), &mut session).unwrap();
        class.push(Range::new(120, 127), &mut session).unwrap();
        let unicode = class.to_unicode(&mut session).unwrap().unwrap();
        let roundtrip = unicode.to_bytes(&mut session).unwrap().unwrap();
        assert_eq!(class.ranges.as_slice(), roundtrip.ranges.as_slice());
        class.push(Range::new(128, 128), &mut session).unwrap();
        assert!(class.to_unicode(&mut session).unwrap().is_none());
        let mut values = Buffer::new();
        values.push('é', &mut session).unwrap();
        let unicode = super::super::unicode::Class::singletons(values, &mut session).unwrap();
        assert!(unicode.to_bytes(&mut session).unwrap().is_none());
    }
    fn assert_members(class: &Class, expected: &[bool; 256]) {
        for byte in 0..=255u8 {
            assert_eq!(
                class
                    .ranges
                    .as_slice()
                    .iter()
                    .any(|r| r.lower <= byte && byte <= r.upper),
                expected[usize::from(byte)]
            );
        }
        for pair in class.ranges.as_slice().windows(2) {
            assert!(u16::from(pair[0].upper) + 1 < u16::from(pair[1].lower));
        }
    }
    #[test]
    fn insertion_and_complement_match_byte_membership() {
        let mut class = Class::empty();
        let mut expected = [false; 256];
        let mut account = Account;
        let mut session = Session::new(&mut account);
        for (a, b) in [
            (255, 255),
            (0, 0),
            (100, 120),
            (30, 20),
            (31, 99),
            (22, 24),
            (1, 19),
            (254, 121),
            (0, 255),
        ] {
            class.push(Range::new(a, b), &mut session).unwrap();
            for i in a.min(b)..=a.max(b) {
                expected[usize::from(i)] = true;
            }
            assert_members(&class, &expected);
            class.negate(&mut session).unwrap();
            assert_members(&class, &expected.map(|x| !x));
            class.negate(&mut session).unwrap();
            assert_members(&class, &expected);
        }
    }
    #[test]
    fn empty_complement_and_exact_duplicate_preserve_folded_state() {
        let mut class = Class::empty();
        class.negate(&mut Session::new(&mut Account)).unwrap();
        assert!(class.folded);
        class
            .push(Range::new(0, 255), &mut Session::new(&mut Account))
            .unwrap();
        assert!(class.folded);
        class.negate(&mut Session::new(&mut Account)).unwrap();
        assert!(class.ranges.as_slice().is_empty());
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
        let mut class = Class::empty();
        for value in [2, 6, 10, 14, 18, 22] {
            class.push(Range::new(value, value), session)?;
        }
        class.negate(session)?;
        class.push(Range::new(3, 21), session)?;
        let mut other = Class::empty();
        other.push(Range::new(0, 8), session)?;
        class.union(&other, session)?;
        // Exercise both allocation-bearing conversions under every refusal
        // boundary, including collection and canonicalization in the receiver.
        let unicode = other.to_unicode(session)?.expect("ASCII class");
        let roundtrip = unicode.to_bytes(session)?.expect("ASCII class");
        assert_eq!(other.ranges.as_slice(), roundtrip.ranges.as_slice());
        Ok(())
    }
    #[test]
    fn refusal_stops_insertion_and_append_before_drain() {
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
