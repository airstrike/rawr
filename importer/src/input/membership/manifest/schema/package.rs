//! Complete pinned TomlPackage and InheritablePackage visitors.
//!
//! Cargo schema mod.rs:146–166 and176–225 declares 17 default fields and33
//! package fields. All are Option fields, but derive's positional visitor has
//! no missing-element defaults (serde_derive de.rs:451–551). Package sequences
//! therefore eventually reach the forbidden final cargo-features slot.

use rawr_toml::document;

use super::super::{session, storage, value, walk, Diagnostic, Span, Step, Tag};
use super::primitive;

mod inherited;

#[cfg(test)]
pub(in crate::input::membership::manifest) mod tests;

/// The exact field identities shared by package and workspace defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum Field {
    Edition,
    RustVersion,
    Name,
    Version,
    Authors,
    Build,
    Metabuild,
    DefaultTarget,
    ForcedTarget,
    Links,
    Exclude,
    Include,
    Publish,
    Workspace,
    ImATeapot,
    Autolib,
    Autobins,
    Autoexamples,
    Autotests,
    Autobenches,
    DefaultRun,
    Description,
    Homepage,
    Documentation,
    Readme,
    Keywords,
    Categories,
    License,
    LicenseFile,
    Repository,
    Resolver,
    Metadata,
    CargoFeatures,
    Badges,
}

#[derive(Clone, Copy)]
struct Definition {
    field: Field,
    name: &'static str,
    shape: Shape,
}

#[derive(Clone, Copy)]
enum Shape {
    Value(primitive::Shape),
    Inheritable(primitive::Shape),
    Metadata,
    Forbidden,
}

