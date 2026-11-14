//! Statements: an expression statement, an assignment, the end of a
//! statement, and the temporaries and scope frames a statement opens.

use super::*;

impl<'a> Parser<'a> {
    /// The statement written from `start`, opened when `opened` calls were
    /// noted, with the scope frames its calls open: bound ahead of it, and
    /// the statement run in their block, or, for a `let` (`inline`), ahead
    /// of it in its own block, so the rest of that block runs inside them.
    pub(super) fn statement_frames(
        &mut self,
        start: usize,
        opened: usize,
        frames: Vec<Frame>,
        inline: bool,
    ) -> Result<(), Error> {
        if frames.is_empty() {
            return Ok(());
        }
        let mut statement = self.output.split_off(start);
        if inline {
            if frames.iter().any(|frame| !frame.after.is_empty()) {
                return Err(self.unsupported(
                    "a `let` that binds a value borrowing a `&mut` buffer its call fills is \
                     not supported in Rawr yet; match on the call in a statement of its own",
                ));
            }
            for frame in frames {
                self.output.push(frame.outer);
                self.output.push(frame.inner);
            }
            self.output.push_text("\n");
            self.output.push(statement);
            return Ok(());
        }
        statement.trim_end();
        match self.last_expression {
            Some((tail, endpoint)) if tail >= start => {
                let value = statement.split_off(tail - start);
                // A block's tail alone in it whose value is the frame its
                // call opens, as tracing's `valueset_all!` block gives the
                // `ValueSet` its event's closure takes: the tail's
                // temporaries live to the end of the enclosing statement
                // before edition 2024, which is the only edition whose
                // rustc lets the value leave the block, so the frame is the
                // enclosing statement's.
                if let [frame] = frames.as_slice()
                    && frame.outer.is_empty()
                    && frame.after.is_empty()
                    && self.output.as_str().trim().is_empty()
                    && statement.as_str().trim().is_empty()
                    && frame
                        .inner
                        .as_str()
                        .strip_prefix("let ")
                        .and_then(|bound| bound.strip_prefix(value.as_str().trim()))
                        .is_some_and(|rest| rest.starts_with(" = "))
                    && self.frame_effects.last().is_some_and(|(_, effects)| {
                        effects.iter().all(|(_, noted)| *noted >= opened)
                    })
                    && let Some(enclosing) = self.frames.last_mut()
                {
                    enclosing.extend(frames);
                    let tail = self.write_output(value, endpoint);
                    self.last_expression = Some((tail, endpoint));
                    return Ok(());
                }
                let mut body = statement;
                body.push_text(" ");
                body.push(value);
                let block = self.framed(frames, body);
                let tail = self.write_output(block, endpoint);
                self.last_expression = Some((tail, endpoint));
            }
            _ => {
                let mut block = self.framed(frames, statement);
                block.push_text("\n");
                self.output.push(block);
            }
        }
        Ok(())
    }

    /// `body` run inside `frames`: `{ frames body }`, or, when a frame
    /// gives back copies, `{ outer; let kept = { frames body }; stores
    /// kept }`.
    pub(super) fn framed(
        &mut self,
        frames: Vec<Frame>,
        body: origin::Fragment,
    ) -> origin::Fragment {
        let mut inner = origin::Fragment::plain("{ ");
        let mut outer = origin::Fragment::default();
        let mut after = origin::Fragment::default();
        for frame in frames {
            outer.push(frame.outer);
            inner.push(frame.inner);
            after.push(frame.after);
        }
        inner.push(body);
        inner.push_text(" }");
        if after.is_empty() {
            return inner;
        }
        let kept = self.temporary("kept");
        let mut block = origin::Fragment::plain("{ ");
        block.push(outer);
        block.push_text(format!("let {kept} = "));
        block.push(inner);
        block.push_text("; ");
        block.push(after);
        block.push_text(format!(" {kept} }}"));
        block
    }

