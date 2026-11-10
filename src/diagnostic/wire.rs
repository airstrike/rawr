//! A report as text that reads back whole: how a runner tells its owner
//! the error its compile found, so the owner shows it as its own compile
//! shows it, in its own file (#397).

use std::fmt;

use serde_json::{json, Value};

use super::{code, Category, ExternalPosition, LabelKind, Report, Severity, Source};
use crate::{Position, Span};

/// Why a text is no report [`Report::encode`] wrote.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// The text is not the shape a report is written in, at this part.
    Malformed(&'static str),
    /// The text names a code Rawr doesn't declare.
    UnknownCode(String),
    /// The report the text holds is not a valid one.
    Report(super::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(part) => write!(formatter, "a sent diagnostic has a malformed {part}"),
            Self::UnknownCode(code) => {
                write!(
                    formatter,
                    "a sent diagnostic names the unknown code `{code}`"
                )
            }
            Self::Report(error) => write!(formatter, "a sent diagnostic is invalid: {error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<super::Error> for Error {
    fn from(error: super::Error) -> Self {
        Self::Report(error)
    }
}

impl Report {
    /// This report as text [`Report::decode`] reads back whole, each of its
    /// sources with its text.
    #[must_use]
    pub fn encode(&self) -> String {
        let sources = self.sources();
        let labels = self.labels().iter().map(|label| {
            json!({
                "source": sources.iter().position(|source| source == label.source()),
                "span": [place(label.span().start()), place(label.span().end())],
                "primary": label.kind() == LabelKind::Primary,
                "message": label.message(),
            })
        });
        let external = self.external_labels().iter().map(|label| {
            let position = label.position();
            let bytes = position.byte_range();
            let (start, end) = position.positions();
            json!({
                "file": position.file(),
                "bytes": [bytes.start, bytes.end],
                "start": [start.0, start.1],
                "end": [end.0, end.1],
                "message": label.message(),
            })
        });
        json!({
            "code": self.code().as_str(),
            "category": self.category().to_string(),
            "warning": self.severity() == Severity::Warning,
            "message": self.message(),
            "sources": sources
                .iter()
                .map(|source| [source.identity(), source.text()])
                .collect::<Vec<_>>(),
            "labels": labels.collect::<Vec<_>>(),
            "external": external.collect::<Vec<_>>(),
            "help": self.help(),
            "notes": self.notes().collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// The report `text` holds, as [`Report::encode`] wrote it.
    ///
    /// # Errors
    ///
    /// Returns an error for a text no report wrote.
    pub fn decode(text: &str) -> Result<Self, Error> {
        let value: Value = serde_json::from_str(text).map_err(|_| Error::Malformed("text"))?;
        let field = |name: &'static str| value.get(name).ok_or(Error::Malformed(name));
        let list = |name: &'static str| field(name)?.as_array().ok_or(Error::Malformed(name));
        let printed = string(field("code")?, "code")?;
        let category = category(string(field("category")?, "category")?)?;
        let code = code::declared(printed, category)
            .ok_or_else(|| Error::UnknownCode(printed.to_owned()))?;
        let message = string(field("message")?, "message")?;
        let mut builder = match field("warning")?.as_bool() {
            Some(true) => Self::warning(code, message),
            Some(false) => Self::error(code, message),
            None => return Err(Error::Malformed("warning")),
        };
        let mut sources = Vec::new();
        for source in list("sources")? {
            let [identity, body] = pair(source).ok_or(Error::Malformed("source"))?;
            let source = Source::new(string(identity, "source")?, string(body, "source")?)?;
            builder = builder.source(source.clone());
            sources.push(source);
        }
        for label in list("labels")? {
            let source = label
                .get("source")
                .and_then(Value::as_u64)
                .and_then(|index| sources.get(usize::try_from(index).ok()?))
                .ok_or(Error::Malformed("label"))?
                .clone();
            let span = label
                .get("span")
                .and_then(span)
                .ok_or(Error::Malformed("span"))?;
            let message = optional(label.get("message"), "label")?.map(Into::into);
            builder = match label.get("primary").and_then(Value::as_bool) {
                Some(true) => builder.primary(source, span, message)?,
                Some(false) => builder.secondary(source, span, message)?,
                None => return Err(Error::Malformed("label")),
            };
        }
        for label in list("external")? {
            let numbers = |name| label.get(name).and_then(numbers);
            let (Some(bytes), Some(start), Some(end)) =
                (numbers("bytes"), numbers("start"), numbers("end"))
            else {
                return Err(Error::Malformed("external label"));
            };
            let file = label
                .get("file")
                .ok_or(Error::Malformed("external label"))?;
            let position = ExternalPosition::checked(
                string(file, "external label")?,
                bytes.0,
                bytes.1,
                start,
                end,
            )?;
            let message = optional(label.get("message"), "external label")?.map(Into::into);
            builder = builder.secondary_external(position, message);
        }
        if let Some(help) = optional(value.get("help"), "help")? {
            builder = builder.help(help);
        }
        for note in list("notes")? {
            builder = builder.note(string(note, "note")?);
        }
        Ok(builder.finish())
    }
}

/// `position` as `[line, position]`, each zero for none.
fn place(position: Position) -> [usize; 2] {
    [
        position.line().unwrap_or(0),
        position.position().unwrap_or(0),
    ]
}

/// The text `value` holds, or a malformed `part`.
fn string<'v>(value: &'v Value, part: &'static str) -> Result<&'v str, Error> {
    value.as_str().ok_or(Error::Malformed(part))
}

/// The text `value` holds, if it holds any, or a malformed `part`.
fn optional(value: Option<&Value>, part: &'static str) -> Result<Option<String>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => string(value, part).map(|text| Some(text.to_owned())),
    }
}

/// The two values of the list `value`.
fn pair(value: &Value) -> Option<&[Value; 2]> {
    <&[Value; 2]>::try_from(value.as_array()?.as_slice()).ok()
}

/// The two numbers of the list `value`.
fn numbers(value: &Value) -> Option<(u32, u32)> {
    let [first, second] = pair(value)?;
    let number = |value: &Value| u32::try_from(value.as_u64()?).ok();
    Some((number(first)?, number(second)?))
}

/// The family `name` prints as.
fn category(name: &str) -> Result<Category, Error> {
    [
        Category::Parse,
        Category::Resolve,
        Category::Framework,
        Category::Runtime,
        Category::Environment,
    ]
    .into_iter()
    .find(|category| category.to_string() == name)
    .ok_or(Error::Malformed("category"))
}

/// The span `[[line, position], [line, position]]` writes.
fn span(value: &Value) -> Option<Span> {
    let position = |value: &Value| {
        let [line, position] = pair(value)?;
        let line = u16::try_from(line.as_u64()?)
            .ok()
            .filter(|line| *line != 0)?;
        let position = u16::try_from(position.as_u64()?).ok()?;
        Some(Position::new(line, position))
    };
    let [start, end] = pair(value)?;
    Some(Span::new(position(start)?, position(end)?))
}

#[cfg(test)]
mod tests {
    use super::super::{code, ExternalPosition, Report, Source};
    use crate::{Position, Span};

    #[test]
    fn a_report_reads_back_whole_from_its_text() {
        let main = Source::new("app/src/main.rs", "fn main() {\n    view::row();\n}\n")
            .expect("the entry has an identity");
        let view = Source::new(
            "app/src/view.rs",
            "pub fn row() {\n    let _ = unknown;\n}\n",
        )
        .expect("the module has an identity");
        let place = ExternalPosition::checked("lib.rs", 4, 9, (1, 4), (1, 9))
            .expect("the external place is in order");
        let report = Report::error(
            code::RESOLVE_UNKNOWN_VARIABLE,
            "cannot find value `unknown` in this scope",
        )
        .primary(
            view,
            Span::new(Position::new(2, 13), Position::new(2, 20)),
            Some("not found in this scope".into()),
        )
        .and_then(|builder| {
            builder.secondary(
                main,
                Span::new(Position::new(2, 5), Position::new(2, 14)),
                None,
            )
        })
        .expect("the labels are in their sources")
        .secondary_external(place, Some("defined here".into()))
        .help("a local variable with a similar name exists")
        .note("the note")
        .finish();
        assert_eq!(Report::decode(&report.encode()), Ok(report));
    }

    #[test]
    fn a_text_no_report_wrote_is_no_report() {
        assert!(Report::decode("cannot find value `unknown` (line 2, position 13)").is_err());
    }
}
