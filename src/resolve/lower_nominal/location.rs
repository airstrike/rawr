//! Position evidence retained while nominal callbacks are emitted.

use crate::{item, program, Position, Span};
use std::cmp::Reverse;
use std::fmt;
use std::ops::Range;

#[derive(Clone)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Output {
    pub(super) generated: Range<usize>,
    pub(super) original: Span,
    pub(super) nesting: usize,
}

pub(super) struct Pending {
    /// `None` for the script's own statements, which no item owns.
    pub(super) callable: Option<item::Key>,
    pub(super) source: program::source::Id,
    pub(super) generated: Range<usize>,
    pub(super) original: Span,
    pub(super) nesting: usize,
}

/// Concatenated compiler source paired with complete user-source ownership.
pub(super) struct Generated {
    text: String,
    origins: Vec<Origin>,
    eof: Option<At>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct At {
    pub(super) source: program::source::Id,
    pub(super) span: Span,
}

struct Origin {
    generated: Range<usize>,
    source: program::source::Id,
    class: Class,
    priority: Priority,
    nesting: usize,
}

enum Class {
    Declaration(Span),
    Body(Span),
    Expression(Span),
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Priority {
    Owned,
    Exact,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Error {
    MissingPosition,
    PositionOutsideGenerated(Span),
    MissingOrigin(Span),
    CrossSourceSpan(Span),
    ReversedOrigin(Span),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPosition => {
                formatter.write_str("compiler diagnostic has no generated position")
            }
            Self::PositionOutsideGenerated(span) => write!(
                formatter,
                "compiler diagnostic span {span} falls outside generated source"
            ),
            Self::MissingOrigin(span) => write!(
                formatter,
                "compiler diagnostic span {span} has no generated-origin proof"
            ),
            Self::CrossSourceSpan(span) => write!(
                formatter,
                "compiler diagnostic span {span} crosses generated source owners"
            ),
            Self::ReversedOrigin(span) => write!(
                formatter,
                "compiler diagnostic span {span} maps to reversed user-source evidence"
            ),
        }
    }
}

impl Generated {
    pub(super) fn new() -> Self {
        Self {
            text: String::new(),
            origins: Vec::new(),
            eof: None,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.text.len()
    }

    pub(super) fn as_str(&self) -> &str {
        &self.text
    }

    pub(super) fn declaration(&mut self, source: program::source::Id, span: Span, text: &str) {
        self.push(source, Class::Declaration(span), Priority::Owned, text);
    }

    pub(super) fn body(
        &mut self,
        source: program::source::Id,
        span: Span,
        text: &str,
        expressions: &[Output],
    ) -> usize {
        let offset = self.len();
        self.push(source, Class::Body(span), Priority::Owned, text);
        for expression in expressions {
            let Some(start) = offset.checked_add(expression.generated.start) else {
                continue;
            };
            let Some(end) = offset.checked_add(expression.generated.end) else {
                continue;
            };
            if start > end || end > self.text.len() {
                continue;
            }
            self.origins.push(Origin {
                generated: start..end,
                source,
                class: Class::Expression(expression.original),
                priority: Priority::Exact,
                nesting: expression.nesting,
            });
        }
        offset
    }

    pub(super) fn translate(&self, span: Span) -> Result<At, Error> {
        if span.is_none() {
            return Err(Error::MissingPosition);
        }
        let start =
            byte_at(&self.text, span.start()).ok_or(Error::PositionOutsideGenerated(span))?;
        let end = byte_at(&self.text, span.end()).ok_or(Error::PositionOutsideGenerated(span))?;
        if start > end {
            return Err(Error::PositionOutsideGenerated(span));
        }
        let first = self.at(start).ok_or(Error::MissingOrigin(span))?;
        let last = self.at(end).ok_or(Error::MissingOrigin(span))?;
        if first.source != last.source {
            return Err(Error::CrossSourceSpan(span));
        }
        let translated = Span::new(first.span.start(), last.span.end());
        if translated.start() > translated.end() {
            return Err(Error::ReversedOrigin(span));
        }
        Ok(At {
            source: first.source,
            span: translated,
        })
    }

    fn push(&mut self, source: program::source::Id, class: Class, priority: Priority, text: &str) {
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        if start < end {
            self.eof = Some(At {
                source,
                span: class.at_end(),
            });
            self.origins.push(Origin {
                generated: start..end,
                source,
                class,
                priority,
                nesting: usize::MAX,
            });
        }
    }

    fn at(&self, byte: usize) -> Option<At> {
        if byte == self.text.len() {
            return self.eof;
        }
        let origin = self
            .origins
            .iter()
            .filter(|origin| origin.generated.contains(&byte))
            .max_by_key(|origin| {
                (
                    origin.priority,
                    Reverse(origin.nesting),
                    Reverse(origin.generated.end - origin.generated.start),
                    origin.generated.start,
                    Reverse(origin.class.ordering_span().start()),
                    Reverse(origin.class.ordering_span().end()),
                )
            })?;
        Some(At {
            source: origin.source,
            span: match origin.class {
                Class::Declaration(span) | Class::Body(span) | Class::Expression(span) => span,
            },
        })
    }
}

impl Class {
    fn ordering_span(&self) -> Span {
        match self {
            Self::Declaration(span) | Self::Body(span) | Self::Expression(span) => *span,
        }
    }

