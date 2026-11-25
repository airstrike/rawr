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
        // `struct S<T>(T) where Vec<T>: Debug;`: the value checks the
        // clause as its parameters' bounds.
        let bounded = generics
            .iter()
            .chain(model.where_bounds())
            .cloned()
            .collect::<Vec<_>>();
        let substitution = self.fresh_generics(&bounded, span);
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

    /// The bounds of the item's type parameters `generics`, which its body
    /// knows them by.
    pub(super) fn read_generics(&mut self, generics: &[item::Generic]) {
        // A trait bound's supertraits' capabilities bound the parameter
        // too, as rustc elaborates them: `E: Error` is `Debug`.
        let elaborated = |generic: &item::Generic| {
            let implied = generic.traits.iter().filter_map(|name| {
                self.models
                    .traits
                    .get(&name.text)
                    .map(|(_, declared)| declared.supertraits.0)
            });
            capability::Set::from_explicit(
                std::iter::once(generic.bounds)
                    .chain(implied)
                    .flat_map(capability::Set::iter),
            )
        };
        self.generics = generics
            .iter()
            .map(|generic| (generic.name.text.clone(), elaborated(generic)))
            .collect();
        self.generic_traits = generic_traits(generics);
        self.generic_crate_bounds = generics
            .iter()
            .filter(|generic| !generic.crate_bounds.is_empty())
            .map(|generic| (generic.name.text.clone(), generic.crate_bounds.clone()))
            .collect();
        self.generic_trait_arguments = generic_trait_arguments(generics);
        self.inference.equalities = generic_equalities(generics);
        self.inference.trait_arguments = self
            .generic_trait_arguments
            .iter()
            .filter_map(|(key, lists)| match &lists[..] {
                [arguments] => Some((key.clone(), arguments.clone())),
                _ => None,
            })
            .collect();
    }

    /// The bounds of the opaque output `opaque` (D1109), which every body
    /// knows it by.
    pub(super) fn opaque_bounds(&self, opaque: &type_ref::Opaque) -> Option<&item::Generic> {
        self.models
            .opaque_outputs
            .get(opaque)
            .map(|output| &output.bounds)
    }

    /// The body of a function whose output `output` writes opaque types
    /// fills each with one hidden type, a fresh type whose bounds are
    /// pending as a call's type parameters' are (D1109): every `return`
    /// and the tail meet it, so a second hidden type is E0308.
    pub(super) fn define_opaque_outputs(&mut self, output: &type_ref::Type) {
        let ty = Ty::from_resolved(output);
        let outputs = Shared::clone(&self.models.opaque_outputs);
        let mut hidden = BTreeMap::new();
        for (opaque, opaque_output) in outputs.iter() {
            let arguments = std::cell::RefCell::new(None);
            let own = |ty: &Ty| match ty {
                Ty::Param(_, Parameter::Opaque(own)) if own.opaque == *opaque => {
                    *arguments.borrow_mut() = Some(own.arguments.clone());
                    true
                }
                _ => false,
            };
            if !contains(&ty, &own) {
                continue;
            }
            let generic = std::slice::from_ref(&opaque_output.bounds);
            let fresh = self.fresh_generics(generic, output.span());
            if let Some(fresh) = fresh.into_values().next() {
                hidden.insert(*opaque, (fresh, arguments.take().unwrap_or_default()));
            }
        }
        if hidden.is_empty() {
            return;
        }
        self.return_ty = Some(substitute(&ty, &|ty| match ty {
            Ty::Param(_, Parameter::Opaque(captured)) => {
                hidden.get(&captured.opaque).map(|(fresh, _)| fresh.clone())
            }
            _ => None,
        }));
        self.hidden_outputs = hidden;
    }

    /// An impl's method `member` of `trait_name` whose `output` refines an
    /// `impl Trait` the trait's signature writes to a type of its own (RFC
    /// 3245) proves the trait's opaque output's bounds of that type, as
    /// rustc does where the type is written.
    pub(super) fn prove_refinements(
        &mut self,
        trait_name: &str,
        member: &str,
        output: &type_ref::Type,
    ) {
        let Some(declared) = self
            .models
            .traits
            .get(trait_name)
            .and_then(|(_, declared)| {
                declared
                    .functions
                    .iter()
                    .find(|function| function.name.text == member)?
                    .output
                    .as_ref()
            })
        else {
            return;
        };
        let mut found = Vec::new();
        output.refinements(declared, &mut found);
        let refined = found
            .into_iter()
            .filter_map(|(written, opaque)| match &opaque.target {
                // Not its own `impl Trait`, which its body fills.
                type_ref::Target::Opaque(Some(opaque))
                    if !matches!(written, type_ref::Type::Path(own)
                        if matches!(own.target, type_ref::Target::Opaque(_))) =>
                {
                    let bounds = self.models.opaque_outputs.get(opaque)?.bounds.clone();
                    Some((Ty::from_resolved(written), written.span(), bounds))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (written, span, bounds) in refined {
            let fresh = self.fresh_generics(std::slice::from_ref(&bounds), span);
            if let Some(fresh) = fresh.into_values().next() {
                let fits = self.unify_type(&fresh, &written);
                debug_assert!(fits, "a fresh type unifies with any type");
            }
        }
    }

    /// The hidden type this body filled each of its function's opaque
    /// outputs with, and the opaque outputs its generic calls took as type
    /// arguments (D1109).
    pub(super) fn hidden(&mut self) -> Hidden {
        Hidden {
            filled: self
                .hidden_outputs
                .iter()
                .map(|(opaque, (hidden, arguments))| Filled {
                    opaque: *opaque,
                    hidden: self.inference.resolve(hidden),
                    arguments: arguments
                        .iter()
                        .map(|argument| self.inference.resolve(argument))
                        .collect(),
                })
                .collect(),
            witnessed: std::mem::take(&mut self.witnessed_opaques),
            sends: std::mem::take(&mut self.opaque_sends),
        }
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
                self.pending_bounds.push((fresh.clone(), kind, span, None));
            }
            substitution.insert(generic.name.text.clone(), fresh);
        }
        // `where Vec<T>: Debug`: the parameter lifted for the clause is
        // `Vec<T>` at this use, which its bounds then check.
        for generic in generics {
            if let Some(fixed) = &generic.fixed {
                let fixed = instantiate(&Ty::from_resolved(fixed), &substitution);
                let fits = self.unify_type(&substitution[&generic.name.text], &fixed);
                debug_assert!(fits, "a fresh type unifies with any type");
            }
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
                    self.pending_dispatches.push((
                        substitution[&generic.name.text].clone(),
                        trait_name.text.clone(),
                        arguments.clone(),
                        span,
                    ));
                    self.pending_traits.push((
                        substitution[&generic.name.text].clone(),
                        trait_name.text.clone(),
                        arguments,
                        span,
                        None,
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
            && arguments.len() == written(&definition.generics).count()
        {
            for (generic, argument) in written(&definition.generics).zip(arguments) {
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

    /// The type each argument of a call of the generic function
    /// `definition` is expected to have when the call is expected to be
    /// `expected`, as rustc's `expected_inputs_for_expected_output`: the
    /// output unified with the expectation in a probe, and each parameter's
    /// type the probe settles (`&str` for `f(&name)` as a `P<&str>`, of
    /// `fn f<T>(t: T) -> P<T>`). None for a parameter it leaves open.
    pub(super) fn expected_inputs(
        &mut self,
        definition: &item::Function,
        expected: &Ty,
    ) -> Vec<Option<Ty>> {
        let Some(output) = &definition.output else {
            return Vec::new();
        };
        let checkpoint = self.inference.checkpoint();
        let substitution: BTreeMap<ImmutableString, Ty> = definition
            .generics
            .iter()
            .map(|generic| (generic.name.text.clone(), self.inference.fresh()))
            .collect();
        let output = instantiate(&Ty::from_resolved(output), &substitution);
        let inputs = if self.unify_type(&output, expected) {
            definition
                .parameters
                .iter()
                .map(|parameter| {
                    let input = instantiate(&Ty::from_resolved(&parameter.ty), &substitution);
                    let input = self.inference.resolve(&input);
                    (!contains_unsettled(&input)).then_some(input)
                })
                .collect()
        } else {
            Vec::new()
        };
        self.inference.rollback(checkpoint);
        inputs
    }

    /// `argument` coerced to `input`, the type the call's expectation gives
    /// its parameter of type `formal`, which then takes that type, as rustc
    /// checks an argument against its expected input. An argument that
    /// doesn't coerce to it is left to the parameter's own coercion, and
    /// the mismatch is given back where its type is not `input`'s.
    pub(super) fn coerce_expected_input(
        &mut self,
        input: &Ty,
        formal: &Ty,
        argument: &mut ParsedExpr,
    ) -> Option<Error> {
        let checkpoint = self.inference.checkpoint();
        let mut coerced = argument.clone();
        match self.coerce_expr(input, &mut coerced) {
            Ok(_) if self.unify_type(formal, input) => {
                self.inference.commit(checkpoint);
                *argument = coerced;
                None
            }
            Ok(_) => {
                self.inference.rollback(checkpoint);
                None
            }
            Err(error) => {
                self.inference.rollback(checkpoint);
                Some(error)
            }
        }
    }

    /// The bounds `generics` write, from the `pending` ones on, as the
    /// call of `function` written `callee` needs its `arguments` to meet
    /// them: each at the argument whose parameter's type is the bounded
    /// generic.
    pub(super) fn place_bounds(
        &mut self,
        (bounds, traits): (usize, usize),
        function: &item::Function,
        callee: Span,
        arguments: &[ParsedExpr],
        substitution: &BTreeMap<ImmutableString, Ty>,
    ) {
        // The bound on the generic whose fresh type is `ty`, where
        // `written` finds it, at the first argument whose parameter's type
        // names that generic.
        let place = |this: &Self, ty: &Ty, written: &dyn Fn(&item::Generic) -> Option<Span>| {
            let generic = function
                .generics
                .iter()
                .find(|generic| substitution.get(&generic.name.text) == Some(ty))?;
            let bound = written(generic)?;
            // A parameter lifted for `where Vec<T>: Debug` is at the first
            // argument whose type names a parameter `Vec<T>` names.
            let named = generic
                .fixed
                .as_ref()
                .map_or(Ty::param(generic.name.text.clone()), Ty::from_resolved);
            let (_, argument) =
                function
                    .parameters
                    .iter()
                    .zip(arguments)
                    .find(|(declared, _)| {
                        contains(&Ty::from_resolved(&declared.ty), &|ty| {
                            matches!(ty, Ty::Param(..)) && contains(&named, &|named| named == ty)
                        })
                    })?;
            let place = crate::resolve::Bound {
                bound,
                callee: Some(callee),
                argument: this.whole_expression(argument.span),
                similar: None,
            };
            Some(crate::resolve::Required {
                item: function.name.text.clone(),
                place: Some(Box::new(place)),
            })
        };
        for index in bounds..self.pending_bounds.len() {
            let (ty, kind, ..) = &self.pending_bounds[index];
            let site = place(self, ty, &|generic| {
                let (_, bound) = generic
                    .written
                    .iter()
                    .find(|(written, _)| written == kind)?;
                Some(*bound)
            });
            self.pending_bounds[index].3 = site;
        }
        for index in traits..self.pending_traits.len() {
            let (ty, trait_name, ..) = &self.pending_traits[index];
            let site = place(self, ty, &|generic| {
                let name = generic
                    .traits
                    .iter()
                    .find(|name| name.text == *trait_name)?;
                Some(generic.trait_bound(name))
            });
            self.pending_traits[index].4 = site;
        }
    }

    /// A trait `ty` implements whose last segment is the unmet trait
    /// `trait_name`'s, as rustc's E0277 notes it: "`A` implements
    /// similarly named trait `std::fmt::Write`, but not `std::io::Write`".
    fn similar_trait(&self, ty: &Ty, trait_name: &str) -> Option<ImmutableString> {
        let last = trait_name.rsplit("::").next()?;
        let std = crate::std_table::traits_named(last)
            .iter()
            .filter_map(|definition| {
                let name = crate::resolve::std_trait::known(definition);
                let holds = || {
                    std_impl::holds(&mut std_impl::Probe(self), definition, ty, &[], true, 0)
                        .is_ok()
                };
                (name != trait_name && holds()).then_some(name)
            });
        let program = self
            .models
            .traits
            .iter()
            .filter(|(name, (_, declared))| {
                declared.std.is_none()
                    && name.rsplit("::").next() == Some(last)
                    && *name != trait_name
                    && self.type_implements(ty, name)
            })
            .map(|(name, _)| name.as_str());
        std.chain(program)
            .next()
            .map(|name| trimmed::trait_name(name.into(), &self.models))
    }

    /// The parameter a `where` clause in scope bounds `ty` as (`Vec<T>` of
    /// `where Vec<T>: Debug`), whose bounds `ty` has in this body.
    pub(super) fn assumed(&self, ty: &Ty) -> Option<Ty> {
        if self.assumed.is_empty() {
            return None;
        }
        let ty = self.inference.resolve(ty);
        self.assumed
            .iter()
            .find(|(fixed, _)| *fixed == ty)
            .map(|(_, name)| Ty::param(name.clone()))
    }

    /// The parameter a `where` clause bounds `ty` as, when it has what
    /// `holds` asks and `ty` has it only by the clause: a value whose own
    /// type has it is written by its type.
    pub(super) fn assumed_for(
        &mut self,
        ty: &Ty,
        holds: impl Fn(&Self, &Ty) -> bool,
    ) -> Option<Ty> {
        let parameter = self
            .assumed(ty)
            .filter(|parameter| holds(self, parameter))?;
        let assumed = std::mem::take(&mut self.assumed);
        let own = holds(self, ty);
        self.assumed = assumed;
        (!own).then_some(parameter)
    }

    /// The name of the program's type `ty` where it is declared, when that
    /// is in the file being lowered.
    pub(super) fn declared_name(&self, ty: &Ty) -> Option<Span> {
        let Ty::Script(id, _) = ty else {
            return None;
        };
        self.models.name_declared_here(self.models.definition(*id))
    }

    /// `::<A, B>` after a generic function's name, or fresh types.
    pub(super) fn instantiate_generics(
        &mut self,
        generics: &[item::Generic],
        span: Span,
    ) -> Result<BTreeMap<ImmutableString, Ty>, Error> {
        let substitution = self.fresh_generics(generics, span);
        if let Some(arguments) = self.pending_turbofish.take() {
            let expected = written(generics).count();
            if arguments.len() != expected {
                return Err(generic_count("function", expected, arguments.len(), span));
            }
            for (generic, argument) in written(generics).zip(arguments) {
                let fresh = substitution[&generic.name.text].clone();
                self.require_type(&fresh, &argument, span)?;
            }
        }
        Ok(substitution)
    }

    /// Check each pending generic bound against its now-known type.
    pub(super) fn finish_bounds(&mut self) -> Result<(), Error> {
        // A template's instance is chosen by its types, which the impls its
        // bounds select fix before a literal falls back (D1115).
        if !self.pending_template_calls.is_empty() {
            self.fix_by_impls();
        }
        // An opaque output's hidden literal is its fallback kind before its
        // bounds are proved: `impl Display` of `42` is an `i32`.
        let hidden_outputs = self
            .hidden_outputs
            .values()
            .map(|(hidden, _)| hidden.clone());
        for hidden in hidden_outputs.collect::<Vec<_>>() {
            if let Some(fallback) = literal_fallback(&self.inference.resolve(&hidden)) {
                let fits = self.unify_type(&hidden, &fallback);
                debug_assert!(fits, "a literal unifies with its fallback kind");
            }
        }
        for (ty, kind, span, site) in std::mem::take(&mut self.pending_bounds) {
            let ty = self.inference.resolve(&ty);
            if matches!(ty, Ty::Infer(_) | Ty::Unknown) {
                continue;
            }
            // rawr proves no auto trait, but refuses what it knows isn't
            // `Send`, as an `Rc`. An opaque output is `Send` as its hidden
            // type is, known once every body is checked (D1109).
            if kind == capability::Kind::Send {
                if contains(&ty, &Ty::is_opaque) {
                    self.opaque_sends.push(super::entry::HiddenSend {
                        ty: ty.clone(),
                        at: span,
                        closure: None,
                        item: site.as_ref().map(|site| site.item.clone()),
                    });
                }
                if let Some(unsendable) = self.unsendable(&ty, &|_| None) {
                    return Err(unsendable.error(&self.models, span, site));
                }
                continue;
            }
            if !self.type_provides(&ty, kind) {
                return Err(self.unmet_bound(&ty, Obligation::Capability(kind), span, site));
            }
        }
        for (ty, trait_name, arguments, span, site) in std::mem::take(&mut self.pending_traits) {
            let ty = self.inference.resolve(&ty);
            if matches!(ty, Ty::Infer(_) | Ty::Unknown) {
                continue;
            }
            // A `From` or `Into` bound selects std's impl, whose types fix
            // what the converted value leaves open.
            if self.prelude_trait(&trait_name)
                && let [argument] = &arguments[..]
            {
                match trait_name.as_str() {
                    "From" => self.settle_conversion(&ty, argument),
                    _ => self.settle_conversion(argument, &ty),
                }
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
                        declared: None,
                        required_by: None,
                        formatted: None,
                        chain: crate::resolve::Chain::default(),
                    });
                }
                // The innermost obligation the program's impls needed:
                // `Opaque: Debug` of `Wrapper<Opaque>: Summary`.
                if arguments.is_empty()
                    && let Ty::Script(id, _) = &ty
                    && self.models.implements(&trait_name, *id)
                {
                    return Err(self.unmet_bound(&ty, Obligation::Trait(trait_name), span, site));
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
                // lifetimes as `'_`: `AsRef<str>`, `IntoView<'_, i64>`.
                let lifetimes = self
                    .models
                    .traits
                    .get(&trait_name)
                    .map_or(0, |(_, declared)| declared.lifetimes);
                let mut site = site;
                if let Some(place) = site.as_mut().and_then(|site| site.place.as_mut()) {
                    place.similar = self.similar_trait(&ty, &trait_name);
                }
                let capability = if arguments.is_empty() && lifetimes == 0 {
                    trimmed::trait_name(trait_name, &self.models)
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
                    declared: self.declared_name(&ty),
                    required_by: site,
                    formatted: None,
                    chain: crate::resolve::Chain::default(),
                });
            }
        }
        for (ty, trait_name, arguments, span) in std::mem::take(&mut self.pending_dispatches) {
            if self.binds_instance(span) {
                continue;
            }
            if let Some(error) = self.untold_impls(&ty, &trait_name, &arguments, span) {
                return Err(error);
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
        let names = witness_generics(generics, &self.models)
            .map(|generic| (generic.name.text.clone(), Need::of(generic)))
            .collect::<Vec<_>>();
        // A call inside a closure may pass this body's own witnesses, which
        // the closure then captures.
        if !names.is_empty() {
            self.capture_witnesses();
        }
        for (name, need) in names {
            let Some(ty) = substitution.get(&name) else {
                continue;
            };
            let placeholder = witness_placeholder(self.pending_witnesses.len());
            arguments.push(origin::Fragment::plain(placeholder.clone()));
            self.pending_witnesses
                .push((placeholder, name, ty.clone(), span, need));
        }
        arguments
    }

    /// Whether this body's generic parameter `name` has a type witness.
    pub(super) fn has_witness(&self, name: &str) -> bool {
        self.host_witnesses.iter().any(|host| host.as_str() == name)
            || self.constants.contains_key(name)
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
    /// id's number, by its key, a negative code for a builtin (an integer
    /// kind's or `f32`'s own, `witness_code`), or the caller's own witness
    /// for one of its generic parameters.
    pub(super) fn witness(&self, ty: &Ty, parameter: &str, span: Span) -> Result<String, Error> {
        if let Some(code) = witness_code(ty) {
            return Ok(code.to_string());
        }
        Ok(match ty {
            Ty::Script(id, _) => id.number().to_string(),
            // A const argument's witness is its value.
            Ty::Const(value) => value.to_string(),
            Ty::Param(name, Parameter::Declared) if self.has_witness(name) => {
                witness_variable(name)
            }
            // An opaque output's witness is its hidden type's, which the
            // generated text reads once every body is checked (D1109),
            // given its type arguments' for a hidden type that is one.
            Ty::Param(_, Parameter::Opaque(captured)) => {
                let [source, line, column, index] = captured.opaque.numbers();
                let arguments = self
                    .opaque_argument_witnesses(&captured.arguments, parameter, span)
                    .iter()
                    .map(|witness| witness.as_deref().unwrap_or("()"))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{HIDDEN_DISPATCHER}({source},{line},{column},{index},[{arguments}])")
            }
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

    /// The witness of each type argument of an opaque output at a call,
    /// which its hidden type's is where it is one of them (D1109): `None`
    /// for one with none, or itself an opaque output.
    fn opaque_argument_witnesses(
        &self,
        arguments: &[Ty],
        parameter: &str,
        span: Span,
    ) -> Box<[Option<String>]> {
        arguments
            .iter()
            .map(|argument| {
                let argument = self.inference.resolve(argument);
                if argument.is_opaque() {
                    return None;
                }
                self.witness(&argument, parameter, span).ok()
            })
            .collect()
    }

    /// Write each pending witness over its placeholder (same length, so
    /// source locations stay exact).
    pub(super) fn finish_witnesses(&mut self) -> Result<(), Error> {
        self.settle_instance_literals();
        self.settle_objects();
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
        self.finish_objects()?;
        #[cfg(feature = "rust-import")]
        self.finish_stand_ins()?;
        self.finish_template_calls();
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
        for pending in std::mem::take(&mut self.pending_operands) {
            let super::operator::PendingOperands {
                placeholder,
                ty,
                at,
            } = pending;
            let code = match self.inference.resolve(&ty) {
                Ty::Int(Int::Var(_)) => kind_code(Ty::Int(Int::I32)),
                Ty::Int(kind) if !kind.wide() && !matches!(kind, Int::U64 | Int::Usize) => {
                    kind_code(Ty::Int(kind))
                }
                Ty::Float(Float::Var(_)) => kind_code(Ty::Float(Float::F64)),
                ty @ Ty::Float(_) => kind_code(ty),
                ty => {
                    return Err(Error::TypeMismatch {
                        expected: Ty::Int(Int::I64).name(&self.models),
                        actual: ty.name(&self.models),
                        expression: at,
                    });
                }
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
        for (placeholder, parameter, ty, span, need) in std::mem::take(&mut self.pending_witnesses)
        {
            let mut ty = self.inference.resolve(&ty);
            while need != Need::Type
                && let Ty::Reference { target, .. } = &ty
            {
                ty = self.inference.resolve(target);
            }
            // A template call read for its instance has none to witness.
            let witness = match self.witness(&ty, &parameter, span) {
                Err(_) if self.collecting_instances => continue,
                witness => witness?,
            };
            if let Ty::Param(_, Parameter::Opaque(captured)) = &ty {
                let arguments = self
                    .opaque_argument_witnesses(&captured.arguments, &parameter, span)
                    .iter()
                    .map(Option::is_some)
                    .collect();
                self.witnessed_opaques.push(Witnessed {
                    opaque: captured.opaque,
                    span,
                    arguments,
                });
            }
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
                Ty::Param(name, _) => {
                    if !self.type_provides(&ty, capability::Kind::Default) {
                        return Err(Error::MissingCapability {
                            owner: name.clone(),
                            capability: "Default".into(),
                            operation: span,
                            derive: false,
                            declared: None,
                            required_by: None,
                            formatted: None,
                            chain: crate::resolve::Chain::default(),
                        });
                    }
                    format!("{DEFAULT_DISPATCHER}({})", witness_variable(name))
                }
                ty => self.default_source(ty, span)?.as_str().to_string(),
            };
            if source.len() > placeholder.len() {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: crate::resolve::Unsupported::said(format!(
                        "`Default::default()` for `{}` is not supported in Rawr yet",
                        ty.name(&self.models)
                    ))
                    .with_help(format!("write `<{}>::default()`", ty.name(&self.models))),
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
        self.fix_by_impls();
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
            let name = &function.definition.name.text;
            Error::InvalidMethodCall {
                owner: ImmutableString::new(),
                method: name.clone(),
                reason: uninstantiated_call(
                    name,
                    &function.definition.generics,
                    &resolved,
                    &self.models,
                )
                .into(),
                call,
            }
        })
    }

    /// Fix what the one impl a pending bound reaches gives, before a call's
    /// instance is chosen, as rustc selects that impl. A literal argument
    /// takes its kind: `50` for `impl Into<Pixels>` is the `u32` of
    /// `From<u32>`. A bound's argument takes the type the impl for a known
    /// type gives: `M` of `impl IntoView<'a, M>` given a `Content<Message>`
    /// is `Message` by `impl<M> IntoView<'_, M> for Content<M>`.
    pub(super) fn fix_by_impls(&mut self) {
        let open = self
            .pending_traits
            .iter()
            .filter(|(ty, _, arguments, ..)| {
                let ty = self.inference.resolve(ty);
                contains_open_literal(&ty)
                    || !is_unsettled(&ty)
                        && arguments.iter().any(|argument| {
                            let argument = self.inference.resolve(argument);
                            contains_inference(&argument) || contains_open_literal(&argument)
                        })
            })
            .cloned()
            .collect::<Vec<_>>();
        for (ty, trait_name, arguments, ..) in open {
            let resolved = self.inference.resolve(&ty);
            // A literal argument of a program type takes what its impl's
            // arguments give it: `Holder<{integer}>: Convert<i64>` by
            // `impl<T> Convert<T> for Holder<T>` holds an `i64`.
            // `7` for `T: Into<Wrapper>`, or `S` of `T: From<S>` at
            // `Wrapper`, is the `i64` of Wrapper's one `From` impl a literal
            // fits, as rustc selects it.
            if let [argument] = &arguments[..] {
                let (target, source) = match trait_name.as_str() {
                    "Into" => (argument.clone(), resolved.clone()),
                    "From" => (resolved.clone(), argument.clone()),
                    _ => (Ty::Unknown, Ty::Unknown),
                };
                let source = self.inference.resolve(&source);
                if contains_open_literal(&source)
                    && let Some(target) = owner_id(&self.inference.resolve(&target))
                    && self.fix_conversion_source(target, &source)
                {
                    continue;
                }
            }
            if contains_open_literal(&resolved) && !matches!(resolved, Ty::Script(..)) {
                self.type_implements_with(&ty, &trait_name, &arguments);
                continue;
            }
            let Some(owner) = owner_id(&resolved) else {
                continue;
            };
            let candidates = self.impl_candidates(&trait_name, owner, owner_arguments(&resolved));
            let mut fitting = Vec::new();
            for candidate in candidates {
                if candidate.len() == arguments.len()
                    && candidate
                        .iter()
                        .zip(arguments.iter())
                        .all(|(candidate, argument)| self.unifies(candidate, argument))
                {
                    fitting.push(candidate);
                }
            }
            if let [candidate] = &fitting[..] {
                for (candidate, argument) in candidate.iter().zip(arguments.iter()) {
                    let _ = self.unify_type(candidate, argument);
                }
            }
        }
    }

    /// Give `source`, an open literal's type, the source type of the one
    /// `From` impl of `target` it fits, if one alone does.
    fn fix_conversion_source(&mut self, target: type_def::Id, source: &Ty) -> bool {
        let sources = self
            .models
            .conversions
            .get(&target)
            .into_iter()
            .flatten()
            .filter_map(|method| method.conversion().map(Ty::from_resolved))
            .collect::<Vec<_>>()
            .into_iter()
            .filter(|from| self.unifies(from, source))
            .collect::<Vec<_>>();
        let [from] = &sources[..] else {
            return false;
        };
        self.unify_type(from, source)
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
        self.fix_by_impls();
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
        instance.ok_or_else(|| Error::InvalidMethodCall {
            owner: definition.owner_name().into(),
            method: definition.name().into(),
            reason: uninstantiated_call(
                definition.name(),
                definition.generics(),
                &resolved,
                &self.models,
            )
            .into(),
            call,
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
            // A trait impl's parameter only its trait's arguments name
            // takes what the body's own bound on the receiver gives.
            #[cfg(feature = "rust-import")]
            if let Some(identity) = self.script_impl_trait(definition) {
                let arguments = definition
                    .trait_arguments()
                    .iter()
                    .map(|argument| instantiate(&Ty::from_resolved(argument), &substitution))
                    .collect::<Vec<_>>();
                self.fix_by_param_env(&receiver, identity, &arguments);
            }
        }
        // `v.by_name::<String>(..)`: the method's own parameters take its
        // turbofish, as rustc's E0107 counts them.
        if let Some(arguments) = self.pending_turbofish.take() {
            let expected = written(definition.own_generics()).count();
            if arguments.len() != expected {
                return Err(generic_count("method", expected, arguments.len(), call));
            }
            for (generic, argument) in written(definition.own_generics()).zip(arguments) {
                let fresh = substitution[&generic.name.text].clone();
                self.require_type(&fresh, &argument, call)?;
            }
        }
        Ok(substitution)
    }

    /// The identity of the crate trait whose program impl `definition` is
    /// a method of, by the impl's full trait path (`Models::script_impls`).
    #[cfg(feature = "rust-import")]
    fn script_impl_trait(&self, definition: &method::Definition) -> Option<[u8; 32]> {
        let written = definition.trait_name()?;
        let last = written.rsplit("::").next().unwrap_or(written);
        let mut identities = self
            .models
            .script_impls
            .iter()
            .filter(|candidate| {
                matches!(&candidate.owner, type_ref::Type::Path(type_ref::Path {
                    target: type_ref::Target::Script { definition: owner, .. },
                    ..
                }) if *owner == definition.owner())
                    && candidate.path.rsplit("::").next() == Some(last)
            })
            .filter_map(|candidate| {
                let path = crate::host::path::Path::parse(&candidate.path).ok()?;
                Some(self.models.host.trait_header(&path)?.identity)
            })
            .collect::<BTreeSet<_>>()
            .into_iter();
        match (identities.next(), identities.next()) {
            (Some(identity), None) => Some(identity),
            _ => None,
        }
    }

    /// rustc's E0599 for an inherent method whose impl's bounds the
    /// receiver's type arguments don't meet, at `substitution`: the probe
    /// passes the method by (`S(Opaque).show()` of `impl<T: Debug> S<T>`).
    /// An argument inference has not settled is the bound's to check later.
    pub(super) fn impl_bounds_met(
        &self,
        definition: &method::Definition,
        substitution: &BTreeMap<ImmutableString, Ty>,
        receiver: &Ty,
        member_span: Span,
    ) -> Result<(), Error> {
        if definition.trait_name().is_some() {
            return Ok(());
        }
        let generics = definition.generics();
        let impl_generics = &generics[..generics.len() - definition.own_generics().len()];
        let mut bounds = Vec::new();
        for generic in impl_generics {
            let Some(ty) = substitution.get(&generic.name.text) else {
                continue;
            };
            let ty = self.inference.resolve(ty);
            if contains_inference(&ty) || contains(&ty, &|ty| matches!(ty, Ty::Param(..))) {
                continue;
            }
            let unmet = |trait_name: ImmutableString, bound: Span| crate::resolve::UnmetBound {
                ty: ty.name(&self.models),
                derive: matches!(ty, Ty::Script(..)) && crate::resolve::derivable(&trait_name),
                trait_name,
                bound,
                declared: self.declared_name(&ty),
            };
            for (kind, bound) in &generic.written {
                if !self.type_provides(&ty, *kind) {
                    bounds.push(unmet(kind.name().into(), *bound));
                }
            }
            // A generic trait's bound is its arguments' to check.
            for name in generic.traits.iter().filter(|name| {
                !generic
                    .trait_arguments
                    .iter()
                    .any(|(bound, _)| *bound == name.text)
            }) {
                if !self.type_implements(&ty, &name.text) {
                    bounds.push(unmet(name.text.clone(), generic.trait_bound(name)));
                }
            }
        }
        // A bound a std type misses through its own (`Vec<Opaque>: Debug`)
        // is rustc's requirement chain, which the later check reports.
        if bounds.is_empty() || bounds.iter().any(|unmet| unmet.declared.is_none()) {
            return Ok(());
        }
        Err(Error::NoItem {
            owner: definition.owner_name().into(),
            member: definition.name().into(),
            reference: member_span,
            receiver: Box::new(self.receiver(definition.owner(), receiver)),
            similar: None,
            unsatisfied: Some(Box::new(crate::resolve::UnmetBounds {
                owner: definition.owner_span(),
                bounds: bounds.into_boxed_slice(),
            })),
        })
    }
}

/// rustc's E0107 for a turbofish on the `kind` (`function`, `method`) at
/// `reference` that gives `found` types for its `expected` parameters.
fn generic_count(kind: &str, expected: usize, found: usize, reference: Span) -> Error {
    let plural = |count: usize| match count {
        1 => "1 generic argument".to_owned(),
        count => format!("{count} generic arguments"),
    };
    let supplied = if found == 1 { "was" } else { "were" };
    Error::StdPath {
        code: "E0107",
        message: format!(
            "{kind} takes {} but {} {supplied} supplied",
            plural(expected),
            plural(found)
        )
        .into(),
        label: Some(format!("expected {}", plural(expected)).into()),
        note: None,
        reference,
    }
}

/// The generic parameters a turbofish names, in order: each but an
/// argument-position `impl Trait`, as rustc fills only the written ones.
pub(super) fn written(generics: &[item::Generic]) -> impl Iterator<Item = &item::Generic> {
    generics.iter().filter(|generic| !generic.lifted)
}

/// The generic parameter `ty` is, or refers to.
/// The generic parameter or opaque associated type `ty` is, or refers to:
/// a value with only its bounds' methods.
pub(super) fn bounded_value(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Param(..) | Ty::Projection(..) => Some(ty.clone()),
        Ty::Reference { target, .. } => bounded_value(target),
        _ => None,
    }
}

/// `ty` with each generic parameter in `substitution` replaced.
pub(super) fn instantiate(ty: &Ty, substitution: &BTreeMap<ImmutableString, Ty>) -> Ty {
    substitute(ty, &|ty| match ty {
        Ty::Param(name, Parameter::Declared) => substitution.get(name).cloned(),
        _ => None,
    })
}

/// `ty` with each type parameter and open literal `replaced` gives a type
/// replaced.
pub(super) fn substitute(ty: &Ty, replaced: &dyn Fn(&Ty) -> Option<Ty>) -> Ty {
    let each =
        |types: &[Ty]| -> Box<[Ty]> { types.iter().map(|ty| substitute(ty, replaced)).collect() };
    match ty {
        // An opaque output's type arguments too (`impl Into<T>` at `T`).
        Ty::Param(name, Parameter::Opaque(captured)) => replaced(ty).unwrap_or_else(|| {
            Ty::Param(
                name.clone(),
                Parameter::opaque(captured.opaque, each(&captured.arguments)),
            )
        }),
        Ty::Param(..) | Ty::Int(Int::Var(_)) | Ty::Float(Float::Var(_)) => {
            replaced(ty).unwrap_or_else(|| ty.clone())
        }
        Ty::Option(element) => Ty::Option(Box::new(substitute(element, replaced))),
        Ty::Boxed(target) => Ty::Boxed(Box::new(substitute(target, replaced))),
        Ty::Counted(counter, target) => {
            Ty::Counted(*counter, Box::new(substitute(target, replaced)))
        }
        Ty::Weak(counter, target) => Ty::Weak(*counter, Box::new(substitute(target, replaced))),
        Ty::Iter(item, std, traits) => {
            Ty::Iter(Box::new(substitute(item, replaced)), *std, *traits)
        }
        Ty::Reverse(inner) => Ty::Reverse(Box::new(substitute(inner, replaced))),
        Ty::Cow(inner) => Ty::Cow(Box::new(substitute(inner, replaced))),
        Ty::Range(element) => Ty::Range(Box::new(substitute(element, replaced))),
        Ty::InclusiveOf(element) => Ty::InclusiveOf(Box::new(substitute(element, replaced))),
        Ty::OpenRange(open, element) => {
            Ty::OpenRange(*open, Box::new(substitute(element, replaced)))
        }
        Ty::Cell(kind, inner) => Ty::Cell(*kind, Box::new(substitute(inner, replaced))),
        Ty::Thread(kind, inner) => Ty::Thread(*kind, Box::new(substitute(inner, replaced))),
        Ty::Keyed(kind, arguments) => Ty::Keyed(
            *kind,
            arguments
                .iter()
                .map(|argument| substitute(argument, replaced))
                .collect(),
        ),
        Ty::Dyn(trait_name, bindings, auto) => Ty::Dyn(
            trait_name.clone(),
            bindings
                .iter()
                .map(|(name, ty)| (name.clone(), substitute(ty, replaced)))
                .collect(),
            *auto,
        ),
        Ty::Result(ok, err) => Ty::Result(
            Box::new(substitute(ok, replaced)),
            Box::new(substitute(err, replaced)),
        ),
        Ty::Array { element, kind } => Ty::Array {
            element: Box::new(substitute(element, replaced)),
            kind: match kind {
                sequence::Kind::Array(length) => {
                    sequence::Kind::Array(length.mapped(|length| substitute(length, replaced)))
                }
                kind => kind.clone(),
            },
        },
        Ty::Tuple(members) => Ty::Tuple(each(members)),
        Ty::Reference { mutable, target } => Ty::Reference {
            mutable: *mutable,
            target: Box::new(substitute(target, replaced)),
        },
        Ty::Fn { inputs, output } => Ty::Fn {
            inputs: each(inputs),
            output: Box::new(substitute(output, replaced)),
        },
        Ty::FnItem {
            key,
            inputs,
            output,
        } => Ty::FnItem {
            key: key.clone(),
            inputs: each(inputs),
            output: Box::new(substitute(output, replaced)),
        },
        Ty::Host { proof, arguments } => Ty::Host {
            proof: proof.clone(),
            arguments: each(arguments),
        },
        Ty::Script(id, arguments) => Ty::Script(*id, each(arguments)),
        Ty::Projection(base, trait_name, name) => Ty::Projection(
            Box::new(substitute(base, replaced)),
            trait_name.clone(),
            name.clone(),
        ),
        _ => ty.clone(),
    }
}

/// The const parameters among `generics`, with their value types.
pub(super) fn const_generics(generics: &[item::Generic]) -> BTreeMap<ImmutableString, Ty> {
    generics
        .iter()
        .filter_map(|generic| {
            let ty = generic.constant.as_ref()?;
            Some((generic.name.text.clone(), Ty::from_resolved(ty)))
        })
        .collect()
}

/// The type each parameter among `generics` lifted for a `where` bound on
/// another type is (`Vec<T>` of `where Vec<T>: Debug`), with its name.
pub(super) fn assumed_generics(generics: &[item::Generic]) -> Box<[(Ty, ImmutableString)]> {
    generics
        .iter()
        .filter_map(|generic| {
            let ty = generic.fixed.as_ref()?;
            Some((Ty::from_resolved(ty), generic.name.text.clone()))
        })
        .collect()
}

/// The generic parameters bounded by `Default` or by a trait with a
/// function without `self`: each takes a hidden type witness, so
/// `T::default()` and `T::new()` reach a type erased at runtime. A const
/// parameter takes its value as its witness, which `N` in the body reads.
pub(super) fn witness_generics<'a>(
    generics: &'a [item::Generic],
    models: &'a Models,
) -> impl Iterator<Item = &'a item::Generic> {
    generics.iter().filter(|generic| {
        generic.host
            || generic.constant.is_some()
            || generic.bounds.contains(capability::Kind::Default)
            || generic
                .traits
                .iter()
                .any(|name| static_trait(models, &name.text))
    })
}

/// What a call's witness for a generic parameter must name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Need {
    /// The type itself: `T::default()` builds one, and a host operation
    /// is instantiated by it.
    Type,
    /// The referent of a reference: a parameter witnessed for its traits
    /// alone may take a reference, which implements them only by
    /// forwarding to its referent's impls (`resolve::forward`).
    Referent,
}

impl Need {
    /// What a call's witness for `generic`, one of `witness_generics`,
    /// must name.
    pub(super) fn of(generic: &item::Generic) -> Self {
        if generic.host || generic.bounds.contains(capability::Kind::Default) {
            Self::Type
        } else {
            Self::Referent
        }
    }
}

/// The witness a dispatcher by runtime type passes an impl method for its
/// generics' witnesses, which no runtime value names.
pub(super) const UNKNOWN_WITNESS: crate::INT = -1;

/// Whether an impl whose header writes its owner's arguments `written`
/// (`i32` of `impl Shape for Foo<i32>`) is for the owner's instance at the
/// resolved `arguments`: each written type is its argument, or its
/// argument's literal fallback, where it is not an impl parameter.
pub(super) fn instance_fits(written: &[type_ref::Type], arguments: &[Ty]) -> bool {
    written.iter().zip(arguments).all(|(written, argument)| {
        let ty = || Ty::from_resolved(written);
        mentions_parameter(written)
            || ty() == *argument
            || literal_fallback(argument).is_some_and(|fallback| fallback == ty())
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
    // A function's opaque outputs too, which bound its body's hidden types
    // (D1109).
    fn functions(functions: &[item::Function]) -> Vec<&item::Generic> {
        functions
            .iter()
            .flat_map(|function| {
                function.generics.iter().chain(
                    function
                        .opaque_outputs
                        .iter()
                        .map(|(_, output)| &output.bounds),
                )
            })
            .collect()
    }
    match kind {
        item::Kind::Struct(item) => item.generics.iter().collect(),
        item::Kind::Enum(item) => item.generics.iter().collect(),
        item::Kind::Function(item) => functions(std::slice::from_ref(item)),
        item::Kind::Impl(item) => item
            .generics
            .iter()
            .chain(functions(&item.functions))
            .collect(),
        item::Kind::Trait(item) => functions(&item.functions),
        item::Kind::Use(_) => Vec::new(),
    }
}

/// The substitution of the type parameters an opaque output captures by
/// its type's `arguments` (D1109): what its bounds name them at.
pub(super) fn opaque_substitution(
    output: &item::OpaqueOutput,
    arguments: &[Ty],
) -> BTreeMap<ImmutableString, Ty> {
    output
        .captured
        .iter()
        .cloned()
        .zip(arguments.iter().cloned())
        .collect()
}

/// Every opaque output of the program's functions (D1109), which
/// any body reads as it reads its own type parameters.
pub(super) fn opaque_outputs(
    items: &[item::Shell],
) -> BTreeMap<type_ref::Opaque, item::OpaqueOutput> {
    items
        .iter()
        .flat_map(|shell| match &shell.kind {
            item::Kind::Function(function) => std::slice::from_ref(function),
            item::Kind::Impl(own) => &own.functions[..],
            item::Kind::Trait(own) => &own.functions[..],
            _ => &[],
        })
        .flat_map(|function| function.opaque_outputs.iter().cloned())
        .collect()
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
    /// Each type `ty` may be, the associated type it gives there, and the
    /// bounds of that impl that fix it, as an impl that forwards it to its
    /// parameter's has (`type Out = S::Out`).
    pub(super) cases: Box<[(Ty, Ty, Vec<ImplProjection>)]>,
    /// The associated type the bound names, by its trait's definition
    /// identity and its name: an opaque type that is `ty` gives it from
    /// its own bound.
    pub(super) associated: Option<([u8; 32], ImmutableString)>,
}

/// Why no impl fixed an `impl Trait`'s hidden type (D604): none fits the
/// type named, or more than one does.
#[cfg(feature = "rust-import")]
pub(super) enum Selection {
    None(String),
    Many(String),
}
