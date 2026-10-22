// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../LICENSE-regex-syntax-MIT.
//! Pinned post-parse NestLimiter with admitted HeapVisitor continuations.
use super::{
    ast::{Arena, Id, Node},
    class::{Item, Set},
    cursor::Span,
};
use crate::input::membership::manifest::{
    load::formatted, session::Session, storage::Buffer, Error, Step, Stop,
};
use rawr_toml::document::Admission;

pub(super) struct Failure {
    pub(super) pattern: String,
    pub(super) span: Span,
    pub(super) limit: u32,
}
enum Abort {
    Stop(Stop),
    Failure(Failure),
}
impl From<Stop> for Abort {
    fn from(stop: Stop) -> Self {
        Self::Stop(stop)
    }
}
type Walk<T> = Result<T, Abort>;
struct Depth(u32);
impl Depth {
    fn enter<A: Admission>(
        &mut self,
        span: Span,
        pattern: &str,
        limit: u32,
        session: &mut Session<'_, A>,
    ) -> Walk<()> {
        session.items(1)?;
        let failure_limit = match self.0.checked_add(1) {
            Some(next) if next <= limit => {
                self.0 = next;
                return Ok(());
            }
            Some(_) => limit,
            None => u32::MAX,
        };
        let pattern = formatted(format_args!("{pattern}"), session)?;
        Err(Abort::Failure(Failure {
            pattern,
            span,
            limit: failure_limit,
        }))
    }
    fn leave<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Walk<()> {
        session.items(1)?;
        let Some(depth) = self.0.checked_sub(1) else {
            return session.fail(Error::State).map_err(Abort::Stop);
        };
        self.0 = depth;
        Ok(())
    }
}
struct Frame<'a> {
    parent: Id,
    tail: &'a [Id],
}
pub(super) fn check<A: Admission>(
    arena: &Arena,
    root: Id,
    pattern: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<(), Failure>> {
    match walk(arena, root, pattern, 250, session) {
        Ok(()) => Ok(Ok(())),
        Err(Abort::Stop(stop)) => Err(stop),
        Err(Abort::Failure(failure)) => Ok(Err(failure)),
    }
}
fn walk<A: Admission>(
    arena: &Arena,
    mut current: Id,
    pattern: &str,
    limit: u32,
    session: &mut Session<'_, A>,
) -> Walk<()> {
    session.check()?;
    let mut depth = Depth(0);
    let mut stack = Buffer::new();
    let mut classes = Buffer::new();
    loop {
        let node = arena.get(current, session)?;
        let structural = !matches!(
            node,
            Node::Empty(_) | Node::Flags { .. } | Node::Primitive(_)
        );
        if structural {
            depth.enter(node.span(), pattern, limit, session)?;
        }
        let next = match node {
            Node::Group(group) => Some((group.child, &[][..])),
            Node::Repetition(repetition) => Some((repetition.child, &[][..])),
            Node::Concat { children, .. } | Node::Alternation { children, .. } => children
                .as_slice()
                .split_first()
                .map(|(&head, tail)| (head, tail)),
            Node::Class(class) => {
                visit_class(
                    &class.set,
                    &mut depth,
                    &mut classes,
                    pattern,
                    limit,
                    session,
                )?;
                None
            }
            _ => None,
        };
        if let Some((child, tail)) = next {
            stack.push(
                Frame {
                    parent: current,
                    tail,
                },
                session,
            )?;
            current = child;
            continue;
        }
        if structural {
            depth.leave(session)?;
        } else {
            session.items(1)?;
        }
        loop {
            session.items(1)?;
            let Some(Frame { parent, tail }) = stack.pop() else {
                return Ok(());
            };
            if let Some((&head, tail)) = tail.split_first() {
                // Concat/alternation in-visits are no-ops for NestLimiter.
                stack.push(Frame { parent, tail }, session)?;
                current = head;
                break;
            }
            depth.leave(session)?;
        }
    }
}
fn visit_class<'a, A: Admission>(
    set: &'a Set,
    depth: &mut Depth,
    stack: &mut Buffer<&'a [Item]>,
    pattern: &str,
    limit: u32,
    session: &mut Session<'_, A>,
) -> Walk<()> {
    match set {
        Set::Empty(_) | Set::Single(_) => {
            session.items(2)?;
        }
        Set::Union { span, items } => {
            depth.enter(*span, pattern, limit, session)?;
            if let Some((_, tail)) = items.as_slice().split_first() {
                stack.push(tail, session)?;
                loop {
                    // Literal/range class items are both base cases.
                    session.items(2)?;
                    let Some(tail) = stack.pop() else {
                        return session.fail(Error::State).map_err(Abort::Stop);
                    };
                    if let Some((_, tail)) = tail.split_first() {
                        stack.push(tail, session)?;
                    } else {
                        break;
                    }
                }
            }
            depth.leave(session)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::parser;
    use super::*;
    use crate::input::membership::manifest::reference::Account;
    #[test]
    fn root_concat_and_class_union_count_toward_limit() {
        for (leaf, groups, succeeds) in [
            ("a", 249, true),
            ("a", 250, false),
            ("[a]", 248, true),
            ("[a]", 249, false),
            ("[ab]", 247, true),
            ("[ab]", 248, false),
        ] {
            let pattern = format!(
                "(?-u)^{}{leaf}{}$",
                "(?:".repeat(groups),
                ")".repeat(groups)
            );
            let result = parser::parse(&pattern, &mut Session::new(&mut Account)).unwrap();
            assert_eq!(result.is_ok(), succeeds, "{leaf} with {groups} groups");
            if let Err(parser::Failure::Nesting(failure)) = result {
                assert_eq!(failure.limit, 250);
                assert_eq!(failure.pattern, pattern);
            }
        }
    }
    #[test]
    fn nesting_error_points_at_first_excess_node() {
        let pattern = format!("(?-u)^{}a{}$", "(?:".repeat(250), ")".repeat(250));
        let Err(parser::Failure::Nesting(failure)) =
            parser::parse(&pattern, &mut Session::new(&mut Account)).unwrap()
        else {
            panic!("nesting");
        };
        assert_eq!(failure.span.start.offset, 6 + 3 * 249);
        assert_eq!(failure.span.end.offset, 6 + 3 * 250 + 2);
    }

    #[test]
    fn nesting_refusal_covers_union_visits_and_error_ownership() {
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
        let pattern = "(?-u)^(?:[ab][cd])$";
        let Ok(parsed) = parser::parse(pattern, &mut Session::new(&mut Account)).unwrap() else {
            panic!("parse");
        };
        for limit in [250, 2, 3] {
            let mut complete = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            let result = walk(
                &parsed.arena,
                parsed.root,
                pattern,
                limit,
                &mut Session::new(&mut complete),
            );
            assert_eq!(result.is_ok(), limit == 250);
            for refuse in 0..complete.calls {
                let mut count = Count { calls: 0, refuse };
                let mut session = Session::new(&mut count);
                assert!(matches!(
                    walk(&parsed.arena, parsed.root, pattern, limit, &mut session),
                    Err(Abort::Stop(Stop::Refused))
                ));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                assert_eq!(count.calls, refuse + 1);
            }
        }
    }
}