    pub(super) fn parse_statement(&mut self) -> Result<(), Error> {
        let pending = self.field_entries.len();
        let outer_lend = std::mem::take(&mut self.lend_self);
        let start = self.output.len();
        let opened = self.calls_noted;
        self.open_temporaries();
        self.flow().open_temporaries();
        self.open_frames();
        let parsed = self.parse_expression_statement(pending);
        // A tail's value goes on into the block's: a temporary it borrows in
        // an extending place goes on with it.
        let end = if self.last_expression.is_some() {
            ScopeEnd::Value
        } else {
            ScopeEnd::Statement
        };
        self.flow().close_temporaries(end);
        let frames = self.close_frames();
        let temporaries = self.close_temporaries();
        self.field_entries.truncate(pending);
        parsed?;
        self.statement_temporaries(start, &temporaries);
        self.statement_frames(start, opened, frames, false)?;
        self.end_lend_self(outer_lend, start);
        self.settle_parses()
    }

    /// The statement written from `start` with its temporaries: dropped
    /// after it, or after its value when it is a tail.
    pub(super) fn statement_temporaries(&mut self, start: usize, temporaries: &[String]) {
        if temporaries.is_empty() {
            return;
        }
        let mut statement = self.output.split_off(start);
        statement.trim_end();
        match self.last_expression {
            Some((tail, endpoint)) if tail >= start => {
                let value = statement.split_off(tail - start);
                let mut block = origin::Fragment::plain("{ ");
                block.push(statement);
                block.push_text(" ");
                block.push(self.with_temporaries(temporaries, value));
                block.push_text(" }");
                let tail = self.write_output(block, endpoint);
                self.last_expression = Some((tail, endpoint));
            }
            _ => {
                let mut block = origin::Fragment::plain("{ ");
                for temporary in temporaries {
                    block.push_text(format!("let {temporary} = (); "));
                }
                block.push(statement);
                block.push_text(" ");
                block.push_text(self.temporary_drops(temporaries));
                block.push_text("}\n");
                self.output.push(block);
            }
        }
    }

    /// End a statement that made a closure changing `self`: bind the
    /// closure's `self` before the statement and write it back after it.
    pub(super) fn end_lend_self(&mut self, outer: bool, start: usize) {
        if !std::mem::replace(&mut self.lend_self, outer) {
            return;
        }
        let mut statement = self.output.split_off(start);
        statement.trim_end();
        match self.last_expression {
            Some((tail, endpoint)) if tail >= start => {
                let value = self.temporary("tail");
                let mut source = origin::Fragment::plain(format!(
                    "{{ let {SELF_CAPTURE} = self; let {value} = "
                ));
                source.push(statement);
                source.push_text(format!("; self = {SELF_CAPTURE}; {value} }}"));
                let tail = self.write_output(source, endpoint);
                self.last_expression = Some((tail, endpoint));
            }
            _ => {
                self.output
                    .push_text(format!("let {SELF_CAPTURE} = self;\n"));
                self.output.push(statement);
                self.output.push_text(format!("\nself = {SELF_CAPTURE};\n"));
            }
        }
    }

    /// Each pending `parse` whose target a statement made known gets its
    /// error type, so later statements can use it. A statement inside the
    /// call's own expression (a closure's body in its chain) can settle it
    /// before the call is written: its helper waits for the call.
    pub(super) fn settle_parses(&mut self) -> Result<(), Error> {
        for pending in std::mem::take(&mut self.pending_parses) {
            let (target, error, span, _) = &pending;
            match self.settle_parse(target, error, *span, false)? {
                Some(helper) => self.settled_parses.push((pending.3, format!("{helper}("))),
                None => self.pending_parses.push(pending),
            }
        }
        for (placeholder, text) in std::mem::take(&mut self.settled_parses) {
            if !self.write_over(&placeholder, &text) {
                self.settled_parses.push((placeholder, text));
            }
        }
        Ok(())
    }

