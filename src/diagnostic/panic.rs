//! A run-time failure as a program built by Cargo prints its panic.

use super::{code, LabelKind, Report};
use crate::Span;

/// The line Rust prints after a panic's message.
pub const BACKTRACE_NOTE: &str =
    "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace";

/// `report`, a run-time failure, as a program built by Cargo prints its
/// panic on stderr: `thread 'main' panicked at <file>:<line>:<column>:`,
/// the message, and Rust's note on backtraces.
///
/// The file is the source identity of the report's primary label. The line
/// and column are where rustc places a panic: the name of a method call, the
/// `[` of an index, else the start of the expression. A report without a
/// position names its source alone.
#[must_use]
pub fn panic(report: &Report) -> String {
    let location = report
        .labels()
        .iter()
        .find(|label| label.kind() == LabelKind::Primary)
        .and_then(|label| {
            let (line, column) = caller(label.source().text(), label.span())?;
            Some(format!("{}:{line}:{column}", label.source().identity()))
        })
        .or_else(|| {
            report
                .sources()
                .first()
                .map(|source| source.identity().to_owned())
        });
    let at = location.map_or_else(String::new, |location| format!(" at {location}"));
    format!(
        "thread 'main' panicked{at}:\n{}\n{BACKTRACE_NOTE}\n",
        message(report)
    )
}

/// The panic's message: a `panic!`'s text, or an evaluation error's text
/// without its position in the lowered program.
fn message(report: &Report) -> String {
    let text = report.message();
    if report.code() == code::RUNTIME_THROW {
        return match text.strip_prefix("runtime error: ") {
            Some(message) => message.to_owned(),
            None => "explicit panic".to_owned(),
        };
    }
    let mut text = text.to_owned();
    // ` (line 2, position 5)` at the end counts in the lowered program.
    let Some(at) = text.rfind(" (line ").filter(|_| text.ends_with(')')) else {
        return text;
    };
    let inside = &text[at + " (line ".len()..text.len() - 1];
    let numbers = inside
        .split_once(", position ")
        .is_some_and(|(line, column)| {
            [line, column]
                .iter()
                .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        });
    if numbers {
        text.truncate(at);
    }
    text
}

/// The line and column rustc gives a panic in the expression `span` of
/// `text`: the name of a method call that ends the expression, the `[` of an
/// index that ends it, else its start.
fn caller(text: &str, span: Span) -> Option<(usize, usize)> {
    let start = span.start();
    let (line, column) = (start.line()?, start.position()?);
    let end = span.end();
    let chars = expression(text, (line, column), (end.line(), end.position()));
    Some(close(&chars).unwrap_or((line, column)))
}

/// The characters of `text` from `start` through `end`, the start of the
/// expression's last token, each with its line and column. The last token
/// may be `()`, whose `)` is past `end`.
fn expression(
    text: &str,
    start: (usize, usize),
    end: (Option<usize>, Option<usize>),
) -> Vec<(char, usize, usize)> {
    let end = match end {
        (Some(line), Some(column)) if (line, column) >= start => (line, column),
        _ => return Vec::new(),
    };
    text.split('\n')
        .enumerate()
        .skip(start.0 - 1)
        .take(end.0 + 1 - start.0)
        .flat_map(|(index, line)| {
            // Each line ends in its `\n`, which ends a `//` comment.
            line.chars()
                .chain(std::iter::once('\n'))
                .enumerate()
                .map(move |(at, c)| (c, index + 1, at + 1))
        })
        .scan(None, |previous, (c, line, column)| {
            let unit = *previous == Some(((end.0, end.1), '(')) && c == ')';
            *previous = Some(((line, column), c));
            Some((c, line, column, unit))
        })
        .filter(|&(_, line, column, unit)| {
            (line, column) >= start && ((line, column) <= end || unit)
        })
        .map(|(c, line, column, _)| (c, line, column))
        .collect()
}

