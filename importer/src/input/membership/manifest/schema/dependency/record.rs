//! Complete derive map visitors: known fields, missing fields, then flatten.
//! Source: Cargo schema mod.rs:718–731,825–869 and serde_derive1.0.228
//! de/struct_.rs:225–371. Flatten disables positional struct sequences.

use rawr_toml::document;

use super::super::super::{
    scalar, session, storage, value, walk, Diagnostic, Error, Span, Step, Tag,
};
use super::super::primitive;
use super::map;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super::super) enum Field {
    Version,
    Registry,
    RegistryIndex,
    Path,
    Base,
    Git,
    Branch,
    Tag,
    Rev,
    Features,
    Optional,
    DefaultFeatures,
    DefaultFeatures2,
    Package,
    Public,
    Artifact,
    Lib,
    Target,
    Workspace,
}

#[derive(Clone, Copy)]
struct Definition {
    field: Field,
    name: &'static str,
    shape: Shape,
}
#[derive(Clone, Copy)]
enum Shape {
    Primitive(primitive::Shape),
    Name(scalar::Kind),
}

const DETAILED: [Definition; 18] = [
    Definition {
        field: Field::Version,
        name: "version",
        shape: Shape::Primitive(primitive::Shape::String),
    },
    Definition {
        field: Field::Registry,
        name: "registry",
        shape: Shape::Name(scalar::Kind::Registry),
    },
    Definition {
        field: Field::RegistryIndex,
        name: "registry-index",
        shape: Shape::Primitive(primitive::Shape::String),
    },
    Definition {
        field: Field::Path,
        name: "path",
        shape: Shape::Primitive(primitive::Shape::String),
    },
    Definition {
        field: Field::Base,
        name: "base",
        shape: Shape::Name(scalar::Kind::PathBase),
    },
    Definition {
        field: Field::Git,
        name: "git",
        shape: Shape::Primitive(primitive::Shape::String),
    },
    Definition {
        field: Field::Branch,
        name: "branch",
        shape: Shape::Primitive(primitive::Shape::String),
    },
    Definition {
        field: Field::Tag,
        name: "tag",
        shape: Shape::Primitive(primitive::Shape::String),
    },
    Definition {
        field: Field::Rev,
        name: "rev",
        shape: Shape::Primitive(primitive::Shape::String),
    },
    Definition {
        field: Field::Features,
        name: "features",
        shape: Shape::Primitive(primitive::Shape::Strings),
    },
    Definition {
        field: Field::Optional,
        name: "optional",
        shape: Shape::Primitive(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::DefaultFeatures,
        name: "default-features",
        shape: Shape::Primitive(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::DefaultFeatures2,
        name: "default_features",
        shape: Shape::Primitive(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Package,
        name: "package",
        shape: Shape::Name(scalar::Kind::Package),
    },
    Definition {
        field: Field::Public,
        name: "public",
        shape: Shape::Primitive(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Artifact,
        name: "artifact",
        shape: Shape::Primitive(primitive::Shape::StringOrStrings),
    },
    Definition {
        field: Field::Lib,
        name: "lib",
        shape: Shape::Primitive(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Target,
        name: "target",
        shape: Shape::Primitive(primitive::Shape::String),
    },
];

const INHERITED: [Definition; 6] = [
    Definition {
        field: Field::Workspace,
        name: "workspace",
        shape: Shape::Primitive(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Features,
        name: "features",
        shape: Shape::Primitive(primitive::Shape::Strings),
    },
    Definition {
        field: Field::DefaultFeatures,
        name: "default-features",
        shape: Shape::Primitive(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::DefaultFeatures2,
        name: "default_features",
        shape: Shape::Primitive(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Optional,
        name: "optional",
        shape: Shape::Primitive(primitive::Shape::Boolean),
    },
    Definition {
        field: Field::Public,
        name: "public",
        shape: Shape::Primitive(primitive::Shape::Boolean),
    },
];

pub(in super::super::super) struct Entry<'d> {
    field: Field,
    value: primitive::Value<'d>,
}

impl<'d> Entry<'d> {
    pub(in super::super::super) fn field(&self) -> Field {
        self.field
    }
    pub(in super::super::super) fn value(&self) -> &primitive::Value<'d> {
        &self.value
    }
}

pub(in super::super::super) struct Detailed<'d> {
    data: Data<'d>,
}
pub(in super::super::super) struct Inherited<'d> {
    data: Data<'d>,
    workspace: bool,
}

struct Data<'d> {
    fields: storage::Buffer<Entry<'d>>,
    extras: map::Map<'d, (), value::Projection<'d>>,
    span: Option<Span>,
}

struct Captured<'d> {
    key: value::Key<'d>,
    value: value::Projection<'d>,
}

impl<'d> Detailed<'d> {
    pub(in super::super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let value::View::Map(cursor) = input.view(session)? else {
            return session.reject(Diagnostic::at(Tag::Type, input.span));
        };
        Self::from_map(cursor, input.span, session)
    }

    pub(super) fn from_map<A: document::Admission>(
        cursor: value::Cursor<'_, 'd>,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        Data::interpret(cursor, span, &DETAILED, false, session).map(|data| Self { data })
    }
    pub(in super::super::super) fn fields(&self) -> &[Entry<'d>] {
        self.data.fields.as_slice()
    }
    pub(in super::super::super) fn get(&self, field: Field) -> Option<&primitive::Value<'d>> {
        self.data.get(field)
    }
    pub(in super::super::super) fn extras(
        &self,
    ) -> impl ExactSizeIterator<Item = (&value::Key<'d>, &value::Projection<'d>)> {
        self.data
            .extras
            .entries()
            .map(|(_, key, value)| (key, value))
    }
}

impl<'d> Inherited<'d> {
    pub(in super::super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let value::View::Map(cursor) = input.view(session)? else {
            return session.reject(Diagnostic::at(Tag::Type, input.span));
        };
        let data = Data::interpret(cursor, input.span, &INHERITED, true, session)?;
        let Some(primitive::Value::Boolean(workspace)) = data.get(Field::Workspace) else {
            return session.fail(Error::State);
        };
        let workspace = *workspace;
        Ok(Self { data, workspace })
    }
    pub(in super::super::super) fn workspace(&self) -> bool {
        self.workspace
    }
    pub(in super::super::super) fn fields(&self) -> &[Entry<'d>] {
        self.data.fields.as_slice()
    }
    pub(in super::super::super) fn get(&self, field: Field) -> Option<&primitive::Value<'d>> {
        self.data.get(field)
    }
    pub(in super::super::super) fn extras(
        &self,
    ) -> impl ExactSizeIterator<Item = (&value::Key<'d>, &value::Projection<'d>)> {
        self.data
            .extras
            .entries()
            .map(|(_, key, value)| (key, value))
    }
    pub(in super::super::super) fn span(&self) -> Option<Span> {
        self.data.span
    }
}

