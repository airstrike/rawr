//! `std::cmp::Reverse<T>`, Rust's tuple struct that orders its value
//! backwards. At runtime it is a `std::cmp::Reverse<Dynamic>`, which
//! `compare_value` orders backwards wherever it is compared: alone, in a
//! tuple, as a sort key or a map's key, or in a derived `Ord`.

use super::*;

impl Parser<'_> {
    /// Whether a body path starting with `name` (already read) is the
    /// constructor `Reverse(..)`: `Reverse` unless the crate declares a type
    /// or function of that name, `cmp::Reverse`, `std::cmp::Reverse`. Reads
    /// the rest of the path.
    pub(super) fn reverse_path(&mut self, name: &str) -> bool {
        let mut ahead = self.input.clone();
        let tokens: Vec<Token> = (0..5)
            .map(|_| ahead.next().map_or(Token::EOF, |(token, _)| token))
            .collect();
        let identifier = |token: &Token, text: &str| matches!(token, Token::Identifier(name) if name.as_str() == text);
        let consumed = match name {
            "std" | "core"
                if tokens[0] == Token::DoubleColon
                    && identifier(&tokens[1], "cmp")
                    && tokens[2] == Token::DoubleColon
                    && identifier(&tokens[3], "Reverse") =>
            {
                4
            }
            "cmp" if tokens[0] == Token::DoubleColon && identifier(&tokens[1], "Reverse") => 2,
            "Reverse"
                if self.models.get(name).is_none()
                    && self.models.function(name).is_none()
                    && !self.bindings.contains_key(name) =>
            {
                0
            }
            _ => return false,
        };
        if tokens[consumed] != Token::LeftParen {
            return false;
        }
        for _ in 0..consumed {
            self.next();
        }
        true
    }

    /// `Reverse<T>` in a body, after the name.
    pub(super) fn parse_reverse_type(&mut self) -> Result<Ty, Error> {
        self.expect(Token::LessThan, "'<' after `Reverse`")?;
        let inner = self.parse_local_type()?;
        self.expect_close_angle("'>' after `Reverse<T`")?;
        Ok(Ty::Reverse(Box::new(inner)))
    }

    /// `Reverse(value)`.
    pub(super) fn parse_reverse(&mut self, name_span: Span) -> Result<ParsedExpr, Error> {
        let (arguments, end) = self.parse_call_arguments("`Reverse` arguments")?;
        let call = span(name_span.start(), end);
        let [value] = <[ParsedExpr; 1]>::try_from(arguments).map_err(|arguments| {
            Error::InvalidConstruction {
                owner: "Reverse".into(),
                reason: format!("`Reverse` takes 1 value, found {}", arguments.len()).into(),
                construction: call,
            }
        })?;
        let ty = Ty::Reverse(Box::new(self.inference.resolve(&value.ty)));
        let source = self.prelude_op(PreludeOp::ReverseWrap, [value.rendered_source()]);
        Ok(self.operator_expr(source, ty, call))
    }

    /// `r.0`, the value; `r.clone()`.
    pub(super) fn parse_reverse_member(
        &mut self,
        value: ParsedExpr,
        inner: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        match member.as_str() {
            "0" => {
                let end = text_end(member_span.start(), &member);
                let source = self.prelude_op(PreludeOp::ReverseInner, [value.rendered_source()]);
                Ok(self.operator_expr(source, inner, span(value.span.start(), end)))
            }
            "clone" if matches!(self.peek(), Token::Unit) => {
                let end = self.next().1;
                if !self.type_provides(&inner, capability::Kind::Clone) {
                    return Err(Error::MissingCapability {
                        owner: Ty::Reverse(Box::new(inner)).name(&self.models),
                        capability: "Clone".into(),
                        operation: member_span,
                    });
                }
                let source = value.rendered_source();
                Ok(self.operator_expr(
                    source,
                    Ty::Reverse(Box::new(inner)),
                    span(value.span.start(), end),
                ))
            }
            _ => Err(Error::UnknownMember {
                owner: Ty::Reverse(Box::new(inner)).name(&self.models),
                member,
                reference: member_span,
            }),
        }
    }
}
