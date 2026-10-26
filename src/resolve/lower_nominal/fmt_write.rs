//! `write!` and `writeln!` into a `String`, which `std::fmt::Write` gives
//! it: the formatted text pushed onto the string, and `Ok(())`, a
//! `fmt::Result`. A `fmt` body's formatter is written in `lower_nominal`.

use super::*;

impl Parser<'_> {
    /// Whether `write!(f, ..)`, after the name, writes into the `fmt` body's
    /// formatter `f`.
    pub(super) fn writes_into_formatter(&self) -> bool {
        let Some(formatter) = &self.formatter else {
            return false;
        };
        let mut ahead = self.input.clone();
        matches!(ahead.next(), Some((Token::Bang, _)))
            && matches!(ahead.next(), Some((Token::LeftParen, _)))
            && matches!(ahead.next(), Some((Token::Identifier(name), _)) if name.as_str() == formatter.as_str())
    }

    /// `write!(s, ..)`, `write!(&mut s, ..)` or `writeln!(s, ..)` into the
    /// `String` place `s`, after the macro's name.
    pub(super) fn parse_write_string(
        &mut self,
        name: &str,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.next();
        self.expect(Token::LeftParen, "'(' after `write!`")?;
        if matches!(self.peek(), Token::Ampersand) && matches!(self.peek_second(), Token::Mut) {
            self.next();
            self.next();
        }
        let target = self.parse_expr()?;
        let mut text_ty = &target.ty;
        while let Ty::Reference {
            mutable: true,
            target,
        } = text_ty
        {
            text_ty = target;
        }
        if !matches!(self.inference.resolve(text_ty), Ty::String) {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "`{name}!` into a `{}`; Rawr writes into a `String` or a `fmt` formatter",
                    target.ty.name(&self.models)
                )
                .into(),
                source: target.span,
            });
        }
        let (mut text, end) = if matches!(self.peek(), Token::Comma) {
            self.next();
            let text = self.parse_format(name_span, true)?;
            let end = text.span.end();
            (text.rendered_source(), end)
        } else {
            let end = self.expect(Token::RightParen, "')' after `writeln!`")?;
            (origin::Fragment::plain("\"\""), end)
        };
        if name == "writeln" {
            text = text.surrounded("(", " + \"\\n\")");
        }
        let code = STR_CHANGES
            .iter()
            .position(|change| *change == "push_str")
            .expect("`push_str` is a `String` change");
        let helper = self.push_op(Op::Prelude(PreludeOp::StrChange));
        let push = self.mutation_call(
            &target,
            &helper,
            vec![code.to_string().into(), "()".into(), text],
        )?;
        let mut source = origin::Fragment::plain("{ ");
        source.push(push);
        source.push_text("; ");
        source.push(self.prelude_op(PreludeOp::Ok, ["()".into()]));
        source.push_text(" }");
        Ok(self.operator_expr(
            source,
            Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Unit)),
            span(name_span.start(), end),
        ))
    }
}
