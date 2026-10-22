// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../LICENSE-regex-syntax-MIT.
//! regex-syntax parse_group for emitted flag and noncapturing openings.
//! The emitter never produces captures or lookarounds. This is an opening
//! transformation, not complete group parsing or a regex acceptance boundary.
use super::{
    ast::{Arena, Group, Node},
    cursor::{Cursor, Span},
    flags::{self, Flags},
};
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::Admission;

pub(super) enum Opening {
    Flags { span: Span, flags: Flags },
    Group(Group),
}
pub(super) fn parse<A: Admission>(
    cursor: &mut Cursor<'_>,
    arena: &mut Arena,
    session: &mut Session<'_, A>,
) -> Step<Result<Opening, flags::Failure>> {
    if cursor.char(session)? != '(' {
        return session.fail(Error::State);
    }
    let span = cursor.span_char(session)?;
    cursor.bump(session)?;
    // Whitespace mode is false for the emitted grammar. Retain the pinned
    // probes and their short-circuit order before flag parsing.
    if cursor.bump_if("?=", session)?
        || cursor.bump_if("?!", session)?
        || cursor.bump_if("?<=", session)?
        || cursor.bump_if("?<!", session)?
    {
        return session.fail(Error::State);
    }
    if cursor.bump_if("?P<", session)? || cursor.bump_if("?<", session)? {
        return session.fail(Error::State);
    }
    if !cursor.bump_if("?", session)? || cursor.eof() {
        return session.fail(Error::State);
    }
    let flags = match Flags::parse(cursor, session)? {
        Ok(flags) => flags,
        Err(failure) => return Ok(Err(failure)),
    };
    let delimiter = cursor.char(session)?;
    cursor.bump(session)?;
    if delimiter == ')' {
        if flags.is_empty() {
            return session.fail(Error::State);
        }
        Ok(Ok(Opening::Flags {
            span: Span {
                start: span.start,
                end: cursor.position(),
            },
            flags,
        }))
    } else {
        if delimiter != ':' {
            return session.fail(Error::State);
        }
        // The source constructs this placeholder before pushing the group
        // continuation; the caller later replaces it with the parsed child.
        let child = arena.push(Node::Empty(cursor.span()), session)?;
        Ok(Ok(Opening::Group(Group { span, flags, child })))
    }
}

#[cfg(test)]
mod tests {
    use super::super::flags::Flag;
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn emitted_openings_preserve_flags_and_placeholder_position() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut arena = Arena::new();
        let mut cursor = Cursor::start("(?-u)^(?:a)$");
        let Ok(Opening::Flags { span, flags }) =
            parse(&mut cursor, &mut arena, &mut session).unwrap()
        else {
            panic!("flags");
        };
        assert_eq!((span.start.offset, span.end.offset), (0, 5));
        assert_eq!(
            flags.state(Flag::Unicode, &mut session).unwrap(),
            Some(false)
        );
        assert_eq!(cursor.char(&mut session).unwrap(), '^');
        cursor.bump(&mut session).unwrap();
        let Ok(Opening::Group(group)) = parse(&mut cursor, &mut arena, &mut session).unwrap()
        else {
            panic!("group");
        };
        assert_eq!((group.span.start.offset, group.span.end.offset), (6, 7));
        assert!(group.flags.is_empty());
        let Node::Empty(span) = arena.get(group.child, &mut session).unwrap() else {
            panic!("placeholder");
        };
        assert_eq!((span.start.offset, span.end.offset), (9, 9));
        assert_eq!(cursor.char(&mut session).unwrap(), 'a');
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
    #[test]
    fn refusal_stops_opening_and_placeholder_construction() {
        for pattern in ["(?-u)", "(?:a)"] {
            let mut complete = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            assert!(parse(
                &mut Cursor::start(pattern),
                &mut Arena::new(),
                &mut Session::new(&mut complete)
            )
            .unwrap()
            .is_ok());
            for refuse in 0..complete.calls {
                let mut count = Count { calls: 0, refuse };
                let mut session = Session::new(&mut count);
                assert!(matches!(
                    parse(&mut Cursor::start(pattern), &mut Arena::new(), &mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                assert_eq!(count.calls, refuse + 1);
            }
        }
    }
}
