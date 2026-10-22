// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../LICENSE-regex-syntax-MIT.
//! regex-syntax 0.8.11 ast/parse.rs cursor operations under one Session.
//! This cursor is only a parser substrate; it does not accept a regex or glob.
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::Admission;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Position {
    pub(super) offset: usize,
    pub(super) line: usize,
    pub(super) column: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Span {
    pub(super) start: Position,
    pub(super) end: Position,
}
pub(super) struct Cursor<'a> {
    pattern: &'a str,
    position: Position,
}
impl<'a> Cursor<'a> {
    pub(super) fn pattern(&self) -> &'a str {
        self.pattern
    }
    pub(super) fn start(pattern: &'a str) -> Self {
        Self {
            pattern,
            position: Position {
                offset: 0,
                line: 1,
                column: 1,
            },
        }
    }
    pub(super) fn position(&self) -> Position {
        self.position
    }
    pub(super) fn eof(&self) -> bool {
        self.position.offset == self.pattern.len()
    }
    pub(super) fn span(&self) -> Span {
        Span {
            start: self.position,
            end: self.position,
        }
    }
    pub(super) fn char<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<char> {
        session.items(1)?;
        match self
            .pattern
            .get(self.position.offset..)
            .and_then(|rest| rest.chars().next())
        {
            Some(c) => Ok(c),
            None => session.fail(Error::State),
        }
    }
    pub(super) fn bump<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<bool> {
        session.items(1)?;
        if self.eof() {
            return Ok(false);
        }
        let c = self.char(session)?;
        let Position {
            offset,
            line,
            column,
        } = self.position;
        let (line, column) = if c == '\n' {
            let Some(line) = line.checked_add(1) else {
                return session.fail(Error::Overflow);
            };
            (line, 1)
        } else {
            let Some(column) = column.checked_add(1) else {
                return session.fail(Error::Overflow);
            };
            (line, column)
        };
        let Some(offset) = offset.checked_add(c.len_utf8()) else {
            return session.fail(Error::Overflow);
        };
        self.position = Position {
            offset,
            line,
            column,
        };
        Ok(!self.eof())
    }
    pub(super) fn bump_if<A: Admission>(
        &mut self,
        prefix: &str,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(prefix.len())?;
        let Some(rest) = self.pattern.get(self.position.offset..) else {
            return session.fail(Error::State);
        };
        if !rest.starts_with(prefix) {
            return Ok(false);
        }
        session.items(prefix.len())?;
        let count = prefix.chars().count();
        for _ in 0..count {
            self.bump(session)?;
        }
        Ok(true)
    }
    pub(super) fn peek<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Option<char>> {
        session.items(1)?;
        if self.eof() {
            return Ok(None);
        }
        let c = self.char(session)?;
        let Some(offset) = self.position.offset.checked_add(c.len_utf8()) else {
            return session.fail(Error::Overflow);
        };
        let Some(rest) = self.pattern.get(offset..) else {
            return session.fail(Error::State);
        };
        Ok(rest.chars().next())
    }
    pub(super) fn span_char<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Span> {
        let c = self.char(session)?;
        let Some(offset) = self.position.offset.checked_add(c.len_utf8()) else {
            return session.fail(Error::Overflow);
        };
        let Some(column) = self.position.column.checked_add(1) else {
            return session.fail(Error::Overflow);
        };
        let mut next = Position {
            offset,
            line: self.position.line,
            column,
        };
        if c == '\n' {
            let Some(line) = next.line.checked_add(1) else {
                return session.fail(Error::Overflow);
            };
            next.line = line;
            next.column = 1;
        }
        Ok(Span {
            start: self.position,
            end: next,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn positions_count_scalars_and_newlines_with_byte_offsets() {
        let mut cursor = Cursor::start("é\nλ");
        let mut account = Account;
        let mut session = Session::new(&mut account);
        for (c, offset, line, column, next) in [
            ('é', 0, 1, 1, Some('\n')),
            ('\n', 2, 1, 2, Some('λ')),
            ('λ', 3, 2, 1, None),
        ] {
            assert_eq!(
                cursor.position(),
                Position {
                    offset,
                    line,
                    column
                }
            );
            assert_eq!(cursor.char(&mut session).unwrap(), c);
            assert_eq!(cursor.peek(&mut session).unwrap(), next);
            let span = cursor.span_char(&mut session).unwrap();
            assert_eq!(span.start, cursor.position());
            assert_eq!(cursor.bump(&mut session).unwrap(), next.is_some());
            assert_eq!(span.end, cursor.position());
        }
        assert!(cursor.eof());
        assert!(!cursor.bump(&mut session).unwrap());
        assert_eq!(cursor.peek(&mut session).unwrap(), None);
        assert_eq!(cursor.span().start, cursor.span().end);
    }
    #[test]
    fn prefix_bump_preserves_position_on_miss_and_at_eof() {
        let mut cursor = Cursor::start("(?-u)^\n");
        let mut account = Account;
        let mut session = Session::new(&mut account);
        assert!(!cursor.bump_if("(?:", &mut session).unwrap());
        assert_eq!(cursor.position().offset, 0);
        assert!(cursor.bump_if("(?-u)", &mut session).unwrap());
        assert_eq!(cursor.position().offset, 5);
        assert!(cursor.bump_if("^\n", &mut session).unwrap());
        assert_eq!(
            cursor.position(),
            Position {
                offset: 7,
                line: 2,
                column: 1
            }
        );
        assert!(cursor.bump_if("", &mut session).unwrap());
        assert!(!cursor.bump_if("x", &mut session).unwrap());
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
    fn run<A: Admission>(session: &mut Session<'_, A>) -> Step<()> {
        let mut cursor = Cursor::start("(?-u)^\né");
        cursor.bump_if("(?-u)", session)?;
        while !cursor.eof() {
            cursor.peek(session)?;
            cursor.span_char(session)?;
            cursor.bump(session)?;
        }
        Ok(())
    }
    #[test]
    fn refusal_stops_cursor_operations() {
        let mut complete = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        run(&mut Session::new(&mut complete)).unwrap();
        for refuse in 0..complete.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(run(&mut session), Err(Stop::Refused)));
            assert!(matches!(
                Cursor::start("a").peek(&mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
