//! Complete flat configuration values and source-ordered merge continuations.

use rawr_toml::document;

use super::super::super::super::{
    session::Session, storage, value, Diagnostic, Error, Span, Step, Tag,
};
use super::super::{definition::Definition, values::Values};
use super::{key::Key, table};

mod credentials;

pub(in super::super) mod include;
pub(in super::super) mod lookup;

/// A complete value tree. Merge consumes both trees, so no failed partial merge
/// can be exposed as a successful value. Node indices never leave this module.
pub(in super::super) struct Tree<'o, 'd> {
    nodes: storage::Buffer<Node<'o, 'd>>,
}

struct Node<'o, 'd> {
    kind: Option<Kind<'d>>,
    definition: Definition<'o>,
    span: Option<Span>,
}

enum Kind<'d> {
    String(&'d str),
    Integer(i64),
    Boolean(bool),
    List(storage::Buffer<Element>),
    Table(table::Table<'d>),
}

struct Element {
    node: usize,
    order: usize,
}

enum Frame<'d> {
    Pair {
        old: usize,
        new: usize,
    },
    Table {
        old: usize,
        incoming: table::IntoIter<'d>,
    },
}

impl Kind<'_> {
    fn desc(&self) -> &'static str {
        match self {
            Self::String(_) => "string",
            Self::Integer(_) => "integer",
            Self::Boolean(_) => "boolean",
            Self::List(_) => "array",
            Self::Table(_) => "table",
        }
    }
    fn container(&self) -> bool {
        matches!(self, Self::List(_) | Self::Table(_))
    }
}

impl<'o, 'd> Tree<'o, 'd> {
    pub(in super::super) fn empty<A: document::Admission>(
        definition: Definition<'o>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let mut nodes = storage::Buffer::new();
        nodes.push(
            Node {
                kind: Some(Kind::Table(table::Table::new())),
                definition,
                span: None,
            },
            session,
        )?;
        Ok(Self { nodes })
    }

