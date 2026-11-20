//! `std::cmp::Reverse<T>`, Rust's tuple struct that orders its value
//! backwards. At runtime it is a `std::cmp::Reverse<Dynamic>`, which
//! `compare_value` orders backwards wherever it is compared: alone, in a
//! tuple, as a sort key or a map's key, or in a derived `Ord`.

use super::*;

impl Parser<'_> {
    /// Whether a body path starting with `name` (already read) is the
    /// constructor `Reverse(..)`, through the scope (`std_path_ahead`):
    /// `Reverse` after `use std::cmp::Reverse;`, or `std::cmp::Reverse`,
    /// unless a function or a local of the name shadows it. Reads the rest
    /// of the path.
    pub(super) fn reverse_path(&mut self, name: &str) -> bool {
        if self.models.function(name).is_some() || self.bindings.contains_key(name) {
            return false;
        }
        let Some((_, consumed, next)) = self.std_path_ahead(name, crate::resolve::std_reverse)
        else {
            return false;
        };
        if next != Token::LeftParen {
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
            Error::argument_count(
                crate::resolve::Callee::Struct,
                "Reverse",
                1,
                arguments.len(),
                call,
            )
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
                        derive: false,
                        declared: None,
                        required_by: None,
                        formatted: None,
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
