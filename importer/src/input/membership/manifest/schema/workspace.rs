//! Complete pinned TomlWorkspace interpretation, before workspace loading.

use rawr_toml::document;

use super::super::{session, storage, value, walk, Diagnostic, Error, Span, Step, Stop, Tag};
use super::{dependency, lints, package, primitive};

#[cfg(test)]
pub(in crate::input::membership::manifest) mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum Field {
    Members,
    Exclude,
    DefaultMembers,
    Resolver,
    Metadata,
    Package,
    Dependencies,
    Lints,
}

const FIELDS: [(Field, &str); 8] = [
    (Field::Members, "members"),
    (Field::Exclude, "exclude"),
    (Field::DefaultMembers, "default-members"),
    (Field::Resolver, "resolver"),
    (Field::Metadata, "metadata"),
    (Field::Package, "package"),
    (Field::Dependencies, "dependencies"),
    (Field::Lints, "lints"),
];

/// Converted workspace values retain their actual family and declaring data.
pub(in super::super) enum Value<'d> {
    Local(primitive::Value<'d>),
    Metadata(value::Projection<'d>),
    Package(package::Defaults<'d>),
    Dependencies(dependency::direct::Map<'d>),
    Lints(lints::Table<'d>),
}

pub(in super::super) struct Entry<'d> {
    field: Field,
    position: package::Position<'d>,
    span: Option<Span>,
    value: Value<'d>,
}

/// Relative callback paths preserve both the outer field and any nested
/// package position without rendering or allocating path strings.
#[derive(Clone, Copy, Debug)]
pub(in super::super) enum Ignored<'d> {
    Field(value::Key<'d>),
    Package {
        position: package::Position<'d>,
        path: package::Ignored<'d>,
    },
}

/// Complete original workspace data, with no member/source interpretation.
pub(in super::super) struct Workspace<'d> {
    entries: storage::Buffer<Entry<'d>>,
    ignored: storage::Buffer<Ignored<'d>>,
    span: Option<Span>,
}

impl<'d> Workspace<'d> {
    pub(in super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut output = Self {
            entries: storage::Buffer::new(),
            ignored: storage::Buffer::new(),
            span: input.span,
        };
        match input.view(session)? {
            value::View::Map(mut cursor) => {
                while let Some((key, child)) = cursor.next(session)? {
                    let Some(key) = key else {
                        return session.fail(Error::State);
                    };
                    if let Some((field, name)) =
                        FIELDS.iter().find(|(_, name)| *name == key.text.as_str())
                    {
                        if output.get(*field).is_some() {
                            return session.reject(Diagnostic {
                                tag: Tag::Duplicate,
                                span: input.span,
                                expected: Some(name),
                            });
                        }
                        output.field(child, *field, package::Position::Map(key), session)?;
                    } else {
                        output.ignored.push(Ignored::Field(key), session)?;
                        walk::run(child, value::Mode::Ignored, session)
                            .map_err(|stop| locate(stop, child.span.or(input.span)))?;
                    }
                }
            }
            value::View::Sequence(mut cursor) => {
                for (index, (field, name)) in FIELDS.iter().enumerate() {
                    let Some((_, child)) = cursor.next(session)? else {
                        return session.reject(Diagnostic {
                            tag: Tag::Missing,
                            span: input.span,
                            expected: Some(name),
                        });
                    };
                    output.field(child, *field, package::Position::Sequence(index), session)?;
                }
                cursor.finish(input.span, session)?;
            }
            _ => return session.reject(Diagnostic::at(Tag::Type, input.span)),
        }
        Ok(output)
    }

    fn field<A: document::Admission>(
        &mut self,
        input: value::Input<'_, 'd>,
        field: Field,
        position: package::Position<'d>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let value = match field {
            Field::Members | Field::Exclude | Field::DefaultMembers => {
                primitive::convert(input, primitive::Shape::Strings, session).map(Value::Local)
            }
            Field::Resolver => {
                primitive::convert(input, primitive::Shape::String, session).map(Value::Local)
            }
            Field::Metadata => walk::run(input, value::Mode::Toml, session).map(Value::Metadata),
            Field::Package => package::Defaults::interpret(input, session).map(Value::Package),
            Field::Dependencies => {
                dependency::direct::Map::interpret(input, session).map(Value::Dependencies)
            }
            Field::Lints => lints::Table::interpret(input, session).map(Value::Lints),
        }
        .map_err(|stop| locate(stop, input.span))?;
        if let Value::Package(package) = &value {
            for path in package.ignored() {
                // The outer record copies only borrowed/inline path components;
                // this retained copy has its own prospective Buffer admission.
                self.ignored.push(
                    Ignored::Package {
                        position,
                        path: *path,
                    },
                    session,
                )?;
            }
        }
        self.entries.push(
            Entry {
                field,
                position,
                span: input.span,
                value,
            },
            session,
        )?;
        Ok(())
    }

    pub(in super::super) fn entries(&self) -> &[Entry<'d>] {
        self.entries.as_slice()
    }
    pub(in super::super) fn get(&self, field: Field) -> Option<&Value<'d>> {
        self.entries
            .as_slice()
            .iter()
            .find(|entry| entry.field == field)
            .map(|entry| &entry.value)
    }
    pub(in super::super) fn ignored(&self) -> &[Ignored<'d>] {
        self.ignored.as_slice()
    }
    pub(in super::super) fn span(&self) -> Option<Span> {
        self.span
    }
}

impl<'d> Entry<'d> {
    pub(in super::super) fn field(&self) -> Field {
        self.field
    }
    pub(in super::super) fn position(&self) -> package::Position<'d> {
        self.position
    }
    pub(in super::super) fn span(&self) -> Option<Span> {
        self.span
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
