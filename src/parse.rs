//! Parsed Rawr compilation units.

use crate::{item, source, AST};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// Parsed source items and the retained script representation they surround.
///
/// The legacy script AST is intentionally private and moves only through
/// [`crate::resolve::resolve`].
///
/// ```compile_fail
/// let engine = rawr::Engine::new();
/// let unit = engine.parse_unit("40 + 2").unwrap();
/// let _ = engine.eval_ast::<i64>(&unit);
/// ```
#[derive(Clone, Debug)]
pub struct Unit {
    /// Source text used to produce this unit.
    source: source::Map,
    /// Source-level Rawr items.
    items: Box<[item::Parsed]>,
    /// Existing script statements outside Rawr item boundaries.
    script: Option<AST>,
    script_source: String,
}

impl Unit {
    pub(crate) fn new(
        source: source::Map,
        items: Box<[item::Parsed]>,
        script: Option<AST>,
        script_source: String,
    ) -> Self {
        Self {
            source,
            items,
            script,
            script_source,
        }
    }

    /// Access the parsed unit's source text and positions.
    #[must_use]
    pub const fn source(&self) -> &source::Map {
        &self.source
    }

    /// Access the parsed Rawr item shells.
    #[must_use]
    pub fn items(&self) -> &[item::Parsed] {
        &self.items
    }

    pub(crate) fn into_parts(self) -> (source::Map, Box<[item::Parsed]>, Option<AST>, String) {
        (self.source, self.items, self.script, self.script_source)
    }
}
