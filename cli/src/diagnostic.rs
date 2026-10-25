//! Diagnostics for people and for agents: the concise rendering of a
//! report, and the same report as data, one object per position, which
//! `rawr mcp` returns.
//!
//! A diagnostic as data has the absolute `file`, the one-based `line` and
//! `column`, `end_line` and `end_column`, the `code`, the `severity`, the
//! `message` without its position, `help`, and where it came from
//! (`rawr` or `rustc`). One from rustc may carry a `suggestion`: a span
//! and its replacement.

use std::io::Write as _;
use std::path::Path;

use rawr::diagnostic::Report;
use serde_json::{json, Value};

/// The file a `rawr check` started by `rawr mcp` records its diagnostics
/// in, one JSON object per line, beside the rendering it prints.
pub const SINK: &str = "RAWR_DIAGNOSTICS";

/// Render one canonical report for terminal output.
#[must_use]
pub fn render(report: &Report) -> String {
    rawr::diagnostic::render(report)
}

/// Print `report` to stderr, and record it as data when [`SINK`] names a
/// file.
pub fn emit(report: &Report) {
    eprint!("{}", render(report));
    let base = std::env::current_dir().unwrap_or_default();
    record(&shape(report, &base));
}

/// Append `diagnostics` to the file [`SINK`] names, if any.
pub fn record(diagnostics: &[Value]) {
    let Some(path) = std::env::var_os(SINK).filter(|path| !path.is_empty()) else {
        return;
    };
    if diagnostics.is_empty() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let text: String = diagnostics
        .iter()
        .map(|diagnostic| format!("{diagnostic}\n"))
        .collect();
    let _ = file.write_all(text.as_bytes());
}

/// A report as data: one diagnostic per primary or labelled position, or
/// one without a position. Relative files are taken against `base`.
#[must_use]
pub fn shape(report: &Report, base: &Path) -> Vec<Value> {
    let view = report.view();
    let severity = view.severity().to_string();
    let message = bare(view.message());
    let absolute = |file: &str| {
        let path = Path::new(file);
        if path.is_absolute() {
            file.to_owned()
        } else {
            base.join(path).to_string_lossy().into_owned()
        }
    };
    let one = |file: Option<String>,
               start: Option<(usize, usize)>,
               end: Option<(usize, usize)>,
               label: Option<&str>| {
        json!({
            "label": label,
            "source": "rawr",
            "file": file,
            "line": start.map(|start| start.0),
            "column": start.map(|start| start.1),
            "end_line": end.map(|end| end.0),
            "end_column": end.map(|end| end.1),
            "code": view.code(),
            "severity": severity,
            "message": message.0,
            "help": view.help(),
        })
    };
    // The primary label, else every label, holds the position.
    let labels = view.labels();
    let primary = labels
        .iter()
        .filter(|label| label.kind() == rawr::diagnostic::LabelKind::Primary)
        .collect::<Vec<_>>();
    let chosen = if primary.is_empty() {
        labels.iter().collect()
    } else {
        primary
    };
    let mut shaped: Vec<Value> = chosen
        .iter()
        .filter_map(|label| {
            let span = label.span();
            let start = (span.start().line()?, span.start().column().unwrap_or(1));
            let end = span
                .end()
                .line()
                .map(|line| (line, span.end().column().unwrap_or(1)));
            Some(one(
                Some(absolute(label.source().identity())),
                Some(start),
                end,
                label.message(),
            ))
        })
        .collect();
    if shaped.is_empty() {
        shaped = view
            .external_labels()
            .iter()
            .map(|label| {
                let ((line, column), (end_line, end_column)) = label.positions();
                // External columns are zero-based.
                one(
                    Some(absolute(label.file())),
                    Some((line as usize, column as usize + 1)),
                    Some((end_line as usize, end_column as usize + 1)),
                    label.message(),
                )
            })
            .collect();
    }
    if shaped.is_empty() {
        // A report carried across the runner protocol keeps its position
        // only in its message: `... (line 215, position 27)`.
        let file = view
            .sources()
            .first()
            .map(|source| absolute(source.identity()));
        shaped.push(one(file, message.1, None, None));
    }
    shaped
}

