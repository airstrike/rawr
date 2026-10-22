// Copyright (c) 2014 The Rust Project Developers. MIT OR Apache-2.0.
//! The complete borrowed dense representation and safe runtime validation from
//! regex-automata 0.4.14, restricted at publication to bstr's fixed trim table.

use std::sync::Arc;

use super::{
    document,
    wire::{self, Cursor, Words},
    Error, Session, Step,
};

pub(super) struct Dfa<'a> {
    transitions: Transitions<'a>,
    starts: Starts<'a>,
    matches: Matches<'a>,
    special: Special,
    accelerators: Words<'a>,
    prefilter: Option<Prefilter>,
    quit: [u128; 2],
    flags: Flags,
}

struct Transitions<'a> {
    table: Words<'a>,
    classes: [u8; 256],
    stride2: usize,
}

struct Starts<'a> {
    table: Words<'a>,
    kind: Kind,
    map: [Start; 256],
    stride: usize,
    pattern_len: Option<usize>,
    universal_unanchored: Option<u32>,
    universal_anchored: Option<u32>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Kind {
    Both,
    Unanchored,
    Anchored,
}

#[derive(Clone, Copy)]
enum Start {
    NonWordByte,
    WordByte,
    Text,
    LineLF,
    LineCR,
    CustomLineTerminator,
}

struct Matches<'a> {
    slices: Words<'a>,
    patterns: Words<'a>,
    pattern_len: usize,
}

struct Special {
    max: u32,
    quit: u32,
    min_match: u32,
    max_match: u32,
    min_accel: u32,
    max_accel: u32,
    min_start: u32,
    max_start: u32,
}

struct Flags {
    has_empty: bool,
    is_utf8: bool,
    is_always_start_anchored: bool,
}

// The enabled alloc Prefilter has a fat Arc, bool AND usize, even when None.
// Keep those real types; an absent value is not permission to shrink its slot.
// No implementation can construct this private, always-absent searcher.
trait Searcher: Send + Sync {}
struct Prefilter {
    _searcher: Arc<dyn Searcher>,
    _is_fast: bool,
    _max_needle_len: usize,
}

impl<'a> Dfa<'a> {
    pub(super) fn decode<A: document::Admission>(
        bytes: &'a [u8],
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let mut cursor = Cursor::header(bytes, session)?;
        let bits = cursor.u32(session)?;
        session.items(3)?;
        let flags = Flags {
            has_empty: bits & 1 != 0,
            is_utf8: bits & 2 != 0,
            is_always_start_anchored: bits & 4 != 0,
        };
        let transitions = Transitions::decode(&mut cursor, session)?;
        let starts = Starts::decode(&mut cursor, session)?;
        let matches = Matches::decode(&mut cursor, session)?;
        let special = Special::decode(&mut cursor, session)?;
        wire::require(
            (special.max as usize >> transitions.stride2) < transitions.len(),
            session,
        )?;

        // Accels retains its count word even when the count is zero.
        let count = cursor.peek_u32(session)? as usize;
        let Some(words) = count.checked_mul(2).and_then(|n| n.checked_add(1)) else {
            return session.fail(Error::Overflow);
        };
        let accelerators = cursor.words(words, session)?;

        let buckets = cursor.take(32, session)?;
        session.items(2)?;
        let Ok(low) = <[u8; 16]>::try_from(&buckets[..16]) else {
            return session.fail(Error::State);
        };
        // Pinned ByteSet::from_bytes reads BOTH buckets from the first slice.
        // They are zero in this fixed table; preserve the source operation.
        let quit = [u128::from_le_bytes(low), u128::from_le_bytes(low)];
        session.items(1)?;
        let result = Self {
            transitions,
            starts,
            matches,
            special,
            accelerators,
            prefilter: None,
            quit,
            flags,
        };
        result.validate(session)?;

        // This is a closed fixed helper, not a general regex engine. Perform
        // complete source validation first, then establish the search closure.
        wire::require(cursor.position() == bytes.len(), session)?;
        wire::require(
            result.transitions.len() == 19 && result.transitions.stride2 == 5,
            session,
        )?;
        wire::require(
            result.transitions.alphabet() == 27 && result.matches.pattern_len == 1,
            session,
        )?;
        wire::require(
            !result.flags.has_empty
                && result.flags.is_utf8
                && !result.flags.is_always_start_anchored,
            session,
        )?;
        wire::require(
            result.accelerators.get(0, session)? == 0
                && result.special.min_accel == 0
                && result.special.max_accel == 0,
            session,
        )?;
        wire::require(result.quit == [0, 0] && result.prefilter.is_none(), session)?;
        wire::require(result.starts.kind != Kind::Unanchored, session)?;
        Ok(result)
    }

