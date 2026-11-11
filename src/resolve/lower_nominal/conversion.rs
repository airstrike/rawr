//! Conversions: `.into()`, `From`, `TryFrom` and `try_into()` between script,
//! std and crate types, each chosen once its types are known.

use super::*;

impl<'a> Parser<'a> {
    /// The `impl From<source>` function of the script type `target`, when
    /// exactly one applies.
    /// The `impl From<S> for Target` that takes `source`, its impl's
    /// parameters instantiated at this use (`impl<T> From<T> for W<T>`):
    /// the impl, its `S` unified with `source`, and its `Self` type,
    /// unified with `expected` when the use names the target's arguments.
    pub(super) fn select_conversion(
        &mut self,
        target: type_def::Id,
        expected: Option<&Ty>,
        source: &Ty,
        span: Span,
    ) -> Result<Option<(method::Definition, Ty, Ty)>, Error> {
        let mut candidates = self
            .models
            .conversions
            .get(&target)
            .cloned()
            .unwrap_or_default();
        // A crate's type that another of its paths names, as a macro's
        // `$crate::compose::Slot` names the `natives::compose::Slot` the
        // program's `impl From<Mine> for Slot<'a, ()>` names: the impls
        // for the type at its other paths, which unify with it.
        if let Some(expected) = expected
            && matches!(self.inference.resolve(expected), Ty::Host { .. })
        {
            candidates.extend(
                self.models
                    .conversions
                    .iter()
                    .filter(|(owner, _)| **owner != target)
                    .flat_map(|(_, methods)| methods)
                    .filter(|method| method.builtin_owner().is_some())
                    .cloned(),
            );
        }
        // An `impl From` is as visible as `From`, from every module.
        let mut fitting = self.fitting_conversions(candidates, expected, source);
        // Several fit an unsuffixed literal: rustc's fallback, `i32` or
        // `f64`, chooses the impl that takes it.
        if fitting.len() > 1
            && let Some(fallback) = literal_fallback(&self.inference.resolve(source))
        {
            fitting = self.fitting_conversions(fitting, expected, &fallback);
        }
        let Ok([candidate]) = <[_; 1]>::try_from(fitting) else {
            return Ok(None);
        };
        let substitution = self.fresh_generics(candidate.generics(), span);
        let from = instantiate(
            &Ty::from_resolved(candidate.conversion().expect("conversions have a source")),
            &substitution,
        );
        let owner = impl_self_ty(&candidate, &substitution);
        if let Some(expected) = expected {
            self.unify_type(&owner, expected);
        }
        self.unify_type(&from, source);
        // `impl<'a, M> From<Mine> for Slot<'a, M>`: the instance at the
        // types this use gives the impl's parameters (D629).
        #[cfg(feature = "rust-import")]
        let candidate = self.bind_method_instance(candidate, &substitution, span)?;
        Ok(Some((candidate, from, owner)))
    }

    /// `ty` as rustc names it in an unsatisfied bound: an unsuffixed
    /// literal's type is its fallback, `i32` or `f64`, by then.
    pub(super) fn fallback_name(&self, ty: &Ty) -> ImmutableString {
        let ty = self.inference.resolve(ty);
        literal_fallback(&ty).unwrap_or(ty).name(&self.models)
    }

    /// The `candidates` whose `impl From<S> for Self` takes `source`, with
    /// `Self` unified with `expected` when the use names it.
    pub(super) fn fitting_conversions(
        &mut self,
        candidates: Vec<method::Definition>,
        expected: Option<&Ty>,
        source: &Ty,
    ) -> Vec<method::Definition> {
        candidates
            .into_iter()
            .filter(|candidate| {
                let (from, owner) = self.conversion_source(candidate);
                let checkpoint = self.inference.checkpoint();
                let fits = expected.is_none_or(|expected| self.unify_type(&owner, expected))
                    && self.unify_type(&from, source);
                self.inference.rollback(checkpoint);
                fits
            })
            .collect()
    }

    /// `S` and `Self` of a conversion's `impl From<S> for Self`, its impl's
    /// parameters fresh and unbounded: what it may take, for probing.
    pub(super) fn conversion_source(&mut self, candidate: &method::Definition) -> (Ty, Ty) {
        let probe = candidate
            .generics()
            .iter()
            .map(|generic| (generic.name.text.clone(), self.inference.fresh()))
            .collect();
        let from = instantiate(
            &Ty::from_resolved(candidate.conversion().expect("conversions have a source")),
            &probe,
        );
        (from, impl_self_ty(candidate, &probe))
    }

    /// The program's `impl TryFrom<S> for T` functions: each `try_from`
    /// with the type it takes, its impl's parameters fresh.
    pub(super) fn program_try_froms(&mut self) -> Vec<(method::Definition, Ty, Ty)> {
        let definitions = self
            .models
            .trait_methods
            .iter()
            .filter(|((_, member), _)| member.as_str() == "try_from")
            .flat_map(|(_, definitions)| definitions.iter())
            .filter(|definition| {
                definition
                    .trait_name()
                    .is_some_and(|name| name == "TryFrom")
            })
            .cloned()
            .collect::<Vec<_>>();
        definitions
            .into_iter()
            .filter_map(|definition| {
                let parameter = definition.parameters().first()?.ty.clone();
                let probe = definition
                    .generics()
                    .iter()
                    .map(|generic| (generic.name.text.clone(), self.inference.fresh()))
                    .collect();
                let from = instantiate(&Ty::from_resolved(&parameter), &probe);
                let owner = impl_self_ty(&definition, &probe);
                Some((definition, from, owner))
            })
            .collect()
    }

    /// Whether some `impl TryFrom<S>` of the program takes a `source`.
    pub(super) fn program_try_from(&mut self, source: &Ty) -> bool {
        self.program_try_froms()
            .into_iter()
            .any(|(_, from, _)| self.unifies(&from, source))
    }

    /// `value.try_into()` where a program's `TryFrom` may take it: a
    /// `Result` of the target the use site needs, through that target's
    /// `try_from` (`finish_conversions`).
    pub(super) fn parse_try_into(&mut self, value: ParsedExpr, end: Position) -> ParsedExpr {
        let source = self.inference.resolve(&value.ty);
        let expression = span(value.span.start(), end);
        let placeholder = conversion_placeholder(self.pending_conversions.len());
        let width = placeholder.len() + 1;
        let placeholder = format!("{placeholder:_<width$}");
        let target = self.inference.fresh();
        let error = self.inference.fresh();
        self.pending_conversions.push(PendingConversion {
            placeholder: placeholder.clone(),
            source,
            target: target.clone(),
            span: expression,
            host: None,
            try_from: Some(error.clone()),
            lending: None,
        });
        let mut rendered = origin::Fragment::call(&placeholder, [value.rendered_source()]);
        rendered.push_text(")");
        self.operator_expr(
            rendered,
            Ty::Result(Box::new(target), Box::new(error)),
            expression,
        )
    }

