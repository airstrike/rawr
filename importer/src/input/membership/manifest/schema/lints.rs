//! Complete pinned lints interpretation, including delayed flattened Content.

use rawr_toml::document;

use super::super::{session, storage, value, walk, Diagnostic, Error, Span, Step, Stop, Tag};

mod map;

#[cfg(test)]
pub(in crate::input::membership::manifest) mod tests;

/// An interpreted workspace flag and the ordinary lint table it accompanies.
pub(in super::super) struct Inherited<'d> {
    workspace: bool,
    declared: Option<value::Key<'d>>,
    table: Table<'d>,
}

/// Converted tools in the pinned BTreeMap's string-key order.
pub(in super::super) struct Table<'d> {
    tools: map::Map<'d, Tool<'d>>,
}

/// Converted lint names in the pinned BTreeMap's string-key order.
pub(in super::super) struct Tool<'d> {
    rules: map::Map<'d, Lint<'d>>,
}

/// The original string-level versus configured-lint choice remains explicit.
pub(in super::super) enum Lint<'d> {
    Level(Level),
    Config(Config<'d>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum Level {
    Forbid,
    Deny,
    Warn,
    Allow,
}

/// Known typed fields and the completely converted insertion-ordered extras.
pub(in super::super) struct Config<'d> {
    level: Level,
    level_key: value::Key<'d>,
    priority: i8,
    priority_key: Option<value::Key<'d>>,
    extras: map::Map<'d, value::Projection<'d>>,
}

struct Captured<'d> {
    key: value::Key<'d>,
    value: value::Projection<'d>,
}

impl<'d> Inherited<'d> {
    pub(in super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let value::View::Map(mut cursor) = input.view(session)? else {
            return session.reject(Diagnostic::at(Tag::Type, input.span));
        };
        session.items(1)?;
        let mut workspace = false;
        let mut declared = None;
        let mut table = Table {
            tools: map::Map::new(),
        };
        while let Some((key, child)) = cursor.next(session)? {
            let Some(key) = key else {
                return session.fail(Error::State);
            };
            if key.text.as_str() == "workspace" {
                // The reached source domain has no unit/None value: raw TOML
                // and every capture of it always deserialize a present Some.
                match child.view(session)? {
                    value::View::Scalar(value::Kind::Boolean(true)) => {
                        session.items(1)?;
                        workspace = true;
                        declared = Some(key);
                    }
                    value::View::Scalar(value::Kind::Boolean(false)) => {
                        return session.reject(Diagnostic::at(
                            Tag::WorkspaceFalse,
                            child.span.or(input.span),
                        ));
                    }
                    _ => {
                        return session.reject(Diagnostic::at(Tag::Type, child.span.or(input.span)))
                    }
                }
            } else {
                let tool = Tool::interpret(child, session)
                    .map_err(|stop| locate(stop, child.span.or(input.span)))?;
                table.tools.push(key, tool, session)?;
            }
        }
        table.tools.finish(false, session)?;
        Ok(Self {
            workspace,
            declared,
            table,
        })
    }

    pub(in super::super) fn workspace(&self) -> bool {
        self.workspace
    }
    pub(in super::super) fn declared(&self) -> Option<value::Key<'d>> {
        self.declared
    }
    pub(in super::super) fn table(&self) -> &Table<'d> {
        &self.table
    }
}

impl<'d> Table<'d> {
    pub(in super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let value::View::Map(mut cursor) = input.view(session)? else {
            return session.reject(Diagnostic::at(Tag::Type, input.span));
        };
        session.items(1)?;
        let mut tools = map::Map::new();
        while let Some((key, child)) = cursor.next(session)? {
            let Some(key) = key else {
                return session.fail(Error::State);
            };
            let tool = Tool::interpret(child, session)
                .map_err(|stop| locate(stop, child.span.or(input.span)))?;
            tools.push(key, tool, session)?;
        }
        tools.finish(false, session)?;
        Ok(Self { tools })
    }

    pub(in super::super) fn tools(
        &self,
    ) -> impl ExactSizeIterator<Item = (&value::Key<'d>, &Tool<'d>)> {
        self.tools.iter()
    }
}

impl<'d> Tool<'d> {
    fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let value::View::Map(mut cursor) = input.view(session)? else {
            return session.reject(Diagnostic::at(Tag::Type, input.span));
        };
        session.items(1)?;
        let mut rules = map::Map::new();
        while let Some((key, child)) = cursor.next(session)? {
            let Some(key) = key else {
                return session.fail(Error::State);
            };
            let lint = Lint::interpret(child, session)
                .map_err(|stop| locate(stop, child.span.or(input.span)))?;
            rules.push(key, lint, session)?;
        }
        rules.finish(false, session)?;
        Ok(Self { rules })
    }

    pub(in super::super) fn rules(
        &self,
    ) -> impl ExactSizeIterator<Item = (&value::Key<'d>, &Lint<'d>)> {
        self.rules.iter()
    }
}

