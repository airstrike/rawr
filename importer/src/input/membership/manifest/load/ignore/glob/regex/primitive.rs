// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../LICENSE-regex-syntax-MIT.
//! Reached regex-syntax 0.8.11 primitive parsing for globset-emitted patterns.
//! Only the emitter produces input to this slice. Its escapes are meta escapes
//! or two lowercase hex digits; other regex escape grammars are not entered.
use super::cursor::{Cursor, Span};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum LiteralKind {
    Verbatim,
    Meta,
    HexFixed,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Literal { kind: LiteralKind, c: char },
    Dot,
    StartLine,
    EndLine,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Primitive {
    pub(super) span: Span,
    pub(super) kind: Kind,
}
/// Retain scratch ownership between primitives, as the pinned parser does.
pub(super) struct Scratch(Buffer<u8>);
impl Scratch {
    pub(super) fn class_item<A: Admission>(
        &mut self,
        cursor: &mut Cursor<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Primitive> {
        let c = cursor.char(session)?;
        if c == '\\' {
            return self.escape(cursor, session);
        }
        let span = cursor.span_char(session)?;
        cursor.bump(session)?;
        Ok(Primitive {
            span,
            kind: Kind::Literal {
                kind: LiteralKind::Verbatim,
                c,
            },
        })
    }
    pub(super) fn new() -> Self {
        Self(Buffer::new())
    }
    pub(super) fn parse<A: Admission>(
        &mut self,
        cursor: &mut Cursor<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Primitive> {
        let c = cursor.char(session)?;
        if c == '\\' {
            return self.escape(cursor, session);
        }
        let span = cursor.span_char(session)?;
        let kind = match c {
            '.' => Kind::Dot,
            '^' => Kind::StartLine,
            '$' => Kind::EndLine,
            _ => Kind::Literal {
                kind: LiteralKind::Verbatim,
                c,
            },
        };
        cursor.bump(session)?;
        Ok(Primitive { span, kind })
    }
    fn escape<A: Admission>(
        &mut self,
        cursor: &mut Cursor<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Primitive> {
        let start = cursor.position();
        if !cursor.bump(session)? {
            return session.fail(Error::State);
        }
        let c = cursor.char(session)?;
        if c == 'x' {
            // Emission always writes exactly two hex digits and never brace
            // syntax, whitespace mode, Unicode escapes or special escapes.
            if !cursor.bump(session)? {
                return session.fail(Error::State);
            }
            self.0.clear();
            for index in 0..2 {
                if index > 0 && !cursor.bump(session)? {
                    return session.fail(Error::State);
                }
                let c = cursor.char(session)?;
                if !c.is_ascii_hexdigit() {
                    return session.fail(Error::State);
                }
                session.string(index + 1)?;
                self.0.push(c as u8, session)?;
            }
            cursor.bump(session)?;
            session.items(self.0.len())?;
            let mut value = 0u8;
            for &byte in self.0.as_slice() {
                let digit = match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    b'A'..=b'F' => byte - b'A' + 10,
                    _ => return session.fail(Error::State),
                };
                value = (value << 4) | digit;
            }
            return Ok(Primitive {
                span: Span {
                    start,
                    end: cursor.position(),
                },
                kind: Kind::Literal {
                    kind: LiteralKind::HexFixed,
                    c: char::from(value),
                },
            });
        }
        cursor.bump(session)?;
        if !matches!(
            c,
            '\\' | '.'
                | '+'
                | '*'
                | '?'
                | '('
                | ')'
                | '|'
                | '['
                | ']'
                | '{'
                | '}'
                | '^'
                | '$'
                | '#'
                | '&'
                | '-'
                | '~'
        ) {
            return session.fail(Error::State);
        }
        Ok(Primitive {
            span: Span {
                start,
                end: cursor.position(),
            },
            kind: Kind::Literal {
                kind: LiteralKind::Meta,
                c,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn primitives_preserve_kind_and_full_escape_spans() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut cursor = Cursor::start("^a\\.\\xc3\\xa9.\n$");
        let mut scratch = Scratch::new();
        for (start, end, kind) in [
            (0, 1, Kind::StartLine),
            (
                1,
                2,
                Kind::Literal {
                    kind: LiteralKind::Verbatim,
                    c: 'a',
                },
            ),
            (
                2,
                4,
                Kind::Literal {
                    kind: LiteralKind::Meta,
                    c: '.',
                },
            ),
            (
                4,
                8,
                Kind::Literal {
                    kind: LiteralKind::HexFixed,
                    c: '\u{c3}',
                },
            ),
            (
                8,
                12,
                Kind::Literal {
                    kind: LiteralKind::HexFixed,
                    c: '\u{a9}',
                },
            ),
            (12, 13, Kind::Dot),
            (
                13,
                14,
                Kind::Literal {
                    kind: LiteralKind::Verbatim,
                    c: '\n',
                },
            ),
            (14, 15, Kind::EndLine),
        ] {
            let primitive = scratch.parse(&mut cursor, &mut session).unwrap();
            assert_eq!(
                (primitive.span.start.offset, primitive.span.end.offset),
                (start, end)
            );
            assert_eq!(primitive.kind, kind);
        }
        assert!(cursor.eof());
        assert_eq!(cursor.position().line, 2);
        assert_eq!(cursor.position().column, 2);
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
        let mut cursor = Cursor::start("^\\xc3\\xa9\\.$");
        let mut scratch = Scratch::new();
        while !cursor.eof() {
            scratch.parse(&mut cursor, session)?;
        }
        Ok(())
    }
    #[test]
    fn refusal_stops_primitive_and_scratch_work() {
        let mut complete = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        run(&mut Session::new(&mut complete)).unwrap();
        for refuse in 0..complete.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(run(&mut session), Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
