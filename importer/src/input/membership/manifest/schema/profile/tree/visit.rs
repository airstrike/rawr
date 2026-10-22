//! Admitted continuations preserve input-order recursion without recursive calls.

use rawr_toml::document;

use super::{map, Entry, Field, Location, PackageSpec, Packages, Profile, Profiles, Value, FIELDS};
use crate::input::membership::manifest::{
    scalar,
    schema::{package, primitive, profile, trim},
    session, spec, storage, value, walk, Diagnostic, Error, Span, Step, Stop, Tag,
};

enum Frame<'p, 'd> {
    Roots(value::Cursor<'p, 'd>),
    Root {
        name: primitive::Name<'d>,
        key: value::Key<'d>,
    },
    Enter {
        input: value::Input<'p, 'd>,
        location: Location<'d>,
    },
    Fields {
        cursor: value::Cursor<'p, 'd>,
        sequence: Option<usize>,
        node: usize,
    },
    Override {
        parent: usize,
        position: package::Position<'d>,
    },
    Packages {
        cursor: value::Cursor<'p, 'd>,
        entries: map::Map<'d, PackageSpec>,
        parent: usize,
        position: package::Position<'d>,
        span: Option<Span>,
    },
    Package {
        spec: PackageSpec,
        key: value::Key<'d>,
    },
}

impl<'d> Profiles<'d> {
    pub(in crate::input::membership::manifest) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let value::View::Map(cursor) = input.view(session)? else {
            return session.reject(Diagnostic::at(Tag::Type, input.span));
        };
        let mut output = Self {
            roots: map::Map::new(),
            nodes: storage::Buffer::new(),
            callbacks: storage::Buffer::new(),
        };
        let mut frames = storage::Buffer::new();
        frames.push(Frame::Roots(cursor), session)?;
        let mut completed = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Roots(mut cursor) => {
                    if let Some((key, child)) = cursor.next(session)? {
                        let Some(key) = key else {
                            return session.fail(Error::State);
                        };
                        let name = primitive::Name::parse(
                            key.text,
                            scalar::Kind::Profile,
                            key.span.or(input.span),
                            session,
                        )?;
                        frames.push(Frame::Roots(cursor), session)?;
                        frames.push(Frame::Root { name, key }, session)?;
                        frames.push(
                            Frame::Enter {
                                input: child,
                                location: Location::Root(key),
                            },
                            session,
                        )?;
                    } else {
                        output
                            .roots
                            .finish(|a, b| a.as_str().cmp(b.as_str()), session)?;
                    }
                }
                Frame::Root { name, key } => {
                    let Some(node) = completed.take() else {
                        return session.fail(Error::State);
                    };
                    output.roots.push(name, key, node, session)?;
                }
                Frame::Enter { input, location } => {
                    let (cursor, sequence) = match input.view(session)? {
                        value::View::Map(cursor) => (cursor, None),
                        value::View::Sequence(cursor) => (cursor, Some(0)),
                        _ => return session.reject(Diagnostic::at(Tag::Type, input.span)),
                    };
                    let node = output.nodes.push(
                        Profile {
                            entries: storage::Buffer::new(),
                            ignored: storage::Buffer::new(),
                            span: input.span,
                            location,
                        },
                        session,
                    )?;
                    frames.push(
                        Frame::Fields {
                            cursor,
                            sequence,
                            node,
                        },
                        session,
                    )?;
                }
                Frame::Fields {
                    mut cursor,
                    sequence,
                    node,
                } => {
                    let span = output.node(node).span;
                    if sequence == Some(FIELDS.len()) {
                        cursor.finish(span, session)?;
                        completed = Some(node);
                        continue;
                    }
                    let Some((key, child)) = cursor.next(session)? else {
                        cursor.finish(span, session)?;
                        completed = Some(node);
                        continue;
                    };
                    let (field, position) = match sequence {
                        Some(index) => (Some(FIELDS[index]), package::Position::Sequence(index)),
                        None => {
                            let Some(key) = key else {
                                return session.fail(Error::State);
                            };
                            (
                                FIELDS
                                    .iter()
                                    .copied()
                                    .find(|(_, name)| *name == key.text.as_str()),
                                package::Position::Map(key),
                            )
                        }
                    };
                    let Some((field, name)) = field else {
                        let package::Position::Map(key) = position else {
                            return session.fail(Error::State);
                        };
                        let ignored = output.nodes.as_mut_slice()[node]
                            .ignored
                            .push(key, session)?;
                        output.callbacks.push((node, ignored), session)?;
                        walk::run(child, value::Mode::Ignored, session)
                            .map_err(|stop| locate(stop, child.span.or(span)))?;
                        frames.push(
                            Frame::Fields {
                                cursor,
                                sequence,
                                node,
                            },
                            session,
                        )?;
                        continue;
                    };
                    if output.node(node).get(field).is_some() {
                        return session.reject(Diagnostic {
                            tag: Tag::Duplicate,
                            span,
                            expected: Some(name),
                        });
                    }
                    frames.push(
                        Frame::Fields {
                            cursor,
                            sequence: sequence.map(|n| n + 1),
                            node,
                        },
                        session,
                    )?;
                    match field {
                        Field::BuildOverride => {
                            frames.push(
                                Frame::Override {
                                    parent: node,
                                    position,
                                },
                                session,
                            )?;
                            frames.push(
                                Frame::Enter {
                                    input: child,
                                    location: Location::BuildOverride {
                                        parent: node,
                                        position,
                                    },
                                },
                                session,
                            )?;
                        }
                        Field::Package => {
                            let value::View::Map(cursor) = child.view(session)? else {
                                return session
                                    .reject(Diagnostic::at(Tag::Type, child.span.or(span)));
                            };
                            frames.push(
                                Frame::Packages {
                                    cursor,
                                    entries: map::Map::new(),
                                    parent: node,
                                    position,
                                    span: child.span,
                                },
                                session,
                            )?;
                        }
                        _ => {
                            let value = leaf(field, child, session)
                                .map_err(|stop| locate(stop, child.span.or(span)))?;
                            output.nodes.as_mut_slice()[node].entries.push(
                                Entry {
                                    field,
                                    position,
                                    value,
                                },
                                session,
                            )?;
                        }
                    }
                }
                Frame::Override { parent, position } => {
                    let Some(node) = completed.take() else {
                        return session.fail(Error::State);
                    };
                    output.nodes.as_mut_slice()[parent].entries.push(
                        Entry {
                            field: Field::BuildOverride,
                            position,
                            value: Value::BuildOverride(node),
                        },
                        session,
                    )?;
                }
                Frame::Packages {
                    mut cursor,
                    mut entries,
                    parent,
                    position,
                    span,
                } => {
                    if let Some((key, child)) = cursor.next(session)? {
                        let Some(key) = key else {
                            return session.fail(Error::State);
                        };
                        let spec = if key.text.as_str() == "*" {
                            PackageSpec::All
                        } else {
                            PackageSpec::Spec(spec::Spec::parse(
                                key.text.as_str(),
                                key.span.or(span),
                                session,
                            )?)
                        };
                        frames.push(
                            Frame::Packages {
                                cursor,
                                entries,
                                parent,
                                position,
                                span,
                            },
                            session,
                        )?;
                        frames.push(Frame::Package { spec, key }, session)?;
                        frames.push(
                            Frame::Enter {
                                input: child,
                                location: Location::Package {
                                    parent,
                                    position,
                                    key,
                                },
                            },
                            session,
                        )?;
                    } else {
                        entries.finish(Ord::cmp, session)?;
                        output.nodes.as_mut_slice()[parent].entries.push(
                            Entry {
                                field: Field::Package,
                                position,
                                value: Value::Package(Packages { entries }),
                            },
                            session,
                        )?;
                    }
                }
                Frame::Package { spec, key } => {
                    let Some(node) = completed.take() else {
                        return session.fail(Error::State);
                    };
                    let Some(Frame::Packages { entries, .. }) = frames.as_mut_slice().last_mut()
                    else {
                        return session.fail(Error::State);
                    };
                    entries.push(spec, key, node, session)?;
                }
            }
        }
        session.items(1)?;
        Ok(output)
    }
}

