//! Loops and jumps: `loop`, `while`, `for`, `break`, `continue` and
//! `return`.

use super::*;

impl<'a> Parser<'a> {
    /// `while`, `loop` or `for`, optionally labeled. The body is a `()`
    /// block. `while` and `for` are `()`; `loop` has the type of its
    /// `break` values, or `!` when nothing breaks out of it.
    pub(super) fn parse_loop(
        &mut self,
        token: Token,
        start: Span,
        label: Option<ImmutableString>,
    ) -> Result<ParsedExpr, Error> {
        let source = match token {
            // `while` over a let chain is `loop { links { body } }` that
            // breaks when a link fails.
            Token::While if self.let_chain_ahead() => {
                let original = self.bindings.clone();
                self.enter_while(label.clone());
                let chain = self.parse_chain();
                self.leave_while_condition();
                let looped = chain.and_then(|(open, close)| {
                    let hit = self.temporary("hit");
                    let mut head = origin::Fragment::plain(format!("{{\nlet {hit} = false;\n"));
                    head.push(open);
                    head.push_text(format!("{hit} = true; "));
                    let tail = format!("{close}\nif !{hit} {{ break; }}\n}}");
                    self.parse_loop_body(
                        origin::Fragment::plain("loop"),
                        start,
                        label,
                        false,
                        Some((head, origin::Fragment::plain(tail))),
                        Some(&original),
                    )
                });
                let inner = std::mem::replace(&mut self.bindings, original);
                self.keep_moves_from(&inner);
                return looped;
            }
            // `while let pattern = subject { body }` is
            // `loop { switch subject { pattern => body, _ => break } }`.
            Token::While if matches!(self.peek(), Token::Let) => {
                let original = self.bindings.clone();
                let scrutinee = self.temporary("match");
                let depth = self.drop_scopes.len();
                // A subject that borrows what its call takes opens its frame
                // in each iteration (D558 tier 1).
                self.open_frames();
                self.enter_while(label.clone());
                let parsed = self.parse_let_condition(&scrutinee);
                self.leave_while_condition();
                let frames = self.close_frames();
                let (subject, arm, binds, operand, condition) = parsed?;
                if frames.iter().any(|frame| !frame.after.is_empty()) {
                    return Err(self.unsupported(
                        "a `while let` whose subject borrows a `&mut` buffer its call fills: \
                         match on the call in the loop's body instead",
                    ));
                }
                // The condition's scope ends with each iteration.
                if self.drop_scoped() {
                    self.loop_drop_depth = Some(depth);
                }
                if !condition.after.is_empty() {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "a `while let` binding that shadows the local it moves a \
                                    value whose type implements `Drop` out of: not supported in \
                                    Rawr yet"
                            .into(),
                        source: start,
                    });
                }
                let mut head = origin::Fragment::plain("{\n");
                for frame in frames {
                    head.push(frame.outer);
                    head.push(frame.inner);
                }
                head.push_text(format!("let {scrutinee} = "));
                head.push(subject);
                head.push_text(format!(
                    ";\n{}__rawr_switch {operand} {{\n",
                    condition.copies
                ));
                head.push(arm);
                head.push_text(" => { ");
                head.push(binds);
                // Without a match, the value made for the condition drops.
                let tail = if condition.otherwise.is_empty() {
                    " },\n_ => break\n}\n}".to_string()
                } else {
                    format!(" }},\n_ => {{ {}break }}\n}}\n}}", condition.otherwise)
                };
                let looped = self.parse_loop_body(
                    origin::Fragment::plain("loop"),
                    start,
                    label,
                    false,
                    Some((head, origin::Fragment::plain(tail))),
                    Some(&original),
                );
                let inner = std::mem::replace(&mut self.bindings, original);
                self.keep_moves_from(&inner);
                return looped;
            }
            Token::While => {
                self.open_temporaries();
                self.flow().open_temporaries();
                self.open_frames();
                self.enter_while(label.clone());
                let condition = self.parse_head(Self::parse_expr);
                let jumped = self.leave_while_condition();
                let frames = self.close_frames();
                self.flow().close_temporaries(ScopeEnd::Statement);
                let temporaries = self.close_temporaries();
                let mut condition = condition?;
                if !temporaries.is_empty() {
                    condition.source = self
                        .with_temporaries(&temporaries, condition.rendered_source())
                        .surrounded("(", ")");
                    condition.prelude = origin::Fragment::default();
                }
                // The condition opens its frames each time it is evaluated.
                if !frames.is_empty() {
                    let rendered = condition.rendered_source();
                    condition.source = self.framed(frames, rendered).surrounded("(", ")");
                    condition.prelude = origin::Fragment::default();
                }
                self.require_type(&Ty::Bool, &condition.ty, condition.span)?;
                if jumped {
                    // A jump from the condition to the `while` must be inside
                    // its loop: `loop { if !(condition) { break; } body }`.
                    let mut head = origin::Fragment::plain("if !");
                    head.push(condition.rendered_source().surrounded("(", ")"));
                    head.push_text(" { break; }\n");
                    return self.parse_loop_body(
                        origin::Fragment::plain("loop"),
                        start,
                        label,
                        false,
                        Some((head, origin::Fragment::default())),
                        None,
                    );
                }
                let mut source = origin::Fragment::plain("while ");
                source.push(condition.rendered_source());
                source
            }
            Token::Loop => origin::Fragment::plain("loop"),
            _ => return self.parse_for(start, label),
        };
        self.parse_loop_body(source, start, label, token == Token::Loop, None, None)
    }

    /// After a loop whose pattern bound locals: each outer local the loop
    /// moved stays moved. A pattern's local of the same name is another
    /// local, which ends with the loop.
    pub(super) fn keep_moves_from(&mut self, inner: &BTreeMap<ImmutableString, Binding>) {
        for (name, binding) in &mut self.bindings {
            if let Some(after) = inner.get(name)
                && after.declaration == binding.declaration
            {
                binding.available &= after.available;
            }
        }
    }

    /// A loop's body. `outside` is the scope before the loop's own
    /// pattern bound anything (`for x in`, `while let`), or `None` when
    /// it binds nothing: only those locals live across iterations.
    pub(super) fn parse_loop_body(
        &mut self,
        mut source: origin::Fragment,
        start: Span,
        label: Option<ImmutableString>,
        value: bool,
        wrap: Option<(origin::Fragment, origin::Fragment)>,
        outside: Option<&BTreeMap<ImmutableString, Binding>>,
    ) -> Result<ParsedExpr, Error> {
        if !matches!(self.peek(), Token::LeftBrace) {
            return Err(self.unsupported("loop body without a block"));
        }
        let original = self.bindings.clone();
        // A pattern's bindings are fresh in each iteration, so moving one
        // in the body is no use of a moved value.
        let lives_across = |name: &ImmutableString, binding: &Binding| {
            outside.is_none_or(|outside| {
                outside
                    .get(name)
                    .is_some_and(|before| before.declaration == binding.declaration)
            })
        };
        let drop_depth = self
            .loop_drop_depth
            .take()
            .unwrap_or(self.drop_scopes.len());
        if std::mem::take(&mut self.loop_entered) {
            // A `while` pushed its frame before its condition.
            let frame = self.loops.last_mut().expect("the `while` pushed its frame");
            frame.drop_depth = drop_depth;
        } else {
            self.flow().loop_enter();
            if let Some(iterator) = self.loop_iterator.take() {
                self.flow().loop_header_use(iterator);
            }
            if !value {
                self.flow().loop_test();
            }
            if let Some((iterator, element, positions, bound)) = self.loop_element.take() {
                let mut flow = self.flow();
                match iterator {
                    Some(iterator) => flow.flow(iterator, element, &positions),
                    None => flow.take(element, &[], true),
                }
                for binding in bound {
                    flow.take(binding, &[], true);
                }
            }
            self.push_loop(label, value, drop_depth);
        }
        let body = self.parse_unexpected_block();
        self.flow().loop_exit();
        let frame = self.loops.pop().expect("the loop pushed its frame");
        let mut body = body?;
        // The iteration's pattern bindings drop at the end of each one.
        let mut iteration = String::new();
        while self.drop_scoped() && self.drop_scopes.len() > drop_depth {
            iteration.push_str(&self.close_drops()?);
        }
        if !iteration.is_empty() {
            let source = std::mem::replace(&mut body.value.source, origin::Fragment::plain("()"));
            body.value.prelude.push(source);
            body.value.prelude.push_text(";\n");
            body.value.prelude.push_text(iteration);
            body.value.prelude.push_text("\n");
        }
        let inner = std::mem::replace(&mut self.bindings, original);
        // A body that goes on to its next iteration with a local of the
        // loop's scope moved uses the moved value there, as rustc's E0382.
        if !matches!(self.inference.resolve(&body.value.ty), Ty::Never)
            && let Some((name, binding)) = self.bindings.iter().find(|(name, binding)| {
                binding.available
                    && lives_across(name, binding)
                    && inner.get(*name).is_some_and(|after| {
                        after.declaration == binding.declaration && !after.available
                    })
            })
        {
            let moved = self
                .moved_at
                .get(&binding.declaration)
                .copied()
                .unwrap_or(binding.declaration);
            let moved = self.name_span(moved, name);
            return Err(Error::MovedValue {
                name: name.clone(),
                moved,
                reference: moved,
                borrowed: false,
                partial: false,
                cause: Some(self.move_cause(name, binding)),
                previous_iteration: true,
            });
        }
        for (name, binding) in &mut self.bindings {
            let after = inner
                .get(name)
                .filter(|after| after.declaration == binding.declaration);
            // A `loop` runs its body before it ends, so a `let x;` it gives
            // a value has one after it.
            if value && self.deferred.contains_key(&binding.declaration) {
                binding.available = after.is_some_and(|after| after.available);
            } else {
                // A local the body shadowed keeps its own state, as
                // `let tx = tx.clone();` leaves `tx` whole.
                binding.available &= after.is_none_or(|after| after.available);
            }
        }
        self.require_type(&Ty::Unit, &body.value.ty, body.value.span)?;
        source.push_text(" ");
        match wrap {
            Some((head, tail)) => {
                let mut block = head;
                block.push(body.value.rendered_block());
                block.push(tail);
                source.push(block.surrounded("{ ", " }"));
            }
            None => source.push(body.value.rendered_block()),
        }
        if frame.targeted {
            // A nested loop jumped here: declare its jump flag and value.
            let jump = &frame.jump;
            source =
                source.surrounded(&format!("{{ let {jump} = 0; let {jump}_value = (); "), " }");
        }
        if !frame.exits.is_empty() {
            // Finish a jump from inside this loop to an enclosing one.
            let mut checks = String::new();
            for target in &frame.exits {
                let outer = &self.loops[*target];
                let jump = &outer.jump;
                if *target + 1 == self.loops.len() {
                    let value = if outer.value {
                        format!(" {jump}_value")
                    } else {
                        String::new()
                    };
                    checks += &format!(
                        " if {jump} == 1 {{ {jump} = 0; break{value}; }} if {jump} == 2 {{ {jump} = 0; continue; }}"
                    );
                } else {
                    checks += &format!(" if {jump} != 0 {{ break; }}");
                }
            }
            source =
                source.surrounded("{ let __rawr_loop = ", &format!(";{checks} __rawr_loop }}"));
        }
        let ty = match (frame.value, frame.ty) {
            (false, _) => Ty::Unit,
            (true, Some(ty)) => ty,
            (true, None) => Ty::Never,
        };
        let span = span(start.start(), body.syntax.end());
        Ok(self.operator_expr(source, ty, span))
    }

    /// The frame of a loop whose head was just entered.
    pub(super) fn push_loop(
        &mut self,
        label: Option<ImmutableString>,
        value: bool,
        drop_depth: usize,
    ) {
        let jump = self.temporary("jump");
        let flow_depth = self.flow().loop_depth();
        // A `loop` expected to be a type, as a typed `let` expects it, has
        // each `break` value coerce to it.
        let ty = if value { self.coercion_target() } else { None };
        self.loops.push(LoopFrame {
            label,
            jump,
            value,
            ty,
            targeted: false,
            exits: BTreeSet::new(),
            drop_depth,
            flow_depth,
            condition: None,
        });
    }

    /// A `while`'s head: its frame is pushed before its condition, which a
    /// labeled jump in it may go to, as in Rust.
    pub(super) fn enter_while(&mut self, label: Option<ImmutableString>) {
        self.flow().loop_enter();
        let drop_depth = self.drop_scopes.len();
        self.push_loop(label, false, drop_depth);
        self.loops
            .last_mut()
            .expect("the frame just pushed")
            .condition = Some(false);
    }

    /// The end of a `while`'s condition: whether a jump in it goes to the
    /// `while`.
    pub(super) fn leave_while_condition(&mut self) -> bool {
        self.flow().loop_test();
        self.loop_entered = true;
        self.loops
            .last_mut()
            .and_then(|frame| frame.condition.take())
            .unwrap_or(false)
    }

    /// `break` or `continue`, with an optional label and `break` value.
    pub(super) fn parse_jump(&mut self, token: Token, start: Span) -> Result<ParsedExpr, Error> {
        let mut end = start;
        let target = if matches!(self.peek(), Token::Apostrophe) {
            self.next();
            let (label, label_span) = self.expect_name("loop label")?;
            end = label_span;
            let Some(target) = self
                .loops
                .iter()
                .rposition(|frame| frame.label.as_ref() == Some(&label))
            else {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!("undeclared label `'{label}`").into(),
                    source: label_span,
                });
            };
            if let Some(jumped) = &mut self.loops[target].condition {
                *jumped = true;
            }
            target
        } else {
            let Some(target) = self.loops.len().checked_sub(1) else {
                return Err(self.unsupported("`break` or `continue` outside a loop"));
            };
            if self.loops[target].condition.is_some() {
                return Err(Error::UnlabeledJumpInWhileCondition {
                    keyword: if token == Token::Break {
                        "break"
                    } else {
                        "continue"
                    }
                    .into(),
                    at: start,
                });
            }
            target
        };
        let breaking = token == Token::Break;
        let mut value = None;
        if breaking
            && !matches!(
                self.peek(),
                Token::SemiColon | Token::RightBrace | Token::Comma
            )
        {
            let expected = self.loops[target].ty.clone();
            let mut parsed = self.parse_expr_expecting(expected.as_ref())?;
            end = parsed.span;
            if !self.loops[target].value {
                return Err(self.unsupported("`break` with a value outside `loop`"));
            }
            match self.loops[target].ty.clone() {
                Some(ty) => {
                    self.coerce_expr(&ty, &mut parsed)?;
                }
                None => self.loops[target].ty = Some(parsed.ty.clone()),
            }
            value = Some(parsed);
        } else if breaking && self.loops[target].value {
            match self.loops[target].ty.clone() {
                Some(ty) => self.require_type(&ty, &Ty::Unit, start)?,
                None => self.loops[target].ty = Some(Ty::Unit),
            }
        }
        let flow_depth = self.loops[target].flow_depth;
        self.exit_drop_uses(self.loops[target].drop_depth);
        self.flow().jump(flow_depth, breaking);
        let keyword = if breaking { "break" } else { "continue" };
        let drops =
            self.temporary_drops_here() + self.exit_drops(self.loops[target].drop_depth)?.as_str();
        if let Some(value) = &mut value {
            self.move_out_value(value)?;
        }
        let mut source = origin::Fragment::plain("{ ");
        if !drops.is_empty() && target + 1 == self.loops.len() {
            let value = value.map(|value| value.rendered_source());
            source = self.jump_with_drops(&drops, keyword, value);
            return Ok(self.operator_expr(source, Ty::Never, span(start.start(), end.end())));
        }
        source.push_text(&drops);
        if target + 1 == self.loops.len() {
            source.push_text(keyword);
            if let Some(value) = value {
                source.push_text(" ");
                source.push(value.rendered_source());
            }
        } else {
            let jump = self.loops[target].jump.clone();
            self.loops[target].targeted = true;
            for frame in &mut self.loops[target + 1..] {
                frame.exits.insert(target);
            }
            let code = if breaking { 1 } else { 2 };
            source.push_text(format!("{jump} = {code}; "));
            if let Some(value) = value {
                source.push_text(format!("{jump}_value = "));
                source.push(value.rendered_source());
                source.push_text("; ");
            }
            source.push_text("break");
        }
        source.push_text("; }");
        Ok(self.operator_expr(source, Ty::Never, span(start.start(), end.end())))
    }

    /// `return value` or a bare `return`, leaving the innermost function,
    /// method or closure with a value of its declared output, as in Rust.
    pub(super) fn parse_return(&mut self, start: Span) -> Result<ParsedExpr, Error> {
        if let Some(reason) = self.return_blocked {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!("explicit `return` {reason}").into(),
                source: start,
            });
        }
        let Some(output) = self.return_ty.clone() else {
            return Err(self.unsupported("`return` outside a function or closure"));
        };
        self.returns += 1;
        let mut source = origin::Fragment::plain("{ return");
        let mut end = start;
        let drops = self.temporary_drops_here() + self.exit_drops(self.drop_base)?.as_str();
        if matches!(
            self.peek(),
            Token::SemiColon | Token::RightBrace | Token::Comma | Token::EOF
        ) {
            if !self.unify_type(&output, &Ty::Unit) {
                return Err(Error::ReturnWithoutValue {
                    output: self.inference.resolve(&output).name(&self.models),
                    at: Span::new(start.start(), text_end(start.start(), "return")),
                });
            }
            if let Some(returned) = self.copied_return("()".into()) {
                source.push_text(" ");
                source.push(returned);
            }
            if !drops.is_empty() {
                let copied = self.copied_return("()".into());
                source = self.jump_with_drops(&drops, "return", copied);
                self.exit_drop_uses(self.drop_base);
                self.flow().ret();
                return Ok(self.operator_expr(source, Ty::Never, span(start.start(), end.end())));
            }
        } else {
            let block_like = self.peek_block_like();
            let mut value = self.parse_expr_expecting(Some(&output))?;
            // `return *p = P::default()`: the assignment is the value, `()`.
            if assignment_token(&self.peek()) {
                value = self.parse_assignment_expr(value)?;
            }
            let mut value = self.returned_place(value, block_like)?;
            end = value.span;
            self.coerce_expr(&output, &mut value)?;
            self.returned(value.span, &value.ty);
            self.consume_imported_value(&value, "returned value", value.span)?;
            self.move_out_value(&mut value)?;
            // The value's own temporaries drop once it is made.
            let drops = self.temporary_drops_here() + self.exit_drops(self.drop_base)?.as_str();
            let returned = value.rendered_source();
            if !drops.is_empty() {
                let returned = self.copied_return(returned.clone()).unwrap_or(returned);
                source = self.jump_with_drops(&drops, "return", Some(returned));
                self.exit_drop_uses(self.drop_base);
                self.flow().ret();
                return Ok(self.operator_expr(source, Ty::Never, span(start.start(), end.end())));
            }
            source.push_text(" ");
            source.push(self.copied_return(returned.clone()).unwrap_or(returned));
        }
        source.push_text("; }");
        self.exit_drop_uses(self.drop_base);
        self.flow().ret();
        Ok(self.operator_expr(source, Ty::Never, span(start.start(), end.end())))
    }

    /// Whether `for` loops over `ty.into_iter()` by std's `IntoIterator`
    /// impl for it: a std type the loop doesn't walk itself, as an
    /// `Option`, a `Result` or a reference to one.
    pub(super) fn std_into_iterator(&self, ty: &Ty) -> bool {
        let inner = match ty {
            Ty::Reference { target, .. } => self.inference.resolve(target),
            ty => ty.clone(),
        };
        let walked = matches!(
            inner,
            Ty::Iter(..)
                | Ty::Array { .. }
                | Ty::Keyed(..)
                | Ty::Range(_)
                | Ty::InclusiveRange(_)
                | Ty::Thread(..)
                | Ty::Boxed(_)
        );
        !walked
            && inner.script().is_none()
            && self.std_declared(ty)
            && std_impl::holds(
                &mut std_impl::Probe(self),
                "core::iter::traits::collect::IntoIterator",
                ty,
                &[],
                true,
                0,
            )
            .is_ok()
    }

    /// `for x in a..b` and `for x in a..=b` over `i64`.
    pub(super) fn parse_for(
        &mut self,
        start: Span,
        label: Option<ImmutableString>,
    ) -> Result<ParsedExpr, Error> {
        // `for (i, x) in ..`, `for &x in ..`: the element is destructured at
        // the top of the body.
        let pattern = if matches!(
            self.peek(),
            Token::LeftParen | Token::Ampersand | Token::And
        ) || matches!(self.peek_second(), Token::Reserved(ref text) if text.as_str() == "@")
            || matches!(self.peek(), Token::Identifier(_))
                && matches!(
                    self.peek_second(),
                    Token::LeftParen | Token::DoubleColon | Token::LeftBrace
                ) {
            Some(self.parse_pattern_no_alt()?)
        } else {
            None
        };
        let mutable = pattern.is_none() && matches!(self.peek(), Token::Mut);
        if mutable {
            self.next();
        }
        let (name, declaration) = if let Some(pattern) = &pattern {
            (self.temporary("element").into(), pattern.span())
        } else if matches!(self.peek(), Token::Underscore) {
            let position = self.next().1;
            (self.temporary("discard").into(), point_span(position))
        } else {
            self.expect_name("`for` loop binding")?
        };
        self.expect(Token::In, "'in' after a `for` loop binding")?;
        if let Some(looped) = self.parse_mutable_for(
            name.clone(),
            declaration,
            pattern.as_ref(),
            start,
            label.clone(),
        )? {
            return Ok(looped);
        }
        let mut lhs = self.parse_head(|parser| parser.parse_binary(0))?;
        // `for x in bag` of a program's type that implements `IntoIterator`
        // loops over `bag.into_iter()`, as Rust's `for` desugars it, as
        // does one of a std type std implements it for that the loop
        // doesn't walk itself: `Option`, `Result` and references to them.
        let iterated = self.inference.resolve(&lhs.ty);
        if iterated.script().is_some()
            && !self.type_implements(&iterated, "Iterator")
            && self.type_implements(&iterated, "IntoIterator")
            || self.std_into_iterator(&iterated)
        {
            let at = lhs.span.end();
            self.splice_tokens(vec![
                (Token::Period, at),
                (Token::Identifier(Box::new("into_iter".into())), at),
                (Token::Unit, at),
            ]);
            lhs = self.parse_postfix_of(lhs)?;
        }
        // `for x in &v` borrows `v` for the whole loop, and each `x` holds
        // that borrow.
        let iterator = self.flow().hold("<iterator>", lhs.span);
        let element = self.flow().local(&name, declaration);
        let positions = match iterator {
            Some(_) => self.element_positions(&lhs.ty),
            None => Vec::new(),
        };
        self.loop_iterator = iterator;
        self.loop_element = Some((iterator, element, positions, Vec::new()));
        // A `Box<dyn Iterator>` is its iterator at runtime.
        if let Ty::Boxed(inner) = self.inference.resolve(&lhs.ty)
            && let iterator @ Ty::Iter(..) = self.inference.resolve(&inner)
        {
            lhs.ty = iterator;
        }
        // `for v in rx` and `for v in &rx`: what the channel receives, each
        // as it comes, while a sender is left. The first takes `rx`.
        let receiver = match self.inference.resolve(&lhs.ty) {
            Ty::Thread(thread::Kind::Receiver, item) => Some((*item, true)),
            Ty::Reference { target, .. } => match *target {
                Ty::Thread(thread::Kind::Receiver, item) => Some((*item, false)),
                _ => None,
            },
            _ => None,
        };
        if let Some((item, owned)) = receiver {
            if owned {
                self.move_local(&mut lhs)?;
            }
            lhs = self.received(lhs.rendered_source(), item, lhs.span);
        }
        let operator = match self.peek() {
            Token::ExclusiveRange => "..",
            Token::InclusiveRange => "..=",
            _ => {
                // An iterator chain of one loop runs the body in it.
                let iterated = self.inference.resolve(&lhs.ty);
                if (matches!(iterated, Ty::Iter(..) | Ty::InclusiveRange(_))
                    || (iterated.script().is_some() || matches!(iterated, Ty::Host { .. }))
                        && self.type_implements(&iterated, "Iterator"))
                    && let Some((pipeline, item)) = self.for_pipeline(&lhs)
                {
                    if self.needs_drop(&item, lhs.span)? {
                        return Err(Error::UnsupportedNominalSyntax {
                            construct: "a `for` over an iterator of values whose type implements \
                                        `Drop`: dropping them where Rust does is not supported in \
                                        Rawr yet; loop over the `Vec` itself"
                                .into(),
                            source: lhs.span,
                        });
                    }
                    let (setup, header, entry, exit) = Self::for_parts(pipeline, &name);
                    let mut looped = self.parse_for_body(
                        name,
                        declaration,
                        mutable,
                        item,
                        header,
                        start,
                        label,
                        pattern.as_ref(),
                        Some((entry, exit)),
                    )?;
                    let mut source = origin::Fragment::plain("{ ");
                    source.push(setup);
                    source.push(looped.source);
                    source.push_text(" }");
                    looped.source = source;
                    return Ok(looped);
                }
                // `for x in v` takes each element; `&v` and `v.iter()` borrow it.
                let ty = match self.inference.resolve(&lhs.ty) {
                    Ty::Iter(item, _) => *item,
                    Ty::Array { element, .. } => *element,
                    Ty::Keyed(kind, arguments) => keyed::keyed_item(kind, &arguments, None),
                    Ty::Reference { mutable, target } => match *target {
                        Ty::Array { element, .. } => Ty::Reference {
                            mutable,
                            target: element,
                        },
                        Ty::Keyed(kind, arguments) => {
                            keyed::keyed_item(kind, &arguments, Some(mutable))
                        }
                        _ => {
                            return Err(self.unsupported("`for` over a value that is not iterable"))
                        }
                    },
                    _ => {
                        let fallback = self.unsupported("`for` over a value that is not iterable");
                        return Err(self.pipeline_error(&lhs, fallback));
                    }
                };
                // `for x in v` takes `v`: a later use is rustc's E0382.
                let mut lhs = lhs;
                if matches!(
                    self.inference.resolve(&lhs.ty),
                    Ty::Array { .. } | Ty::Keyed(..)
                ) {
                    self.move_local(&mut lhs)?;
                    if self.needs_drop(&ty, lhs.span)? {
                        return self.parse_owned_for(
                            name,
                            declaration,
                            mutable,
                            ty,
                            lhs,
                            start,
                            label,
                            pattern.as_ref(),
                        );
                    }
                }
                let mut source = origin::Fragment::plain(format!("for {name} in "));
                source.push(lhs.rendered_source());
                return self.parse_for_body(
                    name,
                    declaration,
                    mutable,
                    ty,
                    source,
                    start,
                    label,
                    pattern.as_ref(),
                    None,
                );
            }
        };
        self.next();
        let rhs = self.parse_head(|parser| parser.parse_binary(0))?;
        // `for c in 'a'..='e'`: the chars between, as a chain of one loop.
        if matches!(self.inference.resolve(&lhs.ty), Ty::Char) {
            self.require_type(&lhs.ty, &rhs.ty, rhs.span)?;
            let range = self.range_iterator(&lhs, Some(&rhs), Ty::Char, operator == "..=");
            let Some((pipeline, item)) = self.for_pipeline(&range) else {
                return Err(self.unsupported("`for` over this `char` range"));
            };
            let (setup, header, entry, exit) = Self::for_parts(pipeline, &name);
            let mut looped = self.parse_for_body(
                name,
                declaration,
                mutable,
                item,
                header,
                start,
                label,
                pattern.as_ref(),
                Some((entry, exit)),
            )?;
            let mut source = origin::Fragment::plain("{ ");
            source.push(setup);
            source.push(looped.source);
            source.push_text(" }");
            looped.source = source;
            return Ok(looped);
        }
        let item = self.integer_bounds(&lhs, Some(&rhs))?;
        let mut source = origin::Fragment::plain(format!("for {name} in "));
        source.push(lhs.rendered_source());
        source.push_text(operator);
        source.push(rhs.rendered_source());
        self.parse_for_body(
            name,
            declaration,
            mutable,
            item,
            source,
            start,
            label,
            pattern.as_ref(),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_for_body(
        &mut self,
        name: ImmutableString,
        declaration: Span,
        mutable: bool,
        ty: Ty,
        source: origin::Fragment,
        start: Span,
        label: Option<ImmutableString>,
        pattern: Option<&pattern::Syntax>,
        fused: Option<(origin::Fragment, origin::Fragment)>,
    ) -> Result<ParsedExpr, Error> {
        let outer = self.bindings.clone();
        // Copies of the outer locals the loop's bindings shadow, made before
        // the loop.
        let mut names = vec![name.clone()];
        if let Some(pattern) = pattern {
            names.extend(drops::pattern_names(pattern));
        }
        let copies = self.shadow_copies(&names)?;
        // Each iteration's element and the pattern's bindings drop at its
        // end, and at a `break` or `continue`.
        let depth = self.drop_scopes.len();
        self.open_drops();
        if self.drop_scoped() {
            self.loop_drop_depth = Some(depth);
        }
        self.bindings.insert(
            name.clone(),
            Binding {
                ty: ty.clone(),
                mutable,
                available: true,
                declaration,
                kind: BindingKind::Local,
            },
        );
        self.note_drop_here(&name, declaration, &ty)?;
        let wrap = match pattern {
            Some(pattern) => match self
                .refuse_refutable(pattern, &ty, BindingSite::ForLoop)
                .and_then(|()| self.destructure(pattern, &name, &ty))
                .inspect(|_| {
                    // The pattern's bindings, given a value each iteration.
                    let bound: Vec<_> = self
                        .bindings
                        .iter()
                        .filter(|(name, binding)| {
                            outer
                                .get(*name)
                                .is_none_or(|before| before.declaration != binding.declaration)
                        })
                        .map(|(name, binding)| (name.clone(), binding.declaration))
                        .collect();
                    let locals: Vec<_> = bound
                        .iter()
                        .map(|(name, declaration)| self.flow().local(name, *declaration))
                        .collect();
                    if let Some((.., defined)) = &mut self.loop_element {
                        defined.extend(locals);
                    }
                })
                .and_then(|statements| {
                    if !self.drops() {
                        return Ok(statements);
                    }
                    let bound = self.pattern_bindings(pattern);
                    let holder = drops::Holder::Local(name.to_string(), Vec::new());
                    let clears = self.pattern_drops(
                        Some(pattern),
                        &ty,
                        &holder,
                        &bound,
                        drops::Clears::InArm,
                    )?;
                    Ok(statements + clears.as_str())
                }) {
                Ok(statements) => Some((
                    origin::Fragment::plain(statements),
                    origin::Fragment::default(),
                )),
                Err(error) => {
                    self.bindings = outer;
                    return Err(error);
                }
            },
            None => None,
        };
        // A fused chain's stages run before the pattern, its exits after
        // the body.
        let wrap = match (fused, wrap) {
            (Some((mut entry, exit)), wrap) => {
                if let Some((statements, _)) = wrap {
                    entry.push(statements);
                }
                Some((entry, exit))
            }
            (None, wrap) => wrap,
        };
        let value = self.parse_loop_body(source, start, label, false, wrap, Some(&outer));
        // The loop's bindings end with it; what it moved stays moved.
        let inner = std::mem::replace(&mut self.bindings, outer);
        self.keep_moves_from(&inner);
        let mut value = value?;
        if !copies.is_empty() {
            value.source = value.source.surrounded(&format!("{{ {copies}"), " }");
        }
        Ok(value)
    }

    /// `for x in v` over a `Vec` or array whose elements have drop glue: the
    /// loop takes each element out of `v` as it goes (`__rawr_clear`), and
    /// what a `break` or `return` leaves drops when the loop ends.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parse_owned_for(
        &mut self,
        name: ImmutableString,
        declaration: Span,
        mutable: bool,
        ty: Ty,
        lhs: ParsedExpr,
        start: Span,
        label: Option<ImmutableString>,
        pattern: Option<&pattern::Syntax>,
    ) -> Result<ParsedExpr, Error> {
        let (items, index) = (self.temporary("items"), self.temporary("index"));
        self.open_drops();
        self.note_drop_here(&items.as_str().into(), lhs.span, &lhs.ty)?;
        let entry = origin::Fragment::plain(format!(
            "{}({items}, {index}); {index} += 1; ",
            crate::engine::KEYWORD_CLEAR
        ));
        let source = origin::Fragment::plain(format!("for {name} in {items}"));
        let looped = self.parse_for_body(
            name,
            declaration,
            mutable,
            ty,
            source,
            start,
            label,
            pattern,
            Some((entry, origin::Fragment::default())),
        );
        let rest = self.close_drops()?;
        let mut looped = looped?;
        let mut source = origin::Fragment::plain(format!("{{ let {items} = "));
        source.push(lhs.rendered_source());
        source.push_text(format!("; let {index} = 0; "));
        source.push(looped.source);
        source.push_text(format!("; {rest}() }}"));
        looped.source = source;
        Ok(looped)
    }
}
