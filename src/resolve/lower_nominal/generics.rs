//! Generic items at one use: fresh type arguments, their bounds and type
//! witnesses, and the instances a call of a template binds.

use super::*;

impl<'a> Parser<'a> {
    /// A generic script type at one construction: fresh type arguments,
    /// and the substitution of its parameters.
    pub(super) fn fresh_nominal(
        &mut self,
        model: &Nominal,
        span: Span,
    ) -> (Ty, BTreeMap<ImmutableString, Ty>) {
        let generics = model.generics().to_vec();
        let substitution = self.fresh_generics(&generics, span);
        let arguments = generics
            .iter()
            .map(|generic| substitution[&generic.name.text].clone())
            .collect();
        (Ty::Script(model.definition().id(), arguments), substitution)
    }

    /// The substitution of a script type's parameters by `arguments`.
    pub(super) fn nominal_substitution(
        &self,
        id: type_def::Id,
        arguments: &[Ty],
    ) -> BTreeMap<ImmutableString, Ty> {
        self.models
            .definition(id)
            .generics()
            .iter()
            .zip(arguments.iter())
            .map(|(generic, argument)| (generic.name.text.clone(), argument.clone()))
            .collect()
    }

    /// Fresh types for `generics` at one use, with their bounds pending.
    pub(super) fn fresh_generics(
        &mut self,
        generics: &[item::Generic],
        span: Span,
    ) -> BTreeMap<ImmutableString, Ty> {
        let mut substitution = BTreeMap::new();
        for generic in generics {
            let fresh = self.inference.fresh();
            for kind in bound_kinds(generic.bounds) {
                self.pending_bounds.push((fresh.clone(), kind, span));
            }
            substitution.insert(generic.name.text.clone(), fresh);
        }
        // `U: Convert<V>`: the trait's arguments may name any parameter.
        // A trait bounding one parameter twice (`U: Convert<i64> +
        // Convert<String>`) is checked once per argument list.
        for generic in generics {
            let mut seen = BTreeSet::new();
            for trait_name in &generic.traits {
                if !seen.insert(trait_name.text.clone()) {
                    continue;
                }
                let mut lists = generic
                    .trait_arguments
                    .iter()
                    .filter(|(name, _)| *name == trait_name.text)
                    .map(|(_, arguments)| {
                        arguments
                            .iter()
                            .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                            .collect()
                    })
                    .collect::<Vec<Box<[Ty]>>>();
                if lists.is_empty() {
                    lists.push(Box::default());
                }
                for arguments in lists {
                    self.pending_traits.push((
                        substitution[&generic.name.text].clone(),
                        trait_name.text.clone(),
                        arguments,
                        span,
                    ));
                }
            }
        }
        // `T: Shape<Output = i64>`: the type `T` is inferred to be must
        // have that associated type.
        for generic in generics {
            for (trait_name, name, ty) in &generic.projections {
                let projection = Ty::Projection(
                    Box::new(substitution[&generic.name.text].clone()),
                    trait_name.text.clone(),
                    name.text.clone(),
                );
                let expected = instantiate(&Ty::from_resolved(ty), &substitution);
                self.pending_projections.push((projection, expected, span));
            }
        }
        substitution
    }

    /// The parameter types of the generic function `definition`, with
    /// each generic parameter a fresh type (the turbofish's, when one is
    /// written), whose bounds the call checks once it is read: what the
    /// arguments are read against, so a closure after an argument that
    /// fixes `T` has the parameter types its `Fn(&T)` bound gives.
    pub(super) fn generic_parameter_hints(&mut self, definition: &item::Function) -> Vec<Ty> {
        let substitution: BTreeMap<ImmutableString, Ty> = definition
            .generics
            .iter()
            .map(|generic| (generic.name.text.clone(), self.inference.fresh()))
            .collect();
        if let Some(arguments) = self.pending_turbofish.clone()
            && arguments.len() == definition.generics.len()
        {
            for (generic, argument) in definition.generics.iter().zip(arguments) {
                let fresh = substitution[&generic.name.text].clone();
                let checkpoint = self.inference.checkpoint();
                if self.unify_type(&fresh, &argument) {
                    self.inference.commit(checkpoint);
                } else {
                    self.inference.rollback(checkpoint);
                }
            }
        }
        definition
            .parameters
            .iter()
            .map(|parameter| instantiate(&Ty::from_resolved(&parameter.ty), &substitution))
            .collect()
    }

    /// `::<A, B>` after a generic function's name, or fresh types.
    pub(super) fn instantiate_generics(
        &mut self,
        generics: &[item::Generic],
        span: Span,
    ) -> Result<BTreeMap<ImmutableString, Ty>, Error> {
        let substitution = self.fresh_generics(generics, span);
        if let Some(arguments) = self.pending_turbofish.take() {
            if arguments.len() != generics.len() {
                return Err(Error::InvalidTypeArguments {
                    ty: "function".into(),
                    expected: generics.len(),
                    actual: arguments.len(),
                    reference: span,
                });
            }
            for (generic, argument) in generics.iter().zip(arguments) {
                let fresh = substitution[&generic.name.text].clone();
                self.require_type(&fresh, &argument, span)?;
            }
        }
        Ok(substitution)
    }

