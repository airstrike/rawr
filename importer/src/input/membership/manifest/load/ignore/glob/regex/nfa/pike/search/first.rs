// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! Pinned is_match → search_slots → search_imp with zero slots and earliest=true.
use super::{cache, Admission, Error, Kind, Pike, Session, Step};

impl Pike {
    pub(in super::super) fn earliest<A: Admission>(
        &self,
        lazy: &mut cache::Lazy,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Option<u32>> {
        let cache = lazy.get(self, session)?;
        // The closed forward producer disables UTF-8. search_slots therefore
        // reaches search_imp directly, without capture scratch or split retries.
        session.items(1)?;
        if self.nfa.0.utf8 || self.nfa.0.reverse {
            return session.fail(Error::State);
        }
        cache.setup(session)?;
        session.items(1)?;
        let all = self.kind == Kind::All;
        let anchored = self.nfa.0.anchored == self.nfa.0.unanchored;
        let start = self.nfa.0.anchored;
        let cache::Cache { stack, curr, next } = cache;
        let mut matched = None;
        // Whole input has start=0, end=len and is never already done. Delayed
        // Match states and End assertions require the inclusive final position.
        for at in 0..=bytes.len() {
            session.items(1)?;
            if curr.length == 0 {
                if matched.is_some() && !all {
                    break;
                }
                if anchored && at > 0 {
                    break;
                }
            }
            if (matched.is_none() || all) && (!anchored || at == 0) {
                // SlotTable::all_absent returns the empty suffix after setup(0).
                // The retained full slot allocation is not cleared or replaced.
                session.items(1)?;
                if next.slots.for_captures != 0 {
                    return session.fail(Error::State);
                }
                self.closure(stack, curr, bytes, at, start, session)?;
            }
            let mut index = 0;
            while let Some(id) = curr.at(index, session)? {
                index += 1;
                let Some(pattern) = self.next(stack, curr, next, bytes, at, id, session)? else {
                    continue;
                };
                // nexts copies the matching state's row into the caller's empty
                // slots. Keep that actual row access; overlap does not do it.
                curr.zero_row(id, session)?;
                matched = Some(pattern);
                if !all {
                    break;
                }
            }
            session.items(1)?;
            // Kind::All still visits the complete ordered batch before earliest
            // stops. No PatternSet capacity/insertion can change this boundary.
            if matched.is_some() {
                break;
            }
            std::mem::swap(curr, next);
            next.clear(session)?;
        }
        Ok(matched)
    }
}
