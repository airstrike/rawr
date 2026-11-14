//! Human rendering for canonical diagnostic reports, as rustc renders its
//! own: through `annotate-snippets`, the crate rustc renders with. Each
//! label is a marker under its source line, the primary one under the
//! `-->` arrow, and notes and help follow as `= note:` and `= help:`.

use annotate_snippets::{Annotation, AnnotationKind, Group, Level, Origin, Renderer, Snippet};

use super::{ExternalLabel, Label, LabelKind, Report, Severity, Source};

/// Whether a rendering carries a terminal's colours.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Color {
    /// Plain text, for a pipe, a file, a log or an agent.
    Never,
    /// rustc's colours, as ANSI escapes, for a terminal.
    Always,
}

/// Render one canonical report as plain text, for clipboards, durable logs
/// and agents.
#[must_use]
pub fn render(report: &Report) -> String {
    render_with(report, Color::Never)
}

/// Render one canonical report as rustc renders its diagnostics, in
/// rustc's colours when `color` is [`Color::Always`].
#[must_use]
pub fn render_with(report: &Report, color: Color) -> String {
    let renderer = match color {
        Color::Never => Renderer::plain(),
        Color::Always => Renderer::styled(),
    };
    // An external label's file is read here, as rustc reads a source it
    // points into, so its line shows under the label.
    let external: Vec<Option<String>> = report
        .external_labels()
        .iter()
        .map(|label| std::fs::read_to_string(label.position().file()).ok())
        .collect();
    let level = match report.severity() {
        Severity::Error => Level::ERROR,
        Severity::Warning => Level::WARNING,
    };
    let title = level.primary_title(report.message());
    let title = match report.code().rustc() {
        Some(code) => title.id(code),
        None => title,
    };
    let mut group = Group::with_title(title);
    for source in labelled(report) {
        let annotations = report
            .labels()
            .iter()
            .filter(|label| label.source() == source)
            .filter_map(annotation);
        group = group.element(
            Snippet::source(source.text())
                .path(shown(source.identity()))
                .annotations(annotations),
        );
    }
    let mut unread = Vec::new();
    for (label, text) in report.external_labels().iter().zip(&external) {
        match text
            .as_deref()
            .and_then(|text| external_snippet(label, text))
        {
            Some(snippet) => group = group.element(snippet),
            None => {
                let ((line, column), _) = label.position().positions();
                group = group.element(
                    Origin::path(shown(label.position().file()))
                        .line(line as usize)
                        .char_column(column as usize + 1),
                );
                unread.extend(label.message());
            }
        }
    }
    for source in report.sources().iter().filter(|source| {
        !report
            .labels()
            .iter()
            .any(|label| label.source() == *source)
    }) {
        group = group.element(Origin::path(shown(source.identity())));
    }
    if let Some(help) = report.help() {
        group = group.element(Level::HELP.message(help));
    }
    for note in report.notes().chain(unread) {
        group = group.element(Level::NOTE.message(note));
    }
    let mut output = renderer.render(&[group]);
    output.push('\n');
    output
}

/// `file` as rustc and cargo show a path: from the current directory when
/// it is under it.
fn shown(file: &str) -> std::borrow::Cow<'_, str> {
    let path = std::path::Path::new(file);
    let relative = std::env::current_dir()
        .ok()
        .and_then(|directory| Some(path.strip_prefix(directory).ok()?.to_str()?.to_owned()))
        .filter(|relative| !relative.is_empty());
    match relative {
        Some(relative) => relative.into(),
        None => file.into(),
    }
}

/// The sources the report's labels are in, the primary label's first, as
/// rustc puts the `-->` arrow at the primary span.
fn labelled(report: &Report) -> Vec<&Source> {
    let primary = report
        .labels()
        .iter()
        .filter(|label| label.kind() == LabelKind::Primary);
    let secondary = report
        .labels()
        .iter()
        .filter(|label| label.kind() == LabelKind::Secondary);
    let mut sources: Vec<&Source> = Vec::new();
    for label in primary.chain(secondary) {
        if !sources.contains(&label.source()) {
            sources.push(label.source());
        }
    }
    sources
}

/// The marker of `label` under its source line, at its byte range in its
/// source's text. A label without a line has no marker.
fn annotation(label: &Label) -> Option<Annotation<'_>> {
    let span = label.span();
    let text = label.source().text();
    let start = offset(
        text,
        span.start().line()?,
        span.start().position().unwrap_or(1),
    );
    let end = offset(text, span.end().line()?, span.end().position().unwrap_or(1)).max(start);
    let kind = match label.kind() {
        LabelKind::Primary => AnnotationKind::Primary,
        LabelKind::Secondary => AnnotationKind::Context,
    };
    Some(kind.span(start..end).label(label.message()))
}

/// The byte offset in `text` of the one-based `line` and character
/// `column`. A column past the line's end is its end.
fn offset(text: &str, line: usize, column: usize) -> usize {
    let start: usize = text
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum();
    let rest = text.get(start..).unwrap_or_default();
    let line = rest.split('\n').next().unwrap_or_default();
    let within = line
        .char_indices()
        .nth(column.saturating_sub(1))
        .map_or(line.len(), |(at, _)| at);
    start + within
}

