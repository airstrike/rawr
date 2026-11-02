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
        if let Some(written) = self.write_into_program_writer(name, name_span)? {
            return Ok(written);
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

    /// `write!(w, ..)` into the local `w` of a type of the program's that
    /// implements `std::fmt::Write`: std's `write_fmt`, which hands the
    /// formatted text to the impl's `write_str`, as `w.write_str(&text)`.
    /// After the macro's `(`; `None`, consuming nothing, for another target.
    fn write_into_program_writer(
        &mut self,
        name: &str,
        name_span: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Token::Identifier(local) = self.peek() else {
            return Ok(None);
        };
        if !matches!(self.peek_second(), Token::Comma | Token::RightParen) {
            return Ok(None);
        }
        let Some(binding) = self.bindings.get(local.as_str()) else {
            return Ok(None);
        };
        let mut ty = self.inference.resolve(&binding.ty);
        while let Ty::Reference { target, .. } = ty {
            ty = self.inference.resolve(&target);
        }
        let Ty::Script(id, _) = ty else {
            return Ok(None);
        };
        let writes = self
            .models
            .trait_methods
            .get(&(id, "write_str".into()))
            .is_some_and(|definitions| {
                definitions.iter().any(|definition| {
                    definition
                        .trait_name()
                        .is_some_and(|name| name.as_str() == "Write")
                })
            });
        if !writes {
            return Ok(None);
        }
        let (local_token, at) = self.next();
        let (arguments, close) = if matches!(self.peek(), Token::Comma) {
            self.next();
            self.splice_tokens(vec![(Token::LeftParen, at)]);
            self.take_argument_tokens()?
        } else {
            let close = self.expect(Token::RightParen, "')' after `writeln!`")?;
            (Vec::new(), close)
        };
        let identifier = |text: &str| Token::Identifier(Box::new(text.into()));
        // `w.write_str(&(format!(..) + "\n"))`.
        let mut tokens = vec![
            (local_token, at),
            (Token::Period, at),
            (identifier("write_str"), at),
            (Token::LeftParen, at),
            (Token::Ampersand, at),
            (Token::LeftParen, at),
        ];
        if arguments.is_empty() {
            tokens.push((Token::StringConstant(Box::new("".into())), at));
        } else {
            tokens.extend([
                (identifier("format"), at),
                (Token::Bang, at),
                (Token::LeftParen, at),
            ]);
            tokens.extend(arguments);
            tokens.push((Token::RightParen, close));
        }
        if name == "writeln" {
            tokens.extend([
                (Token::Plus, close),
                (Token::StringConstant(Box::new("\n".into())), close),
            ]);
        }
        tokens.extend([(Token::RightParen, close), (Token::RightParen, close)]);
        let _ = name_span;
        self.splice_tokens(tokens);
        self.parse_primary().map(Some)
    }
}
