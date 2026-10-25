//! A report on a wrapped program, as the snippet shows it.

use crate::{without_tests, SOURCE_NAME};

/// A rendered report as the user's own source shows it: what
/// [`wrap`](crate::wrap) added to a line (the imports and `#[preview]` on
/// the first) is cut from the quoted line, its marker and its columns. A
/// position on a line the wrapping added after the source moves to the end
/// of the source's last line.
pub fn report(text: &str, program: &str, source: &str) -> String {
    let source = without_tests(source);
    let own: Vec<&str> = source.split('\n').collect();
    // The last line with text: a source ending in a newline ends on it.
    let last = own
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map_or(1, |at| at + 1);
    let end = own[last - 1].chars().count();
    let added = program
        .split('\n')
        .zip(source.split('\n'))
        .enumerate()
        .filter_map(|(index, (wrapped, own))| {
            added_span(wrapped, own).map(|span| (index + 1, span))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut out = String::with_capacity(text.len());
    // The quoted line's added span, and whether it was moved to `last`.
    let mut quoted = None;
    let mut moved = false;
    for line in text.lines() {
        let rewritten = if let Some(location) = line
            .strip_prefix(" --> ")
            .or_else(|| line.strip_prefix(" ::: "))
            .and_then(|rest| rest.strip_prefix(SOURCE_NAME))
            .and_then(|rest| rest.strip_prefix(':'))
        {
            quoted = None;
            moved = false;
            let arrow = &line[..5];
            let location = shift_location(location, &added, last, end);
            format!("{arrow}{SOURCE_NAME}:{location}")
        } else if let Some((number, rest)) = quoted_line(line) {
            let gutter = &line[..line.len() - rest.len()];
            moved = number > last;
            quoted = if moved {
                None
            } else {
                added.get(&number).copied()
            };
            if moved {
                let width = gutter.len() - 3;
                format!("{last:>width$} | {}", own[last - 1])
            } else {
                match quoted {
                    Some(span) => format!("{gutter}{}", cut(rest, span)),
                    None => line.to_owned(),
                }
            }
        } else if let Some(at) = line.find('|')
            && line[..at].trim().is_empty()
            && (moved || quoted.is_some())
        {
            let marker = line.get(at + 2..).unwrap_or_default();
            let rest = if std::mem::take(&mut moved) {
                format!("{}^", " ".repeat(end))
            } else {
                cut(marker, quoted.take().expect("a quoted span"))
            };
            format!("{}{rest}", &line[..at + 2])
        } else {
            without_positions(line)
        };
        out.push_str(&rewritten);
        out.push('\n');
    }
    out
}

/// `line` without a runtime error's ` (line 2, position 1)`: it counts in
/// the wrapped program, and the ` --> ` line shows where it is.
pub fn without_positions(line: &str) -> String {
    let mut out = line.to_owned();
    while let Some(at) = out.find(" (line ") {
        let rest = &out[at + " (line ".len()..];
        let digits =
            |text: &str| text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let line_digits = digits(rest);
        let Some(after) = rest[line_digits..].strip_prefix(", position ") else {
            break;
        };
        let position_digits = digits(after);
        if line_digits == 0 || position_digits == 0 || !after[position_digits..].starts_with(')') {
            break;
        }
        let end = out.len() - after.len() + position_digits + 1;
        out.replace_range(at..end, "");
    }
    out
}

/// Whether `line` and `column` (1-based, in `program`) fall in text the
/// user wrote. Text that [`wrap`](crate::wrap) or a plain program's wrapping added is
/// not theirs.
pub fn in_source(program: &str, source: &str, line: usize, column: usize) -> bool {
    let source = without_tests(source);
    let Some(index) = line.checked_sub(1) else {
        return false;
    };
    let (Some(wrapped), Some(own)) = (
        program.split('\n').nth(index),
        source.split('\n').nth(index),
    ) else {
        return false;
    };
    added_span(wrapped, own)
        .is_none_or(|(start, length)| column <= start || column > start + length)
}

/// Where `wrapped` adds text to `own`: the char column it starts at and its
/// length in chars.
fn added_span(wrapped: &str, own: &str) -> Option<(usize, usize)> {
    let wrapped: Vec<char> = wrapped.chars().collect();
    let own: Vec<char> = own.chars().collect();
    if wrapped.len() <= own.len() {
        return None;
    }
    let start = wrapped.iter().zip(&own).take_while(|(a, b)| a == b).count();
    let length = wrapped.len() - own.len();
    (wrapped[start + length..] == own[start..]).then_some((start, length))
}

/// ` 12 | text`: the quoted line's number and text.
fn quoted_line(line: &str) -> Option<(usize, &str)> {
    let (gutter, rest) = line.split_once(" | ")?;
    let number = gutter.trim().parse().ok()?;
    Some((number, rest))
}

fn cut(text: &str, (start, length): (usize, usize)) -> String {
    text.chars()
        .enumerate()
        .filter(|(index, _)| *index < start || *index >= start + length)
        .map(|(_, c)| c)
        .collect()
}

/// `1:310-1:324` with the added columns taken out, and a line past `last`
/// moved to its end, column `end + 1`.
fn shift_location(
    location: &str,
    added: &std::collections::BTreeMap<usize, (usize, usize)>,
    last: usize,
    end: usize,
) -> String {
    location
        .split('-')
        .map(|point| {
            let Some((line, column)) = point.split_once(':') else {
                return point.to_owned();
            };
            let (Ok(number), Ok(column)) = (line.parse::<usize>(), column.parse::<usize>()) else {
                return point.to_owned();
            };
            if number > last {
                return format!("{last}:{}", end + 1);
            }
            let column = match added.get(&number) {
                Some(&(start, length)) if column > start + length => column - length,
                Some(&(start, _)) if column > start => start + 1,
                _ => column,
            };
            format!("{number}:{column}")
        })
        .collect::<Vec<_>>()
        .join("-")
}
