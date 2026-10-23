//! The crate's `Cargo.toml`, read for its package and dependencies.

use std::ops::Range;

use toml_edit::Item;

use crate::{ImmutableString, Position, Span};

pub(crate) mod cargo;
pub(crate) mod rust;
pub(crate) mod workspace;

#[derive(Debug)]
pub(crate) struct Error {
    pub(crate) message: ImmutableString,
    pub(crate) span: Span,
}

fn item_span(text: &str, item: &Item) -> Span {
    item.span()
        .map(|range| byte_span(text, range))
        .unwrap_or_else(|| Span::new(Position::START, Position::START))
}

pub(super) fn byte_span(text: &str, range: Range<usize>) -> Span {
    Span::new(
        byte_position(text, range.start),
        byte_position(text, range.end),
    )
}

fn byte_position(text: &str, byte: usize) -> Position {
    let mut line = 1_u16;
    let mut column = 0_u16;
    for character in text[..byte.min(text.len())].chars() {
        if character == '\n' {
            line = line.saturating_add(1);
            column = 0;
        } else {
            column = column.saturating_add(1);
        }
    }
    Position::new(line, column.saturating_add(1))
}

fn valid_package_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

fn valid_alias(alias: &str) -> bool {
    crate::tokenizer::is_valid_identifier(alias)
        && crate::tokenizer::Token::lookup_symbol_from_syntax(alias).is_none()
        && alias != "crate"
        && alias != "super"
}