impl<'d> Data<'d> {
    fn interpret<A: document::Admission>(
        mut cursor: value::Cursor<'_, 'd>,
        span: Option<Span>,
        definitions: &[Definition],
        required_workspace: bool,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let mut data = Self {
            fields: storage::Buffer::new(),
            extras: map::Map::new(),
            span,
        };
        let mut captured = storage::Buffer::new();
        while let Some((key, input)) = cursor.next(session)? {
            let Some(key) = key else {
                return session.fail(Error::State);
            };
            if let Some(definition) = definitions
                .iter()
                .find(|field| field.name == key.text.as_str())
            {
                if data.get(definition.field).is_some() {
                    return session.reject(Diagnostic {
                        tag: Tag::Duplicate,
                        span,
                        expected: Some(definition.name),
                    });
                }
                let value = match definition.shape {
                    Shape::Primitive(shape) => primitive::convert(input, shape, session)?,
                    Shape::Name(kind) => {
                        let value::View::Scalar(value::Kind::String(text)) = input.view(session)?
                        else {
                            return session.reject(Diagnostic::at(Tag::Type, input.span));
                        };
                        primitive::Value::Name(primitive::Name::parse(
                            text, kind, input.span, session,
                        )?)
                    }
                };
                data.fields.push(
                    Entry {
                        field: definition.field,
                        value,
                    },
                    session,
                )?;
            } else {
                let value = walk::run(input, value::Mode::Content, session)?;
                captured.push(Captured { key, value }, session)?;
            }
        }
        if required_workspace && data.get(Field::Workspace).is_none() {
            return session.reject(Diagnostic {
                tag: Tag::Missing,
                span,
                expected: Some("workspace"),
            });
        }
        // Unknown fields were captured before missing-field checks; their
        // generic TOML conversion runs only now through borrowed ContentRef.
        for captured in captured.as_slice() {
            session.string(captured.key.text.as_str().len())?;
            let value = walk::run(
                value::Input::replay(&captured.value, 0),
                value::Mode::Toml,
                session,
            )?;
            data.extras.push((), captured.key, value, session)?;
        }
        data.extras.finish(session)?;
        Ok(data)
    }

    fn get(&self, field: Field) -> Option<&primitive::Value<'d>> {
        self.fields
            .as_slice()
            .iter()
            .find(|entry| entry.field == field)
            .map(|entry| &entry.value)
    }
}
