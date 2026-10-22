//! Complete string-to-Platform map, with callback records outside replacement.

use rawr_toml::document;

use crate::input::membership::manifest::schema::platform;
use crate::input::membership::manifest::{
    session, storage, value, Diagnostic, Error, Span, Step, Tag,
};

use super::{locate, table, Entry};

/// An original map position and its ignored Platform field.
#[derive(Clone, Copy)]
pub(in crate::input::membership::manifest) struct Ignored<'d> {
    parent: value::Key<'d>,
    field: value::Key<'d>,
}

/// Converted platforms plus every callback, including replaced values' paths.
pub(in crate::input::membership::manifest) struct Platforms<'d> {
    values: table::Table<'d, (), platform::Platform<'d>>,
    ignored: storage::Buffer<Ignored<'d>>,
    span: Option<Span>,
}

impl<'d> Platforms<'d> {
    pub(in crate::input::membership::manifest) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let value::View::Map(mut cursor) = input.view(session)? else {
            return session.reject(Diagnostic::at(Tag::Type, input.span));
        };
        let mut values = table::Table::new();
        let mut ignored = storage::Buffer::new();
        while let Some((key, child)) = cursor.next(session)? {
            let Some(key) = key else {
                return session.fail(Error::State);
            };
            let platform = platform::Platform::interpret(child, session)
                .map_err(|stop| locate(stop, child.span.or(input.span)))?;
            for &field in platform.ignored() {
                ignored.push(Ignored { parent: key, field }, session)?;
            }
            values.push((), key, platform, child.span, session)?;
        }
        values.finish(session)?;
        Ok(Self {
            values,
            ignored,
            span: input.span,
        })
    }

    pub(in crate::input::membership::manifest) fn entries(
        &self,
    ) -> impl ExactSizeIterator<Item = &Entry<'d, (), platform::Platform<'d>>> {
        self.values.entries()
    }
    pub(in crate::input::membership::manifest) fn entry(
        &self,
        index: usize,
    ) -> Option<&Entry<'d, (), platform::Platform<'d>>> {
        self.values.entry(index)
    }
    pub(in crate::input::membership::manifest) fn ignored(&self) -> &[Ignored<'d>] {
        self.ignored.as_slice()
    }
    pub(in crate::input::membership::manifest) fn span(&self) -> Option<Span> {
        self.span
    }
}

impl<'d> Ignored<'d> {
    pub(in crate::input::membership::manifest) fn parent(&self) -> &value::Key<'d> {
        &self.parent
    }
    pub(in crate::input::membership::manifest) fn field(&self) -> &value::Key<'d> {
        &self.field
    }
}