    /// The error type of each pending `try_into()` whose target is known
    /// by now: its `TryFrom` impl's `Error`, as rustc selects the impl once
    /// the target is, before the end of the body chooses its helper.
    pub(super) fn settle_try_from_errors(&mut self) {
        let pending = self
            .pending_conversions
            .iter()
            .filter_map(|conversion| {
                let error = conversion.try_from.as_ref()?;
                let target = self.inference.resolve(&conversion.target);
                (!matches!(target, Ty::Infer(_))
                    && matches!(self.inference.resolve(error), Ty::Infer(_)))
                .then(|| (target, conversion.source.clone(), error.clone()))
            })
            .collect::<Vec<_>>();
        for (target, source, error) in pending {
            for (_, from, owner) in self.program_try_froms() {
                let checkpoint = self.inference.checkpoint();
                if !(self.unify_type(&owner, &target) && self.unify_type(&from, &source)) {
                    self.inference.rollback(checkpoint);
                    continue;
                }
                let declared = self.inference.resolve(&Ty::Projection(
                    Box::new(target.clone()),
                    "TryFrom".into(),
                    "Error".into(),
                ));
                if matches!(declared, Ty::Projection(..)) {
                    self.inference.rollback(checkpoint);
                } else {
                    self.inference.commit(checkpoint);
                    self.unify_type(&error, &declared);
                }
                break;
            }
        }
    }

    /// The helper of `source.try_into()` into `target`: the program's
    /// `TryFrom<source>` for it, its `Error` the call's `error`.
    pub(super) fn try_from_helper(
        &mut self,
        target: &Ty,
        source: &Ty,
        error: &Ty,
        call: Span,
    ) -> Result<String, Error> {
        for (definition, from, owner) in self.program_try_froms() {
            let checkpoint = self.inference.checkpoint();
            if !(self.unify_type(&owner, target) && self.unify_type(&from, source)) {
                self.inference.rollback(checkpoint);
                continue;
            }
            self.inference.commit(checkpoint);
            let declared = self.inference.resolve(&Ty::Projection(
                Box::new(target.clone()),
                "TryFrom".into(),
                "Error".into(),
            ));
            if !matches!(declared, Ty::Projection(..)) {
                self.unify_type(error, &declared);
            }
            return Ok(method_name(definition.id()));
        }
        let (source, target) = (source.name(&self.models), target.name(&self.models));
        Err(Error::MissingCapability {
            owner: target.clone(),
            capability: format!("TryFrom<{source}>").into(),
            operation: call,
            derive: false,
            required_by: None,
        })
    }

    /// Whether `value.clone_from(..)` is `Clone`'s provided method: the
    /// value's type is `Clone` and declares no `clone_from` of its own.
    pub(super) fn provided_clone_from(&mut self, value: &ParsedExpr) -> bool {
        let ty = self.inference.resolve(&value.ty);
        let ty = without_references(&ty).clone();
        if let Ty::Script(id, _) = &ty
            && self.own_member(*id, "clone_from")
        {
            return false;
        }
        self.type_provides(&ty, capability::Kind::Clone)
    }

    /// `place.clone_from(&source)`: std's provided body, `*self =
    /// source.clone()`, into the receiver's place.
    pub(super) fn parse_clone_from(
        &mut self,
        mut value: ParsedExpr,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        // `Clone::clone_from(&mut a, &b)` is `(a).clone_from(&b)`: `a`'s
        // place.
        if value.lvalue.is_none()
            && let Some(name) = variable_name(
                value
                    .source
                    .as_str()
                    .trim_matches(|c: char| c == '(' || c == ')' || c.is_whitespace()),
            )
            && self.bindings.contains_key(name.as_str())
        {
            value.lvalue = Some(Lvalue::Binding {
                binding: name,
                ty: value.ty.clone(),
            });
        }
        let (arguments, close) = self.take_argument_tokens()?;
        let (source, extra) = split_first_argument(arguments);
        let call = span(value.span.start(), close);
        if source.is_empty() || !extra.is_empty() {
            return Err(Error::argument_count(
                Callee::Method,
                crate::resolve::Qualified("Clone", "clone_from").to_string(),
                1,
                usize::from(!source.is_empty()) + usize::from(!extra.is_empty()),
                call,
            ));
        }
        if let Some(Lvalue::Binding { binding: name, .. }) = &value.lvalue
            && !matches!(self.inference.resolve(&value.ty), Ty::Reference { .. })
            && let Some(binding) = self.mutation_binding(name)
            && !binding.mutable
            && matches!(binding.kind, BindingKind::Local | BindingKind::Invocation)
        {
            return Err(Error::BorrowConflict {
                code: "E0596",
                message: format!(
                    "cannot borrow `{name}` as mutable, as it is not declared as mutable"
                )
                .into(),
                access: value.span,
                borrow: Span::NONE,
            });
        }
        let identifier = |name: &str| Token::Identifier(Box::new(name.into()));
        let mut tokens = vec![(Token::LeftParen, member_span.start())];
        tokens.extend(without_reference(source));
        tokens.extend([
            (Token::RightParen, close),
            (Token::Period, close),
            (identifier("clone"), close),
            (Token::Unit, close),
        ]);
        self.splice_tokens(tokens);
        let mut cloned = self.parse_expr()?;
        let target = without_references(&self.inference.resolve(&value.ty)).clone();
        self.coerce_expr(&target, &mut cloned)?;
        let mut source = origin::Fragment::plain("{ ");
        source.push(self.keyed_place(&value)?);
        source.push_text(" = ");
        source.push(cloned.rendered_source());
        source.push_text("; }");
        Ok(self.operator_expr(source, Ty::Unit, call))
    }

