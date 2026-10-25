// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned PikeVM::which_overlapping_imp for the emitted whole-haystack call.
//! Input is borrowed, Anchored::No and earliest=false; no meta engine is built.
use super::super::{builder::Id, finalize::State};
use super::{cache, info::Kind, set::Set, Admission, Error, Pike, Session, Step};

mod active;
mod first;
mod stack;

impl Pike {
    pub(in super::super) fn overlapping<A: Admission>(
        &self,
        lazy: &mut cache::Lazy,
        bytes: &[u8],
        patterns: &mut Set,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        // The wrapper gets/initializes its cache before the search sets it up.
        let cache = lazy.get(self, session)?;
        cache.setup(session)?;
        session.items(1)?;
        let all = self.kind == Kind::All;
        let anchored = self.nfa.0.anchored == self.nfa.0.unanchored;
        let start = self.nfa.0.anchored;
        let cache::Cache { stack, curr, next } = cache;
        // Empty input still visits position zero; the final position permits
        // End assertions and Match states after the final consuming byte.
        for at in 0..=bytes.len() {
            session.items(1)?;
            let any_matches = !patterns.is_empty();
            if curr.length == 0 {
                if any_matches && !all {
                    break;
                }
                if anchored && at > 0 {
                    break;
                }
            }
            if !any_matches || all {
                self.closure(stack, curr, bytes, at, start, session)?;
            }
            self.nexts(stack, curr, next, bytes, at, patterns, session)?;
            // Do not short-circuit within nexts when an insertion fills the
            // set: the source finishes the current ordered state batch first.
            session.items(1)?;
            if patterns.is_full() {
                break;
            }
            std::mem::swap(curr, next);
            next.clear(session)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn nexts<A: Admission>(
        &self,
        stack: &mut super::storage::Array<cache::Frame>,
        curr: &mut cache::Active,
        next: &mut cache::Active,
        bytes: &[u8],
        at: usize,
        patterns: &mut Set,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let mut index = 0;
        while let Some(id) = curr.at(index, session)? {
            index += 1;
            let Some(pattern) = self.next(stack, curr, next, bytes, at, id, session)? else {
                continue;
            };
            // Finalized emitted NFAs have utf8=false. Retain the source check
            // for completeness without invoking Unicode tables or decoding.
            session.items(1)?;
            if self.nfa.0.has_empty
                && self.nfa.0.utf8
                && bytes
                    .get(at)
                    .is_some_and(|byte| byte & 0b1100_0000 == 0b1000_0000)
            {
                continue;
            }
            // Only the original fixed insufficient-capacity result is ignored.
            // A terminal admission refusal always propagates through `?`.
            let _ = patterns.insert(pattern, session)?;
            if self.kind != Kind::All {
                break;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn next<A: Admission>(
        &self,
        stack: &mut super::storage::Array<cache::Frame>,
        curr: &mut cache::Active,
        next: &mut cache::Active,
        bytes: &[u8],
        at: usize,
        id: Id,
        session: &mut Session<'_, A>,
    ) -> Step<Option<u32>> {
        session.items(1)?;
        let state = match self.nfa.0.states.get(id.index()) {
            Some(state) => state,
            None => return session.fail(Error::State),
        };
        let target = match state {
            State::Match { pattern } => return Ok(Some(*pattern)),
            State::ByteRange { trans } => {
                session.items(1)?;
                bytes
                    .get(at)
                    .filter(|&&byte| trans.start <= byte && byte <= trans.end)
                    .map(|_| trans.next)
            }
            State::Sparse { transitions } => {
                session.items(1)?;
                let Some(&byte) = bytes.get(at) else {
                    return Ok(None);
                };
                let mut target = None;
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
                        target = Some(trans.next);
                        break;
                    }
                }
                target
            }
            _ => None,
        };
        if let Some(target) = target {
            curr.zero_row(id, session)?;
            // A matching transition establishes at < bytes.len(). Rust slices
            // cannot have usize::MAX elements, so this increment cannot wrap.
            self.closure(stack, next, bytes, at + 1, target, session)?;
        }
        Ok(None)
    }

    fn closure<A: Admission>(
        &self,
        stack: &mut super::storage::Array<cache::Frame>,
        next: &mut cache::Active,
        bytes: &[u8],
        at: usize,
        start: Id,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        stack.push(cache::Frame::Explore(start), session)?;
        while let Some(frame) = stack.pop(session)? {
            let cache::Frame::Explore(mut id) = frame else {
                // Search setup cleared every old frame, and zero-slot search
                // never pushes a RestoreCapture frame.
                return session.fail(Error::State);
            };
            loop {
                if !next.insert(id, session)? {
                    break;
                }
                session.items(1)?;
                let state = match self.nfa.0.states.get(id.index()) {
                    Some(state) => state,
                    None => return session.fail(Error::State),
                };
                match state {
                    State::Fail
                    | State::Match { .. }
                    | State::ByteRange { .. }
                    | State::Sparse { .. } => {
                        next.zero_row(id, session)?;
                        break;
                    }
                    State::Look { look, next: target } => {
                        session.items(1)?;
                        let matches = match look {
                            1 => at == 0,
                            2 => at == bytes.len(),
                            _ => return session.fail(Error::State),
                        };
                        if !matches {
                            break;
                        }
                        id = *target;
                    }
                    State::Union { alternates } => {
                        session.items(1)?;
                        let Some((&first, rest)) = alternates.split_first() else {
                            break;
                        };
                        id = first;
                        stack.extend_reverse(rest, session)?;
                    }
                    State::BinaryUnion { alt1, alt2 } => {
                        id = *alt1;
                        stack.push(cache::Frame::Explore(*alt2), session)?;
                    }
                    State::Capture { next: target, .. } => {
                        // All rows have zero active capture slots. The source
                        // skips restore-frame allocation and follows `next`.
                        session.items(1)?;
                        id = *target;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
