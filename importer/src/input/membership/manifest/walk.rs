//! Explicit generic-value continuations, including early datetime-map return.

use rawr_toml::document;

use super::{datetime, session, storage, value, Diagnostic, Error, Span, Step, Tag};

enum Frame<'p, 'd> {
    Enter {
        input: value::Input<'p, 'd>,
        target: Option<(usize, Option<value::Key<'d>>)>,
        context: Option<Span>,
    },
    Children {
        cursor: value::Cursor<'p, 'd>,
        parent: Option<usize>,
        map: bool,
        context: Option<Span>,
    },
}

pub(super) fn project<'d, A: document::Admission>(
    input: value::Input<'_, 'd>,
    mode: value::Mode,
    admission: &mut A,
) -> Result<value::Projection<'d>, Error<A::Error>> {
    let mut session = session::Session::new(admission);
    let result = run(input, mode, &mut session);
    session.finish(result)
}

pub(super) fn run<'d, A: document::Admission>(
    input: value::Input<'_, 'd>,
    mode: value::Mode,
    session: &mut session::Session<'_, A>,
) -> Step<value::Projection<'d>> {
    run_with_root(input, mode, false, session)
}

/// Cargo configuration parses a Table at the root, not a generic Value.
/// Nested values still use the complete generic TOML conversion.
pub(super) fn toml_table<'d, A: document::Admission>(
    document: &'d document::Document<'d>,
    session: &mut session::Session<'_, A>,
) -> Step<value::Projection<'d>> {
    run_with_root(
        value::Input::root(document),
        value::Mode::Toml,
        true,
        session,
    )
}

fn run_with_root<'d, A: document::Admission>(
    input: value::Input<'_, 'd>,
    mode: value::Mode,
    table_root: bool,
    session: &mut session::Session<'_, A>,
) -> Step<value::Projection<'d>> {
    session.check()?;
    let mut output = value::Projection::new(mode);
    // Content's ignored visitor returns unit without inspecting descendants.
    // Its retained flat storage remains owned by the caller.
    if mode == value::Mode::Ignored && input.ignores_as_unit() {
        return Ok(output);
    }
    let mut frames = storage::Buffer::new();
    let mut scratch = storage::Buffer::new();
    frames.push(
        Frame::Enter {
            input,
            target: None,
            context: input.span,
        },
        session,
    )?;
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Enter {
                input,
                target,
                context,
            } => {
                let context = input.span.or(context);
                let view = input.view(session).map_err(|stop| locate(stop, context))?;
                let (kind, cursor) = match view {
                    value::View::Scalar(kind) => (scalar(kind, mode, context, session)?, None),
                    value::View::Sequence(cursor) => (
                        value::Kind::Sequence(value::Children::default()),
                        Some((cursor, false)),
                    ),
                    value::View::Map(mut cursor) => {
                        if mode == value::Mode::Toml
                            && (!table_root || target.is_some())
                            && let Some((key, input)) = cursor.next(session)?
                        {
                            if key
                                .as_ref()
                                .is_some_and(|key| key.text.as_str() == datetime::FIELD)
                            {
                                let value =
                                    input.view(session).map_err(|stop| locate(stop, context))?;
                                let value::View::Scalar(value::Kind::String(text)) = value else {
                                    return session
                                        .reject(Diagnostic::at(Tag::Type, input.span.or(context)));
                                };
                                let date =
                                    text.as_str().parse::<document::Datetime>().map_err(|_| {
                                        // A diagnostic item is admitted by the error path below.
                                        Diagnostic::at(Tag::DateTime, input.span.or(context))
                                    });
                                let date = match date {
                                    Ok(value) => value,
                                    Err(error) => return session.reject(error),
                                };
                                cursor.finish(context, session)?;
                                output.append(
                                    value::Node {
                                        kind: value::Kind::DateTime(date),
                                        span: context,
                                    },
                                    target,
                                    session,
                                )?;
                                // Pinned ValueVisitor returns here, without visiting trailing values.
                                continue;
                            }
                            let index = output.append(
                                value::Node {
                                    kind: value::Kind::Map(value::Children::default()),
                                    span: context,
                                },
                                target,
                                session,
                            )?;
                            frames.push(
                                Frame::Children {
                                    cursor,
                                    parent: Some(index),
                                    map: true,
                                    context,
                                },
                                session,
                            )?;
                            frames.push(
                                Frame::Enter {
                                    input,
                                    target: Some((index, key)),
                                    context,
                                },
                                session,
                            )?;
                            continue;
                        }
                        (
                            value::Kind::Map(value::Children::default()),
                            Some((cursor, true)),
                        )
                    }
                };
                let parent = if mode == value::Mode::Ignored {
                    None
                } else {
                    Some(output.append(
                        value::Node {
                            kind,
                            span: context,
                        },
                        target,
                        session,
                    )?)
                };
                if let Some((cursor, map)) = cursor {
                    frames.push(
                        Frame::Children {
                            cursor,
                            parent,
                            map,
                            context,
                        },
                        session,
                    )?;
                }
            }
            Frame::Children {
                mut cursor,
                parent,
                map,
                context,
            } => {
                if let Some((key, input)) = cursor.next(session)? {
                    frames.push(
                        Frame::Children {
                            cursor,
                            parent,
                            map,
                            context,
                        },
                        session,
                    )?;
                    frames.push(
                        Frame::Enter {
                            input,
                            target: parent.map(|parent| (parent, key)),
                            context,
                        },
                        session,
                    )?;
                } else if map
                    && mode == value::Mode::SerdeValue
                    && let Some(parent) = parent
                {
                    output.sort(parent, &mut scratch, session)?;
                }
            }
        }
    }
    Ok(output)
}

fn scalar<'d, A: document::Admission>(
    kind: value::Kind<'d>,
    mode: value::Mode,
    span: Option<Span>,
    session: &mut session::Session<'_, A>,
) -> Step<value::Kind<'d>> {
    match (mode, kind) {
        (
            value::Mode::Content | value::Mode::SerdeValue | value::Mode::Toml,
            value::Kind::I128(_) | value::Kind::U128(_),
        ) => session.reject(Diagnostic::at(Tag::Integer128, span)),
        (value::Mode::Toml, value::Kind::U64(value)) => match i64::try_from(value) {
            Ok(value) => Ok(value::Kind::I64(value)),
            Err(_) => session.reject(Diagnostic::at(Tag::Integer64, span)),
        },
        _ => Ok(kind),
    }
}

fn locate(stop: super::Stop, context: Option<Span>) -> super::Stop {
    match stop {
        super::Stop::Rejected(mut diagnostic) => {
            diagnostic.span = diagnostic.span.or(context);
            super::Stop::Rejected(diagnostic)
        }
        super::Stop::Refused => super::Stop::Refused,
    }
}
