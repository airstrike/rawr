//! Where a string literal's value is in its source: a diagnostic about
//! `{name}` in a `format!` string points at `name` inside the literal, as
//! rustc's does.

use std::ops::Range;

use crate::{Position, Span};

/// The source span of the characters `range` (char indices) of `value`,
/// in the literal `written` starts with, which is at `at`. None when
/// `written` doesn't start with a string literal whose value is `value`,
/// as for a literal a macro wrote.
pub(super) fn value_span(
    written: &str,
    at: Position,
    value: &str,
    range: Range<usize>,
) -> Option<Span> {
    let mut source = written.chars().peekable();
    let mut position = at;
    // `r#"..."#` has no escapes and ends at `"` and its hashes.
    let raw = source.next_if_eq(&'r').is_some();
    if raw {
        step('r', &mut position);
    }
    let mut hashes = 0;
    while raw && source.next_if_eq(&'#').is_some() {
        step('#', &mut position);
        hashes += 1;
    }
    if source.next()? != '"' {
        return None;
    }
    step('"', &mut position);
    // Where each character of the value starts and ends.
    let mut spans = Vec::with_capacity(value.len());
    let mut decoded = value.chars();
    loop {
        let start = position;
        let character = source.next()?;
        step(character, &mut position);
        let produced = match character {
            '"' if !raw => break,
            // A `"` with fewer hashes than the opening is the value's own,
            // as are those hashes.
            '"' => {
                let mut closing = 0;
                while closing < hashes && source.next_if_eq(&'#').is_some() {
                    step('#', &mut position);
                    closing += 1;
                }
                if closing == hashes {
                    break;
                }
                if decoded.next()? != '"' {
                    return None;
                }
                spans.push((start, text_end(start, 1)));
                for hash in 0..closing {
                    if decoded.next()? != '#' {
                        return None;
                    }
                    let at = text_end(start, 1 + hash);
                    spans.push((at, text_end(at, 1)));
                }
                continue;
            }
            '\\' if !raw => {
                let escape = source.next()?;
                step(escape, &mut position);
                match escape {
                    'n' => Some('\n'),
                    'r' => Some('\r'),
                    't' => Some('\t'),
                    '0' => Some('\0'),
                    '\\' | '\'' | '"' => Some(escape),
                    'x' => {
                        let mut code = 0;
                        for _ in 0..2 {
                            let digit = source.next()?;
                            step(digit, &mut position);
                            code = code * 16 + digit.to_digit(16)?;
                        }
                        char::from_u32(code)
                    }
                    'u' => {
                        if source.next()? != '{' {
                            return None;
                        }
                        step('{', &mut position);
                        let mut code = 0;
                        loop {
                            let digit = source.next()?;
                            step(digit, &mut position);
                            match digit {
                                '}' => break,
                                '_' => {}
                                digit => code = code * 16 + digit.to_digit(16)?,
                            }
                        }
                        Some(char::from_u32(code)?)
                    }
                    // A line ending continues the string past the next
                    // line's leading whitespace.
                    '\n' => {
                        while let Some(blank) = source.next_if(|next| next.is_whitespace()) {
                            step(blank, &mut position);
                        }
                        None
                    }
                    _ => return None,
                }
            }
            character => Some(character),
        };
        if let Some(produced) = produced {
            if decoded.next()? != produced {
                return None;
            }
            spans.push((start, position));
        }
    }
    if decoded.next().is_some() {
        return None;
    }
    let (start, _) = *spans.get(range.start)?;
    let (_, end) = *spans.get(range.end.checked_sub(1)?)?;
    Some(Span::new(start, end))
}

/// Moves `position` past `character`.
fn step(character: char, position: &mut Position) {
    if character == '\n' {
        position.new_line();
    }
    position.advance();
}

/// `count` characters on from `at`, on its line.
fn text_end(mut at: Position, count: usize) -> Position {
    for _ in 0..count {
        at.advance();
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns(written: &str, value: &str, range: Range<usize>) -> Option<(usize, usize)> {
        let span = value_span(written, Position::new(1, 5), value, range)?;
        Some((span.start().position()?, span.end().position()?))
    }

    #[test]
    fn a_plain_literal_maps_one_to_one() {
        assert_eq!(columns("\"{words:?}\");", "{words:?}", 1..6), Some((7, 12)));
    }

    #[test]
    fn escapes_shift_what_follows() {
        assert_eq!(
            columns("\"\\\"{x}\\u{41}{y}\"", "\"{x}A{y}", 6..7),
            Some((18, 19))
        );
    }

    #[test]
    fn a_raw_literal_ends_at_its_hashes() {
        assert_eq!(columns("r#\"a\"{b}\"#", "a\"{b}", 3..4), Some((11, 12)));
    }

    #[test]
    fn a_literal_of_another_value_has_no_span() {
        assert_eq!(columns("\"{x}\"", "{y}", 1..2), None);
        assert_eq!(columns("name", "name", 0..1), None);
    }
}
