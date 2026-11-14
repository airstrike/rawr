//! `let` and the bindings it makes: `let pattern = value;`, `let x;`,
//! `let .. else`, and the destructuring of irrefutable patterns.

use super::*;

impl<'a> Parser<'a> {
    /// Bind each pattern parameter (`(x, y): (i64, i64)`) at the top of the
    /// body, from the compiler-private name the parameter takes.
    pub(super) fn destructure_parameters(
        &mut self,
        parameters: &[item::Parameter],
        body: Span,
    ) -> Result<(), Error> {
        for parameter in parameters {
            let Some(pattern) = &parameter.pattern else {
                continue;
            };
            let ty = Ty::from_resolved(&parameter.ty);
            // `mut r: &mut T` is the borrow the parameter is, as `let mut
            // r = parameter;` keeps it: the caller's place, or one a new
            // borrow gives it (`reborrow.rs`).
            if let crate::item::ParameterPattern::Binding {
                name,
                mutable: true,
            } = pattern
                && matches!(ty, Ty::Reference { mutable: true, .. })
            {
                let at = name.span.start();
                let identifier = |text: &str| (Token::Identifier(Box::new(text.into())), at);
                self.splice_tokens(vec![
                    (Token::Let, at),
                    (Token::Mut, at),
                    identifier(name.text.as_str()),
                    (Token::Equals, at),
                    identifier(parameter.name.text.as_str()),
                    (Token::SemiColon, at),
                ]);
                self.parse_binding(true)?;
                continue;
            }
            let syntax = self.parameter_pattern_syntax(pattern)?;
            // `E::A(x): E` of a one-variant enum, or `Some(x): Option<T>`:
            // matched as `let` matches it, `let pattern = parameter;`, after
            // the coverage check finds it irrefutable, or refused as rustc's
            // E0005.
            if syntax.may_refute() {
                let at = syntax.span();
                self.splice_tokens(vec![
                    (
                        Token::Identifier(Box::new(parameter.name.text.as_str().into())),
                        at.start(),
                    ),
                    (Token::SemiColon, at.end()),
                ]);
                let pending = self.field_entries.len();
                let value_start = self.flow().value_start();
                let subject = self.parse_expr()?;
                self.expect(Token::SemiColon, "';' after a parameter's value")?;
                let matched = self.matched(&subject, value_start);
                self.bind_let_pattern(
                    pattern::Site::Argument,
                    at.start(),
                    &syntax,
                    subject,
                    Some(matched),
                    None,
                    at.end(),
                    pending,
                )?;
                continue;
            }
            let statements = self.destructure(&syntax, &parameter.name.text, &ty)?;
            self.write_output(statements, body);
            // The parameter's value keeps what the pattern does not move out.
            if self.drops() {
                let bound = self.pattern_bindings(&syntax);
                let holder = drops::Holder::Local(parameter.name.text.to_string(), Vec::new());
                let clears =
                    self.pattern_drops(Some(&syntax), &ty, &holder, &bound, drops::Clears::InArm)?;
                self.write_output(clears, body);
            }
        }
        Ok(())
    }

    /// Whether a method call follows: `.name(`.
    #[cfg(feature = "rust-import")]
    pub(super) fn method_follows(&self) -> bool {
        let mut ahead = self.input.clone();
        matches!(
            (ahead.next(), ahead.next(), ahead.next()),
            (
                Some((Token::Period, _)),
                Some((Token::Identifier(_), _)),
                Some((Token::LeftParen | Token::Unit, _))
            )
        )
    }