    pub(super) fn parse_expression_statement(&mut self, pending: usize) -> Result<(), Error> {
        let start = self.position();
        let operation_count = self.ops.len();
        let checkpoint = self.inference.checkpoint();
        let block_like = self.peek_block_like();
        let outer = std::mem::replace(&mut self.block_statement, block_like.then_some(start));
        let value = self.parse_expr_in(block_like);
        self.block_statement = outer;
        let value = value?;
        // Nothing runs after a diverging expression (`panic!()`).
        if matches!(self.inference.resolve(&value.ty), Ty::Never) {
            self.flow().diverge();
        }

        self.reject_invocation(&value.ty, value.span, "used except to call its methods")?;

        if assignment_token(&self.peek()) {
            let (rendered, end) = self.parse_assignment(value)?;
            let rendered = self.with_field_entries(pending, rendered);
            self.write_output(rendered, span(start, end.end()));
            self.last_type = Ty::Unit;
            self.last_expression = None;
            self.consume_terminator(start)?;
        } else {
            // As in Rust, a block-like expression statement needs no `;`
            // when it is not the tail, and its type must then be `()`. The
            // statements a false `#[cfg(..)]` removes after it go first, as
            // rustc removes them before it finds the tail.
            if block_like {
                self.skip_left_out_statements()?;
            }
            let terminated = if block_like
                && !matches!(
                    self.peek(),
                    Token::SemiColon | Token::RightBrace | Token::EOF
                ) {
                self.require_type(&Ty::Unit, &value.ty, value.span)?;
                true
            } else {
                self.consume_terminator(start)?
            };
            // A tail returning a `&mut` place gives its path.
            let value = if terminated {
                value
            } else {
                self.returned_place(value, block_like)?
            };
            if terminated {
                if value.ty.usage() == crate::host::usage::Policy::Required
                    && value.outcome != crate::host::usage::Outcome::Discardable
                {
                    return Err(Error::UnusedValue {
                        value: value.ty.name(&self.models),
                        expression: value.span,
                    });
                }
                if value.evaluation == crate::host::usage::Evaluation::Elidable {
                    let mut ops = std::mem::take(&mut self.ops);
                    #[cfg(feature = "rust-import")]
                    self.collect_imported_ops(&ops[operation_count..])?;
                    ops.truncate(operation_count);
                    self.ops = ops;
                    self.inference.rollback(checkpoint);
                    self.last_type = Ty::Unit;
                    self.last_expression = None;
                    return Ok(());
                }
                // As in Rust, a block ending in a diverging statement
                // (`return x;`, `break;`) diverges.
                let diverges = matches!(self.inference.resolve(&value.ty), Ty::Never);
                let dropped = if diverges {
                    None
                } else {
                    self.drop_statement_value(&value)?
                };
                let mut rendered = match dropped {
                    Some(dropped) => dropped,
                    None => self
                        .discarded(&value)
                        .unwrap_or_else(|| value.rendered_source()),
                };
                rendered.push_text(";");
                let rendered = self.with_field_entries(pending, rendered);
                self.write_output(rendered, value.span);
                self.last_type = if diverges { Ty::Never } else { Ty::Unit };
                self.last_expression = None;
            } else if self.field_entries.len() > pending {
                // A tail that changes a field, as through `entry` or a
                // `&mut self` call on it: stored back after it, then the
                // tail's value.
                if self.inference.resolve(&value.ty) == Ty::Unit {
                    let mut rendered = value.rendered_source();
                    rendered.push_text(";");
                    let rendered = self.with_field_entries(pending, rendered);
                    self.write_output(rendered, value.span);
                    self.last_type = Ty::Unit;
                    self.last_expression = None;
                } else {
                    self.consume_imported_value(&value, "returned value", value.span)?;
                    let tail = self.temporary("tail");
                    let mut rendered = origin::Fragment::plain(format!("let {tail} = "));
                    rendered.push(value.rendered_source());
                    rendered.push_text(";");
                    let rendered = self.with_field_entries(pending, rendered);
                    self.write_output(rendered, value.span);
                    let output_start = self.write_output(origin::Fragment::plain(tail), value.span);
                    self.last_type = value.ty;
                    self.last_expression = Some((output_start, value.span));
                }
            } else {
                self.consume_imported_value(&value, "returned value", value.span)?;
                let mut value = value;
                // A block's value moves out of a local its tail names, or
                // a part of one (T151).
                self.move_out_value(&mut value)?;
                let output_start = self.write_output(value.rendered_source(), value.span);
                self.last_type = value.ty;
                self.last_expression = Some((output_start, value.span));
            }
        }

        self.inference.commit(checkpoint);
        Ok(())
    }