const PACKAGE: [Definition; 33] = [
    Definition {
        field: Field::Edition,
        name: "edition",
        shape: Shape::Inheritable(primitive::Shape::String),
    },
    Definition {
        field: Field::RustVersion,
        name: "rust-version",
        shape: Shape::Inheritable(primitive::Shape::Rust),
    },
    Definition {
        field: Field::Name,
        name: "name",
        shape: Shape::Value(primitive::Shape::Name),
    },
    Definition {
        field: Field::Version,
        name: "version",
        shape: Shape::Inheritable(primitive::Shape::Version { trim: true }),
    },
    Definition {
        field: Field::Authors,
        name: "authors",
        shape: Shape::Inheritable(primitive::Shape::Strings),
    },
    Definition {
        field: Field::Build,
        name: "build",
        shape: Shape::Value(primitive::Shape::Build),
    },
    Definition {
        field: Field::Metabuild,
        name: "metabuild",
        shape: Shape::Value(primitive::Shape::StringOrStrings),
    },
    Definition {
        field: Field::DefaultTarget,
        name: "default-target",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::ForcedTarget,
        name: "forced-target",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::Links,
        name: "links",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::Exclude,
        name: "exclude",
        shape: Shape::Inheritable(primitive::Shape::Strings),
    },
    Definition {
        field: Field::Include,
        name: "include",
        shape: Shape::Inheritable(primitive::Shape::Strings),
    },
    Definition {
        field: Field::Publish,
        name: "publish",
        shape: Shape::Inheritable(primitive::Shape::StringsOrBool),
    },
    Definition {
        field: Field::Workspace,
        name: "workspace",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::ImATeapot,
        name: "im-a-teapot",
        shape: Shape::Value(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Autolib,
        name: "autolib",
        shape: Shape::Value(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Autobins,
        name: "autobins",
        shape: Shape::Value(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Autoexamples,
        name: "autoexamples",
        shape: Shape::Value(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Autotests,
        name: "autotests",
        shape: Shape::Value(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Autobenches,
        name: "autobenches",
        shape: Shape::Value(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::DefaultRun,
        name: "default-run",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::Description,
        name: "description",
        shape: Shape::Inheritable(primitive::Shape::String),
    },
    Definition {
        field: Field::Homepage,
        name: "homepage",
        shape: Shape::Inheritable(primitive::Shape::String),
    },
    Definition {
        field: Field::Documentation,
        name: "documentation",
        shape: Shape::Inheritable(primitive::Shape::String),
    },
    Definition {
        field: Field::Readme,
        name: "readme",
        shape: Shape::Inheritable(primitive::Shape::StringOrBool),
    },
    Definition {
        field: Field::Keywords,
        name: "keywords",
        shape: Shape::Inheritable(primitive::Shape::Strings),
    },
    Definition {
        field: Field::Categories,
        name: "categories",
        shape: Shape::Inheritable(primitive::Shape::Strings),
    },
    Definition {
        field: Field::License,
        name: "license",
        shape: Shape::Inheritable(primitive::Shape::String),
    },
    Definition {
        field: Field::LicenseFile,
        name: "license-file",
        shape: Shape::Inheritable(primitive::Shape::String),
    },
    Definition {
        field: Field::Repository,
        name: "repository",
        shape: Shape::Inheritable(primitive::Shape::String),
    },
    Definition {
        field: Field::Resolver,
        name: "resolver",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::Metadata,
        name: "metadata",
        shape: Shape::Metadata,
    },
    Definition {
        field: Field::CargoFeatures,
        name: "cargo-features",
        shape: Shape::Forbidden,
    },
];

const DEFAULTS: [Definition; 17] = [
    Definition {
        field: Field::Version,
        name: "version",
        shape: Shape::Value(primitive::Shape::Version { trim: false }),
    },
    Definition {
        field: Field::Authors,
        name: "authors",
        shape: Shape::Value(primitive::Shape::Strings),
    },
    Definition {
        field: Field::Description,
        name: "description",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::Homepage,
        name: "homepage",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::Documentation,
        name: "documentation",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::Readme,
        name: "readme",
        shape: Shape::Value(primitive::Shape::StringOrBool),
    },
    Definition {
        field: Field::Keywords,
        name: "keywords",
        shape: Shape::Value(primitive::Shape::Strings),
    },
    Definition {
        field: Field::Categories,
        name: "categories",
        shape: Shape::Value(primitive::Shape::Strings),
    },
    Definition {
        field: Field::License,
        name: "license",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::LicenseFile,
        name: "license-file",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::Repository,
        name: "repository",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::Publish,
        name: "publish",
        shape: Shape::Value(primitive::Shape::StringsOrBool),
    },
    Definition {
        field: Field::Edition,
        name: "edition",
        shape: Shape::Value(primitive::Shape::String),
    },
    Definition {
        field: Field::Badges,
        name: "badges",
        shape: Shape::Value(primitive::Shape::Badges),
    },
    Definition {
        field: Field::Exclude,
        name: "exclude",
        shape: Shape::Value(primitive::Shape::Strings),
    },
    Definition {
        field: Field::Include,
        name: "include",
        shape: Shape::Value(primitive::Shape::Strings),
    },
    Definition {
        field: Field::RustVersion,
        name: "rust-version",
        shape: Shape::Value(primitive::Shape::Rust),
    },
];

/// Relative Cargo callback path. Cargo stringify skips Some/newtype wrappers
/// (util/toml/mod.rs:207–232); map spellings and sequence indices remain exact.
#[derive(Clone, Copy, Debug)]
pub(in super::super) enum Position<'d> {
    Map(value::Key<'d>),
    Sequence(usize),
}

#[derive(Clone, Copy, Debug)]
pub(in super::super) struct Ignored<'d> {
    parent: Option<Position<'d>>,
    key: value::Key<'d>,
}

impl<'d> Ignored<'d> {
    pub(in super::super) fn parent(&self) -> Option<Position<'d>> {
        self.parent
    }
    pub(in super::super) fn key(&self) -> &value::Key<'d> {
        &self.key
    }
}

pub(in super::super) enum Value<'d> {
    Local(primitive::Value<'d>),
    Inherited,
    Metadata(value::Projection<'d>),
}

pub(in super::super) struct Entry<'d> {
    field: Field,
    position: Position<'d>,
    span: Option<Span>,
    value: Value<'d>,
}

impl<'d> Entry<'d> {
    pub(in super::super) fn field(&self) -> Field {
        self.field
    }
    pub(in super::super) fn position(&self) -> Position<'d> {
        self.position
    }
    pub(in super::super) fn span(&self) -> Option<Span> {
        self.span
    }
    pub(in super::super) fn value(&self) -> &Value<'d> {
        &self.value
    }
}

/// Complete package interpretation, still unresolved and associated with the
/// declaring Document through borrowed payloads and retained input spans.
pub(in super::super) struct Package<'d> {
    data: Data<'d>,
}
pub(in super::super) struct Defaults<'d> {
    data: Data<'d>,
}

struct Data<'d> {
    entries: storage::Buffer<Entry<'d>>,
    ignored: storage::Buffer<Ignored<'d>>,
    span: Option<Span>,
}

