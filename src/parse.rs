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
    /// Original imports before legacy hoisting and deduplication.
    imports: Box<[item::Use]>,
    /// Accepted inner attributes of this source file.
    inner_attributes: Box<[item::Attribute]>,
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
            imports: Box::default(),
            inner_attributes: Box::default(),
            statements,
            script_source,
        }
    }

    pub(crate) fn with_scope(
        mut self,
        imports: Box<[item::Use]>,
        inner_attributes: Box<[item::Attribute]>,
    ) -> Self {
        self.imports = imports;
        self.inner_attributes = inner_attributes;
        self
    }

    /// Every original import, with its lexical scope, before hoisting or deduplication.
    #[must_use]
    pub fn imports(&self) -> &[item::Use] {
        &self.imports
    }

    /// Accepted inner attributes at the beginning of this source file.
    /// Unsupported prelude-changing attributes cause parsing to fail.
    #[must_use]
    pub fn inner_attributes(&self) -> &[item::Attribute] {
        &self.inner_attributes
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

#[cfg(test)]
mod tests {
    use crate::{item, Engine, Position, Span};

    fn span(source: &str, fragment: &str) -> Span {
        let start = source.find(fragment).unwrap();
        Span::new(
            Position::new(1, start as u16 + 1),
            Position::new(1, (start + fragment.len()) as u16 + 1),
        )
    }

    #[test]
    fn lexical_imports_preserve_duplicate_locations_and_innermost_blocks() {
        let source = "use dep::Trait; fn left() { use dep::Trait; { use dep::Trait; } fn nested() { use dep::Trait; } } fn right() { use dep::Trait; }";
        let unit = Engine::new().parse_unit(source).unwrap();
        let imports = unit.imports();
        assert_eq!(imports.len(), 5);
        assert_eq!(imports[0].scope, None);
        assert_eq!(
            imports[1].scope,
            Some(span(
                source,
                "{ use dep::Trait; { use dep::Trait; } fn nested() { use dep::Trait; } }"
            ))
        );
        assert_eq!(imports[2].scope, Some(span(source, "{ use dep::Trait; }")));
        let nested = source.find("fn nested()").unwrap();
        assert_eq!(
            imports[3].scope.unwrap().start(),
            Position::new(1, (nested + "fn nested() ".len()) as u16 + 1)
        );
        let right = source.find("fn right()").unwrap();
        assert_eq!(
            imports[4].scope.unwrap().start(),
            Position::new(1, (right + "fn right() ".len()) as u16 + 1)
        );
        assert!(imports
            .windows(2)
            .all(|pair| pair[0].span.start() < pair[1].span.start()));
        // Existing resolution retains its compatibility view; evidence never does.
        assert_eq!(
            unit.items()
                .iter()
                .filter(|item| matches!(item, item::Parsed::Use(_)))
                .count(),
            1
        );
    }

    #[test]
    fn lexical_imports_keep_grouped_anonymous_global_and_glob_paths() {
        let source = "fn left() { use ::dep::{Trait as _, nested::{Other as _, more::*}}; } fn right() { use dep::nested::more::*; }";
        let unit = Engine::new().parse_unit(source).unwrap();
        assert_eq!(unit.imports().len(), 4);
        let left = span(
            source,
            "{ use ::dep::{Trait as _, nested::{Other as _, more::*}}; }",
        );
        assert!(unit.imports()[..3]
            .iter()
            .all(|used| used.scope == Some(left)));
        let named = unit
            .imports()
            .iter()
            .flat_map(item::Use::bindings)
            .collect::<Vec<_>>();
        assert_eq!(named.len(), 2);
        assert!(named.iter().all(|(name, _)| name.text.starts_with("_ ")));
        let paths = named
            .iter()
            .map(|(_, path)| path.iter().map(|part| part.as_str()).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        assert!(paths.contains(&vec!["dep", "Trait"]));
        assert!(paths.contains(&vec!["dep", "nested", "Other"]));
        assert!(unit.imports()[..3].iter().all(item::Use::global));
        let globs = unit
            .imports()
            .iter()
            .filter(|used| matches!(used.tree, item::UseTree::Glob { .. }))
            .collect::<Vec<_>>();
        assert_eq!(globs.len(), 2);
        assert_ne!(globs[0].scope, globs[1].scope);
    }

    #[test]
    fn lexical_imports_exclude_disabled_items_and_parser_failures_do_not_leak() {
        let engine = Engine::new();
        let source = "#[cfg(test)] fn disabled() { use dep::Hidden; } fn enabled() { let text = \"use dep::Fake; { }\"; use dep::Visible; }";
        let unit = engine.parse_unit(source).unwrap();
        assert_eq!(unit.imports().len(), 1);
        assert_eq!(unit.imports()[0].bindings()[0].0.text, "Visible");
        assert!(engine
            .parse_unit("#![allow(dead_code)] fn broken() { use dep::Leaked;")
            .is_err());
        let clean = engine.parse_unit("fn clean() {}").unwrap();
        assert!(clean.imports().is_empty());
        assert!(clean.inner_attributes().is_empty());
    }

    #[test]
    fn lexical_imports_retain_lints_and_refuse_unsupported_prelude_changes() {
        let engine = Engine::new();
        let unit = engine
            .parse_unit("#![allow(dead_code)] use dep::Trait;")
            .unwrap();
        assert_eq!(unit.inner_attributes().len(), 1);
        assert_eq!(
            unit.inner_attributes()[0].path.segments[0].name.text,
            "allow"
        );
        for attribute in ["no_std", "no_implicit_prelude"] {
            let error = engine
                .parse_unit(format!("#![{attribute}] fn main() {{}}"))
                .unwrap_err();
            assert!(error.to_string().contains("unsupported inner attribute"));
            assert!(error.to_string().contains(attribute));
        }
    }
}
