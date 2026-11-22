//! Where rustc shows an item a note points at: from its visibility or
//! first keyword through its signature, the part of its header before its
//! body (rustc's `def_span`).

use crate::{Position, Span};

use super::Source;

/// The keywords that introduce an item the program names.
const ITEMS: [&str; 7] = ["fn", "struct", "enum", "union", "mod", "trait", "type"];

/// The qualifiers that may stand before an item's keyword, after its
/// visibility.
const QUALIFIERS: [&str; 5] = ["const", "async", "unsafe", "safe", "default"];

/// The item `name` names in `source`, as rustc shows it in a note at its
/// definition: `pub fn f(a: i32) -> i32`, `struct P<T> where T: Copy`,
/// `mod n`. A name no item keyword stands before is shown alone.
pub(super) fn span(source: &Source, name: Span) -> Span {
    let text = source.text();
    let (Some(line), Some(column)) = (name.start().line(), name.start().position()) else {
        return name;
    };
    let Some(written) = text.split('\n').nth(line.saturating_sub(1)) else {
        return name;
    };
    let before: String = written.chars().take(column.saturating_sub(1)).collect();
    let Some(start) = item_start(&before) else {
        return name;
    };
    let start = Position::new(
        u16::try_from(line).unwrap_or(u16::MAX),
        u16::try_from(start + 1).unwrap_or(u16::MAX),
    );
    match signature_end(text, name.end()) {
        Some(end) => Span::new(start, end),
        None => Span::new(start, name.end()),
    }
}

/// What rustc calls the item `name` names in `source`, by the keyword
/// before it: `module` of `mod inner`.
pub(super) fn kind(source: &Source, name: Span) -> &'static str {
    let text = source.text();
    let before: Option<String> =
        name.start()
            .line()
            .zip(name.start().position())
            .and_then(|(line, column)| {
                let written = text.split('\n').nth(line.saturating_sub(1))?;
                Some(written.chars().take(column.saturating_sub(1)).collect())
            });
    let kinds = [
        ("fn", "function"),
        ("struct", "struct"),
        ("enum", "enum"),
        ("union", "union"),
        ("mod", "module"),
        ("trait", "trait"),
        ("type", "type alias"),
        ("const", "constant"),
        ("static", "static"),
        ("macro_rules!", "macro"),
    ];
    before
        .and_then(|before| {
            kinds
                .iter()
                .find(|(keyword, _)| word_before(&before, keyword).is_some())
                .map(|(_, kind)| *kind)
        })
        .unwrap_or("item")
}

/// The type `name` names in `source` from its visibility or keyword
/// through its name and generics, as rustc marks a type that "must
/// implement" a trait: `pub struct Value<T>`, without its fields.
pub(super) fn header(source: &Source, name: Span) -> Span {
    let whole = span(source, name);
    if whole == name {
        return name;
    }
    let after = Span::new(name.end(), whole.end()).text(source.text());
    let generics = match after.strip_prefix('<') {
        Some(rest) => {
            let mut depth = 1_usize;
            let close = rest.char_indices().find_map(|(index, character)| {
                match character {
                    '<' => depth += 1,
                    '>' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(index)
            });
            close.map_or(0, |index| rest[..=index].chars().count() + 1)
        }
        None => 0,
    };
    let mut end = name.end();
    for _ in 0..generics {
        end.advance();
    }
    Span::new(whole.start(), end)
}

/// The column, as a count of characters, where the item whose name
/// follows `before` starts: its visibility, else its first qualifier, else
/// its keyword.
fn item_start(before: &str) -> Option<usize> {
    let mut rest = before.trim_end();
    rest = ITEMS
        .iter()
        .find_map(|keyword| word_before(rest, keyword))?;
    while let Some(stripped) = QUALIFIERS
        .iter()
        .find_map(|qualifier| word_before(rest, qualifier))
        .or_else(|| extern_before(rest))
    {
        rest = stripped;
    }
    if let Some(stripped) = visibility_before(rest) {
        rest = stripped;
    }
    let rest = rest.trim_end();
    let skipped = before[rest.len()..]
        .chars()
        .take_while(|character| character.is_whitespace())
        .count();
    Some(rest.chars().count() + skipped)
}

/// `text` without `word`, the whole word at its end, and the spaces
/// before it.
fn word_before<'text>(text: &'text str, word: &str) -> Option<&'text str> {
    let rest = text.trim_end().strip_suffix(word)?;
    let bounded = rest
        .chars()
        .next_back()
        .is_none_or(|character| !(character.is_alphanumeric() || character == '_'));
    bounded.then_some(rest)
}

/// `text` without the `extern` or `extern "abi"` at its end.
fn extern_before(text: &str) -> Option<&str> {
    let text = text.trim_end();
    let text = match text.strip_suffix('"') {
        Some(abi) => abi.rfind('"').map(|open| &abi[..open])?,
        None => text,
    };
    word_before(text, "extern")
}

/// `text` without the `pub` or `pub(...)` at its end.
fn visibility_before(text: &str) -> Option<&str> {
    let text = text.trim_end();
    match text.strip_suffix(')') {
        Some(inner) => word_before(&inner[..inner.rfind('(')?], "pub"),
        None => word_before(text, "pub"),
    }
}

/// Where an item's signature ends, its name ending at `after`: before the
/// `{` that opens its body, or at the `;` that ends it, which rustc's span
/// keeps after a tuple struct's fields or a type alias's type and leaves
/// out after a unit struct's or a module's name. `None` when the text ends
/// first.
fn signature_end(text: &str, after: Position) -> Option<Position> {
    let mut line = after.line()?;
    let mut column = after.position()?;
    let lines: Vec<&str> = text.split('\n').collect();
    let mut depth = 0_usize;
    // The end of the last character that is no space, and that character.
    let mut last = ((line, column), ' ');
    // Whether a type alias's `=` came first.
    let mut aliased = false;
    loop {
        let written = lines.get(line.saturating_sub(1))?;
        for (index, character) in written.chars().enumerate().skip(column.saturating_sub(1)) {
            match character {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                '=' if depth == 0 => aliased = true,
                '{' if depth == 0 => return position(last.0),
                ';' if depth == 0 => {
                    return if aliased || last.1 == ')' {
                        position((line, index + 2))
                    } else {
                        position(last.0)
                    };
                }
                _ => {}
            }
            if !character.is_whitespace() {
                last = ((line, index + 2), character);
            }
        }
        line += 1;
        column = 1;
    }
}

/// The position of a one-based line and character column.
fn position((line, column): (usize, usize)) -> Option<Position> {
    Some(Position::new(
        u16::try_from(line).ok()?,
        u16::try_from(column).ok()?,
    ))
}
