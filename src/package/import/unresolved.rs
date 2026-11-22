//! A `use` of a dependency's path that its name index lacks, as rustc's
//! E0432 reports it.

use crate::diagnostic;
use crate::{Position, Span};

/// A dependency path whose segment `missing` the module `parent` names has
/// no public child of, with the closest child's name.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct Import {
    path: String,
    parent: Box<str>,
    missing: Box<str>,
    suggestion: Option<Box<str>>,
}

impl Import {
    /// The import of `path` its dependency's name index says is missing:
    /// `None` for any other lookup.
    pub(super) fn from_lookup(
        path: String,
        lookup: rawr_import::api::ledger::Lookup,
    ) -> Option<Self> {
        let rawr_import::api::ledger::Lookup::Missing {
            parent,
            missing,
            suggestion,
        } = lookup
        else {
            return None;
        };
        Some(Self {
            path,
            parent,
            missing,
            suggestion,
        })
    }

    /// The report of a path outside a `use` that names it, at `at` in
    /// `source`: rustc's E0432 headline with the module that lacks the
    /// segment and the closest name.
    pub(super) fn plain(
        &self,
        source: &diagnostic::Source,
        at: Span,
    ) -> Result<diagnostic::Report, diagnostic::Error> {
        let Self {
            path,
            parent,
            missing,
            suggestion,
        } = self;
        let mut message = format!("unresolved import `{path}`: no `{missing}` in `{parent}`");
        if let Some(suggestion) = suggestion {
            message.push_str(&format!("; did you mean `{suggestion}`?"));
        }
        Ok(
            diagnostic::Report::error(diagnostic::code::RESOLVE_UNKNOWN_IMPORT, message)
                .primary(source.clone(), at, None)?
                .finish(),
        )
    }

    /// Whether the missing segment is the path's last, an item the module
    /// lacks, rather than a module on the way.
    fn is_last(&self) -> bool {
        self.parent.split("::").count() + 1 == self.path.split("::").count()
    }

    /// The path rustc names the import by: through the segment that is
    /// missing.
    fn named(&self) -> &str {
        if self.is_last() {
            return &self.path;
        }
        let segments = self.parent.split("::").count() + 1;
        let end = self
            .path
            .match_indices("::")
            .nth(segments - 1)
            .map_or(self.path.len(), |(at, _)| at);
        &self.path[..end]
    }

    /// rustc's label: "no `b` in `a`" for an item the module lacks, and
    /// "could not find `b` in `a`" for a module on the way, the module
    /// named by its last segment, and a crate's root as "the root".
    fn label(&self) -> String {
        let module = match self.parent.rsplit_once("::") {
            Some((_, last)) => last,
            None if self.is_last() => "the root",
            None => &self.parent,
        };
        if self.is_last() {
            format!("no `{}` in `{module}`", self.missing)
        } else {
            format!("could not find `{}` in `{module}`", self.missing)
        }
    }

    /// The report at `leaf`, the span of the name the import binds in
    /// `source`: the whole import marked for an item the module lacks, and
    /// the missing segment for a module on the way, with rustc's help
    /// naming the closest child at the segment.
    pub(super) fn report(
        &self,
        source: &diagnostic::Source,
        leaf: Span,
    ) -> Result<diagnostic::Report, diagnostic::Error> {
        let segments = Segments::before(source.text(), leaf);
        let segment = segments
            .as_ref()
            .and_then(|segments| segments.of(&self.missing))
            .unwrap_or(leaf);
        let marked = match (&segments, self.is_last()) {
            (Some(segments), true) => Span::new(segments.start, leaf.end()),
            _ => segment,
        };
        let mut builder = diagnostic::Report::error(
            diagnostic::code::RESOLVE_UNKNOWN_IMPORT,
            format!("unresolved import `{}`", self.named()),
        )
        .primary(source.clone(), marked, Some(self.label().into()))?;
        if let Some(suggestion) = &self.suggestion {
            let edit = diagnostic::suggestion::Edit::new(source.clone(), segment, &**suggestion)?;
            builder = builder.suggestion(diagnostic::Suggestion::new(
                "a similar name exists in the module",
                diagnostic::suggestion::Style::Inline,
                edit,
            ));
        }
        Ok(builder.finish())
    }
}