    pub(super) fn parse_binding(&mut self, let_keyword: bool) -> Result<(), Error> {
        let pending = self.field_entries.len();
        let outer_lend = std::mem::take(&mut self.lend_self);
        let start = self.output.len();
        let opened = self.calls_noted;
        let depth = self.temporary_scopes.len();
        self.open_temporaries();
        self.flow().open_temporaries();
        self.open_frames();
        let bound = self.parse_binding_statement(let_keyword);
        self.flow().close_temporaries(ScopeEnd::Let);
        let frames = self.close_frames();
        self.field_entries.truncate(pending);
        // A form of `let` that does not drop its temporaries with its value.
        while self.temporary_scopes.len() > depth {
            let temporaries = self.close_temporaries();
            if bound.is_ok() && !temporaries.is_empty() {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a temporary value whose type implements `Drop` in this form of \
                                `let`: dropping it where Rust does is not supported in Rawr yet"
                        .into(),
                    source: point_span(self.position()),
                });
            }
        }
        bound?;
        self.statement_frames(start, opened, frames, true)?;
        self.end_lend_self(outer_lend, start);
        self.settle_parses()
    }

    /// The statement `rendered` with the field maps its `entry` calls
    /// changed (`field_entries` after `pending`) copied out before it and
    /// stored back after it.
    pub(super) fn with_field_entries(
        &mut self,
        pending: usize,
        rendered: origin::Fragment,
    ) -> origin::Fragment {
        if self.field_entries.len() <= pending {
            return rendered;
        }
        let entries = self.field_entries.split_off(pending);
        let mut statement = origin::Fragment::default();
        for (read, _) in &entries {
            statement.push(read.clone());
        }
        statement.push(rendered);
        for (_, store) in entries {
            statement.push(store);
        }
        statement
    }

    pub(super) fn parse_binding_statement(&mut self, let_keyword: bool) -> Result<(), Error> {
        let pending = self.field_entries.len();
        let start = self.next().1;
        if let_keyword && self.let_else_ahead() {
            return self.parse_let_else(start);
        }
        if let_keyword
            && matches!(
                self.peek(),
                Token::LeftParen
                    | Token::LeftBracket
                    | Token::Ampersand
                    | Token::And
                    | Token::Underscore
            )
            || let_keyword
                && matches!(self.peek_second(), Token::Reserved(ref text) if text.as_str() == "@")
            // `let W(x) = w;`, `let module::W(x) = w;`.
            || let_keyword
                && matches!(self.peek(), Token::Identifier(_))
                && matches!(
                    self.peek_second(),
                    Token::LeftParen | Token::DoubleColon | Token::LeftBrace
                )
            || let_keyword
                && matches!(self.peek(), Token::Identifier(ref name) if name.as_str() == "ref")
                && matches!(self.peek_second(), Token::Identifier(_) | Token::Mut)
            // `let None = o;` and `let 1 = n;`, which the coverage check
            // refuses as rustc's E0005.
            || let_keyword
                && matches!(
                    self.peek(),
                    Token::Identifier(ref name) if Prelude::from_name(name) == Some(Prelude::None)
                )
            || let_keyword
                && matches!(
                    self.peek(),
                    Token::IntegerConstant(_)
                        | Token::FloatConstant(_)
                        | Token::ByteConstant(_)
                        | Token::ByteStringConstant(_)
                        | Token::CharConstant(_)
                        | Token::StringConstant(_)
                        | Token::True
                        | Token::False
                        | Token::Minus
                        | Token::UnaryMinus
                )
        {
            if let_keyword && self.split_deferred_tuple() {
                return self.parse_binding_statement(true);
            }
            return self.parse_pattern_binding(start);
        }
        let mutable = let_keyword && matches!(self.peek(), Token::Mut);
        if mutable {
            self.next();
        }
        let (name, name_span) = self.expect_name("binding name")?;
        let annotation = if matches!(self.peek(), Token::Colon) {
            self.next();
            // `let mut f: fn(i32) -> i32`: a call of `f` borrows it shared.
            if matches!(self.peek(), Token::Fn) {
                self.shared_call_fns.insert(name_span);
            }
            Some(self.parse_local_type()?)
        } else {
            None
        };
        if let_keyword && matches!(self.peek(), Token::SemiColon) {
            return self.parse_deferred_binding(start, name, name_span, mutable, annotation);
        }
        // `const X = 3;`: a constant needs its type, as rustc says.
        if !let_keyword && annotation.is_none() {
            return Err(Error::Syntax {
                message: "missing type for `const` item".into(),
                label: "provide a type for the constant".into(),
                help: None,
                at: name_span,
            });
        }
        self.expect(Token::Equals, "'=' after binding name")?;
        // `let grains = map.entry(k).or_default();`: each use of `grains`
        // is the entry again, so `*grains += n` changes the map. Rust's
        // borrow keeps the map and the entry as they are meanwhile.
        if !mutable
            && annotation.is_none()
            && let Some(tokens) = self
                .entry_alias_tokens()
                .or_else(|| self.downcast_alias_tokens())
        {
            self.entry_aliases.insert(name.clone(), (name_span, tokens));
        } else {
            self.entry_aliases.remove(&name);
        }
        // `let r = &mut place;` keeps the borrow: `r` is the place.
        let borrowing =
            matches!(self.peek(), Token::Ampersand) && matches!(self.peek_second(), Token::Mut);
        let borrowed_value = matches!(self.peek(), Token::Ampersand | Token::And);
        let moved_borrow = matches!(self.peek(), Token::Identifier(ref alias) if self.aliases.contains_key(alias.as_str()))
            && matches!(self.peek_second(), Token::SemiColon);
        let block_like = self.peek_block_like();
        let outer = self.mutable_borrow;
        if borrowing {
            self.mutable_borrow = Some(self.expression_stack.len() + 1);
        }
        self.last_cell_borrow = None;
        let value_start = self.flow().value_start();
        // `let f: fn(&usize) -> usize = |x| *x * 4;`: the closure's
        // parameters have the annotation's types, as in Rust.
        if matches!(self.peek(), Token::Pipe | Token::Or)
            && let Some(Ty::Fn { inputs, .. }) = annotation
                .as_ref()
                .map(|annotation| self.inference.resolve(annotation))
            && !inputs.iter().any(contains_inference)
        {
            self.closure_hint = Some(inputs);
        }
        let value = self.parse_expr_expecting(annotation.as_ref());
        self.closure_hint = None;
        self.mutable_borrow = outer;
        let mut value = value?;
        // `let mut it = v.iter_mut();`: each `it.next()` lends the next
        // element of `v`.
        match self.pending_iter_mut.take() {
            Some(mut held) if held.declaration == value.span => {
                held.declaration = name_span;
                self.held_iter_mut.insert(name.clone(), held);
            }
            _ => {
                self.held_iter_mut.remove(&name);
            }
        }
        // A local's type is sized (rustc's E0277): `let x = *s;`.
        if self.inference.resolve(&value.ty) == Ty::UnsizedStr {
            return Err(Error::MissingCapability {
                owner: "str".into(),
                capability: "Sized".into(),
                operation: name_span,
                derive: false,
                required_by: None,
            });
        }
        // `let mut f = make();` of `fn make() -> impl Fn(i32)`: a call of `f`
        // borrows it shared, as `Fn::call` does (T179).
        if self.shared_call_fns.contains(&value.span) {
            self.shared_call_fns.insert(name_span);
        }
        // `let v = cell.borrow_mut();`: the borrow lasts for `v`'s block.
        if let Some(borrow) = self.last_cell_borrow.take()
            && borrow.span == value.span
            && annotation.is_none()
        {
            self.entry_aliases.remove(&name);
            self.bound_value(&name, name_span, &value, value_start);
            let rendered = self.bind_cell_borrow(&name, name_span, mutable, borrow);
            let rendered = self.with_field_entries(pending, rendered);
            self.write_output(rendered, span(start, value.span.end()));
            self.last_type = Ty::Unit;
            self.last_expression = None;
            return self
                .consume_binding_terminator(start, let_keyword)
                .map(|_| ());
        }
        // `let r = c.slot(0);`, `let x = v.get_mut(0).unwrap();`: the call
        // is made here, once, and each use of `r` is the place it gave.
        if let Ty::Reference { mutable: true, .. } = self.inference.resolve(&value.ty)
            && let Some(Lvalue::Nested(nested)) = &value.lvalue
            && Self::reached_once(nested)
        {
            let nested = (**nested).clone();
            if let Some(annotation) = &annotation {
                self.coerce_expr(annotation, &mut value)?;
            }
            let (statement, held) = self.hold_place(nested);
            let rendered = self.with_field_entries(pending, statement);
            self.write_output(rendered, span(start, value.span.end()));
            self.entry_aliases.remove(&name);
            self.bound_value(&name, name_span, &value, value_start);
            self.borrow_lvalue(name, name_span, value.ty, Lvalue::Nested(Box::new(held)));
            self.last_type = Ty::Unit;
            self.last_expression = None;
            return self
                .consume_binding_terminator(start, let_keyword)
                .map(|_| ());
        }
        // `let r = if c { &mut self.a } else { &mut self.b };` in a function
        // returning `&mut`: each branch gave its path below `self`
        // (`returned_place`), and `r` is the place it names.
        if self.place_output
            && self.closures.is_empty()
            && block_like
            && value.lvalue.is_none()
            && matches!(
                self.inference.resolve(&value.ty),
                Ty::Reference { mutable: true, .. }
            )
        {
            let path = self.temporary("path");
            let mut rendered = origin::Fragment::plain(format!("let {path} = "));
            rendered.push(value.rendered_source());
            rendered.push_text(";");
            let rendered = self.with_field_entries(pending, rendered);
            self.write_output(rendered, span(start, value.span.end()));
            let receiver = Lvalue::Binding {
                binding: "self".into(),
                ty: Ty::Unknown,
            };
            let step = PlaceStep::Path {
                path: origin::Fragment::plain(path),
                call: None,
            };
            let held = places::place_step(&receiver, step, value.ty.clone());
            self.entry_aliases.remove(&name);
            self.bound_value(&name, name_span, &value, value_start);
            self.borrow_lvalue(name, name_span, value.ty, held);
            self.last_type = Ty::Unit;
            self.last_expression = None;
            return self
                .consume_binding_terminator(start, let_keyword)
                .map(|_| ());
        }
        // A closure that changes `self` writes it back after its statement,
        // so it cannot outlive that statement in a local.
        if self.lend_self
            && matches!(
                self.inference.resolve(&value.ty),
                Ty::Fn { .. } | Ty::FnItem { .. } | Ty::Boxed(..)
            )
        {
            return Err(self.unsupported("a closure that changes `self` kept in a local"));
        }
        // `let r = &s;`, `let r = &s.a.b;` of a value Rust moves: `r` is the
        // place, read where it is at each use, as Rust's shared borrow
        // keeps it unchanged meanwhile (D568). A local shadowed ahead is
        // copied as before.
        if borrowed_value
            && !borrowing
            && !mutable
            && annotation.is_none()
            && let Ty::Reference {
                mutable: false,
                target,
            } = self.inference.resolve(&value.ty)
            && self.moves_by_value(&target)
            && let Some((root, place)) = self.shared_borrow(&value)
            && !self.root_shadowed_later(&root)
        {
            let rendered = self.with_field_entries(pending, origin::Fragment::default());
            if !rendered.is_empty() {
                self.write_output(rendered, value.span);
            }
            self.entry_aliases.remove(&name);
            self.bound_value(&name, name_span, &value, value_start);
            let lvalue = Lvalue::Place {
                binding: root,
                ty: Ty::Unknown,
                place,
            };
            self.borrow_lvalue(name, name_span, value.ty, lvalue);
            self.last_type = Ty::Unit;
            self.last_expression = None;
            return self
                .consume_binding_terminator(start, let_keyword)
                .map(|_| ());
        }
        if let Ty::Reference {
            mutable: true,
            target,
        } = self.inference.resolve(&value.ty)
            && (borrowing || moved_borrow)
        {
            // `let mut r = &mut a;` given a new borrow later (`r = &mut b`),
            // or `let s = r;` of such a local: a runtime local holding
            // which local it refers to (`reborrow.rs`).
            if let Some((source, selected)) = self.reborrow_source(&value)
                && (selected || mutable && self.rebound_later(&name) || self.shadowed_later(&value))
            {
                if let Some(annotation) = &annotation {
                    self.coerce_expr(annotation, &mut value)?;
                }
                let rendered = origin::Fragment::plain(format!("let {name} = {source};"));
                let rendered = self.with_field_entries(pending, rendered);
                self.write_output(rendered, span(start, value.span.end()));
                self.entry_aliases.remove(&name);
                self.bound_value(&name, name_span, &value, value_start);
                self.select_local(name, name_span, value.ty.clone(), mutable);
                self.last_type = Ty::Unit;
                self.last_expression = None;
                return self
                    .consume_binding_terminator(start, let_keyword)
                    .map(|_| ());
            }
            // `let r = &mut s.cells[i];`: `r` is the place, reached again
            // from `s` at each use, its indexes read once, where the borrow
            // is made (T392). When a `let` ahead shadows `s`, `r` holds the
            // place's path instead, which reaches the first `s`.
            if let Some(lvalue @ (Lvalue::Field(_) | Lvalue::Nested(_))) = value.lvalue.clone() {
                if let Some(annotation) = &annotation {
                    self.coerce_expr(annotation, &mut value)?;
                }
                if !self.in_async_body()
                    && self.root_shadowed_later(lvalue.binding())
                    && let Some(located) = self.place_located(&lvalue)
                {
                    let holder = self.temporary("part");
                    let mut rendered = origin::Fragment::plain(format!("let {holder} = "));
                    rendered.push(located);
                    rendered.push_text(";");
                    let rendered = self.with_field_entries(pending, rendered);
                    self.write_output(rendered, span(start, value.span.end()));
                    self.entry_aliases.remove(&name);
                    self.bound_value(&name, name_span, &value, value_start);
                    self.borrow_located(name, name_span, value.ty, &holder);
                    self.last_type = Ty::Unit;
                    self.last_expression = None;
                    return self
                        .consume_binding_terminator(start, let_keyword)
                        .map(|_| ());
                }
                let mut indexes = origin::Fragment::default();
                let lvalue = self.hoist_indexes(lvalue, &mut indexes);
                let rendered = self.with_field_entries(pending, indexes);
                if !rendered.is_empty() {
                    self.write_output(rendered, value.span);
                }
                self.entry_aliases.remove(&name);
                self.bound_value(&name, name_span, &value, value_start);
                self.borrow_lvalue(name, name_span, value.ty, lvalue);
                self.last_type = Ty::Unit;
                self.last_expression = None;
                return self
                    .consume_binding_terminator(start, let_keyword)
                    .map(|_| ());
            }
            // `let s = r;` moves the borrow `r` into `s`.
            let Some(Lvalue::Binding { binding: root, .. } | Lvalue::Place { binding: root, .. }) =
                value.lvalue.clone()
            else {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a `&mut` borrow kept in a local other than `let r = &mut place;`"
                        .into(),
                    source: value.span,
                });
            };
            let place = match &value.lvalue {
                Some(Lvalue::Place { place, .. }) => place.clone(),
                _ => origin::Fragment::plain(root.as_str()),
            };
            // `let b: &mut dyn Bump = &mut c;`: the borrow as the annotation's
            // type, an unsized coercion included.
            let target = match &annotation {
                Some(annotation) => {
                    self.coerce_expr(annotation, &mut value)?;
                    match self.inference.resolve(annotation) {
                        Ty::Reference { target, .. } => target,
                        _ => target,
                    }
                }
                None => target,
            };
            // `let r = &mut v[i];`: the index is read once, where the
            // borrow is made, into a local the place then names (T392).
            let mut indexes = origin::Fragment::default();
            let place = self.hoist_chain(place, &mut indexes);
            if !indexes.is_empty() {
                self.write_output(indexes, value.span);
            }
            self.bound_value(&name, name_span, &value, value_start);
            self.borrow_place(name, name_span, *target, place, root.clone());
            self.last_type = Ty::Unit;
            self.last_expression = None;
            return self
                .consume_binding_terminator(start, let_keyword)
                .map(|_| ());
        }

        self.reject_invocation(&value.ty, value.span, "kept in a `let` binding")?;

        if let Some(expected) = &annotation {
            self.coerce_expr(expected, &mut value)?;
        }

        self.consume_imported_value(&value, &name, value.span)?;
        self.move_local(&mut value)?;
        // The temporaries the value made drop when it is made, unless a
        // borrow of one is the value, which Rust keeps to the block's end.
        let temporaries = self.close_temporaries();
        if !temporaries.is_empty() {
            if borrowed_value {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a `let` that borrows a temporary value whose type implements \
                                `Drop`: keeping it to the block's end is not supported in Rawr yet"
                        .into(),
                    source: value.span,
                });
            }
            value.source = self.with_temporaries(&temporaries, value.rendered_source());
            value.prelude = origin::Fragment::default();
        }
        // `let x = todo!();`: a value that never comes coerces to any
        // type, so the local's is the one its uses give it.
        let ty = annotation.unwrap_or_else(|| match self.inference.resolve(&value.ty) {
            Ty::Never => {
                let open = self.inference.fresh();
                self.diverging.push(open.clone());
                open
            }
            _ => value.ty.clone(),
        });
        // An immutable local iterator stays its chain: what it evaluates is
        // evaluated here, and each use runs it.
        let bound = match self.inference.resolve(&ty) {
            Ty::Iter(item, _) if !mutable && value.prelude.is_empty() => self
                .pipelines
                .get(value.source.as_str())
                .cloned()
                .map(|pipeline| (pipeline, *item)),
            _ => None,
        };
        let hold = self.note_drop(&name, name_span, &ty)?;
        self.bound_value(&name, name_span, &value, value_start);
        self.local_types.push(LocalType {
            name: name.clone(),
            declaration: name_span,
            ty: ty.clone(),
            value: Some(value.span),
        });
        let keyword = self.binding_keyword(mutable, &ty);
        self.bindings.insert(
            name.clone(),
            Binding {
                ty,
                mutable,
                available: true,
                declaration: name_span,
                kind: BindingKind::Local,
            },
        );
        // A shadowed value with drop glue is kept until its scope ends.
        if let Some(hold) = hold {
            let made = self.temporary("made");
            let mut rendered = origin::Fragment::plain(format!("let {made} = "));
            rendered.push(value.rendered_source());
            rendered.push_text(format!(
                "; {hold} {keyword} {name} = {}({made});",
                crate::engine::KEYWORD_MOVE
            ));
            let rendered = self.with_field_entries(pending, rendered);
            self.write_output(rendered, span(start, value.span.end()));
            self.last_type = Ty::Unit;
            self.last_expression = None;
            return self
                .consume_binding_terminator(start, let_keyword)
                .map(|_| ());
        }
        let mut rendered = origin::Fragment::plain(format!("{keyword} {name} = "));
        if let Some((pipeline, item)) = bound {
            let (code, pipeline, names) = self.bound_pipeline(pipeline);
            self.bound_iterators.insert(
                name.clone(),
                BoundIterator {
                    declaration: name_span,
                    pipeline,
                    item,
                    names,
                },
            );
            rendered = code;
            rendered.push_text(format!("{keyword} {name} = ()"));
        } else {
            rendered.push(value.rendered_source());
        }
        rendered.push_text(";");
        let rendered = self.with_field_entries(pending, rendered);
        self.write_output(rendered, span(start, value.span.end()));
        self.last_type = Ty::Unit;
        self.last_expression = None;
        self.consume_binding_terminator(start, let_keyword)
            .map(|_| ())
    }

    /// `let x;`, `let x: i32;`: a local an assignment gives its value
    /// later. Its type is the annotation's, else the one its assignments
    /// give it, and it has no value until one does (`deferred`).
    /// `let (a, (mut b, _));` and `let (a, b): (i32, String);` after the
    /// `let`: a tuple pattern with no value, read as a `let x;` for each of
    /// its names, with its part of the type, as Rust declares them. The
    /// tokens up to the `;` are taken and those `let`s put in their place;
    /// `false`, taking nothing, for any other pattern or one with a value.
    pub(super) fn split_deferred_tuple(&mut self) -> bool {
        if !matches!(self.peek(), Token::LeftParen) {
            return false;
        }
        let mut ahead = self.input.clone();
        let mut written = Vec::new();
        let mut depth = 0_i32;
        let mut colon = None;
        loop {
            let Some(token) = ahead.next() else {
                return false;
            };
            match token.0 {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::LessThan => {
                    depth += 1;
                }
                Token::RightParen
                | Token::RightBracket
                | Token::RightBrace
                | Token::GreaterThan => {
                    depth -= 1;
                }
                Token::RightShift => depth -= 2,
                Token::SemiColon if depth == 0 => break,
                Token::Colon if depth == 0 && colon.is_none() => colon = Some(written.len()),
                Token::Equals | Token::EOF if depth == 0 => return false,
                _ => {}
            }
            written.push(token);
        }
        let (pattern, ty) = match colon {
            Some(at) => (&written[..at], Some(&written[at + 1..])),
            None => (&written[..], None),
        };
        let mut lets = Vec::new();
        if deferred_lets(pattern, ty, &mut lets).is_none() || lets.is_empty() {
            return false;
        }
        // The last `let`'s `;` is the statement's own.
        lets.pop();
        for _ in 0..written.len() {
            self.next();
        }
        self.splice_tokens(lets);
        true
    }

    pub(super) fn parse_deferred_binding(
        &mut self,
        start: Position,
        name: ImmutableString,
        name_span: Span,
        mutable: bool,
        annotation: Option<Ty>,
    ) -> Result<(), Error> {
        let ty = annotation.unwrap_or_else(|| self.inference.fresh());
        self.entry_aliases.remove(&name);
        if mutable {
            self.flow().local(&name, name_span);
        } else {
            self.flow().single_assignment(&name, name_span);
        }
        self.deferred.insert(name_span, None);
        // It drops where its scope ends on the paths that gave it a value:
        // on the others its slot holds `()`, which the glue passes over.
        let hold = self.note_drop(&name, name_span, &ty)?.unwrap_or_default();
        self.local_types.push(LocalType {
            name: name.clone(),
            declaration: name_span,
            ty: ty.clone(),
            value: None,
        });
        self.bindings.insert(
            name.clone(),
            Binding {
                ty,
                mutable,
                available: false,
                declaration: name_span,
                kind: BindingKind::Local,
            },
        );
        self.write_output(
            origin::Fragment::plain(format!("{hold}let {name};")),
            span(start, name_span.end()),
        );
        self.last_type = Ty::Unit;
        self.last_expression = None;
        self.consume_terminator(start).map(|_| ())
    }

    /// `let (a, (mut b, _)) = value;`, `let &x = r;`, `let whole @ (a, b) = v;`:
    /// an irrefutable pattern binds the parts of the value.
    /// A `use` inside a body: the item parser hoisted it to the file's
    /// items, so its tokens are passed over here.
    pub(super) fn skip_nested_use(&mut self) -> Result<(), Error> {
        let start = self.expect(Token::Use, "`use` to start an import")?;
        loop {
            match self.next() {
                (Token::SemiColon, _) => return Ok(()),
                (Token::EOF, position) => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "`use` without a `;`".into(),
                        source: span(start, position),
                    });
                }
                _ => {}
            }
        }
    }

    /// The tokens up to the `;` ahead when they are a map entry's `&mut`:
    /// `map.entry(k)` ended by `or_default()`, `or_insert(v)` or
    /// `or_insert_with(f)`.
    pub(super) fn entry_alias_tokens(&mut self) -> Option<Vec<(Token, Position)>> {
        if self.split_angle.is_some() {
            return None;
        }
        let mut ahead = self.input.clone();
        let mut tokens = Vec::new();
        let mut depth = 0usize;
        loop {
            let (token, position) = ahead.next()?;
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.checked_sub(1)?;
                }
                Token::SemiColon if depth == 0 => break,
                Token::EOF => return None,
                _ => {}
            }
            tokens.push((token, position));
        }
        let entry = tokens.windows(2).position(|pair| {
            matches!(&pair[0].0, Token::Period)
                && matches!(&pair[1].0, Token::Identifier(name) if name.as_str() == "entry")
        })?;
        // The last call of the chain is the entry's value.
        let last = tokens
            .iter()
            .rposition(|(token, _)| matches!(token, Token::Period))?;
        let ends = matches!(
            tokens.get(last + 1),
            Some((Token::Identifier(name), _))
                if matches!(name.as_str(), "or_default" | "or_insert" | "or_insert_with")
        );
        (ends && last > entry).then_some(tokens)
    }

    /// `let state = tree.state.downcast_mut::<State>();`: each use of
    /// `state` is the Tree's state again, so `state.count += 1` changes it.
    pub(super) fn downcast_alias_tokens(&mut self) -> Option<Vec<(Token, Position)>> {
        let mut ahead = self.input.clone();
        let mut tokens = Vec::new();
        let mut depth = 0usize;
        loop {
            let (token, position) = ahead.next()?;
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.checked_sub(1)?;
                }
                Token::SemiColon if depth == 0 => break,
                Token::EOF => return None,
                _ => {}
            }
            tokens.push((token, position));
        }
        let last = tokens
            .iter()
            .rposition(|(token, _)| matches!(token, Token::Period))?;
        matches!(
            tokens.get(last + 1),
            Some((Token::Identifier(name), _)) if name.as_str() == "downcast_mut"
        )
        .then_some(tokens)
    }

    /// Whether the `let` ahead is `let pattern = value else { .. };`: an
    /// `else` before its `;` that no `}` precedes, since Rust forbids a
    /// value ending in `}` there.
    pub(super) fn let_else_ahead(&mut self) -> bool {
        // `let x = ..` and `let mut x = ..` bind whatever the value is.
        // `let None = ..` names a unit variant, a refutable pattern.
        if matches!(self.peek(), Token::Mut)
            || matches!(self.peek(), Token::Identifier(ref name) if !name.starts_with(char::is_uppercase))
                && matches!(self.peek_second(), Token::Equals | Token::Colon)
        {
            return false;
        }
        let mut depth = 0usize;
        let mut previous = Token::EOF;
        for (token, _) in self.input.clone() {
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                Token::SemiColon | Token::EOF if depth == 0 => return false,
                Token::Else if depth == 0 => return previous != Token::RightBrace,
                _ => {}
            }
            previous = token;
        }
        false
    }

    /// `let pattern = value else { diverging };`: the pattern's bindings
    /// after the statement, or the `else` block, which must diverge.
    pub(super) fn parse_let_else(&mut self, start: Position) -> Result<(), Error> {
        let pending = self.field_entries.len();
        let syntax = self.parse_pattern_syntax()?;
        self.expect(Token::Equals, "'=' after a `let` pattern")?;
        let (subject, parts) = self.parse_subject()?;
        self.expect(Token::Else, "`else` after a `let` value")?;
        if !matches!(self.peek(), Token::LeftBrace) {
            return Err(self.unsupported("`let ... else` without a block"));
        }
        let original = self.bindings.clone();
        let split = self.flow().split();
        self.flow().arm(split);
        let otherwise = self.parse_unexpected_block();
        let otherwise_end = self.flow().arm_end();
        self.flow().arm(split);
        let matched = self.flow().arm_end();
        self.flow().join(&[otherwise_end, matched]);
        self.bindings = original;
        let otherwise = otherwise?;
        if !matches!(self.inference.resolve(&otherwise.value.ty), Ty::Never) {
            return Err(Error::TypeMismatch {
                expected: "`!`".into(),
                actual: otherwise.value.ty.name(&self.models),
                expression: otherwise.syntax,
            });
        }
        let end = otherwise.syntax.end();
        self.consume_terminator(start)?;
        self.bind_let_pattern(
            pattern::Site::Let,
            start,
            &syntax,
            subject,
            Some(parts),
            Some(otherwise),
            end,
            pending,
        )
    }

    /// Whether a slice pattern in `syntax` matches a slice of unknown
    /// length in `ty`, which only a coverage check finds irrefutable
    /// (`[..]`, `[a @ ..]`), where an array's length decides it.
    pub(super) fn matches_unsized_slice(&self, syntax: &pattern::Syntax, ty: &Ty) -> bool {
        let ty = self.referenced(ty);
        match (syntax, &ty) {
            (pattern::Syntax::Slice { .. }, Ty::Array { kind, .. })
                if !matches!(kind, sequence::Kind::Array(_)) =>
            {
                true
            }
            (pattern::Syntax::Slice { before, after, .. }, Ty::Array { element, .. }) => before
                .iter()
                .chain(after.iter())
                .any(|member| self.matches_unsized_slice(member, element)),
            (pattern::Syntax::Tuple { members, .. }, Ty::Tuple(types)) => members
                .iter()
                .zip(types.iter())
                .any(|(member, ty)| self.matches_unsized_slice(member, ty)),
            (
                pattern::Syntax::TupleStruct {
                    members, fields, ..
                },
                _,
            ) => members
                .iter()
                .zip(fields.iter())
                .any(|(member, ty)| self.matches_unsized_slice(member, ty)),
            (
                pattern::Syntax::Reference { pattern, .. } | pattern::Syntax::At { pattern, .. },
                _,
            ) => self.matches_unsized_slice(pattern, &ty),
            _ => false,
        }
    }

    /// Bind an irrefutable pattern to the value at `path`, of type `ty`; the
    /// statements that do it. A refutable pattern is rejected, as in Rust.
    pub(super) fn destructure(
        &mut self,
        syntax: &pattern::Syntax,
        path: &str,
        ty: &Ty,
    ) -> Result<String, Error> {
        let mut statements = String::new();
        let mode = self.binding_mode.replace(None);
        let destructured = self.destructure_into(syntax, path, ty, &mut statements);
        self.binding_mode.set(mode);
        destructured.map(|()| statements)
    }

    /// The type a pattern below a reference matches, entering Rust's
    /// default `ref` binding mode (`ref mut` only through `&mut` all the
    /// way), with the mode to restore.
    pub(super) fn enter_reference(&self, ty: &Ty) -> (Ty, Option<Option<bool>>) {
        let mut ty = self.inference.resolve(ty);
        let mode = self.binding_mode.get();
        let mut entered = None;
        while let Ty::Reference { target, mutable } = ty {
            let outer = entered.or(mode);
            entered = Some(outer.map_or(mutable, |outer| outer && mutable));
            ty = self.inference.resolve(&target);
            self.trace_enter(borrowck::Projection::Deref);
        }
        match entered {
            Some(entered) => {
                self.binding_mode.set(Some(entered));
                (ty, Some(mode))
            }
            None => (ty, None),
        }
    }

    pub(super) fn destructure_into(
        &mut self,
        syntax: &pattern::Syntax,
        path: &str,
        ty: &Ty,
        statements: &mut String,
    ) -> Result<(), Error> {
        match syntax {
            pattern::Syntax::Discard { .. } => Ok(()),
            pattern::Syntax::UnitStruct { ty: unit, span } => {
                let target = match self.inference.resolve(ty) {
                    Ty::Reference { target, .. } => *target,
                    ty => ty,
                };
                self.require_type(unit, &target, *span)
            }
            pattern::Syntax::Binding {
                name,
                span,
                mutable,
            } => {
                let keyword = self.binding_keyword(*mutable, ty);
                let _ = write!(statements, " {keyword} {name} = {path};");

                // Below a reference the binding is one more reference, even
                // to a member that is itself a reference: `&(i64, &i64)`
                // binds `&&i64`.
                let ty = match (self.binding_mode.get(), self.inference.resolve(ty)) {
                    (Some(mutable), ty) => Ty::Reference {
                        mutable,
                        target: Box::new(ty),
                    },
                    (None, ty) => ty,
                };
                self.trace_bind(name, *span, &ty);
                self.bindings.insert(
                    name.clone(),
                    Binding {
                        ty,
                        mutable: *mutable,
                        available: true,
                        declaration: *span,
                        kind: BindingKind::Local,
                    },
                );
                Ok(())
            }
            // `ref x` binds a shared reference, a copy at runtime; `ref mut
            // x` is the destructured place itself, as `let x = &mut v;`.
            pattern::Syntax::Ref {
                name,
                span,
                mutable: false,
            } => {
                let _ = write!(statements, " const {name} = {path};");
                let target = self.inference.resolve(ty);
                let ty = Ty::Reference {
                    mutable: false,
                    target: Box::new(target.clone()),
                };
                self.trace_ref(false);
                self.trace_bind(name, *span, &ty);
                self.bindings.insert(
                    name.clone(),
                    Binding {
                        ty: Ty::Reference {
                            mutable: false,
                            target: Box::new(target),
                        },
                        mutable: false,
                        available: true,
                        declaration: *span,
                        kind: BindingKind::Local,
                    },
                );
                Ok(())
            }
            pattern::Syntax::Ref { name, span, .. } => {
                let Some(root) = self.place_root.clone() else {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "`ref mut` other than of a local's place or its tuple members"
                            .into(),
                        source: *span,
                    });
                };
                let target = self.inference.resolve(ty);
                self.trace_ref(true);
                self.trace_bind(
                    name,
                    *span,
                    &Ty::Reference {
                        mutable: true,
                        target: Box::new(target.clone()),
                    },
                );
                self.borrow_place(name.clone(), *span, target, path.into(), root);
                Ok(())
            }
            pattern::Syntax::At {
                name,
                name_span,
                pattern,
            } => {
                self.destructure_into(
                    &pattern::Syntax::Binding {
                        name: name.clone(),
                        span: *name_span,
                        mutable: false,
                    },
                    path,
                    ty,
                    statements,
                )?;
                self.destructure_into(pattern, path, ty, statements)
            }
            pattern::Syntax::TupleStruct {
                ty: pattern_ty,
                definition,
                fields,
                members,
                span,
            } => {
                let traced = self.trace_length();
                let (target, mode) = self.enter_reference(ty);
                // A field is read through its helper, not a place.
                let root = self.place_root.take();
                let names = match self.models.definition(definition.id()) {
                    Nominal::Struct { item, .. } => item
                        .fields
                        .iter()
                        .map(|field| field.name.text.clone())
                        .collect(),
                    Nominal::Enum { .. } => Vec::new(),
                };
                let mut destructure = || {
                    self.require_type(pattern_ty, &target, *span)?;
                    for (slot, (member, ty)) in members.iter().zip(fields.iter()).enumerate() {
                        let path = field_place(origin::Fragment::plain(path), slot);
                        let path = path.as_str();
                        let field = names
                            .get(slot)
                            .cloned()
                            .unwrap_or_else(|| slot.to_string().into());
                        let step = self.trace_enter_at(
                            borrowck::Projection::Field(field),
                            borrows::TraceRegion::Field(definition.id(), None, slot),
                        );
                        self.destructure_into(member, path, ty, statements)?;
                        self.trace_leave(step);
                    }
                    Ok(())
                };
                let destructured = destructure();
                self.place_root = root;
                if let Some(mode) = mode {
                    self.binding_mode.set(mode);
                }
                self.trace_leave(traced);
                destructured
            }
            pattern::Syntax::Tuple { members, span } => {
                // A tuple pattern also matches through `&(A, B)`, binding
                // references to its members.
                let traced = self.trace_length();
                let (target, mode) = self.enter_reference(ty);
                let discard = pattern::Syntax::Discard { span: *span };
                let arity = match &target {
                    Ty::Tuple(types) => types.len(),
                    _ => members.len(),
                };
                let members = match tuple_members(members, arity, &discard) {
                    Ok(members) => members,
                    Err(error) => {
                        if let Some(mode) = mode {
                            self.binding_mode.set(mode);
                        }
                        self.trace_leave(traced);
                        return Err(error);
                    }
                };
                let types = (0..members.len())
                    .map(|_| self.inference.fresh())
                    .collect::<Box<[_]>>();
                let mut destructure = || {
                    self.require_type(&Ty::Tuple(types.clone()), &target, *span)?;
                    for (index, (member, ty)) in members.iter().zip(types.iter()).enumerate() {
                        let step = self.trace_enter_at(
                            borrowck::Projection::Field(index.to_string().into()),
                            borrows::TraceRegion::At(borrowck::Path::of(&[borrowck::Step::Arg(
                                index as u8,
                            )])),
                        );
                        self.destructure_into(member, &format!("{path}[{index}]"), ty, statements)?;
                        self.trace_leave(step);
                    }
                    Ok(())
                };
                let destructured = destructure();
                if let Some(mode) = mode {
                    self.binding_mode.set(mode);
                }
                self.trace_leave(traced);
                destructured
            }
            pattern::Syntax::Slice {
                before,
                rest,
                after,
                span,
            } => {
                let traced = self.trace_length();
                let (target, mode) = self.enter_reference(ty);
                let destructured = self.destructure_array(
                    before,
                    rest.as_deref(),
                    after,
                    *span,
                    path,
                    &target,
                    statements,
                );
                if let Some(mode) = mode {
                    self.binding_mode.set(mode);
                }
                self.trace_leave(traced);
                destructured
            }
            pattern::Syntax::Reference {
                mutable,
                pattern,
                span,
            } => {
                let target = self.inference.fresh();
                let resolved = self.inference.resolve(ty);
                let expected = Ty::Reference {
                    mutable: *mutable,
                    target: Box::new(target.clone()),
                };
                match resolved {
                    Ty::Reference {
                        mutable: actual, ..
                    } if *mutable && !actual => {
                        return Err(Error::TypeMismatch {
                            expected: resolved.name(&self.models),
                            actual: "&mut _".into(),
                            expression: *span,
                        })
                    }
                    Ty::Reference { target: inner, .. } => {
                        // `&pat` matches the reference and binds by move.
                        let mode = self.binding_mode.replace(None);
                        let step = self.trace_enter(borrowck::Projection::Deref);
                        let destructured = self.destructure_into(pattern, path, &inner, statements);
                        self.trace_leave(step);
                        self.binding_mode.set(mode);
                        return destructured;
                    }
                    _ => {}
                }
                self.require_type(&expected, ty, *span)?;
                // References are the referenced value at runtime.
                let mode = self.binding_mode.replace(None);
                let step = self.trace_enter(borrowck::Projection::Deref);
                let destructured = self.destructure_into(pattern, path, &target, statements);
                self.trace_leave(step);
                self.binding_mode.set(mode);
                destructured
            }
            // `let Point { x, .. } = point;` on a facade struct: each field
            // read and bound.
            pattern::Syntax::Enum(pattern)
                if pattern.host_struct && matches!(self.inference.resolve(ty), Ty::Host { .. }) =>
            {
                let ty = self.inference.resolve(ty);
                if !drops::pattern_names(syntax).is_empty() {
                    self.trace_unmodeled("a pattern binding a part of a host value", syntax.span());
                }
                let traced = self.begin_trace();
                let mut out = pattern::Host::default();
                let hosted =
                    self.host_pattern(syntax, origin::Fragment::plain(path), &ty, &mut out);
                self.end_trace(traced);
                hosted?;
                if !out.tests.is_empty() {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "a refutable pattern in a binding; use `if let` or `match`"
                            .into(),
                        source: pattern.owner_span,
                    });
                }
                for (binding, value) in out.binds.into_iter().zip(out.values) {
                    let value = value.as_str().to_string();
                    let binding_pattern = pattern::Syntax::Binding {
                        name: binding.name.clone(),
                        span: binding.span,
                        mutable: binding.mutable,
                    };
                    self.destructure_into(&binding_pattern, &value, &binding.ty, statements)?;
                }
                Ok(())
            }
            other => Err(Error::UnsupportedNominalSyntax {
                construct: "a refutable pattern in a binding; use `if let` or `match`".into(),
                source: other.span(),
            }),
        }
    }

    /// `let [a, rest @ .., z] = array;`: irrefutable on an array whose
    /// length the pattern fits, as in Rust; a slice's is refutable.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn destructure_array(
        &mut self,
        before: &[pattern::Syntax],
        rest: Option<&pattern::Syntax>,
        after: &[pattern::Syntax],
        pattern: Span,
        path: &str,
        ty: &Ty,
        statements: &mut String,
    ) -> Result<(), Error> {
        let (element, length) = match ty {
            Ty::Array {
                element,
                kind: sequence::Kind::Array(length),
            } => (element, *length),
            Ty::Array { .. } => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a refutable pattern in a binding: a slice pattern on a slice \
                                of unknown length; use `if let` or `match`"
                        .into(),
                    source: pattern,
                })
            }
            other => {
                return Err(Error::TypeMismatch {
                    expected: other.name(&self.models),
                    actual: "an array or slice".into(),
                    expression: pattern,
                })
            }
        };
        let fixed = before.len() + after.len();
        if rest.is_none() && fixed != length || fixed > length {
            let least = if rest.is_some() { "at least " } else { "" };
            return Err(Error::TypeMismatch {
                expected: format!("[_; {length}]").into(),
                actual: format!(
                    "a pattern of {fixed} elements (pattern requires {least}{fixed} elements \
                     but array has {length})"
                )
                .into(),
                expression: pattern,
            });
        }
        let trailing = length - after.len();
        let members = before.iter().enumerate().chain(
            after
                .iter()
                .enumerate()
                .map(|(index, member)| (trailing + index, member)),
        );
        for (index, member) in members {
            let step = self.trace_enter(borrowck::Projection::ConstantIndex {
                offset: index as u32,
                min_length: length as u32,
                from_end: false,
            });
            self.destructure_into(member, &format!("{path}[{index}]"), element, statements)?;
            self.trace_leave(step);
        }
        if let Some(pattern::Syntax::Rest {
            binding: Some((name, span)),
            ..
        }) = rest
        {
            // A subslice is not a place these facts name.
            self.trace_unmodeled("a slice pattern binding its middle", *span);
            let middle = Ty::Array {
                element: element.clone(),
                kind: sequence::Kind::Array(length - fixed),
            };
            let binding = pattern::Syntax::Binding {
                name: name.clone(),
                span: *span,
                mutable: false,
            };
            let helper = self.push_op(Op::Prelude(PreludeOp::Slice));
            let path = format!("{helper}({path}, {}, {}, false)", before.len(), trailing);
            let traced = self.begin_trace();
            let destructured = self.destructure_into(&binding, &path, &middle, statements);
            self.end_trace(traced);
            destructured?;
        }
        Ok(())
    }

    /// `ref name` or `ref mut name`, when the next tokens are one.
    pub(super) fn parse_ref_binding(&mut self) -> Result<Option<pattern::Syntax>, Error> {
        if !(matches!(self.peek(), Token::Identifier(ref name) if name.as_str() == "ref")
            && matches!(self.peek_second(), Token::Identifier(_) | Token::Mut))
        {
            return Ok(None);
        }
        self.next();
        let mutable = matches!(self.peek(), Token::Mut);
        if mutable {
            self.next();
        }
        let (name, span) = self.expect_name("a `ref` binding")?;
        Ok(Some(pattern::Syntax::Ref {
            name,
            span,
            mutable,
        }))
    }
}

