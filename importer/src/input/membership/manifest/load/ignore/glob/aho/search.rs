// Copyright the aho-corasick contributors. MIT OR Unlicense.
//! Standard unanchored overlapping iteration, automaton.rs:1423–1550.
use rawr_toml::document::Admission;

use super::engine::Engine;
use crate::input::membership::manifest::{session::Session, Error, Step};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) struct Match {
    pub(in super::super) pattern: usize,
    pub(in super::super) start: usize,
    pub(in super::super) end: usize,
}

// Preserve the source overlapping cursor, including its distinct initial
// empty matches and pending matches before advancing to the next byte.
pub(in super::super) struct Matches<'e, 'h> {
    engine: &'e Engine,
    haystack: &'h [u8],
    matched: Option<Match>,
    id: Option<u32>,
    at: usize,
    next_match_index: Option<usize>,
}

impl Engine {
    pub(in super::super) fn overlapping<'e, 'h, A: Admission>(
        &'e self,
        haystack: &'h [u8],
        session: &mut Session<'_, A>,
    ) -> Step<Matches<'e, 'h>> {
        session.items(1)?;
        // Standard and Unanchored are fixed by construction; the source
        // iterator's three configuration checks cannot reject this owner.
        let _ = self.start();
        Ok(Matches {
            engine: self,
            haystack,
            matched: None,
            id: None,
            at: 0,
            next_match_index: None,
        })
    }
}

impl Matches<'_, '_> {
    pub(in super::super) fn next<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Match>> {
        session.items(1)?;
        self.matched = None;
        let mut id = match self.id {
            None => {
                let id = self.engine.start();
                if self.engine.is_match(id) {
                    let index = self.next_match_index.unwrap_or(0);
                    let length = self.engine.match_len(id, session)?;
                    if index < length {
                        self.next_match_index = Some(index + 1);
                        self.matched = Some(self.get_match(id, index, 0, session)?);
                        return Ok(self.matched);
                    }
                }
                self.at = 0;
                self.id = Some(id);
                self.next_match_index = None;
                id
            }
            Some(id) => {
                if let Some(index) = self.next_match_index {
                    let length = self.engine.match_len(id, session)?;
                    if index < length {
                        self.next_match_index = Some(index + 1);
                        self.matched = Some(self.get_match(id, index, self.at + 1, session)?);
                        return Ok(self.matched);
                    }
                    self.at += 1;
                    self.next_match_index = None;
                }
                id
            }
        };
        while self.at < self.haystack.len() {
            session.items(1)?;
            id = self
                .engine
                .next_state(id, self.haystack[self.at], session)?;
            if self.engine.is_special(id) {
                self.id = Some(id);
                if id == 0 {
                    return Ok(None);
                }
                if self.engine.is_match(id) {
                    self.next_match_index = Some(1);
                    self.matched = Some(self.get_match(id, 0, self.at + 1, session)?);
                    return Ok(self.matched);
                }
                if let Some(prefilter) = self.engine.prefilter() {
                    match prefilter.find(self.haystack, self.at, session)? {
                        None => return Ok(None),
                        Some(position) if position > self.at => {
                            self.at = position;
                            continue;
                        }
                        Some(_) => {}
                    }
                }
            }
            self.at += 1;
        }
        self.id = Some(id);
        Ok(None)
    }

    fn get_match<A: Admission>(
        &self,
        id: u32,
        index: usize,
        end: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Match> {
        let pattern = self.engine.match_pattern(id, index, session)?;
        let length = self.engine.pattern_len(pattern);
        let Some(start) = end.checked_sub(length) else {
            return session.fail(Error::State);
        };
        Ok(Match {
            pattern: pattern as usize,
            start,
            end,
        })
    }
}