impl<'d> Lint<'d> {
    pub(in super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, 'd>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        match input.view(session)? {
            value::View::Scalar(value::Kind::String(text)) => {
                Level::text(text.as_str(), input.span, session).map(Self::Level)
            }
            value::View::Map(cursor) => {
                // serde-untagged's erased MapAccess converts nested errors to
                // custom text. The outer input restores its own span.
                Config::interpret(cursor, input.span, session)
                    .map(Self::Config)
                    .map_err(|stop| match stop {
                        Stop::Rejected(mut error) => {
                            error.span = input.span;
                            Stop::Rejected(error)
                        }
                        Stop::Refused => Stop::Refused,
                    })
            }
            _ => session.reject(Diagnostic::at(Tag::Type, input.span)),
        }
    }
}

impl Level {
    fn text<A: document::Admission>(
        text: &str,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let value = match text {
            "forbid" => Self::Forbid,
            "deny" => Self::Deny,
            "warn" => Self::Warn,
            "allow" => Self::Allow,
            _ => return session.reject(Diagnostic::at(Tag::Variant, span)),
        };
        session.items(1)?;
        Ok(value)
    }

    fn interpret<A: document::Admission>(
        input: value::Input<'_, '_>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        match input.shape() {
            value::Shape::String => {
                let value::View::Scalar(value::Kind::String(text)) = input.view(session)? else {
                    return session.fail(Error::State);
                };
                Self::text(text.as_str(), input.span, session)
            }
            value::Shape::Map(1) => {
                let value::View::Map(mut cursor) = input.view(session)? else {
                    return session.fail(Error::State);
                };
                let Some((Some(key), payload)) = cursor.next(session)? else {
                    return session.fail(Error::State);
                };
                let level = Self::text(key.text.as_str(), key.span.or(input.span), session)?;
                // Unit enums inspect their payload shape without dispatching
                // numeric conversion or visiting a nonempty container.
                // Content here replays the borrowed FlatMapAccess adapter.
                let raw_unit = matches!(input.mode(), value::Mode::Direct | value::Mode::Toml);
                match (raw_unit, payload.shape()) {
                    (true, value::Shape::Map(0) | value::Shape::Sequence(0)) => Ok(level),
                    (true, value::Shape::Map(_) | value::Shape::Sequence(_)) => {
                        session.reject(Diagnostic::at(Tag::Length, payload.span.or(input.span)))
                    }
                    _ => session.reject(Diagnostic::at(Tag::Type, payload.span.or(input.span))),
                }
            }
            value::Shape::Map(_) => session.reject(Diagnostic::at(Tag::Length, input.span)),
            _ => session.reject(Diagnostic::at(Tag::Type, input.span)),
        }
    }
}

impl<'d> Config<'d> {
    fn interpret<A: document::Admission>(
        mut cursor: value::Cursor<'_, 'd>,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let mut level = None;
        let mut priority = None;
        let mut captured = storage::Buffer::new();
        while let Some((key, input)) = cursor.next(session)? {
            let Some(key) = key else {
                return session.fail(Error::State);
            };
            match key.text.as_str() {
                "level" => {
                    if level.is_some() {
                        return duplicate("level", span, session);
                    }
                    level = Some((key, Level::interpret(input, session)?));
                }
                "priority" => {
                    if priority.is_some() {
                        return duplicate("priority", span, session);
                    }
                    let value = match input.view(session)? {
                        value::View::Scalar(value::Kind::I64(value)) => i8::try_from(value).ok(),
                        value::View::Scalar(value::Kind::U64(value)) => i8::try_from(value).ok(),
                        _ => None,
                    };
                    let Some(value) = value else {
                        return session.reject(Diagnostic {
                            tag: Tag::Type,
                            span: input.span,
                            expected: Some("i8"),
                        });
                    };
                    session.items(1)?;
                    priority = Some((key, value));
                }
                _ => {
                    let value = walk::run(input, value::Mode::Content, session)?;
                    captured.push(Captured { key, value }, session)?;
                }
            }
        }
        let Some((level_key, level)) = level else {
            return session.reject(Diagnostic {
                tag: Tag::Missing,
                span,
                expected: Some("level"),
            });
        };
        let (priority_key, priority) = match priority {
            Some((key, priority)) => (Some(key), priority),
            None => (None, 0),
        };
        let mut extras = map::Map::new();
        for captured in captured.as_slice() {
            session.string(captured.key.text.as_str().len())?;
            let value = walk::run(
                value::Input::replay(&captured.value, 0),
                value::Mode::Toml,
                session,
            )?;
            extras.push(captured.key, value, session)?;
        }
        extras.finish(true, session)?;
        Ok(Self {
            level,
            level_key,
            priority,
            priority_key,
            extras,
        })
    }

    pub(in super::super) fn level(&self) -> Level {
        self.level
    }
    pub(in super::super) fn level_key(&self) -> value::Key<'d> {
        self.level_key
    }
    pub(in super::super) fn priority(&self) -> i8 {
        self.priority
    }
    pub(in super::super) fn priority_key(&self) -> Option<value::Key<'d>> {
        self.priority_key
    }
    pub(in super::super) fn extras(
        &self,
    ) -> impl ExactSizeIterator<Item = (&value::Key<'d>, &value::Projection<'d>)> {
        self.extras.iter()
    }
}

fn duplicate<T, A: document::Admission>(
    name: &'static str,
    span: Option<Span>,
    session: &mut session::Session<'_, A>,
) -> Step<T> {
    session.reject(Diagnostic {
        tag: Tag::Duplicate,
        span,
        expected: Some(name),
    })
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
