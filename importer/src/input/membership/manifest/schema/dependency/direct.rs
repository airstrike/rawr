//! Cargo TomlDependency's string/map visitor and direct package-name map.
//! The pinned Boolean diagnostic is represented by its bounded rejection tag;
//! its advisory String formatting does not affect accepted dependency values.

use rawr_toml::document;

use super::super::super::{scalar, session, value, Diagnostic, Error, Step, Tag};
use super::super::primitive;
use super::{map, record};

pub(in super::super::super) enum Kind<'d> {
    Simple(value::Text<'d>),
    Detailed(record::Detailed<'d>),
}

pub(in super::super::super) struct Direct<'d> {
    kind: Kind<'d>,
    span: Option<super::super::super::Span>,
}

pub(in super::super::super) struct Map<'d> {
    values: map::Map<'d, primitive::Name<'d>, Direct<'d>>,
}

impl<'d> Direct<'d> {
    pub(in super::super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let kind = match input.view(session)? {
            value::View::Scalar(value::Kind::String(text)) => Kind::Simple(text),
            value::View::Map(cursor) => record::Detailed::from_map(cursor, input.span, session)
                .map(Kind::Detailed)
                .map_err(|stop| primitive::erase(stop, input.span))?,
            _ => return session.reject(Diagnostic::at(Tag::Type, input.span)),
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
            let value = Direct::interpret(child, session)
                .map_err(|stop| locate(stop, child.span.or(input.span)))?;
            values.push(name, key, value, session)?;
        }
        values.finish(session)?;
        Ok(Self { values })
    }

    pub(in super::super::super) fn entry(
        &self,
        index: usize,
    ) -> Option<(&primitive::Name<'d>, &value::Key<'d>, &Direct<'d>)> {
        self.values.entry(index)
    }

    pub(in super::super::super) fn entries(
        &self,
    ) -> impl ExactSizeIterator<Item = (&primitive::Name<'d>, &value::Key<'d>, &Direct<'d>)> {
        self.values.entries()
    }
}

pub(super) fn locate(
    stop: super::super::super::Stop,
    span: Option<super::super::super::Span>,
) -> super::super::super::Stop {
    match stop {
        super::super::super::Stop::Rejected(mut error) => {
            error.span = error.span.or(span);
            super::super::super::Stop::Rejected(error)
        }
        super::super::super::Stop::Refused => super::super::super::Stop::Refused,
    }
}
