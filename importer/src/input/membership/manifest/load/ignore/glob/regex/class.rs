// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../LICENSE-regex-syntax-MIT.
//! regex-syntax 0.8.11 bracketed classes reached by globset emission.
//! Escaped metacharacters prevent nested classes and set operators. Retain
//! the reached class continuation, union shapes, ranges and source spans.
use super::{
    cursor::{Cursor, Span},
    primitive::{Kind, Primitive, Scratch},
};
use crate::input::membership::manifest::{
    load::formatted, session::Session, storage::Buffer, Error, Step,
};
use rawr_toml::document::Admission;

pub(super) enum Item {
    Literal(Primitive),
    Range {
        span: Span,
        start: Primitive,
        end: Primitive,
    },
}
impl Item {
    fn span(&self) -> Span {
        match self {
            Self::Literal(literal) => literal.span,
            Self::Range { span, .. } => *span,
        }
    }
}
pub(super) enum Set {
    // regex-syntax's empty-class span; the arena admits Node at its full size.
    #[allow(dead_code)]
    Empty(Span),
    Single(Item),
    Union {
        span: Span,
        items: Buffer<Item>,
    },
}
pub(super) struct Class {
    pub(super) span: Span,
    pub(super) negated: bool,
    pub(super) set: Set,
}
struct Union {
    span: Span,
    items: Buffer<Item>,
}
impl Union {
    fn new(span: Span) -> Self {
        Self {
            span,
            items: Buffer::new(),
        }
    }
    fn push<A: Admission>(&mut self, item: Item, session: &mut Session<'_, A>) -> Step<()> {
        session.check()?;
        if self.items.len() == 0 {
            self.span.start = item.span().start;
        }
        self.span.end = item.span().end;
        self.items.push(item, session)?;
        Ok(())
    }
    fn into_set<A: Admission>(mut self, session: &mut Session<'_, A>) -> Step<Set> {
        session.items(1)?;
        match self.items.len() {
            0 => Ok(Set::Empty(self.span)),
            1 => match self.items.pop() {
                Some(item) => Ok(Set::Single(item)),
                None => session.fail(Error::State),
            },
            _ => Ok(Set::Union {
                span: self.span,
                items: self.items,
            }),
        }
    }
}
struct Frame {
    _parent: Union,
    class: Class,
}
/// Parser-owned continuation capacity, reused across successive classes.
pub(super) struct Stack(Buffer<Frame>);
impl Stack {
    pub(super) fn new() -> Self {
        Self(Buffer::new())
    }
}
/// Pinned ClassRangeInvalid: the complete regex pattern is owned on failure.
pub(super) struct Failure {
    pub(super) pattern: String,
    pub(super) span: Span,
}