    /// Whether some `impl From<T>` could take a value of type `source`.
    pub(super) fn converts_from(&mut self, source: &Ty) -> bool {
        // The prelude's `From<bool>` and `From<char>`.
        if !builtin_conversions_from(&self.inference.resolve(source)).is_empty() {
            return true;
        }
        // A generic target `T: From<S>` in scope takes an `S`; `Vec<T>`
        // takes `[T; N]`.
        if !self.generic_from_sources(source).is_empty()
            || matches!(
                self.inference.resolve(source),
                Ty::Array {
                    kind: sequence::Kind::Array(_),
                    ..
                }
            )
        {
            return true;
        }
        let candidates = self
            .models
            .conversions
            .values()
            .flatten()
            .filter(|candidate| candidate.conversion().is_some())
            .cloned()
            .collect::<Vec<_>>();
        candidates.iter().any(|candidate| {
            let (from, _) = self.conversion_source(candidate);
            self.unifies(&from, source)
        })
    }

    /// `value.into()` on a value some `impl From` takes: a conversion chosen
    /// by the type the context requires, as Rust infers it. A value of a
    /// type no `From` impl takes converts only to itself.
    pub(super) fn parse_conversion_into(&mut self, value: ParsedExpr, end: Position) -> ParsedExpr {
        let source = self.inference.resolve(&value.ty);
        let expression = span(value.span.start(), end);
        if !self.converts_from(&source) {
            let mut value = value;
            value.span = expression;
            value.lvalue = None;
            return value.retain_origin();
        }
        // Wide enough for `dispatcher(witness, ` of a generic target
        // `T: From<S>` in scope, which `finish_conversions` may write.
        let width = self
            .generic_trait_arguments
            .keys()
            .filter(|(_, trait_name)| trait_name == "From")
            .map(|(param, _)| generic_conversion_call(param, FALLBACK_INSTANCE).len())
            .max();
        if width.is_some() {
            self.capture_witnesses();
        }
        // One more than any helper name, for the `(` written after it.
        let placeholder = conversion_placeholder(self.pending_conversions.len());
        let width = width.unwrap_or(0).max(placeholder.len() + 1);
        let placeholder = format!("{placeholder:_<width$}");
        let target = self.inference.fresh();
        self.pending_conversions.push(PendingConversion {
            placeholder: placeholder.clone(),
            source,
            target: target.clone(),
            span: expression,
            host: None,
            try_from: None,
            lending: None,
        });
        // `placeholder(value))`: the placeholder becomes `helper(`, or
        // `dispatcher(witness, ` for a generic target.
        let mut rendered = origin::Fragment::call(&placeholder, [value.rendered_source()]);
        rendered.push_text(")");
        self.operator_expr(rendered, target, expression)
    }

    /// `value.into()` of a facade value whose type has its own `into`
    /// forms: a conversion chosen once the target is known
    /// (`finish_conversions`), by an `into` form's output or the target's
    /// `from` of the value, as Rust's `Into` comes from `From`.
    ///
    /// Candidates of different traits, or a trait's and an impl's own,
    /// are not chosen by the target: the call is the method call it is,
    /// which the receiver and lexical scope settle (D741).
    pub(super) fn parse_host_into(
        &mut self,
        value: ParsedExpr,
        source: &Ty,
        end: Position,
    ) -> Result<ParsedExpr, Error> {
        if !matches!(source, Ty::Host { .. }) {
            return Ok(value);
        }
        let Some((forms, owner)) = self.conversion_forms(source) else {
            let at = span(value.span.start(), end);
            self.pending_arguments = Some((Vec::new(), end));
            return self.parse_host_method(value, "into".into(), at);
        };
        Ok(self.parse_target_into(value, source, forms, owner, end))
    }

    /// The `into` forms of a facade value's type, each a method without
    /// inputs, and their owner: at the path its alias names, or for an
    /// imported type no alias of the program names, at one of its crate's
    /// paths that has them. `None` where the crate's impls give the type
    /// `into`s of more than one method, which no target chooses among.
    pub(super) fn conversion_forms(
        &self,
        source: &Ty,
    ) -> Option<(
        Vec<crate::host::item::Definition>,
        Option<crate::host::facade::Dependency>,
    )> {
        let Ty::Host { proof, .. } = source else {
            return Some((Vec::new(), None));
        };
        let path = format!("{}::into", proof.definition.path());
        #[cfg(feature = "rust-import")]
        let path = match self.imported_member_owner(source, "into") {
            Some(owner) if self.models.host_items(&path).is_none() => {
                format!("{}::into", owner.path())
            }
            _ => path,
        };
        let forms = self.models.host_items(&path).map(<[_]>::to_vec);
        // A crate's `From<Degrees>` for both `f32` and `f64` gives
        // `Degrees` an `into` of each, alike in their inputs, so they are a
        // family. Each is an instance of `Into::into`, by the importer's
        // identity of the trait's method, so the conversion's target picks
        // one, as rustc infers `Into`'s argument and selects the target's
        // `From<Degrees>` impl. A crate trait's own `into` among them is
        // another method, which only the call's scope tells apart.
        #[cfg(feature = "rust-import")]
        let forms = match forms {
            Some(forms) => Some(forms),
            None => match self.models.host.pending_family(&path) {
                Some(family) => {
                    let methods = family.methods();
                    let member = methods.first().and_then(|item| item.trait_member());
                    if member.is_none() || methods.iter().any(|item| item.trait_member() != member)
                    {
                        return None;
                    }
                    Some(methods.to_vec())
                }
                None => None,
            },
        };
        let forms = forms
            .unwrap_or_default()
            .into_iter()
            .filter(|item| {
                item.kind() == crate::host::item::Kind::Method
                    && item.signature().parameters().is_empty()
            })
            .collect();
        Some((forms, self.models.item_owner(&path).cloned()))
    }

    /// Keep the `into` of the imported type `ty` as a miss when none of
    /// its paths has one yet: a crate's `impl From<Degrees> for f32` makes
    /// it a member of `Degrees`, which the importer brings once it is
    /// demanded. A pass that fails demands it with what else it missed, so
    /// the conversion is checked again; asked already, the failure is the
    /// pass's own error, as rustc's E0277.
    #[cfg(feature = "rust-import")]
    pub(super) fn demand_into(&mut self, ty: &Ty) {
        if let Some(owner) = self.imported_public_path(ty, "into") {
            self.recovered.push(Error::UnknownMember {
                owner,
                member: "into".into(),
                reference: Span::NONE,
            });
        }
    }

