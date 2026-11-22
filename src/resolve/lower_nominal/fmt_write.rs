//! `write!` and `writeln!` into a `String`, which `std::fmt::Write` gives
//! it: the formatted text pushed onto the string, and `Ok(())`, a
//! `fmt::Result`. Into another writer, as rustc expands them: its
//! `write_fmt`. A `fmt` body's formatter is written in `lower_nominal`.

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

    /// rustc's E0599 for `write!` into `target` where no `Write` trait it
    /// implements is in scope (`std_trait_in_scope`): `None` where one is. A
    /// `String` has only `std::fmt::Write`'s `write_fmt`, so `std::io::Write`
    /// in scope is the same E0599 (#1428).
    fn unscoped_write(&self, owner: &Ty, target: Span) -> Option<Error> {
        let at = target.start();
        let scoped = self.std_trait_in_scope("core::fmt::Write", at)
            || !matches!(owner, Ty::String)
                && self.std_trait_in_scope("core::io::write::Write", at);
        // std's writer of bytes (`Stdout`) has `io::Write`'s.
        let writer = match self.std_writer(owner) {
            true => "std::io::Write",
            false => "std::fmt::Write",
        };
        (!scoped).then(|| Error::UnscopedWrite {
            owner: owner.name(&self.models),
            at: target,
            writer,
            placement: self.models.use_placement(),
        })
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
        let target_start = self.position();
        if matches!(self.peek(), Token::Ampersand) && matches!(self.peek_second(), Token::Mut) {
            self.next();
            self.next();
        }
        if let Some(written) = self.write_into_program_writer(name, name_span, target_start)? {
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
        let owner = self.inference.resolve(text_ty);
        if !matches!(owner, Ty::String) {
            if !self.std_writer(&owner) {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: crate::resolve::Unsupported::said(format!(
                        "`{name}!` into a `{}` is not supported in Rawr yet",
                        target.ty.name(&self.models)
                    ))
                    .with_help("write into a `String` or a `fmt` formatter"),
                    source: target.span,
                });
            }
            if let Some(error) = self.unscoped_write(&owner, target.span) {
                return Err(error);
            }
            self.splice_write_fmt(name, target.span.start())?;
            return Ok(target);
        }
        if let Some(error) = self.unscoped_write(&owner, span(target_start, target.span.start())) {
            return Err(error);
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

    /// `write!(w, ..)` into the local `w` of any type but a `String`, as
    /// rustc expands it: `w.write_fmt(format_args!(..))`, the `write_fmt`
    /// of the `Write` trait in scope that `w`'s type implements: a
    /// program's impl, std's (`Vec<u8>`), or a generic's bound (#2483).
    /// `writeln!` adds the newline to the format string, as std's does.
    /// After the macro's `(`; `None`, consuming nothing, for another target.
    fn write_into_program_writer(
        &mut self,
        name: &str,
        name_span: Span,
        target_start: Position,
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
        // A writer by an impl, its own or std's, or by a generic's bound. A
        // std facade's writer (`Stdout`) is any target's, below, and a
        // crate's has no `write_fmt` rawr runs yet.
        let writes = !matches!(ty, Ty::String | Ty::Str | Ty::Host { .. })
            && (self.type_implements(&ty, "Write")
                || super::op::owner_id(&ty).is_some_and(|owner| {
                    self.models
                        .trait_methods
                        .contains_key(&(owner, "write_fmt".into()))
                }));
        if !writes {
            return Ok(None);
        }
        let (local_token, at) = self.next();
        if let Some(error) = self.unscoped_write(&ty, span(target_start, at)) {
            return Err(error);
        }
        self.splice_write_fmt(name, at)?;
        self.splice_tokens(vec![(local_token, at)]);
        let _ = name_span;
        self.parse_primary().map(Some)
    }

    /// Whether `ty` is a writer std's facade holds, which has the `write_fmt`
    /// of std's `Write` impl for it: `Stdout`, `Stderr`.
    fn std_writer(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Host { proof, .. } if self
            .models
            .host_items(&format!("{}::write_fmt", proof.definition.path()))
            .is_some())
    }

    /// The rest of `write!(w, ..)` after its writer `w`, as rustc expands
    /// it, `.write_fmt(format_args!(..))`, put back in the input for the
    /// writer's postfix to call. `writeln!` adds the newline to the format
    /// string, as std's does.
    fn splice_write_fmt(&mut self, name: &str, at: Position) -> Result<(), Error> {
        let (mut arguments, close) = if matches!(self.peek(), Token::Comma) {
            self.next();
            self.splice_tokens(vec![(Token::LeftParen, at)]);
            self.take_argument_tokens()?
        } else {
            let close = self.expect(Token::RightParen, "')' after `writeln!`")?;
            (Vec::new(), close)
        };
        if name == "writeln" {
            match arguments.first_mut() {
                Some((Token::StringConstant(format), _)) => {
                    **format = format!("{format}\n").into();
                }
                None => arguments.push((Token::StringConstant(Box::new("\n".into())), close)),
                Some((_, at)) => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "`writeln!` whose format is not a string literal".into(),
                        source: point_span(*at),
                    });
                }
            }
        } else if arguments.is_empty() {
            arguments.push((Token::StringConstant(Box::new("".into())), close));
        }
        let identifier = |text: &str| Token::Identifier(Box::new(text.into()));
        let mut tokens = vec![
            (Token::Period, at),
            (identifier("write_fmt"), at),
            (Token::LeftParen, at),
            (identifier("format_args"), at),
            (Token::Bang, at),
            (Token::LeftParen, at),
        ];
        tokens.extend(arguments);
        tokens.extend([(Token::RightParen, close), (Token::RightParen, close)]);
        self.splice_tokens(tokens);
        Ok(())
    }
}
