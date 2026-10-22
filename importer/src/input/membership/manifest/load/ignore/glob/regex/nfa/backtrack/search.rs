// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned try_is_match → try_search_slots → search_imp/backtrack/step.
//! The reached caller supplies whole input, Anchored::No, and no capture slots.
use super::super::{builder::Id, finalize::State, storage::Array};
use super::{cache, Admission, Engine, Error, Session, Step};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Hit {
    pub(super) pattern: u32,
    pub(super) offset: usize,
}

impl Engine {
    pub(super) fn search<A: Admission>(
        &self,
        lazy: &mut cache::Lazy,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Option<Hit>, cache::TooLong>> {
        let cache = lazy.get(self, session)?;
        // All emitted NFAs have utf8=false. Consequently try_search_slots
        // bypasses UTF-8 empty-match scratch/copy/retry, including byte splits.
        session.items(1)?;
        if self.nfa.0.utf8 || self.nfa.0.reverse {
            return session.fail(Error::State);
        }
        // slots is empty in try_is_match: no clearing work or restore frames.
        if let Err(error) = cache.setup(self, bytes.len(), session)? {
            return Ok(Err(error));
        }
        session.items(1)?;
        let anchored = self.nfa.0.anchored == self.nfa.0.unanchored;
        let start = self.nfa.0.anchored;
        if anchored {
            return self.backtrack(cache, bytes, 0, start, session).map(Ok);
        }
        // A failure-only expression need not retain its syntactic anchoring.
        // Keep the source retry loop; the emitted strategy has no prefilter.
        for at in 0..=bytes.len() {
            session.items(1)?;
            if let Some(hit) = self.backtrack(cache, bytes, at, start, session)? {
                return Ok(Ok(Some(hit)));
            }
        }
        Ok(Ok(None))
    }

    fn backtrack<A: Admission>(
        &self,
        cache: &mut cache::Cache,
        bytes: &[u8],
        at: usize,
        sid: Id,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Hit>> {
        cache.stack.push(cache::Frame::Step { sid, at }, session)?;
        while let Some(frame) = cache.stack.pop(session)? {
            match frame {
                cache::Frame::Step { sid, at } => {
                    if let Some(hit) = self.step(cache, bytes, sid, at, session)? {
                        return Ok(Some(hit));
                    }
                }
                // setup cleared old frames and no capture slots were supplied.
                cache::Frame::RestoreCapture { .. } => return session.fail(Error::State),
            }
        }
        Ok(None)
    }

    fn step<A: Admission>(
        &self,
        cache: &mut cache::Cache,
        bytes: &[u8],
        mut sid: Id,
        mut at: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Hit>> {
        loop {
            if !cache.visited.insert(sid, at, session)? {
                return Ok(None);
            }
            session.items(1)?;
            let Some(state) = self.nfa.0.states.get(sid.index()) else {
                return session.fail(Error::State);
            };
            match state {
                State::ByteRange { trans } => {
                    session.items(1)?;
                    let Some(&byte) = bytes.get(at) else {
                        return Ok(None);
                    };
                    if !(trans.start <= byte && byte <= trans.end) {
                        return Ok(None);
                    }
                    sid = trans.next;
                    at += 1;
                }
                State::Sparse { transitions } => {
                    session.items(1)?;
                    let Some(&byte) = bytes.get(at) else {
                        return Ok(None);
                    };
                    let mut next = None;
                    let mut index = 0;
                    loop {
                        session.items(1)?;
                        let Some(trans) = transitions.get(index) else {
                            break;
                        };
                        index += 1;
                        if trans.start > byte {
                            break;
                        }
                        if byte <= trans.end {
                            next = Some(trans.next);
                            break;
                        }
                    }
                    let Some(next) = next else {
                        return Ok(None);
                    };
                    sid = next;
                    at += 1;
                }
                State::Look { look, next } => {
                    session.items(1)?;
                    let matches = match look {
                        1 => at == 0,
                        2 => at == bytes.len(),
                        _ => return session.fail(Error::State),
                    };
                    if !matches {
                        return Ok(None);
                    }
                    sid = *next;
                }
                State::Union { alternates } => {
                    session.items(1)?;
                    let Some((&first, rest)) = alternates.split_first() else {
                        return Ok(None);
                    };
                    sid = first;
                    cache.stack.extend_reverse(rest, at, session)?;
                }
                State::BinaryUnion { alt1, alt2 } => {
                    sid = *alt1;
                    cache
                        .stack
                        .push(cache::Frame::Step { sid: *alt2, at }, session)?;
                }
                State::Capture { next, .. } => {
                    // No slot can be in bounds for the empty captures slice.
                    // Source still visits this state and follows its next edge.
                    sid = *next;
                }
                State::Fail => return Ok(None),
                State::Match { pattern } => {
                    return Ok(Some(Hit {
                        pattern: *pattern,
                        offset: at,
                    }));
                }
            }
        }
    }
}

impl Array<cache::Frame> {
    fn extend_reverse<A: Admission>(
        &mut self,
        alternatives: &[Id],
        at: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        // Vec::extend sees this exact-size iterator before moving its frames.
        self.reserve(alternatives.len(), session)?;
        let mut alternatives = alternatives.iter().rev();
        loop {
            session.items(1)?;
            let Some(&sid) = alternatives.next() else {
                break;
            };
            self.values.push(cache::Frame::Step { sid, at });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