/// The `let x: T;` of each name `pattern` binds, a tuple pattern of names
/// (`mut` or not) and `_` with no value, each with its part of `ty`, the
/// written type, pushed onto `lets`. `None` for any other pattern, or a
/// type that is not a tuple of as many parts.
fn deferred_lets(
    pattern: &[(Token, Position)],
    ty: Option<&[(Token, Position)]>,
    lets: &mut Vec<(Token, Position)>,
) -> Option<()> {
    let name = match pattern {
        [(Token::Underscore, _)] => return Some(()),
        [(Token::Identifier(name), at)] => Some((false, name, *at)),
        [(Token::Mut, _), (Token::Identifier(name), at)] => Some((true, name, *at)),
        _ => None,
    };
    if let Some((mutable, name, at)) = name {
        lets.push((Token::Let, at));
        if mutable {
            lets.push((Token::Mut, at));
        }
        lets.push((Token::Identifier(name.clone()), at));
        if let Some(ty) = ty {
            lets.push((Token::Colon, at));
            lets.extend(ty.iter().cloned());
        }
        lets.push((Token::SemiColon, at));
        return Some(());
    }
    let [(Token::LeftParen, _), inside @ .., (Token::RightParen, _)] = pattern else {
        return None;
    };
    let parts = top_level_parts(inside);
    let types = match ty {
        Some([(Token::LeftParen, _), inside @ .., (Token::RightParen, _)]) => {
            let types = top_level_parts(inside);
            if types.len() != parts.len() {
                return None;
            }
            types.into_iter().map(Some).collect()
        }
        Some(_) => return None,
        None => vec![None; parts.len()],
    };
    for (part, ty) in parts.into_iter().zip(types) {
        deferred_lets(part, ty, lets)?;
    }
    Some(())
}

/// `tokens`, the inside of a `(..)`, split at its top-level commas, with
/// no empty part after a trailing comma.
fn top_level_parts(tokens: &[(Token, Position)]) -> Vec<&[(Token, Position)]> {
    let mut parts = Vec::new();
    let mut depth = 0_i32;
    let mut from = 0;
    for (index, (token, _)) in tokens.iter().enumerate() {
        match token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::LessThan => {
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace | Token::GreaterThan => {
                depth -= 1;
            }
            Token::RightShift => depth -= 2,
            Token::Comma if depth == 0 => {
                parts.push(&tokens[from..index]);
                from = index + 1;
            }
            _ => {}
        }
    }
    if from < tokens.len() {
        parts.push(&tokens[from..]);
    }
    parts
}
