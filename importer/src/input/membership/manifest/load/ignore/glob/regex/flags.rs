// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../LICENSE-regex-syntax-MIT.
//! Pinned regex-syntax 0.8.11 parse_flags and Flags operations.
use super::cursor::{Cursor, Span};
use crate::input::membership::manifest::{
    load::formatted, session::Session, storage::Buffer, Step,
};
use rawr_toml::document::Admission;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flag {
    CaseInsensitive,
    MultiLine,
    DotMatchesNewLine,
    SwapGreed,
    Unicode,
    Crlf,
    IgnoreWhitespace,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Negation,
    Flag(Flag),
}
pub(super) struct Item {
    span: Span,
    pub(super) kind: Kind,
}
pub(super) struct Flags {
    pub(super) span: Span,
    pub(super) items: Buffer<Item>,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Invalid {
    RepeatedNegation { original: Span },
    Duplicate { original: Span },
    UnexpectedEof,
    DanglingNegation,
    Unrecognized,
}
pub(super) struct Failure {
    pub(super) kind: Invalid,
    pub(super) pattern: String,
    pub(super) span: Span,
}
fn failure<A: Admission>(
    cursor: &Cursor<'_>,
    kind: Invalid,
    span: Span,
    session: &mut Session<'_, A>,
) -> Step<Result<Flags, Failure>> {
    let pattern = formatted(format_args!("{}", cursor.pattern()), session)?;
    Ok(Err(Failure {
        kind,
        pattern,
        span,
    }))
}
impl Flags {
    pub(super) fn is_empty(&self) -> bool {
        self.items.len() == 0
    }
    fn add<A: Admission>(
        &mut self,
        item: Item,
        session: &mut Session<'_, A>,
    ) -> Step<Option<usize>> {
        for (index, existing) in self.items.as_slice().iter().enumerate() {
            session.items(1)?;
            if existing.kind == item.kind {
                return Ok(Some(index));
            }
        }
        self.items.push(item, session)?;
        Ok(None)
    }
    pub(super) fn state<A: Admission>(
        &self,
        flag: Flag,
        session: &mut Session<'_, A>,
    ) -> Step<Option<bool>> {
        session.check()?;
        let mut negated = false;
        for item in self.items.as_slice() {
            session.items(1)?;
            match item.kind {
                Kind::Negation => negated = true,
                Kind::Flag(current) if current == flag => return Ok(Some(!negated)),
                _ => {}
            }
        }
        Ok(None)
    }
    /// Entry is immediately after '(?'; leave ':' or ')' for the caller.
    pub(super) fn parse<A: Admission>(
        cursor: &mut Cursor<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        session.check()?;
        let mut flags = Self {
            span: cursor.span(),
            items: Buffer::new(),
        };
        let mut last_negation = None;
        loop {
            let c = cursor.char(session)?;
            if c == ':' || c == ')' {
                break;
            }
            let span = cursor.span_char(session)?;
            let kind = if c == '-' {
                last_negation = Some(span);
                Kind::Negation
            } else {
                last_negation = None;
                let flag = match c {
                    'i' => Flag::CaseInsensitive,
                    'm' => Flag::MultiLine,
                    's' => Flag::DotMatchesNewLine,
                    'U' => Flag::SwapGreed,
                    'u' => Flag::Unicode,
                    'R' => Flag::Crlf,
                    'x' => Flag::IgnoreWhitespace,
                    _ => return failure(cursor, Invalid::Unrecognized, span, session),
                };
                Kind::Flag(flag)
            };
            if let Some(index) = flags.add(Item { span, kind }, session)? {
                let original = flags.items.as_slice()[index].span;
                let invalid = match kind {
                    Kind::Negation => Invalid::RepeatedNegation { original },
                    Kind::Flag(_) => Invalid::Duplicate { original },
                };
                return failure(cursor, invalid, span, session);
            }
            if !cursor.bump(session)? {
                return failure(cursor, Invalid::UnexpectedEof, cursor.span(), session);
            }
        }
        if let Some(span) = last_negation {
            return failure(cursor, Invalid::DanglingNegation, span, session);
        }
        flags.span.end = cursor.position();
        Ok(Ok(flags))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn flag_state_keeps_negation_and_delimiter_position() {
        for (pattern, unicode, insensitive, end) in [
            ("-u)", Some(false), None, 2),
            ("i-u:", Some(false), Some(true), 3),
            (":", None, None, 0),
        ] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let mut cursor = Cursor::start(pattern);
            let Ok(flags) = Flags::parse(&mut cursor, &mut session).unwrap() else {
                panic!("flags");
            };
            assert_eq!(flags.state(Flag::Unicode, &mut session).unwrap(), unicode);
            assert_eq!(
                flags.state(Flag::CaseInsensitive, &mut session).unwrap(),
                insensitive
            );
            assert_eq!(
                flags.state(Flag::IgnoreWhitespace, &mut session).unwrap(),
                None
            );
            assert_eq!(cursor.position().offset, end);
            assert_eq!(flags.span.end, cursor.position());
        }
    }
    #[test]
    fn flag_errors_keep_pattern_and_exact_span() {
        for (pattern, expected, start, end) in [
            ("-)", Invalid::DanglingNegation, 0, 1),
            ("u", Invalid::UnexpectedEof, 1, 1),
            ("q)", Invalid::Unrecognized, 0, 1),
        ] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let Err(error) = Flags::parse(&mut Cursor::start(pattern), &mut session).unwrap()
            else {
                panic!("error");
            };
            assert_eq!(error.kind, expected);
            assert_eq!(error.pattern, pattern);
            assert_eq!(
                (error.span.start.offset, error.span.end.offset),
                (start, end)
            );
        }
        for pattern in ["uu)", "--u)"] {
            let Err(error) =
                Flags::parse(&mut Cursor::start(pattern), &mut Session::new(&mut Account)).unwrap()
            else {
                panic!("duplicate");
            };
            let original = match error.kind {
                Invalid::Duplicate { original } | Invalid::RepeatedNegation { original } => {
                    original
                }
                _ => panic!("kind"),
            };
            assert_eq!((original.start.offset, error.span.start.offset), (0, 1));
        }
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
    fn flag_refusal_covers_success_and_error_ownership() {
        for pattern in ["i-u)", "uu)", "--u)", "-)", "u", "q)"] {
            let mut complete = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            let _ = Flags::parse(
                &mut Cursor::start(pattern),
                &mut Session::new(&mut complete),
            )
            .unwrap();
            for refuse in 0..complete.calls {
                let mut count = Count { calls: 0, refuse };
                let mut session = Session::new(&mut count);
                assert!(matches!(
                    Flags::parse(&mut Cursor::start(pattern), &mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                assert_eq!(count.calls, refuse + 1);
            }
        }
    }
}
