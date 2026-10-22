//! Validated FeatureName keys and complete string-vector values.

use rawr_toml::document;

use crate::input::membership::manifest::schema::primitive;
use crate::input::membership::manifest::{
    scalar, session, storage, value, Diagnostic, Error, Span, Step, Tag,
};

use super::{locate, table, Entry};

/// Complete original feature data in Cargo's validated string-key order.
pub(in crate::input::membership::manifest) struct Features<'d> {
    values: table::Table<'d, primitive::Name<'d>, storage::Buffer<value::Text<'d>>>,
    span: Option<Span>,
}

impl<'d> Features<'d> {
    pub(in crate::input::membership::manifest) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let value::View::Map(mut cursor) = input.view(session)? else {
            return session.reject(Diagnostic::at(Tag::Type, input.span));
        };
        let mut values = table::Table::new();
        while let Some((key, child)) = cursor.next(session)? {
            let Some(key) = key else {
                return session.fail(Error::State);
            };
            let name = primitive::Name::parse(
                key.text,
                scalar::Kind::Feature,
                key.span.or(input.span),
                session,
            )?;
            let converted = primitive::convert(child, primitive::Shape::Strings, session)
                .map_err(|stop| locate(stop, child.span.or(input.span)))?;
            let primitive::Value::Strings(strings) = converted else {
                return session.fail(Error::State);
            };
            values.push(name, key, strings, child.span, session)?;
        }
        values.finish(session)?;
        Ok(Self {
            values,
            span: input.span,
        })
    }

    pub(in crate::input::membership::manifest) fn entries(
        &self,
    ) -> impl ExactSizeIterator<Item = &Entry<'d, primitive::Name<'d>, storage::Buffer<value::Text<'d>>>>
    {
        self.values.entries()
    }
    pub(in crate::input::membership::manifest) fn span(&self) -> Option<Span> {
        self.span
    }
}
