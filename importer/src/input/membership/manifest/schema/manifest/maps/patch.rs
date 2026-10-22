//! Patch source strings enclose direct dependency maps without URL conversion.

use rawr_toml::document;

use crate::input::membership::manifest::schema::dependency;
use crate::input::membership::manifest::{session, value, Diagnostic, Error, Span, Step, Tag};

use super::{locate, table, Entry};

/// Complete unresolved patches in original source-string order.
pub(in crate::input::membership::manifest) struct Patch<'d> {
    values: table::Table<'d, (), dependency::direct::Map<'d>>,
    span: Option<Span>,
}

impl<'d> Patch<'d> {
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
            let dependencies = dependency::direct::Map::interpret(child, session)
                .map_err(|stop| locate(stop, child.span.or(input.span)))?;
            values.push((), key, dependencies, child.span, session)?;
        }
        values.finish(session)?;
        Ok(Self {
            values,
            span: input.span,
        })
    }

    pub(in crate::input::membership::manifest) fn entry(
        &self,
        index: usize,
    ) -> Option<&Entry<'d, (), dependency::direct::Map<'d>>> {
        self.values.entry(index)
    }

    pub(in crate::input::membership::manifest) fn entries(
        &self,
    ) -> impl ExactSizeIterator<Item = &Entry<'d, (), dependency::direct::Map<'d>>> {
        self.values.entries()
    }
    pub(in crate::input::membership::manifest) fn span(&self) -> Option<Span> {
        self.span
    }
}