    fn validate<A: document::Admission>(&self, session: &mut Session<'_, A>) -> Step<()> {
        // Same ordering as dense::DFA::from_bytes: accelerators, match table,
        // transitions, starts, then accelerator-state correspondence.
        for chunk in self.accelerators.bytes()[4..].chunks(8) {
            session.items(1)?;
            wire::require(chunk.len() >= 4 && chunk[0] < 4, session)?;
        }
        let stride = self.transitions.stride();
        let match_len = if self.special.min_match == 0 {
            0
        } else {
            ((self.special.max_match - self.special.min_match) as usize / stride) + 1
        };
        wire::require(self.matches.len() == match_len, session)?;
        for index in 0..self.matches.len() {
            session.items(1)?;
            let start = self.matches.slices.get(index * 2, session)? as usize;
            let length = self.matches.slices.get(index * 2 + 1, session)? as usize;
            wire::require(start < self.matches.patterns.len(), session)?;
            let Some(end) = start.checked_add(length) else {
                return session.fail(Error::Overflow);
            };
            wire::require(end <= self.matches.patterns.len(), session)?;
            for at in start..end {
                let pattern = self.matches.patterns.get(at, session)?;
                wire::require((pattern as usize) < self.matches.pattern_len, session)?;
            }
        }
        for index in 0..self.transitions.len() {
            session.items(1)?;
            let state = (index * stride) as u32;
            if state <= self.special.max {
                wire::require(
                    state == 0
                        || state == self.special.quit
                        || self.special.is_match(state)
                        || self.special.is_start(state)
                        || self.special.is_accel(state),
                    session,
                )?;
                if self.special.is_match(state) {
                    let at = (state - self.special.min_match) as usize / stride;
                    wire::require(self.matches.slices.get(at * 2 + 1, session)? != 0, session)?;
                }
            }
            // Source validates only the alphabet, not padding to the stride.
            for class in 0..self.transitions.alphabet() {
                let next = self
                    .transitions
                    .table
                    .get(state as usize + class, session)?;
                wire::require(self.transitions.valid(next), session)?;
            }
        }
        for state in [
            self.starts.universal_unanchored,
            self.starts.universal_anchored,
        ] {
            session.items(1)?;
            if let Some(state) = state {
                wire::require(self.transitions.valid(state), session)?;
            }
        }
        for index in 0..self.starts.table.len() {
            wire::require(
                self.transitions
                    .valid(self.starts.table.get(index, session)?),
                session,
            )?;
        }
        for index in 0..self.transitions.len() {
            session.items(1)?;
            let state = (index * stride) as u32;
            if self.special.is_accel(state) {
                let index = (state - self.special.min_accel) as usize / stride;
                wire::require(index < self.accelerators.get(0, session)? as usize, session)?;
                session.items(1)?;
                let Some(&length) = self.accelerators.bytes().get(4 + index * 8) else {
                    return session.fail(Error::State);
                };
                wire::require((1..=3).contains(&length), session)?;
            }
        }
        Ok(())
    }

