//! Complete target vectors and their original sequence callback positions.

use rawr_toml::document;

use crate::input::membership::manifest::schema;
use crate::input::membership::manifest::{session, storage, value, Diagnostic, Span, Step, Tag};

use super::locate;

/// One complete target at its original array position.
pub(in crate::input::membership::manifest) struct Entry<'d> {
    target: schema::Target<'d>,
    span: Option<Span>,
}

/// Original array index and ignored field, independent of the target payload.
#[derive(Clone, Copy)]
pub(in crate::input::membership::manifest) struct Ignored<'d> {
    index: usize,
    field: value::Key<'d>,
}

/// All target instances in encounter order, including complete ignored values.
pub(in crate::input::membership::manifest) struct Targets<'d> {
    values: storage::Buffer<Entry<'d>>,
    ignored: storage::Buffer<Ignored<'d>>,
    span: Option<Span>,
}

impl<'d> Targets<'d> {
    pub(in crate::input::membership::manifest) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let value::View::Sequence(mut cursor) = input.view(session)? else {
            return session.reject(Diagnostic::at(Tag::Type, input.span));
        };
        let mut values = storage::Buffer::new();
        let mut ignored = storage::Buffer::new();
        while let Some((_, child)) = cursor.next(session)? {
            let target = schema::Target::interpret(child, session)
                .map_err(|stop| locate(stop, child.span.or(input.span)))?;
            for &field in target.unused() {
                ignored.push(
                    Ignored {
                        index: values.len(),
                        field,
                    },
                    session,
                )?;
            }
            values.push(
                Entry {
                    target,
                    span: child.span,
                },
                session,
            )?;
        }
        Ok(Self {
            values,
            ignored,
            span: input.span,
        })
    }

    pub(in crate::input::membership::manifest) fn entries(&self) -> &[Entry<'d>] {
        self.values.as_slice()
    }
    pub(in crate::input::membership::manifest) fn ignored(&self) -> &[Ignored<'d>] {
        self.ignored.as_slice()
    }
    pub(in crate::input::membership::manifest) fn span(&self) -> Option<Span> {
        self.span
    }
}

impl<'d> Entry<'d> {
    pub(in crate::input::membership::manifest) fn target(&self) -> &schema::Target<'d> {
        &self.target
    }
    pub(in crate::input::membership::manifest) fn span(&self) -> Option<Span> {
        self.span
    }
}

impl<'d> Ignored<'d> {
    pub(in crate::input::membership::manifest) fn index(&self) -> usize {
        self.index
    }
    pub(in crate::input::membership::manifest) fn field(&self) -> &value::Key<'d> {
        &self.field
    }
}
