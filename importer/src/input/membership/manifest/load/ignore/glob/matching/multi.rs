// Copyright the ripgrep contributors. MIT OR Unlicense.
//! The one source MultiStrategyBuilder shared by prefix, suffix and regex.
use super::{Admission, Buffer, Session, Step};

pub(in super::super) struct Builder {
    // Future conversions borrow literals through matcher construction, then
    // move map/longest. Keep actual fields; no tuple or duplicate builder.
    pub(in super::super) literals: Buffer<String>,
    pub(in super::super) map: Buffer<usize>,
    pub(in super::super) longest: usize,
}

impl Builder {
    pub(in super::super) fn new() -> Self {
        Self {
            literals: Buffer::new(),
            map: Buffer::new(),
            longest: 0,
        }
    }

    pub(in super::super) fn add<A: Admission>(
        &mut self,
        index: usize,
        literal: String,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if literal.len() > self.longest {
            self.longest = literal.len();
        }
        self.map.push(index, session)?;
        self.literals.push(literal, session)?;
        Ok(())
    }

    pub(in super::super) fn is_empty(&self) -> bool {
        self.literals.len() == 0
    }
}
