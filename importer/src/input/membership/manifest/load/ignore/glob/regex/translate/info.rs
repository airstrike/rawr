// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../LICENSE-regex-syntax-MIT.
//! Property collection from regex-automata 0.4.14 meta::RegexInfo::new.
//! Shared configuration ownership follows in the private NFA engine stage.
use super::{hir, properties, Expression};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Step};
use rawr_toml::document::Admission;

pub(in super::super) struct Properties {
    patterns: Buffer<properties::Properties>,
    union: properties::Properties,
}
impl Properties {
    /// Keep the source's HIR reference vector and cloned property ownership
    /// alive through the caller's next construction stage.
    pub(in super::super) fn with<A: Admission, T>(
        expressions: &[Expression],
        session: &mut Session<'_, A>,
        operation: impl FnOnce(&Self, &mut Session<'_, A>) -> Step<T>,
    ) -> Step<T> {
        Self::with_owned(expressions, session, |properties, session| {
            operation(&properties, session)
        })
    }

    /// Transfer the collected properties without cloning them a second time.
    /// The HIR reference vector remains live throughout the downstream call.
    pub(in super::super) fn with_owned<A: Admission, T>(
        expressions: &[Expression],
        session: &mut Session<'_, A>,
        operation: impl FnOnce(Self, &mut Session<'_, A>) -> Step<T>,
    ) -> Step<T> {
        session.check()?;
        let mut hirs: Buffer<&hir::Translated> = Buffer::new();
        hirs.reserve_more(expressions.len(), session)?;
        for expression in expressions {
            hirs.push(&expression.0, session)?;
        }
        let mut patterns = Buffer::new();
        for hir in hirs.as_slice() {
            let copy = hir.properties().copy(session)?;
            patterns.push(copy, session)?;
        }
        let union = properties::Properties::union_iter(patterns.as_slice().iter(), session)?;
        let properties = Self { patterns, union };
        let result = operation(properties, session);
        drop(hirs);
        result
    }
    pub(in super::super) fn pattern_len(&self) -> usize {
        self.patterns.len()
    }
    pub(in super::super) fn anchored_start(&self) -> bool {
        self.union.get().look_set_prefix & 1 != 0
    }

    pub(in super::super) fn anchored_end(&self) -> bool {
        self.union.get().look_set_suffix & 2 != 0
    }

    pub(in super::super) fn minimum_len(&self) -> Option<usize> {
        self.union.get().minimum_len
    }

    pub(in super::super) fn maximum_len(&self) -> Option<usize> {
        self.union.get().maximum_len
    }

    /// meta::wrappers::OnePassEngine::new:376 and util::look::LookSet:354.
    pub(in super::super) fn onepass_candidate(&self) -> bool {
        onepass_candidate(self.union.get())
    }
}

fn onepass_candidate(properties: &properties::Record) -> bool {
    const WORD_UNICODE: u32 = (1 << 8) | (1 << 9) | (1 << 12) | (1 << 13) | (1 << 16) | (1 << 17);
    properties.explicit_captures_len != 0 || properties.look_set & WORD_UNICODE != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onepass_heuristic_uses_explicit_captures_and_all_six_unicode_word_looks() {
        let mut record = properties::Record {
            minimum_len: Some(0),
            maximum_len: None,
            look_set: 0,
            look_set_prefix: 0,
            look_set_suffix: 0,
            look_set_prefix_any: 0,
            look_set_suffix_any: 0,
            utf8: false,
            explicit_captures_len: 0,
            static_explicit_captures_len: Some(0),
            literal: false,
            alternation_literal: false,
        };
        assert!(!onepass_candidate(&record));
        for bit in 0..18 {
            record.look_set = 1 << bit;
            assert_eq!(
                onepass_candidate(&record),
                matches!(bit, 8 | 9 | 12 | 13 | 16 | 17),
                "look bit {bit}"
            );
        }
        record.look_set = 0;
        record.explicit_captures_len = 1;
        assert!(onepass_candidate(&record));
    }
}