    pub(super) fn end<A: document::Admission>(
        &self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<usize> {
        session.items(1)?;
        // Input::new(...).anchored(Yes): no lookbehind at the whole-input end.
        // Start::Text=2; source does not take the universal-start shortcut.
        let mut state = self.starts.table.get(self.starts.stride + 2, session)?;
        let mut matched = None;
        for (at, &byte) in bytes.iter().enumerate().rev() {
            session.items(1)?;
            let class = self.transitions.classes[usize::from(byte)];
            state = self
                .transitions
                .table
                .get(state as usize + usize::from(class), session)?;
            if state <= self.special.max {
                if self.special.is_start(state) {
                    // There are no accelerators in the validated fixed table.
                } else if self.special.is_match(state) {
                    self.pattern(state, session)?;
                    session.items(1)?;
                    matched = Some(at + 1); // delayed reverse match
                } else if state == 0 {
                    return Ok(matched.unwrap_or(bytes.len()));
                } else {
                    // No quit bytes or accelerators are in this fixed helper.
                    return session.fail(Error::State);
                }
            }
        }
        // Empty input also initializes and takes this EOI transition. Dead
        // termination above does not; earliest=false keeps the latest match.
        session.items(1)?;
        state = self
            .transitions
            .table
            .get(state as usize + self.transitions.alphabet() - 1, session)?;
        if self.special.is_match(state) {
            self.pattern(state, session)?;
            session.items(1)?;
            matched = Some(0);
        }
        Ok(matched.unwrap_or(bytes.len()))
    }

    fn pattern<A: document::Admission>(
        &self,
        state: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let index = (state - self.special.min_match) as usize / self.transitions.stride();
        let start = self.matches.slices.get(index * 2, session)?;
        wire::require(
            self.matches.patterns.get(start as usize, session)? == 0,
            session,
        )
    }
}

impl<'a> Transitions<'a> {
    fn decode<A: document::Admission>(
        cursor: &mut Cursor<'a>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let states = cursor.usize(session)?;
        let stride2 = cursor.usize(session)?;
        let source = cursor.take(256, session)?;
        session.items(256)?;
        let mut classes = [0; 256];
        classes.copy_from_slice(source);
        let alphabet = usize::from(classes[255]) + 2;
        for &class in &classes {
            wire::require(usize::from(class) < alphabet, session)?;
        }
        wire::require((1..=9).contains(&stride2), session)?;
        wire::require(alphabet <= 1 << stride2, session)?;
        let Some(count) = states.checked_mul(1 << stride2) else {
            return session.fail(Error::Overflow);
        };
        let table = cursor.words(count, session)?;
        Ok(Self {
            table,
            classes,
            stride2,
        })
    }
    fn stride(&self) -> usize {
        1 << self.stride2
    }
    fn len(&self) -> usize {
        self.table.len() / self.stride()
    }
    fn alphabet(&self) -> usize {
        usize::from(self.classes[255]) + 2
    }
    fn valid(&self, state: u32) -> bool {
        (state as usize) < self.table.len() && (state as usize).is_multiple_of(self.stride())
    }
}

impl<'a> Starts<'a> {
    fn decode<A: document::Admission>(
        cursor: &mut Cursor<'a>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let kind = match cursor.u32(session)? {
            0 => Kind::Both,
            1 => Kind::Unanchored,
            2 => Kind::Anchored,
            _ => return session.fail(Error::State),
        };
        let source = cursor.take(256, session)?;
        session.items(256)?;
        let mut map = [Start::NonWordByte; 256];
        for (slot, &byte) in map.iter_mut().zip(source) {
            session.items(1)?;
            *slot = match byte {
                0 => Start::NonWordByte,
                1 => Start::WordByte,
                2 => Start::Text,
                3 => Start::LineLF,
                4 => Start::LineCR,
                5 => Start::CustomLineTerminator,
                _ => return session.fail(Error::State),
            };
        }
        let stride = cursor.usize(session)?;
        wire::require(stride == 6, session)?;
        let count = cursor.usize(session)?;
        let pattern_len = if count == u32::MAX as usize {
            None
        } else {
            Some(count)
        };
        wire::require(
            pattern_len.is_none_or(|count| count <= i32::MAX as usize),
            session,
        )?;
        let universal_unanchored = cursor.optional_id(session)?;
        let universal_anchored = cursor.optional_id(session)?;
        let Some(count) = pattern_len
            .unwrap_or(0)
            .checked_add(2)
            .and_then(|n| n.checked_mul(stride))
        else {
            return session.fail(Error::Overflow);
        };
        let table = cursor.words(count, session)?;
        Ok(Self {
            table,
            kind,
            map,
            stride,
            pattern_len,
            universal_unanchored,
            universal_anchored,
        })
    }
}

impl<'a> Matches<'a> {
    fn decode<A: document::Admission>(
        cursor: &mut Cursor<'a>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let count = cursor.usize(session)?;
        let Some(pairs) = count.checked_mul(2) else {
            return session.fail(Error::Overflow);
        };
        let slices = cursor.words(pairs, session)?;
        let pattern_len = cursor.usize(session)?;
        let count = cursor.usize(session)?;
        let patterns = cursor.words(count, session)?;
        Ok(Self {
            slices,
            patterns,
            pattern_len,
        })
    }
    fn len(&self) -> usize {
        self.slices.len() / 2
    }
}

impl Special {
    fn decode<A: document::Admission>(
        cursor: &mut Cursor<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let mut id = || {
            let value = cursor.u32(session)?;
            wire::require(value < i32::MAX as u32, session)?;
            Ok(value)
        };
        let result = Self {
            max: id()?,
            quit: id()?,
            min_match: id()?,
            max_match: id()?,
            min_accel: id()?,
            max_accel: id()?,
            min_start: id()?,
            max_start: id()?,
        };
        for (min, max) in [
            (result.min_match, result.max_match),
            (result.min_accel, result.max_accel),
            (result.min_start, result.max_start),
        ] {
            wire::require((min == 0) == (max == 0) && min <= max, session)?;
            wire::require(min == 0 || result.quit < min, session)?;
            wire::require(max <= result.max, session)?;
        }
        wire::require(result.quit <= result.max, session)?;
        wire::require(
            result.min_match == 0 || result.min_accel == 0 || result.min_accel >= result.min_match,
            session,
        )?;
        wire::require(
            result.min_match == 0 || result.min_start == 0 || result.min_start >= result.min_match,
            session,
        )?;
        wire::require(
            result.min_accel == 0 || result.min_start == 0 || result.min_start >= result.min_accel,
            session,
        )?;
        Ok(result)
    }
    fn is_match(&self, state: u32) -> bool {
        self.min_match != 0 && (self.min_match..=self.max_match).contains(&state)
    }
    fn is_start(&self, state: u32) -> bool {
        self.min_start != 0 && (self.min_start..=self.max_start).contains(&state)
    }
    fn is_accel(&self, state: u32) -> bool {
        self.min_accel != 0 && (self.min_accel..=self.max_accel).contains(&state)
    }
}
