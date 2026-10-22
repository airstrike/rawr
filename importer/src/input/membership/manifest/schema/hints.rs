//! Cargo's complete Hints struct visitor over the retained source adapter.

use rawr_toml::document;

use super::super::{session, storage, value, walk, Diagnostic, Error, Span, Step, Stop, Tag};

#[cfg(test)]
mod tests;

/// Converted optional metadata with its field origin and ignored callbacks.
pub(in super::super) struct Hints<'d> {
    mostly_unused: Option<value::Projection<'d>>,
    declared: Option<value::Key<'d>>,
    unused: storage::Buffer<value::Key<'d>>,
}

impl<'d> Hints<'d> {
    pub(in super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let mut mostly_unused = None;
        let mut declared = None;
        let mut unused = storage::Buffer::new();
        match input.view(session)? {
            value::View::Map(mut cursor) => {
                while let Some((key, child)) = cursor.next(session)? {
                    let Some(key) = key else {
                        return session.fail(Error::State);
                    };
                    if key.text.as_str() == "mostly-unused" {
                        if declared.is_some() {
                            return session.reject(Diagnostic {
                                tag: Tag::Duplicate,
                                span: input.span,
                                expected: Some("mostly-unused"),
                            });
                        }
                        mostly_unused = Some(
                            walk::run(child, value::Mode::Toml, session)
                                .map_err(|stop| locate(stop, child.span.or(input.span)))?,
                        );
                        declared = Some(key);
                    } else {
                        unused.push(key, session)?;
                        walk::run(child, value::Mode::Ignored, session)
                            .map_err(|stop| locate(stop, child.span.or(input.span)))?;
                    }
                }
            }
            value::View::Sequence(mut cursor) => {
                let Some((_, child)) = cursor.next(session)? else {
                    return session.reject(Diagnostic {
                        tag: Tag::Missing,
                        span: input.span,
                        expected: Some("mostly-unused"),
                    });
                };
                mostly_unused = Some(
                    walk::run(child, value::Mode::Toml, session)
                        .map_err(|stop| locate(stop, child.span.or(input.span)))?,
                );
                declared = Some(value::Key {
                    text: value::Text::Borrowed("mostly-unused"),
                    span: None,
                });
                // Array exhaustion is required only by the retained adapter
                // that actually requests it; raw TOML leaves extras unvisited.
                cursor.finish(input.span, session)?;
            }
            _ => return session.reject(Diagnostic::at(Tag::Type, input.span)),
        }
        Ok(Self {
            mostly_unused,
            declared,
            unused,
        })
    }

    pub(in super::super) fn mostly_unused(&self) -> Option<&value::Projection<'d>> {
        self.mostly_unused.as_ref()
    }
    pub(in super::super) fn declared(&self) -> Option<value::Key<'d>> {
        self.declared
    }
    pub(in super::super) fn unused(&self) -> &[value::Key<'d>] {
        self.unused.as_slice()
    }
}

fn locate(stop: Stop, span: Option<Span>) -> Stop {
    match stop {
        Stop::Rejected(mut error) => {
            error.span = error.span.or(span);
            Stop::Rejected(error)
        }
        Stop::Refused => Stop::Refused,
    }
}