/// The segments of a written path that ends at a name, each with its span.
struct Segments<'text> {
    start: Position,
    names: Vec<(&'text str, Span)>,
}

impl<'text> Segments<'text> {
    /// The path written up to and including `leaf` on its line of `text`:
    /// its names joined by `::`, back to what starts it (`use `, a `{` or
    /// a `,` of a group).
    fn before(text: &'text str, leaf: Span) -> Option<Self> {
        let line = leaf.start().line()?;
        let written = text.split('\n').nth(line.checked_sub(1)?)?;
        let end_column = leaf.end().position()?;
        let end = written
            .char_indices()
            .nth(end_column.checked_sub(1)?)
            .map_or(written.len(), |(at, _)| at);
        let mut names = Vec::new();
        let mut rest = &written[..end];
        loop {
            let name_start = rest
                .rfind(|character: char| !(character.is_alphanumeric() || character == '_'))
                .map_or(0, |at| at + 1);
            let name = &rest[name_start..];
            if name.is_empty() {
                return None;
            }
            let column = u16::try_from(written[..name_start].chars().count() + 1).ok()?;
            let width = u16::try_from(name.chars().count()).ok()?;
            let line = u16::try_from(line).ok()?;
            names.push((
                name,
                Span::new(
                    Position::new(line, column),
                    Position::new(line, column + width),
                ),
            ));
            match rest[..name_start].strip_suffix("::") {
                Some(before) => rest = before,
                None => break,
            }
        }
        names.reverse();
        Some(Self {
            start: names.first()?.1.start(),
            names,
        })
    }

    /// The span of the segment `name`.
    fn of(&self, name: &str) -> Option<Span> {
        self.names
            .iter()
            .find(|(written, _)| *written == name)
            .map(|(_, span)| *span)
    }
}

#[cfg(test)]
mod tests {
    use super::Import;
    use crate::diagnostic;
    use crate::{Position, Span};

    fn missing(path: &str, parent: &str, segment: &str, suggestion: Option<&str>) -> Import {
        let lookup = rawr_import::api::ledger::Lookup::Missing {
            parent: parent.into(),
            missing: segment.into(),
            suggestion: suggestion.map(Into::into),
        };
        Import::from_lookup(path.into(), lookup).unwrap()
    }

    fn rendered(import: &Import, text: &str, leaf: &str) -> String {
        let source = diagnostic::Source::new("src/main.rs", text).unwrap();
        let at = text.find(leaf).unwrap() as u16 + 1;
        let span = Span::new(
            Position::new(1, at),
            Position::new(1, at + leaf.len() as u16),
        );
        diagnostic::render(&import.report(&source, span).unwrap())
    }

    /// An item a dependency's module lacks marks the whole import, as
    /// rustc's E0432 does, with the closest name as help at its segment.
    #[test]
    fn a_missing_item_of_a_dependency_reads_as_rustc_s_e0432() {
        let import = missing("app::widget::buton", "app::widget", "buton", Some("button"));
        let shown = rendered(&import, "use app::widget::buton;\n", "buton");
        assert_eq!(
            shown,
            "error[E0432]: unresolved import `app::widget::buton`\n \
             --> src/main.rs:1:5\n  |\n\
             1 | use app::widget::buton;\n  \
             |     ^^^^^^^^^^^^^-----\n  \
             |     |            |\n  \
             |     |            help: a similar name exists in the module: `button`\n  \
             |     no `buton` in `widget`\n"
        );
    }

    /// A module missing on the way is marked at its own segment, and the
    /// import is named through it.
    #[test]
    fn a_missing_module_of_a_dependency_is_marked_at_its_segment() {
        let import = missing("app::wdget::Button", "app", "wdget", None);
        let shown = rendered(&import, "use app::wdget::Button;\n", "Button");
        assert!(
            shown.starts_with("error[E0432]: unresolved import `app::wdget`\n"),
            "{shown}"
        );
        assert!(
            shown.contains("^^^^^ could not find `wdget` in `app`"),
            "{shown}"
        );
    }
}
