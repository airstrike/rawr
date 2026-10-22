// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../LICENSE-regex-syntax-MIT.
//! Pinned translator flag and literal semantics. HIR construction follows these
//! operations; this module does not yet expose a compiled expression.
use super::{
    flags as ast,
    primitive::{Kind, LiteralKind, Primitive},
};
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::Admission;
pub(super) mod bytes;
mod class;
mod hir;
mod info;
mod properties;
pub(super) mod unicode;
pub(super) use hir::view;
pub(super) use info::Properties;

/// Translated expression ownership; this is not an executable matcher.
pub(super) struct Expression(hir::Translated);
impl Expression {
    pub(super) fn any_byte<A: Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        hir::Translated::any_byte(session).map(Self)
    }
    pub(super) fn view(&self) -> view::View<'_> {
        self.0.view()
    }
}
pub(super) struct Failure(class::Failure);
impl Failure {
    pub(super) fn render<A: Admission>(
        &self,
        pattern: usize,
        session: &mut Session<'_, A>,
    ) -> Step<String> {
        crate::input::membership::manifest::load::formatted(format_args!(
            "regex translation error in pattern {} at bytes {}..{}: non-ASCII literal in byte class",
            pattern, self.0.span.start.offset, self.0.span.end.offset,
        ), session)
    }
}
pub(super) fn tree<A: Admission>(
    source: &super::Regex,
    parsed: &super::parser::Parsed,
    session: &mut Session<'_, A>,
) -> Step<Result<Expression, Failure>> {
    bytes(source.bytes(), parsed, session)
}

