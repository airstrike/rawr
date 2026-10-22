//! meta::Regex::{is_match,which_overlapping_matches} whole-haystack gates.
use super::{info, Admission, Regex, Session, Set, Step};

impl Regex {
    pub(in super::super::super::super) fn is_match<A: Admission>(
        &mut self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        // Source Input::from(bytes).earliest(true), before the impossible gate.
        // Core's Boolean API carries that earliest setting into every engine.
        session.items(1)?;
        if impossible(&self.imp.info, bytes.len(), session)? {
            return Ok(false);
        }
        let cache = self.execution.get(session)?;
        self.imp.strategy.is_match(cache, bytes, session)
    }

    pub(in super::super::super::super) fn overlapping<A: Admission>(
        &mut self,
        bytes: &[u8],
        patterns: &mut Set,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if impossible(&self.imp.info, bytes.len(), session)? {
            return Ok(());
        }
        // This API never clears the caller-owned PatternSet. Source fallback
        // and repeated calls both retain IDs already inserted there.
        let cache = self.execution.get(session)?;
        self.imp
            .strategy
            .overlapping(cache, bytes, patterns, session)
    }
}

pub(super) fn impossible<A: Admission>(
    info: &info::Info,
    length: usize,
    session: &mut Session<'_, A>,
) -> Step<bool> {
    // Only full byte haystacks are passed by the reached globset APIs. The
    // source start>0 and end<haystack.len() guards are both false; their RHS
    // anchor-property reads are not reached. Anchored::No also adds no anchor.
    session.items(1)?;
    let Some(minimum) = info.minimum_len() else {
        // Pinned source returns false immediately, even for zero patterns.
        return Ok(false);
    };
    session.items(1)?;
    if length < minimum {
        return Ok(true);
    }
    session.items(1)?;
    if !info.anchored_start() {
        return Ok(false);
    }
    session.items(1)?;
    if !info.anchored_end() {
        return Ok(false);
    }
    session.items(1)?;
    let Some(maximum) = info.maximum_len() else {
        return Ok(false);
    };
    session.items(1)?;
    Ok(length > maximum)
}
