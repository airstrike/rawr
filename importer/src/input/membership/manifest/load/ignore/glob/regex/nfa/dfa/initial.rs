// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! dense::DFA::initial and its transition/start/match/accelerator ownership.
use super::super::{finalize, info, storage::Array};
use super::{Admission, Error, Session, Step};

pub(super) struct Initial {
    pub(super) transitions: Array<u32>,
    pub(super) classes: [u8; 256],
    pub(super) stride2: usize,
    pub(super) starts: Starts,
    pub(super) matches: Matches,
    pub(super) special: [u32; 8],
    pub(super) accelerators: Array<u32>,
    pre: Option<info::Prefilter>,
    // dense::DFA's quit set; the shared Core is admitted at its full size.
    #[allow(dead_code)]
    quit: [u128; 2],
    pub(super) flags: Flags,
}

pub(super) struct Starts {
    pub(super) table: Array<u32>,
    pub(super) anchored_only: bool,
    pub(super) byte_map: [Start; 256],
    pub(super) stride: usize,
    pub(super) pattern_len: Option<usize>,
    pub(super) universal_unanchored: Option<u32>,
    pub(super) universal_anchored: Option<u32>,
}

pub(super) struct Matches {
    pub(super) slices: Array<u32>,
    pub(super) patterns: Array<u32>,
    pub(super) pattern_len: usize,
}

pub(super) struct Flags {
    pub(super) has_empty: bool,
    pub(super) utf8: bool,
    pub(super) anchored: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Start {
    NonWordByte,
    WordByte,
    Text,
    LineLF,
    LineCR,
    CustomLineTerminator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    StartStates,
    States,
    DfaSize(usize),
    DeterminizeSize(usize),
    MatchPatternIds,
}

impl Initial {
    /// This private producer has no prefilter and disables UTF-8 empty checks.
    pub(super) fn has_closed_query_config(&self) -> bool {
        self.pre.is_none() && !self.flags.utf8
    }

    pub(super) fn new<A: Admission>(
        nfa: &finalize::Nfa,
        anchored_only: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        session.items(1)?;
        // Core's emitted grammar has only Start/End looks and no prefilter.
        // unicode_word_boundary(true) consequently adds no quit-byte classes.
        if nfa.0.looks & !3 != 0 {
            return session.fail(Error::State);
        }
        let classes = nfa.0.byte_classes(session)?;
        let alphabet = usize::from(classes[255]) + 2;
        let stride = alphabet.next_power_of_two();
        let stride2 = stride.trailing_zeros() as usize;

        // TransitionTable::minimal adds DEAD, then QUIT, as separate reached
        // extend calls. An alphabet of two can reuse the first four-slot extent.
        let mut transitions = Array::new();
        transitions.resize(stride, 0, session)?;
        transitions.resize(2 * stride, 0, session)?;

        let starts = match Starts::dead(
            nfa.0.starts.len(),
            anchored_only,
            nfa.0.line_terminator,
            session,
        )? {
            Ok(starts) => starts,
            Err(error) => return Ok(Err(error)),
        };
        let matches = Matches {
            slices: Array::new(),
            patterns: Array::new(),
            pattern_len: nfa.0.starts.len(),
        };
        // Accels::empty is vec![0], an exact one-u32 allocation, after all
        // transition and start ownership. It is present even for no patterns.
        let accelerators = exact(1, session)?;
        Ok(Ok(Self {
            transitions,
            classes,
            stride2,
            starts,
            matches,
            special: [0; 8],
            accelerators,
            pre: None,
            quit: [0; 2],
            flags: Flags {
                has_empty: nfa.0.has_empty,
                utf8: nfa.0.utf8,
                anchored: nfa.0.anchored == nfa.0.unanchored,
            },
        }))
    }

    pub(super) fn stride(&self) -> usize {
        1 << self.stride2
    }

    /// This is the pinned semantic memory_usage projection, not the physical
    /// cumulative account. Initial tables have at most two rows/256 classes;
    /// a successful start allocation proves the other lengths fit.
    pub(super) fn memory_usage(&self) -> usize {
        (self.transitions.values.len()
            + self.starts.table.values.len()
            + self.matches.slices.values.len()
            + self.matches.patterns.values.len()
            + self.accelerators.values.len())
            * 4
    }
}

impl Starts {
    fn dead<A: Admission>(
        patterns: usize,
        anchored_only: bool,
        line_terminator: u8,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        session.items(1)?;
        if patterns > i32::MAX as usize {
            return session.fail(Error::State);
        }
        let Some(length) = patterns
            .checked_mul(6)
            .and_then(|length| length.checked_add(12))
        else {
            return Ok(Err(Failure::StartStates));
        };
        if length > isize::MAX as usize {
            return Ok(Err(Failure::StartStates));
        }
        // StartTable always retains both groups, even for an anchored-only DFA.
        let table = exact(length, session)?;
        let byte_map = Start::byte_map(line_terminator, session)?;
        Ok(Ok(Self {
            table,
            anchored_only,
            byte_map,
            stride: 6,
            pattern_len: Some(patterns),
            universal_unanchored: None,
            universal_anchored: None,
        }))
    }
}

impl Start {
    pub(in super::super) fn byte_map<A: Admission>(
        line_terminator: u8,
        session: &mut Session<'_, A>,
    ) -> Step<[Self; 256]> {
        session.items(256)?;
        let mut map = [Self::NonWordByte; 256];
        session.items(3)?;
        map[usize::from(b'\n')] = Self::LineLF;
        map[usize::from(b'\r')] = Self::LineCR;
        map[usize::from(b'_')] = Self::WordByte;
        for (start, end) in [(b'0', b'9'), (b'A', b'Z'), (b'a', b'z')] {
            for byte in start..=end {
                session.items(1)?;
                map[usize::from(byte)] = Self::WordByte;
            }
        }
        session.items(1)?;
        if !matches!(line_terminator, b'\r' | b'\n') {
            map[usize::from(line_terminator)] = Self::CustomLineTerminator;
        }
        Ok(map)
    }
}

fn exact<A: Admission>(length: usize, session: &mut Session<'_, A>) -> Step<Array<u32>> {
    Ok(Array {
        values: finalize::storage::repeat(0, length, session)?,
        admitted: length,
    })
}
