// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Owned meta::RegexInfo after the source-ordered property collection.
use super::super::translate;
use super::storage;
use crate::input::membership::manifest::{session::Session, Step};
use rawr_toml::document::Admission;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super::super) enum Kind {
    All,
    LeftmostFirst,
}

pub(super) struct Info(pub(super) Arc<Owned>);
pub(super) struct Owned {
    config: Config,
    properties: translate::Properties,
}

// Retain the pinned Config's complete field-shaped fixed storage. Only the
// globset constructors below set fields; this is not a new configurable engine.
#[derive(Default)]
// The pinned meta Config keeps every field; the shared Info is admitted at its full size.
#[allow(dead_code)]
struct Config {
    match_kind: Option<Kind>,
    utf8_empty: Option<bool>,
    autopre: Option<bool>,
    pre: Option<Option<Prefilter>>,
    which_captures: Option<Captures>,
    nfa_size_limit: Option<Option<usize>>,
    onepass_size_limit: Option<Option<usize>>,
    hybrid_cache_capacity: Option<usize>,
    hybrid: Option<bool>,
    dfa: Option<bool>,
    dfa_size_limit: Option<Option<usize>>,
    dfa_state_limit: Option<Option<usize>>,
    onepass: Option<bool>,
    backtrack: Option<bool>,
    byte_classes: Option<bool>,
    line_terminator: Option<u8>,
}
// The pinned WhichCaptures; only its Option is stored, never constructed.
#[allow(dead_code)]
enum Captures {
    All,
    Implicit,
    None,
}
// Every emitted expression is start-anchored: strategy::new selects no
// prefilter. This unconstructed payload retains the source's Arc trait-object,
// Boolean and length storage without importing any matching capability.
pub(super) struct Prefilter {
    _engine: Arc<dyn Send + Sync>,
    _is_fast: bool,
    _max_needle_len: usize,
}

impl Info {
    /// The actual RegexInfo clone passed to strategy::Core::new.
    pub(super) fn share<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        Ok(Self(Arc::clone(&self.0)))
    }

    pub(super) fn with<A: Admission, T>(
        expressions: &[translate::Expression],
        kind: Kind,
        session: &mut Session<'_, A>,
        operation: impl FnOnce(Self, &mut Session<'_, A>) -> Step<T>,
    ) -> Step<T> {
        session.items(1)?;
        let config = Config {
            match_kind: match kind {
                Kind::All => Some(Kind::All),
                Kind::LeftmostFirst => None,
            },
            utf8_empty: Some(false),
            nfa_size_limit: Some(Some(10 * (1 << 20))),
            hybrid_cache_capacity: Some(10 * (1 << 20)),
            ..Config::default()
        };
        translate::Properties::with_owned(expressions, session, |properties, session| {
            let info = Self(storage::shared(Owned { config, properties }, session)?);
            operation(info, session)
        })
    }

    pub(super) fn kind(&self) -> Kind {
        self.0.config.match_kind.unwrap_or(Kind::LeftmostFirst)
    }

    pub(super) fn pattern_len(&self) -> usize {
        self.0.properties.pattern_len()
    }

    pub(super) fn anchored_start(&self) -> bool {
        self.0.properties.anchored_start()
    }

    pub(super) fn anchored_end(&self) -> bool {
        self.0.properties.anchored_end()
    }

    pub(super) fn minimum_len(&self) -> Option<usize> {
        self.0.properties.minimum_len()
    }

    pub(super) fn maximum_len(&self) -> Option<usize> {
        self.0.properties.maximum_len()
    }

    pub(super) fn auto_prefilter(&self) -> bool {
        self.0.config.autopre.unwrap_or(true)
    }

    pub(super) fn nfa_size_limit(&self) -> Option<usize> {
        self.0.config.nfa_size_limit.unwrap_or(Some(10 * (1 << 20)))
    }

    pub(super) fn dfa_state_limit(&self) -> Option<usize> {
        self.0.config.dfa_state_limit.unwrap_or(Some(30))
    }

    pub(super) fn dfa_size_limit(&self) -> Option<usize> {
        self.0.config.dfa_size_limit.unwrap_or(Some(40 * (1 << 10)))
    }

    pub(super) fn dfa(&self) -> bool {
        self.0.config.dfa.unwrap_or(true)
    }

    pub(super) fn onepass(&self) -> bool {
        self.0.config.onepass.unwrap_or(true)
    }

    pub(super) fn onepass_candidate(&self) -> bool {
        self.0.properties.onepass_candidate()
    }

    pub(super) fn hybrid(&self) -> bool {
        self.0.config.hybrid.unwrap_or(true)
    }

    pub(super) fn hybrid_cache_capacity(&self) -> usize {
        self.0.config.hybrid_cache_capacity.unwrap_or(2 * (1 << 20))
    }

    #[cfg(test)]
    pub(super) fn test_nfa_limit(&mut self, limit: Option<usize>) {
        Arc::get_mut(&mut self.0).unwrap().config.nfa_size_limit = Some(limit);
    }

    #[cfg(test)]
    pub(super) fn test_dfa_limit(&mut self, limit: Option<usize>) {
        Arc::get_mut(&mut self.0).unwrap().config.dfa_size_limit = Some(limit);
    }

    #[cfg(test)]
    pub(super) fn test_hybrid_capacity(&mut self, capacity: usize) {
        Arc::get_mut(&mut self.0)
            .unwrap()
            .config
            .hybrid_cache_capacity = Some(capacity);
    }

    #[cfg(test)]
    pub(super) fn test_engines(&mut self, dfa: bool, hybrid: bool) {
        let config = &mut Arc::get_mut(&mut self.0).unwrap().config;
        config.dfa = Some(dfa);
        config.hybrid = Some(hybrid);
    }
}
