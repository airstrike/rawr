// Copyright the ripgrep contributors. MIT OR Unlicense.
//! globset 0.4.18 lib.rs:792–856 and MultiStrategyBuilder::{prefix,suffix}.
use super::super::aho;
use super::multi;
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;

#[cfg(test)]
mod tests;

pub(in super::super) enum Kind {
    Prefix,
    Suffix,
}

pub(in super::super) struct Strategy {
    matcher: aho::Engine,
    map: Buffer<usize>,
    longest: usize,
}

impl Strategy {
    pub(in super::super) fn build<A: Admission>(
        source: multi::Builder,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, aho::Failure>> {
        // The source Strings remain owned by source through construction, and
        // drop after map/longest have moved into the successful strategy.
        let matcher = match aho::Engine::build(source.literals.as_slice(), session)? {
            Ok(matcher) => matcher,
            Err(error) => return Ok(Err(error)),
        };
        Ok(Ok(Self {
            matcher,
            map: source.map,
            longest: source.longest,
        }))
    }

    pub(in super::super) fn is_match<A: Admission>(
        &self,
        kind: Kind,
        path: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        let path = self.path(&kind, path, session)?;
        let mut matches = self.matcher.overlapping(path, session)?;
        while let Some(found) = matches.next(session)? {
            session.items(1)?;
            if at_edge(&kind, path.len(), &found) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(in super::super) fn matches_into<A: Admission>(
        &self,
        kind: Kind,
        path: &[u8],
        output: &mut Buffer<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let path = self.path(&kind, path, session)?;
        let mut matches = self.matcher.overlapping(path, session)?;
        while let Some(found) = matches.next(session)? {
            session.items(1)?;
            if at_edge(&kind, path.len(), &found) {
                let Some(&index) = self.map.as_slice().get(found.pattern) else {
                    return session.fail(Error::State);
                };
                output.push(index, session)?;
            }
        }
        Ok(())
    }

    fn path<'a, A: Admission>(
        &self,
        kind: &Kind,
        path: &'a [u8],
        session: &mut Session<'_, A>,
    ) -> Step<&'a [u8]> {
        session.items(1)?;
        if path.len() <= self.longest {
            return Ok(path);
        }
        Ok(match kind {
            Kind::Prefix => &path[..self.longest],
            Kind::Suffix => &path[path.len() - self.longest..],
        })
    }
}

fn at_edge(kind: &Kind, length: usize, found: &aho::search::Match) -> bool {
    match kind {
        Kind::Prefix => found.start == 0,
        Kind::Suffix => found.end == length,
    }
}
