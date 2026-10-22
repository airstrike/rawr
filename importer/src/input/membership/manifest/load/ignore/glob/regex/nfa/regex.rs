// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Closed meta::Regex ownership with exclusive, retained lazy execution.
use super::super::translate::Expression;
use super::{builder, core, info, pike::set::Set, storage};
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::Admission;
use std::sync::Arc;

mod search;
mod strategy;
#[cfg(test)]
mod tests;

/// A completed strategy and its own execution owner. No Clone or cache guard
/// escapes; a mutable borrow excludes overlapping use of the retained cache.
pub(in super::super::super) struct Regex {
    imp: Arc<Compiled>,
    execution: Execution,
}

struct Compiled {
    strategy: Arc<core::Core>,
    info: info::Info,
}

// Preserve the source Pool's create-before-owner_val field/drop order. The
// direct factory owns its independent strategy Arc before a cache ever exists.
struct Execution {
    factory: Arc<core::Core>,
    cache: Option<core::cache::Cache>,
}

/// Preserve the original structured NFA construction failure without exposing
/// an engine builder or converting a semantic failure into Session refusal.
#[derive(Debug, PartialEq, Eq)]
pub(in super::super::super) struct Failure(builder::Failure);
impl Failure {
    pub(in super::super::super) fn render<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<String> {
        crate::input::membership::manifest::load::formatted(
            format_args!("regex compilation failed: {:?}", self.0),
            session,
        )
    }
}

impl Regex {
    pub(in super::super::super) fn pattern_len(&self) -> usize {
        self.imp.info.pattern_len()
    }

    pub(in super::super) fn new<A: Admission>(
        expressions: &[Expression],
        kind: info::Kind,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        session.check()?;
        // GlobSet::new returns its empty strategy before new_regex_set, and
        // required-extension entries each compile exactly one expression. A
        // zero-HIR meta builder would reach prefilter extraction; it must not
        // be admitted here by pretending that its union is start-anchored.
        if expressions.is_empty() {
            return session.fail(Error::State);
        }
        info::Info::with(expressions, kind, session, |info, session| {
            Self::from_info(info, expressions, session)
        })
    }

    fn from_info<A: Admission>(
        info: info::Info,
        expressions: &[Expression],
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        strategy::prefilter(&info, session)?;
        let shared = info.share(session)?;
        let core = match core::Core::new(shared, expressions, session)? {
            Ok(core) => core,
            Err(cause) => return Ok(Err(Failure(cause))),
        };
        strategy::finish(&info, session)?;
        let strategy = storage::shared(core, session)?;
        let execution = Execution::new(&strategy, session)?;
        let imp = storage::shared(Compiled { strategy, info }, session)?;
        Ok(Ok(Self { imp, execution }))
    }
}

impl Execution {
    fn new<A: Admission>(strategy: &Arc<core::Core>, session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        Ok(Self {
            factory: Arc::clone(strategy),
            cache: None,
        })
    }

    fn get<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<&mut core::cache::Cache> {
        session.items(1)?;
        if self.cache.is_none() {
            let cache = self.factory.create_cache(session)?;
            self.cache = Some(cache);
        }
        match self.cache.as_mut() {
            Some(cache) => Ok(cache),
            None => session.fail(Error::State),
        }
    }
}
