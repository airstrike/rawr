//! Pinned package scalars and finite, nonrecursive container visitors.
//!
//! Cargo schema mod.rs:370–599,1692–1797 supplies the scalar unions below.
//! Serde's map/sequence adapters remain represented by the original Cursor.

use rawr_toml::document;

use super::super::{scalar, session, storage, value, Diagnostic, Span, Step, Tag};

mod badges;
mod version;

pub(in super::super) use badges::Badges;
pub(in super::super) use version::Version;

#[derive(Clone, Copy)]
pub(in super::super) enum Shape {
    String,
    Boolean,
    Strings,
    Version { trim: bool },
    Rust,
    Name,
    StringOrBool,
    StringsOrBool,
    StringOrStrings,
    Build,
    Badges,
}

/// Actual converted package data; each collection owns only flat elements.
pub(in super::super) enum Value<'d> {
    String(value::Text<'d>),
    Boolean(bool),
    Strings(storage::Buffer<value::Text<'d>>),
    Version(Version<'d>),
    Rust(scalar::Rust),
    Name(Name<'d>),
    StringOrBool(StringOrBool<'d>),
    StringsOrBool(StringsOrBool<'d>),
    Build(Build<'d>),
    Badges(Badges<'d>),
}

pub(in super::super) enum StringOrBool<'d> {
    String(value::Text<'d>),
    Boolean(bool),
}
pub(in super::super) enum StringsOrBool<'d> {
    Strings(storage::Buffer<value::Text<'d>>),
    Boolean(bool),
}
pub(in super::super) enum Build<'d> {
    Auto(bool),
    Single(value::Text<'d>),
    Multiple(storage::Buffer<value::Text<'d>>),
}

/// The exact Text spelling checked under one pinned name grammar.
/// It never borrows its own inline bytes.
pub(in super::super) struct Name<'d> {
    text: value::Text<'d>,
    kind: scalar::Kind,
}

impl<'d> Name<'d> {
    /// The caller admits the input string and its retained typed slot.
    pub(in super::super) fn parse<A: document::Admission>(
        text: value::Text<'d>,
        kind: scalar::Kind,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        match scalar::Name::parse(text.as_str(), kind) {
            Ok(_) => Ok(Self { text, kind }),
            Err(error) => session.reject(Diagnostic::at(Tag::Scalar(error), span)),
        }
    }

    pub(in super::super) fn as_str(&self) -> &str {
        self.text.as_str()
    }

    pub(in super::super) fn kind(&self) -> scalar::Kind {
        self.kind
    }
}

pub(in super::super) fn convert<'d, A: document::Admission>(
    input: value::Input<'_, 'd>,
    shape: Shape,
    session: &mut session::Session<'_, A>,
) -> Step<Value<'d>> {
    from_view(input.view(session)?, shape, input.span, session)
}

pub(in super::super) fn from_view<'d, A: document::Admission>(
    view: value::View<'_, 'd>,
    shape: Shape,
    span: Option<Span>,
    session: &mut session::Session<'_, A>,
) -> Step<Value<'d>> {
    session.check()?;
    match (shape, view) {
        (Shape::String, value::View::Scalar(value::Kind::String(text))) => Ok(Value::String(text)),
        (Shape::Boolean, value::View::Scalar(value::Kind::Boolean(value))) => {
            Ok(Value::Boolean(value))
        }
        (Shape::Strings, value::View::Sequence(cursor)) => {
            strings(cursor, span, session).map(Value::Strings)
        }
        (Shape::StringOrStrings, value::View::Sequence(cursor)) => strings(cursor, span, session)
            .map(Value::Strings)
            .map_err(|stop| erase(stop, span)),
        (Shape::StringOrStrings, value::View::Scalar(value::Kind::String(text))) => {
            let mut values = storage::Buffer::new();
            values.push(text, session)?;
            Ok(Value::Strings(values))
        }
        (Shape::Version { trim }, value::View::Scalar(value::Kind::String(text))) => {
            Version::parse(text, trim, span, session).map(Value::Version)
        }
        (Shape::Rust, value::View::Scalar(value::Kind::String(text))) => {
            match scalar::Rust::parse(text.as_str()) {
                Ok(value) => Ok(Value::Rust(value)),
                Err(error) => session.reject(Diagnostic::at(Tag::Scalar(error), span)),
            }
        }
        (Shape::Name, value::View::Scalar(value::Kind::String(text))) => {
            Name::parse(text, scalar::Kind::Package, span, session).map(Value::Name)
        }
        (Shape::StringOrBool, value::View::Scalar(value::Kind::String(text))) => {
            Ok(Value::StringOrBool(StringOrBool::String(text)))
        }
        (Shape::StringOrBool, value::View::Scalar(value::Kind::Boolean(value))) => {
            Ok(Value::StringOrBool(StringOrBool::Boolean(value)))
        }
        (Shape::StringsOrBool, value::View::Scalar(value::Kind::Boolean(value))) => {
            Ok(Value::StringsOrBool(StringsOrBool::Boolean(value)))
        }
        (Shape::StringsOrBool, value::View::Sequence(cursor)) => strings(cursor, span, session)
            .map(|values| Value::StringsOrBool(StringsOrBool::Strings(values)))
            .map_err(|stop| erase(stop, span)),
        (Shape::Build, value::View::Scalar(value::Kind::Boolean(value))) => {
            Ok(Value::Build(Build::Auto(value)))
        }
        (Shape::Build, value::View::Scalar(value::Kind::String(text))) => {
            Ok(Value::Build(Build::Single(text)))
        }
        (Shape::Build, value::View::Sequence(cursor)) => strings(cursor, span, session)
            .map(|values| Value::Build(Build::Multiple(values)))
            .map_err(|stop| erase(stop, span)),
        (Shape::Badges, value::View::Map(cursor)) => {
            Badges::interpret(cursor, span, session).map(Value::Badges)
        }
        _ => session.reject(Diagnostic::at(Tag::Type, span)),
    }
}

/// serde-untagged erases nested concrete errors through its map/seq adapter and
/// recreates the outer source error (map.rs:100–117, seq.rs:80–88). Keep the
/// bounded semantic tag, restore only its actual outer span, and never rewrite
/// the terminal resource latch.
pub(in super::super) fn erase(stop: super::super::Stop, span: Option<Span>) -> super::super::Stop {
    match stop {
        super::super::Stop::Rejected(mut diagnostic) => {
            diagnostic.span = span;
            super::super::Stop::Rejected(diagnostic)
        }
        super::super::Stop::Refused => super::super::Stop::Refused,
    }
}

fn strings<'d, A: document::Admission>(
    mut cursor: value::Cursor<'_, 'd>,
    span: Option<Span>,
    session: &mut session::Session<'_, A>,
) -> Step<storage::Buffer<value::Text<'d>>> {
    let mut values = storage::Buffer::new();
    while let Some((_, input)) = cursor.next(session)? {
        match input.view(session)? {
            value::View::Scalar(value::Kind::String(text)) => {
                values.push(text, session)?;
            }
            _ => return session.reject(Diagnostic::at(Tag::Type, input.span.or(span))),
        }
    }
    Ok(values)
}
