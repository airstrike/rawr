//! Complete pinned TomlPlatform fields, including separate underscore aliases.

use rawr_toml::document;

use super::super::{session, storage, value, walk, Diagnostic, Error, Span, Step, Stop, Tag};
use super::{dependency, package};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum Field {
    Dependencies,
    BuildDependencies,
    BuildDependencies2,
    DevDependencies,
    DevDependencies2,
}

const FIELDS: [(Field, &str); 5] = [
    (Field::Dependencies, "dependencies"),
    (Field::BuildDependencies, "build-dependencies"),
    (Field::BuildDependencies2, "build_dependencies"),
    (Field::DevDependencies, "dev-dependencies"),
    (Field::DevDependencies2, "dev_dependencies"),
];

pub(in super::super) struct Entry<'d> {
    field: Field,
    position: package::Position<'d>,
    span: Option<Span>,
    dependencies: dependency::inheritable::Map<'d>,
}

/// Converted dependencies remain in their original five distinct field slots.
pub(in super::super) struct Platform<'d> {
    entries: storage::Buffer<Entry<'d>>,
    ignored: storage::Buffer<value::Key<'d>>,
    span: Option<Span>,
}

impl<'d> Platform<'d> {
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
                        output.ignored.push(key, session)?;
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
        let dependencies = dependency::inheritable::Map::interpret(input, session)
            .map_err(|stop| locate(stop, input.span))?;
        self.entries.push(
            Entry {
                field,
                position,
                span: input.span,
                dependencies,
            },
            session,
        )?;
        Ok(())
    }

    pub(in super::super) fn entries(&self) -> &[Entry<'d>] {
        self.entries.as_slice()
    }
    pub(in super::super) fn get(&self, field: Field) -> Option<&dependency::inheritable::Map<'d>> {
        self.entries
            .as_slice()
            .iter()
            .find(|entry| entry.field == field)
            .map(|entry| &entry.dependencies)
    }
    pub(in super::super) fn ignored(&self) -> &[value::Key<'d>] {
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
    pub(in super::super) fn dependencies(&self) -> &dependency::inheritable::Map<'d> {
        &self.dependencies
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
