//! Borrowed adapter inputs and flat, converted output. Child ownership is by
//! index; dropping partial output never recursively drops a generic subtree.

use rawr_toml::document;

use super::{datetime, session, storage, Diagnostic, Span, Step, Tag};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Mode {
    Direct,
    Ignored,
    Content,
    SerdeValue,
    Toml,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Text<'d> {
    Borrowed(&'d str),
    Date(datetime::Text),
}

impl Text<'_> {
    pub(super) fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(value) => value,
            Self::Date(value) => value.as_str(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Key<'d> {
    pub(super) text: Text<'d>,
    pub(super) span: Option<Span>,
}

/// Replay borrows flat staging storage independently of retained document text.
#[derive(Clone, Copy)]
pub(super) struct Input<'p, 'd> {
    source: Source<'p, 'd>,
    pub(super) span: Option<Span>,
}

#[derive(Clone, Copy)]
enum Source<'p, 'd> {
    Raw(&'d document::Value<'d>),
    Root(&'d document::Table<'d>),
    Replay(&'p Projection<'d>, usize),
    Date(document::Datetime),
}

/// Intrinsic enum-dispatch shape, before `deserialize_any` scalar conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Shape {
    String,
    Map(usize),
    Sequence(usize),
    Other,
}

impl<'p, 'd> Input<'p, 'd> {
    pub(super) fn raw(value: &'d document::Spanned<document::Value<'d>>) -> Self {
        Self {
            source: Source::Raw(value.value()),
            span: Some(value.span().into()),
        }
    }

    pub(super) fn root(value: &'d document::Document<'d>) -> Self {
        Self {
            source: Source::Root(value.table()),
            span: Some(Span { start: 0, end: 0 }),
        }
    }

    pub(super) fn replay(value: &'p Projection<'d>, index: usize) -> Self {
        // Content and serde-value erase nested TOML spans. The outer caller
        // applies its context only if the selected error still has no span.
        Self {
            source: Source::Replay(value, index),
            span: None,
        }
    }

    pub(super) fn ignores_as_unit(self) -> bool {
        matches!(self.source, Source::Replay(projection, _) if projection.mode == Mode::Content)
    }

    pub(super) fn mode(self) -> Mode {
        match self.source {
            Source::Replay(projection, _) => projection.mode,
            _ => Mode::Direct,
        }
    }

    /// A constant-time discriminant/length read; callers check the session first.
    pub(super) fn shape(self) -> Shape {
        match self.source {
            Source::Raw(value) => match value {
                document::Value::String(_) => Shape::String,
                document::Value::Table(value) => Shape::Map(value.len()),
                document::Value::Array(value) => Shape::Sequence(value.values().len()),
                _ => Shape::Other,
            },
            Source::Root(value) => Shape::Map(value.len()),
            Source::Date(_) => Shape::String,
            Source::Replay(projection, index) => match projection.nodes.as_slice()[index].kind {
                Kind::String(_) => Shape::String,
                Kind::Map(children) => Shape::Map(children.length),
                Kind::Sequence(children) => Shape::Sequence(children.length),
                _ => Shape::Other,
            },
        }
    }

    pub(super) fn view<A: document::Admission>(
        self,
        session: &mut session::Session<'_, A>,
    ) -> Step<View<'p, 'd>> {
        session.check()?;
        let view = match self.source {
            Source::Raw(value) => match value {
                document::Value::String(value) => {
                    View::Scalar(Kind::String(Text::Borrowed(value.as_str())))
                }
                document::Value::Integer(value) => {
                    let text = value.as_str();
                    let radix = value.radix();
                    let kind = if let Ok(value) = value.to_i64() {
                        Kind::I64(value)
                    } else if let Ok(value) = u64::from_str_radix(text, radix) {
                        Kind::U64(value)
                    } else if let Ok(value) = i128::from_str_radix(text, radix) {
                        Kind::I128(value)
                    } else if let Ok(value) = u128::from_str_radix(text, radix) {
                        Kind::U128(value)
                    } else {
                        return session.reject(Diagnostic::at(Tag::IntegerOverflow, self.span));
                    };
                    View::Scalar(kind)
                }
                document::Value::Float(value) => {
                    let Some(value) = value.to_f64() else {
                        return session.reject(Diagnostic::at(Tag::FloatOverflow, self.span));
                    };
                    View::Scalar(Kind::Float(value))
                }
                document::Value::Boolean(value) => View::Scalar(Kind::Boolean(*value)),
                document::Value::DateTime(value) => View::Map(Cursor::Date(Some(*value))),
                document::Value::Array(value) => {
                    View::Sequence(Cursor::Array(value.values().iter()))
                }
                document::Value::Table(value) => View::Map(Cursor::Table(value.iter())),
            },
            Source::Root(value) => View::Map(Cursor::Table(value.iter())),
            Source::Date(value) => {
                return Ok(View::Scalar(Kind::String(Text::Date(
                    datetime::Text::render(value, session)?,
                ))));
            }
            Source::Replay(projection, index) => match projection.nodes.as_slice()[index].kind {
                Kind::Map(children) => View::Map(Cursor::Replay {
                    projection,
                    next: children.first,
                }),
                Kind::Sequence(children) => View::Sequence(Cursor::Replay {
                    projection,
                    next: children.first,
                }),
                Kind::DateTime(value) if projection.mode == Mode::Toml => {
                    return Ok(View::Scalar(Kind::String(Text::Date(
                        datetime::Text::render(value, session)?,
                    ))));
                }
                Kind::DateTime(value) => View::Map(Cursor::Date(Some(value))),
                scalar => View::Scalar(scalar),
            },
        };
        if let View::Scalar(Kind::String(text)) = &view {
            session.string(text.as_str().len())?;
        }
        Ok(view)
    }
}

