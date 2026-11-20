//! Where rustc's E0004 suggests the arm that covers the values a
//! `match`'s arms miss: after the last arm, on a line of its own when the
//! arms are on lines of their own, or between braces of its own for a
//! `match` with no arms.

use crate::resolve::Arms;
use crate::{Position, Span};

use super::suggestion::{Edit, Style};
use super::{Error, Source, Suggestion};

/// rustc's suggestion of the arm `pattern => todo!()` after `arms`, in
/// `source`, under `message`.
pub(super) fn suggestion(
    arms: &Arms,
    pattern: &str,
    message: String,
    source: &Source,
) -> Result<Option<Suggestion>, Error> {
    let text = source.text();
    let arm = format!("{pattern} => todo!()");
    let (span, code) = match &*arms.arms {
        // The braces are written again, the arm inside them.
        [] => {
            let indent = indentation(text, arms.after_scrutinee.start());
            (
                arms.after_scrutinee,
                format!(" {{\n{indent}    {arm},\n{indent}}}"),
            )
        }
        // After the one arm, on a line of its own when a line ends after
        // it.
        [only] => {
            let after = slice(text, only.end(), arms.after_scrutinee.end());
            let trailing = after.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
            let multiline = slice(text, only.start(), only.end()).contains('\n')
                || after[..after.len() - trailing.len()].contains('\n');
            let spacing = if multiline {
                format!("\n{}", indentation(text, only.start()))
            } else {
                " ".to_owned()
            };
            (point(only.end()), format!(",{spacing}{arm}"))
        }
        // After the last arm, on a line of its own when it is on a line
        // after the arm before it.
        [.., previous, last] => {
            let spacing = if slice(text, previous.end(), last.start()).contains('\n') {
                format!("\n{}", indentation(text, last.start()))
            } else {
                " ".to_owned()
            };
            (point(last.end()), format!(",{spacing}{arm}"))
        }
    };
    let edit = Edit::new(source.clone(), span, code)?;
    Ok(Some(Suggestion::new(message, Style::Verbose, edit)))
}

/// An empty span at `at`, where an edit inserts.
const fn point(at: Position) -> Span {
    Span::new(at, at)
}

/// The text of `text` from `start` to `end`.
fn slice(text: &str, start: Position, end: Position) -> &str {
    let start = byte(text, start);
    text.get(start..byte(text, end).max(start))
        .unwrap_or_default()
}

/// The whitespace that starts the line `at` is on.
fn indentation(text: &str, at: Position) -> &str {
    let start: usize = text
        .split_inclusive('\n')
        .take(at.line().unwrap_or(1).saturating_sub(1))
        .map(str::len)
        .sum();
    let rest = text.get(start..).unwrap_or_default();
    let width = rest
        .find(|c: char| !c.is_whitespace() || c == '\n')
        .unwrap_or(rest.len());
    &rest[..width]
}

/// The byte offset of `at` in `text`.
fn byte(text: &str, at: Position) -> usize {
    super::render::offset(text, at.line().unwrap_or(1), at.position().unwrap_or(1))
}
