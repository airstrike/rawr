//! The places Rust can't move out of that the lowering gives no place of
//! their own (`*r`, `*rc`, `[s][0]`, a static item), and what rustc
//! refuses of each.

use super::{ParsedExpr, Span, Unplaced};
use std::collections::BTreeMap;

/// Each place by the expression it is: its span and its source. A crate
/// macro's expansion reports every token at the call, so expressions
/// there share a span (`Metadata::new(name, &__CALLSITE)` and the
/// `__CALLSITE` it borrows), and only the source tells them apart.
#[derive(Default)]
pub(super) struct Unmovable(BTreeMap<Span, Vec<(String, Unplaced)>>);

impl Unmovable {
    /// Note what Rust refuses of the place `value` is.
    pub(super) fn insert(&mut self, value: &ParsedExpr, unplaced: Unplaced) {
        let places = self.0.entry(value.span).or_default();
        let source = value.source.as_str();
        match places.iter_mut().find(|(written, _)| written == source) {
            Some((_, held)) => *held = unplaced,
            None => places.push((source.to_owned(), unplaced)),
        }
    }

    /// What Rust refuses of `value`, when it is one of these places.
    pub(super) fn get(&self, value: &ParsedExpr) -> Option<&Unplaced> {
        let source = value.source.as_str();
        self.0
            .get(&value.span)?
            .iter()
            .find(|(written, _)| written == source)
            .map(|(_, unplaced)| unplaced)
    }
}
