//! Branches: `match`, `if`, `if let` and let chains, and `matches!`, over
//! script, scalar and facade values.

use super::*;

impl Parser<'_> {
    /// The program's enum or struct `ty` is, behind any references, where
    /// this file declares it, with the declarations of an enum's variants
    /// `missed` names, as rustc's E0004 notes them.
    pub(super) fn uncovered_type<'m>(
        &self,
        ty: &Ty,
        missed: impl Iterator<Item = &'m str>,
    ) -> Option<Box<crate::resolve::Uncovered>> {
        let mut ty = self.inference.resolve(ty);
        while let Ty::Reference { target, .. } = ty {
            ty = self.inference.resolve(&target);
        }
        let Ty::Script(id, _) = ty else {
            return None;
        };
        let model = self.models.definition(id);
        let name = self.models.name_declared_here(model)?;
        let missed: Vec<&str> = missed.collect();
        let variants = match model {
            Nominal::Enum { item, .. } => item
                .variants
                .iter()
                .filter(|variant| missed.contains(&variant.name.text.as_str()))
                .map(|variant| variant.name.span)
                .collect(),
            Nominal::Struct { .. } => Box::default(),
        };
        Some(Box::new(crate::resolve::Uncovered { name, variants }))
    }
}

impl<'a> Parser<'a> {
    /// `matches!(value, pattern)` or `matches!(value, pattern if guard)`:
    /// Rust's `match value { pattern if guard => true, _ => false }`.
    pub(super) fn parse_matches(&mut self, name_span: Span) -> Result<ParsedExpr, Error> {
        let (tokens, end) = self.take_argument_tokens()?;
        let mut depth = 0usize;
        let comma = tokens.iter().position(|(token, _)| {
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                }
                Token::Comma => return depth == 0,
                _ => {}
            }
            false
        });
        let Some(comma) = comma.filter(|&comma| comma > 0 && comma + 1 < tokens.len()) else {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "`matches!` without a value and a pattern".into(),
                source: span(name_span.start(), end),
            });
        };
        let mut tokens = tokens.into_iter();
        let mut spliced = vec![(Token::LeftParen, name_span.start())];
        spliced.extend(tokens.by_ref().take(comma));
        let (_, comma) = tokens.next().expect("the comma was found");
        spliced.push((Token::RightParen, comma));
        spliced.push((Token::LeftBrace, comma));
        spliced.extend(tokens);
        spliced.extend([
            (Token::DoubleArrow, end),
            (Token::True, end),
            (Token::Comma, end),
            (Token::Underscore, end),
            (Token::DoubleArrow, end),
            (Token::False, end),
            (Token::RightBrace, end),
        ]);
        self.splice_tokens(spliced);
        self.parse_match(name_span)
    }

    /// What a pattern's binding or discard checks its part against: the
    /// part's type as inference has it at the pattern. A type inference
    /// settles only later (`let Ok(n) = s.parse()`, whose target a later
    /// use gives), like a generic payload, takes any value.
    pub(super) fn payload_type(&self, expected: &Ty) -> NominalPayloadType {
        match self.inference.resolve(expected) {
            Ty::Unit => NominalPayloadType::Unit,
            Ty::Param(..) | Ty::Infer(_) => NominalPayloadType::Any,
            // A stored `&mut` is the reference to its place (`stored.rs`).
            Ty::Reference { mutable: true, .. } => NominalPayloadType::Any,
            Ty::Script(definition, ..) => {
                let definition = self.models.definition(definition).definition();
                NominalPayloadType::Script {
                    identity: type_def::Identity::Script(definition.key().clone()),
                    schema: definition.schema().clone(),
                }
            }
            // An imported Rust value is a handle the generated runner names:
            // the pattern takes any value, and the bridge checks the handle
            // where it is used.
            other => other
                .native_type_id(&self.models)
                .map_or(NominalPayloadType::Any, NominalPayloadType::Host),
        }
    }

    /// The name of a matched value's type `ty` as rustc gives it in E0004,
    /// once inference is done: an integer or a float still open is its
    /// fallback, `Option<i32>`. The fallback isn't kept.
    pub(super) fn matched_type(&mut self, ty: &Ty) -> ImmutableString {
        let checkpoint = self.inference.checkpoint();
        let mut ty = self.inference.resolve(ty);
        loop {
            let open = std::cell::RefCell::new(None);
            contains(&ty, &|inner| {
                let fallback = literal_fallback(inner);
                let found = fallback.is_some();
                if let Some(fallback) = fallback {
                    open.replace(Some((inner.clone(), fallback)));
                }
                found
            });
            let Some((open, fallback)) = open.into_inner() else {
                break;
            };
            if !self.unify_type(&fallback, &open) {
                break;
            }
            ty = self.inference.resolve(&ty);
        }
        let name = ty.name(&self.models);
        self.inference.rollback(checkpoint);
        name
    }

    /// A match's `arms` as written, and the match after its `scrutinee`
    /// through the `}` at `close`, where rustc's E0004 suggests an arm.
    pub(super) fn written_arms(
        &self,
        arms: Vec<Span>,
        scrutinee: Span,
        mut close: Position,
    ) -> Box<crate::resolve::Arms> {
        close.advance();
        Box::new(crate::resolve::Arms {
            arms: arms.into(),
            after_scrutinee: span(scrutinee.end(), close),
        })
    }

    /// Where rustc points at a match's scrutinee: the whole of a local's
    /// name, which its use's span only starts.
    pub(super) fn scrutinee_span(&self, subject: &ParsedExpr) -> Span {
        match &subject.lvalue {
            Some(Lvalue::Binding { binding, .. }) => self.name_span(subject.span, binding),
            _ => subject.span,
        }
    }

    /// The `&` and `&mut ` of each reference `ty` is, outermost first: a
    /// witness of the value below them reads through them, as `&None`.
    pub(super) fn reference_prefix(&self, ty: &Ty) -> String {
        let mut prefix = String::new();
        let mut ty = self.inference.resolve(ty);
        while let Ty::Reference { mutable, target } = ty {
            prefix.push_str(if mutable { "&mut " } else { "&" });
            ty = self.inference.resolve(&target);
        }
        prefix
    }

    /// The value below every `&` of a pattern's subject, which its
    /// patterns match: `&&T` matches as `T` does, as rustc's default binding
    /// mode reads it.
    pub(super) fn referenced(&self, ty: &Ty) -> Ty {
        let mut ty = self.inference.resolve(ty);
        while let Ty::Reference { target, .. } = ty {
            ty = self.inference.resolve(&target);
        }
        ty
    }

    /// The subject of a pattern switch: a nominal enum is validated first.
    pub(super) fn pattern_subject(
        &mut self,
        subject: ParsedExpr,
        subject_ty: &Ty,
    ) -> Result<origin::Fragment, Error> {
        let subject_span = subject.span;
        let mut source = match subject_ty {
            Ty::Option(_)
            | Ty::Result(..)
            | Ty::Cow(_)
            | Ty::Tuple(_)
            | Ty::Array { .. }
            | Ty::Int(_)
            | Ty::Char
            | Ty::String
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Float(_)
            | Ty::Ordering
            | Ty::Host { .. }
            // A type with no values, whose match has no arms.
            | Ty::Never
            | Ty::ParseError(ParseError::Infallible) => subject.rendered_source(),
            Ty::Script(definition_id, ..) => {
                let Nominal::Enum { definition, item } =
                    self.models.definition(*definition_id).clone()
                else {
                    // A struct is matched by `_`, a binding or its unit value.
                    return Ok(subject.rendered_source());
                };
                let helper = self.push_op(Op::MatchSubject {
                    definition,
                    item: Box::new(item),
                });
                origin::Fragment::call(&helper, [subject.rendered_source()])
            }
            _ => {
                return Err(Error::InvalidMatch {
                    owner: subject_ty.name(&self.models),
                    reason: "the scrutinee must have a statically known nominal enum type".into(),
                    pattern: subject_span,
                })
            }
        };
        source.cover(subject_span);
        Ok(source)
    }

    pub(super) fn parse_match(&mut self, match_span: Span) -> Result<ParsedExpr, Error> {
        let dereferenced = matches!(self.peek(), Token::Multiply);
        let framed = self.frames.last().map_or(0, Vec::len);
        let (mut subject, matched) = self.parse_subject()?;
        // `match v.last_mut() { Some(x) => .., whole => .. }`: an arm binding
        // the whole `Option<&mut T>` holds it as `let o = v.last_mut();`
        // does, `Some` of the reference to the element's place (D1113).
        if subject.lvalue.is_some()
            && !self.holds_stored(&subject)
            && matches!(self.inference.resolve(&subject.ty), Ty::Option(element)
                if matches!(self.inference.resolve(&element), Ty::Reference { mutable: true, .. }))
            && self.whole_binding_ahead()
        {
            self.store_borrow(&mut subject)?;
        }
        // Matching through `&T` matches the referenced value.
        let subject_ty = self.referenced(&subject.ty);
        if matches!(
            subject_ty,
            Ty::Int(_) | Ty::Char | Ty::String | Ty::Str | Ty::Bool | Ty::Float(_)
        ) {
            return self.parse_literal_match(subject, matched, subject_ty, match_span);
        }
        if let Some(variants) = self
            .host_variants(&subject_ty)
            .or_else(|| self.host_value_variants(&subject_ty))
        {
            return self
                .parse_host_match(subject, matched, subject_ty, variants, match_span, framed);
        }
        let subject_span = subject.span;
        let scrutinee_span = self.scrutinee_span(&subject);
        // A catch-all binding arm binds the scrutinee as written, `&T` included.
        let binding_ty = self.inference.resolve(&subject.ty);
        let scrutinee = self.temporary("match");
        // Kept for the arms whose `&mut` bindings reach its parts.
        let place = subject.clone();
        // The match is on the place where it is when an arm binds a part of
        // it by `&mut` (`PlacePattern`).
        let mut matched_place: Option<PlacePattern> = None;
        // The match's own scope holds a value made for it.
        self.open_drops();
        let holder = self.subject_holder(&place, &scrutinee, dereferenced)?;
        let subject = self.pattern_subject(subject, &subject_ty)?;
        self.expect(Token::LeftBrace, "'{' to start a match")?;
        let split = self.flow().split();
        // Each arm's tests follow the tests of the arms before it.
        let mut tested = split;
        let mut arm_ends = Vec::new();
        let mut arms = Vec::new();
        let mut written_arms = Vec::new();
        // The value as matched, its references included: a referenced
        // value always has values (`specialize_coverage_types`).
        let coverage_types = vec![binding_ty.clone()];
        let mut coverage_rows = Vec::<Vec<coverage::Pattern>>::new();
        // The unguarded arms' patterns, which name what a facade part's
        // tests miss where their coverage can't (`host_rows_witnesses`).
        let mut unguarded = Vec::new();
        let mut wildcard = false;
        let original_bindings = self.bindings.clone();
        let mut arm_bindings = Vec::new();
        // Copies of the outer locals the arms' bindings shadow, and moves
        // out of a shadowed subject, after the match.
        let mut match_copies = String::new();
        let mut match_after = String::new();

        while !matches!(self.peek(), Token::RightBrace | Token::EOF) {
            if self.arm_left_out()? {
                continue;
            }
            // An arm no value reaches, after `_` or under arms that cover
            // its pattern, is checked as any arm is and never runs: rustc
            // warns of it (`unreachable_patterns`) and builds no path to
            // it, so it adds nothing to the match.
            let mut unreachable = wildcard;
            let (copies, after) = (match_copies.len(), match_after.len());
            let placed = matched_place.is_some();

            let arm_start = self.position();
            self.flow().arm(tested);
            tested = self.flow().split();
            // The bindings a facade pattern in the arm makes, ahead of it.
            let mut arm_prefix = origin::Fragment::default();
            // The arm's bindings drop at its end; what they move out of the
            // subject is left `()` there first.
            self.open_drops();
            let mut clears = String::new();
            self.splice_std_alias();
            // `_`, or a binding `name` that matches anything and binds it,
            // as in Rust.
            let catch_all = match self.peek() {
                Token::Underscore => {
                    self.next();
                    self.flow().arm(tested);
                    Some(None)
                }
                Token::Identifier(name)
                    if Prelude::from_name(&name).is_none()
                        && self.unit_struct(&name).is_none()
                        && name.as_str() != "ref"
                        && !matches!(
                            self.peek_second(),
                            Token::DoubleColon
                                | Token::LeftParen
                                | Token::LeftBrace
                                | Token::Pipe
                                | Token::Bang
                        )
                        && !matches!(self.peek_second(), Token::Reserved(ref text) if text.as_str() == "@") =>
                {
                    let (name, declaration) = self.expect_name("match binding")?;
                    let trace = self.binding_trace(&name, declaration, &binding_ty);
                    self.flow().arm(tested);
                    self.pattern_binds(trace, &matched, subject_span, &[])?;
                    match_copies.push_str(&self.shadow_copies(std::slice::from_ref(&name))?);
                    // The binding takes the subject by value.
                    self.move_local(&mut place.clone())?;
                    self.bindings.insert(
                        name.clone(),
                        Binding {
                            ty: binding_ty.clone(),
                            mutable: false,
                            available: true,
                            declaration,
                            kind: BindingKind::Local,
                        },
                    );
                    clears = self.pattern_drops(
                        None,
                        &binding_ty,
                        &holder,
                        &[(name.clone(), declaration, binding_ty.clone())],
                        drops::Clears::InArm,
                    )?;
                    let (deferred, after) = self.take_deferred_clears();
                    match_copies.push_str(&deferred);
                    match_after.push_str(&after);
                    // A `&mut` binding of the whole place is the place.
                    let whole = PatternBinding {
                        name: name.clone(),
                        span: declaration,
                        ty: binding_ty.clone(),
                        mutable: false,
                    };
                    let mut proof = NominalPattern::Binding {
                        name: name.clone(),
                        expected: NominalPayloadType::Any,
                    };
                    match self.pattern_place(&place, &mut proof, &[whole])? {
                        Some(whole) => {
                            self.bind_whole(&whole, name, declaration, &scrutinee);
                            matched_place.get_or_insert(whole);
                            Some(None)
                        }
                        None => Some(Some(name)),
                    }
                }
                _ => None,
            };
            let (label, guard, bound) = if let Some(bound) = catch_all {
                if matches!(self.peek(), Token::If) {
                    self.next();
                    let normalize_private_calls =
                        std::mem::replace(&mut self.normalize_private_calls, false);
                    self.open_temporaries();
                    self.flow().open_temporaries();
                    let guard = self.parse_expr();
                    let end = ScopeEnd::Statement(self.last_read());
                    self.flow().close_temporaries(end);
                    let temporaries = self.close_temporaries();
                    self.normalize_private_calls = normalize_private_calls;
                    let mut guard = guard?;
                    if !temporaries.is_empty() {
                        guard.source = self
                            .with_temporaries(&temporaries, guard.rendered_source())
                            .surrounded("(", ")");
                        guard.prelude = origin::Fragment::default();
                    }
                    self.require_type(&Ty::Bool, &guard.ty, guard.span)?;
                    ("_".to_string(), Some(guard.rendered_source()), bound)
                } else {
                    wildcard = true;
                    ("_".to_string(), None, bound)
                }
            } else {
                let syntax = self.parse_pattern_syntax()?;
                if let pattern::Syntax::Ref {
                    span,
                    mutable: true,
                    ..
                } = syntax
                    && (self.holds_stored(&place) || self.stored_temporary(&place))
                {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "a `ref mut` binding of a whole value that holds `&mut`s".into(),
                        source: span,
                    });
                }
                let mut resolved =
                    self.resolve_root_pattern(&syntax, &binding_ty)
                        .map_err(|error| {
                            syntax.declared(error.matched(|| self.subject_origin(&place)), true)
                        })?;
                let trace = std::mem::take(&mut resolved.trace);
                self.pattern_tests(&trace, &matched, subject_span);
                self.flow().arm(tested);
                self.pattern_binds(trace, &matched, subject_span, &resolved.bindings)?;
                match_copies.push_str(&self.shadow_copies(&drops::pattern_names(&syntax))?);
                let host = self.take_host_checks()?;
                let alternatives = resolved.coverage.alternatives();
                if alternatives.iter().all(|alternative| {
                    self.coverage_pattern_is_covered(
                        &coverage_types,
                        &coverage_rows,
                        std::slice::from_ref(alternative),
                    )
                }) {
                    unreachable = true;
                }

                let arm_place =
                    self.pattern_place(&place, &mut resolved.proof, &resolved.bindings)?;
                // A pattern that binds a part by value, whose type is not
                // `Copy`, moves the subject in this arm, as in Rust.
                if resolved
                    .bindings
                    .iter()
                    .any(|binding| self.moves_by_value(&binding.ty))
                {
                    self.move_local(&mut place.clone())?;
                }
                for binding in &resolved.bindings {
                    self.bindings.insert(
                        binding.name.clone(),
                        Binding {
                            ty: binding.ty.clone(),
                            mutable: binding.mutable,
                            available: true,
                            declaration: binding.span,
                            kind: BindingKind::Local,
                        },
                    );
                }
                if let Some(arm_place) = &arm_place {
                    self.bind_parts(arm_place);
                }
                let stored = self.holds_stored(&place) || self.stored_temporary(&place);
                self.bind_stored(stored, &resolved.bindings);
                let bound = resolved
                    .bindings
                    .iter()
                    .map(|binding| (binding.name.clone(), binding.span, binding.ty.clone()))
                    .collect::<Vec<_>>();
                clears = self.pattern_drops(
                    Some(&syntax),
                    &binding_ty,
                    &holder,
                    &bound,
                    drops::Clears::InArm,
                )?;
                let (deferred, after) = self.take_deferred_clears();
                match_copies.push_str(&deferred);
                match_after.push_str(&after);

                self.bind_host_pattern(&host);
                let host_binds = host.rendered_binds();
                let guard = if matches!(self.peek(), Token::If) {
                    self.next();
                    let normalize_private_calls =
                        std::mem::replace(&mut self.normalize_private_calls, false);
                    self.open_temporaries();
                    self.flow().open_temporaries();
                    let guard = self.parse_expr();
                    let end = ScopeEnd::Statement(self.last_read());
                    self.flow().close_temporaries(end);
                    let temporaries = self.close_temporaries();
                    self.normalize_private_calls = normalize_private_calls;
                    let mut guard = guard?;
                    if !temporaries.is_empty() {
                        guard.source = self
                            .with_temporaries(&temporaries, guard.rendered_source())
                            .surrounded("(", ")");
                        guard.prelude = origin::Fragment::default();
                    }
                    self.require_type(&Ty::Bool, &guard.ty, guard.span)?;
                    Some(guard.rendered_source())
                } else {
                    None
                };
                // The arm's coverage holds what its crate parts' tests cover
                // (`host_coverage`), so only a written guard leaves it out.
                let guarded = guard.is_some();
                // Facade tests guard the arm; its bindings read the value.
                let guard = match (host.condition(), guard) {
                    (None, None) => None,
                    (None, Some(guard)) if host.binds.is_empty() => Some(guard),
                    (None, Some(guard)) => {
                        let mut test = origin::Fragment::plain("({ ");
                        test.push(host.rendered_guard_binds());
                        test.push(guard);
                        test.push_text(" })");
                        Some(test)
                    }
                    (Some(condition), None) => Some(condition),
                    (Some(condition), Some(guard)) => {
                        let mut test = condition.surrounded("(", ") && ({ ");
                        test.push(host.rendered_guard_binds());
                        test.push(guard);
                        test.push_text(" })");
                        Some(test)
                    }
                };
                arm_prefix = host_binds;
                if !guarded {
                    coverage_rows.extend(
                        alternatives
                            .into_iter()
                            .map(|alternative| vec![alternative]),
                    );
                    unguarded.push(syntax);
                }

                let label =
                    self.push_root_pattern(resolved.proof, &resolved.bindings, arm_place.as_ref());
                if matched_place.is_none() {
                    matched_place = arm_place;
                }
                (label, guard, None)
            };

            self.expect(Token::DoubleArrow, "'=>' after a match pattern")?;
            // A block arm needs no ',' after it, as in Rust.
            let block_arm = matches!(self.peek(), Token::LeftBrace);
            let action = self.parse_match_action();
            written_arms.push(span(arm_start, self.written_end()));
            if !unreachable {
                arm_ends.push(self.flow().arm_end());
            }
            let mut action = action?;
            let arm_drops = self.close_drops()?;
            if !clears.is_empty() || !arm_drops.is_empty() {
                let mut block = origin::Fragment::plain(format!("{{ {clears}"));
                block.push(self.then_drops(action.value.rendered_source(), &arm_drops));
                block.push_text(" }");
                action.value.source = block;
                action.value.prelude = origin::Fragment::default();
            }
            if !arm_prefix.is_empty() {
                let mut block = origin::Fragment::plain("{ ");
                block.push(arm_prefix);
                block.push(action.value.rendered_source());
                block.push_text(" }");
                action.value.source = block;
                action.value.prelude = origin::Fragment::default();
            }
            if unreachable {
                match_copies.truncate(copies);
                match_after.truncate(after);
                if !placed {
                    matched_place = None;
                }
            } else {
                arm_bindings.push(self.bindings.clone());
                arms.push((label, guard, action, bound));
            }
            self.bindings = original_bindings.clone();

            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBrace) && !block_arm {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "match arms require ','".into(),
                    source: span(arm_start, self.position()),
                });
            }
        }

        let end = self.expect(Token::RightBrace, "'}' to close a match")?;
        self.flow().join(&arm_ends);
        let arm_spans: Vec<(Span, borrowck::Block)> = arms
            .iter()
            .map(|(_, _, action, _)| action.value.span)
            .zip(arm_ends.iter().copied())
            .collect();
        self.flow()
            .either(span(match_span.start(), end), &arm_spans);
        let valued = arms
            .iter()
            .map(|(_, _, action, _)| (action.value.span, &action.value.ty))
            .collect::<Vec<_>>();
        self.note_fn_branches(span(match_span.start(), end), &valued);
        if !wildcard {
            let witnesses = self.coverage_witnesses(&coverage_types, &coverage_rows, true);
            let mut sealed = Vec::new();
            if let Some(first) = witnesses.first().and_then(|witness| witness.first()) {
                first.sealed(&mut sealed);
            }
            let prefix = self.reference_prefix(&binding_ty);
            let mut rendered = witnesses
                .iter()
                .map(|witness| {
                    let witness = witness
                        .first()
                        .map_or_else(|| "_".to_string(), coverage::Witness::render);
                    format!("{prefix}{witness}")
                })
                .collect::<Vec<_>>();
            // A facade part tested at run time covers no whole value of its
            // own (`host_coverage`): the arms' patterns as written tell what
            // they miss, so `S { a: Some(_) }` and `S { a: None }` cover
            // every value together.
            if !rendered.is_empty()
                && coverage_rows
                    .iter()
                    .flatten()
                    .any(coverage::Pattern::tested)
                && let Some(written) = self.host_rows_witnesses(
                    &unguarded.iter().collect::<Vec<_>>(),
                    &binding_ty,
                    match_span,
                )?
            {
                rendered = written;
            }
            if !rendered.is_empty() {
                // A match with no arms on a type that isn't an enum names
                // no witness: rustc calls the type non-empty.
                let non_empty = written_arms.is_empty() && !self.enumerated(&binding_ty);
                // The variant each witness names, as `B` of `&E::B(_)`.
                let missed = rendered
                    .iter()
                    .filter(|_| !non_empty)
                    .filter_map(|witness| {
                        let witness = witness.trim_start_matches(['&', ' ']);
                        witness.trim_start_matches("mut ").split(['(', ' ']).next()
                    })
                    .filter_map(|head| head.rsplit("::").next());
                let uncovered = self.uncovered_type(&binding_ty, missed);
                let witnesses = if non_empty {
                    Box::default()
                } else {
                    rendered.into_iter().map(Into::into).collect()
                };
                let uninhabited_target = match self.inference.resolve(&binding_ty) {
                    Ty::Reference { target, .. } => self.uninhabited(&target),
                    _ => false,
                };
                return Err(Error::NonExhaustive {
                    witnesses,
                    ty: self.matched_type(&binding_ty),
                    scrutinee: scrutinee_span,
                    arms: self.written_arms(written_arms, scrutinee_span, end),
                    uncovered,
                    sealed: sealed.into(),
                    uninhabited_target,
                });
            }
        }
        self.bindings = original_bindings;
        let merged = arm_bindings
            .iter()
            .zip(&arms)
            .map(|(bindings, (_, _, action, _))| (bindings, &action.value.ty))
            .collect::<Vec<_>>();
        self.merge_branches(&merged);

        let output_type = self.coerce_branches(
            &mut arms
                .iter_mut()
                .map(|(_, _, action, _)| &mut action.value)
                .collect::<Vec<_>>(),
        )?;
        // `_ if guard => a, rest..` is `_ => if guard { a } else { match rest }`
        // on the same subject, which is evaluated once. A catch-all binding
        // `name` is `_` with `let name = subject;` ahead of its guard and arm.
        let match_drops = match_after + self.close_drops()?.as_str();
        let matched = if match_drops.is_empty() {
            String::new()
        } else {
            format!("let {} = ", self.temporary("matched"))
        };
        let (subject, operand) = match &matched_place {
            Some(place) => {
                // A binding of `Some(&mut place)` itself would be a copy.
                if place.optional() && arms.iter().any(|(.., bound)| bound.is_some()) {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: crate::resolve::Unsupported::said(
                            "a binding of a whole `Option<&mut T>` beside `&mut` \
                                    bindings of its value: not supported in Rawr yet",
                        ),
                        source: span(match_span.start(), end),
                    });
                }
                (place.locate.clone(), place.operand(&scrutinee))
            }
            None => (subject, scrutinee.clone()),
        };
        let mut source = origin::Fragment::plain(format!("{{\nlet {scrutinee} = "));
        source.push(subject);
        source.push_text(format!(
            ";\n{match_copies}{matched}__rawr_switch {operand} {{\n"
        ));
        let mut closers = Vec::new();
        for (label, guard, action, bound) in arms {
            let bind = bound.map(|name| format!("{{ let {name} = {operand}; "));
            if label == "_"
                && let Some(guard) = guard
            {
                source.push_text("_ => ");
                source.push_text(bind.as_deref().unwrap_or_default());
                source.push_text("if ");
                source.push(guard);
                source.push_text(" { ");
                source.push(action.value.rendered_source());
                source.push_text(format!(" }} else {{ __rawr_switch {operand} {{\n"));
                closers.push(if bind.is_some() { "} } }\n" } else { "} }\n" });
                continue;
            }
            source.push_text(label);
            if let Some(guard) = guard {
                source.push_text(" if ");
                source.push(guard);
            }
            source.push_text(" => ");
            match bind {
                Some(bind) => {
                    source.push_text(bind);
                    source.push(action.value.rendered_source());
                    source.push_text(" }");
                }
                None => source.push(action.value.rendered_source()),
            }
            source.push_text(",\n");
        }
        source.push_text("}\n");
        for closer in closers.into_iter().rev() {
            source.push_text(closer);
        }
        // The value made for the match drops after it.
        if let Some(matched) = matched
            .strip_prefix("let ")
            .and_then(|matched| matched.strip_suffix(" = "))
        {
            source.push_text(format!(";\n{match_drops}{matched}\n"));
        }
        source.push_text("}");
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: output_type,
            span: span(match_span.start(), end),
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// `let pattern = subject` heading an `if let` or `while let`: the
    /// value `scrutinee` takes, the arm label, its bindings, and what the
    /// switch is on, with the pattern's bindings in scope. A pattern matched
    /// on a `&mut` place matches it where it is (`PlacePattern`).
    pub(super) fn parse_let_condition(
        &mut self,
        scrutinee: &str,
        temporaries: drops::Subject,
    ) -> Result<
        (
            origin::Fragment,
            origin::Fragment,
            origin::Fragment,
            String,
            drops::Condition,
        ),
        Error,
    > {
        self.expect(Token::Let, "`let` in a condition")?;
        let syntax = self.parse_pattern_syntax()?;
        self.expect(Token::Equals, "'=' after a `let` pattern")?;
        let dereferenced = matches!(self.peek(), Token::Multiply);
        let scoped = temporaries == drops::Subject::Scoped;
        if scoped {
            self.open_temporaries();
        }
        let parsed = self.parse_subject();
        let made = if scoped {
            self.close_temporaries()
        } else {
            Vec::new()
        };
        let (subject, matched) = parsed?;
        // Matching through `&T` binds references, Rust's default binding mode.
        let written_ty = self.inference.resolve(&subject.ty);
        let subject_ty = self.referenced(&written_ty);
        let mut resolved = self
            .resolve_root_pattern(&syntax, &written_ty)
            .map_err(|error| error.matched(|| self.subject_origin(&subject)))?;
        let trace = std::mem::take(&mut resolved.trace);
        self.pattern_facts(trace, &matched, subject.span, &resolved.bindings)?;
        let stored = self.holds_stored(&subject) || self.stored_temporary(&subject);
        let place = self.pattern_place(&subject, &mut resolved.proof, &resolved.bindings)?;
        // The condition's scope: the value made for it, then its bindings,
        // which the caller closes after the consequent or body.
        self.open_drops();
        let temporaries = self.scope_temporaries(made)?;
        let holder = self.subject_holder(&subject, scrutinee, dereferenced)?;
        let mut otherwise = match &holder {
            drops::Holder::Local(place, _)
                if place == scrutinee && self.needs_drop(&subject.ty, subject.span)? =>
            {
                drops::glue_call(scrutinee, &self.drop_shape(&subject.ty))
            }
            _ => String::new(),
        };
        otherwise.push_str(&self.temporary_drops(&temporaries));
        let (subject, operand) = match &place {
            Some(place) => (place.locate.clone(), place.operand(scrutinee)),
            None => (
                self.pattern_subject(subject, &subject_ty)?,
                scrutinee.to_string(),
            ),
        };
        let host = self.take_host_checks()?;
        for binding in &resolved.bindings {
            self.bindings.insert(
                binding.name.clone(),
                Binding {
                    ty: binding.ty.clone(),
                    mutable: binding.mutable,
                    available: true,
                    declaration: binding.span,
                    kind: BindingKind::Local,
                },
            );
        }
        if let Some(place) = &place {
            self.bind_parts(place);
        }
        self.bind_stored(stored, &resolved.bindings);
        let bound = resolved
            .bindings
            .iter()
            .map(|binding| (binding.name.clone(), binding.span, binding.ty.clone()))
            .collect::<Vec<_>>();
        let copies = self.shadow_copies(&drops::pattern_names(&syntax))?;
        let clears = self.pattern_drops(
            Some(&syntax),
            &written_ty,
            &holder,
            &bound,
            drops::Clears::InArm,
        )?;
        let (deferred, after) = self.take_deferred_clears();
        let copies = copies + deferred.as_str();
        self.bind_host_pattern(&host);
        let mut arm = origin::Fragment::plain(self.push_root_pattern(
            resolved.proof,
            &resolved.bindings,
            place.as_ref(),
        ));
        if let Some(condition) = host.condition() {
            arm.push_text(" if ");
            arm.push(condition);
        }
        let mut binds = host.rendered_binds();
        binds.push_text(clears);
        Ok((
            subject,
            arm,
            binds,
            operand,
            drops::Condition {
                otherwise,
                temporaries,
                copies,
                after,
            },
        ))
    }

    /// The subject of a `match`, `if let` or `while let`, which may be a
    /// `&mut` borrow of a place.
    pub(super) fn parse_subject(&mut self) -> Result<(ParsedExpr, Matched), Error> {
        self.subject_deref = matches!(self.peek(), Token::Multiply);
        let borrowing =
            matches!(self.peek(), Token::Ampersand) && matches!(self.peek_second(), Token::Mut);
        let outer = self.mutable_borrow;
        if borrowing {
            self.mutable_borrow = Some(self.expression_stack.len() + 1);
        }
        let start = self.flow().value_start();
        let subject = self.parse_head(Self::parse_expr);
        self.mutable_borrow = outer;
        let subject = subject?;
        // A match reads only what its patterns test and bind.
        let matched = self.matched(&subject, start);
        Ok((subject, matched))
    }

    /// Whether an arm of the match whose `{` is ahead binds the whole value:
    /// `name =>`, `name if ..`, `mut name` or `ref mut name`, of a name no
    /// constant, unit struct or prelude variant takes.
    fn whole_binding_ahead(&self) -> bool {
        let tokens: Vec<Token> = self.input.clone().map(|(token, _)| token).collect();
        if !matches!(tokens.first(), Some(Token::LeftBrace)) {
            return false;
        }
        let mut depth = 0usize;
        let mut arm_start = false;
        for (at, token) in tokens.iter().enumerate() {
            let starts = std::mem::take(&mut arm_start);
            match token {
                Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart => {
                    depth += 1;
                    arm_start = depth == 1;
                    continue;
                }
                Token::RightBrace | Token::RightParen | Token::RightBracket => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return false;
                    }
                    arm_start = depth == 1 && matches!(token, Token::RightBrace);
                    continue;
                }
                Token::Comma if depth == 1 => {
                    arm_start = true;
                    continue;
                }
                _ => {}
            }
            if !starts {
                continue;
            }
            let rest = &tokens[at..];
            let name = match rest {
                [Token::Identifier(word), Token::Mut, Token::Identifier(name), ..]
                    if word.as_str() == "ref" =>
                {
                    Some((name, &rest[3..]))
                }
                [Token::Mut, Token::Identifier(name), ..] => Some((name, &rest[2..])),
                [Token::Identifier(name), ..] => Some((name, &rest[1..])),
                _ => None,
            };
            if let Some((name, [Token::DoubleArrow | Token::If, ..])) = name
                && name.as_str() != "ref"
                && Prelude::from_name(name).is_none()
                && self.unit_struct(name).is_none()
                && !self
                    .models
                    .function(name)
                    .is_some_and(|function| function.definition.body.constant)
            {
                return true;
            }
        }
        false
    }

    pub(super) fn next_after_else(&mut self) -> Token {
        self.next();
        self.peek()
    }

    pub(super) fn temporary(&mut self, role: &str) -> String {
        self.temporaries += 1;
        format!("__rawr_{role}_{}", self.temporaries)
    }

    /// `match` on an `i64`, `f64`, `char`, `&str` or `bool` with literal, range,
    /// `|`, `@` and binding patterns. Arms are tried in order, as in Rust.
    pub(super) fn parse_literal_match(
        &mut self,
        subject: ParsedExpr,
        matched: Matched,
        subject_ty: Ty,
        match_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let written = self.inference.resolve(&subject.ty);
        // A binding arm binds the subject as written, `&T` included, as
        // Rust does. Below a `&mut` it is the value referred to.
        let binding_ty = if self.shared_references(&written) {
            written.clone()
        } else {
            subject_ty.clone()
        };
        let temporary = self.temporary("match");
        self.expect(Token::LeftBrace, "'{' to start a match")?;
        // Each arm's tests follow the tests of the arms before it.
        let mut tested = self.flow().split();
        let mut arm_ends = Vec::new();
        let original_bindings = self.bindings.clone();
        let mut arm_bindings = Vec::new();
        let mut arms = Vec::new();
        let mut written_arms = Vec::new();
        let mut exhaustive = false;
        let (mut saw_true, mut saw_false) = (false, false);
        // The values the unguarded arms of an integer or `char` match
        // cover, `None` once one's pattern isn't read (`Scalar::covered`).
        let scalar = Scalar::of(&subject_ty);
        let mut covered = Some(Vec::new());
        while !matches!(self.peek(), Token::RightBrace | Token::EOF) {
            if self.arm_left_out()? {
                continue;
            }
            let pattern_start = self.position();
            self.flow().arm(tested);
            tested = self.flow().split();
            // An arm after a catch-all is checked as any arm is, and never
            // runs: rustc warns of it (`unreachable_patterns`) and builds
            // no path to it.
            let unreachable = exhaustive;
            let syntax = self.parse_pattern_syntax()?;
            let mut binding = None;
            let test = self
                .scalar_condition(&syntax, &written, &temporary, &mut binding)
                .map_err(|error| {
                    syntax.declared(error.matched(|| self.subject_origin(&subject)), true)
                })?;
            let trace = self.scalar_trace(
                test.is_some(),
                binding
                    .as_ref()
                    .map(|(name, declaration, bound)| (name, *declaration, *bound)),
                &written,
            );
            self.pattern_tests(&trace, &matched, subject.span);
            self.flow().arm(tested);
            self.pattern_binds(trace, &matched, subject.span, &[])?;
            let catch_all = test.is_none();
            let tests = test.into_iter().collect::<Vec<_>>();
            let mut seen = [false; 2];
            bool_literals(&syntax, &mut seen);
            if let Some((name, declaration, Bound::Unique)) = &binding {
                // `ref mut v` is the matched place itself, as `let v = &mut
                // place;` is.
                let (root, place) = match &subject.lvalue {
                    Some(Lvalue::Binding { binding, .. }) => {
                        (binding.clone(), origin::Fragment::plain(binding.as_str()))
                    }
                    Some(Lvalue::Place { binding, place, .. }) => (binding.clone(), place.clone()),
                    _ => {
                        return Err(Error::UnsupportedNominalSyntax {
                            construct: "`ref mut` of a value that is not a local's place".into(),
                            source: subject.span,
                        })
                    }
                };
                self.borrow_place(name.clone(), *declaration, written.clone(), place, root);
            } else if let Some((name, declaration, bound)) = &binding {
                // `ref v` borrows the value as written, a copy at runtime.
                let ty = match bound {
                    Bound::Shared => Ty::Reference {
                        mutable: false,
                        target: Box::new(written.clone()),
                    },
                    Bound::Value | Bound::Mutable | Bound::Unique => binding_ty.clone(),
                };
                self.bindings.insert(
                    name.clone(),
                    Binding {
                        ty,
                        mutable: *bound == Bound::Mutable,
                        available: true,
                        declaration: *declaration,
                        kind: BindingKind::Local,
                    },
                );
            }
            let bind = match &binding {
                Some((name, _, bound)) if *bound != Bound::Unique => {
                    format!("let {name} = {temporary}; ")
                }
                _ => String::new(),
            };
            let mut condition = origin::Fragment::plain(if tests.is_empty() {
                "true".to_string()
            } else {
                tests.join(" || ")
            });
            let guarded = matches!(self.peek(), Token::If);
            if guarded {
                self.next();
                let guard = self.parse_expr()?;
                self.require_type(&Ty::Bool, &guard.ty, guard.span)?;
                let mut test = origin::Fragment::plain(format!(
                    "({condition_text}) && ({{ {bind}",
                    condition_text = condition.as_str()
                ));
                test.push(guard.rendered_source());
                test.push_text(" })");
                condition = test;
            }
            if !guarded {
                saw_false |= seen[0];
                saw_true |= seen[1];
                covered = covered.zip(scalar).and_then(|(mut covered, scalar)| {
                    scalar.covered(&syntax, &mut covered)?;
                    Some(covered)
                });
            }
            exhaustive |= catch_all && !guarded || (saw_true && saw_false);
            self.expect(Token::DoubleArrow, "'=>' after a match pattern")?;
            // A block arm needs no ',' after it, as in Rust.
            let block_arm = matches!(self.peek(), Token::LeftBrace);
            let action = self.parse_match_action()?;
            written_arms.push(span(pattern_start, self.written_end()));
            let bindings = std::mem::replace(&mut self.bindings, original_bindings.clone());
            if !unreachable {
                arm_ends.push(self.flow().arm_end());
                arm_bindings.push(bindings);
                arms.push((condition, bind, action));
            }
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBrace) && !block_arm {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "match arms require ','".into(),
                    source: span(pattern_start, self.position()),
                });
            }
        }
        let end = self.expect(Token::RightBrace, "'}' to close a match")?;
        self.flow().join(&arm_ends);
        let arm_spans: Vec<(Span, borrowck::Block)> = arms
            .iter()
            .map(|(_, _, action)| action.value.span)
            .zip(arm_ends.iter().copied())
            .collect();
        self.flow()
            .either(span(match_span.start(), end), &arm_spans);
        let valued = arms
            .iter()
            .map(|(_, _, action)| (action.value.span, &action.value.ty))
            .collect::<Vec<_>>();
        self.note_fn_branches(span(match_span.start(), end), &valued);
        if !exhaustive {
            // rustc's witnesses: the ranges no arm covers, the `bool` no arm
            // names, `&_` for a `&str`, or `_` (a float's, or an integer's
            // no unguarded arm names).
            let witnesses = match (scalar, &covered) {
                (Some(scalar), Some(covered)) if !covered.is_empty() => scalar
                    .uncovered(covered)
                    .into_iter()
                    .map(|(low, high)| scalar.witness(low, high))
                    .collect(),
                _ if matches!(subject_ty, Ty::Bool) => [(saw_false, "false"), (saw_true, "true")]
                    .into_iter()
                    .filter(|(seen, _)| !seen)
                    .map(|(_, value)| value.to_string())
                    .collect(),
                _ if matches!(subject_ty, Ty::Str) => vec!["&_".to_string()],
                _ => vec!["_".to_string()],
            };
            if !witnesses.is_empty() {
                let prefix = self.reference_prefix(&written);
                // A match with no arms on a type that isn't an enum names
                // no witness: rustc calls the type non-empty.
                let witnesses = if written_arms.is_empty() {
                    Box::default()
                } else {
                    witnesses
                        .iter()
                        .map(|witness| format!("{prefix}{witness}").into())
                        .collect()
                };
                return Err(Error::NonExhaustive {
                    witnesses,
                    ty: self.matched_type(&written),
                    scrutinee: self.scrutinee_span(&subject),
                    arms: self.written_arms(written_arms, self.scrutinee_span(&subject), end),
                    uncovered: None,
                    sealed: Box::default(),
                    uninhabited_target: false,
                });
            }
        }
        // A local is available after the match when each arm that goes on
        // left it available, as a `let x;` each arm assigns.
        let merged = arm_bindings
            .iter()
            .zip(&arms)
            .map(|(bindings, (_, _, action))| (bindings, &action.value.ty))
            .collect::<Vec<_>>();
        self.merge_branches(&merged);
        let output = self.coerce_branches(
            &mut arms
                .iter_mut()
                .map(|(_, _, action)| &mut action.value)
                .collect::<Vec<_>>(),
        )?;
        let mut source = origin::Fragment::plain(format!("{{\nlet {temporary} = "));
        source.push(subject.rendered_source());
        source.push_text(";\n");
        let count = arms.len();
        for (index, (condition, bind, action)) in arms.into_iter().enumerate() {
            if index + 1 == count {
                source.push_text(format!("{{ {bind}"));
            } else {
                source.push_text("if ");
                source.push(condition);
                source.push_text(format!(" {{ {bind}"));
            }
            source.push(action.value.rendered_block());
            source.push_text(if index + 1 == count { " }" } else { " } else " });
        }
        source.push_text("\n}");
        Ok(self.operator_expr(source, output, span(match_span.start(), end)))
    }

    /// A facade enum's variants: the shared, parameterless methods named
    /// for them (`Status::Active`), a `bool` test or, for a tuple variant,
    /// its field when the value is that variant (`Key::Named`); and its
    /// `CamelCase` associated values of itself (`Key::Unidentified`). Its
    /// named fields are shared, parameterless readers (`Status::is_checked`).
    pub(super) fn host_variants(&self, ty: &Ty) -> Option<Vec<ImmutableString>> {
        let Ty::Host { proof, .. } = ty else {
            return None;
        };
        // An enum's variants, in its declaration order: an imported Rust
        // enum's, each matched through the bridge
        // (`imported_variant_pattern`), or a linked enum's, as std's
        // `fmt::Alignment`, each the facade's associated value of its name
        // matched by `==`.
        if let crate::host::type_def::Shape::Enum { variants, .. } = proof.definition.shape() {
            return Some(
                variants
                    .iter()
                    .map(|variant| variant.name().into())
                    .collect(),
            );
        }
        let path = proof.definition.path();
        let own = crate::host::type_ref::Type::registered(path.as_str()).ok();
        let bool_ty = crate::host::type_ref::Type::builtin(crate::host::type_ref::Builtin::Bool);
        let camel = |name: &str| name.starts_with(|c: char| c.is_ascii_uppercase());
        let mut readers = false;
        let mut variants = Vec::<ImmutableString>::new();
        for item in self.models.host.members(&path) {
            let name = item.path_ref().name();
            if !camel(name) {
                continue;
            }
            let reader = item.receiver() == Some(receiver::Mode::Shared)
                && item.signature().parameters().is_empty()
                && (*item.signature().output() == bool_ty
                    || matches!(
                        item.signature().output().shape(),
                        crate::host::type_ref::Shape::Option(_)
                    ));
            let value = item.kind() == crate::host::item::Kind::AssociatedValue
                && own.as_ref() == Some(item.signature().output())
                && name.chars().any(|c| c.is_ascii_lowercase());
            readers |= reader;
            if (reader || value) && !variants.iter().any(|known| known.as_str() == name) {
                variants.push(name.into());
            }
        }
        (readers && !variants.is_empty()).then_some(variants)
    }

    /// A facade enum of unit variants, as iced's `tooltip::Position`: the
    /// type's `CamelCase` associated values of itself, matched by `==`.
    /// Only a type with no associated function making one is closed.
    pub(super) fn host_value_variants(&self, ty: &Ty) -> Option<Vec<ImmutableString>> {
        let Ty::Host { proof, .. } = ty else {
            return None;
        };
        let path = proof.definition.path();
        let own = crate::host::type_ref::Type::registered(path.as_str()).ok()?;
        let mut variants = Vec::new();
        for item in self.models.host.members(&path) {
            if *item.signature().output() != own {
                continue;
            }
            match item.kind() {
                crate::host::item::Kind::AssociatedValue => {
                    let name = item.path_ref().name();
                    if name.starts_with(|c: char| c.is_ascii_uppercase())
                        && name.chars().any(|c| c.is_ascii_lowercase())
                    {
                        variants.push(ImmutableString::from(name));
                    }
                }
                crate::host::item::Kind::AssociatedFunction => return None,
                _ => {}
            }
        }
        let equal = proof
            .definition
            .capabilities()
            .contains(capability::Kind::PartialEq);
        (equal && !variants.is_empty()).then_some(variants)
    }

    /// Read `member` of the facade value `value` of type `ty`.
    pub(super) fn host_read(
        &mut self,
        value: origin::Fragment,
        ty: &Ty,
        member: ImmutableString,
        at: Span,
    ) -> Result<ParsedExpr, Error> {
        let value = self.operator_expr(value, ty.clone(), at);
        self.pending_arguments = Some((Vec::new(), at.end()));
        let outer = std::mem::replace(&mut self.reading_variant, true);
        let read = self.parse_host_method(value, member, at);
        self.reading_variant = outer;
        read
    }

    /// Lower the facade patterns met below the script pattern just resolved:
    /// each hidden name's tests and the bindings its pattern makes.
    pub(super) fn take_host_checks(&mut self) -> Result<pattern::Host, Error> {
        let checks = std::mem::take(&mut *self.host_checks.borrow_mut());
        let mut pattern = pattern::Host::default();
        for (name, syntax, ty, mode) in checks {
            let outer = self.host_binding_mode.replace(mode);
            let lowered = self.host_pattern(
                &syntax,
                origin::Fragment::plain(name.as_str()),
                &ty,
                &mut pattern,
            );
            self.host_binding_mode.set(outer);
            lowered?;
        }
        Ok(pattern)
    }

    /// The crate's unit struct a pattern's path names, of no parameters,
    /// as its type.
    #[cfg(feature = "rust-import")]
    pub(super) fn imported_unit_struct(&mut self, path: &[(ImmutableString, Span)]) -> Option<Ty> {
        if matches!(
            self.peek(),
            Token::LeftParen | Token::Unit | Token::LeftBrace
        ) {
            return None;
        }
        let full = self.host_pattern_owner(path);
        let definition = self
            .models
            .host
            .source_type(&full)
            .filter(|definition| definition.identity().imported().is_some())?;
        let crate::host::type_def::Shape::Struct {
            form: crate::host::type_def::Form::Unit,
            ..
        } = definition.shape()
        else {
            return None;
        };
        if !definition.parameters().is_empty() {
            return None;
        }
        let owner = self.models.host.type_owner(&full).cloned();
        Some(Ty::Host {
            proof: HostTyProof::new(definition.clone(), Some(definition.clone()), owner),
            arguments: Box::default(),
        })
    }

    /// `field` of `value`, a facade struct of type `ty`, as a pattern on it
    /// reads it: an imported struct's by its selector, as `event.split`
    /// reads it.
    pub(super) fn host_struct_field(
        &mut self,
        value: origin::Fragment,
        ty: &Ty,
        field: &ImmutableString,
        at: Span,
    ) -> Result<ParsedExpr, Error> {
        #[cfg(feature = "rust-import")]
        if let Ty::Host { proof, .. } = ty
            && proof.definition.identity().imported().is_some()
            && matches!(
                proof.definition.shape(),
                crate::host::type_def::Shape::Struct { .. }
            )
        {
            let definition = proof.definition.clone();
            let receiver = self.operator_expr(value, ty.clone(), at);
            return self.read_imported_field(receiver, definition, field.clone(), at);
        }
        self.host_read(value, ty, field.clone(), at)
    }

    pub(super) fn require_new_host_binding(
        &self,
        out: &pattern::Host,
        name: &ImmutableString,
        span: Span,
    ) -> Result<(), Error> {
        if out.binds.iter().any(|binding| binding.name == *name) {
            return Err(Error::InvalidMatch {
                owner: name.clone(),
                reason: format!("binding `{name}` appears more than once in the pattern").into(),
                pattern: span,
            });
        }
        Ok(())
    }

    /// `match` on a facade enum: `Status::Hovered`, `Status::Active { is_checked }`,
    /// `Key::Named(key::Named::Tab)`, `A | B`, `_` and bindings, lowered to
    /// the facade's variant tests and field readers.
    pub(super) fn parse_host_match(
        &mut self,
        subject: ParsedExpr,
        matched: Matched,
        subject_ty: Ty,
        variants: Vec<ImmutableString>,
        match_span: Span,
        framed: usize,
    ) -> Result<ParsedExpr, Error> {
        let Ty::Host { proof, .. } = &subject_ty else {
            unreachable!("host matches are selected for host types");
        };
        // A match through `&T` binds references, as rustc's default binding
        // mode does.
        let written = match self.inference.resolve(&subject.ty) {
            Ty::Reference { mutable, .. } => Ty::Reference {
                mutable,
                target: Box::new(subject_ty.clone()),
            },
            _ => subject_ty.clone(),
        };
        let temporary = self.temporary("match");
        self.expect(Token::LeftBrace, "'{' to start a match")?;
        // Each arm's tests follow the tests of the arms before it.
        let mut tested = self.flow().split();
        let mut arm_ends = Vec::new();
        let original_bindings = self.bindings.clone();
        let mut covered = Vec::<ImmutableString>::new();
        // The arms without a guard, whose patterns below a variant cover it
        // together.
        let mut unguarded = Vec::new();
        let mut exhaustive = false;
        let mut arms = Vec::new();
        let mut written_arms = Vec::new();
        while !matches!(self.peek(), Token::RightBrace | Token::EOF) {
            if self.arm_left_out()? {
                continue;
            }
            let pattern_start = self.position();
            self.flow().arm(tested);
            tested = self.flow().split();
            let syntax = self.parse_pattern_syntax()?;
            let mut compiled = pattern::Host::default();
            self.host_pattern(
                &syntax,
                origin::Fragment::plain(temporary.as_str()),
                &written,
                &mut compiled,
            )?;
            let trace = self.host_trace(&compiled, &written, syntax.span());
            self.pattern_tests(&trace, &matched, subject.span);
            self.flow().arm(tested);
            self.pattern_binds(trace, &matched, subject.span, &[])?;
            self.bind_host_pattern(&compiled);
            let binds = compiled.rendered_binds();
            let mut condition = compiled
                .condition()
                .unwrap_or_else(|| origin::Fragment::plain("true"));
            if matches!(self.peek(), Token::If) {
                self.next();
                let guard = self.parse_expr()?;
                self.require_type(&Ty::Bool, &guard.ty, guard.span)?;
                let mut test = condition.surrounded("(", ") && ({ ");
                test.push(compiled.rendered_guard_binds());
                test.push(guard.rendered_source());
                test.push_text(" })");
                condition = test;
            } else if compiled.tests.is_empty() {
                exhaustive = true;
            } else {
                Self::host_covers(&syntax, &mut covered);
                unguarded.push(syntax);
            }
            self.expect(Token::DoubleArrow, "'=>' after a match pattern")?;
            // A block arm needs no ',' after it, as in Rust.
            let block_arm = matches!(self.peek(), Token::LeftBrace);
            let action = self.parse_match_action()?;
            written_arms.push(span(pattern_start, self.written_end()));
            arm_ends.push(self.flow().arm_end());
            self.bindings = original_bindings.clone();
            arms.push((condition, binds, action));
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(self.peek(), Token::RightBrace) && !block_arm {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "match arms require ','".into(),
                    source: span(pattern_start, self.position()),
                });
            }
        }
        let end = self.expect(Token::RightBrace, "'}' to close a match")?;
        self.flow().join(&arm_ends);
        let arm_spans: Vec<(Span, borrowck::Block)> = arms
            .iter()
            .map(|(_, _, action)| action.value.span)
            .zip(arm_ends.iter().copied())
            .collect();
        self.flow()
            .either(span(match_span.start(), end), &arm_spans);
        let valued = arms
            .iter()
            .map(|(_, _, action)| (action.value.span, &action.value.ty))
            .collect::<Vec<_>>();
        self.note_fn_branches(span(match_span.start(), end), &valued);
        // A `#[non_exhaustive]` crate enum has values outside its crate's
        // variants, which only an arm matching anything covers.
        let sealed = matches!(
            &subject_ty,
            Ty::Host { proof, .. } if matches!(
                proof.definition.shape(),
                crate::host::type_def::Shape::Enum { non_exhaustive: true, .. }
            )
        );
        if !exhaustive
            && (sealed
                || !variants.iter().all(|variant| covered.contains(variant))
                    && !self.host_rows_cover(
                        unguarded.iter().map(|syntax| vec![syntax]).collect(),
                        std::slice::from_ref(&subject_ty),
                        match_span,
                    )?)
        {
            // rustc's witnesses: an imported enum's, of the values its
            // patterns as written miss (`host_rows_witnesses`); a linked
            // enum's variants no arm covers, by the type's name; `_` where
            // arms cover each variant only in part, and alone for a
            // `#[non_exhaustive]` enum, whose values outside its variants no
            // arm but a wildcard covers.
            let owner = coverage::host_type_name(&proof.definition, &self.models);
            let prefix = self.reference_prefix(&written);
            let imported = matches!(
                proof.definition.shape(),
                crate::host::type_def::Shape::Enum { .. }
            );
            let mut witnesses = Vec::<ImmutableString>::new();
            if !sealed
                && imported
                && let Some(written) = self.host_rows_witnesses(
                    &unguarded.iter().collect::<Vec<_>>(),
                    &written,
                    match_span,
                )?
            {
                witnesses = written.into_iter().map(Into::into).collect();
            }
            if !sealed && !imported {
                witnesses.extend(
                    variants
                        .iter()
                        .filter(|variant| !covered.contains(variant))
                        .map(|variant| format!("{prefix}{owner}::{variant}").into()),
                );
            }
            if witnesses.is_empty() {
                witnesses.push(format!("{prefix}_").into());
            }
            return Err(Error::NonExhaustive {
                witnesses: witnesses.into(),
                ty: self.matched_type(&written),
                scrutinee: self.scrutinee_span(&subject),
                arms: self.written_arms(written_arms, self.scrutinee_span(&subject), end),
                uncovered: None,
                sealed: sealed
                    .then(|| ImmutableString::from(owner))
                    .into_iter()
                    .collect(),
                uninhabited_target: false,
            });
        }
        let output = self.coerce_branches(
            &mut arms
                .iter_mut()
                .map(|(_, _, action)| &mut action.value)
                .collect::<Vec<_>>(),
        )?;
        let frames = self.subject_frames(framed, &subject, &arms);
        let mut source = origin::Fragment::plain(format!("{{\nlet {temporary} = "));
        source.push(subject.rendered_source());
        source.push_text(";\n");
        let arm = self.temporary("arm");
        let mut deferred = Vec::new();
        let count = arms.len();
        for (index, (condition, binds, action)) in arms.into_iter().enumerate() {
            if index + 1 == count {
                source.push_text("{ ");
            } else {
                source.push_text("if ");
                source.push(condition);
                source.push_text(" { ");
            }
            if frames.is_empty() || !binds.is_empty() {
                source.push(binds);
                source.push(action.value.rendered_block());
            } else {
                source.push_text(format!("{arm} = {}; ()", deferred.len() + 1));
                deferred.push(action.value.rendered_block());
            }
            source.push_text(if index + 1 == count { " }" } else { " } else " });
        }
        source.push_text("\n}");
        if !frames.is_empty() {
            source = self.deferred_arms(frames, source, &arm, deferred);
        }
        Ok(self.operator_expr(source, output, span(match_span.start(), end)))
    }

    /// The scope frames the subject of a match opened, from the `framed`th
    /// of its statement's on, taken to end with the match's tests, when an
    /// arm binds nothing of the subject: rustc ends the subject's borrow
    /// where no arm left to run uses it, so such an arm runs after the
    /// frames end, and may move or change what they lent, as text_editor's
    /// `key_binding` closure's `_ => Binding::from_key_press(key_press)`
    /// moves the press it matched `key_press.key.as_ref()` of (#2005).
    /// None when every arm binds, or when the subject or a frame's value
    /// uses what it borrows when it drops, which Rust drops after the arm.
    fn subject_frames(
        &mut self,
        framed: usize,
        subject: &ParsedExpr,
        arms: &[(origin::Fragment, origin::Fragment, Branch)],
    ) -> Vec<Frame> {
        if arms.iter().all(|(_, binds, _)| !binds.is_empty())
            || self.drop_use(&subject.ty) != drops::DropUse::None
        {
            return Vec::new();
        }
        let Some(frames) = self
            .frames
            .last_mut()
            .filter(|frames| frames.len() > framed)
        else {
            return Vec::new();
        };
        if frames[framed..].iter().any(|frame| frame.drops) {
            return Vec::new();
        }
        frames.split_off(framed)
    }

    /// `tests`, a match whose arms that bind nothing set `arm` to their
    /// number instead of running, inside `frames`, and then the deferred
    /// arm it chose, or else the value of the arm that ran.
    fn deferred_arms(
        &mut self,
        frames: Vec<Frame>,
        tests: origin::Fragment,
        arm: &str,
        deferred: Vec<origin::Fragment>,
    ) -> origin::Fragment {
        let value = self.temporary("tested");
        let mut source = origin::Fragment::plain(format!("{{\nlet {arm} = 0;\nlet {value} = "));
        source.push(self.framed(frames, tests));
        source.push_text(";\n");
        for (index, body) in deferred.into_iter().enumerate() {
            source.push_text(format!("if {arm} == {} ", index + 1));
            source.push(body);
            source.push_text(" else ");
        }
        source.push_text(format!("{{ {value} }}\n}}"));
        source
    }

    /// The test a pattern on a scalar subject of type `written`, held in
    /// `temporary`, makes: `None` when it matches every value. `name` and
    /// `name @ p` bind the subject. A `&` pattern matches one reference
    /// of `written`; any other pattern matches the scalar below them all,
    /// as rustc's default binding mode reads it.
    pub(super) fn scalar_condition(
        &self,
        syntax: &pattern::Syntax,
        written: &Ty,
        temporary: &str,
        binding: &mut Option<(ImmutableString, Span, Bound)>,
    ) -> Result<Option<String>, Error> {
        let ty = &self.referenced(written);
        let mut bind = |name: &ImmutableString, span: Span, bound: Bound| {
            if binding.as_ref().is_some_and(|(bound, _, _)| bound != name) {
                return Err(Error::InvalidMatch {
                    owner: ty.name(&self.models),
                    reason: "a scalar pattern binds one name".into(),
                    pattern: span,
                });
            }
            *binding = Some((name.clone(), span, bound));
            Ok(())
        };
        Ok(match syntax {
            pattern::Syntax::Discard { .. } => None,
            pattern::Syntax::Binding {
                name,
                span,
                mutable,
            } => {
                let bound = if *mutable {
                    Bound::Mutable
                } else {
                    Bound::Value
                };
                bind(name, *span, bound)?;
                None
            }
            pattern::Syntax::Ref {
                name,
                span,
                mutable: false,
            } => {
                bind(name, *span, Bound::Shared)?;
                None
            }
            pattern::Syntax::Ref {
                name,
                span,
                mutable: true,
            } => {
                bind(name, *span, Bound::Unique)?;
                None
            }
            pattern::Syntax::At {
                name,
                name_span,
                pattern,
            } => {
                bind(name, *name_span, Bound::Value)?;
                self.scalar_condition(pattern, written, temporary, binding)?
            }
            pattern::Syntax::Reference {
                mutable, pattern, ..
            } => match self.inference.resolve(written) {
                Ty::Reference {
                    mutable: actual,
                    target,
                } if actual || !mutable => {
                    self.scalar_condition(pattern, &target, temporary, binding)?
                }
                _ => return Err(self.pattern_mismatch(written, syntax.found(), syntax.span())),
            },
            pattern::Syntax::Literal {
                literal,
                ty: actual,
                span,
            } => {
                self.require_pattern_type(ty, actual, *span)?;
                Some(format!("{temporary} == {}", scalar_text(literal)))
            }
            pattern::Syntax::Range {
                low,
                high,
                inclusive,
                span,
            } => {
                let mut tests = Vec::new();
                for (_, actual) in low.iter().chain(high.iter()) {
                    if !matches!(actual, Ty::Int(_) | Ty::Char | Ty::Float(_)) {
                        return Err(Error::UnsupportedNominalSyntax {
                            construct: "range pattern".into(),
                            source: *span,
                        });
                    }
                    self.require_pattern_type(ty, actual, *span)?;
                }
                reject_empty_range(
                    low.as_ref(),
                    high.as_ref(),
                    *inclusive,
                    &ty.name(&self.models),
                    *span,
                )?;
                if let Some((low, _)) = low {
                    tests.push(format!("{temporary} >= {}", scalar_text(low)));
                }
                if let Some((high, _)) = high {
                    let operator = if *inclusive { "<=" } else { "<" };
                    tests.push(format!("{temporary} {operator} {}", scalar_text(high)));
                }
                Some(format!("({})", tests.join(" && ")))
            }
            pattern::Syntax::Or { alternatives, .. } => {
                let mut tests = Vec::new();
                for alternative in alternatives.iter() {
                    match self.scalar_condition(alternative, written, temporary, binding)? {
                        Some(test) => tests.push(test),
                        None => return Ok(None),
                    }
                }
                Some(format!("({})", tests.join(" || ")))
            }
            pattern::Syntax::Slice { span, .. } => return Err(self.slice_pattern(ty, *span)),
            other => return Err(self.pattern_mismatch(ty, other.found(), other.span())),
        })
    }

    /// The head of `if`, `while`, `for` or `match`, where Rust reads `{`
    /// as the block rather than a struct literal.
    pub(super) fn parse_head(
        &mut self,
        parse: fn(&mut Self) -> Result<ParsedExpr, Error>,
    ) -> Result<ParsedExpr, Error> {
        let outer = std::mem::replace(&mut self.no_struct, true);
        let value = parse(self);
        self.no_struct = outer;
        // A head that is a block, as `matches!(..)`'s match, is wrapped so
        // the script reads it as the head and not as the body. So is one
        // with statements before its value, as a call that stages its
        // arguments (`for x in native::wrapped(value)`) renders, which
        // becomes its block.
        value.map(|mut value| {
            if !value.prelude.is_empty() {
                value.source = value.rendered_block();
                value.prelude = origin::Fragment::default();
            }
            if value.source.as_str().trim_start().starts_with('{') {
                // An iterator's chain stays the wrapped value's, so a `for`
                // over it runs the chain in its own loop.
                let chain = self.pipelines.remove(value.source.as_str());
                value.source = std::mem::take(&mut value.source).surrounded("(", ")");
                if let Some(chain) = chain {
                    self.pipelines
                        .insert(value.source.as_str().to_owned(), chain);
                }
            }
            value
        })
    }

    /// The head ahead is a let chain, as in Rust 2024: `let` joined to
    /// another condition by `&&` (`if let A = x && let B = y && c`).
    pub(super) fn let_chain_ahead(&mut self) -> bool {
        let leading_let = matches!(self.peek(), Token::Let);
        let mut depth = 0usize;
        let mut previous = Token::EOF;
        // Between a `let` and its `=`, a brace is a struct pattern's
        // (`let Preview::Ready { thumbnail, .. } = &preview`), not the block.
        let mut pattern = false;
        for (token, _) in self.input.clone() {
            match token {
                Token::Let if depth == 0 && previous == Token::And => return true,
                Token::Let if depth == 0 => pattern = true,
                Token::Equals if depth == 0 => pattern = false,
                Token::LeftBrace if pattern => depth += 1,
                Token::LeftParen | Token::LeftBracket => depth += 1,
                Token::RightBrace if pattern => {
                    let Some(next) = depth.checked_sub(1) else {
                        return false;
                    };
                    depth = next;
                }
                Token::RightParen | Token::RightBracket => {
                    let Some(next) = depth.checked_sub(1) else {
                        return false;
                    };
                    depth = next;
                }
                Token::LeftBrace | Token::SemiColon | Token::EOF if depth == 0 => return false,
                Token::And if depth == 0 && leading_let => return true,
                _ => {}
            }
            previous = token;
        }
        false
    }

    /// The links of a let chain up to its block: each `let` a switch on
    /// its subject whose bindings the later links and the block see, each
    /// other condition an `if`. The text that opens them and the text that
    /// closes them.
    pub(super) fn parse_chain(&mut self) -> Result<(origin::Fragment, String), Error> {
        let mut open = origin::Fragment::default();
        let mut close = String::new();
        loop {
            let depth = Some(self.expression_stack.len() + 1);
            let outer = std::mem::replace(&mut self.chain_link, depth);
            if matches!(self.peek(), Token::Let) {
                let scrutinee = self.temporary("match");
                let parsed = self.parse_let_condition(&scrutinee, drops::Subject::Statement);
                self.chain_link = outer;
                let (subject, arm, binds, operand, condition) = parsed?;
                // A link's drops would need the chain's scopes.
                let link_drops = self.close_drops()?;
                if !link_drops.is_empty()
                    || !condition.otherwise.is_empty()
                    || !condition.copies.is_empty()
                    || !condition.after.is_empty()
                    || binds.as_str().contains(crate::engine::KEYWORD_CLEAR)
                {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: crate::resolve::Unsupported::said(
                            "a let chain over a value whose type implements `Drop`: \
                                    not supported in Rawr yet",
                        ),
                        source: point_span(self.position()),
                    });
                }
                open.push_text(format!("{{ let {scrutinee} = "));
                open.push(subject);
                open.push_text(format!("; __rawr_switch {operand} {{ "));
                open.push(arm);
                open.push_text(" => { ");
                open.push(binds);
                close.insert_str(0, " }, _ => () } }");
            } else {
                let condition = self.parse_head(Self::parse_expr);
                self.chain_link = outer;
                let condition = condition?;
                self.require_type(&Ty::Bool, &condition.ty, condition.span)?;
                open.push_text("if ");
                open.push(condition.rendered_source());
                open.push_text(" { ");
                close.insert_str(0, " }");
            }
            if !matches!(self.peek(), Token::And) {
                return Ok((open, close));
            }
            self.next();
        }
    }

    /// `if` over a let chain. The block runs inside every link and marks
    /// that it ran; otherwise the alternative runs.
    pub(super) fn parse_if_chain(&mut self, if_span: Span) -> Result<ParsedExpr, Error> {
        let original_bindings = self.bindings.clone();
        let chain = self.parse_chain();
        let split = self.flow().split();
        self.flow().arm(split);
        let chain = chain.and_then(|chain| {
            if matches!(self.peek(), Token::LeftBrace) {
                Ok(chain)
            } else {
                Err(self.unsupported("`if` consequent without a block"))
            }
        });
        let consequent = chain.and_then(|chain| Ok((chain, self.parse_match_action()?)));
        let ((open, close), consequent) = consequent?;
        let consequent_bindings = std::mem::replace(&mut self.bindings, original_bindings.clone());
        let consequent_end = self.flow().arm_end();
        self.flow().arm(split);
        let alternative = if !matches!(self.peek(), Token::Else) {
            self.require_else_less(if_span, &consequent)?;
            Branch {
                value: self.literal("()", Ty::Unit, consequent.syntax),
                syntax: consequent.syntax,
            }
        } else if matches!(self.next_after_else(), Token::If) {
            let nested = self.next().1;
            let value = self.parse_if(point_span(nested))?;
            Branch {
                syntax: value.span,
                value,
            }
        } else {
            if !matches!(self.peek(), Token::LeftBrace) {
                return Err(self.unsupported("`if` alternative without a block"));
            }
            self.parse_match_action()?
        };
        let alternative_bindings = std::mem::replace(&mut self.bindings, original_bindings);
        let alternative_end = self.flow().arm_end();
        self.flow().join(&[consequent_end, alternative_end]);
        self.merge_branches(&[
            (&consequent_bindings, &consequent.value.ty),
            (&alternative_bindings, &alternative.value.ty),
        ]);
        let mut branches = [consequent, alternative];
        let output = self.coerce_branches(
            &mut branches
                .iter_mut()
                .map(|branch| &mut branch.value)
                .collect::<Vec<_>>(),
        )?;
        let [consequent, alternative] = branches;
        let span = span(if_span.start(), alternative.syntax.end());
        self.flow().either(
            span,
            &[
                (consequent.value.span, consequent_end),
                (alternative.value.span, alternative_end),
            ],
        );
        self.note_fn_branches(
            span,
            &[
                (consequent.value.span, &consequent.value.ty),
                (alternative.value.span, &alternative.value.ty),
            ],
        );
        let (hit, value) = (self.temporary("hit"), self.temporary("chain"));
        let mut source =
            origin::Fragment::plain(format!("{{\nlet {hit} = false;\nlet {value} = ();\n"));
        source.push(open);
        source.push_text(format!("{hit} = true; {value} = "));
        source.push(consequent.value.rendered_block());
        source.push_text(";");
        source.push_text(close);
        source.push_text(format!("\nif {hit} {{ {value} }} else "));
        source.push(alternative.value.rendered_block());
        source.push_text("\n}");
        Ok(self.operator_expr(source.surrounded("(", ")"), output, span))
    }

    /// An `if` with no `else` has type `()`, so its consequent must be
    /// `()`. As a statement, rustc reports the consequent (E0308); where its
    /// value is wanted, the `if` (E0317). A function body's tail is wanted
    /// unless the function returns `()`.
    pub(super) fn require_else_less(
        &mut self,
        if_span: Span,
        consequent: &Branch,
    ) -> Result<(), Error> {
        self.else_less.insert(if_span.start());
        let ty = self.inference.resolve(&consequent.value.ty);
        if self.unify_type(&Ty::Unit, &ty) || matches!(ty, Ty::Never) {
            return Ok(());
        }
        let statement = self.block_statement == Some(if_span.start());
        let tail = matches!(self.peek(), Token::RightBrace | Token::EOF);
        let wanted = !statement
            || tail
                && self
                    .return_ty
                    .as_ref()
                    .is_some_and(|output| !matches!(self.inference.resolve(output), Ty::Unit));
        if !wanted {
            return self.require_type(&Ty::Unit, &ty, consequent.value.span);
        }
        Err(Error::IfWithoutElse {
            expected: ty.name(&self.models),
            expression: span(if_span.start(), consequent.syntax.end()),
        })
    }

    pub(super) fn parse_if(&mut self, if_span: Span) -> Result<ParsedExpr, Error> {
        if self.let_chain_ahead() {
            return self.parse_if_chain(if_span);
        }
        // `if let Some(v) = map.get_mut(&k)`: `v` is the value's place.
        if matches!(self.peek(), Token::Let)
            && let Some(value) = self.parse_get_mut_if(if_span)?
        {
            return Ok(value);
        }
        // `if let pattern = subject`: a switch on the subject, whose
        // bindings are in scope only in the consequent.
        let scrutinee = if matches!(self.peek(), Token::Let) {
            self.temporary("match")
        } else {
            String::new()
        };
        let mut operand = scrutinee.clone();
        let mut otherwise = None;
        let mut temporaries = Vec::new();
        // The borrow facts' temporaries of the subject, which end on each
        // path out of the `if let`.
        let mut ending = None;
        let mut copies = String::new();
        let mut after = String::new();
        let (pattern, condition, original_bindings) = if matches!(self.peek(), Token::Let) {
            let original_bindings = self.bindings.clone();
            self.flow().open_temporaries();
            let parsed = self.parse_let_condition(&scrutinee, drops::Subject::Scoped);
            ending = Some(self.flow().take_temporaries());
            let (subject, arm, binds, switched, condition) = parsed?;
            temporaries = condition.temporaries;
            otherwise = Some(condition.otherwise);
            copies = condition.copies;
            after = condition.after;
            operand = switched;
            let condition = self.literal("true", Ty::Bool, if_span);
            (Some((subject, arm, binds)), condition, original_bindings)
        } else {
            // The condition is a temporary scope.
            self.open_temporaries();
            self.flow().open_temporaries();
            let condition = self.parse_head(Self::parse_expr);
            let end = ScopeEnd::Statement(self.last_read());
            self.flow().close_temporaries(end);
            let temporaries = self.close_temporaries();
            let mut condition = condition?;
            if !temporaries.is_empty() {
                condition.source = self
                    .with_temporaries(&temporaries, condition.rendered_source())
                    .surrounded("(", ")");
                condition.prelude = origin::Fragment::default();
            }
            self.require_type(&Ty::Bool, &condition.ty, condition.span)?;
            (None, condition, self.bindings.clone())
        };

        if !matches!(self.peek(), Token::LeftBrace) {
            return Err(self.unsupported("`if` consequent without a block"));
        }
        let split = self.flow().split();
        self.flow().arm(split);
        let mut consequent = self.parse_match_action()?;
        // The condition's scope ends with the consequent; without a match,
        // the value made for it drops before the alternative, as in Rust
        // 2024.
        if otherwise.is_some() {
            let drops = self.close_drops()?;
            if !drops.is_empty() {
                let block = consequent.value.rendered_block();
                consequent.value.source = self.then_drops(block, &drops);
                consequent.value.prelude = origin::Fragment::default();
            }
        }
        let consequent_bindings = self.bindings.clone();
        self.bindings = original_bindings.clone();
        if let Some(ending) = &ending {
            self.flow().end_temporaries(ending);
        }
        let consequent_end = self.flow().arm_end();
        self.flow().arm(split);
        if let Some(ending) = &ending {
            self.flow().end_temporaries(ending);
        }
        let alternative = if !matches!(self.peek(), Token::Else) {
            self.require_else_less(if_span, &consequent)?;
            Branch {
                value: self.literal("()", Ty::Unit, consequent.syntax),
                syntax: consequent.syntax,
            }
        } else if matches!(self.next_after_else(), Token::If) {
            let nested = self.next().1;
            let value = self.parse_if(point_span(nested))?;
            Branch {
                syntax: value.span,
                value,
            }
        } else {
            if !matches!(self.peek(), Token::LeftBrace) {
                return Err(self.unsupported("`if` alternative without a block"));
            }
            self.parse_match_action()?
        };
        let alternative_bindings = self.bindings.clone();
        self.bindings = original_bindings;
        let alternative_end = self.flow().arm_end();
        self.flow().join(&[consequent_end, alternative_end]);
        self.merge_branches(&[
            (&consequent_bindings, &consequent.value.ty),
            (&alternative_bindings, &alternative.value.ty),
        ]);

        let mut branches = [consequent, alternative];
        let output = self.coerce_branches(
            &mut branches
                .iter_mut()
                .map(|branch| &mut branch.value)
                .collect::<Vec<_>>(),
        )?;
        let [consequent, alternative] = branches;
        let span = span(if_span.start(), alternative.syntax.end());
        self.flow().either(
            span,
            &[
                (consequent.value.span, consequent_end),
                (alternative.value.span, alternative_end),
            ],
        );
        self.note_fn_branches(
            span,
            &[
                (consequent.value.span, &consequent.value.ty),
                (alternative.value.span, &alternative.value.ty),
            ],
        );
        if let Some((subject, arm, binds)) = pattern {
            let matched = if after.is_empty() {
                String::new()
            } else {
                format!("let {} = ", self.temporary("matched"))
            };
            let mut source = origin::Fragment::plain("{\n");
            source.push_text(drops::declarations(&temporaries));
            source.push_text(format!("let {scrutinee} = "));
            source.push(subject);
            source.push_text(format!(";\n{copies}{matched}__rawr_switch {operand} {{\n"));
            source.push(arm);
            source.push_text(" => { ");
            source.push(binds);
            source.push(consequent.value.rendered_block());
            source.push_text(" },\n_ => ");
            match otherwise.filter(|otherwise| !otherwise.is_empty()) {
                Some(otherwise) => {
                    source.push_text(format!("{{ {otherwise}"));
                    source.push(alternative.value.rendered_block());
                    source.push_text(" }");
                }
                None => source.push(alternative.value.rendered_block()),
            }
            source.push_text("\n}");
            // A move out of a local the binding shadowed follows the match.
            if let Some(matched) = matched
                .strip_prefix("let ")
                .and_then(|matched| matched.strip_suffix(" = "))
            {
                source.push_text(format!(";\n{after}{matched}"));
            }
            source.push_text("\n}");
            return Ok(self.operator_expr(source.surrounded("(", ")"), output, span));
        }
        let mut source = origin::Fragment::plain("if ");
        source.push(condition.source);
        source.push_text(" ");
        source.push(consequent.value.rendered_block());
        source.push_text(" else ");
        source.push(alternative.value.rendered_block());
        Ok(ParsedExpr {
            prelude: condition.prelude,
            source,
            ty: output,
            span,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin())
    }

    /// The type the block or arm ahead is expected to have: the
    /// expectation of the expression it gives the value of.
    pub(super) fn block_expectation(&self) -> Option<Ty> {
        self.collect_hint
            .as_ref()
            .filter(|(_, depth)| *depth == self.expression_stack.len())
            .map(|(expected, _)| expected.clone())
    }

    /// A block whose value is not the enclosing expression's, as a loop's
    /// body, a closure's or a `let`-`else`'s: no expectation reaches it.
    pub(super) fn parse_unexpected_block(&mut self) -> Result<Branch, Error> {
        let outer = self.collect_hint.take();
        let block = self.parse_match_action();
        self.collect_hint = outer;
        block
    }

    pub(super) fn parse_match_action(&mut self) -> Result<Branch, Error> {
        if matches!(self.peek(), Token::LeftBrace) {
            let start = self.next().1;
            let outer_output = std::mem::take(&mut self.output);
            let outer_last_type = self.last_type.clone();
            let outer_last_expression = self.last_expression.take();
            let outer_macros = self.local_macros.len();
            let outer_statics = self.block_statics.len();
            self.last_type = Ty::Unit;
            self.open_drops();
            let (outer_tails, escapes) = self.open_tails();
            self.flow().open_block();
            let outer_locals: BTreeSet<Span> = self
                .bindings
                .values()
                .map(|binding| binding.declaration)
                .collect();
            // A closure parameter's types the block's place gives reach its
            // tail closure, as `on_key_event({ let t = ..; move |key| .. })`
            // has rustc check the tail against the expected type (#1450).
            let closure_hint = self.closure_hint.take();
            let closure_output = self.closure_output.take();
            // The type the block is expected to have reaches its tail.
            let expected = self.block_expectation();

            while !matches!(self.peek(), Token::RightBrace | Token::EOF) {
                if self.closure_ahead() {
                    self.closure_hint.clone_from(&closure_hint);
                    self.closure_output.clone_from(&closure_output);
                }
                match self.peek() {
                    Token::Let => self.parse_binding(true)?,
                    // `const fn f()` is a nested `fn` item.
                    Token::Const if matches!(self.peek_second(), Token::Fn) => {
                        self.next();
                        self.skip_nested_fn()?;
                    }
                    Token::Const => self.parse_binding(false)?,
                    _ if self.static_ahead() => self.parse_block_statics()?,
                    Token::Fn => self.skip_nested_fn()?,
                    Token::Use => self.skip_nested_use()?,
                    _ if self.macro_definition_ahead() => self.parse_local_macro()?,
                    _ if self.macro_statement_ahead() => self.parse_macro_statement()?,
                    _ if self.nested_item_ahead() => self.skip_nested_item()?,
                    _ => self.parse_tail_expecting(expected.as_ref())?,
                }
            }
            // A block's own macros and statics end with it.
            self.local_macros.truncate(outer_macros);
            self.block_statics.truncate(outer_statics);
            let end = self.expect(Token::RightBrace, "'}' after a match arm block")?;
            // The block's own locals end with it.
            let declared: Vec<Span> = self
                .bindings
                .values()
                .map(|binding| binding.declaration)
                .filter(|declaration| !outer_locals.contains(declaration))
                .collect();
            let mut drop_uses = self.block_drop_uses();
            drop_uses.extend(self.cell_guard_drops(&declared));
            self.flow()
                .storage_dead(&declared, &drop_uses, point_span(end));
            let mut block_output = std::mem::replace(&mut self.output, outer_output);
            let ty = std::mem::replace(&mut self.last_type, outer_last_type);
            let inner_last_expression =
                std::mem::replace(&mut self.last_expression, outer_last_expression);
            let syntax = span(start, end);
            let tail = inner_last_expression.is_some();
            let (mut prelude, mut source, endpoint) = match inner_last_expression {
                Some((start, endpoint)) => {
                    let mut source = block_output.split_off(start);
                    source.trim_end();
                    (block_output, source, endpoint)
                }
                None => (block_output, origin::Fragment::plain("()"), syntax),
            };
            let handed = self.close_tails(outer_tails);
            let temporaries = self.escaped_tails(handed, escapes);
            self.close_block_drops(&mut prelude, &mut source, tail, &temporaries)?;
            return Ok(Branch {
                value: ParsedExpr {
                    prelude,
                    source,
                    ty,
                    span: endpoint,
                    lvalue: None,
                    outcome: crate::host::usage::Outcome::Conservative,
                    evaluation: crate::host::usage::Evaluation::Required,
                }
                .retain_origin(),
                syntax,
            });
        }

        let start = self.position();
        let pending = self.field_entries.len();
        let block_like = self.peek_block_like();
        // An arm is a temporary scope, and opens its own scope frames.
        self.open_temporaries();
        self.flow().open_temporaries();
        self.open_frames();
        let expected = self.block_expectation();
        let value = self.parse_expr_expecting(expected.as_ref());
        let frames = self.close_frames();
        self.flow().close_temporaries(ScopeEnd::Value);
        let temporaries = self.close_temporaries();
        let mut value = value?;
        if matches!(self.inference.resolve(&value.ty), Ty::Never) {
            self.flow().diverge();
        }
        if !frames.is_empty() {
            let rendered = value.rendered_source();
            value.source = self.framed(frames, rendered);
            value.prelude = origin::Fragment::default();
            value.lvalue = None;
        }
        if !temporaries.is_empty() {
            if assignment_token(&self.peek()) {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "an assignment to a temporary place whose type implements `Drop`"
                        .into(),
                    source: value.span,
                });
            }
            value.source = self.with_temporaries(&temporaries, value.rendered_source());
            value.prelude = origin::Fragment::default();
        }
        if !assignment_token(&self.peek()) {
            // An arm returning a `&mut` place gives its path.
            value = self.returned_place(value, block_like)?;
            self.consume_imported_value(&value, "branch result", value.span)?;
            self.move_out_value(&mut value)?;
            // A bare arm such as `Key::Char(c) => app.input.push(c)` reads
            // and stores its place itself, as a block arm's statements do.
            // Left to the whole `match`, its store would undo another
            // arm's change to the same place.
            if self.field_entries.len() > pending {
                let entries = self.field_entries.split_off(pending);
                let result = self.temporary("arm");
                let mut block = origin::Fragment::plain("{ ");
                for (read, _) in &entries {
                    block.push(read.clone());
                }
                block.push_text(format!(" let {result} = "));
                block.push(value.rendered_source());
                block.push_text("; ");
                for (_, store) in entries {
                    block.push(store);
                }
                block.push_text(format!(" {result} }}"));
                value.source = block;
                value.prelude = origin::Fragment::default();
            }
            return Ok(Branch {
                syntax: value.span,
                value,
            });
        }

        let (assignment, operator_at) = self.next();
        let Some(lvalue) = value.lvalue else {
            let refused = self.unplaced_assignment(&value, &assignment, operator_at)?;
            return Err(
                refused.unwrap_or_else(|| self.unsupported("assignment target in match arm"))
            );
        };
        let binding_name = lvalue.binding().clone();
        let Some(binding) = self.mutation_binding(&binding_name) else {
            return Err(self.unsupported("assignment through an unknown binding"));
        };
        // A `let x;` not declared `mut` takes its one assignment here, as
        // in a statement; the borrow check follows the paths that assign it.
        let deferred = matches!(lvalue, Lvalue::Binding { .. })
            .then(|| self.deferred.get(&binding.declaration).copied())
            .flatten();
        // A place below a `&mut` local changes through the reference, and
        // one below a `&` local can't, whether or not the local is `mut`.
        let reference = match self.inference.resolve(&binding.ty) {
            Ty::Reference { mutable, .. } if !matches!(lvalue, Lvalue::Binding { .. }) => {
                Some(mutable)
            }
            _ => None,
        };
        let shared = reference == Some(false) && matches!(binding.kind, BindingKind::Local);
        let immutable =
            (!binding.mutable && reference.is_none() && matches!(binding.kind, BindingKind::Local))
                .then_some(binding.declaration)
                .filter(|_| binding.available || deferred.is_none())
                .map(|declaration| deferred.flatten().unwrap_or(declaration));
        if !binding.mutable && !matches!(binding.kind, BindingKind::Local) {
            return Err(match &binding.kind {
                BindingKind::Receiver { .. } if !matches!(lvalue, Lvalue::Binding { .. }) => {
                    self.receiver_assignment(&binding.ty, value.span)
                }
                BindingKind::Receiver { owner, .. } => Error::ImmutableReceiver {
                    owner: owner.clone(),
                    mutation: value.span,
                    receiver: binding.declaration,
                },
                BindingKind::Local | BindingKind::Invocation => {
                    self.immutable_assignment(&lvalue, binding.declaration, value.span)
                }
            });
        }
        if let Some(declaration) = immutable.filter(|_| deferred.is_none()) {
            return Err(self.immutable_assignment(&lvalue, declaration, value.span));
        }
        // `Some(n) => r = &mut b,` of a `&mut` local given new borrows.
        if compound_operator(&assignment).is_none()
            && !shared
            && let Some((selector, mutable)) = self.rebound_selector(&value.ty, &lvalue)
        {
            let (rebind, end) =
                self.rebind_selector(value.span, &value.ty, selector, mutable, false)?;
            let value = ParsedExpr {
                prelude: origin::Fragment::default(),
                source: rebind.surrounded("{ ", " }"),
                ty: Ty::Unit,
                span: span(start, end.end()),
                lvalue: None,
                outcome: crate::host::usage::Outcome::Conservative,
                evaluation: crate::host::usage::Evaluation::Required,
            }
            .retain_origin();
            return Ok(Branch {
                syntax: value.span,
                value,
            });
        }
        self.refuse_rebound_alias(value.span, &value.ty)?;
        let value_start = self.flow().value_start();
        let mut replacement = self.parse_expr()?;
        let compound = compound_operator(&assignment).is_some();
        let kind = match compound_operator(&assignment) {
            Some(_) => self.compound_kind(&value.ty, &replacement.ty),
            None => record::Assignment::Plain,
        };
        self.assigned_place(
            value.span,
            &value.ty,
            &lvalue,
            &replacement,
            value_start,
            kind,
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
        let whole = span(value.span.start(), replacement.span.end());
        if shared {
            return Err(self.shared_local_assignment(&binding_name, value.span, whole));
        }
        if let Some(first) = immutable {
            return Err(self.immutable_assignment(
                &lvalue,
                self.expression_span(first),
                self.expression_span(whole),
            ));
        }
        self.consume_imported_value(&replacement, &binding_name, replacement.span)?;
        self.move_local(&mut replacement)?;
        if compound_operator(&assignment).is_none()
            && self.needs_drop(lvalue.ty(), replacement.span)?
            && let Some(read) = self.replaced_read(&lvalue)
        {
            self.drop_replaced(&lvalue.ty().clone(), read, &mut replacement)?;
        }
        let mut source = origin::Fragment::plain("{ ");
        match lvalue {
            Lvalue::Binding { .. } => {
                source.push_text(format!("{binding_name} = "));
                source.push(replacement.rendered_source());
                source.push_text("; }");
                if let Some(binding) = self.bindings.get_mut(&binding_name) {
                    binding.available = true;
                    if let Some(first @ None) = self.deferred.get_mut(&binding.declaration) {
                        *first = Some(whole);
                    }
                }
            }
            Lvalue::Field(field) => {
                source.push(field.place());
                source.push_text(" = ");
                source.push(replacement.rendered_source());
                source.push_text("; }");
            }
            Lvalue::Slice(_) => return Err(self.unsupported("assigning to a slice range")),
            Lvalue::Nested(nested) => {
                let stored = match opened {
                    Some(parts) => parts.store(replacement.rendered_source()),
                    None if compound => self.store_nested(&nested, replacement.rendered_source()),
                    None => self.assign_nested(&nested, replacement.rendered_source()),
                };
                source.push(stored);
                source.push_text(" }");
            }
            Lvalue::Place { place, .. } => {
                source.push(place);
                source.push_text(" = ");
                source.push(replacement.rendered_source());
                source.push_text("; }");
            }
        }
        let value = ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: Ty::Unit,
            span: span(start, replacement.span.end()),
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin();
        Ok(Branch {
            syntax: value.span,
            value,
        })
    }
}

/// How an arm of a match on a scalar or a string binds the value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Bound {
    /// `v`, a copy or a move of the value.
    Value,
    /// `mut v`.
    Mutable,
    /// `ref v`, a shared borrow of the value as written.
    Shared,
    /// `ref mut v`, the matched place.
    Unique,
}