/// A message without a leading `error[CODE]: ` and without a trailing
/// `(line N, position M)`, and that position.
fn bare(message: &str) -> (String, Option<(usize, usize)>) {
    let mut text = message.trim();
    for severity in ["error", "warning"] {
        if let Some(rest) = text.strip_prefix(severity)
            && let Some(rest) = rest.strip_prefix('[')
            && let Some((_, rest)) = rest.split_once("]: ")
        {
            text = rest;
        }
    }
    let position = text.rsplit_once(" (line ").and_then(|(before, rest)| {
        let (line, rest) = rest.split_once(", position ")?;
        let column = rest.strip_suffix(')')?;
        Some((
            before,
            (line.parse::<usize>().ok()?, column.parse::<usize>().ok()?),
        ))
    });
    match position {
        Some((before, position)) => (before.trim_end().to_owned(), Some(position)),
        None => (text.to_owned(), None),
    }
}

/// One rustc message from cargo's `--message-format=json`, as data. Its
/// files are relative to the workspace root, the nearest ancestor of
/// `manifest`'s directory that holds them.
#[must_use]
pub fn from_rustc(message: &Value, manifest: Option<&Path>) -> Option<Value> {
    let severity = match message["level"].as_str()? {
        "error" | "error: internal compiler error" => "error",
        "warning" => "warning",
        _ => return None,
    };
    let text = message["message"].as_str().unwrap_or_default();
    // rustc's closing summaries repeat what the others said.
    if text.starts_with("aborting due to")
        || text.ends_with("warning emitted")
        || text.ends_with("warnings emitted")
        || text.starts_with("could not compile")
    {
        return None;
    }
    let absolute = |file: &str| -> String {
        let path = Path::new(file);
        if path.is_absolute() {
            return file.to_owned();
        }
        let directory = manifest.and_then(Path::parent);
        directory
            .into_iter()
            .flat_map(Path::ancestors)
            .map(|ancestor| ancestor.join(path))
            .find(|candidate| candidate.exists())
            .unwrap_or_else(|| path.to_path_buf())
            .to_string_lossy()
            .into_owned()
    };
    let spans = message["spans"].as_array();
    let primary = spans.and_then(|spans| {
        spans
            .iter()
            .find(|span| span["is_primary"].as_bool() == Some(true))
            .or_else(|| spans.first())
    });
    let help: Vec<&str> = message["children"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|child| matches!(child["level"].as_str(), Some("help" | "note")))
        .filter_map(|child| child["message"].as_str())
        .collect();
    let suggestion = message["children"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|child| child["spans"].as_array().into_iter().flatten())
        .find(|span| span["suggested_replacement"].is_string())
        .map(|span| {
            json!({
                "file": span["file_name"].as_str().map(&absolute),
                "line": span["line_start"],
                "column": span["column_start"],
                "end_line": span["line_end"],
                "end_column": span["column_end"],
                "replacement": span["suggested_replacement"],
            })
        });
    let mut shaped = json!({
        "source": "rustc",
        "file": primary.and_then(|span| span["file_name"].as_str()).map(&absolute),
        "line": primary.map(|span| span["line_start"].clone()),
        "column": primary.map(|span| span["column_start"].clone()),
        "end_line": primary.map(|span| span["line_end"].clone()),
        "end_column": primary.map(|span| span["column_end"].clone()),
        "label": primary.and_then(|span| span["label"].as_str()),
        "code": message["code"]["code"],
        "severity": severity,
        "message": text,
        "help": (!help.is_empty()).then(|| help.join("\n")),
    });
    if let Some(suggestion) = suggestion {
        shaped["suggestion"] = suggestion;
    }
    Some(shaped)
}

