// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Source-ordered meta::strategy::Core cache creation and reset.
use super::{backtrack, hybrid, pike, Admission, Core, Session, Step};

mod captures;
#[cfg(test)]
mod tests;

/// All required fields are completed before this value can be returned.
pub(in super::super) struct Cache {
    capmatches: captures::Captures,
    pub(super) pike: pike::cache::Lazy,
    pub(super) backtrack: backtrack::cache::Lazy,
    onepass: Onepass,
    pub(super) hybrid: hybrid::Cache,
    revhybrid: ReverseHybrid,
}

// These are cache outcomes, not engine construction declines. The closed Core
// constructor has already evaluated the actual one-pass configuration/predicate.
enum Onepass {
    None,
}

// Core never installs the reverse-only strategy's cache. Keep its field
// explicit; reset_cache does not touch it.
enum ReverseHybrid {
    None,
}

impl Cache {
    pub(super) fn new<A: Admission>(core: &Core, session: &mut Session<'_, A>) -> Step<Self> {
        session.check()?;
        let capmatches = captures::Captures::all(&core.nfa.0.groups, session)?;
        let pike = core.pike.create_cache(session)?;
        let backtrack = core.backtrack.create_cache(session)?;
        let onepass = Onepass::new(&core.onepass, session)?;
        let hybrid = core.hybrid.create_cache(session)?;
        let revhybrid = ReverseHybrid::none(session)?;
        Ok(Self {
            capmatches,
            pike,
            backtrack,
            onepass,
            hybrid,
            revhybrid,
        })
    }

    /// Preserve captures and reverse-only cache exactly as Core::reset_cache.
    pub(in super::super) fn reset<A: Admission>(
        &mut self,
        core: &Core,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.pike.reset(&core.pike, session)?;
        self.backtrack.reset(&core.backtrack, session)?;
        self.onepass.reset(&core.onepass, session)?;
        self.hybrid.reset(&core.hybrid, session)
    }
}

impl Onepass {
    fn new<A: Admission>(engine: &super::Onepass, session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        match engine {
            super::Onepass::Disabled | super::Onepass::NotUseful => Ok(Self::None),
        }
    }

    fn reset<A: Admission>(
        &mut self,
        engine: &super::Onepass,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        match (self, engine) {
            (Self::None, super::Onepass::Disabled | super::Onepass::NotUseful) => Ok(()),
        }
    }
}

impl ReverseHybrid {
    fn none<A: Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        Ok(Self::None)
    }
}