impl<'d> Package<'d> {
    pub(in super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        Data::interpret(input, &PACKAGE, session).map(|data| Self { data })
    }
    pub(in super::super) fn entries(&self) -> &[Entry<'d>] {
        self.data.entries.as_slice()
    }
    pub(in super::super) fn get(&self, field: Field) -> Option<&Value<'d>> {
        self.data.get(field)
    }
    pub(in super::super) fn ignored(&self) -> &[Ignored<'d>] {
        self.data.ignored.as_slice()
    }
    pub(in super::super) fn span(&self) -> Option<Span> {
        self.data.span
    }
}

impl<'d> Defaults<'d> {
    pub(in super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        Data::interpret(input, &DEFAULTS, session).map(|data| Self { data })
    }
    pub(in super::super) fn entries(&self) -> &[Entry<'d>] {
        self.data.entries.as_slice()
    }
    pub(in super::super) fn get(&self, field: Field) -> Option<&Value<'d>> {
        self.data.get(field)
    }
    pub(in super::super) fn ignored(&self) -> &[Ignored<'d>] {
        self.data.ignored.as_slice()
    }
}

impl<'d> Data<'d> {
    fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        definitions: &[Definition],
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut data = Self {
            entries: storage::Buffer::new(),
            ignored: storage::Buffer::new(),
            span: input.span,
        };
        match input.view(session)? {
            value::View::Map(mut cursor) => {
                while let Some((key, input)) = cursor.next(session)? {
                    let Some(key) = key else {
                        return session.fail(super::super::Error::State);
                    };
                    match definitions
                        .iter()
                        .find(|field| field.name == key.text.as_str())
                    {
                        Some(field) => {
                            if data.get(field.field).is_some() {
                                return session.reject(Diagnostic {
                                    tag: Tag::Duplicate,
                                    span: data.span,
                                    expected: Some(field.name),
                                });
                            }
                            data.field(input, *field, Position::Map(key), session)?;
                        }
                        None => ignore(input, None, key, &mut data.ignored, session)?,
                    }
                }
            }
            value::View::Sequence(mut cursor) => {
                for (index, field) in definitions.iter().enumerate() {
                    let Some((_, input)) = cursor.next(session)? else {
                        return session.reject(Diagnostic {
                            tag: Tag::Missing,
                            span: data.span,
                            expected: Some(field.name),
                        });
                    };
                    data.field(input, *field, Position::Sequence(index), session)?;
                }
                cursor.finish(data.span, session)?;
            }
            _ => return session.reject(Diagnostic::at(Tag::Type, input.span)),
        }
        Ok(data)
    }

    fn field<A: document::Admission>(
        &mut self,
        input: value::Input<'_, 'd>,
        field: Definition,
        position: Position<'d>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let converted = match field.shape {
            Shape::Value(shape) => Value::Local(primitive::convert(input, shape, session)?),
            Shape::Inheritable(shape) => match input.view(session)? {
                value::View::Map(cursor) => {
                    let result = inherited::interpret(
                        cursor,
                        input.span,
                        position,
                        &mut self.ignored,
                        session,
                    );
                    if matches!(shape, primitive::Shape::Version { .. }) {
                        result.map_err(|stop| primitive::erase(stop, input.span))?;
                    } else {
                        result?;
                    }
                    Value::Inherited
                }
                view => Value::Local(primitive::from_view(view, shape, input.span, session)?),
            },
            Shape::Metadata => Value::Metadata(walk::run(input, value::Mode::Toml, session)?),
            // InvalidCargoFeatures::deserialize never calls its deserializer.
            // Even dispatcher overflow or malformed generic content is not read.
            Shape::Forbidden => {
                return session.reject(Diagnostic::at(Tag::CargoFeatures, input.span))
            }
        };
        self.entries.push(
            Entry {
                field: field.field,
                position,
                span: input.span,
                value: converted,
            },
            session,
        )?;
        Ok(())
    }

    fn get(&self, field: Field) -> Option<&Value<'d>> {
        self.entries
            .as_slice()
            .iter()
            .find(|entry| entry.field == field)
            .map(|entry| &entry.value)
    }
}

fn ignore<'d, A: document::Admission>(
    input: value::Input<'_, 'd>,
    parent: Option<Position<'d>>,
    key: value::Key<'d>,
    ignored: &mut storage::Buffer<Ignored<'d>>,
    session: &mut session::Session<'_, A>,
) -> Step<()> {
    // serde_ignored invokes its callback before asking the source to ignore.
    ignored.push(Ignored { parent, key }, session)?;
    walk::run(input, value::Mode::Ignored, session)?;
    Ok(())
}