/// Where rustc places a panic in an expression that ends in a call or an
/// index: the method's name, or the `[`. `None` for any other expression.
fn close(chars: &[(char, usize, usize)]) -> Option<(usize, usize)> {
    let last = chars.iter().rposition(|(c, ..)| !c.is_whitespace())?;
    // The opening bracket of the last group at depth 0, and where it closed.
    let mut depth = 0usize;
    let mut open = None;
    let mut closed = None;
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at].0;
        match c {
            '"' => at = string_end(chars, at),
            '\'' => at = char_end(chars, at),
            '/' if chars.get(at + 1).is_some_and(|next| next.0 == '/') => {
                while at < chars.len() && chars[at].0 != '\n' {
                    at += 1;
                }
            }
            '(' | '[' | '{' => {
                if depth == 0 {
                    open = Some(at);
                }
                depth += 1;
            }
            ')' | ']' | '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    closed = Some(at);
                }
            }
            _ => {}
        }
        at += 1;
    }
    let open = open?;
    if closed != Some(last) || last < open {
        return None;
    }
    let before = |mut at: usize| {
        while at > 0 && chars[at - 1].0.is_whitespace() {
            at -= 1;
        }
        at.checked_sub(1)
    };
    match chars[open].0 {
        // `value[index]`, not `vec![..]` or a literal.
        '[' => {
            let previous = before(open)?;
            let c = chars[previous].0;
            (c.is_alphanumeric() || matches!(c, '_' | ')' | ']'))
                .then_some((chars[open].1, chars[open].2))
        }
        // `receiver.name(..)` or `receiver.name::<T>(..)`.
        '(' => {
            let mut end = before(open)?;
            if chars[end].0 == '>' {
                let mut angle = 0usize;
                loop {
                    match chars[end].0 {
                        '>' => angle += 1,
                        '<' => angle -= 1,
                        _ => {}
                    }
                    if angle == 0 {
                        break;
                    }
                    end = end.checked_sub(1)?;
                }
                end = end.checked_sub(3)?;
                if chars[end + 1].0 != ':' || chars[end + 2].0 != ':' {
                    return None;
                }
            }
            let ident = |c: char| c.is_alphanumeric() || c == '_';
            if !ident(chars[end].0) {
                return None;
            }
            let mut name = end;
            while name > 0 && ident(chars[name - 1].0) {
                name -= 1;
            }
            let dot = before(name)?;
            (chars[dot].0 == '.').then_some((chars[name].1, chars[name].2))
        }
        _ => None,
    }
}

/// The index of the `"` closing the string literal opened at `at`, a raw
/// string's `"#` included.
fn string_end(chars: &[(char, usize, usize)], at: usize) -> usize {
    let mut hashes = 0;
    let mut back = at;
    while back > 0 && chars[back - 1].0 == '#' {
        hashes += 1;
        back -= 1;
    }
    let raw = back > 0 && chars[back - 1].0 == 'r';
    let mut next = at + 1;
    while next < chars.len() {
        match chars[next].0 {
            '\\' if !raw => next += 1,
            '"' if (1..=hashes).all(|n| chars.get(next + n).is_some_and(|c| c.0 == '#')) => {
                return next + hashes;
            }
            _ => {}
        }
        next += 1;
    }
    chars.len()
}

/// The index of the `'` closing the char literal opened at `at`, or `at`
/// itself for a lifetime.
fn char_end(chars: &[(char, usize, usize)], at: usize) -> usize {
    match chars.get(at + 1).map(|c| c.0) {
        Some('\\') => (at + 2..chars.len())
            .find(|&next| chars[next].0 == '\'')
            .unwrap_or(chars.len()),
        Some(_) if chars.get(at + 2).is_some_and(|c| c.0 == '\'') => at + 2,
        _ => at,
    }
}

#[cfg(test)]
mod tests {
    use super::{panic, BACKTRACE_NOTE};
    use crate::diagnostic::{code, Report, Source};
    use crate::{Position, Span};

    fn at(text: &str, line: u16, start: u16, end: u16) -> String {
        let source = Source::new("src/main.rs", text).unwrap();
        let span = Span::new(Position::new(line, start), Position::new(line, end));
        let report = Report::error(code::RUNTIME_THROW, "runtime error: boom")
            .primary(source, span, None)
            .unwrap()
            .finish();
        panic(&report)
    }

    #[test]
    fn a_panic_reads_as_rust_prints_it() {
        let text = "fn main() {\n    panic!(\"boom\");\n}\n";
        assert_eq!(
            at(text, 2, 5, 18),
            format!("thread 'main' panicked at src/main.rs:2:5:\nboom\n{BACKTRACE_NOTE}\n")
        );
    }

    #[test]
    fn a_method_call_panics_at_its_name() {
        let text = "fn main() {\n    r.expect(\"x (y\");\n    v.iter().map(|x| x).unwrap::<i32>();\n    values[ i ];\n    foo(x);\n    a.len() / 0;\n    o.unwrap();\n}\n";
        assert!(at(text, 2, 5, 20).contains("src/main.rs:2:7:"));
        assert!(at(text, 3, 5, 38).contains("src/main.rs:3:25:"));
        assert!(at(text, 4, 5, 15).contains("src/main.rs:4:11:"));
        assert!(at(text, 5, 5, 10).contains("src/main.rs:5:5:"));
        assert!(at(text, 6, 5, 15).contains("src/main.rs:6:5:"));
        assert!(at(text, 7, 5, 13).contains("src/main.rs:7:7:"));
    }

    #[test]
    fn an_evaluation_error_loses_its_lowered_position() {
        let report = Report::error(
            code::RUNTIME_EVALUATION,
            "index out of bounds (line 2, position 5)",
        )
        .source(Source::new("src/main.rs", "").unwrap())
        .finish();
        assert_eq!(
            panic(&report),
            format!(
                "thread 'main' panicked at src/main.rs:\nindex out of bounds\n{BACKTRACE_NOTE}\n"
            )
        );
    }
}
