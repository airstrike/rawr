// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! hybrid::search's whole-input earliest and overlapping forward searches.
use super::super::{pike, Failure};
use super::transition::Anchored;
use super::{Admission, Cache, Error, Machine, Session, Step, DEAD, MATCH, MAX_ID, START, UNKNOWN};

struct Found {
    pattern: u32,
    offset: usize,
}

#[derive(Default)]
struct Overlapping {
    found: Option<Found>,
    id: Option<u32>,
    at: usize,
    next_match_index: Option<usize>,
}

impl Cache {
    pub(in super::super) fn is_match<A: Admission>(
        &mut self,
        dfa: &Machine,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Result<bool, Failure>> {
        self.earliest(dfa, bytes, session)
            .map(|result| result.map(|found| found.is_some()))
    }

    fn earliest<A: Admission>(
        &mut self,
        dfa: &Machine,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Option<Found>, Failure>> {
        closed(dfa, session)?;
        let mut sid = match self.start(dfa, Anchored::No, None, session)? {
            Ok(sid) => sid,
            Err(_) => return Ok(Err(Failure::GaveUp { offset: 0 })),
        };
        let mut at = 0;
        self.search_start(at, session)?;
        while at < bytes.len() {
            session.items(1)?;
            if tagged(sid) {
                self.search_update(at, session)?;
                sid = match self.next(dfa, sid, Some(bytes[at]), session)? {
                    Ok(sid) => sid,
                    Err(_) => return Ok(Err(Failure::GaveUp { offset: at })),
                };
            } else {
                // Preserve find_fwd_imp's four-transition grouping and its
                // previous ID. Only an UNKNOWN exit updates progress and
                // invokes subset construction; cached untagged runs do not.
                let mut previous = sid;
                while at < bytes.len() {
                    previous = self.cached(dfa, sid, Some(bytes[at]), session)?;
                    if tagged(previous) || bytes.len() - at <= 3 {
                        std::mem::swap(&mut previous, &mut sid);
                        break;
                    }
                    at += 1;
                    sid = self.cached(dfa, previous, Some(bytes[at]), session)?;
                    if tagged(sid) {
                        break;
                    }
                    at += 1;
                    previous = self.cached(dfa, sid, Some(bytes[at]), session)?;
                    if tagged(previous) {
                        std::mem::swap(&mut previous, &mut sid);
                        break;
                    }
                    at += 1;
                    sid = self.cached(dfa, previous, Some(bytes[at]), session)?;
                    if tagged(sid) {
                        break;
                    }
                    at += 1;
                }
                if sid & UNKNOWN != 0 {
                    self.search_update(at, session)?;
                    sid = match self.next(dfa, previous, Some(bytes[at]), session)? {
                        Ok(sid) => sid,
                        Err(_) => return Ok(Err(Failure::GaveUp { offset: at })),
                    };
                }
            }
            if tagged(sid) {
                if sid & START != 0 {
                    // The closed config has no prefilter and never tags starts.
                } else if sid & MATCH != 0 {
                    let pattern = self.match_pattern(dfa, sid, 0, session)?;
                    self.search_finish(at, session)?;
                    return Ok(Ok(Some(Found {
                        pattern,
                        offset: at,
                    })));
                } else if sid & DEAD != 0 {
                    self.search_finish(at, session)?;
                    return Ok(Ok(None));
                } else {
                    // No quit byte exists in this producer; UNKNOWN cannot be
                    // returned by next. Neither is a semantic GaveUp.
                    return session.fail(Error::State);
                }
            }
            at += 1;
        }
        let found = match self.eoi(dfa, bytes.len(), &mut sid, session)? {
            Ok(found) => found,
            Err(error) => return Ok(Err(error)),
        };
        self.search_finish(bytes.len(), session)?;
        Ok(Ok(found))
    }

    pub(in super::super) fn overlapping<A: Admission>(
        &mut self,
        dfa: &Machine,
        bytes: &[u8],
        set: &mut pike::set::Set,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        closed(dfa, session)?;
        let mut state = Overlapping::default();
        loop {
            session.items(1)?;
            if let Err(error) = self.find_overlapping(dfa, bytes, &mut state, session)? {
                return Ok(Err(error));
            }
            let Some(found) = &state.found else {
                return Ok(Ok(()));
            };
            // Unlike dense's insert, pinned hybrid calls try_insert and
            // discards Full. It checks fullness only after this attempt.
            let _ = set.insert(found.pattern, session)?;
            if set.is_full() {
                return Ok(Ok(()));
            }
        }
    }

    fn find_overlapping<A: Admission>(
        &mut self,
        dfa: &Machine,
        bytes: &[u8],
        state: &mut Overlapping,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        session.items(1)?;
        state.found = None;
        let mut sid = match state.id {
            None => {
                state.at = 0;
                match self.start(dfa, Anchored::No, None, session)? {
                    Ok(sid) => sid,
                    Err(_) => return Ok(Err(Failure::GaveUp { offset: 0 })),
                }
            }
            Some(sid) => {
                if let Some(index) = state.next_match_index
                    && index < self.match_len(dfa, sid, session)?
                {
                    state.next_match_index = Some(index + 1);
                    let pattern = self.match_pattern(dfa, sid, index, session)?;
                    state.found = Some(Found {
                        pattern,
                        offset: state.at,
                    });
                    return Ok(Ok(()));
                }
                state.at += 1;
                if state.at > bytes.len() {
                    return Ok(Ok(()));
                }
                sid
            }
        };
        self.search_start(state.at, session)?;
        while state.at < bytes.len() {
            session.items(1)?;
            sid = match self.next(dfa, sid, Some(bytes[state.at]), session)? {
                Ok(sid) => sid,
                Err(_) => return Ok(Err(Failure::GaveUp { offset: state.at })),
            };
            if tagged(sid) {
                state.id = Some(sid);
                if sid & START != 0 {
                    // No prefilter in this fixed producer.
                } else if sid & MATCH != 0 {
                    state.next_match_index = Some(1);
                    let pattern = self.match_pattern(dfa, sid, 0, session)?;
                    state.found = Some(Found {
                        pattern,
                        offset: state.at,
                    });
                    self.search_finish(state.at, session)?;
                    return Ok(Ok(()));
                } else if sid & DEAD != 0 {
                    self.search_finish(state.at, session)?;
                    return Ok(Ok(()));
                } else {
                    return session.fail(Error::State);
                }
            }
            state.at += 1;
            self.search_update(state.at, session)?;
        }
        // Source overlapping search finishes progress even on an EOI semantic
        // error. Its earliest search instead returns that error before finish.
        let result = self.eoi(dfa, bytes.len(), &mut sid, session)?;
        state.id = Some(sid);
        let result = match result {
            Ok(found) => {
                state.found = found;
                if state.found.is_some() {
                    state.next_match_index = Some(1);
                }
                Ok(())
            }
            Err(error) => Err(error),
        };
        self.search_finish(bytes.len(), session)?;
        Ok(result)
    }

    fn eoi<A: Admission>(
        &mut self,
        dfa: &Machine,
        offset: usize,
        sid: &mut u32,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Option<Found>, Failure>> {
        *sid = match self.next(dfa, *sid, None, session)? {
            Ok(sid) => sid,
            Err(_) => return Ok(Err(Failure::GaveUp { offset })),
        };
        if *sid & MATCH != 0 {
            let pattern = self.match_pattern(dfa, *sid, 0, session)?;
            Ok(Ok(Some(Found { pattern, offset })))
        } else {
            Ok(Ok(None))
        }
    }
}

fn tagged(id: u32) -> bool {
    id & !MAX_ID != 0
}

fn closed<A: Admission>(dfa: &Machine, session: &mut Session<'_, A>) -> Step<()> {
    session.items(1)?;
    if dfa.nfa().0.reverse || dfa.nfa().0.utf8 {
        return session.fail(Error::State);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
