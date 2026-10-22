// Copyright the ripgrep contributors. MIT OR Unlicense.
//! The complete private GlobSet owner and its closed seven-strategy dispatch.
use std::alloc::Layout;

use rawr_toml::document::{Admission, Request};

use super::super::paths::Candidate;
use super::{aho, collection::Collection, literal, matching, Built};
use crate::input::membership::manifest::{session::Session, storage, storage::Buffer, Error, Step};

#[cfg(test)]
mod tests;

pub(in super::super) struct Set {
    len: usize,
    strategies: Vec<Strategy>,
}

/// The source unwrap at the mandatory Aho boundary is deliberately distinct
/// from GlobSet's returned regex diagnostic. See compiled/PROVENANCE.md.
pub(in super::super) enum Failure {
    Aho(aho::Failure),
    Regex(matching::Failure),
}

enum Strategy {
    Literal(literal::Strategy),
    Basename(literal::Strategy),
    Extension(literal::Strategy),
    Prefix(matching::edge::Strategy),
    Suffix(matching::edge::Strategy),
    Required(matching::required::Strategy),
    Regex(matching::set::Strategy),
}

impl Set {
    pub(in super::super) fn build<A: Admission>(
        globs: &[Built],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        let Some(rules) = Collection::new(globs, session)?.into_rules() else {
            return Ok(Ok(Self {
                len: 0,
                strategies: Vec::new(),
            }));
        };
        // Exact source Vec::with_capacity(7), before any builder conversion.
        // Nothing may grow this Vec beyond those seven admitted slots.
        let layout = match Layout::array::<Strategy>(7) {
            Ok(layout) => layout,
            Err(_) => return session.fail(Error::Overflow),
        };
        session.admit(Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        let mut strategies = Vec::new();
        if let Err(error) = strategies.try_reserve_exact(7) {
            return session.fail(Error::Allocation(error));
        }
        // strategies drops before remaining Rules on every failure. Rules
        // itself retains the reverse local declaration order of the source.
        session.items(1)?;
        if !rules.extensions.is_empty() {
            push(
                &mut strategies,
                Strategy::Extension(rules.extensions),
                session,
            )?;
        }
        session.items(1)?;
        if !rules.basenames.is_empty() {
            push(
                &mut strategies,
                Strategy::Basename(rules.basenames),
                session,
            )?;
        }
        session.items(1)?;
        if !rules.literals.is_empty() {
            push(&mut strategies, Strategy::Literal(rules.literals), session)?;
        }
        session.items(1)?;
        if !rules.suffixes.is_empty() {
            let strategy = match matching::edge::Strategy::build(rules.suffixes, session)? {
                Ok(strategy) => strategy,
                Err(error) => return Ok(Err(Failure::Aho(error))),
            };
            push(&mut strategies, Strategy::Suffix(strategy), session)?;
        }
        session.items(1)?;
        if !rules.prefixes.is_empty() {
            let strategy = match matching::edge::Strategy::build(rules.prefixes, session)? {
                Ok(strategy) => strategy,
                Err(error) => return Ok(Err(Failure::Aho(error))),
            };
            push(&mut strategies, Strategy::Prefix(strategy), session)?;
        }
        session.items(1)?;
        if !rules.required.is_empty() {
            let strategy = match matching::required::Strategy::build(rules.required, session)? {
                Ok(strategy) => strategy,
                Err(error) => return Ok(Err(Failure::Regex(error))),
            };
            push(&mut strategies, Strategy::Required(strategy), session)?;
        }
        session.items(1)?;
        if !rules.regexes.is_empty() {
            let strategy = match rules.regexes.build(session)? {
                Ok(strategy) => strategy,
                Err(error) => return Ok(Err(Failure::Regex(error))),
            };
            push(&mut strategies, Strategy::Regex(strategy), session)?;
        }
        Ok(Ok(Self {
            len: rules.len,
            strategies,
        }))
    }

    pub(in super::super) fn len(&self) -> usize {
        self.len
    }

    pub(in super::super) fn is_match<A: Admission>(
        &mut self,
        candidate: &Candidate<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        if self.len == 0 {
            return Ok(false);
        }
        for strategy in &mut self.strategies {
            session.items(1)?;
            if strategy.is_match(candidate, session)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(in super::super) fn matches_into<A: Admission>(
        &mut self,
        candidate: &Candidate<'_>,
        output: &mut Buffer<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        output.clear();
        session.items(1)?;
        if self.len == 0 {
            return Ok(());
        }
        for strategy in &mut self.strategies {
            session.items(1)?;
            strategy.matches_into(candidate, output, session)?;
        }
        // Equal IDs have no associated identity. The established admitted
        // allocation-free sort gives the source's same ascending sequence.
        storage::try_sort(output.as_mut_slice(), |left, right| {
            session.items(1)?;
            Ok(left.cmp(right))
        })?;
        dedup(output, session)
    }
}

impl Strategy {
    fn is_match<A: Admission>(
        &mut self,
        candidate: &Candidate<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        match self {
            Self::Literal(strategy) | Self::Basename(strategy) | Self::Extension(strategy) => {
                strategy.is_match(candidate, session)
            }
            Self::Prefix(strategy) => {
                strategy.is_match(matching::edge::Kind::Prefix, candidate.path, session)
            }
            Self::Suffix(strategy) => {
                strategy.is_match(matching::edge::Kind::Suffix, candidate.path, session)
            }
            Self::Required(strategy) => {
                strategy.is_match(candidate.path, candidate.extension, session)
            }
            Self::Regex(strategy) => strategy.is_match(candidate.path, session),
        }
    }

    fn matches_into<A: Admission>(
        &mut self,
        candidate: &Candidate<'_>,
        output: &mut Buffer<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        match self {
            Self::Literal(strategy) | Self::Basename(strategy) | Self::Extension(strategy) => {
                strategy.matches_into(candidate, output, session)
            }
            Self::Prefix(strategy) => strategy.matches_into(
                matching::edge::Kind::Prefix,
                candidate.path,
                output,
                session,
            ),
            Self::Suffix(strategy) => strategy.matches_into(
                matching::edge::Kind::Suffix,
                candidate.path,
                output,
                session,
            ),
            Self::Required(strategy) => {
                strategy.matches_into(candidate.path, candidate.extension, output, session)
            }
            Self::Regex(strategy) => strategy.matches_into(candidate.path, output, session),
        }
    }
}

fn push<A: Admission>(
    strategies: &mut Vec<Strategy>,
    strategy: Strategy,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.items(1)?;
    if strategies.len() == 7 {
        return session.fail(Error::State);
    }
    strategies.push(strategy);
    Ok(())
}

fn dedup<A: Admission>(output: &mut Buffer<usize>, session: &mut Session<'_, A>) -> Step<()> {
    session.items(1)?;
    if output.len() < 2 {
        return Ok(());
    }
    let values = output.as_mut_slice();
    let mut write = 1;
    for read in 1..values.len() {
        session.items(1)?;
        if values[read] != values[write - 1] {
            values[write] = values[read];
            write += 1;
        }
    }
    session.items(1)?;
    output.truncate(write);
    Ok(())
}
