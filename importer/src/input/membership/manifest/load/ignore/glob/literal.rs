// Copyright the ripgrep contributors. MIT OR Unlicense.
//! globset 0.4.18 lib.rs:696-789 literal, basename and extension queries.
use super::super::paths::Candidate;
use super::table;
use crate::input::membership::manifest::{session::Session, storage::Buffer, Step};
use rawr_toml::document::Admission;

pub(super) enum Field {
    Path,
    Basename,
    Extension,
}

pub(super) struct Strategy {
    field: Field,
    indices: table::Indices,
}

impl Strategy {
    pub(super) fn new(field: Field) -> Self {
        Self {
            field,
            indices: table::Indices::new(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub(super) fn add<A: Admission>(
        &mut self,
        index: usize,
        literal: String,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.indices.add(index, literal, session)
    }

    pub(super) fn is_match<A: Admission>(
        &self,
        candidate: &Candidate<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        let Some(bytes) = self.bytes(candidate) else {
            return Ok(false);
        };
        self.indices.contains(bytes, session)
    }

    pub(super) fn matches_into<A: Admission>(
        &self,
        candidate: &Candidate<'_>,
        matches: &mut Buffer<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let Some(bytes) = self.bytes(candidate) else {
            return Ok(());
        };
        if let Some(hits) = self.indices.get(bytes, session)? {
            // Source extends from an exact-size slice. Keep prior strategy hits
            // and admit the whole append before copying its global rule IDs.
            matches.reserve_more(hits.len(), session)?;
            for &index in hits {
                matches.push(index, session)?;
            }
        }
        Ok(())
    }

    fn bytes<'a>(&self, candidate: &Candidate<'a>) -> Option<&'a [u8]> {
        match self.field {
            Field::Path => Some(candidate.path),
            Field::Basename => (!candidate.basename.is_empty()).then_some(candidate.basename),
            Field::Extension => (!candidate.extension.is_empty()).then_some(candidate.extension),
        }
    }
}
