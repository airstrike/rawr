// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../LICENSE-regex-syntax-MIT.
//! Flat ownership for the reached regex-syntax AST constructors. Parsing,
//! bracketed classes and post-parse nesting validation are separate stages.
use super::{cursor::Span, flags::Flags, primitive::Primitive};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Id(usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Repeat {
    ZeroOrOne,
    ZeroOrMore,
    OneOrMore,
}
pub(super) struct Group {
    pub(super) span: Span,
    pub(super) flags: Flags,
    pub(super) child: Id,
}
pub(super) struct Repetition {
    pub(super) span: Span,
    pub(super) operator: Span,
    pub(super) kind: Repeat,
    pub(super) greedy: bool,
    pub(super) child: Id,
}
pub(super) enum Node {
    Empty(Span),
    Flags { span: Span, flags: Flags },
    Primitive(Primitive),
    Class(super::class::Class),
    Group(Group),
    Repetition(Repetition),
    Alternation { span: Span, children: Buffer<Id> },
    Concat { span: Span, children: Buffer<Id> },
}
impl Node {
    pub(super) fn span(&self) -> Span {
        match self {
            Self::Empty(span)
            | Self::Flags { span, .. }
            | Self::Alternation { span, .. }
            | Self::Concat { span, .. } => *span,
            Self::Primitive(primitive) => primitive.span,
            Self::Class(class) => class.span,
            Self::Group(group) => group.span,
            Self::Repetition(repetition) => repetition.span,
        }
    }
}
pub(super) struct Arena(Buffer<Node>);
#[derive(Clone, Copy)]
pub(super) enum Sequence {
    Alternation,
    Concat,
}
impl Arena {
    pub(super) fn new() -> Self {
        Self(Buffer::new())
    }
    pub(super) fn get<A: Admission>(&self, id: Id, session: &mut Session<'_, A>) -> Step<&Node> {
        session.items(1)?;
        match self.0.as_slice().get(id.0) {
            Some(node) => Ok(node),
            None => session.fail(Error::State),
        }
    }
    /// Child indices always refer backward to admitted nodes. Published nodes
    /// cannot be mutated to introduce recursive ownership or a forward edge.
    pub(super) fn push<A: Admission>(
        &mut self,
        node: Node,
        session: &mut Session<'_, A>,
    ) -> Step<Id> {
        session.check()?;
        match &node {
            Node::Group(group) => {
                self.get(group.child, session)?;
            }
            Node::Repetition(repetition) => {
                self.get(repetition.child, session)?;
            }
            Node::Alternation { children, .. } | Node::Concat { children, .. } => {
                for &child in children.as_slice() {
                    self.get(child, session)?;
                }
            }
            _ => {}
        }
        Ok(Id(self.0.push(node, session)?))
    }
    /// Pinned Concat/Alternation::into_ast: zero creates Empty, one moves the
    /// child unchanged (including its span), multiple retain the sequence.
    pub(super) fn sequence<A: Admission>(
        &mut self,
        kind: Sequence,
        span: Span,
        mut children: Buffer<Id>,
        session: &mut Session<'_, A>,
    ) -> Step<Id> {
        session.items(1)?;
        match children.len() {
            0 => self.push(Node::Empty(span), session),
            1 => {
                let Some(id) = children.pop() else {
                    return session.fail(Error::State);
                };
                self.get(id, session)?;
                Ok(id)
            }
            _ => self.push(
                match kind {
                    Sequence::Alternation => Node::Alternation { span, children },
                    Sequence::Concat => Node::Concat { span, children },
                },
                session,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        cursor::{Cursor, Position},
        primitive::Scratch,
    };
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn span(start: usize, end: usize) -> Span {
        Span {
            start: Position {
                offset: start,
                line: 1,
                column: start + 1,
            },
            end: Position {
                offset: end,
                line: 1,
                column: end + 1,
            },
        }
    }
    #[test]
    fn sequence_collapse_preserves_child_span_and_order() {
        for kind in [Sequence::Concat, Sequence::Alternation] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let mut arena = Arena::new();
            let empty = arena
                .sequence(kind, span(2, 3), Buffer::new(), &mut session)
                .unwrap();
            assert!(matches!(
                arena.get(empty, &mut session).unwrap(),
                Node::Empty(_)
            ));
            let primitive = Scratch::new()
                .parse(&mut Cursor::start("a"), &mut session)
                .unwrap();
            let first = arena
                .push(Node::Primitive(primitive), &mut session)
                .unwrap();
            let mut children = Buffer::new();
            children.push(first, &mut session).unwrap();
            let single = arena
                .sequence(kind, span(0, 50), children, &mut session)
                .unwrap();
            assert_eq!(single, first);
            assert_eq!(arena.get(single, &mut session).unwrap().span(), span(0, 1));
            let mut children = Buffer::new();
            children.push(first, &mut session).unwrap();
            children.push(empty, &mut session).unwrap();
            let many = arena
                .sequence(kind, span(0, 50), children, &mut session)
                .unwrap();
            let node = arena.get(many, &mut session).unwrap();
            assert_eq!(node.span(), span(0, 50));
            let children = match node {
                Node::Concat { children, .. } | Node::Alternation { children, .. } => children,
                _ => panic!("sequence"),
            };
            assert_eq!(children.as_slice(), &[first, empty]);
        }
    }
    #[test]
    fn deep_repetition_ownership_drops_without_recursive_children() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let mut child = arena.push(Node::Empty(span(0, 0)), &mut session).unwrap();
        for _ in 0..10_000 {
            child = arena
                .push(
                    Node::Repetition(Repetition {
                        span: span(0, 1),
                        operator: span(0, 1),
                        kind: Repeat::ZeroOrMore,
                        greedy: true,
                        child,
                    }),
                    &mut session,
                )
                .unwrap();
        }
        assert_eq!(arena.0.len(), 10_001);
        drop(arena);
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let position = self.calls;
            self.calls += 1;
            if position == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    fn construct<A: Admission>(session: &mut Session<'_, A>) -> Step<()> {
        let mut arena = Arena::new();
        let one = arena.push(Node::Empty(span(0, 0)), session)?;
        let two = arena.push(Node::Empty(span(1, 1)), session)?;
        let mut children = Buffer::new();
        children.push(one, session)?;
        children.push(two, session)?;
        arena.sequence(Sequence::Concat, span(0, 1), children, session)?;
        Ok(())
    }
    #[test]
    fn refusal_is_terminal_during_tree_construction() {
        let mut complete = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        construct(&mut Session::new(&mut complete)).unwrap();
        for refuse in 0..complete.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(construct(&mut session), Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
