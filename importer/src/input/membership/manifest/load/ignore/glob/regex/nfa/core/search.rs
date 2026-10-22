// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Pinned Core Boolean/overlap dispatch with semantic-only engine fallback.
use super::{cache::Cache, pike::set::Set, Admission, Core, Onepass, Session, Step};

#[cfg(test)]
mod tests;

impl Core {
    pub(in super::super) fn is_match<A: Admission>(
        &self,
        cache: &mut Cache,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        if self.dfa.has_engine() {
            if let Ok(matched) = self.dfa.is_match(bytes, session)? {
                return Ok(matched);
            }
        } else {
            session.items(1)?;
            if self.hybrid.has_engine()
                && let Ok(matched) = self.hybrid.is_match(&mut cache.hybrid, bytes, session)?
            {
                return Ok(matched);
            }
        }
        // A selected dense engine's semantic failure never tries hybrid.
        // Original Session refusal has already returned through `?` above.
        self.is_match_nofail(cache, bytes, session)
    }

    fn is_match_nofail<A: Admission>(
        &self,
        cache: &mut Cache,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        match self.onepass {
            Onepass::Disabled | Onepass::NotUseful => {}
        }
        if let Some(matched) = self
            .backtrack
            .is_match(&mut cache.backtrack, bytes, session)?
        {
            return Ok(matched);
        }
        // Backtracker decline (including earliest haystacks over 128 bytes)
        // differs from its completed false; only decline reaches Pike.
        self.pike.is_match(&mut cache.pike, bytes, session)
    }

    pub(in super::super) fn overlapping<A: Admission>(
        &self,
        cache: &mut Cache,
        bytes: &[u8],
        patterns: &mut Set,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if self.dfa.has_engine() {
            if self.dfa.overlapping(bytes, patterns, session)?.is_ok() {
                return Ok(());
            }
        } else {
            session.items(1)?;
            if self.hybrid.has_engine()
                && self
                    .hybrid
                    .overlapping(&mut cache.hybrid, bytes, patterns, session)?
                    .is_ok()
            {
                return Ok(());
            }
        }
        // The source sends semantic overlap failure directly to Pike, keeping
        // any IDs inserted before failure. It neither clears nor uses BB.
        self.pike
            .overlapping(&mut cache.pike, bytes, patterns, session)
    }
}
