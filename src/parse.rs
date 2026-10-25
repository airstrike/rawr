//! Parsed Rawr compilation units.

use crate::{item, source, Position};

/// Parsed source items and the statements they surround.
///
/// A unit is not runnable on its own; it moves through
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
    /// Where the first statement outside the items starts, when there is one.
    statements: Option<Position>,
    /// The source with the items blanked: the statements, which the checker
    /// reads as the body of `fn main`.
    script_source: String,
}

impl Unit {
    pub(crate) fn new(
        source: source::Map,
        items: Box<[item::Parsed]>,
        statements: Option<Position>,
        script_source: String,
    ) -> Self {
        Self {
            source,
            items,
            statements,
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

    /// The items, where the statements start and the statements' source.
    pub(crate) fn parts(&self) -> (&[item::Parsed], Option<Position>, &str) {
        (&self.items, self.statements, &self.script_source)
    }

    pub(crate) fn into_parts(self) -> (source::Map, Box<[item::Parsed]>, Option<Position>, String) {
        (self.source, self.items, self.statements, self.script_source)
    }
}
