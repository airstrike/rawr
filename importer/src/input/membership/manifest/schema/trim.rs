//! Cargo's trim-paths union, including its entire string-vector first pass.

use rawr_toml::document;

use super::super::{session, storage, value, Diagnostic, Step, Tag};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum Trim {
    Diagnostics,
    Macro,
    Object,
}

pub(in super::super) enum Paths {
    All,
    Values(storage::Buffer<Trim>),
}

impl Paths {
    pub(in super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, '_>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        // serde-untagged erases and reconstructs callback errors; nested
        // sequence spans are replaced by the enclosing source value.
        Self::visit(input, session).map_err(|stop| super::primitive::erase(stop, input.span))
    }

    fn visit<A: document::Admission>(
        input: value::Input<'_, '_>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        match input.view(session)? {
            value::View::Scalar(value::Kind::Boolean(true)) => Ok(Self::All),
            value::View::Scalar(value::Kind::Boolean(false)) => {
                Ok(Self::Values(storage::Buffer::new()))
            }
            value::View::Scalar(value::Kind::String(text)) => match text.as_str() {
                "all" => Ok(Self::All),
                "none" => Ok(Self::Values(storage::Buffer::new())),
                value => {
                    let trim = convert(value, input.span, session)?;
                    let mut values = storage::Buffer::new();
                    values.push(trim, session)?;
                    Ok(Self::Values(values))
                }
            },
            value::View::Sequence(mut cursor) => {
                let mut strings = storage::Buffer::new();
                while let Some((_, child)) = cursor.next(session)? {
                    let value::View::Scalar(value::Kind::String(text)) = child.view(session)?
                    else {
                        return session
                            .reject(Diagnostic::at(Tag::Type, child.span.or(input.span)));
                    };
                    strings.push(text, session)?;
                }
                let mut values = storage::Buffer::new();
                for text in strings.as_slice() {
                    // StringDeserializer erases the element's original TOML
                    // span. The outer array value supplies error context.
                    let trim = convert(text.as_str(), input.span, session)?;
                    values.push(trim, session)?;
                }
                Ok(Self::Values(values))
            }
            _ => session.reject(Diagnostic::at(Tag::Type, input.span)),
        }
    }
}

fn convert<A: document::Admission>(
    value: &str,
    span: Option<super::super::Span>,
    session: &mut session::Session<'_, A>,
) -> Step<Trim> {
    session.check()?;
    match value {
        "diagnostics" => Ok(Trim::Diagnostics),
        "macro" => Ok(Trim::Macro),
        "object" => Ok(Trim::Object),
        _ => session.reject(Diagnostic::at(Tag::Variant, span)),
    }
}
