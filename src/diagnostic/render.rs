//! Concise human rendering for canonical diagnostic reports.

use std::fmt::Write as _;

use super::{Label, LabelKind, Report};

/// Render one canonical report for terminals, clipboards, and durable text logs.
#[must_use]
pub fn render(report: &Report) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "{}[{}]: {}",
        report.severity(),
        report.code(),
        report.message()
    );
    for source in report.sources().iter().filter(|source| {
        !report
            .labels()
            .iter()
            .any(|label| label.source() == *source)
    }) {
        let _ = writeln!(output, " --> {}", source.identity());
    }
    for label in report.labels() {
        render_label(&mut output, label);
    }
    for label in report.external_labels() {
        let ((start_line, start_column), (end_line, end_column)) = label.position().positions();
        let bytes = label.position().byte_range();
        let _ = write!(
            output,
            " ::: {}:{start_line}:{start_column}",
            label.position().file()
        );
        if start_line != end_line || start_column != end_column {
            let _ = write!(output, "-{end_line}:{end_column}");
        }
        let _ = write!(output, " [bytes {}..{}]", bytes.start, bytes.end);
        if let Some(message) = label.message() {
            let _ = write!(output, " {message}");
        }
        let _ = writeln!(output);
    }
    if let Some(help) = report.help() {
        let _ = writeln!(output, "  = help: {help}");
    }
    for note in report.notes() {
        let _ = writeln!(output, "  = note: {note}");
    }
    output
}

fn render_label(output: &mut String, label: &Label) {
    let span = label.span();
    let start = span.start();
    let end = span.end();
    let (Some(start_line), Some(end_line)) = (start.line(), end.line()) else {
        return;
    };
    let start_column = start.position().unwrap_or(1);
    let end_column = end.position().unwrap_or(1);
    let arrow = match label.kind() {
        LabelKind::Primary => "-->",
        LabelKind::Secondary => ":::",
    };
    let _ = write!(
        output,
        " {arrow} {}:{start_line}:{start_column}",
        label.source().identity()
    );
    if start_line != end_line || start_column != end_column {
        let _ = write!(output, "-{end_line}:{end_column}");
    }
    let _ = writeln!(output);

    let width = decimal_width(start_line.max(end_line));
    if let Some(line) = source_line(label, start_line) {
        render_source_line(output, width, start_line, line);
        let prefix = marker_prefix(line, start_column);
        let count = if start_line == end_line {
            end_column.saturating_sub(start_column).max(1)
        } else {
            line.chars().count().saturating_sub(start_column - 1).max(1)
        };
        render_marker(output, width, &prefix, count, label, true);
    }

    if end_line != start_line {
        let _ = writeln!(output, " {:width$} | ...", "", width = width);
        if let Some(line) = source_line(label, end_line).filter(|_| end_column > 1) {
            render_source_line(output, width, end_line, line);
            render_marker(output, width, "", end_column - 1, label, false);
        }
    }
}

fn source_line(label: &Label, line: usize) -> Option<&str> {
    label
        .source()
        .text()
        .split('\n')
        .nth(line.saturating_sub(1))
}

fn render_source_line(output: &mut String, width: usize, number: usize, line: &str) {
    let _ = writeln!(
        output,
        " {number:>width$} | {line}",
        number = number,
        width = width
    );
}

fn render_marker(
    output: &mut String,
    width: usize,
    prefix: &str,
    count: usize,
    label: &Label,
    show_message: bool,
) {
    let marker = match label.kind() {
        LabelKind::Primary => '^',
        LabelKind::Secondary => '-',
    };
    let markers: String = std::iter::repeat_n(marker, count).collect();
    let _ = write!(output, " {:width$} | {prefix}{markers}", "", width = width);
    if let Some(message) = label.message().filter(|_| show_message) {
        let _ = write!(output, " {message}");
    }
    let _ = writeln!(output);
}

fn marker_prefix(line: &str, one_based_column: usize) -> String {
    line.chars()
        .take(one_based_column.saturating_sub(1))
        .map(|character| if character == '\t' { '\t' } else { ' ' })
        .collect()
}

const fn decimal_width(mut value: usize) -> usize {
    let mut width = 1;
    while value >= 10 {
        value /= 10;
        width += 1;
    }
    width
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::diagnostic::{code, Report, Source};
    use crate::{Position, Span};

    #[test]
    fn renderer_handles_unicode_multiline_and_cross_source_labels() {
        let primary = Source::new("src/main.rs", "αβγ\nline two\n").unwrap();
        let secondary = Source::new("src/other.rs", "first\n").unwrap();
        let report = Report::error(code::RESOLVE_DUPLICATE_ITEM, "duplicate `β`")
            .primary(
                primary,
                Span::new(Position::new(1, 2), Position::new(2, 5)),
                Some("duplicate spans two lines".into()),
            )
            .unwrap()
            .secondary(
                secondary,
                Span::new(Position::new(1, 1), Position::new(1, 6)),
                Some("first declared here".into()),
            )
            .unwrap()
            .help("rename it")
            .note("characters, not bytes, determine columns")
            .finish();

        assert_eq!(
            render(&report),
            concat!(
                "error[RAWR-RESOLVE-DUPLICATE-ITEM]: duplicate `β`\n",
                " --> src/main.rs:1:2-2:5\n",
                " 1 | αβγ\n",
                "   |  ^^ duplicate spans two lines\n",
                "   | ...\n",
                " 2 | line two\n",
                "   | ^^^^\n",
                " ::: src/other.rs:1:1-1:6\n",
                " 1 | first\n",
                "   | ----- first declared here\n",
                "  = help: rename it\n",
                "  = note: characters, not bytes, determine columns\n",
            )
        );
    }
}