/// Diagnostics as an agent reads them: one `file:line:col: severity[CODE]:
/// message` line each, help indented beneath.
#[must_use]
pub fn lines(diagnostics: &[Value]) -> String {
    let mut text = String::new();
    for diagnostic in diagnostics {
        let field = |key: &str| diagnostic[key].as_str().unwrap_or_default();
        let number = |key: &str| diagnostic[key].as_u64();
        let mut place = field("file").to_owned();
        if let Some(line) = number("line") {
            place.push_str(&format!(":{line}"));
            if let Some(column) = number("column") {
                place.push_str(&format!(":{column}"));
            }
        }
        if !place.is_empty() {
            place.push_str(": ");
        }
        let code = field("code");
        let code = if code.is_empty() {
            String::new()
        } else {
            format!("[{code}]")
        };
        text.push_str(&format!(
            "{place}{}{code}: {}\n",
            field("severity"),
            field("message")
        ));
        if !field("label").is_empty() {
            text.push_str(&format!("  label: {}\n", field("label")));
        }
        for help in field("help").lines() {
            text.push_str(&format!("  help: {help}\n"));
        }
        if let Some(replacement) = diagnostic["suggestion"]["replacement"].as_str() {
            text.push_str(&format!(
                "  suggestion: replace {}:{}-{}:{} with {replacement:?}\n",
                diagnostic["suggestion"]["line"],
                diagnostic["suggestion"]["column"],
                diagnostic["suggestion"]["end_line"],
                diagnostic["suggestion"]["end_column"],
            ));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use rawr::diagnostic::{code, Report, Source};
    use rawr::{Position, Span};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn cli_uses_the_core_canonical_report_rendering() {
        let source = Source::new("src/main.rs", "alpha\n").unwrap();
        let report = Report::error(code::PARSE_SYNTAX, "invalid source")
            .primary(
                source,
                Span::new(Position::new(1, 1), Position::new(1, 6)),
                Some("parse failed".into()),
            )
            .unwrap()
            .finish();

        assert_eq!(super::render(&report), rawr::diagnostic::render(&report));
    }

    #[test]
    fn a_report_becomes_one_positioned_diagnostic_with_an_absolute_file() {
        let source = Source::new("src/main.rs", "let x: i64 = \"a\";\n").unwrap();
        let report = Report::error(code::PARSE_SYNTAX, "mismatched types")
            .primary(
                source,
                Span::new(Position::new(1, 14), Position::new(1, 17)),
                Some("expected `i64`".into()),
            )
            .unwrap()
            .help("use a number")
            .finish();
        let shaped = super::shape(&report, Path::new("/work/app"));
        assert_eq!(shaped.len(), 1);
        let diagnostic = &shaped[0];
        assert_eq!(diagnostic["file"], "/work/app/src/main.rs");
        assert_eq!(
            (&diagnostic["line"], &diagnostic["column"]),
            (&json!(1), &json!(14))
        );
        assert_eq!(
            (&diagnostic["end_line"], &diagnostic["end_column"]),
            (&json!(1), &json!(17))
        );
        assert_eq!(diagnostic["message"], "mismatched types");
        assert_eq!(diagnostic["help"], "use a number");
        assert_eq!(diagnostic["severity"], "error");
        assert_eq!(
            super::lines(&shaped),
            format!(
                "/work/app/src/main.rs:1:14: error[{}]: mismatched types\n  label: expected `i64`\n  help: use a number\n",
                diagnostic["code"].as_str().unwrap()
            )
        );
    }

    #[test]
    fn a_message_carried_across_the_runner_loses_its_prefix_and_gives_its_position() {
        let source = Source::new("/app/src/main.rs", "fn main() {}\n").unwrap();
        let report = Report::error(
            code::PARSE_SYNTAX,
            "error[RAWR-TYPE]: mismatched types (line 3, position 7)",
        )
        .source(source)
        .finish();
        let shaped = super::shape(&report, Path::new("/elsewhere"));
        assert_eq!(shaped[0]["file"], "/app/src/main.rs");
        assert_eq!(shaped[0]["line"], 3);
        assert_eq!(shaped[0]["column"], 7);
        assert_eq!(shaped[0]["message"], "mismatched types");
    }

    #[test]
    fn a_rustc_message_keeps_its_code_help_and_suggestion() {
        let message = json!({
            "level": "error",
            "message": "mismatched types",
            "code": { "code": "E0308" },
            "spans": [
                { "file_name": "/c/src/main.rs", "is_primary": true, "line_start": 3,
                  "column_start": 18, "line_end": 3, "column_end": 21 },
            ],
            "children": [
                { "level": "help", "message": "try using a conversion method",
                  "spans": [ { "file_name": "/c/src/main.rs", "line_start": 3, "column_start": 18,
                               "line_end": 3, "column_end": 21, "suggested_replacement": "\"a\".to_string()" } ] },
            ],
        });
        let shaped = super::from_rustc(&message, None).unwrap();
        assert_eq!(shaped["code"], "E0308");
        assert_eq!(shaped["source"], "rustc");
        assert_eq!(shaped["line"], 3);
        assert_eq!(shaped["suggestion"]["replacement"], "\"a\".to_string()");
        assert!(super::lines(&[shaped])
            .starts_with("/c/src/main.rs:3:18: error[E0308]: mismatched types\n  help: try using"));
        let summary = json!({ "level": "error", "message": "aborting due to 1 previous error" });
        assert_eq!(super::from_rustc(&summary, None), None);
    }
}