pub(super) fn bytes<A: Admission>(
    source: &[u8],
    parsed: &super::parser::Parsed,
    session: &mut Session<'_, A>,
) -> Step<Result<Expression, Failure>> {
    session.items(source.len())?;
    let Ok(pattern) = std::str::from_utf8(source) else {
        return session.fail(Error::State);
    };
    hir::Translated::from_tree(parsed, pattern, session)
        .map(|result| result.map(Expression).map_err(Failure))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Flags {
    case_insensitive: Option<bool>,
    multi_line: Option<bool>,
    dot_matches_new_line: Option<bool>,
    swap_greed: Option<bool>,
    unicode: Option<bool>,
    crlf: Option<bool>,
}
impl Flags {
    /// globset's syntax config changes dot behavior; UTF-8 validity is a
    /// separate translator setting and is fixed to false for this producer.
    pub(super) fn initial() -> Self {
        Self {
            dot_matches_new_line: Some(true),
            ..Self::default()
        }
    }
    pub(super) fn set<A: Admission>(
        &mut self,
        flags: &ast::Flags,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let old = *self;
        let mut new = Self::default();
        let mut enable = true;
        for item in flags.items.as_slice() {
            session.items(1)?;
            match item.kind {
                ast::Kind::Negation => enable = false,
                ast::Kind::Flag(ast::Flag::CaseInsensitive) => new.case_insensitive = Some(enable),
                ast::Kind::Flag(ast::Flag::MultiLine) => new.multi_line = Some(enable),
                ast::Kind::Flag(ast::Flag::DotMatchesNewLine) => {
                    new.dot_matches_new_line = Some(enable)
                }
                ast::Kind::Flag(ast::Flag::SwapGreed) => new.swap_greed = Some(enable),
                ast::Kind::Flag(ast::Flag::Unicode) => new.unicode = Some(enable),
                ast::Kind::Flag(ast::Flag::Crlf) => new.crlf = Some(enable),
                ast::Kind::Flag(ast::Flag::IgnoreWhitespace) => {}
            }
        }
        new.case_insensitive = new.case_insensitive.or(old.case_insensitive);
        new.multi_line = new.multi_line.or(old.multi_line);
        new.dot_matches_new_line = new.dot_matches_new_line.or(old.dot_matches_new_line);
        new.swap_greed = new.swap_greed.or(old.swap_greed);
        new.unicode = new.unicode.or(old.unicode);
        new.crlf = new.crlf.or(old.crlf);
        *self = new;
        Ok(old)
    }
    pub(super) fn unicode(self) -> bool {
        self.unicode.unwrap_or(true)
    }
    pub(super) fn case_insensitive(self) -> bool {
        self.case_insensitive.unwrap_or(false)
    }
    pub(super) fn multi_line(self) -> bool {
        self.multi_line.unwrap_or(false)
    }
    pub(super) fn dot_matches_new_line(self) -> bool {
        self.dot_matches_new_line.unwrap_or(false)
    }
    pub(super) fn swap_greed(self) -> bool {
        self.swap_greed.unwrap_or(false)
    }
    pub(super) fn crlf(self) -> bool {
        self.crlf.unwrap_or(false)
    }

    /// ast_literal_to_scalar with globset's utf8=false configuration. A hex
    /// escape above ASCII remains one byte when Unicode mode is disabled.
    pub(super) fn scalar<A: Admission>(
        self,
        literal: &Primitive,
        session: &mut Session<'_, A>,
    ) -> Step<Scalar> {
        session.items(1)?;
        let Kind::Literal { ref kind, c } = literal.kind else {
            return session.fail(Error::State);
        };
        if !self.unicode()
            && matches!(kind, LiteralKind::HexFixed)
            && let Ok(byte) = u8::try_from(c)
            && !byte.is_ascii()
        {
            return Ok(Scalar::Byte(byte));
        }
        Ok(Scalar::Char(c))
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Scalar {
    Char(char),
    Byte(u8),
}

#[cfg(test)]
mod tests {
    use super::super::{cursor::Cursor, primitive::Scratch};
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};

    fn parsed(source: &str) -> ast::Flags {
        ast::Flags::parse(&mut Cursor::start(source), &mut Session::new(&mut Account))
            .unwrap()
            .ok()
            .unwrap()
    }
    #[test]
    fn settings_merge_and_restore_group_state() {
        let mut flags = Flags::initial();
        assert!(flags.unicode());
        assert!(flags.dot_matches_new_line());
        let original = flags
            .set(&parsed("-u)"), &mut Session::new(&mut Account))
            .unwrap();
        assert!(!flags.unicode());
        assert!(flags.dot_matches_new_line());
        let old = flags
            .set(&parsed("imUR-sx:"), &mut Session::new(&mut Account))
            .unwrap();
        assert!(
            flags.case_insensitive() && flags.multi_line() && flags.swap_greed() && flags.crlf()
        );
        assert!(!flags.dot_matches_new_line() && !flags.unicode());
        flags = old;
        assert!(!flags.case_insensitive() && !flags.multi_line());
        assert!(!flags.swap_greed() && !flags.crlf());
        assert!(flags.dot_matches_new_line() && !flags.unicode());
        flags = original;
        assert!(flags.unicode());
    }
    #[test]
    fn emitted_utf8_hex_bytes_are_not_encoded_twice() {
        let mut flags = Flags::initial();
        flags
            .set(&parsed("-u)"), &mut Session::new(&mut Account))
            .unwrap();
        for (source, expected) in [
            ("\\xc3", Scalar::Byte(0xc3)),
            ("\\xa9", Scalar::Byte(0xa9)),
            ("\\x7f", Scalar::Char('\x7f')),
            ("\\*", Scalar::Char('*')),
            ("é", Scalar::Char('é')),
        ] {
            let value = Scratch::new()
                .parse(&mut Cursor::start(source), &mut Session::new(&mut Account))
                .unwrap();
            assert_eq!(
                flags
                    .scalar(&value, &mut Session::new(&mut Account))
                    .unwrap(),
                expected
            );
            if source == "\\xc3" {
                assert_eq!(
                    Flags::initial()
                        .scalar(&value, &mut Session::new(&mut Account))
                        .unwrap(),
                    Scalar::Char('Ã')
                );
            }
        }
    }
    struct Refuse;
    impl Admission for Refuse {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            Err(())
        }
    }
    #[test]
    fn refused_flag_scan_does_not_publish_partial_settings() {
        let ast = parsed("im-s:");
        let mut flags = Flags::initial();
        let before = flags;
        let mut account = Refuse;
        let mut session = Session::new(&mut account);
        assert!(matches!(flags.set(&ast, &mut session), Err(Stop::Refused)));
        assert_eq!(flags, before);
        assert!(matches!(session.check(), Err(Stop::Refused)));
    }
}
