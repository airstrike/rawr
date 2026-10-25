// Copyright the ripgrep contributors. MIT OR Unlicense.
//! Pinned globset 0.4.18 regex emission with an explicit continuation stack.
//! Options are Cargo's literal_separator=true, case_insensitive=false and
//! empty_alternates=false. Regex compilation still follows this transformation.
use super::{Parsed, Token};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;
mod ast;
pub(super) mod batch;
mod class;
mod cursor;
mod flags;
mod group;
mod nesting;
pub(super) mod nfa;
mod parser;
mod primitive;
mod translate;

pub(super) struct Regex(Text);
impl Regex {
    fn tree<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<parser::Parsed, parser::Failure>> {
        tree(self.bytes(), session)
    }
    pub(super) fn bytes(&self) -> &[u8] {
        self.0 .0.as_slice()
    }
}

/// One borrowed parser entry for emitted storage and the source-owned strings
/// retained by GlobSet strategy builders. Neither path copies the regex text.
fn tree<A: Admission>(
    bytes: &[u8],
    session: &mut Session<'_, A>,
) -> Step<Result<parser::Parsed, parser::Failure>> {
    session.items(bytes.len())?;
    let Ok(pattern) = std::str::from_utf8(bytes) else {
        return session.fail(Error::State);
    };
    parser::parse(pattern, session)
}
struct Text(Buffer<u8>);
impl Text {
    fn new() -> Self {
        Self(Buffer::new())
    }
    fn append<A: Admission>(&mut self, bytes: &[u8], session: &mut Session<'_, A>) -> Step<()> {
        let Some(length) = self.0.len().checked_add(bytes.len()) else {
            return session.fail(Error::Overflow);
        };
        session.string(length)?;
        for &byte in bytes {
            self.0.push(byte, session)?;
        }
        Ok(())
    }
    fn literal<A: Admission>(&mut self, c: char, session: &mut Session<'_, A>) -> Step<()> {
        let mut encoded = [0; 4];
        for &byte in c.encode_utf8(&mut encoded).as_bytes() {
            if byte <= 0x7f {
                // regex-syntax 0.8.11 lib.rs:262, the exact escape_into predicate.
                if matches!(
                    byte,
                    b'\\'
                        | b'.'
                        | b'+'
                        | b'*'
                        | b'?'
                        | b'('
                        | b')'
                        | b'|'
                        | b'['
                        | b']'
                        | b'{'
                        | b'}'
                        | b'^'
                        | b'$'
                        | b'#'
                        | b'&'
                        | b'-'
                        | b'~'
                ) {
                    self.append(b"\\", session)?;
                }
                self.append(&[byte], session)?;
            } else {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                self.append(
                    &[
                        b'\\',
                        b'x',
                        HEX[(byte >> 4) as usize],
                        HEX[(byte & 15) as usize],
                    ],
                    session,
                )?;
            }
        }
        Ok(())
    }
}
struct Alternatives<'a> {
    branches: &'a [usize],
    next: usize,
    parts: Buffer<Text>,
}
struct Frame<'a> {
    branch: usize,
    next: usize,
    output: Text,
    pending: Option<Alternatives<'a>>,
}
impl Frame<'_> {
    fn new(branch: usize) -> Self {
        Self {
            branch,
            next: 0,
            output: Text::new(),
            pending: None,
        }
    }
}
impl Parsed {
    pub(super) fn regex<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Regex> {
        session.check()?;
        let Some(root) = self.branches.as_slice().get(self.root) else {
            return session.fail(Error::State);
        };
        let mut frame = Frame::new(self.root);
        frame.output.append(b"(?-u)^", session)?;
        if matches!(root.as_slice(), [Token::RecursivePrefix]) {
            frame.output.append(b".*$", session)?;
            return Ok(Regex(frame.output));
        }
        let mut parents: Buffer<Frame<'_>> = Buffer::new();
        loop {
            session.items(1)?;
            if let Some(pending) = &mut frame.pending {
                if let Some(&branch) = pending.branches.get(pending.next) {
                    pending.next += 1;
                    parents.push(frame, session)?;
                    frame = Frame::new(branch);
                    continue;
                }
                let Some(pending) = frame.pending.take() else {
                    return session.fail(Error::State);
                };
                if !pending.parts.as_slice().is_empty() {
                    frame.output.append(b"(?:", session)?;
                    let mut joined = Text::new();
                    for (i, part) in pending.parts.as_slice().iter().enumerate() {
                        if i != 0 {
                            joined.append(b"|", session)?;
                        }
                        joined.append(part.0.as_slice(), session)?;
                    }
                    frame.output.append(joined.0.as_slice(), session)?;
                    frame.output.append(b")", session)?;
                }
            }
            let Some(branch) = self.branches.as_slice().get(frame.branch) else {
                return session.fail(Error::State);
            };
            let Some(token) = branch.as_slice().get(frame.next) else {
                let Some(mut parent) = parents.pop() else {
                    frame.output.append(b"$", session)?;
                    return Ok(Regex(frame.output));
                };
                let Some(pending) = &mut parent.pending else {
                    return session.fail(Error::State);
                };
                if !frame.output.0.as_slice().is_empty() {
                    pending.parts.push(frame.output, session)?;
                }
                frame = parent;
                continue;
            };
            frame.next += 1;
            match token {
                Token::Literal(c) => frame.output.literal(*c, session)?,
                Token::Any => frame.output.append(b"[^/]", session)?,
                Token::ZeroOrMore => frame.output.append(b"[^/]*", session)?,
                Token::RecursivePrefix => frame.output.append(b"(?:/?|.*/)", session)?,
                Token::RecursiveSuffix => frame.output.append(b"/.*", session)?,
                Token::RecursiveZeroOrMore => frame.output.append(b"(?:/|/.*/)", session)?,
                Token::Class { negated, ranges } => {
                    frame.output.append(b"[", session)?;
                    if *negated {
                        frame.output.append(b"^", session)?;
                    }
                    for &(a, b) in ranges.as_slice() {
                        frame.output.literal(a, session)?;
                        if a != b {
                            frame.output.append(b"-", session)?;
                            frame.output.literal(b, session)?;
                        }
                    }
                    frame.output.append(b"]", session)?;
                }
                Token::Alternates(branches) => {
                    frame.pending = Some(Alternatives {
                        branches: branches.as_slice(),
                        next: 0,
                        parts: Buffer::new(),
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn emission_preserves_pinned_empty_alternatives_and_byte_escaping() {
        for (glob, expected) in [
            ("**", "(?-u)^.*$"),
            ("a/**/b?", "(?-u)^a(?:/|/.*/)b[^/]$"),
            ("{,a,,}", "(?-u)^(?:a)$"),
            ("{{,},}", "(?-u)^$"),
            ("a{b,{c,d},}e", "(?-u)^a(?:b|(?:c|d))e$"),
            ("{**,a}", "(?-u)^(?:[^/]*[^/]*|a)$"),
            ("é", "(?-u)^\\xc3\\xa9$"),
            ("[!a-c-]", "(?-u)^[^a-c\\-]$"),
            ("\\#\\&\\~", "(?-u)^\\#\\&\\~$"),
        ] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let Ok(parsed) = super::super::parse(glob, &mut session).unwrap() else {
                panic!("parse")
            };
            assert_eq!(
                parsed.regex(&mut session).unwrap().bytes(),
                expected.as_bytes(),
                "{glob}"
            );
        }
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
    fn emission_refuses_before_each_reached_growth() {
        let Ok(parsed) =
            super::super::parse("a{b,{é,},}*", &mut Session::new(&mut Account)).unwrap()
        else {
            panic!("parse")
        };
        let mut complete = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        parsed.regex(&mut Session::new(&mut complete)).unwrap();
        for refuse in 0..complete.calls {
            let mut account = Count { calls: 0, refuse };
            let mut session = Session::new(&mut account);
            assert!(matches!(parsed.regex(&mut session), Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(account.calls, refuse + 1);
        }
    }
}