    /// Check each pending generic bound against its now-known type.
    pub(super) fn finish_bounds(&mut self) -> Result<(), Error> {
        for (ty, kind, span) in std::mem::take(&mut self.pending_bounds) {
            let ty = self.inference.resolve(&ty);
            if matches!(ty, Ty::Infer(_) | Ty::Unknown) {
                continue;
            }
            // rawr proves no auto trait, but refuses what it knows isn't
            // `Send`, as an `Rc`.
            let provided = match kind {
                capability::Kind::Send => !self.not_send(&ty),
                _ => self.type_provides(&ty, kind),
            };
            if !provided {
                return Err(Error::MissingCapability {
                    owner: ty.name(&self.models),
                    capability: kind.name().into(),
                    operation: span,
                    derive: matches!(ty, Ty::Script(..)),
                    required_by: None,
                });
            }
        }
        for (ty, trait_name, arguments, span) in std::mem::take(&mut self.pending_traits) {
            let ty = self.inference.resolve(&ty);
            if matches!(ty, Ty::Infer(_) | Ty::Unknown) {
                continue;
            }
            if !self.type_implements_with(&ty, &trait_name, &arguments) {
                if self.prelude_trait(&trait_name)
                    && let [argument] = &arguments[..]
                {
                    let (target, source) = match trait_name.as_str() {
                        "From" => (ty.clone(), argument.clone()),
                        _ => (argument.clone(), ty.clone()),
                    };
                    if let Some(error) = self.conversion_not_yet(&target, &source, span) {
                        return Err(error);
                    }
                    // rustc names the `From` impl an `Into` bound needs.
                    return Err(Error::MissingCapability {
                        owner: target.name(&self.models),
                        capability: format!("From<{}>", self.fallback_name(&source)).into(),
                        operation: span,
                        derive: false,
                        required_by: None,
                    });
                }
                // rustc names the innermost type std's impls needed it
                // of: `Vec<i32>` of `Rc<Vec<i32>>: Display`.
                let ty = match trait_name.as_str() {
                    "Display" if self.std_declared(&ty) => std_impl::holds(
                        &mut std_impl::Probe(self),
                        "core::fmt::Display",
                        &ty,
                        &[],
                        true,
                        0,
                    )
                    .err()
                    .map_or(ty, |(inner, _)| inner),
                    _ => ty,
                };
                // rustc writes the trait with its arguments, each of its
                // lifetimes as `'_`: `AsRef<str>`, `IntoContent<'_, i64>`.
                let lifetimes = self
                    .models
                    .traits
                    .get(&trait_name)
                    .map_or(0, |(_, declared)| declared.lifetimes);
                let capability = if arguments.is_empty() && lifetimes == 0 {
                    trait_name
                } else {
                    let written = std::iter::repeat_n(ImmutableString::from("'_"), lifetimes)
                        .chain(arguments.iter().map(|argument| {
                            match self.inference.resolve(argument) {
                                Ty::Str => "str".into(),
                                argument => argument.name(&self.models),
                            }
                        }))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{trait_name}<{written}>").into()
                };
                return Err(Error::MissingCapability {
                    owner: ty.name(&self.models),
                    capability,
                    operation: span,
                    derive: matches!(ty, Ty::Script(..)),
                    required_by: None,
                });
            }
        }
        for (projection, expected, span) in std::mem::take(&mut self.pending_projections) {
            if let Ty::Projection(base, ..) = &projection
                && matches!(self.inference.resolve(base), Ty::Infer(_) | Ty::Unknown)
            {
                continue;
            }
            self.require_type(&expected, &projection, span)?;
        }
        Ok(())
    }

    /// Placeholder arguments for the witnesses a call of an item declaring
    /// `generics` passes, instantiated by `substitution`.
    pub(super) fn witness_arguments(
        &mut self,
        generics: &[item::Generic],
        substitution: &BTreeMap<ImmutableString, Ty>,
        span: Span,
    ) -> Vec<origin::Fragment> {
        let mut arguments = Vec::new();
        // A parameter witnessed for its traits alone may take a reference,
        // which implements them only by forwarding to its referent's impls
        // (`resolve::forward`): the referent's witness serves.
        let names = witness_generics(generics, &self.models)
            .map(|generic| {
                (
                    generic.name.text.clone(),
                    !generic.host && !generic.bounds.contains(capability::Kind::Default),
                )
            })
            .collect::<Vec<_>>();
        // A call inside a closure may pass this body's own witnesses, which
        // the closure then captures.
        if !names.is_empty() {
            self.capture_witnesses();
        }
        for (name, forwarded) in names {
            let Some(ty) = substitution.get(&name) else {
                continue;
            };
            let placeholder = witness_placeholder(self.pending_witnesses.len());
            arguments.push(origin::Fragment::plain(placeholder.clone()));
            self.pending_witnesses
                .push((placeholder, name, ty.clone(), span, forwarded));
        }
        arguments
    }

