//! The map arm shared by Cargo's package inheritance visitors. All reached
//! wrappers admit only a map, so TomlInheritedField's sequence arm is unreachable
//! here. Its required workspace bool is converted in encounter order and false
//! rejects immediately (Cargo schema mod.rs:622–657).

use rawr_toml::document;

use super::super::super::{session, storage, value, Diagnostic, Span, Step, Tag};
use super::{Ignored, Position};

pub(super) fn interpret<'d, A: document::Admission>(
    mut cursor: value::Cursor<'_, 'd>,
    span: Option<Span>,
    position: Position<'d>,
    ignored: &mut storage::Buffer<Ignored<'d>>,
    session: &mut session::Session<'_, A>,
) -> Step<()> {
    let mut workspace = false;
    while let Some((key, input)) = cursor.next(session)? {
        let Some(key) = key else {
            return session.fail(super::super::super::Error::State);
        };
        if key.text.as_str() == "workspace" {
            if workspace {
                return session.reject(Diagnostic {
                    tag: Tag::Duplicate,
                    span,
                    expected: Some("workspace"),
                });
            }
            match input.view(session)? {
                value::View::Scalar(value::Kind::Boolean(true)) => workspace = true,
                value::View::Scalar(value::Kind::Boolean(false)) => {
                    return session.reject(Diagnostic::at(Tag::WorkspaceFalse, input.span.or(span)))
                }
                _ => return session.reject(Diagnostic::at(Tag::Type, input.span.or(span))),
            }
        } else {
            super::ignore(input, Some(position), key, ignored, session)?;
        }
    }
    if !workspace {
        return session.reject(Diagnostic {
            tag: Tag::Missing,
            span,
            expected: Some("workspace"),
        });
    }
    Ok(())
}
