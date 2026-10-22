// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Unicode range collection and canonicalization reached by alternation.
use crate::input::membership::manifest::{
    session::Session,
    storage::{self, Buffer},
    Step,
};
use rawr_toml::document::Admission;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Range {
    lower: char,
    upper: char,
}
impl Range {
    pub(super) fn new(a: char, b: char) -> Self {
        Self {
            lower: a.min(b),
            upper: a.max(b),
        }
    }
    fn union(self, other: Self) -> Option<Self> {
        let next = |c: char| {
            if c == '\u{d7ff}' {
                0xe000
            } else {
                u32::from(c) + 1
            }
        };
        if next(self.upper) < u32::from(other.lower) || next(other.upper) < u32::from(self.lower) {
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
    pub(in super::super) fn ranges(&self) -> impl ExactSizeIterator<Item = (char, char)> + '_ {
        self.ranges.as_slice().iter().map(|r| (r.lower, r.upper))
    }
    pub(in super::super) fn is_ascii(&self) -> bool {
        self.ranges
            .as_slice()
            .last()
            .is_none_or(|r| r.upper <= '\x7f')
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
    pub(super) fn to_bytes<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<super::bytes::Class>> {
        session.check()?;
        if self
            .ranges
            .as_slice()
            .last()
            .is_some_and(|range| range.upper > '\x7f')
        {
            return Ok(None);
        }
        let mut ranges = Buffer::new();
        ranges.reserve_more(self.ranges.len(), session)?;
        for range in self.ranges.as_slice() {
            // The canonical maximum established that every endpoint is ASCII.
            ranges.push(
                super::bytes::Range::new(range.lower as u8, range.upper as u8),
                session,
            )?;
        }
        super::bytes::Class::from_ranges(ranges, session).map(Some)
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
    pub(super) fn is_empty(&self) -> bool {
        self.ranges.len() == 0
    }
    pub(super) fn minimum_len(&self) -> Option<usize> {
        self.ranges.as_slice().first().map(|r| r.lower.len_utf8())
    }
    pub(super) fn maximum_len(&self) -> Option<usize> {
        self.ranges.as_slice().last().map(|r| r.upper.len_utf8())
    }
    pub(super) fn singleton(&self) -> Option<char> {
        match self.ranges.as_slice() {
            [range] if range.lower == range.upper => Some(range.lower),
            _ => None,
        }
    }
    pub(in super::super) fn singletons<A: Admission>(
        chars: Buffer<char>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let mut ranges = Buffer::new();
        ranges.reserve_more(chars.len(), session)?;
        for c in chars.into_iter() {
            ranges.push(Range::new(c, c), session)?;
        }
        Self::from_ranges(ranges, session)
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
        // Equal Range values have no payload identity, so stable ordering does
        // not affect this projection. Use the existing allocation-free sort.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn chars(values: &[char]) -> Buffer<char> {
        let mut result = Buffer::new();
        for &c in values {
            result.push(c, &mut Session::new(&mut Account)).unwrap();
        }
        result
    }
    #[test]
    fn union_preserves_unicode_ranges_and_equal_shortcut() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut class = Class::singletons(chars(&['a', 'c']), &mut session).unwrap();
        let other = Class::singletons(chars(&['b', 'é']), &mut session).unwrap();
        class.union(&other, &mut session).unwrap();
        assert_eq!(
            class.ranges.as_slice(),
            &[Range::new('a', 'c'), Range::new('é', 'é')]
        );
        let duplicate = Class::singletons(chars(&['a', 'b', 'c', 'é']), &mut session).unwrap();
        class.folded = true;
        class.union(&duplicate, &mut session).unwrap();
        assert!(class.folded);
    }
    #[test]
    fn canonicalization_merges_duplicates_adjacency_and_surrogate_gap() {
        let class = Class::singletons(
            chars(&['c', 'a', 'b', 'a', '\u{e000}', '\u{d7ff}', '\u{10ffff}']),
            &mut Session::new(&mut Account),
        )
        .unwrap();
        assert_eq!(
            class.ranges.as_slice(),
            &[
                Range::new('a', 'c'),
                Range::new('\u{d7ff}', '\u{e000}'),
                Range::new('\u{10ffff}', '\u{10ffff}')
            ]
        );
        assert!(!class.folded);
        let empty = Class::singletons(chars(&[]), &mut Session::new(&mut Account)).unwrap();
        assert!(empty.folded && empty.ranges.len() == 0);
    }
    #[test]
    fn canonical_input_does_not_append_merge_storage() {
        let class =
            Class::singletons(chars(&['a', 'c', 'e']), &mut Session::new(&mut Account)).unwrap();
        assert_eq!(
            class.ranges.as_slice(),
            &[
                Range::new('a', 'a'),
                Range::new('c', 'c'),
                Range::new('e', 'e')
            ]
        );
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
    fn refusal_covers_collection_sorting_and_merge_growth() {
        let values = ['z', 'e', 'a', 'b', 'a'];
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        Class::singletons(chars(&values), &mut Session::new(&mut count)).unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                Class::singletons(chars(&values), &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