    /// Whether this body's generic parameter `name` has a type witness.
    pub(super) fn has_witness(&self, name: &str) -> bool {
        self.host_witnesses.iter().any(|host| host.as_str() == name)
            || self
                .generics
                .get(name)
                .is_some_and(|bounds| bounds.contains(capability::Kind::Default))
            || self
                .generic_traits
                .get(name)
                .is_some_and(|traits| traits.iter().any(|name| static_trait(&self.models, name)))
    }

    /// Each enclosing closure captures this body's type witnesses, which a
    /// lifted closure would not have.
    pub(super) fn capture_witnesses(&mut self) {
        if self.closures.is_empty() {
            return;
        }
        let witnesses = self
            .generics
            .keys()
            .filter(|name| self.has_witness(name))
            .map(|name| ImmutableString::from(witness_variable(name)))
            .collect::<Vec<_>>();
        for scope in &mut self.closures {
            scope.captures.extend(witnesses.iter().cloned());
        }
    }

    /// The witness of a type a `Default`-bounded parameter takes: its script
    /// definition index, a negative code for a builtin, or the caller's own
    /// witness for one of its generic parameters.
    pub(super) fn witness(&self, ty: &Ty, parameter: &str, span: Span) -> Result<String, Error> {
        if let Some(code) = builtin_code(ty) {
            return Ok(code.to_string());
        }
        Ok(match ty {
            Ty::Script(id, _) => id.index().to_string(),
            Ty::Param(name) if self.has_witness(name) => witness_variable(name),
            Ty::Infer(_) => {
                return Err(Error::TypeAnnotationsNeeded {
                    parameter: parameter.into(),
                    reference: span,
                });
            }
            _ => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!(
                        "`{}` as the type argument of a generic call",
                        ty.name(&self.models)
                    )
                    .into(),
                    source: span,
                });
            }
        })
    }

    /// Write each pending witness over its placeholder (same length, so
    /// source locations stay exact).
    pub(super) fn finish_witnesses(&mut self) -> Result<(), Error> {
        self.settle_instance_literals();
        // Rust's fallback: an unconstrained literal is an `i32` or `f64`.
        for (id, integer) in std::mem::take(&mut self.literal_kinds) {
            let (kind, default) = if integer {
                (Ty::Int(Int::Var(id)), Ty::Int(Int::I32))
            } else {
                (Ty::Float(Float::Var(id)), Ty::Float(Float::F64))
            };
            if self.inference.resolve(&kind) == kind {
                self.inference.bind(id, default);
            }
        }
        for (id, value, at) in std::mem::take(&mut self.literal_values) {
            let value = Wide::Signed(value.into());
            if let Ty::Int(kind) = self.inference.resolve(&Ty::Int(Int::Var(id)))
                && value.fits(kind).is_err()
            {
                return Err(self.overflowing(kind, value, at));
            }
        }
        for pending in std::mem::take(&mut self.pending_wide_literals) {
            let kind = match self.inference.resolve(&pending.ty) {
                Ty::Int(Int::Var(_)) => Int::I32,
                Ty::Int(kind) => kind,
                _ => continue,
            };
            if pending.value.fits(kind).is_err() {
                return Err(self.overflowing(kind, pending.value, pending.at));
            }
            let [signed, unsigned, bits] = &pending.texts;
            let text = match kind {
                Int::I128 => signed,
                Int::U128 => unsigned,
                _ => bits,
            };
            let replacement = format!("{text:<width$}", width = pending.placeholder.len());
            self.output
                .replace_same_length(&pending.placeholder, &replacement);
            for closure in &mut self.lifted {
                closure
                    .body
                    .replace_same_length(&pending.placeholder, &replacement);
            }
        }
        for (placeholder, ty) in std::mem::take(&mut self.pending_int_kinds) {
            let code = match self.inference.resolve(&ty) {
                Ty::Int(Int::Var(_)) => kind_code(Ty::Int(Int::I32)),
                ty => kind_code(ty),
            };
            let replacement = format!("{code:<width$}", width = placeholder.len());
            self.output.replace_same_length(&placeholder, &replacement);
            for closure in &mut self.lifted {
                closure.body.replace_same_length(&placeholder, &replacement);
            }
        }
        for pending in std::mem::take(&mut self.pending_float_kinds) {
            let single = self.inference.resolve(&pending.ty) == Ty::Float(Float::F32);
            let (placeholder, replacement) = match pending.kind {
                PendingFloatKind::Literal(value) => {
                    let text = if single {
                        format!("{:?}", value as f32 as f64)
                    } else {
                        format!("{value:?}")
                    };
                    (pending.placeholder, text)
                }
                PendingFloatKind::Round(helper) if single => (pending.placeholder, helper),
                PendingFloatKind::Round(_) => (pending.placeholder, String::new()),
            };
            let replacement = format!("{replacement:<width$}", width = placeholder.len());
            self.output.replace_same_length(&placeholder, &replacement);
            for closure in &mut self.lifted {
                closure.body.replace_same_length(&placeholder, &replacement);
            }
        }
        self.finish_instances()?;
        self.finish_defaults()?;
        for (placeholder, parameter, ty, span, forwarded) in
            std::mem::take(&mut self.pending_witnesses)
        {
            let mut ty = self.inference.resolve(&ty);
            while forwarded && let Ty::Reference { target, .. } = &ty {
                ty = self.inference.resolve(target);
            }
            let witness = self.witness(&ty, &parameter, span)?;
            if witness.len() > placeholder.len() {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a generic parameter name this long as a `Default` witness".into(),
                    source: span,
                });
            }
            let replacement = format!("{witness:<width$}", width = placeholder.len());
            self.output.replace_same_length(&placeholder, &replacement);
            for closure in &mut self.lifted {
                closure.body.replace_same_length(&placeholder, &replacement);
            }
        }
        Ok(())
    }

    /// Write each `Default::default()` over its placeholder now that
    /// inference knows its type: the type's default, or the default
    /// dispatcher run with a generic parameter's witness.
    pub(super) fn finish_defaults(&mut self) -> Result<(), Error> {
        for (placeholder, ty, span) in std::mem::take(&mut self.pending_defaults) {
            let ty = self.inference.resolve(&ty);
            let source = match &ty {
                Ty::Infer(_) | Ty::Unknown => {
                    return Err(Error::InvalidMethodCall {
                        owner: "Default".into(),
                        method: "default".into(),
                        reason: "type annotations needed: cannot infer the type of \
                                 `Default::default()` (E0790)"
                            .into(),
                        call: span,
                    });
                }
                Ty::Param(name) => {
                    if !self.type_provides(&ty, capability::Kind::Default) {
                        return Err(Error::MissingCapability {
                            owner: name.clone(),
                            capability: "Default".into(),
                            operation: span,
                            derive: false,
                            required_by: None,
                        });
                    }
                    format!("{DEFAULT_DISPATCHER}({})", witness_variable(name))
                }
                ty => self.default_source(ty, span)?.as_str().to_string(),
            };
            if source.len() > placeholder.len() {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!(
                        "`Default::default()` for `{}` is not supported in Rawr yet; write \
                         `<{0}>::default()`",
                        ty.name(&self.models)
                    )
                    .into(),
                    source: span,
                });
            }
            let replacement = format!("{source:<width$}", width = placeholder.len());
            self.output.replace_same_length(&placeholder, &replacement);
            for closure in &mut self.lifted {
                closure.body.replace_same_length(&placeholder, &replacement);
            }
        }
        Ok(())
    }

    /// Write each generic trait call's instance dispatcher over its
    /// placeholder, now that inference knows the trait's arguments.
    /// Before the literal fallback, as Rust selects impls: a trait
    /// argument that holds a literal's `{integer}` or `{float}` (`&[{integer}]`
    /// of `Color::try_from(&v[..])`) takes the types of the one impl whose
    /// arguments it unifies with.
    pub(super) fn settle_instance_literals(&mut self) {
        for (_, trait_name, _, arguments, _, _, _) in self.pending_instances.clone() {
            let arguments = arguments
                .iter()
                .map(|ty| self.inference.resolve(ty))
                .collect::<Vec<_>>();
            if !arguments.iter().any(contains_open_literal) {
                continue;
            }
            let Some(instances) = self.models.trait_instances.get(&trait_name).cloned() else {
                continue;
            };
            let matching = instances
                .iter()
                .map(|instance| instance.iter().map(Ty::from_resolved).collect::<Vec<_>>())
                .filter(|instance| {
                    instance.len() == arguments.len()
                        && instance
                            .iter()
                            .zip(&arguments)
                            .all(|(instance, argument)| self.unifies(instance, argument))
                })
                .collect::<Vec<_>>();
            if let [instance] = &matching[..] {
                for (argument, ty) in arguments.iter().zip(instance) {
                    self.unify_type(ty, argument);
                }
            }
        }
    }

    pub(super) fn finish_instances(&mut self) -> Result<(), Error> {
        for (placeholder, trait_name, method, arguments, span, by_witness, self_ty) in
            std::mem::take(&mut self.pending_instances)
        {
            let arguments = arguments
                .iter()
                .map(|ty| self.inference.resolve(ty))
                .collect::<Vec<_>>();
            if arguments.iter().any(contains_inference) {
                let instance = arguments
                    .iter()
                    .map(|argument| match argument {
                        Ty::Infer(_) => "_".to_string(),
                        argument => argument.name(&self.models).to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let self_ty = match self.inference.resolve(&self_ty) {
                    Ty::Infer(_) => "_".to_string(),
                    self_ty => self_ty.name(&self.models).to_string(),
                };
                let note = format!(
                    "multiple `impl`s satisfying `{self_ty}: {trait_name}<{instance}>` found"
                );
                return Err(self.annotations_needed(
                    &Ty::Tuple(arguments.into_boxed_slice()),
                    note,
                    span,
                ));
            }
            let index = self
                .models
                .trait_instances
                .get(&trait_name)
                .and_then(|instances| {
                    instances.iter().position(|instance| {
                        instance.len() == arguments.len()
                            && instance.iter().zip(&arguments).all(|(instance, argument)| {
                                Ty::from_resolved(instance) == *argument
                            })
                    })
                })
                .unwrap_or(FALLBACK_INSTANCE);
            let replacement = if by_witness {
                static_instance_dispatcher_name(&trait_name, &method, index)
            } else {
                instance_dispatcher_name(&trait_name, &method, index)
            };
            self.output.replace_same_length(&placeholder, &replacement);
            for closure in &mut self.lifted {
                closure.body.replace_same_length(&placeholder, &replacement);
            }
        }
        Ok(())
    }

    /// `function`'s instance for the types `substitution` gives its generic
    /// parameters, when rawr compiles it per instantiation (D629). Each such
    /// call is recorded, so the instances its callers use are known.
    #[cfg(feature = "rust-import")]
    pub(super) fn bind_instance(
        &mut self,
        function: ScriptFunction,
        substitution: &BTreeMap<ImmutableString, Ty>,
        call: Span,
    ) -> Result<ScriptFunction, Error> {
        if !self.models.instances.templates.contains_key(&function.key) {
            return Ok(function);
        }
        // A type a crate trait's bound leaves open, as the renderer of an
        // argument `Container<Column<Element<Message, Theme, _>>>` for
        // `impl Widget<Message>`, is what that trait's defaults and the
        // crate's impls fix (D604). One that stays open has no instance.
        for generic in function.definition.generics.iter() {
            for bound in generic.crate_bounds.iter() {
                let _ = self.fix_opaque_in(bound, &substitution[&generic.name.text], substitution);
            }
        }
        // A literal argument takes the kind the one impl its bound reaches
        // gives, as rustc selects it, before its instance is chosen: `50`
        // for `impl Into<Pixels>` is the `u32` of `From<u32>`.
        let literals = self
            .pending_traits
            .iter()
            .filter(|(ty, ..)| contains_open_literal(&self.inference.resolve(ty)))
            .cloned()
            .collect::<Vec<_>>();
        for (ty, trait_name, arguments, _) in literals {
            self.type_implements_with(&ty, &trait_name, &arguments);
        }
        let types = function
            .definition
            .generics
            .iter()
            .map(|generic| substitution[&generic.name.text].clone())
            .collect::<Vec<_>>();
        self.instance_calls
            .push((function.key.clone(), types.clone(), call));
        let Some(instances) = self.models.instances.templates.get(&function.key) else {
            return Ok(function);
        };
        let resolved = types
            .iter()
            .map(|ty| self.inference.resolve(ty))
            .collect::<Vec<_>>();
        // A generic of this body's own reaching one compiled per
        // instantiation makes this body one too.
        self.reaches_host |= resolved.iter().any(reaches_param);
        let instance = self
            .called_instance(&resolved, call)
            .and_then(|names| {
                instances
                    .iter()
                    .find(|(written, _)| *written == names)
                    .map(|(_, key)| key.clone())
            })
            .inspect(|key| self.take_instance_types(key, &types))
            .and_then(|key| self.models.functions_by_key.get(&key).cloned());
        // While calls are collected, a call with no instance yet reads
        // on with its template's signature.
        if instance.is_none() && self.collecting_instances {
            return Ok(function);
        }
        instance.ok_or_else(|| {
            let generics = function
                .definition
                .generics
                .iter()
                .map(|generic| generic.name.text.as_str())
                .collect::<Vec<_>>();
            let name = &function.definition.name.text;
            Error::InvalidMethodCall {
                owner: ImmutableString::new(),
                method: name.clone(),
                reason: uninstantiated_call(name, &generics, &resolved, &self.models).into(),
                call,
            }
        })
    }

    /// The names of the types a call of a template gives its parameters:
    /// those its arguments fix, else those the end of this body resolves,
    /// as rustc infers a parameter from a later use of the call's value.
    #[cfg(feature = "rust-import")]
    pub(super) fn called_instance(
        &self,
        resolved: &[Ty],
        call: Span,
    ) -> Option<Box<[ImmutableString]>> {
        if let Some(names) = instance_names(resolved, &self.models) {
            return Some(names.into_boxed_slice());
        }
        let caller = self.callable.clone()?;
        self.models.instances.calls.get(&(caller, call)).cloned()
    }

    /// The call's types, `types` in its template's parameter order, are
    /// those of the instance `key` it binds to.
    #[cfg(feature = "rust-import")]
    pub(super) fn take_instance_types(&mut self, key: &crate::item::Key, types: &[Ty]) {
        let Some(named) = self.models.instances.types.get(key).cloned() else {
            return;
        };
        for (ty, (_, named)) in types.iter().zip(named.iter()) {
            let _ = self.unify_type(ty, &Ty::from_resolved(named));
        }
    }

    /// `definition`'s instance for the types `substitution` gives its impl's
    /// and its own generic parameters, when rawr compiles the method per
    /// instantiation (D629), as `bind_instance` binds a function's.
    #[cfg(feature = "rust-import")]
    pub(super) fn bind_method_instance(
        &mut self,
        definition: method::Definition,
        substitution: &BTreeMap<ImmutableString, Ty>,
        call: Span,
    ) -> Result<method::Definition, Error> {
        if !self
            .models
            .instances
            .templates
            .contains_key(definition.key())
        {
            return Ok(definition);
        }
        for generic in definition.generics() {
            for bound in generic.crate_bounds.iter() {
                let _ = self.fix_opaque_in(bound, &substitution[&generic.name.text], substitution);
            }
        }
        let types = definition
            .generics()
            .iter()
            .map(|generic| substitution[&generic.name.text].clone())
            .collect::<Vec<_>>();
        self.instance_calls
            .push((definition.key().clone(), types.clone(), call));
        let Some(instances) = self.models.instances.templates.get(definition.key()) else {
            return Ok(definition);
        };
        let resolved = types
            .iter()
            .map(|ty| self.inference.resolve(ty))
            .collect::<Vec<_>>();
        self.reaches_host |= resolved.iter().any(reaches_param);
        let instance = self
            .called_instance(&resolved, call)
            .and_then(|names| {
                instances
                    .iter()
                    .find(|(written, _)| *written == names)
                    .map(|(_, key)| key.clone())
            })
            .inspect(|key| self.take_instance_types(key, &types))
            .and_then(|key| self.models.method(definition.owner(), key.name()).cloned());
        // While calls are collected, a call with no instance yet reads on
        // with its template's signature.
        if instance.is_none() && self.collecting_instances {
            return Ok(definition);
        }
        instance.ok_or_else(|| {
            let generics = definition
                .generics()
                .iter()
                .map(|generic| generic.name.text.as_str())
                .collect::<Vec<_>>();
            Error::InvalidMethodCall {
                owner: definition.owner_name().into(),
                method: definition.name().into(),
                reason: uninstantiated_call(definition.name(), &generics, &resolved, &self.models)
                    .into(),
                call,
            }
        })
    }

    /// A generic impl's method at one call: fresh parameters, the owner's
    /// unified with the receiver's type arguments when there is a receiver.
    pub(super) fn method_substitution(
        &mut self,
        definition: &method::Definition,
        receiver: Option<&Ty>,
        call: Span,
    ) -> Result<BTreeMap<ImmutableString, Ty>, Error> {
        let substitution = self.fresh_generics(definition.generics(), call);
        // A blanket impl's owner is a parameter the receiver fixes.
        let blanket = definition.owner().builtin_code() == Some(crate::resolve::BLANKET_OWNER);
        if let Some(receiver) = receiver
            && (!definition.owner_arguments().is_empty() || blanket)
        {
            let receiver = match self.inference.resolve(receiver) {
                Ty::Reference { target, .. } => *target,
                ty => ty,
            };
            let owner = impl_self_ty(definition, &substitution);
            self.require_type(&owner, &receiver, call)?;
        }
        // `v.by_name::<String>(..)`: the method's own parameters take its
        // turbofish, as rustc's E0107 counts them.
        if let Some(arguments) = self.pending_turbofish.take() {
            let own = definition.own_generics();
            if arguments.len() != own.len() {
                return Err(Error::InvalidTypeArguments {
                    ty: "method".into(),
                    expected: own.len(),
                    actual: arguments.len(),
                    reference: call,
                });
            }
            for (generic, argument) in own.iter().zip(arguments) {
                let fresh = substitution[&generic.name.text].clone();
                self.require_type(&fresh, &argument, call)?;
            }
        }
        Ok(substitution)
    }
}

/// The generic parameter `ty` is, or refers to.
/// The generic parameter or opaque associated type `ty` is, or refers to:
/// a value with only its bounds' methods.
pub(super) fn bounded_value(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Param(_) | Ty::Projection(..) => Some(ty.clone()),
        Ty::Reference { target, .. } => bounded_value(target),
        _ => None,
    }
}