    /// `place = value` or a compound assignment, after `place`.
    pub(super) fn parse_assignment(
        &mut self,
        value: ParsedExpr,
    ) -> Result<(origin::Fragment, Span), Error> {
        let assignment = self.next().0;
        let Some(lvalue) = value.lvalue else {
            // rustc's borrow check comes after the value's type check, and
            // points at the whole assignment.
            if let Some(Unplaced {
                assigned: Some(message),
                ..
            }) = self.unmovable.get(&value.span).cloned()
            {
                let replacement = self.parse_expr()?;
                return Err(Error::BorrowConflict {
                    code: "E0594",
                    message,
                    access: span(value.span.start(), replacement.span.end()),
                    borrow: Span::NONE,
                });
            }
            return Err(self.unsupported("assignment target"));
        };
        let binding_name = lvalue.binding().clone();
        let Some(binding) = self.mutation_binding(&binding_name) else {
            return Err(self.unsupported("assignment through an unknown binding"));
        };
        // rustc checks an assignment's types before its mutability: an
        // immutable local is reported once the value's type is.
        // A `let x;` not declared `mut` takes one assignment, where it has
        // no value yet; another is E0384 at the first.
        let deferred = matches!(lvalue, Lvalue::Binding { .. })
            .then(|| self.deferred.get(&binding.declaration).copied())
            .flatten();
        // A place below a `&mut` local is changed through the reference,
        // which needs no `mut` of its own, as `pixel.0[3] += 1` of a crate
        // method's `&mut Pixel` (#1318).
        let through_reference = !matches!(lvalue, Lvalue::Binding { .. })
            && matches!(
                self.inference.resolve(&binding.ty),
                Ty::Reference { mutable: true, .. }
            );
        let immutable =
            (!binding.mutable && !through_reference && matches!(binding.kind, BindingKind::Local))
                .then_some(binding.declaration)
                .filter(|_| binding.available || deferred.is_none())
                .map(|declaration| deferred.flatten().unwrap_or(declaration));
        // A place below a `self` that can't change it is rustc's E0594 once
        // the value's type is checked, as an immutable local's is.
        let receiver = (!binding.mutable
            && matches!(binding.kind, BindingKind::Receiver { .. })
            && !matches!(lvalue, Lvalue::Binding { .. }))
        .then(|| binding.ty.clone());
        if !binding.mutable && !matches!(binding.kind, BindingKind::Local) && receiver.is_none() {
            return Err(match &binding.kind {
                BindingKind::Receiver { owner, .. } => Error::ImmutableReceiver {
                    owner: owner.clone(),
                    mutation: value.span,
                    receiver: binding.declaration,
                },
                BindingKind::Local => unreachable!("an immutable local is reported below"),
                BindingKind::Invocation => Error::InvalidHostOperation {
                    item: lvalue.ty().name(&self.models),
                    reason:
                        "assigning to the parameter that receives it is not supported in Rawr yet"
                            .into(),
                    operation: value.span,
                },
            });
        }
        let referent =
            matches!(binding.ty, Ty::Reference { .. }) && !matches!(value.ty, Ty::Reference { .. });
        // `r = &mut b;` of a `&mut` local given new borrows.
        if compound_operator(&assignment).is_none()
            && immutable.is_none()
            && receiver.is_none()
            && let Some((selector, mutable)) = self.rebound_selector(&value.ty, &lvalue)
        {
            return self.rebind_selector(value.span, &value.ty, selector, mutable);
        }
        self.refuse_rebound_alias(value.span, &value.ty)?;
        let value_start = self.flow().value_start();
        let mut replacement = self.parse_expr()?;
        let compound = compound_operator(&assignment).is_some();
        self.assigned_place(
            value.span,
            &value.ty,
            &lvalue,
            &replacement,
            value_start,
            compound,
        );
        self.effect(value.span.start());
        let mut opened = None;
        if let Some(operator) = compound_operator(&assignment) {
            let target = match &lvalue {
                Lvalue::Nested(nested) if opens_compound(nested) => {
                    let (target, parts) = self.open_compound(nested, value.span, &mut replacement);
                    opened = Some(parts);
                    target
                }
                _ => self.operator_expr(value.source, lvalue.ty().clone(), value.span),
            };
            replacement = self.compound_assignment(operator, target, replacement)?;
        }
        self.coerce_expr(lvalue.ty(), &mut replacement)?;
        if let Some(declaration) = immutable {
            let assignment = span(value.span.start(), replacement.span.end());
            return Err(self.immutable_assignment(
                &lvalue,
                self.expression_span(declaration),
                self.expression_span(assignment),
            ));
        }
        if let Some(receiver) = receiver {
            let assignment = span(value.span.start(), replacement.span.end());
            return Err(self.receiver_assignment(&receiver, assignment));
        }
        self.consume_imported_value(&replacement, &binding_name, replacement.span)?;
        self.move_local(&mut replacement)?;
        if compound_operator(&assignment).is_none()
            && self.needs_drop(lvalue.ty(), replacement.span)?
            && let Some(read) = self.replaced_read(&lvalue)
        {
            self.drop_replaced(&lvalue.ty().clone(), read, &mut replacement)?;
        }
        if referent && matches!(&lvalue, Lvalue::Binding { .. } | Lvalue::Place { .. }) {
            let target = match &lvalue {
                Lvalue::Binding { .. } => origin::Fragment::plain(binding_name.as_str()),
                Lvalue::Place { place, .. } => place.clone(),
                _ => unreachable!("a direct referent has a direct target"),
            };
            let helper = self.push_op(Op::Prelude(PreludeOp::ReferentStore));
            let mut rendered = target.surrounded("(", &format!(").{helper}("));
            rendered.push(replacement.rendered_source());
            rendered.push_text(");");
            return Ok((rendered, replacement.span));
        }
        let rendered = match lvalue {
            Lvalue::Binding { .. } => {
                let mut rendered = origin::Fragment::plain(format!("{binding_name} = "));
                rendered.push(replacement.rendered_source());
                rendered.push_text(";");
                if let Some(binding) = self.bindings.get_mut(&binding_name) {
                    binding.available = true;
                    if let Some(first @ None) = self.deferred.get_mut(&binding.declaration) {
                        *first = Some(span(value.span.start(), replacement.span.end()));
                    }
                }
                rendered
            }
            Lvalue::Place { place, .. } => {
                let mut rendered = place;
                rendered.push_text(" = ");
                rendered.push(replacement.rendered_source());
                rendered.push_text(";");
                rendered
            }
            Lvalue::Field(field) => {
                let mut rendered = field.place();
                rendered.push_text(" = ");
                rendered.push(replacement.rendered_source());
                rendered.push_text(";");
                rendered
            }
            Lvalue::Slice(_) => return Err(self.unsupported("assigning to a slice range")),
            Lvalue::Nested(nested) => match opened {
                Some(parts) => parts.store(replacement.rendered_source()),
                None => self.store_nested(&nested, replacement.rendered_source()),
            },
        };
        Ok((rendered, replacement.span))
    }

