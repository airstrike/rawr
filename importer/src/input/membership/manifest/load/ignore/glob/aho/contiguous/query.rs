// Copyright the aho-corasick contributors. MIT OR Unlicense.
//! Exact compact-state decoding for Standard unanchored search.
use rawr_toml::document::Admission;

use super::{words, Nfa, DENSE, FAIL, ONE};
use crate::input::membership::manifest::{session::Session, Step};

impl Nfa {
    pub(in super::super) fn start(&self) -> u32 {
        self.special.start
    }

    pub(in super::super) fn is_special(&self, id: u32) -> bool {
        id <= self.special.max_special
    }

    pub(in super::super) fn is_match(&self, id: u32) -> bool {
        id != 0 && id <= self.special.max_match
    }

    pub(in super::super) fn pattern_len(&self, pattern: u32) -> usize {
        self.pattern_lens.slice()[pattern as usize] as usize
    }

    pub(in super::super) fn match_len(&self, id: u32) -> usize {
        let state = &self.repr.slice()[id as usize..];
        let packed = state[self.match_offset(state)];
        if packed & (1 << 31) == 0 {
            packed as usize
        } else {
            1
        }
    }

    pub(in super::super) fn match_pattern(&self, id: u32, index: usize) -> u32 {
        let state = &self.repr.slice()[id as usize..];
        let start = self.match_offset(state);
        let packed = state[start];
        if packed & (1 << 31) == 0 {
            state[start + 1 + index]
        } else {
            debug_assert_eq!(index, 0);
            packed & !(1 << 31)
        }
    }

    fn match_offset(&self, state: &[u32]) -> usize {
        let kind = state[0] & 0xFF;
        // ONE is reserved for nonmatching states by construction.
        if kind == DENSE {
            2 + self.alphabet_len
        } else {
            let transitions = kind as usize;
            2 + words(transitions) + transitions
        }
    }

    pub(in super::super) fn next_state<A: Admission>(
        &self,
        mut id: u32,
        byte: u8,
        session: &mut Session<'_, A>,
    ) -> Step<u32> {
        let repr = self.repr.slice();
        let class = self.classes[usize::from(byte)];
        loop {
            session.items(1)?;
            let offset = id as usize;
            let kind = repr[offset] & 0xFF;
            if kind == DENSE {
                let next = repr[offset + 2 + usize::from(class)];
                if next != FAIL {
                    return Ok(next);
                }
            } else if kind == ONE {
                if class == ((repr[offset] >> 8) & 0xFF) as u8 {
                    return Ok(repr[offset + 2]);
                }
            } else {
                let classes_len = words(kind as usize);
                let transitions = offset + 2 + classes_len;
                for (index, &chunk) in repr[offset + 2..][..classes_len].iter().enumerate() {
                    session.items(1)?;
                    let classes = chunk.to_ne_bytes();
                    if classes[0] == class {
                        return Ok(repr[transitions + index * 4]);
                    }
                    if classes[1] == class {
                        return Ok(repr[transitions + index * 4 + 1]);
                    }
                    if classes[2] == class {
                        return Ok(repr[transitions + index * 4 + 2]);
                    }
                    if classes[3] == class {
                        return Ok(repr[transitions + index * 4 + 3]);
                    }
                }
            }
            // Failure points closer to the start; the unanchored start's
            // complete row never fails. Padding repeats its last real class,
            // so the earlier comparison returns before any padding index.
            id = repr[offset + 1];
        }
    }
}