/// `ty` with each generic parameter in `substitution` replaced.
pub(super) fn instantiate(ty: &Ty, substitution: &BTreeMap<ImmutableString, Ty>) -> Ty {
    let each = |types: &[Ty]| -> Box<[Ty]> {
        types
            .iter()
            .map(|ty| instantiate(ty, substitution))
            .collect()
    };
    match ty {
        Ty::Param(name) => substitution
            .get(name)
            .cloned()
            .unwrap_or_else(|| ty.clone()),
        Ty::Option(element) => Ty::Option(Box::new(instantiate(element, substitution))),
        Ty::Boxed(target) => Ty::Boxed(Box::new(instantiate(target, substitution))),
        Ty::Counted(counter, target) => {
            Ty::Counted(*counter, Box::new(instantiate(target, substitution)))
        }
        Ty::Weak(counter, target) => {
            Ty::Weak(*counter, Box::new(instantiate(target, substitution)))
        }
        Ty::Iter(item, std) => Ty::Iter(Box::new(instantiate(item, substitution)), *std),
        Ty::Reverse(inner) => Ty::Reverse(Box::new(instantiate(inner, substitution))),
        Ty::Cow(inner) => Ty::Cow(Box::new(instantiate(inner, substitution))),
        Ty::Range(element) => Ty::Range(Box::new(instantiate(element, substitution))),
        Ty::InclusiveOf(element) => Ty::InclusiveOf(Box::new(instantiate(element, substitution))),
        Ty::Cell(kind, inner) => Ty::Cell(*kind, Box::new(instantiate(inner, substitution))),
        Ty::Thread(kind, inner) => Ty::Thread(*kind, Box::new(instantiate(inner, substitution))),
        Ty::Keyed(kind, arguments) => Ty::Keyed(
            *kind,
            arguments
                .iter()
                .map(|argument| instantiate(argument, substitution))
                .collect(),
        ),
        Ty::Dyn(trait_name, bindings) => Ty::Dyn(
            trait_name.clone(),
            bindings
                .iter()
                .map(|(name, ty)| (name.clone(), instantiate(ty, substitution)))
                .collect(),
        ),
        Ty::Result(ok, err) => Ty::Result(
            Box::new(instantiate(ok, substitution)),
            Box::new(instantiate(err, substitution)),
        ),
        Ty::Array { element, kind } => Ty::Array {
            element: Box::new(instantiate(element, substitution)),
            kind: *kind,
        },
        Ty::Tuple(members) => Ty::Tuple(each(members)),
        Ty::Reference { mutable, target } => Ty::Reference {
            mutable: *mutable,
            target: Box::new(instantiate(target, substitution)),
        },
        Ty::Fn { inputs, output } => Ty::Fn {
            inputs: each(inputs),
            output: Box::new(instantiate(output, substitution)),
        },
        Ty::FnItem {
            key,
            inputs,
            output,
        } => Ty::FnItem {
            key: key.clone(),
            inputs: each(inputs),
            output: Box::new(instantiate(output, substitution)),
        },
        Ty::Host { proof, arguments } => Ty::Host {
            proof: proof.clone(),
            arguments: each(arguments),
        },
        Ty::Script(id, arguments) => Ty::Script(*id, each(arguments)),
        Ty::Projection(base, trait_name, name) => Ty::Projection(
            Box::new(instantiate(base, substitution)),
            trait_name.clone(),
            name.clone(),
        ),
        _ => ty.clone(),
    }
}

