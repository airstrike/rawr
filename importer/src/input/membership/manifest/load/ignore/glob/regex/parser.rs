// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../LICENSE-regex-syntax-MIT.
//! Connected emitted-regex AST construction and pinned post-parse nesting pass.
use super::{
    ast::{self, Arena, Id, Node, Sequence},
    class,
    cursor::{Cursor, Span},
    flags::{self, Flag},
    group::{self, Opening},
    primitive::Scratch,
};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;

pub(super) struct Parsed {
    pub(super) arena: Arena,
    pub(super) root: Id,
}
pub(super) enum Failure {
    Flags(flags::Failure),
    Class(class::Failure),
    Nesting(super::nesting::Failure),
}
struct Concat {
    span: Span,
    children: Buffer<Id>,
}
impl Concat {
    fn new(span: Span) -> Self {
        Self {
            span,
            children: Buffer::new(),
        }
    }
    fn finish<A: Admission>(
        self,
        arena: &mut Arena,
        kind: Sequence,
        session: &mut Session<'_, A>,
    ) -> Step<Id> {
        arena.sequence(kind, self.span, self.children, session)
    }
}
enum Frame {
    Group { parent: Concat, group: ast::Group },
    Alternate(Concat),
}
struct Parser<'a> {
    cursor: Cursor<'a>,
    arena: Arena,
    stack: Buffer<Frame>,
    scratch: Scratch,
    classes: class::Stack,
}
pub(super) fn parse<A: Admission>(
    pattern: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<Parsed, Failure>> {
    session.check()?;
    let mut parser = Parser {
        cursor: Cursor::start(pattern),
        arena: Arena::new(),
        stack: Buffer::new(),
        scratch: Scratch::new(),
        classes: class::Stack::new(),
    };
    parser.run(session)
}
impl Parser<'_> {
    fn run<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<Result<Parsed, Failure>> {
        let mut concat = Concat::new(self.cursor.span());
        while !self.cursor.eof() {
            session.items(1)?;
            match self.cursor.char(session)? {
                '(' => match group::parse(&mut self.cursor, &mut self.arena, session)? {
                    Err(failure) => return Ok(Err(Failure::Flags(failure))),
                    Ok(Opening::Flags { span, flags }) => {
                        if flags.state(Flag::IgnoreWhitespace, session)? == Some(true) {
                            return session.fail(Error::State);
                        }
                        let id = self.arena.push(Node::Flags { span, flags }, session)?;
                        concat.children.push(id, session)?;
                    }
                    Ok(Opening::Group(group)) => {
                        if group.flags.state(Flag::IgnoreWhitespace, session)? == Some(true) {
                            return session.fail(Error::State);
                        }
                        self.stack.push(
                            Frame::Group {
                                parent: concat,
                                group,
                            },
                            session,
                        )?;
                        concat = Concat::new(self.cursor.span());
                    }
                },
                ')' => concat = self.close(concat, session)?,
                '|' => concat = self.alternate(concat, session)?,
                '[' => {
                    let class = match class::parse(
                        &mut self.cursor,
                        &mut self.scratch,
                        &mut self.classes,
                        session,
                    )? {
                        Ok(class) => class,
                        Err(failure) => return Ok(Err(Failure::Class(failure))),
                    };
                    let id = self.arena.push(Node::Class(class), session)?;
                    concat.children.push(id, session)?;
                }
                '?' | '*' | '+' => self.repeat(&mut concat, session)?,
                '{' => return session.fail(Error::State), // no counted repeats are emitted
                _ => {
                    let primitive = self.scratch.parse(&mut self.cursor, session)?;
                    let id = self.arena.push(Node::Primitive(primitive), session)?;
                    concat.children.push(id, session)?;
                }
            }
        }
        concat.span.end = self.cursor.position();
        let root = match self.stack.pop() {
            None => concat.finish(&mut self.arena, Sequence::Concat, session)?,
            Some(Frame::Alternate(mut alternate)) => {
                alternate.span.end = self.cursor.position();
                let child = concat.finish(&mut self.arena, Sequence::Concat, session)?;
                alternate.children.push(child, session)?;
                alternate.finish(&mut self.arena, Sequence::Alternation, session)?
            }
            Some(Frame::Group { .. }) => return session.fail(Error::State),
        };
        if self.stack.pop().is_some() {
            return session.fail(Error::State);
        }
        // The source retains parser scratch/continuations through NestLimiter.
        if let Err(failure) =
            super::nesting::check(&self.arena, root, self.cursor.pattern(), session)?
        {
            return Ok(Err(Failure::Nesting(failure)));
        }
        // Syntax-tree completion is not HIR translation or matcher authority.
        let arena = std::mem::replace(&mut self.arena, Arena::new());
        Ok(Ok(Parsed { arena, root }))
    }
    fn alternate<A: Admission>(
        &mut self,
        mut concat: Concat,
        session: &mut Session<'_, A>,
    ) -> Step<Concat> {
        concat.span.end = self.cursor.position();
        let span = concat.span;
        if let Some(Frame::Alternate(alternate)) = self.stack.as_mut_slice().last_mut() {
            let child = concat.finish(&mut self.arena, Sequence::Concat, session)?;
            alternate.children.push(child, session)?;
        } else {
            let mut alternate = Concat::new(span);
            // Pinned vec![concat.into_ast()] allocates before evaluating its
            // element, unlike push on an existing alternation's vector.
            alternate.children.reserve_next(session)?;
            let child = concat.finish(&mut self.arena, Sequence::Concat, session)?;
            alternate.children.push(child, session)?;
            self.stack.push(Frame::Alternate(alternate), session)?;
        }
        self.cursor.bump(session)?;
        Ok(Concat::new(self.cursor.span()))
    }
    fn close<A: Admission>(
        &mut self,
        mut concat: Concat,
        session: &mut Session<'_, A>,
    ) -> Step<Concat> {
        let (mut parent, mut group, alternate) = match self.stack.pop() {
            Some(Frame::Group { parent, group }) => (parent, group, None),
            Some(Frame::Alternate(alternate)) => match self.stack.pop() {
                Some(Frame::Group { parent, group }) => (parent, group, Some(alternate)),
                _ => return session.fail(Error::State),
            },
            None => return session.fail(Error::State),
        };
        concat.span.end = self.cursor.position();
        let end = concat.span.end;
        self.cursor.bump(session)?;
        group.span.end = self.cursor.position();
        let child = concat.finish(&mut self.arena, Sequence::Concat, session)?;
        group.child = match alternate {
            Some(mut alternate) => {
                alternate.span.end = end;
                alternate.children.push(child, session)?;
                alternate.finish(&mut self.arena, Sequence::Alternation, session)?
            }
            None => child,
        };
        let id = self.arena.push(Node::Group(group), session)?;
        parent.children.push(id, session)?;
        Ok(parent)
    }
    fn repeat<A: Admission>(
        &mut self,
        concat: &mut Concat,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let kind = match self.cursor.char(session)? {
            '?' => ast::Repeat::ZeroOrOne,
            '*' => ast::Repeat::ZeroOrMore,
            '+' => ast::Repeat::OneOrMore,
            _ => return session.fail(Error::State),
        };
        let start = self.cursor.position();
        let Some(child) = concat.children.pop() else {
            return session.fail(Error::State);
        };
        let node = self.arena.get(child, session)?;
        if matches!(node, Node::Empty(_) | Node::Flags { .. }) {
            return session.fail(Error::State);
        }
        let span = node.span();
        let mut greedy = true;
        if self.cursor.bump(session)? && self.cursor.char(session)? == '?' {
            greedy = false;
            self.cursor.bump(session)?;
        }
        let end = self.cursor.position();
        let node = Node::Repetition(ast::Repetition {
            span: Span {
                start: span.start,
                end,
            },
            operator: Span { start, end },
            kind,
            greedy,
            child,
        });
        let id = self.arena.push(node, session)?;
        concat.children.push(id, session)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn emitted_patterns_reach_nesting_checked_trees() {
        for pattern in [
            "",
            "**",
            "a/**/b?",
            "{a,[b-d],{c,d}}",
            "[!é-雪]",
            "**/*.rs",
            "a\\*b",
        ] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let Ok(glob) = super::super::super::parse(pattern, &mut session).unwrap() else {
                panic!("glob");
            };
            let regex = glob.regex(&mut session).unwrap();
            let Ok(draft) = regex.tree(&mut session).unwrap() else {
                panic!("tree");
            };
            let root = draft.arena.get(draft.root, &mut session).unwrap();
            assert_eq!(
                (root.span().start.offset, root.span().end.offset),
                (0, regex.bytes().len())
            );
            assert!(matches!(root, Node::Concat { .. }));
        }
    }
    #[test]
    fn nested_alternate_group_keeps_children_and_span() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let Ok(draft) = parse("(?-u)^(?:a|b)$", &mut session).unwrap() else {
            panic!("parse");
        };
        let Node::Concat { children, .. } = draft.arena.get(draft.root, &mut session).unwrap()
        else {
            panic!("concat");
        };
        assert_eq!(children.len(), 4);
        let Node::Group(group) = draft
            .arena
            .get(children.as_slice()[2], &mut session)
            .unwrap()
        else {
            panic!("group");
        };
        assert_eq!((group.span.start.offset, group.span.end.offset), (6, 13));
        let Node::Alternation { span, children } =
            draft.arena.get(group.child, &mut session).unwrap()
        else {
            panic!("alternation");
        };
        assert_eq!((span.start.offset, span.end.offset), (9, 12));
        assert_eq!(children.len(), 2);
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let index = self.calls;
            self.calls += 1;
            if index == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn connected_tree_refusal_is_terminal() {
        let pattern = "(?-u)^(?:a|[b-d])[^/]*$";
        let mut complete = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        assert!(parse(pattern, &mut Session::new(&mut complete))
            .unwrap()
            .is_ok());
        for refuse in 0..complete.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(parse(pattern, &mut session), Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }

    #[test]
    fn first_alternate_reserves_before_concat_but_existing_alternate_after() {
        struct Allocations(Vec<usize>);
        impl Admission for Allocations {
            type Error = ();
            fn admit(&mut self, request: rawr_toml::document::Request) -> Result<(), ()> {
                if let rawr_toml::document::Request::Allocation { bytes, .. } = request {
                    self.0.push(bytes);
                }
                Ok(())
            }
        }
        for existing in [false, true] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let cursor = Cursor::start("|");
            let span = cursor.span();
            let mut parser = Parser {
                cursor,
                arena: Arena::new(),
                stack: Buffer::new(),
                scratch: Scratch::new(),
                classes: class::Stack::new(),
            };
            let mut ids = Vec::new();
            for _ in 0..4 {
                ids.push(parser.arena.push(Node::Empty(span), &mut session).unwrap());
            }
            let mut concat = Concat::new(span);
            concat.children.push(ids[0], &mut session).unwrap();
            concat.children.push(ids[1], &mut session).unwrap();
            if existing {
                let mut alternate = Concat::new(span);
                for &id in &ids {
                    alternate.children.push(id, &mut session).unwrap();
                }
                parser
                    .stack
                    .push(Frame::Alternate(alternate), &mut session)
                    .unwrap();
            }
            let mut allocations = Allocations(Vec::new());
            parser
                .alternate(concat, &mut Session::new(&mut allocations))
                .unwrap();
            let nodes = std::mem::size_of::<Node>() * 8;
            let ids = std::mem::size_of::<Id>();
            if existing {
                assert_eq!(allocations.0, vec![nodes, ids * 8]);
            } else {
                assert_eq!(
                    allocations.0,
                    vec![ids * 4, nodes, std::mem::size_of::<Frame>() * 4]
                );
            }
        }
    }
}