pub(super) enum View<'p, 'd> {
    Scalar(Kind<'d>),
    Sequence(Cursor<'p, 'd>),
    Map(Cursor<'p, 'd>),
}

pub(super) enum Cursor<'p, 'd> {
    Array(std::slice::Iter<'d, document::Spanned<document::Value<'d>>>),
    Table(document::Iter<'d, 'd>),
    Replay {
        projection: &'p Projection<'d>,
        next: Option<usize>,
    },
    Date(Option<document::Datetime>),
}

impl<'p, 'd> Cursor<'p, 'd> {
    pub(super) fn finish<A: document::Admission>(
        &self,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        // Content and generic TOML explicitly check their remaining entries.
        // Raw TOML and serde-value invoke the visitor directly without end().
        if matches!(self, Self::Replay { projection, next: Some(_) } if matches!(projection.mode, Mode::Content | Mode::Toml))
        {
            return session.reject(Diagnostic::at(Tag::Length, span));
        }
        Ok(())
    }

    pub(super) fn next<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<(Option<Key<'d>>, Input<'p, 'd>)>> {
        session.check()?;
        let value = match self {
            Self::Array(values) => values.next().map(|value| (None, Input::raw(value))),
            Self::Table(values) => values.next().map(|(key, value)| {
                (
                    Some(Key {
                        text: Text::Borrowed(key.as_str()),
                        span: Some(key.span().into()),
                    }),
                    Input::raw(value),
                )
            }),
            Self::Replay { projection, next } => next.take().map(|index| {
                let edge = &projection.edges.as_slice()[index];
                *next = edge.next;
                let key = edge.key.map(|key| Key {
                    text: key.text,
                    span: None,
                });
                (key, Input::replay(projection, edge.child))
            }),
            Self::Date(value) => value.take().map(|value| {
                (
                    Some(Key {
                        text: Text::Borrowed(datetime::FIELD),
                        span: None,
                    }),
                    Input {
                        source: Source::Date(value),
                        span: None,
                    },
                )
            }),
        };
        if let Some((Some(key), _)) = &value {
            session.string(key.text.as_str().len())?;
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Children {
    pub(super) first: Option<usize>,
    pub(super) last: Option<usize>,
    pub(super) length: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Kind<'d> {
    String(Text<'d>),
    I64(i64),
    U64(u64),
    I128(i128),
    U128(u128),
    Float(f64),
    Boolean(bool),
    DateTime(document::Datetime),
    Sequence(Children),
    Map(Children),
}

#[derive(Debug)]
pub(super) struct Node<'d> {
    pub(super) kind: Kind<'d>,
    pub(super) span: Option<Span>,
}

#[derive(Debug)]
pub(super) struct Edge<'d> {
    pub(super) key: Option<Key<'d>>,
    pub(super) child: usize,
    pub(super) next: Option<usize>,
}

#[derive(Debug)]
pub(super) struct Projection<'d> {
    mode: Mode,
    pub(super) nodes: storage::Buffer<Node<'d>>,
    pub(super) edges: storage::Buffer<Edge<'d>>,
}

impl<'d> Projection<'d> {
    pub(super) fn new(mode: Mode) -> Self {
        Self {
            mode,
            nodes: storage::Buffer::new(),
            edges: storage::Buffer::new(),
        }
    }

    pub(super) fn append<A: document::Admission>(
        &mut self,
        node: Node<'d>,
        target: Option<(usize, Option<Key<'d>>)>,
        session: &mut session::Session<'_, A>,
    ) -> Step<usize> {
        let child = self.nodes.push(node, session)?;
        if let Some((parent, key)) = target {
            let index = self.edges.push(
                Edge {
                    key,
                    child,
                    next: None,
                },
                session,
            )?;
            let children = match &mut self.nodes.as_mut_slice()[parent].kind {
                Kind::Map(children) | Kind::Sequence(children) => children,
                _ => return session.fail(super::Error::State),
            };
            if let Some(last) = children.last {
                self.edges.as_mut_slice()[last].next = Some(index);
            } else {
                children.first = Some(index);
            }
            children.last = Some(index);
            children.length += 1;
        }
        Ok(child)
    }

    pub(super) fn sort<A: document::Admission>(
        &mut self,
        index: usize,
        scratch: &mut storage::Buffer<usize>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let Kind::Map(children) = self.nodes.as_slice()[index].kind else {
            return session.fail(super::Error::State);
        };
        if children.length < 2 {
            return Ok(());
        }
        scratch.clear();
        let mut next = children.first;
        while let Some(index) = next {
            scratch.push(index, session)?;
            next = self.edges.as_slice()[index].next;
        }
        let edges = self.edges.as_slice();
        storage::sort(scratch.as_mut_slice(), |left, right| {
            let left = edges[*left].key.as_ref().map(|key| key.text.as_str());
            let right = edges[*right].key.as_ref().map(|key| key.text.as_str());
            left.cmp(&right)
        });
        for pair in scratch.as_slice().windows(2) {
            self.edges.as_mut_slice()[pair[0]].next = Some(pair[1]);
        }
        let first = scratch.as_slice().first().copied();
        let last = scratch.as_slice().last().copied();
        if let Some(last) = last {
            self.edges.as_mut_slice()[last].next = None;
        }
        self.nodes.as_mut_slice()[index].kind = Kind::Map(Children {
            first,
            last,
            ..children
        });
        Ok(())
    }
}