/// The generic parameters bounded by `Default` or by a trait with a
/// function without `self`: each takes a hidden type witness, so
/// `T::default()` and `T::new()` reach a type erased at runtime.
pub(super) fn witness_generics<'a>(
    generics: &'a [item::Generic],
    models: &'a Models,
) -> impl Iterator<Item = &'a item::Generic> {
    generics.iter().filter(|generic| {
        generic.host
            || generic.bounds.contains(capability::Kind::Default)
            || generic
                .traits
                .iter()
                .any(|name| static_trait(models, &name.text))
    })
}

/// Whether a resolved type names a generic parameter.
pub(super) fn mentions_parameter(ty: &type_ref::Type) -> bool {
    match ty {
        type_ref::Type::Unit(_) => false,
        type_ref::Type::Tuple { members, .. } => members.iter().any(mentions_parameter),
        type_ref::Type::Reference { target, .. } => mentions_parameter(target),
        type_ref::Type::Path(path) => {
            matches!(
                path.target,
                type_ref::Target::Parameter | type_ref::Target::Projection(..)
            ) || path.arguments.iter().any(|argument| match argument {
                type_ref::Argument::Type(ty) => mentions_parameter(ty),
                type_ref::Argument::Lifetime(_) => false,
            })
        }
    }
}