    /// `place = value` or a compound assignment after `place`, as an
    /// expression: Rust's assignment expression, whose type is `()`.
    pub(super) fn parse_assignment_expr(&mut self, place: ParsedExpr) -> Result<ParsedExpr, Error> {
        let start = place.span;
        let (rendered, end) = self.parse_assignment(place)?;
        Ok(self.operator_expr(
            rendered.surrounded("{ ", " }"),
            Ty::Unit,
            span(start.start(), end.end()),
        ))
    }

    pub(super) fn consume_terminator(&mut self, start: Position) -> Result<bool, Error> {
        self.terminate(start, Continuation::Statement)
    }

    /// The end of a binding: a `let`'s value may also go on with `else`.
    pub(super) fn consume_binding_terminator(
        &mut self,
        start: Position,
        let_keyword: bool,
    ) -> Result<bool, Error> {
        let continuation = if let_keyword {
            Continuation::Let
        } else {
            Continuation::Statement
        };
        self.terminate(start, continuation)
    }

    pub(super) fn terminate(
        &mut self,
        start: Position,
        continuation: Continuation,
    ) -> Result<bool, Error> {
        if matches!(self.peek(), Token::SemiColon) {
            self.next();
            return Ok(true);
        }
        if matches!(self.peek(), Token::EOF | Token::RightBrace) {
            return Ok(false);
        }
        if let Some(error) = self.expected_operator(continuation) {
            return Err(error);
        }
        Err(Error::UnsupportedNominalSyntax {
            construct: "statement without ';'".into(),
            source: span(start, self.position()),
        })
    }

