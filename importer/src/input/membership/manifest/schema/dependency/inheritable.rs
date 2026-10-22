//! Complete initial capture and sorted inherited trial before semantic fallback.

use rawr_toml::document;

use super::super::super::{scalar, session, value, walk, Diagnostic, Error, Step, Stop, Tag};
use super::super::primitive;
use super::{direct, map, record, Direct};

pub(in super::super::super) enum Kind<'d> {
    Value(Direct<'d>),
    Inherit(record::Inherited<'d>),
}

pub(in super::super::super) struct Inheritable<'d> {
    kind: Kind<'d>,
    span: Option<super::super::super::Span>,
}

pub(in super::super::super) struct Map<'d> {
    values: map::Map<'d, primitive::Name<'d>, Inheritable<'d>>,
}

impl<'d> Inheritable<'d> {
    pub(in super::super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        // Capture finishes in original encounter order before the trial sees
        // serde-value's sorted maps. Borrowing replaces its unbounded clone.
        let captured = walk::run(input, value::Mode::SerdeValue, session)?;
        let replay = value::Input::replay(&captured, 0);
        let kind = match record::Inherited::interpret(replay, session) {
            Ok(value) => {
                // A false flag rejects only after all known fields, required
                // fields and flattened extras have successfully completed.
                if value.workspace() {
                    Kind::Inherit(value)
                } else {
                    return session.reject(Diagnostic::at(Tag::WorkspaceFalse, input.span));
                }
            }
            Err(Stop::Refused) => return Err(Stop::Refused),
            Err(Stop::Rejected(_)) => {
                // The trial's items/layouts/diagnostic remain spent. This uses
                // the same original Session; no refusal can enter this arm.
                Direct::interpret(replay, session)
                    .map(Kind::Value)
                    .map_err(|stop| direct::locate(stop, input.span))?
            }
        };
        Ok(Self {
            kind,
            span: input.span,
        })
    }

    pub(in super::super::super) fn kind(&self) -> &Kind<'d> {
        &self.kind
    }
    pub(in super::super::super) fn span(&self) -> Option<super::super::super::Span> {
        self.span
    }
}

impl<'d> Map<'d> {
    pub(in super::super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let value::View::Map(mut cursor) = input.view(session)? else {
            return session.reject(Diagnostic::at(Tag::Type, input.span));
        };
        let mut values = map::Map::new();
        while let Some((key, child)) = cursor.next(session)? {
            let Some(key) = key else {
                return session.fail(Error::State);
            };
            let name = primitive::Name::parse(
                key.text,
                scalar::Kind::Package,
                key.span.or(input.span),
                session,
            )?;
            let value = Inheritable::interpret(child, session)
                .map_err(|stop| direct::locate(stop, child.span.or(input.span)))?;
            values.push(name, key, value, session)?;
        }
        values.finish(session)?;
        Ok(Self { values })
    }

    pub(in super::super::super) fn entries(
        &self,
    ) -> impl ExactSizeIterator<Item = (&primitive::Name<'d>, &value::Key<'d>, &Inheritable<'d>)>
    {
        self.values.entries()
    }

    pub(in super::super::super) fn entry(
        &self,
        index: usize,
    ) -> Option<(&primitive::Name<'d>, &value::Key<'d>, &Inheritable<'d>)> {
        self.values.entry(index)
    }
}