pub(super) fn parse<A: Admission>(
    cursor: &mut Cursor<'_>,
    scratch: &mut Scratch,
    stack: &mut Stack,
    session: &mut Session<'_, A>,
) -> Step<Result<Class, Failure>> {
    session.check()?;
    if stack.0.len() != 0 {
        return session.fail(Error::State);
    }
    let parent = Union::new(cursor.span());
    if cursor.char(session)? != '[' {
        return session.fail(Error::State);
    }
    let start = cursor.position();
    if !cursor.bump(session)? {
        return session.fail(Error::State);
    }
    let negated = if cursor.char(session)? == '^' {
        if !cursor.bump(session)? {
            return session.fail(Error::State);
        }
        true
    } else {
        false
    };
    let mut union = Union::new(cursor.span());
    // These special opening literals are always escaped by the emitter.
    if matches!(cursor.char(session)?, '-' | ']') {
        return session.fail(Error::State);
    }
    let class = Class {
        span: Span {
            start,
            end: cursor.position(),
        },
        negated,
        set: Set::Empty(cursor.span()),
    };
    stack.0.push(
        Frame {
            _parent: parent,
            class,
        },
        session,
    )?;
    loop {
        session.items(1)?;
        if cursor.eof() {
            return session.fail(Error::State);
        }
        let c = cursor.char(session)?;
        if c == ']' {
            let set = union.into_set(session)?;
            // pop_class_op sees an Open, pops it and pushes it back before
            // pop_class consumes it. Keep that reached admitted operation.
            let Some(frame) = stack.0.pop() else {
                return session.fail(Error::State);
            };
            stack.0.push(frame, session)?;
            let Some(mut frame) = stack.0.pop() else {
                return session.fail(Error::State);
            };
            cursor.bump(session)?;
            frame.class.span.end = cursor.position();
            frame.class.set = set;
            return Ok(Ok(frame.class));
        }
        if c == '['
            || (c == '&' && cursor.peek(session)? == Some('&'))
            || (c == '-' && cursor.peek(session)? == Some('-'))
            || (c == '~' && cursor.peek(session)? == Some('~'))
        {
            return session.fail(Error::State);
        }
        let first = scratch.class_item(cursor, session)?;
        if cursor.eof() {
            return session.fail(Error::State);
        }
        let item = if cursor.char(session)? != '-'
            || cursor.peek(session)? == Some(']')
            || cursor.peek(session)? == Some('-')
        {
            Item::Literal(first)
        } else {
            if !cursor.bump(session)? {
                return session.fail(Error::State);
            }
            let second = scratch.class_item(cursor, session)?;
            let span = Span {
                start: first.span.start,
                end: second.span.end,
            };
            let (Kind::Literal { c: start, .. }, Kind::Literal { c: end, .. }) =
                (&first.kind, &second.kind)
            else {
                return session.fail(Error::State);
            };
            if start > end {
                let pattern = formatted(format_args!("{}", cursor.pattern()), session)?;
                return Ok(Err(Failure { pattern, span }));
            }
            Item::Range {
                span,
                start: first,
                end: second,
            }
        };
        union.push(item, session)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn classes_preserve_negation_ranges_and_union_shape() {
        for (pattern, negated, count) in [
            ("[^/]", true, 1),
            ("[a-c\\-]", false, 2),
            ("[\\xc3\\xa9]", false, 2),
            ("[.\\$]", false, 2),
        ] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let mut cursor = Cursor::start(pattern);
            let Ok(class) = parse(
                &mut cursor,
                &mut Scratch::new(),
                &mut Stack::new(),
                &mut session,
            )
            .unwrap() else {
                panic!("class");
            };
            assert!(cursor.eof());
            assert_eq!(
                (class.span.start.offset, class.span.end.offset),
                (0, pattern.len())
            );
            assert_eq!(class.negated, negated);
            let items = match &class.set {
                Set::Single(item) => std::slice::from_ref(item),
                Set::Union { items, .. } => items.as_slice(),
                Set::Empty(_) => panic!("empty"),
            };
            assert_eq!(items.len(), count);
            if pattern == "[a-c\\-]" {
                let Item::Range { span, start, end } = &items[0] else {
                    panic!("range");
                };
                assert_eq!((span.start.offset, span.end.offset), (1, 4));
                assert!(matches!(start.kind, Kind::Literal { c: 'a', .. }));
                assert!(matches!(end.kind, Kind::Literal { c: 'c', .. }));
            }
            if pattern == "[.\\$]" {
                assert!(matches!(
                    &items[0],
                    Item::Literal(Primitive {
                        kind: Kind::Literal { c: '.', .. },
                        ..
                    })
                ));
            }
        }
    }
    #[test]
    fn invalid_range_owns_pattern_at_reached_span() {
        let Err(failure) = parse(
            &mut Cursor::start("[z-a]"),
            &mut Scratch::new(),
            &mut Stack::new(),
            &mut Session::new(&mut Account),
        )
        .unwrap() else {
            panic!("range error");
        };
        assert_eq!(failure.pattern, "[z-a]");
        assert_eq!((failure.span.start.offset, failure.span.end.offset), (1, 4));
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
    fn class_refusal_is_terminal_including_range_error() {
        for pattern in ["[a-c\\-]", "[z-a]"] {
            let mut complete = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            let _ = parse(
                &mut Cursor::start(pattern),
                &mut Scratch::new(),
                &mut Stack::new(),
                &mut Session::new(&mut complete),
            )
            .unwrap();
            for refuse in 0..complete.calls {
                let mut count = Count { calls: 0, refuse };
                let mut session = Session::new(&mut count);
                assert!(matches!(
                    parse(
                        &mut Cursor::start(pattern),
                        &mut Scratch::new(),
                        &mut Stack::new(),
                        &mut session
                    ),
                    Err(Stop::Refused)
                ));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                assert_eq!(count.calls, refuse + 1);
            }
        }
    }

    #[test]
    fn successive_classes_reuse_continuation_capacity() {
        struct Allocations(usize);
        impl Admission for Allocations {
            type Error = ();
            fn admit(&mut self, request: rawr_toml::document::Request) -> Result<(), ()> {
                if matches!(request, rawr_toml::document::Request::Allocation { .. }) {
                    self.0 += 1;
                }
                Ok(())
            }
        }
        let mut account = Allocations(0);
        let mut stack = Stack::new();
        let mut scratch = Scratch::new();
        assert!(parse(
            &mut Cursor::start("[a]"),
            &mut scratch,
            &mut stack,
            &mut Session::new(&mut account)
        )
        .unwrap()
        .is_ok());
        let first = account.0;
        assert!(parse(
            &mut Cursor::start("[b]"),
            &mut scratch,
            &mut stack,
            &mut Session::new(&mut account)
        )
        .unwrap()
        .is_ok());
        // Both classes own an item buffer; only the first grows the parser's
        // retained continuation buffer.
        assert_eq!(first, 2);
        assert_eq!(account.0 - first, 1);
    }
}