    /// Notes each call as its arguments close, for `frame_effects`: a `(`
    /// or `()` after a name that is not a type's or a variant's, after a
    /// `)` or `]`, or `()` after a turbofish.
    pub(super) fn track_calls(&mut self, token: &Token, position: Position) {
        let callee = self.callee.take();
        match token {
            Token::LeftParen => self
                .call_parens
                .push(callee.filter(|(_, turbofish)| !turbofish).map(|(at, _)| at)),
            Token::RightParen => {
                if let Some(Some(at)) = self.call_parens.pop() {
                    self.effect(at);
                }
            }
            Token::Unit => {
                if let Some((at, _)) = callee {
                    self.effect(at);
                }
            }
            _ => {}
        }
        self.callee = match token {
            Token::Identifier(name) if !name.starts_with(char::is_uppercase) => {
                Some((position, false))
            }
            Token::RightParen | Token::RightBracket | Token::Unit => Some((position, false)),
            Token::GreaterThan => Some((position, true)),
            _ => None,
        };
    }

    /// A call or assignment starting at `at`, in each statement being
    /// parsed.
    pub(super) fn effect(&mut self, at: Position) {
        for (_, effects) in &mut self.frame_effects {
            effects.push((at, self.calls_noted));
        }
        self.calls_noted += 1;
    }

    /// What `take` gives, with the calls among the tokens it reads left
    /// out of `frame_effects`: tokens taken to be parsed later are noted
    /// as they are parsed, where they run.
    pub(super) fn unnoted<T>(&mut self, take: impl FnOnce(&mut Self) -> T) -> T {
        let noted = self.noted_effects();
        let taken = take(self);
        self.unnote(noted);
        taken
    }

    /// How many calls and assignments each statement being parsed has noted
    /// so far, for `unnote`.
    pub(super) fn noted_effects(&self) -> Vec<usize> {
        self.frame_effects
            .iter()
            .map(|(_, effects)| effects.len())
            .collect()
    }

    /// Leaves out of `frame_effects` each call or assignment noted since
    /// `noted_effects` gave `noted`.
    pub(super) fn unnote(&mut self, noted: Vec<usize>) {
        for ((_, effects), noted) in self.frame_effects.iter_mut().zip(noted) {
            effects.truncate(noted);
        }
    }

    /// Starts collecting the scope frames of a statement.
    pub(super) fn open_frames(&mut self) {
        self.frames.push(Vec::new());
        let start = self.position();
        self.frame_effects.push((start, Vec::new()));
    }

    /// The scope frames the statement since `open_frames` opens.
    pub(super) fn close_frames(&mut self) -> Vec<Frame> {
        self.frame_effects.pop();
        self.frames.pop().unwrap_or_default()
    }
}
