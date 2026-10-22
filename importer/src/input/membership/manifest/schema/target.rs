//! Cargo TomlTarget's complete ordered map and positional sequence visitor.

use rawr_toml::document;

use super::super::{session, storage, value, walk, Diagnostic, Span, Step, Tag};

#[derive(Clone, Copy)]
enum Shape {
    String,
    Boolean,
    Strings,
}

const FIELDS: [(&str, Shape); 15] = [
    ("name", Shape::String),
    ("crate-type", Shape::Strings),
    ("crate_type", Shape::Strings),
    ("path", Shape::String),
    ("filename", Shape::String),
    ("test", Shape::Boolean),
    ("doctest", Shape::Boolean),
    ("bench", Shape::Boolean),
    ("doc", Shape::Boolean),
    ("doc-scrape-examples", Shape::Boolean),
    ("proc-macro", Shape::Boolean),
    ("proc_macro", Shape::Boolean),
    ("harness", Shape::Boolean),
    ("required-features", Shape::Strings),
    ("edition", Shape::String),
];

pub(in super::super) struct Target<'d> {
    values: value::Projection<'d>,
    unused: storage::Buffer<value::Key<'d>>,
}

impl<'d> Target<'d> {
    pub(in super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let mut values = value::Projection::new(value::Mode::Direct);
        let root = values.append(
            value::Node {
                kind: value::Kind::Map(value::Children::default()),
                span: input.span,
            },
            None,
            session,
        )?;
        let mut unused = storage::Buffer::new();
        match input.view(session)? {
            value::View::Map(mut cursor) => {
                while let Some((key, value)) = cursor.next(session)? {
                    let Some(key) = key else {
                        return session.fail(super::super::Error::State);
                    };
                    if let Some((_, shape)) =
                        FIELDS.iter().find(|(name, _)| *name == key.text.as_str())
                    {
                        convert(value, *shape, key, &mut values, session)?;
                    } else {
                        unused.push(key, session)?;
                        // Replay provenance decides whether ignored values
                        // recurse or use Content's immediate unit result.
                        walk::run(value, value::Mode::Ignored, session)?;
                    }
                }
            }
            value::View::Sequence(mut cursor) => {
                for (name, shape) in FIELDS {
                    let Some((_, value)) = cursor.next(session)? else {
                        return session.reject(Diagnostic {
                            tag: Tag::Missing,
                            span: input.span,
                            expected: Some(name),
                        });
                    };
                    convert(
                        value,
                        shape,
                        value::Key {
                            text: value::Text::Borrowed(name),
                            span: None,
                        },
                        &mut values,
                        session,
                    )?;
                }
                // Raw and serde-value leave trailing values unconverted;
                // Content and generic TOML require exhaustion.
                cursor.finish(input.span, session)?;
            }
            _ => return session.reject(Diagnostic::at(Tag::Type, input.span)),
        }
        debug_assert_eq!(root, 0);
        Ok(Self { values, unused })
    }

    pub(in super::super) fn values(&self) -> &value::Projection<'d> {
        &self.values
    }
    pub(in super::super) fn unused(&self) -> &[value::Key<'d>] {
        self.unused.as_slice()
    }
}

fn convert<'d, A: document::Admission>(
    input: value::Input<'_, 'd>,
    shape: Shape,
    key: value::Key<'d>,
    output: &mut value::Projection<'d>,
    session: &mut session::Session<'_, A>,
) -> Step<()> {
    let view = input.view(session)?;
    let kind = match (shape, view) {
        (Shape::String, value::View::Scalar(value::Kind::String(text))) => {
            value::Kind::String(text)
        }
        (Shape::Boolean, value::View::Scalar(value::Kind::Boolean(value))) => {
            value::Kind::Boolean(value)
        }
        (Shape::Strings, value::View::Sequence(mut cursor)) => {
            let parent = output.append(
                value::Node {
                    kind: value::Kind::Sequence(value::Children::default()),
                    span: input.span,
                },
                Some((0, Some(key))),
                session,
            )?;
            while let Some((_, value)) = cursor.next(session)? {
                let value::View::Scalar(value::Kind::String(text)) = value.view(session)? else {
                    return mismatch(value.span.or(input.span), session);
                };
                output.append(
                    value::Node {
                        kind: value::Kind::String(text),
                        span: value.span,
                    },
                    Some((parent, None)),
                    session,
                )?;
            }
            return Ok(());
        }
        _ => return mismatch(input.span, session),
    };
    output.append(
        value::Node {
            kind,
            span: input.span,
        },
        Some((0, Some(key))),
        session,
    )?;
    Ok(())
}

fn mismatch<A: document::Admission>(
    span: Option<Span>,
    session: &mut session::Session<'_, A>,
) -> Step<()> {
    session.reject(Diagnostic::at(Tag::Type, span))
}
