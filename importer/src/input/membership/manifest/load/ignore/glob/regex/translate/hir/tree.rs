// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! Reached AST visitor and translator stacks for globset-emitted expressions.
use super::super::super::{ast, parser, primitive};
use super::super::{
    bytes,
    class::{Classes, Failure},
    Flags, Scalar,
};
use super::{arena::Arena, Expr, Kind, Literal, Look};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;

pub(in super::super) struct Translated {
    arena: Arena,
    root: Expr,
}
enum Frame {
    Expr(Expr),
    Literal(Buffer<u8>),
    Repetition,
    Group(Flags),
    Concat,
    Alternation,
    Branch,
}
enum Visit<'a> {
    Enter(ast::Id),
    Exit(&'a ast::Node),
}
struct Pending<'a> {
    node: &'a ast::Node,
    remaining: &'a [ast::Id],
}
impl Translated {
    pub(in super::super) fn any_byte<A: Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        let mut ranges = Buffer::new();
        ranges.push(bytes::Range::new(0, 255), session)?;
        let class = bytes::Class::from_ranges(ranges, session)?;
        let root = Expr::bytes(class, session)?;
        Ok(Self {
            arena: Arena::new(),
            root,
        })
    }
    pub(in super::super) fn view(&self) -> super::view::View<'_> {
        super::view::View::new(&self.arena, &self.root)
    }
    pub(in super::super) fn properties(&self) -> &super::Properties {
        &self.root.properties
    }
    pub(in super::super) fn from_tree<A: Admission>(
        parsed: &parser::Parsed,
        pattern: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        let mut arena = Arena::new();
        let mut flags = Flags::initial();
        let mut classes = Classes::new();
        let mut frames = Buffer::new();
        let mut visits = Buffer::new();
        let mut current = Some(Visit::Enter(parsed.root));
        while let Some(visit) = current.take() {
            session.items(1)?;
            match visit {
                Visit::Enter(id) => {
                    let node = parsed.arena.get(id, session)?;
                    match node {
                        ast::Node::Group(group) => {
                            let old = flags.set(&group.flags, session)?;
                            frames.push(Frame::Group(old), session)?;
                        }
                        ast::Node::Repetition(_) => {
                            frames.push(Frame::Repetition, session)?;
                        }
                        ast::Node::Concat { .. } => {
                            frames.push(Frame::Concat, session)?;
                        }
                        ast::Node::Alternation { children, .. } => {
                            frames.push(Frame::Alternation, session)?;
                            if children.len() != 0 {
                                frames.push(Frame::Branch, session)?;
                            }
                        }
                        _ => {}
                    }
                    let child = match node {
                        ast::Node::Group(group) => Some((group.child, &[][..])),
                        ast::Node::Repetition(repetition) => Some((repetition.child, &[][..])),
                        ast::Node::Concat { children, .. }
                        | ast::Node::Alternation { children, .. } => children
                            .as_slice()
                            .split_first()
                            .map(|(&id, rest)| (id, rest)),
                        _ => None,
                    };
                    if let Some((id, remaining)) = child {
                        // Pre-visit precedes the sole continuation allocation.
                        visits.push(Pending { node, remaining }, session)?;
                        current = Some(Visit::Enter(id));
                    } else {
                        // Leaves, including complete class visits, never
                        // allocate an outer AST continuation.
                        current = Some(Visit::Exit(node));
                    }
                }
                Visit::Exit(node) => {
                    match node {
                        ast::Node::Empty(_) => {
                            frames.push(Frame::Expr(Expr::empty(session)?), session)?;
                        }
                        ast::Node::Flags {
                            flags: directive, ..
                        } => {
                            flags.set(directive, session)?;
                            frames.push(Frame::Expr(Expr::empty(session)?), session)?;
                        }
                        ast::Node::Primitive(value) => match value.kind {
                            primitive::Kind::Literal { .. } => {
                                if flags.case_insensitive() {
                                    return session.fail(Error::State);
                                }
                                match flags.scalar(value, session)? {
                                    Scalar::Byte(byte) => append(&mut frames, &[byte], session)?,
                                    Scalar::Char(c) => {
                                        let mut bytes = [0; 4];
                                        append(
                                            &mut frames,
                                            c.encode_utf8(&mut bytes).as_bytes(),
                                            session,
                                        )?;
                                    }
                                }
                            }
                            primitive::Kind::Dot => {
                                // The emitter fixes dot_matches_new_line and disables Unicode.
                                if flags.unicode() || !flags.dot_matches_new_line() {
                                    return session.fail(Error::State);
                                }
                                let mut ranges = Buffer::new();
                                ranges.push(bytes::Range::new(0, 255), session)?;
                                let class = bytes::Class::from_ranges(ranges, session)?;
                                frames.push(Frame::Expr(Expr::bytes(class, session)?), session)?;
                            }
                            primitive::Kind::StartLine | primitive::Kind::EndLine => {
                                if flags.multi_line() {
                                    return session.fail(Error::State);
                                }
                                let look = if matches!(value.kind, primitive::Kind::StartLine) {
                                    Look::Start
                                } else {
                                    Look::End
                                };
                                frames.push(Frame::Expr(Expr::look(look, session)?), session)?;
                            }
                        },
                        ast::Node::Class(class) => {
                            let expr = match classes.expression(class, flags, pattern, session)? {
                                Ok(expr) => expr,
                                Err(error) => return Ok(Err(error)),
                            };
                            frames.push(Frame::Expr(expr), session)?;
                        }
                        ast::Node::Group(_) => {
                            let expr = pop_expr(&mut frames, session)?;
                            let Some(Frame::Group(old)) = frames.pop() else {
                                return session.fail(Error::State);
                            };
                            flags = old;
                            frames.push(Frame::Expr(expr), session)?;
                        }
                        ast::Node::Repetition(repetition) => {
                            let expr = pop_expr(&mut frames, session)?;
                            if !matches!(frames.pop(), Some(Frame::Repetition)) {
                                return session.fail(Error::State);
                            }
                            let (min, max) = match repetition.kind {
                                ast::Repeat::ZeroOrOne => (0, Some(1)),
                                ast::Repeat::ZeroOrMore => (0, None),
                                ast::Repeat::OneOrMore => (1, None),
                            };
                            let greedy = repetition.greedy != flags.swap_greed();
                            let expr = arena.repetition(expr, min, max, greedy, session)?;
                            frames.push(Frame::Expr(expr), session)?;
                        }
                        ast::Node::Concat { .. } | ast::Node::Alternation { .. } => {
                            let alternate = matches!(node, ast::Node::Alternation { .. });
                            let mut children = Buffer::new();
                            loop {
                                session.items(1)?;
                                let Some(frame) = frames.pop() else {
                                    return session.fail(Error::State);
                                };
                                if matches!(
                                    (&frame, alternate),
                                    (Frame::Concat, false) | (Frame::Alternation, true)
                                ) {
                                    break;
                                }
                                let expr = expression(frame, session)?;
                                if alternate && !matches!(frames.pop(), Some(Frame::Branch)) {
                                    return session.fail(Error::State);
                                }
                                if alternate || !matches!(expr.kind, Kind::Empty) {
                                    children.push(expr, session)?;
                                }
                            }
                            session.items(children.len())?;
                            children.as_mut_slice().reverse();
                            let expr = if alternate {
                                arena.alternation(children, session)?
                            } else {
                                arena.concat(children, session)?
                            };
                            frames.push(Frame::Expr(expr), session)?;
                        }
                    }
                    if let Some(Pending { node, remaining }) = visits.pop() {
                        if let Some((&id, rest)) = remaining.split_first() {
                            if matches!(node, ast::Node::Alternation { .. }) {
                                frames.push(Frame::Branch, session)?;
                            }
                            visits.push(
                                Pending {
                                    node,
                                    remaining: rest,
                                },
                                session,
                            )?;
                            current = Some(Visit::Enter(id));
                        } else {
                            current = Some(Visit::Exit(node));
                        }
                    }
                }
            }
        }
        if frames.len() != 1 {
            return session.fail(Error::State);
        }
        let root = pop_expr(&mut frames, session)?;
        Ok(Ok(Self { arena, root }))
    }
}
fn expression<A: Admission>(frame: Frame, session: &mut Session<'_, A>) -> Step<Expr> {
    match frame {
        Frame::Expr(expr) => Ok(expr),
        Frame::Literal(bytes) => {
            let literal = Literal::copy(bytes.as_slice(), session)?;
            drop(bytes);
            Expr::literal(literal, session)
        }
        _ => session.fail(Error::State),
    }
}
fn pop_expr<A: Admission>(frames: &mut Buffer<Frame>, session: &mut Session<'_, A>) -> Step<Expr> {
    session.items(1)?;
    match frames.pop() {
        Some(frame) => expression(frame, session),
        None => session.fail(Error::State),
    }
}
fn append<A: Admission>(
    frames: &mut Buffer<Frame>,
    bytes: &[u8],
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.items(1)?;
    if let Some(Frame::Literal(literal)) = frames.as_mut_slice().last_mut() {
        literal.reserve_more(bytes.len(), session)?;
        for &byte in bytes {
            literal.push(byte, session)?;
        }
    } else {
        let mut literal = Buffer::new();
        literal.reserve_more(bytes.len(), session)?;
        for &byte in bytes {
            literal.push(byte, session)?;
        }
        frames.push(Frame::Literal(literal), session)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn emitted(glob: &str) -> super::super::super::super::Regex {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        super::super::super::super::super::parse(glob, &mut session)
            .unwrap()
            .ok()
            .unwrap()
            .regex(&mut session)
            .unwrap()
    }
    #[test]
    fn emitted_patterns_reach_complete_hir_properties() {
        for (glob, minimum, maximum, utf8) in [
            ("é", Some(2), Some(2), true),
            ("{a,b}", Some(1), Some(1), true),
            ("[a-z]", Some(1), Some(1), true),
            ("?", Some(1), Some(1), false),
            ("*", Some(0), None, false),
        ] {
            let source = emitted(glob);
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let parsed = source.tree(&mut session).unwrap().ok().unwrap();
            let translated = Translated::from_tree(
                &parsed,
                std::str::from_utf8(source.bytes()).unwrap(),
                &mut session,
            )
            .unwrap()
            .ok()
            .unwrap();
            let props = translated.root.properties.get();
            assert_eq!(
                (props.minimum_len, props.maximum_len, props.utf8),
                (minimum, maximum, utf8),
                "{glob}"
            );
        }
    }
    #[test]
    fn emitted_unicode_class_remains_byte_oriented() {
        let source = emitted("[é]");
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let parsed = source.tree(&mut session).unwrap().ok().unwrap();
        let pattern = std::str::from_utf8(source.bytes()).unwrap();
        let result = Translated::from_tree(&parsed, pattern, &mut session).unwrap();
        // The emitter encodes Unicode as hex bytes, which are valid in this
        // byte class; this must not take the UnicodeNotAllowed branch.
        assert!(result.is_ok());
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let call = self.calls;
            self.calls += 1;
            if call == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn translation_refusal_is_terminal_through_nested_branches() {
        let source = emitted("{a*,b[0-9]?}/é");
        let parsed = source
            .tree(&mut Session::new(&mut Account))
            .unwrap()
            .ok()
            .unwrap();
        let pattern = std::str::from_utf8(source.bytes()).unwrap();
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        assert!(
            Translated::from_tree(&parsed, pattern, &mut Session::new(&mut count))
                .unwrap()
                .is_ok()
        );
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                Translated::from_tree(&parsed, pattern, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
