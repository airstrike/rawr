//! Reading the token stream: the next token, lookahead, the expected
//! tokens of a construct, and where a name is in its source.

use super::*;

impl<'a> Parser<'a> {
    /// `at`, a point where `name` is written, over the name, as rustc
    /// marks a local where it is declared, moved and used.
    pub(super) fn name_span(&self, at: Span, name: &str) -> Span {
        let named = span(at.start(), text_end(at.start(), name));
        if at.start() == at.end() && named.text(self.text) == name {
            named
        } else {
            at
        }
    }

    /// Where `name` is in `written`, the inside of a `{...}` that starts
    /// at char `at` of a `format!` string's value, in the literal at
    /// `literal`: rustc points at it there. None for a literal the source
    /// doesn't show as written.
    pub(super) fn template_name_span(
        &self,
        literal: Span,
        template: &str,
        written: &str,
        at: usize,
        name: &str,
    ) -> Option<Span> {
        let identifier = |character: char| character == '_' || character.is_alphanumeric();
        let offset = written.match_indices(name).find_map(|(offset, _)| {
            let before = written[..offset].chars().next_back();
            let after = written[offset + name.len()..].chars().next();
            (!before.is_some_and(identifier) && !after.is_some_and(identifier))
                .then(|| written[..offset].chars().count())
        })?;
        let start = at + offset;
        literal::value_span(
            literal.text(self.text),
            literal.start(),
            template,
            start..start + name.chars().count(),
        )
    }

    pub(super) fn expect_name(
        &mut self,
        construct: &str,
    ) -> Result<(ImmutableString, Span), Error> {
        let (token, position) = self.next();
        match token {
            Token::Identifier(name)
                if name.starts_with(SENTINEL)
                    || name.starts_with(METHOD_SENTINEL)
                    || name.starts_with(NOMINAL_MATCH_SENTINEL) =>
            {
                Err(Error::UnsupportedNominalSyntax {
                    construct: "compiler-private nominal intrinsic".into(),
                    source: point_span(position),
                })
            }
            Token::Identifier(name) => Ok(((*name).into(), point_span(position))),
            Token::Reserved(name) if is_valid_identifier(&name) => {
                Ok(((*name).into(), point_span(position)))
            }
            token => Err(Error::UnsupportedNominalSyntax {
                construct: format!("{construct}; found `{token}`").into(),
                source: point_span(position),
            }),
        }
    }

    pub(super) fn expect_qualified_name(&mut self) -> Result<(ImmutableString, Span), Error> {
        let (token, position) = self.next();
        match token {
            Token::Identifier(name) | Token::Reserved(name) => {
                Ok(((*name).into(), point_span(position)))
            }
            Token::SelfValue => Err(Error::StdPath {
                code: "E0433",
                message: "`self` in paths can only be used in start position or last position"
                    .into(),
                label: Some("can only be used in path start position or last position".into()),
                note: None,
                reference: Span::new(position, text_end(position, "self")),
            }),
            token => Err(Error::UnsupportedNominalSyntax {
                construct: format!("qualified nominal member; found `{token}`").into(),
                source: point_span(position),
            }),
        }
    }

    pub(super) fn expect_member_name(&mut self) -> Result<(ImmutableString, Span), Error> {
        let (token, position) = self.next();
        match token {
            Token::Identifier(name) | Token::Reserved(name) => {
                Ok(((*name).into(), point_span(position)))
            }
            Token::IntegerConstant(index) if index >= 0 => {
                Ok((index.to_string().into(), point_span(position)))
            }
            // `t.0.1`: the number `0.1` is two tuple fields.
            Token::FloatConstant(ref number)
                if let Some((first, second)) = number.1.split_once('.')
                    && !first.is_empty()
                    && first.bytes().all(|byte| byte.is_ascii_digit())
                    && !second.is_empty()
                    && second.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                let second_position = Position::new(
                    position.line().unwrap_or(0) as u16,
                    (position.position().unwrap_or(0) + first.len() + 1) as u16,
                );
                self.pending_member = Some((second.into(), point_span(second_position)));
                Ok((first.into(), point_span(position)))
            }
            token => Err(Error::UnsupportedNominalSyntax {
                construct: format!("field or capability method; found `{token}`").into(),
                source: point_span(position),
            }),
        }
    }

    pub(super) fn expect(&mut self, expected: Token, construct: &str) -> Result<Position, Error> {
        let (token, position) = self.next();
        if token == expected {
            Ok(position)
        } else {
            Err(Error::UnsupportedNominalSyntax {
                construct: format!("{construct}; found `{token}`").into(),
                source: point_span(position),
            })
        }
    }

    pub(super) fn unsupported(&mut self, construct: &str) -> Error {
        Error::UnsupportedNominalSyntax {
            construct: construct.into(),
            source: point_span(self.position()),
        }
    }

    /// The `>` closing generic arguments, splitting a `>>`.
    pub(super) fn expect_close_angle(&mut self, construct: &str) -> Result<Position, Error> {
        if matches!(self.peek(), Token::RightShift) {
            let (_, position) = self.next();
            let mut second = position;
            second.advance();
            self.split_angle = Some(second);
            return Ok(position);
        }
        self.expect(Token::GreaterThan, construct)
    }

    /// The token after the next one.
    pub(super) fn peek_second(&mut self) -> Token {
        let mut ahead = self.input.clone();
        ahead.next();
        ahead.next().map_or(Token::EOF, |(token, _)| token)
    }

    pub(super) fn peek_identifier(&mut self) -> Option<&str> {
        if self.split_angle.is_some() {
            return None;
        }
        match self.input.peek() {
            Some((Token::Identifier(name), _)) => Some(name),
            _ => None,
        }
    }

    pub(super) fn peek(&mut self) -> Token {
        if self.split_angle.is_some() {
            return Token::GreaterThan;
        }
        self.input
            .peek()
            .expect("token streams always end with EOF")
            .0
            .clone()
    }

    pub(super) fn position(&mut self) -> Position {
        if let Some(position) = self.split_angle {
            return position;
        }
        self.input
            .peek()
            .expect("token streams always end with EOF")
            .1
    }

    pub(super) fn next(&mut self) -> (Token, Position) {
        let (token, position) = match self.split_angle.take() {
            Some(position) => (Token::GreaterThan, position),
            None => self
                .input
                .next()
                .expect("token streams always end with EOF"),
        };
        self.track_calls(&token, position);
        self.models.at = position;
        (token, position)
    }
}