    /// `value.into()` chosen once its target is known
    /// (`finish_conversions`): the value itself when the target is its own
    /// type or no use fixes one, else one of `forms` whose output is the
    /// target or the target's `from` that takes it, as rustc's `Into`
    /// comes from the target's `From`.
    pub(super) fn parse_target_into(
        &mut self,
        value: ParsedExpr,
        source: &Ty,
        forms: Vec<crate::host::item::Definition>,
        owner: Option<crate::host::facade::Dependency>,
        end: Position,
    ) -> ParsedExpr {
        let expression = span(value.span.start(), end);
        let placeholder = conversion_placeholder(self.pending_conversions.len());
        // Room for the longest helper an op can be named.
        let width = placeholder.len() + 1;
        let placeholder = format!("{placeholder:_<width$}");
        let target = self.inference.fresh();
        self.pending_conversions.push(PendingConversion {
            placeholder: placeholder.clone(),
            source: source.clone(),
            target: target.clone(),
            span: expression,
            host: Some(HostInto {
                forms,
                owner,
                receiver: value.span,
            }),
            try_from: None,
            lending: None,
        });
        let mut rendered = origin::Fragment::call(&placeholder, [value.rendered_source()]);
        rendered.push_text(")");
        self.operator_expr(rendered, target, expression)
    }