/// An external label's marker under its line in `text`, the file it names
/// as read now: `None` when its byte range no longer falls in the file.
fn external_snippet<'text>(
    label: &'text ExternalLabel,
    text: &'text str,
) -> Option<Snippet<'text, Annotation<'text>>> {
    let bytes = label.position().byte_range();
    let range = bytes.start as usize..bytes.end as usize;
    text.get(range.clone())?;
    Some(
        Snippet::source(text)
            .path(shown(label.position().file()))
            .annotation(AnnotationKind::Context.span(range).label(label.message())),
    )
}

#[cfg(test)]
mod tests {
    use super::{render, render_with, Color};
    use crate::diagnostic::{code, ExternalPosition, Report, Source};
    use crate::{Position, Span};

    #[cfg(feature = "rust-import")]
    #[test]
    fn a_position_in_a_derives_expansion_is_reported_at_the_derive() {
        use crate::diagnostic::source;

        let item = Source::new("src/main.rs", "#[derive(Serialize)]\nstruct Task;\n").unwrap();
        let derive = Span::new(Position::new(1, 10), Position::new(1, 19));
        let expansion = Source::expansion(
            "target/rawr/expand/k.rs",
            "// derive serde::Serialize\nimpl Serialize for Task {}\n",
            source::Origin {
                source: item,
                span: derive,
                name: "Serialize".into(),
            },
        )
        .unwrap();
        let report = Report::error(code::RESOLVE_DUPLICATE_ITEM, "duplicate `Task`")
            .source(expansion.clone())
            .primary(
                expansion,
                Span::new(Position::new(2, 6), Position::new(2, 15)),
                None,
            )
            .unwrap()
            .finish();

        assert_eq!(
            render(&report),
            concat!(
                "error: duplicate `Task`\n",
                " --> src/main.rs:1:10\n",
                "  |\n",
                "1 | #[derive(Serialize)]\n",
                "  |          ^^^^^^^^^\n",
                "  |\n",
                "  = note: this error originates in the derive macro `Serialize`\n",
            )
        );
    }

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
                "error: duplicate `β`\n",
                " --> src/main.rs:1:2\n",
                "  |\n",
                "1 |   αβγ\n",
                "  |  __^\n",
                "2 | | line two\n",
                "  | |____^ duplicate spans two lines\n",
                "  |\n",
                " ::: src/other.rs:1:1\n",
                "  |\n",
                "1 |   first\n",
                "  |   ----- first declared here\n",
                "  |\n",
                "  = help: rename it\n",
                "  = note: characters, not bytes, determine columns\n",
            )
        );
    }

    /// A label in a file the report keeps no text of shows that file's
    /// line, read as rustc reads a source it points into.
    #[test]
    fn an_external_label_shows_its_source_line() {
        let directory = std::env::temp_dir().join(format!("rawr-render-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("lib.rs");
        std::fs::write(&file, "pub fn first() {}\npub fn second() {}\n").unwrap();
        let file = file.to_string_lossy().into_owned();
        let source = Source::new("src/main.rs", "fn main() { second(1) }\n").unwrap();
        let report = Report::error(
            code::RESOLVE_ARGUMENT_COUNT,
            "this function takes 0 arguments but 1 argument was supplied",
        )
        .primary(
            source,
            Span::new(Position::new(1, 13), Position::new(1, 19)),
            None,
        )
        .unwrap()
        .secondary_external(
            ExternalPosition::checked(file.as_str(), 25, 31, (2, 7), (2, 13)).unwrap(),
            Some("function defined here".into()),
        )
        .finish();
        let rendered = render(&report);
        std::fs::remove_dir_all(&directory).unwrap();

        assert_eq!(
            rendered,
            format!(
                concat!(
                    "error[E0061]: this function takes 0 arguments but 1 argument was supplied\n",
                    " --> src/main.rs:1:13\n",
                    "  |\n",
                    "1 | fn main() {{ second(1) }}\n",
                    "  |             ^^^^^^\n",
                    "  |\n",
                    " ::: {file}:2:8\n",
                    "  |\n",
                    "2 | pub fn second() {{}}\n",
                    "  |        ------ function defined here\n",
                ),
                file = file
            )
        );
    }

    #[test]
    fn a_coloured_rendering_is_the_plain_one_with_escapes() {
        let source = Source::new("src/main.rs", "let x = 1;\n").unwrap();
        let report = Report::error(code::RESOLVE_TYPE_MISMATCH, "mismatched types")
            .primary(
                source,
                Span::new(Position::new(1, 9), Position::new(1, 10)),
                Some("expected `bool`".into()),
            )
            .unwrap()
            .finish();
        let coloured = render_with(&report, Color::Always);

        assert!(coloured.contains("\u{1b}["), "{coloured}");
        assert_eq!(plain(&coloured), render(&report));
    }

    /// `text` without its SGR escapes.
    fn plain(text: &str) -> String {
        let mut plain = String::new();
        let mut rest = text;
        while let Some(at) = rest.find('\u{1b}') {
            plain.push_str(&rest[..at]);
            let end = rest[at..].find('m').map_or(rest.len(), |end| at + end + 1);
            rest = &rest[end..];
        }
        plain.push_str(rest);
        plain
    }
}
