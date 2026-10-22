// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned dense forward queries reached by globset 0.4.18.
//! Whole borrowed haystack, Anchored::No, no prefilter, UTF-8-empty=false.
use super::super::pike::set::Set;
use super::{initial::Initial, Admission, Dfa, Error, Session, Step};

mod table;
#[cfg(test)]
mod tests;

/// Source search failure only; admission refusal stays outside this Result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Failure {
    Quit { byte: u8, offset: usize },
    UnsupportedAnchored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Half {
    pattern: u32,
    offset: usize,
}

#[derive(Default)]
struct Overlapping {
    id: Option<u32>,
    at: usize,
    next_match: Option<usize>,
}

impl Dfa {
    /// meta::Regex::is_match supplies earliest=true to its forward half search.
    pub(in super::super) fn is_match<A: Admission>(
        &self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Result<bool, Failure>> {
        session.items(1)?;
        let Some(pair) = &self.completed else {
            return session.fail(Error::State);
        };
        Ok(first(&pair.forward, bytes, session)?.map(|found| found.is_some()))
    }

    /// Automaton::try_which_overlapping_matches does not clear the caller's set.
    pub(in super::super) fn overlapping<A: Admission>(
        &self,
        bytes: &[u8],
        patterns: &mut Set,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        session.items(1)?;
        let Some(pair) = &self.completed else {
            return session.fail(Error::State);
        };
        let mut state = Overlapping::default();
        loop {
            session.items(1)?;
            let found = match state.next(&pair.forward, bytes, session)? {
                Ok(found) => found,
                Err(error) => return Ok(Err(error)),
            };
            let Some(found) = found else { break };
            // Dense Automaton uses PatternSet::insert (panicking on Full),
            // unlike Pike/hybrid's try_insert. Globset supplies enough slots;
            // a violated private producer contract is terminal internal State,
            // never a successful match, semantic fallback or new panic policy.
            if patterns.insert(found.pattern, session)?.is_err() {
                return session.fail(Error::State);
            }
            session.items(1)?;
            if patterns.is_full() {
                break;
            }
        }
        Ok(Ok(()))
    }
}

// Test-only table fixtures exercise the real semantic-error search path. They
// do not alter production construction, configuration or query return values.
#[cfg(test)]
impl Dfa {
    pub(in super::super) fn test_quit_at_start(&mut self, byte: u8) {
        let dense = &mut self.completed.as_mut().unwrap().forward;
        let start = dense.starts.table.values[2];
        let class = usize::from(dense.classes[usize::from(byte)]);
        dense.transitions.values[start as usize + class] = dense.special[1];
    }

    pub(in super::super) fn test_unsupported_start(&mut self) {
        self.completed
            .as_mut()
            .unwrap()
            .forward
            .starts
            .anchored_only = true;
    }