    /// The helper of a facade value's `.into()` into `target`: the one
    /// `into` form of its type whose output is `target`, or `target`'s one
    /// `from` that takes it. `None` when neither converts it. More than one
    /// that fits, or a target the program has not fixed where the forms
    /// are a trait method's instances, is rustc's E0283: rustc infers
    /// `Into<U>`'s `U` before it selects an impl, never a first fit. A
    /// target whose type is known but whose arguments are still open, as
    /// `Slot<'_, _>` in a row whose message only a later child fixes, is
    /// fixed enough: rustc selects the one impl that unifies, and its
    /// unification binds what it can (#1403).
    pub(super) fn host_into_helper(
        &mut self,
        host: &HostInto,
        source: &Ty,
        target: &Ty,
        call: Span,
    ) -> Result<Option<String>, Error> {
        let mut fitting = Vec::new();
        for item in &host.forms {
            let checkpoint = self.inference.checkpoint();
            if self
                .instantiate_host_signature(item, Some(source), Some(&[]), Span::NONE)
                .is_ok_and(|signature| self.unify_type(&signature.output, target))
            {
                fitting.push(item);
            }
            self.inference.rollback(checkpoint);
        }
        #[cfg(feature = "rust-import")]
        let instances = fitting.iter().all(|item| item.trait_member().is_some());
        #[cfg(not(feature = "rust-import"))]
        let instances = false;
        match fitting[..] {
            [] => {}
            [item] if !(instances && is_unsettled(&self.inference.resolve(target))) => {
                let signature =
                    self.instantiate_host_signature(item, Some(source), Some(&[]), Span::NONE)?;
                self.unify_type(&signature.output, target);
                let receiver =
                    self.operator_expr(origin::Fragment::default(), source.clone(), host.receiver);
                let output = self.inference.resolve(&signature.output);
                return self
                    .push_intrinsic(
                        item.clone(),
                        host.owner.clone(),
                        Some(&receiver),
                        &mut Vec::new(),
                        output,
                        signature.requirements,
                        call,
                    )
                    .map(Some);
            }
            _ => return Err(self.ambiguous_conversion(target, source, call, fitting.len())),
        }
        let Some((items, owner)) = self.target_from(target) else {
            return Ok(None);
        };
        let argument =
            self.operator_expr(origin::Fragment::default(), source.clone(), host.receiver);
        let fits = |parser: &mut Self, item: &crate::host::item::Definition| {
            parser
                .instantiate_host_signature(
                    item,
                    None,
                    Some(std::slice::from_ref(&argument)),
                    Span::NONE,
                )
                .ok()
                .filter(|signature| {
                    matches!(&signature.inputs[..], [input] if parser.unify_type(input, source))
                        && parser.unify_type(&signature.output, target)
                })
        };
        let mut fitting = Vec::new();
        for item in items
            .into_iter()
            .filter(|item| item.kind() == crate::host::item::Kind::AssociatedFunction)
        {
            let checkpoint = self.inference.checkpoint();
            if fits(self, &item).is_some() {
                fitting.push(item);
            }
            self.inference.rollback(checkpoint);
        }
        // Several take an unsuffixed literal: rustc's fallback, `i32` or
        // `f64`, chooses the one that takes it.
        if fitting.len() > 1
            && let Some(fallback) = literal_fallback(&self.inference.resolve(source))
        {
            let argument =
                self.operator_expr(origin::Fragment::default(), fallback.clone(), host.receiver);
            fitting.retain(|item| {
                let checkpoint = self.inference.checkpoint();
                let fits = self
                    .instantiate_host_signature(
                        item,
                        None,
                        Some(std::slice::from_ref(&argument)),
                        Span::NONE,
                    )
                    .is_ok_and(|signature| {
                        matches!(&signature.inputs[..], [input] if self.unify_type(input, &fallback))
                            && self.unify_type(&signature.output, target)
                    });
                self.inference.rollback(checkpoint);
                fits
            });
        }
        let item = match <[_; 1]>::try_from(fitting) {
            Ok([item]) => item,
            Err(fitting) if fitting.is_empty() => return Ok(None),
            Err(fitting) => {
                return Err(self.ambiguous_conversion(target, source, call, fitting.len()));
            }
        };
        // A value that borrows the text opens a scope frame (D558 tier 1),
        // which only a conversion whose target is known as it is parsed
        // opens ahead of its statement (`text_from_target`).
        if item.opens() && source.is_text() {
            let target = target.name(&self.models);
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "a `{target}` that borrows the text it converts, where only a later use \
                     gives the type, is not supported in Rawr yet; name the type where the \
                     conversion is written, by `T::from(..)`, `Into::<T>::into(..)` or a `let` \
                     annotation"
                )
                .into(),
                source: call,
            });
        }
        let signature =
            fits(self, &item).ok_or_else(|| self.ambiguous_conversion(target, source, call, 1))?;
        let output = self.inference.resolve(&signature.output);
        self.push_intrinsic(
            item,
            owner,
            None,
            &mut vec![argument],
            output,
            signature.requirements,
            call,
        )
        .map(Some)
    }

    /// The library type `target`'s `from` functions and their owner: at
    /// the path its alias names, or for an imported type no alias of the
    /// program names, as a field's `Background` is, at one of its crate's
    /// paths. `None` when neither has one.
    pub(super) fn target_from(&self, target: &Ty) -> Option<FromItems> {
        let Ty::Host { proof, .. } = target else {
            return None;
        };
        let path = format!("{}::from", proof.definition.path());
        #[cfg(feature = "rust-import")]
        let path = match self.imported_member_owner(target, "from") {
            Some(owner) if self.models.host_items(&path).is_none() => {
                format!("{}::from", owner.path())
            }
            _ => path,
        };
        let items = self.models.host_items(&path)?.to_vec();
        Some((items, self.models.item_owner(&path).cloned()))
    }

    /// rustc's E0277 for `source` converted into the library type
    /// `target`, none of whose `From` impls takes it: the bound, and the
    /// type each impl takes, as rustc lists the trait's other impls. `None`
    /// when `target` has no `from` to list.
    pub(super) fn unsatisfied_from(&mut self, target: &Ty, source: &Ty, at: Span) -> Option<Error> {
        let (items, _) = self.target_from(target)?;
        let mut takes = Vec::new();
        for item in items
            .iter()
            .filter(|item| item.kind() == crate::host::item::Kind::AssociatedFunction)
        {
            let checkpoint = self.inference.checkpoint();
            if let Ok(signature) = self.instantiate_host_signature(item, None, None, Span::NONE)
                && let [input] = &signature.inputs[..]
            {
                // Rawr's text is the `&str` the impl takes.
                let input = match self.inference.resolve(input) {
                    Ty::Str => "&str".into(),
                    input => input.name(&self.models),
                };
                takes.push(format!("          From<{input}>"));
            }
            self.inference.rollback(checkpoint);
        }
        if takes.is_empty() {
            return None;
        }
        takes.sort();
        takes.dedup();
        let target = target.name(&self.models);
        let bound = format!("From<{}>", source.name(&self.models));
        Some(Error::StdPath {
            code: "E0277",
            message: format!("the trait bound `{target}: {bound}` is not satisfied").into(),
            label: Some(format!("the trait `{bound}` is not implemented for `{target}`").into()),
            note: Some(
                format!(
                    "the following other types implement trait `From<T>`:\n{}",
                    takes.join("\n")
                )
                .into(),
            ),
            reference: at,
        })
    }

    /// The `from` functions of the library type `target` for a conversion
    /// of text of type `source` whose target the context gives as it is
    /// parsed (`Into::<T>::into(x)`, `<T as From<S>>::from(x)`, `let t: T
    /// = x.into()`): called as `T::from(x)` is, so the impl for the text's
    /// own type is selected as rustc selects it (`From<&'a str>` for a
    /// literal, `From<String>` for a `String`) and a value that borrows
    /// the text opens its scope frame ahead of the statement. `None` for
    /// another source or target. An imported target whose `from` the
    /// program never named is demanded, as `finish_conversions` demands it.
    pub(super) fn text_from_target(
        &mut self,
        source: &Ty,
        target: &Ty,
        call: Span,
    ) -> Result<Option<FromItems>, Error> {
        let target = self.inference.resolve(target);
        if !self.inference.resolve(source).is_text() || !matches!(target, Ty::Host { .. }) {
            return Ok(None);
        }
        let from = self.target_from(&target);
        if from.is_none() {
            self.demand_from(&target, call)?;
        }
        Ok(from)
    }

    /// rustc's E0283 for a conversion of `source` whose target the program
    /// leaves open, or that `impls` impls fit: at the local whose type it
    /// is, as rustc asks for that local's type, or at the call.
    pub(super) fn ambiguous_conversion(
        &self,
        target: &Ty,
        source: &Ty,
        call: Span,
        impls: usize,
    ) -> Error {
        let bound = format!("From<{}>", source.name(&self.models));
        let note = if impls > 1 {
            format!("multiple `impl`s satisfying `_: {bound}` found")
        } else {
            format!("the type must implement `{bound}`")
        };
        self.annotations_needed(target, note, call)
    }

    /// rustc's E0283 for a call whose types `target` leaves open, with
    /// `note`: at the local whose type holds them, as rustc asks for that
    /// local's type, or at the call.
    pub(super) fn annotations_needed(&self, target: &Ty, note: String, call: Span) -> Error {
        let target = self.inference.resolve(target);
        let open = |ty: &Ty| {
            matches!(
                ty,
                Ty::Infer(_) | Ty::Int(Int::Var(_)) | Ty::Float(Float::Var(_))
            ) && contains(&target, &|inner| inner == ty)
        };
        let local = self.local_types.iter().find_map(|local| {
            let ty = self.inference.resolve(&local.ty);
            contains(&ty, &open).then_some((local, ty))
        });
        match local {
            Some((local, ty)) => Error::LocalTypeNeeded {
                code: "E0283",
                name: local.name.clone(),
                ty: (!open(&ty)).then(|| ty.name(&self.models)),
                declaration: self.name_span(local.declaration, &local.name),
                needed: Some(call),
                note: Some(note.into()),
            },
            None => Error::StdPath {
                code: "E0283",
                message: "type annotations needed".into(),
                label: Some("type must be known at this point".into()),
                note: Some(note.into()),
                reference: call,
            },
        }
    }

    /// Whether the library type `target` has a `from` that takes `source`,
    /// as `impl From<f32> for Pixels` makes `f32: Into<Pixels>` hold. A
    /// literal of a kind still open takes the kind of the one `from` that
    /// fits it, as rustc selects the one impl: `50` is the `u32` of
    /// `From<u32>`. An imported type's `from` the program never named is
    /// demanded with the failure, as `finish_conversions` demands it.
    pub(super) fn host_from(&mut self, target: &Ty, source: &Ty) -> bool {
        let Ty::Host { proof, .. } = target else {
            return false;
        };
        let path = format!("{}::from", proof.definition.path());
        #[cfg(feature = "rust-import")]
        let path = match self.imported_member_owner(target, "from") {
            Some(owner) if self.models.host_items(&path).is_none() => {
                format!("{}::from", owner.path())
            }
            _ => path,
        };
        let Some(items) = self.models.host_items(&path).map(<[_]>::to_vec) else {
            #[cfg(feature = "rust-import")]
            if let Some(owner) = self.imported_public_path(target, "from") {
                self.recovered.push(Error::UnknownMember {
                    owner,
                    member: "from".into(),
                    reference: Span::NONE,
                });
            }
            return false;
        };
        let argument = self.operator_expr(origin::Fragment::default(), source.clone(), Span::NONE);
        let fits = |parser: &mut Self, item: &crate::host::item::Definition| {
            parser
                .instantiate_host_signature(
                    item,
                    None,
                    Some(std::slice::from_ref(&argument)),
                    Span::NONE,
                )
                .is_ok_and(|signature| {
                    matches!(&signature.inputs[..], [input] if parser.unify_type(input, source))
                        && parser.unify_type(&signature.output, target)
                })
        };
        let mut fitting = Vec::new();
        for item in items
            .iter()
            .filter(|item| item.kind() == crate::host::item::Kind::AssociatedFunction)
        {
            // A facade's number input takes each number of its kind.
            if item.imported_identity().is_none()
                && matches!(item.signature().parameters(), [parameter]
                    if host_number_accepts(parameter, source))
            {
                return true;
            }
            let checkpoint = self.inference.checkpoint();
            if fits(self, item) {
                fitting.push(item);
            }
            self.inference.rollback(checkpoint);
        }
        if let [item] = fitting[..] {
            fits(self, item);
        }
        !fitting.is_empty()
    }

    /// The helper of a `text.into()` into the library type `target`: the
    /// target's one `from` that takes the text's own type. `None` when none
    /// does. An imported target whose `from` the program never named is
    /// demanded, as `finish_conversions` demands it.
    pub(super) fn text_host_from(
        &mut self,
        into: &TextInto,
        target: &Ty,
    ) -> Result<Option<String>, Error> {
        let host = HostInto {
            forms: Vec::new(),
            owner: None,
            receiver: into.span,
        };
        let helper = self.host_into_helper(&host, &into.source, target, into.span)?;
        if helper.is_none() {
            self.demand_from(target, into.span)?;
        }
        Ok(helper)
    }

    /// The helper of a pending `.into()` of a value a std `From` may also
    /// take into the library type `target`, as `padding: [4, 0].into()` is
    /// a `Padding` through its `From<[u16; 2]>` while `Vec` takes any
    /// array: the target's one `from` that takes it. An imported target
    /// with no `from` yet, which the program never named, is demanded, as
    /// `finish_conversions` demands it.
    fn host_from_helper(
        &mut self,
        target: &Ty,
        conversion: &PendingConversion,
    ) -> Result<Option<String>, Error> {
        let host = HostInto {
            forms: Vec::new(),
            owner: None,
            receiver: conversion.span,
        };
        let source = self.inference.resolve(&conversion.source);
        let helper = self.host_into_helper(&host, &source, target, conversion.span)?;
        if helper.is_none() && self.target_from(target).is_none() {
            self.demand_from(target, conversion.span)?;
        }
        Ok(helper)
    }

    /// The `from` of the imported type `target` the program never named,
    /// demanded at `at` (`UnknownMember`), which the importer brings for
    /// the next pass. `Ok` when one of its paths has one.
    #[cfg_attr(not(feature = "rust-import"), allow(unused_variables))]
    pub(super) fn demand_from(&self, target: &Ty, at: Span) -> Result<(), Error> {
        #[cfg(feature = "rust-import")]
        if let Some(owner) = self.imported_public_path(target, "from") {
            return Err(Error::UnknownMember {
                owner,
                member: "from".into(),
                reference: at,
            });
        }
        Ok(())
    }

    /// The generic parameters `T` in scope with a bound `T: From<S>` that
    /// takes `source`, each with its `S`.
    pub(super) fn generic_from_sources(&mut self, source: &Ty) -> Vec<(ImmutableString, Ty)> {
        let bounds = self
            .generic_trait_arguments
            .iter()
            .filter(|((_, trait_name), _)| trait_name == "From")
            .flat_map(|((param, _), lists)| {
                lists
                    .iter()
                    .filter_map(|list| match &list[..] {
                        [from] => Some((param.clone(), from.clone())),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        bounds
            .into_iter()
            .filter(|(_, from)| self.unifies(from, source))
            .collect()
    }

    /// Choose each pending `.into()`'s `From` impl, now that its target is
    /// known, and write it over the placeholder (same length, so source
    /// locations stay exact).
    pub(super) fn finish_conversions(&mut self) -> Result<(), Error> {
        self.fall_back_diverging();
        for (target, item, collect) in std::mem::take(&mut self.pending_collects) {
            self.collected_into(&target, &item, collect)?;
        }
        // A `text.into()` no use gives a target: rustc's E0283, as std has
        // many `From<String>` impls. Its target is the text's own type, a
        // `String`, or a std type of std's `From` of text, which an `if`'s
        // other branch may give (`Cow<str>`); another is a mismatch.
        for into in std::mem::take(&mut self.text_into) {
            let helper = match self.inference.resolve(&into.target) {
                Ty::Infer(_) => {
                    return Err(Error::StdPath {
                        code: "E0283",
                        message: "type annotations needed".into(),
                        label: Some("type must be known at this point".into()),
                        note: None,
                        reference: into.span,
                    });
                }
                target if target == Ty::String || target == into.source => String::new(),
                target => match self.std_conversion(&target, &into.source) {
                    Some(op) => op
                        .map(|op| self.push_op(Op::Prelude(op)))
                        .unwrap_or_default(),
                    // A crate type's `From` of the text, as a component
                    // crate's `impl<'a, M> From<&'a str> for Slot<'a, M>`
                    // takes a literal and its `From<String>` a `String`: the
                    // impl for the text's own type, its parameters inferred
                    // from it, as rustc selects it.
                    None if matches!(target, Ty::Host { .. })
                        && let Some(helper) = self.text_host_from(&into, &target)? =>
                    {
                        helper
                    }
                    None => {
                        return Err(Error::TypeMismatch {
                            expected: target.name(&self.models),
                            actual: Ty::String.name(&self.models),
                            expression: into.span,
                        });
                    }
                },
            };
            self.write_over(&into.placeholder, &format!("{helper}("));
        }
        self.finish_formats()?;
        for (placeholder, text) in std::mem::take(&mut self.settled_parses) {
            self.write_over(&placeholder, &text);
        }
        for (target, error, span, placeholder) in std::mem::take(&mut self.pending_parses) {
            if let Some(helper) = self.settle_parse(&target, &error, span, true)? {
                self.write_over(&placeholder, &format!("{helper}("));
            }
        }
        // A crate value's `.into()` whose target holds a type only another
        // conversion settles waits for it, as rustc's obligations do: in
        // `row![badge![icon(), "x"].style(..)].into()`, the badge's own
        // `From` impl gives the icon's slot its message type. What no
        // conversion settles is selected last, and its error is rustc's.
        let mut pending = std::mem::take(&mut self.pending_conversions);
        let mut ready = Vec::with_capacity(pending.len());
        loop {
            let (waiting, settled): (Vec<_>, Vec<_>) =
                pending.into_iter().partition(|conversion| {
                    let target = self.inference.resolve(&conversion.target);
                    conversion.try_from.is_none()
                        && conversion.host.is_some()
                        && !matches!(target, Ty::Infer(_))
                        && contains_unsettled(&target)
                        && !self.unifies(&target, &conversion.source)
                });
            if settled.is_empty() {
                ready.extend(waiting);
                break;
            }
            pending = waiting;
            for conversion in settled {
                self.finish_conversion(conversion)?;
            }
            if pending.is_empty() {
                break;
            }
        }
        for conversion in ready {
            self.finish_conversion(conversion)?;
        }
        Ok(())
    }

    /// The pending `.into()` `conversion`'s `From` impl, chosen and written
    /// over its placeholder (`finish_conversions`).
    pub(super) fn finish_conversion(&mut self, conversion: PendingConversion) -> Result<(), Error> {
        {
            let target = self.inference.resolve(&conversion.target);
            let helper = if let Some(lending) = conversion.lending {
                self.lending_helper(lending, &conversion)?
            } else if let Some(error) = &conversion.try_from {
                self.try_from_helper(&target, &conversion.source, error, conversion.span)?
            } else if matches!(target, Ty::Infer(_)) {
                let bound = format!("From<{}>", conversion.source.name(&self.models));
                if let Some(error) = self.unsettled(&target, conversion.span, Some(bound)) {
                    return Err(error);
                }
                let source = conversion.source.name(&self.models);
                return Err(Error::InvalidMethodCall {
                    owner: source.clone(),
                    method: "into".into(),
                    reason: format!(
                        "type annotations needed: the type must implement `From<{source}>` (E0283)"
                    )
                    .into(),
                    call: conversion.span,
                });
            } else if self.unifies(&target, &conversion.source) {
                self.unify_type(&target, &conversion.source);
                String::new()
            } else if let Some(host) = &conversion.host {
                let source = self.inference.resolve(&conversion.source);
                match self.host_into_helper(host, &source, &target, conversion.span)? {
                    Some(helper) => helper,
                    // std's own `From`: `impl<T> From<T> for Option<T>`,
                    // `impl From<u16> for i64`.
                    None if let Ty::Option(inner) = &target
                        && self.unifies(inner, &source) =>
                    {
                        self.unify_type(inner, &source);
                        self.push_op(Op::Prelude(PreludeOp::Some))
                    }
                    None if let Some(op) = self.std_conversion(&target, &source) => op
                        .map(|op| self.push_op(Op::Prelude(op)))
                        .unwrap_or_default(),
                    None if let Some(error) =
                        self.conversion_not_yet(&target, &source, conversion.span) =>
                    {
                        return Err(error);
                    }
                    // An imported target whose `from` the program never
                    // named, as a field's `Background`: its member, which
                    // the importer brings once it is demanded.
                    #[cfg(feature = "rust-import")]
                    None if let Some(owner) = self.imported_public_path(&target, "from") => {
                        return Err(Error::UnknownMember {
                            owner,
                            member: "from".into(),
                            reference: conversion.span,
                        });
                    }
                    None => {
                        // A crate's `impl From<Degrees> for f32` gives the
                        // source an `into` of its own, asked for with the
                        // failure.
                        #[cfg(feature = "rust-import")]
                        if host.forms.is_empty() {
                            self.demand_into(&source);
                        }
                        if let Some(error) =
                            self.unsatisfied_from(&target, &source, conversion.span)
                        {
                            return Err(error);
                        }
                        let (source, target) =
                            (source.name(&self.models), target.name(&self.models));
                        return Err(Error::InvalidMethodCall {
                            owner: source.clone(),
                            method: "into".into(),
                            reason: format!(
                                "`{target}` has no `From<{source}>`, so `{source}` does not convert \
                                 into it (E0277)"
                            )
                            .into(),
                            call: conversion.span,
                        });
                    }
                }
            } else if let Ty::Param(param) = &target
                && let Some((_, from)) = self
                    .generic_from_sources(&conversion.source)
                    .into_iter()
                    .find(|(name, _)| name == param)
            {
                // `let t: T = x.into()` under `T: From<X>`: `T::from(x)`
                // through the witness of `T`.
                self.unify_type(&from, &conversion.source);
                let from = self.inference.resolve(&from);
                let index = self
                    .models
                    .trait_instances
                    .get("From")
                    .and_then(|instances| {
                        instances.iter().position(|instance| {
                            matches!(&instance[..], [instance] if Ty::from_resolved(instance) == from)
                        })
                    })
                    .unwrap_or(FALLBACK_INSTANCE);
                generic_conversion_call(param, index)
            } else if let Ty::Option(inner) = &target
                && self.unifies(inner, &conversion.source)
            {
                // `impl<T> From<T> for Option<T>`.
                self.unify_type(inner, &conversion.source);
                self.push_op(Op::Prelude(PreludeOp::Some))
            } else if let (
                Ty::Array {
                    element,
                    kind: sequence::Kind::Vec,
                },
                Ty::Array {
                    element: from,
                    kind: sequence::Kind::Array(_),
                },
            ) = (&target, self.inference.resolve(&conversion.source))
                && self.unifies(element, &from)
            {
                // `impl<T, const N: usize> From<[T; N]> for Vec<T>`.
                self.unify_type(element, &from);
                String::new()
            } else if let Some((definition, ..)) = match owner_id(&target) {
                Some(id) => {
                    self.select_conversion(id, Some(&target), &conversion.source, conversion.span)?
                }
                None => None,
            } {
                method_name(definition.id())
            } else if matches!(target, Ty::Host { .. })
                && let Some(helper) = self.host_from_helper(&target, &conversion)?
            {
                helper
            } else if let Some(op) = self.std_conversion(&target, &conversion.source) {
                op.map(|op| self.push_op(Op::Prelude(op)))
                    .unwrap_or_default()
            } else {
                if let Some(error) =
                    self.conversion_not_yet(&target, &conversion.source, conversion.span)
                {
                    return Err(error);
                }
                return Err(Error::MissingCapability {
                    owner: target.name(&self.models),
                    capability: format!("From<{}>", self.fallback_name(&conversion.source)).into(),
                    operation: conversion.span,
                    derive: matches!(target, Ty::Script(..)),
                    required_by: None,
                });
            };
            let helper = if helper.ends_with(", ") {
                helper
            } else {
                format!("{helper}(")
            };
            let replacement = format!("{helper:<width$}", width = conversion.placeholder.len());
            debug_assert_eq!(replacement.len(), conversion.placeholder.len());
            self.output
                .replace_same_length(&conversion.placeholder, &replacement);
            for closure in &mut self.lifted {
                closure
                    .body
                    .replace_same_length(&conversion.placeholder, &replacement);
            }
        }
        Ok(())
    }

    /// An argument for a parameter of type `T: Into<Length>`, where the
    /// library converts into `Length`: the converted value, so the function
    /// passes it on as a `Length`.
    pub(super) fn convert_into_host(
        &mut self,
        generics: &[item::Generic],
        parameter: &type_ref::Type,
        argument: &mut ParsedExpr,
    ) -> Result<(), Error> {
        let type_ref::Type::Path(type_ref::Path {
            name,
            target: type_ref::Target::Parameter,
            ..
        }) = parameter
        else {
            return Ok(());
        };
        let Some(target) = generics
            .iter()
            .find(|generic| generic.name.text == *name)
            .and_then(|generic| {
                generic
                    .trait_arguments
                    .iter()
                    .find(|(trait_name, _)| trait_name.as_str() == "Into")
            })
            .and_then(|(_, arguments)| arguments.first())
        else {
            return Ok(());
        };
        let Ty::Host { proof, .. } = self.inference.resolve(&Ty::from_resolved(target)) else {
            return Ok(());
        };
        let path = format!("{}::__into", proof.definition.path());
        let Some(into) = self.models.host_items(&path).map(<[_]>::to_vec) else {
            return Ok(());
        };
        if matches!(
            self.inference.resolve(&argument.ty),
            Ty::Param(_) | Ty::Projection(..) | Ty::Infer(_) | Ty::Unknown
        ) {
            return Ok(());
        }
        let owner = self.models.item_owner(&path).cloned();
        let span = argument.span;
        let ty = argument.ty.clone();
        let mut converted = self.call_host_function(&into, owner, vec![argument.clone()], span)?;
        // The call's own type stays the caller's, which the bound checks.
        converted.ty = ty;
        *argument = converted;
        Ok(())
    }

    /// `value.into()` is the identity, and the expected type at the use site
    /// coerces it as for any other value. A projection is not a `From`
    /// conversion: iced has no `From<widget>` for `Element`, so a widget's
    /// `.into()` is rejected as rustc rejects it (E0277).
    pub(super) fn parse_into(
        &mut self,
        mut value: ParsedExpr,
        end: Position,
    ) -> Result<ParsedExpr, Error> {
        let source = self.inference.resolve(&value.ty);
        if let Some(target) = self.widget_erasure(&source) {
            let call = span(value.span.start(), end);
            return Err(self.no_widget_conversion(&source, &target, "into", call));
        }
        value.span = span(value.span.start(), end);
        value.lvalue = None;
        // Text converts into the std type its `let` or caller expects
        // through std's `From` of it (`Cow<str>`).
        if source.is_text()
            && let Some(target) = self
                .collect_hint
                .as_ref()
                .filter(|(_, depth)| *depth == self.expression_stack.len())
                .map(|(target, _)| self.inference.resolve(target))
            && !matches!(target, Ty::String | Ty::Infer(_))
            && let Some(op) = self.std_conversion(&target, &source)
        {
            if let Some(op) = op {
                value.source = self.prelude_op(op, [value.rendered_source()]);
                value.prelude = origin::Fragment::default();
            }
            value.ty = target;
            return Ok(value.retain_origin());
        }
        // Text converts into the library type its `let` or caller expects
        // through that type's `From` of it, as `T::from(text)` does.
        if let Some(target) = self
            .collect_hint
            .as_ref()
            .filter(|(_, depth)| *depth == self.expression_stack.len())
            .map(|(target, _)| target.clone())
            && let Some((items, owner)) = self.text_from_target(&source, &target, value.span)?
        {
            let call = value.span;
            let mut converted = self.call_host_function(&items, owner, vec![value], call)?;
            self.coerce_expr(&target, &mut converted)?;
            return Ok(converted);
        }
        // Text converts into a `String`, through `From<&str>` or
        // `From<T> for T` (or into a boxed error, `box_returned_error`):
        // the target the use site gives, which rustc needs (E0283), or
        // one inference gives later: `placeholder(text))`, the
        // placeholder written as the conversion once it is known.
        if source.is_text() {
            let target = self.inference.fresh();
            let placeholder = text_into_placeholder(self.text_into.len());
            let mut rendered = origin::Fragment::call(&placeholder, [value.rendered_source()]);
            rendered.push_text(")");
            value.source = rendered;
            value.prelude = origin::Fragment::default();
            self.text_into.push(TextInto {
                span: value.span,
                source,
                target: target.clone(),
                placeholder,
            });
            value.ty = target;
        }
        Ok(value.retain_origin())
    }
}

/// A `text.into()` whose target the use site gives once inference has it:
/// a `String`, or a std type of std's `From` of text (`Cow<str>`), written
/// over the placeholder before the text's `(text)`.
pub(super) struct TextInto {
    pub(super) span: Span,
    pub(super) source: Ty,
    pub(super) target: Ty,
    pub(super) placeholder: String,
}

pub(super) struct PendingConversion {
    pub(super) placeholder: String,
    pub(super) source: Ty,
    pub(super) target: Ty,
    pub(super) span: Span,
    /// A facade value's `.into()`: its type's own `into` forms, chosen by
    /// their output, or the target's `from` of it.
    pub(super) host: Option<HostInto>,
    /// `.try_into()`: the error type of the `Result` it gives, the target's
    /// `TryFrom::Error` (#757).
    pub(super) try_from: Option<Ty>,
    /// `.as_ref()` or `.borrow()` of a native value: the std trait whose
    /// impl lends the target (`lending`).
    pub(super) lending: Option<super::lending::Lending>,
}

/// The `into` forms of a facade value's type, and the value's span.
/// A library type's `from` functions and the dependency that owns them.
pub(super) type FromItems = (
    Vec<crate::host::item::Definition>,
    Option<crate::host::facade::Dependency>,
);

pub(super) struct HostInto {
    pub(super) forms: Vec<crate::host::item::Definition>,
    pub(super) owner: Option<crate::host::facade::Dependency>,
    pub(super) receiver: Span,
}