/// Each generic parameter's generic trait bounds' arguments:
/// `(U, Convert)` to `[[i64]]` for `U: Convert<i64>`, and to
/// `[[i64], [String]]` for `U: Convert<i64> + Convert<String>`.
pub(super) fn generic_trait_arguments(
    generics: &[item::Generic],
) -> BTreeMap<(ImmutableString, ImmutableString), Vec<Box<[Ty]>>> {
    let mut all = BTreeMap::<_, Vec<Box<[Ty]>>>::new();
    for generic in generics {
        for (trait_name, arguments) in &generic.trait_arguments {
            all.entry((generic.name.text.clone(), trait_name.clone()))
                .or_default()
                .push(arguments.iter().map(Ty::from_resolved).collect());
        }
    }
    all
}

/// The positional trait arguments of a trait object, kept among its
/// bindings under the names `0`, `1`, ..: `i64` of `dyn Convert<i64>`.
pub(super) fn dyn_arguments(bindings: &[(ImmutableString, Ty)]) -> Vec<Ty> {
    let mut positional = bindings
        .iter()
        .filter_map(|(name, ty)| Some((name.parse::<usize>().ok()?, ty.clone())))
        .collect::<Vec<_>>();
    positional.sort_by_key(|(index, _)| *index);
    positional.into_iter().map(|(_, ty)| ty).collect()
}