    fn at_end(&self) -> Span {
        match self {
            Self::Declaration(span) | Self::Body(span) | Self::Expression(span) => *span,
        }
    }
}

/// The byte at `position` in `source`. A position's column is 1-based, and
/// a half-open span's end may sit one past a line's last character: that
/// is the line's newline, or the text's end.
pub(super) fn byte_at(source: &str, position: Position) -> Option<usize> {
    let line = position.line()?;
    let column = position.position().map_or(0, |column| column - 1);
    let mut current_line = 1_usize;
    let mut current_column = 0_usize;
    for (byte, character) in source.char_indices() {
        if current_line == line && current_column == column {
            return Some(byte);
        }
        if character == '\n' {
            current_line = current_line.saturating_add(1);
            current_column = 0;
        } else {
            current_column = current_column.saturating_add(1);
        }
    }
    (current_line == line && current_column == column).then_some(source.len())
}

/// Compiler-created evidence translating one generated callback range.
pub(crate) struct Location {
    pub(crate) callable: Option<crate::item::Key>,
    pub(crate) source: crate::program::source::Id,
    pub(crate) generated: Span,
    pub(crate) original: Span,
    pub(crate) nesting: usize,
}

pub(super) fn retain_locations(
    retained: &mut Vec<Pending>,
    body_offset: usize,
    callable: Option<&crate::item::Key>,
    source: crate::program::source::Id,
    locations: Vec<Output>,
) {
    retained.extend(locations.into_iter().filter_map(|location| {
        let start = body_offset.checked_add(location.generated.start)?;
        let end = body_offset
            .checked_add(location.generated.end)?
            .checked_sub(1)?;
        Some(Pending {
            callable: callable.cloned(),
            source,
            generated: start..end.saturating_add(1),
            original: location.original,
            nesting: location.nesting,
        })
    }));
}

pub(super) fn resolve_locations(generated: &str, locations: Vec<Pending>) -> Vec<Location> {
    let mut lines = vec![0];
    lines.extend(
        generated
            .char_indices()
            .filter_map(|(index, character)| (character == '\n').then_some(index + 1)),
    );
    locations
        .into_iter()
        .filter_map(|location| {
            let end = location.generated.end.checked_sub(1)?;
            Some(Location {
                callable: location.callable,
                source: location.source,
                generated: Span::new(
                    position_at(generated, &lines, location.generated.start),
                    position_at(generated, &lines, end),
                ),
                original: location.original,
                nesting: location.nesting,
            })
        })
        .collect()
}

pub(super) fn position_at(source: &str, lines: &[usize], byte: usize) -> Position {
    let line = lines
        .partition_point(|start| *start <= byte)
        .saturating_sub(1);
    let column = source[lines[line]..byte].chars().count();
    Position::new(
        u16::try_from(line.saturating_add(1)).unwrap_or(u16::MAX),
        u16::try_from(column.saturating_add(1)).unwrap_or(u16::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::{Generated, Output};
    use crate::{program, Position, Span};

    #[test]
    fn generated_body_and_framing_retain_exact_origins_across_prefixes() {
        let source = program::source::Id::from_index(2);
        let declaration = span(8, 12, 8, 16);
        let body = span(8, 20, 10, 2);
        let expression = span(9, 5, 9, 18);
        let mut generated = Generated::new();
        generated.declaration(
            program::source::Id::ROOT,
            span(1, 1, 1, 5),
            "fn prior() {}\n",
        );
        generated.declaration(source, declaration, "__rawr_private fn lowered() {\n");
        generated.body(
            source,
            body,
            "helper(invalid);\n",
            &[Output {
                generated: 0..16,
                original: expression,
                nesting: 1,
            }],
        );
        generated.declaration(source, declaration, "}\n");

        let header = generated
            .translate(Span::new(Position::new(2, 4), Position::new(2, 4)))
            .expect("generated header has exact declaration ownership");
        assert_eq!(header.source, source);
        assert_eq!(header.span, declaration);

        let emitted = generated
            .translate(Span::new(Position::new(3, 8), Position::new(3, 8)))
            .expect("generated body has exact expression ownership");
        assert_eq!(emitted.source, source);
        assert_eq!(emitted.span, expression);
    }

    #[test]
    fn explicit_nesting_selects_the_inner_origin_in_any_input_order() {
        let source = program::source::Id::from_index(5);
        let body = span(1, 1, 1, 18);
        let outer = span(8, 3, 12, 4);
        let inner = span(10, 7, 10, 14);

        for locations in [
            vec![
                Output {
                    generated: 7..14,
                    original: inner,
                    nesting: 1,
                },
                Output {
                    generated: 0..16,
                    original: outer,
                    nesting: 3,
                },
            ],
            vec![
                Output {
                    generated: 0..16,
                    original: outer,
                    nesting: 3,
                },
                Output {
                    generated: 7..14,
                    original: inner,
                    nesting: 1,
                },
            ],
        ] {
            let mut generated = Generated::new();
            generated.body(source, body, "helper(invalid);\n", &locations);

            let translated = generated
                .translate(Span::new(Position::new(1, 9), Position::new(1, 9)))
                .expect("overlapping exact origins retain explicit specificity");
            assert_eq!(translated.source, source);
            assert_eq!(translated.span, inner);
        }
    }

    #[test]
    fn generated_eof_uses_explicit_final_segment_ownership() {
        let source = program::source::Id::from_index(4);
        let declaration = span(7, 4, 7, 11);
        let mut generated = Generated::new();
        generated.declaration(source, declaration, "fn broken(");

        let eof = generated
            .translate(Span::new(Position::new(1, 10), Position::new(1, 10)))
            .expect("generated EOF has explicit framing ownership");

        assert_eq!(eof.source, source);
        assert_eq!(eof.span, declaration);
    }

    #[test]
    fn generated_translation_rejects_missing_positions_without_fabricating_evidence() {
        let generated = Generated::new();

        assert_eq!(
            generated.translate(Span::NONE),
            Err(super::Error::MissingPosition)
        );
        assert_eq!(
            generated.translate(Span::new(Position::START, Position::START)),
            Err(super::Error::MissingOrigin(Span::new(
                Position::START,
                Position::START
            )))
        );
    }

    /// A column is 1-based, and a span that ends a line ends one past its
    /// last character, as table's `.style(if .. { text::success } ..)`
    /// does: that is the newline, not outside the text (#562).
    #[test]
    fn a_span_ending_a_line_ends_at_its_newline() {
        let source = "fn f() {\n    text::success\n}";
        let line = |column| super::byte_at(source, Position::new(2, column));
        assert_eq!(line(5), Some(13));
        assert_eq!(&source[13..line(18).unwrap()], "text::success");
        assert_eq!(line(18), Some(source.find("success\n").unwrap() + 7));
        assert_eq!(line(19), None);
        assert_eq!(line(0), Some(9));
        // The last line's end is the text's end.
        assert_eq!(
            super::byte_at(source, Position::new(3, 2)),
            Some(source.len())
        );
    }

    fn span(start_line: u16, start_column: u16, end_line: u16, end_column: u16) -> Span {
        Span::new(
            Position::new(start_line, start_column),
            Position::new(end_line, end_column),
        )
    }
}
