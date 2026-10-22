//! Complete top-level typed manifest fields over the original value adapter.

pub(super) mod maps;
pub(in super::super) use maps::{Features, Targets};
mod unused;

#[cfg(test)]
mod resources;
#[cfg(test)]
mod tests;

use rawr_toml::document;

use super::super::{session, storage, value, walk, Diagnostic, Error, Span, Step, Stop, Tag};
use super::{dependency, hints, lints, package, primitive, profile, workspace, Target};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum Field {
    CargoFeatures,
    Package,
    Project,
    Badges,
    Features,
    Lib,
    Bin,
    Example,
    Test,
    Bench,
    Dependencies,
    DevDependencies,
    DevDependencies2,
    BuildDependencies,
    BuildDependencies2,
    Target,
    Lints,
    Hints,
    Workspace,
    Profile,
    Patch,
    Replace,
}

const FIELDS: [(Field, &str); 22] = [
    (Field::CargoFeatures, "cargo-features"),
    (Field::Package, "package"),
    (Field::Project, "project"),
    (Field::Badges, "badges"),
    (Field::Features, "features"),
    (Field::Lib, "lib"),
    (Field::Bin, "bin"),
    (Field::Example, "example"),
    (Field::Test, "test"),
    (Field::Bench, "bench"),
    (Field::Dependencies, "dependencies"),
    (Field::DevDependencies, "dev-dependencies"),
    (Field::DevDependencies2, "dev_dependencies"),
    (Field::BuildDependencies, "build-dependencies"),
    (Field::BuildDependencies2, "build_dependencies"),
    (Field::Target, "target"),
    (Field::Lints, "lints"),
    (Field::Hints, "hints"),
    (Field::Workspace, "workspace"),
    (Field::Profile, "profile"),
    (Field::Patch, "patch"),
    (Field::Replace, "replace"),
];

pub(in super::super) enum Value<'d> {
    Primitive(primitive::Value<'d>),
    Package(package::Package<'d>),
    Features(maps::Features<'d>),
    Target(Target<'d>),
    Targets(maps::Targets<'d>),
    Dependencies(dependency::inheritable::Map<'d>),
    Platforms(maps::Platforms<'d>),
    Lints(lints::Inherited<'d>),
    Hints(hints::Hints<'d>),
    Workspace(workspace::Workspace<'d>),
    Profiles(profile::Profiles<'d>),
    Patch(maps::Patch<'d>),
    Replace(maps::Replace<'d>),
}

pub(in super::super) struct Entry<'d> {
    field: Field,
    position: package::Position<'d>,
    value: Value<'d>,
}

enum Event<'d> {
    Field(usize),
    Ignored(value::Key<'d>),
}

/// Typed, unresolved input. This does not establish workspace or package selection.
pub(in super::super) struct Original<'d> {
    entries: storage::Buffer<Entry<'d>>,
    events: storage::Buffer<Event<'d>>,
    unused: unused::Unused,
    span: Option<Span>,
}

impl<'d> Original<'d> {
    pub(in super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut output = Self {
            entries: storage::Buffer::new(),
            events: storage::Buffer::new(),
            unused: unused::Unused::new(),
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
                        // The skipped _unused_keys member is not a deserialize field.
                        output.events.push(Event::Ignored(key), session)?;
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
        output.unused = unused::collect(&output, session)?;
        Ok(output)
    }

    fn field<A: document::Admission>(
        &mut self,
        input: value::Input<'_, 'd>,
        field: Field,
        position: package::Position<'d>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let converted = convert(input, field, session)
            .map_err(|stop| locate(stop, input.span.or(self.span)))?;
        let index = self.entries.push(
            Entry {
                field,
                position,
                value: converted,
            },
            session,
        )?;
        self.events.push(Event::Field(index), session)?;
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
    pub(in super::super) fn unused(&self) -> &unused::Unused {
        &self.unused
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
    pub(in super::super) fn value(&self) -> &Value<'d> {
        &self.value
    }
}

fn convert<'d, A: document::Admission>(
    input: value::Input<'_, 'd>,
    field: Field,
    session: &mut session::Session<'_, A>,
) -> Step<Value<'d>> {
    Ok(match field {
        Field::CargoFeatures => Value::Primitive(primitive::convert(
            input,
            primitive::Shape::Strings,
            session,
        )?),
        Field::Badges => Value::Primitive(primitive::convert(
            input,
            primitive::Shape::Badges,
            session,
        )?),
        Field::Package | Field::Project => {
            Value::Package(package::Package::interpret(input, session)?)
        }
        Field::Features => Value::Features(maps::Features::interpret(input, session)?),
        Field::Lib => Value::Target(Target::interpret(input, session)?),
        Field::Bin | Field::Example | Field::Test | Field::Bench => {
            Value::Targets(maps::Targets::interpret(input, session)?)
        }
        Field::Dependencies
        | Field::DevDependencies
        | Field::DevDependencies2
        | Field::BuildDependencies
        | Field::BuildDependencies2 => {
            Value::Dependencies(dependency::inheritable::Map::interpret(input, session)?)
        }
        Field::Target => Value::Platforms(maps::Platforms::interpret(input, session)?),
        Field::Lints => Value::Lints(lints::Inherited::interpret(input, session)?),
        Field::Hints => Value::Hints(hints::Hints::interpret(input, session)?),
        Field::Workspace => Value::Workspace(workspace::Workspace::interpret(input, session)?),
        Field::Profile => Value::Profiles(profile::Profiles::interpret(input, session)?),
        Field::Patch => Value::Patch(maps::Patch::interpret(input, session)?),
        Field::Replace => Value::Replace(maps::Replace::interpret(input, session)?),
    })
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