    pub(in super::super) fn definition(&self) -> Definition<'o> {
        self.nodes.as_slice()[0].definition
    }

    #[cfg(test)]
    pub(in super::super) fn snapshot(&self) -> serde_json::Value {
        tests::rendered(self, 0)
    }

    pub(in super::super) fn from_values<A: document::Admission>(
        values: Values<'o, 'd>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let (definition, projection) = values.into_parts();
        let mut nodes = storage::Buffer::new();
        for source in projection.nodes.as_slice() {
            session.items(1)?;
            let kind = match source.kind {
                value::Kind::String(value::Text::Borrowed(text)) => Kind::String(text),
                value::Kind::I64(integer) => Kind::Integer(integer),
                value::Kind::Boolean(boolean) => Kind::Boolean(boolean),
                value::Kind::Sequence(children) => {
                    let mut elements = storage::Buffer::new();
                    let mut next = children.first;
                    while let Some(index) = next {
                        let edge = &projection.edges.as_slice()[index];
                        next = edge.next;
                        elements.push(
                            Element {
                                node: edge.child,
                                order: elements.len(),
                            },
                            session,
                        )?;
                    }
                    Kind::List(elements)
                }
                value::Kind::Map(children) => {
                    let mut table = table::Table::new();
                    let mut next = children.first;
                    while let Some(index) = next {
                        let edge = &projection.edges.as_slice()[index];
                        next = edge.next;
                        let Some(value::Key {
                            text: value::Text::Borrowed(key),
                            ..
                        }) = edge.key
                        else {
                            return session.fail(Error::State);
                        };
                        table.insert(key, edge.child, session)?;
                    }
                    Kind::Table(table)
                }
                _ => return session.fail(Error::State),
            };
            nodes.push(
                Node {
                    kind: Some(kind),
                    definition,
                    span: source.span,
                },
                session,
            )?;
        }
        if nodes.len() == 0 {
            return session.fail(Error::State);
        }
        Ok(Self { nodes })
    }

    pub(in super::super) fn merge<A: document::Admission>(
        mut self,
        incoming: Self,
        force: bool,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let root = self.nodes.len();
        for mut node in incoming.nodes.into_iter() {
            session.items(1)?;
            match node.kind.as_mut() {
                Some(Kind::Table(table)) => table.rebase(root, session)?,
                Some(Kind::List(elements)) => {
                    for element in elements.as_mut_slice() {
                        session.items(1)?;
                        let Some(index) = element.node.checked_add(root) else {
                            return session.fail(Error::Overflow);
                        };
                        element.node = index;
                    }
                }
                _ => {}
            }
            self.nodes.push(node, session)?;
        }
        self.merge_at(0, root, force, session)?;
        Ok(self)
    }

    fn merge_at<A: document::Admission>(
        &mut self,
        old: usize,
        new: usize,
        force: bool,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let mut key = Key::new();
        let mut frames = storage::Buffer::new();
        frames.push(Frame::Pair { old, new }, session)?;
        while let Some(frame) = frames.pop() {
            session.items(1)?;
            match frame {
                Frame::Pair { old, new } => {
                    self.pair(old, new, force, &key, &mut frames, session)?
                }
                Frame::Table { old, mut incoming } => {
                    if let Some((part, new)) = incoming.next(session)? {
                        let Some(Kind::Table(table)) = self.nodes.as_mut_slice()[old].kind.as_mut()
                        else {
                            return session.fail(Error::State);
                        };
                        let existing = table.merge(part, new, session)?;
                        frames.push(Frame::Table { old, incoming }, session)?;
                        if let Some(old) = existing {
                            key.push(part, session)?;
                            frames.push(Frame::Pair { old, new }, session)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn pair<A: document::Admission>(
        &mut self,
        old: usize,
        new: usize,
        force: bool,
        key: &Key<'_>,
        frames: &mut storage::Buffer<Frame<'d>>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        let Some(left) = self.nodes.as_slice()[old].kind.as_ref() else {
            return session.fail(Error::State);
        };
        let Some(right) = self.nodes.as_slice()[new].kind.as_ref() else {
            return session.fail(Error::State);
        };
        let higher = self.nodes.as_slice()[new]
            .definition
            .is_higher_priority(&self.nodes.as_slice()[old].definition);
        match (left, right) {
            (Kind::Table(_), Kind::Table(_)) => {
                let Some(Kind::Table(table)) = self.nodes.as_mut_slice()[new].kind.take() else {
                    return session.fail(Error::State);
                };
                frames.push(
                    Frame::Table {
                        old,
                        incoming: table.into_iter(session)?,
                    },
                    session,
                )?;
            }
            (Kind::List(_), Kind::List(_)) => {
                let nonmergeable = key.is_nonmergeable_list(session)?;
                let Some(Kind::List(mut left)) = self.nodes.as_mut_slice()[old].kind.take() else {
                    return session.fail(Error::State);
                };
                let Some(Kind::List(mut right)) = self.nodes.as_mut_slice()[new].kind.take() else {
                    return session.fail(Error::State);
                };
                if nonmergeable {
                    if force || higher {
                        left = right;
                    }
                } else if force {
                    for element in right.into_iter() {
                        left.push(element, session)?;
                    }
                } else {
                    for element in left.into_iter() {
                        right.push(element, session)?;
                    }
                    left = right;
                }
                // Encode stable order explicitly for allocation-free heapsort.
                // Even a kept nonmergeable list is sorted in the pinned source.
                for (order, element) in left.as_mut_slice().iter_mut().enumerate() {
                    session.items(1)?;
                    element.order = order;
                }
                let nodes = self.nodes.as_slice();
                storage::sort(left.as_mut_slice(), |a, b| {
                    nodes[a.node]
                        .definition
                        .cmp(&nodes[b.node].definition)
                        .then(a.order.cmp(&b.order))
                });
                self.nodes.as_mut_slice()[old].kind = Some(Kind::List(left));
            }
            (left, right) if left.container() || right.container() => {
                return session.reject(Diagnostic {
                    tag: Tag::ConfigMerge,
                    span: self.nodes.as_slice()[new].span,
                    expected: Some(left.desc()),
                });
            }
            _ => {
                if force || higher {
                    self.nodes.as_mut_slice().swap(old, new);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
