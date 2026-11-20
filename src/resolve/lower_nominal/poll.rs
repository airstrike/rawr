//! Polling a future by hand: `std::pin::Pin`, which rawr holds as the
//! pointer it pins, and `Future::poll(future, cx)` of a future the program
//! holds, whose `Poll<T>` and `Context` are the core future's
//! (`future::facade`).

use super::*;

impl Parser<'_> {
    /// `Pin::new(pointer)` or `Pin::new_unchecked(pointer)`, after its
    /// path: the pointer itself. Rawr's values stay where a reference
    /// reaches them, so pinning one changes nothing at runtime.
    pub(super) fn parse_pin_constructor(
        &mut self,
        member: &str,
        start: Position,
    ) -> Result<ParsedExpr, Error> {
        let (arguments, end) = self.parse_borrowing_arguments("`Pin` arguments")?;
        let call = span(start, end);
        let [pointer] = <[ParsedExpr; 1]>::try_from(arguments).map_err(|arguments| {
            Error::argument_count(
                Callee::Function,
                crate::resolve::Qualified("std::pin::Pin", member).to_string(),
                1,
                arguments.len(),
                call,
            )
        })?;
        Ok(ParsedExpr {
            span: call,
            ..pointer
        })
    }

    /// Whether the macro path `path`, as a `use` qualifies it, is std's
    /// `task::ready!`: in full, or by a crate's re-export of it.
    pub(super) fn names_std_ready(&self, path: &str) -> bool {
        let std = |path: &str| {
            crate::std_table::any(path)
                .is_some_and(|std| std.definition == crate::resolve::STD_READY)
        };
        std(path)
            || self
                .models
                .host
                .std_alias(path)
                .is_some_and(|alias| std(&alias))
    }

    /// `ready!(poll)`, at the `!`: core's own expansion, the `Ready` value
    /// of `poll` or a `return` of `Pending` from the enclosing function.
    pub(super) fn parse_ready_macro(&mut self) -> Result<ParsedExpr, Error> {
        let at = self.next().1;
        let (argument, _) = self.take_argument_tokens()?;
        let ident = |text: &str| (Token::Identifier(Box::new(text.into())), at);
        let variant = |name: &str| {
            [
                ident("core"),
                (Token::DoubleColon, at),
                ident("task"),
                (Token::DoubleColon, at),
                ident("Poll"),
                (Token::DoubleColon, at),
                ident(name),
            ]
        };
        let ready = "__rawr_ready";
        let mut tokens = vec![(Token::Reserved(Box::new("match".into())), at)];
        tokens.extend(argument);
        tokens.push((Token::LeftBrace, at));
        tokens.extend(variant("Ready"));
        tokens.extend([
            (Token::LeftParen, at),
            ident(ready),
            (Token::RightParen, at),
            (Token::DoubleArrow, at),
            ident(ready),
            (Token::Comma, at),
        ]);
        tokens.extend(variant("Pending"));
        tokens.extend([
            (Token::DoubleArrow, at),
            (Token::LeftBrace, at),
            (Token::Return, at),
        ]);
        tokens.extend(variant("Pending"));
        tokens.extend([
            (Token::SemiColon, at),
            (Token::RightBrace, at),
            (Token::RightBrace, at),
        ]);
        self.splice_tokens(tokens);
        self.parse_primary()
    }

    /// Whether the type path `name` names `std::pin::Pin`: in full, by a
    /// `use` of it or of `std::pin`, unless the program declares a `Pin`.
    pub(super) fn pin_named(&self, name: &str) -> bool {
        self.std_scoped(name)
            .is_some_and(|path| crate::resolve::std_pin(&path))
    }

    /// `Pin<P>` in a body, after its path: `P`, which a `Pin` is at
    /// runtime.
    pub(super) fn parse_pin_type(&mut self) -> Result<Ty, Error> {
        self.expect(Token::LessThan, "'<' after `Pin`")?;
        let pointer = self.parse_local_type()?;
        self.expect_close_angle("'>' after `Pin<P`")?;
        Ok(pointer)
    }

    /// The future a receiver of type `ty` reaches through its references
    /// and boxes, as `Pin<&mut F>` and `Pin<Box<F>>` are at runtime, and
    /// that future's output: the core future's, which `Future::poll` polls.
    fn polled_future(&self, ty: &Ty) -> Option<Ty> {
        match self.inference.resolve(ty) {
            Ty::Reference { target, .. } | Ty::Boxed(target) => self.polled_future(&target),
            ty => self.task_output(&ty),
        }
    }

    /// `future.poll(cx)`, also as `Future::poll(future, cx)`, of a future
    /// the program holds, at its `(`: one poll of it with the polling
    /// task's context, `Poll<T>` of its output. `None` when the receiver is
    /// no core future, as a crate's future is, whose `poll` is its own
    /// impl's, which polls it where its slot holds it.
    pub(super) fn parse_future_poll(
        &mut self,
        future: &ParsedExpr,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Some(output) = self.polled_future(&future.ty) else {
            return Ok(None);
        };
        let (mut arguments, end) = self.parse_call_arguments("`poll` arguments")?;
        let call = span(future.span.start(), end);
        if arguments.len() != 1 {
            return Err(Error::argument_count(
                Callee::Method,
                "poll",
                1,
                arguments.len(),
                call,
            ));
        }
        let items = self
            .models
            .host_items(crate::future::facade::POLL_ONCE)
            .map(<[_]>::to_vec)
            .ok_or_else(|| self.unsupported("`Future::poll` in a program without async"))?;
        let owner = self
            .models
            .item_owner(crate::future::facade::POLL_ONCE)
            .cloned();
        let mut receiver = future.clone();
        receiver.prelude = origin::Fragment::default();
        arguments.insert(0, receiver);
        let polled = self.call_host_function(&items, owner, arguments, call)?;
        let ty = self.poll_type(output);
        self.require_type(&ty, &polled.ty, call)?;
        Ok(Some(ParsedExpr { ty, ..polled }))
    }

    /// `T` of `ty` when it is std's `Poll<T>`.
    pub(super) fn poll_output(&self, ty: &Ty) -> Option<Ty> {
        match self.inference.resolve(ty) {
            Ty::Host { proof, arguments }
                if proof.definition.path().as_str() == crate::future::facade::POLL =>
            {
                arguments.first().cloned()
            }
            _ => None,
        }
    }

    /// Whether the path `segments` names std's `task::Poll`: in full, by a
    /// `use` of it or of a module on the way, or by a crate's re-export,
    /// as tokio's `macros::support::Poll`.
    pub(super) fn names_std_poll(&self, segments: &[&str]) -> bool {
        let [first, ..] = segments else {
            return false;
        };
        let written = segments.join("::");
        let qualified = match self.models.import(first) {
            Some(binding) => format!("{}{}", binding.qualified, &written[first.len()..]),
            None => written,
        };
        crate::resolve::std_reexport(&qualified, &self.models.host).as_str()
            == crate::future::facade::POLL
            || self.models.host.std_alias(&qualified).is_some_and(|std| {
                crate::resolve::std_reexport(&std, &self.models.host).as_str()
                    == crate::future::facade::POLL
            })
    }

    /// `Poll::Ready(value)` or `Poll::Pending` in an expression, after the
    /// variant's name: `Some(value)` or `None` at runtime.
    pub(super) fn parse_poll_constructor(
        &mut self,
        variant: Prelude,
        start: Span,
    ) -> Result<ParsedExpr, Error> {
        let output = self.inference.fresh();
        if variant == Prelude::Pending {
            self.flow().set_origins(start, Vec::new());
            let source = self.prelude_op(PreludeOp::None, []);
            let ty = self.poll_type(output);
            return Ok(self.operator_expr(source, ty, start));
        }
        let (mut arguments, end) = self.parse_call_arguments("`Poll::Ready` arguments")?;
        let expression = span(start.start(), end);
        let [value] = arguments.as_mut_slice() else {
            return Err(Error::argument_count(
                Callee::Variant,
                "Ready",
                1,
                arguments.len(),
                expression,
            ));
        };
        self.reject_invocation(&value.ty, value.span, "stored in a `Poll`")?;
        self.coerce_expr(&output, value)?;
        self.move_local(value)?;
        let ty = self.poll_type(self.inference.resolve(&value.ty));
        self.compose(
            expression,
            &ty,
            &[(
                &*value,
                borrows::Slot::At(borrowck::Path::of(&[borrowck::Step::Arg(0)])),
            )],
        );
        let source = self.prelude_op(PreludeOp::Some, [value.rendered_source()]);
        Ok(self.operator_expr(source, ty, expression))
    }

    /// `future.into_future()`, also as `IntoFuture::into_future(future)`,
    /// at its `()`: the future itself, or the core's future of a crate's,
    /// which code then polls by hand as it polls any other (`core_future`).
    /// `None` when the receiver is no future.
    pub(super) fn parse_into_future(
        &mut self,
        future: &ParsedExpr,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Some(output) = self.awaited_output(&future.ty, future.span)? else {
            return Ok(None);
        };
        let end = self.next().1;
        let mut future = future.clone();
        future.span = span(future.span.start(), end);
        self.move_local(&mut future)?;
        if self.task_output(&future.ty).is_some() {
            return Ok(Some(future));
        }
        let at = future.span;
        self.core_future(future, &output, at).map(Some)
    }

    /// `poll.map(f)` of a `Poll<T>` `value` holds, at its `(`: an `Option`'s
    /// `map` of it, which it is at runtime, of `Poll<U>`.
    pub(super) fn parse_poll_map(
        &mut self,
        value: ParsedExpr,
        element: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let option = ParsedExpr {
            ty: Ty::Option(Box::new(element.clone())),
            ..value
        };
        let mapped = self.parse_option_method(option, element, member, member_span)?;
        let Ty::Option(output) = self.inference.resolve(&mapped.ty) else {
            return Err(self.unsupported("`Poll::map` of other than an `Option`'s `map`"));
        };
        Ok(ParsedExpr {
            ty: self.poll_type(*output),
            ..mapped
        })
    }

    /// std's `Poll<T>` of `output`, the core future's `Poll`.
    pub(super) fn poll_type(&self, output: Ty) -> Ty {
        let path = crate::future::facade::POLL;
        let definition = self
            .models
            .host
            .type_(path)
            .expect("the core future's facade declares `Poll`")
            .clone();
        Ty::Host {
            proof: HostTyProof::new(definition, None, self.models.host.type_owner(path).cloned()),
            arguments: Box::new([output]),
        }
    }
}
