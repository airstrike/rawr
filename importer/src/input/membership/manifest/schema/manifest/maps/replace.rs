//! Replacement strings enclose direct dependencies without PackageIdSpec parsing.

use rawr_toml::document;

use crate::input::membership::manifest::schema::dependency;
use crate::input::membership::manifest::{session, value, Diagnostic, Error, Span, Step, Tag};

use super::{locate, table, Entry};

/// Complete unresolved replacements in exact original string-key order.
pub(in crate::input::membership::manifest) struct Replace<'d> {
    values: table::Table<'d, (), dependency::Direct<'d>>,
    span: Option<Span>,
}

impl<'d> Replace<'d> {
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
            let dependency = dependency::Direct::interpret(child, session)
                .map_err(|stop| locate(stop, child.span.or(input.span)))?;
            values.push((), key, dependency, child.span, session)?;
        }
        values.finish(session)?;
        Ok(Self {
            values,
            span: input.span,
        })
    }

    pub(in crate::input::membership::manifest) fn entries(
        &self,
    ) -> impl ExactSizeIterator<Item = &Entry<'d, (), dependency::Direct<'d>>> {
        self.values.entries()
    }
    pub(in crate::input::membership::manifest) fn span(&self) -> Option<Span> {
        self.span
    }
}