/// The supertraits of the trait `name`, transitively.
pub(super) fn supertraits(models: &Models, name: &str) -> Vec<ImmutableString> {
    models
        .traits
        .get(name)
        .map(|(_, declared)| {
            declared
                .supertraits
                .1
                .iter()
                .map(|name| name.text.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the trait `name` has functions without `self`, which a generic
/// parameter bounded by it calls through its type witness.
pub(super) fn static_trait(models: &Models, name: &str) -> bool {
    models.traits.get(name).is_some_and(|(_, declared)| {
        declared
            .functions
            .iter()
            .any(|function| function.receiver.is_none())
    })
}

/// Every generic parameter list an item declares.
pub(super) fn item_generics(kind: &item::Kind) -> Vec<&item::Generic> {
    match kind {
        item::Kind::Struct(item) => item.generics.iter().collect(),
        item::Kind::Enum(item) => item.generics.iter().collect(),
        item::Kind::Function(item) => item.generics.iter().collect(),
        item::Kind::Impl(item) => item
            .generics
            .iter()
            .chain(
                item.functions
                    .iter()
                    .flat_map(|function| function.generics.iter()),
            )
            .collect(),
        item::Kind::Trait(item) => item
            .functions
            .iter()
            .flat_map(|function| function.generics.iter())
            .collect(),
        item::Kind::Use(_) => Vec::new(),
    }
}

/// `Iterator` and the traits whose `Item` is its own, which Rawr's iterator
/// type stands for.
pub(super) fn is_iterator_trait(name: &str) -> bool {
    matches!(
        name,
        "Iterator" | "DoubleEndedIterator" | "ExactSizeIterator"
    )
}

/// `T::Output = U` of each `T: Trait<Output = U>` in `generics`.
pub(super) fn generic_equalities(
    generics: &[item::Generic],
) -> BTreeMap<(ImmutableString, ImmutableString, ImmutableString), Ty> {
    generics
        .iter()
        .flat_map(|generic| {
            generic.projections.iter().map(|(trait_name, name, ty)| {
                (
                    (
                        generic.name.text.clone(),
                        trait_name.text.clone(),
                        name.text.clone(),
                    ),
                    Ty::from_resolved(ty),
                )
            })
        })
        .collect()
}

/// The user trait bounds of each generic parameter in `generics`.
/// The generic parameters a signature names inside a host type.
pub(super) fn host_witnesses(generics: &[item::Generic]) -> Vec<ImmutableString> {
    generics
        .iter()
        .filter(|generic| generic.host)
        .map(|generic| generic.name.text.clone())
        .collect()
}

pub(super) fn generic_traits(
    generics: &[item::Generic],
) -> BTreeMap<ImmutableString, Box<[ImmutableString]>> {
    generics
        .iter()
        .filter(|generic| !generic.traits.is_empty())
        .map(|generic| {
            (
                generic.name.text.clone(),
                generic
                    .traits
                    .iter()
                    .map(|name| name.text.clone())
                    .collect(),
            )
        })
        .collect()
}

/// The capabilities a bound set names.
pub(super) fn bound_kinds(bounds: capability::Set) -> impl Iterator<Item = capability::Kind> {
    [
        capability::Kind::Default,
        capability::Kind::Debug,
        capability::Kind::Clone,
        capability::Kind::Copy,
        capability::Kind::PartialEq,
        capability::Kind::Eq,
        // An `Rc` is not `Send` (rustc's E0277).
        capability::Kind::Send,
    ]
    .into_iter()
    .filter(move |kind| bounds.contains(*kind))
}

/// The head `dispatcher(witness, ` of `T::from(x)` for a generic target
/// `T` and its `From` instance `index`, written over a conversion
/// placeholder before the converted value's `(x))`.
pub(super) fn generic_conversion_call(param: &str, index: usize) -> String {
    format!(
        "{}({}, ",
        static_instance_dispatcher_name("From", "from", index),
        witness_variable(param)
    )
}

/// A trait call whose generic trait's dispatcher waits for inference:
/// `(placeholder, trait, method, arguments, span, static, Self)`.
pub(super) type PendingInstance = (
    String,
    ImmutableString,
    ImmutableString,
    Box<[Ty]>,
    Span,
    bool,
    Ty,
);

/// An impl's where-clause bounds at an instance (D604): each bounded
/// type, the bound's trait identity and the trait's arguments, then each
/// bound that fixes an associated type (`ImplProjection`).
#[cfg(feature = "rust-import")]
pub(super) struct ImplBounds {
    pub(super) traits: Vec<(Ty, [u8; 32], Vec<Ty>)>,
    pub(super) projections: Vec<ImplProjection>,
}

/// A bound `<ty as Trait>::Name == value` of an impl at an instance, with
/// the associated type each type `ty` may be gives (`facade::
/// ImplProjection`).
#[cfg(feature = "rust-import")]
#[derive(Debug)]
pub(super) struct ImplProjection {
    pub(super) ty: Ty,
    pub(super) value: Ty,
    pub(super) cases: Box<[(Ty, Ty)]>,
}

/// Why no impl fixed an `impl Trait`'s hidden type (D604): none fits the
/// type named, or more than one does.
#[cfg(feature = "rust-import")]
pub(super) enum Selection {
    None(String),
    Many(String),
}
