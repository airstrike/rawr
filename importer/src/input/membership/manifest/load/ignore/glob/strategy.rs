// Copyright the ripgrep contributors. MIT OR Unlicense.
//! globset 0.4.18 strategy selection, with Cargo's fixed glob options.
use super::{Parsed, Token};
use crate::input::membership::manifest::{session::Session, storage::Buffer, Error, Step};
use rawr_toml::document::Admission;

pub(super) enum Strategy {
    Basename(Literal),
    Literal(Literal),
    Extension(Literal),
    Prefix(Literal),
    Suffix { literal: Literal, component: bool },
    RequiredExtension(Literal),
    Regex,
}
pub(super) struct Literal(Buffer<u8>);
impl Literal {
    fn new() -> Self {
        Self(Buffer::new())
    }
    pub(super) fn bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    /// The strategy routines build complete UTF-8 scalars. Move that existing
    /// allocation into the source String, charging the safe validation scan.
    pub(super) fn into_string<A: Admission>(self, session: &mut Session<'_, A>) -> Step<String> {
        session.items(self.0.len())?;
        match String::from_utf8(self.0.into_bytes()) {
            Ok(text) => Ok(text),
            Err(_) => session.fail(Error::State),
        }
    }
    fn push<A: Admission>(&mut self, c: char, session: &mut Session<'_, A>) -> Step<()> {
        let mut encoded = [0; 4];
        let bytes = c.encode_utf8(&mut encoded).as_bytes();
        let Some(length) = self.0.len().checked_add(bytes.len()) else {
            return session.fail(Error::Overflow);
        };
        session.string(length)?;
        for &byte in bytes {
            self.0.push(byte, session)?;
        }
        Ok(())
    }
}
impl Parsed {
    pub(super) fn strategy<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Strategy> {
        session.check()?;
        let Some(tokens) = self.branches.as_slice().get(self.root) else {
            return session.fail(Error::State);
        };
        let tokens = tokens.as_slice();
        if let Some(lit) = basename(tokens, session)? {
            return Ok(Strategy::Basename(lit));
        }
        if let Some(lit) = literal(tokens, session)? {
            return Ok(Strategy::Literal(lit));
        }
        if let Some(lit) = extension(tokens, session)? {
            return Ok(Strategy::Extension(lit));
        }
        if let Some(lit) = prefix(tokens, session)? {
            return Ok(Strategy::Prefix(lit));
        }
        if let Some((literal, component)) = suffix(tokens, session)? {
            return Ok(Strategy::Suffix { literal, component });
        }
        if let Some(lit) = required_extension(tokens, session)? {
            return Ok(Strategy::RequiredExtension(lit));
        }
        Ok(Strategy::Regex)
    }
}
fn basename<A: Admission>(tokens: &[Token], session: &mut Session<'_, A>) -> Step<Option<Literal>> {
    session.items(1)?;
    if !matches!(tokens.first(), Some(Token::RecursivePrefix)) || tokens.len() == 1 {
        return Ok(None);
    }
    let tokens = &tokens[1..];
    // Upstream validates the entire basename token slice before collecting it.
    for token in tokens {
        session.items(1)?;
        match token {
            Token::Literal('/') => return Ok(None),
            Token::Literal(_) | Token::Any | Token::ZeroOrMore => {}
            _ => return Ok(None),
        }
    }
    literal(tokens, session)
}
fn literal<A: Admission>(tokens: &[Token], session: &mut Session<'_, A>) -> Step<Option<Literal>> {
    session.items(1)?;
    let mut lit = Literal::new();
    for token in tokens {
        session.items(1)?;
        let Token::Literal(c) = token else {
            return Ok(None);
        };
        lit.push(*c, session)?;
    }
    Ok((!lit.bytes().is_empty()).then_some(lit))
}
fn extension<A: Admission>(
    tokens: &[Token],
    session: &mut Session<'_, A>,
) -> Step<Option<Literal>> {
    session.items(1)?;
    // With literal_separator=true, this optimization requires a recursive prefix.
    if !matches!(tokens.first(), Some(Token::RecursivePrefix))
        || !matches!(tokens.get(1), Some(Token::ZeroOrMore))
        || !matches!(tokens.get(2), Some(Token::Literal('.')))
    {
        return Ok(None);
    }
    let mut lit = Literal::new();
    lit.push('.', session)?;
    for token in &tokens[3..] {
        session.items(1)?;
        match token {
            Token::Literal('.' | '/') => return Ok(None),
            Token::Literal(c) => lit.push(*c, session)?,
            _ => return Ok(None),
        }
    }
    Ok(Some(lit))
}
fn prefix<A: Admission>(tokens: &[Token], session: &mut Session<'_, A>) -> Step<Option<Literal>> {
    session.items(1)?;
    let Some(last) = tokens.last() else {
        return Ok(None);
    };
    let (end, separator) = match last {
        Token::ZeroOrMore => return Ok(None),
        Token::RecursiveSuffix => (tokens.len() - 1, true),
        _ => (tokens.len(), false),
    };
    let mut lit = Literal::new();
    for token in &tokens[..end] {
        session.items(1)?;
        let Token::Literal(c) = token else {
            return Ok(None);
        };
        lit.push(*c, session)?;
    }
    if separator {
        lit.push('/', session)?;
    }
    Ok((!lit.bytes().is_empty()).then_some(lit))
}
fn suffix<A: Admission>(
    tokens: &[Token],
    session: &mut Session<'_, A>,
) -> Step<Option<(Literal, bool)>> {
    session.items(1)?;
    let Some(first) = tokens.first() else {
        return Ok(None);
    };
    let mut lit = Literal::new();
    let (start, component) = match first {
        Token::RecursivePrefix if matches!(tokens.get(1), Some(Token::Literal(_))) => {
            lit.push('/', session)?;
            (1, true)
        }
        Token::RecursivePrefix => (1, false),
        _ => (0, false),
    };
    let Some(token) = tokens.get(start) else {
        return Ok(None);
    };
    if matches!(token, Token::ZeroOrMore) {
        return Ok(None);
    }
    for token in &tokens[start..] {
        session.items(1)?;
        let Token::Literal(c) = token else {
            return Ok(None);
        };
        lit.push(*c, session)?;
    }
    Ok((!lit.bytes().is_empty() && lit.bytes() != b"/").then_some((lit, component)))
}
fn required_extension<A: Admission>(
    tokens: &[Token],
    session: &mut Session<'_, A>,
) -> Step<Option<Literal>> {
    session.items(1)?;
    let mut reversed = Buffer::new();
    for token in tokens.iter().rev() {
        session.items(1)?;
        match token {
            Token::Literal('/') => return Ok(None),
            Token::Literal(c) => {
                reversed.push(*c, session)?;
                if *c == '.' {
                    break;
                }
            }
            _ => return Ok(None),
        }
    }
    if reversed.as_slice().last() != Some(&'.') {
        return Ok(None);
    }
    session.items(reversed.len())?;
    reversed.as_mut_slice().reverse();
    let mut lit = Literal::new();
    for &c in reversed.as_slice() {
        lit.push(c, session)?;
    }
    Ok(Some(lit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn pinned_strategy_precedence_and_literals() {
        for (glob, kind, bytes, component) in [
            ("**/foo", "basename", "foo", false),
            ("foo", "literal", "foo", false),
            ("**/*.rs", "extension", ".rs", false),
            ("**/*.", "extension", ".", false),
            ("foo/**", "prefix", "foo/", false),
            ("**/foo/bar", "suffix", "/foo/bar", true),
            ("*.rs", "required", ".rs", false),
            ("**/*.tar.gz", "required", ".gz", false),
            ("foo?.é", "required", ".é", false),
            ("a*b", "regex", "", false),
            ("**", "regex", "", false),
            ("", "regex", "", false),
            ("foo/*", "regex", "", false),
            ("{foo,bar}", "regex", "", false),
        ] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let Ok(parsed) = super::super::parse(glob, &mut session).unwrap() else {
                panic!("parse");
            };
            let strategy = parsed.strategy(&mut session).unwrap();
            let (actual, literal, entire) = match &strategy {
                Strategy::Basename(lit) => ("basename", lit.bytes(), false),
                Strategy::Literal(lit) => ("literal", lit.bytes(), false),
                Strategy::Extension(lit) => ("extension", lit.bytes(), false),
                Strategy::Prefix(lit) => ("prefix", lit.bytes(), false),
                Strategy::Suffix { literal, component } => ("suffix", literal.bytes(), *component),
                Strategy::RequiredExtension(lit) => ("required", lit.bytes(), false),
                Strategy::Regex => ("regex", &b""[..], false),
            };
            assert_eq!(
                (actual, literal, entire),
                (kind, bytes.as_bytes(), component),
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
    fn refusal_is_terminal_during_strategy_selection() {
        for glob in [
            "**/name",
            "literal",
            "**/*.rs",
            "prefix/**",
            "**/a/b",
            "*é.tar.gz",
            "a*b",
        ] {
            let Ok(parsed) = super::super::parse(glob, &mut Session::new(&mut Account)).unwrap()
            else {
                panic!("parse");
            };
            let mut complete = Count {
                calls: 0,
                refuse: usize::MAX,
            };
            parsed.strategy(&mut Session::new(&mut complete)).unwrap();
            for refuse in 0..complete.calls {
                let mut account = Count { calls: 0, refuse };
                let mut session = Session::new(&mut account);
                assert!(matches!(parsed.strategy(&mut session), Err(Stop::Refused)));
                assert!(matches!(session.check(), Err(Stop::Refused)));
                assert_eq!(account.calls, refuse + 1);
            }
        }
    }
}
