//! Position evidence retained while nominal callbacks are emitted.

use std::{cmp::Reverse, fmt, ops::Range};

use crate::{item, program, Position, Span};

#[derive(Clone)]
pub(super) struct Output {
    pub(super) generated: Range<usize>,
    pub(super) original: Span,
    pub(super) nesting: usize,
}

pub(super) struct Pending {
    pub(super) callable: item::Key,
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
    Source,
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

    pub(super) fn source(&mut self, source: program::source::Id, text: &str) {
        self.push(source, Class::Source, Priority::Owned, text);
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
                span: class.at_end(text),
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
                Class::Source => {
                    let start = position_at(
                        &self.text[origin.generated.clone()],
                        byte.checked_sub(origin.generated.start)?,
                    );
                    Span::new(start, start)
                }
            },
        })
    }
}

impl Class {
    fn ordering_span(&self) -> Span {
        match self {
            Self::Declaration(span) | Self::Body(span) | Self::Expression(span) => *span,
            Self::Source => Span::NONE,
        }
    }

    fn at_end(&self, text: &str) -> Span {
        match self {
            Self::Declaration(span) | Self::Body(span) | Self::Expression(span) => *span,
            Self::Source => {
                let end = position_at(text, text.len());
                Span::new(end, end)
            }
        }
    }
}

pub(super) fn byte_at(source: &str, position: Position) -> Option<usize> {
    let line = position.line()?;
    let column = position.position().unwrap_or(0);
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

fn position_at(source: &str, byte: usize) -> Position {
    let mut line = 1_u16;
    let mut column = 0_u16;
    for character in source[..byte].chars() {
        if character == '\n' {
            line = line.saturating_add(1);
            column = 0;
        } else {
            column = column.saturating_add(1);
        }
    }
    Position::new(line, column)
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
        generated.declaration(source, declaration, "private fn lowered() {\n");
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
    fn generated_source_identity_mapping_ignores_preceding_functions() {
        let source = program::source::Id::from_index(3);
        let mut plain = Generated::new();
        plain.source(source, "let broken = ;\n");
        let plain = plain
            .translate(Span::new(Position::new(1, 14), Position::new(1, 14)))
            .expect("plain generated position has exact source ownership");

        let mut prefixed = Generated::new();
        prefixed.declaration(
            program::source::Id::ROOT,
            span(1, 1, 1, 3),
            "fn prior() {}\n",
        );
        prefixed.source(source, "let broken = ;\n");
        let prefixed = prefixed
            .translate(Span::new(Position::new(2, 14), Position::new(2, 14)))
            .expect("prefixed generated position has exact source ownership");

        assert_eq!(plain.source, source);
        assert_eq!(prefixed.source, source);
        assert_eq!(plain.span, prefixed.span);
        assert_eq!(
            plain.span,
            Span::new(Position::new(1, 14), Position::new(1, 14))
        );
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

    fn span(start_line: u16, start_column: u16, end_line: u16, end_column: u16) -> Span {
        Span::new(
            Position::new(start_line, start_column),
            Position::new(end_line, end_column),
        )
    }
}