    pub(in super::super) fn test_match_then_quit(&mut self, prefix: &[u8], byte: u8, pattern: u32) {
        let dense = &mut self.completed.as_mut().unwrap().forward;
        let mut before = dense.starts.table.values[2];
        for &byte in prefix {
            before = dense.transitions.values
                [before as usize + usize::from(dense.classes[usize::from(byte)])];
        }
        let stride = 1_usize << dense.stride2;
        let matched = (dense.special[2]..=dense.special[3])
            .step_by(stride)
            .find(|&id| {
                let index = ((id - dense.special[2]) as usize >> dense.stride2) * 2;
                let offset = dense.matches.slices.values[index] as usize;
                let length = dense.matches.slices.values[index + 1] as usize;
                dense.matches.patterns.values[offset..offset + length] == [pattern]
            })
            .expect("fixture must have a match row containing only the requested pattern");
        let class = usize::from(dense.classes[usize::from(byte)]);
        dense.transitions.values[before as usize + class] = matched;
        dense.transitions.values[matched as usize + class] = dense.special[1];
    }
}

fn first<A: Admission>(
    dense: &Initial,
    bytes: &[u8],
    session: &mut Session<'_, A>,
) -> Step<Result<Option<Half>, Failure>> {
    let mut id = match table::start(dense, session)? {
        Ok(id) => id,
        Err(error) => return Ok(Err(error)),
    };
    let mut at = 0;
    // The pinned four-byte unroll is expanded into the same ordered byte
    // transitions. Each transition is admitted before either input/table read.
    while at < bytes.len() {
        session.items(1)?;
        id = table::next(dense, id, Some(bytes[at]), session)?;
        if table::special(dense, id) {
            if table::start_state(dense, id) {
                if table::accelerated(dense, id) {
                    at = table::accelerate(dense, id, bytes, at + 1, session)?;
                    continue;
                }
            } else if table::matched(dense, id) {
                // Earliest returns before a match state's acceleration or
                // any following quit byte, exactly as find_fwd_imp does.
                let pattern = table::pattern(dense, id, 0, session)?;
                return Ok(Ok(Some(Half {
                    pattern,
                    offset: at,
                })));
            } else if table::accelerated(dense, id) {
                at = table::accelerate(dense, id, bytes, at + 1, session)?;
                continue;
            } else if id == 0 {
                return Ok(Ok(None));
            } else {
                return Ok(Err(Failure::Quit {
                    byte: bytes[at],
                    offset: at,
                }));
            }
        }
        at += 1;
    }
    // Even an empty haystack and an accelerator miss take the EOI transition.
    Ok(Ok(eoi(dense, &mut id, bytes.len(), session)?))
}

impl Overlapping {
    fn next<A: Admission>(
        &mut self,
        dense: &Initial,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Option<Half>, Failure>> {
        session.items(1)?;
        let mut id = match self.id {
            None => {
                self.at = 0;
                match table::start(dense, session)? {
                    Ok(id) => id,
                    Err(error) => return Ok(Err(error)),
                }
            }
            Some(id) => {
                if let Some(index) = self.next_match
                    && index < table::match_len(dense, id, session)?
                {
                    let pattern = table::pattern(dense, id, index, session)?;
                    self.next_match = Some(index + 1);
                    return Ok(Ok(Some(Half {
                        pattern,
                        offset: self.at,
                    })));
                }
                // End-of-haystack matches can have multiple pattern IDs.
                // Only after those IDs are exhausted does the cursor retire.
                let Some(at) = self.at.checked_add(1) else {
                    return session.fail(Error::Overflow);
                };
                self.at = at;
                if self.at > bytes.len() {
                    return Ok(Ok(None));
                }
                id
            }
        };
        while self.at < bytes.len() {
            session.items(1)?;
            id = table::next(dense, id, Some(bytes[self.at]), session)?;
            if table::special(dense, id) {
                self.id = Some(id);
                if table::start_state(dense, id) {
                    if table::accelerated(dense, id) {
                        self.at = table::accelerate(dense, id, bytes, self.at + 1, session)?;
                        continue;
                    }
                } else if table::matched(dense, id) {
                    self.next_match = Some(1);
                    let pattern = table::pattern(dense, id, 0, session)?;
                    return Ok(Ok(Some(Half {
                        pattern,
                        offset: self.at,
                    })));
                } else if table::accelerated(dense, id) {
                    self.at = table::accelerate(dense, id, bytes, self.at + 1, session)?;
                    continue;
                } else if id == 0 {
                    return Ok(Ok(None));
                } else {
                    return Ok(Err(Failure::Quit {
                        byte: bytes[self.at],
                        offset: self.at,
                    }));
                }
            }
            self.at += 1;
        }
        let found = eoi(dense, &mut id, bytes.len(), session)?;
        self.id = Some(id);
        if found.is_some() {
            self.next_match = Some(1);
        }
        Ok(Ok(found))
    }
}

fn eoi<A: Admission>(
    dense: &Initial,
    id: &mut u32,
    offset: usize,
    session: &mut Session<'_, A>,
) -> Step<Option<Half>> {
    *id = table::next(dense, *id, None, session)?;
    // Pinned eoi_fwd does not report Quit for the synthetic EOI symbol.
    if !table::matched(dense, *id) {
        return Ok(None);
    }
    let pattern = table::pattern(dense, *id, 0, session)?;
    Ok(Some(Half { pattern, offset }))
}