fn leaf<'d, A: document::Admission>(
    field: Field,
    input: value::Input<'_, 'd>,
    session: &mut session::Session<'_, A>,
) -> Step<Value<'d>> {
    let shape = match field {
        Field::OptLevel => return profile::Opt::interpret(input, session).map(Value::Opt),
        Field::Debug => return profile::Debug::interpret(input, session).map(Value::Debug),
        Field::CodegenUnits => return profile::units(input, session).map(Value::Units),
        Field::TrimPaths => return trim::Paths::interpret(input, session).map(Value::Trim),
        Field::Lto | Field::Strip => primitive::Shape::StringOrBool,
        Field::CodegenBackend
        | Field::SplitDebuginfo
        | Field::Panic
        | Field::DirName
        | Field::Inherits => primitive::Shape::String,
        Field::DebugAssertions
        | Field::Rpath
        | Field::OverflowChecks
        | Field::Incremental
        | Field::HintMostlyUnused => primitive::Shape::Boolean,
        Field::Rustflags => primitive::Shape::Strings,
        Field::Package | Field::BuildOverride => return session.fail(Error::State),
    };
    primitive::convert(input, shape, session).map(Value::Primitive)
}

fn locate(stop: Stop, span: Option<Span>) -> Stop {
    match stop {
        Stop::Rejected(mut diagnostic) => {
            diagnostic.span = diagnostic.span.or(span);
            Stop::Rejected(diagnostic)
        }
        Stop::Refused => Stop::Refused,
    }
}
