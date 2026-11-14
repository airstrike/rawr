//! Trait resolution: the impl of a trait a type has, a receiver's method
//! in Rust's probe order, and the bounds of generic parameters and
//! associated types.

use super::*;

impl<'a> Parser<'a> {
    /// The tag a value of `ty` crosses with as an object of the crate's
    /// trait `trait_name` at `arguments` (D872), when an impl of the trait
    /// for `ty` itself, with no parameters of its own, is among the
    /// crate's: a number, `bool` or `char`, or a crate type with no
    /// parameters (`object::tag`). `None` otherwise.
    #[cfg(feature = "rust-import")]
    pub(super) fn native_implementor(
        &mut self,
        ty: &Ty,
        trait_name: &str,
        arguments: &[Ty],
    ) -> Option<String> {
        use rawr_import::classify::representability::convert::{Builtin, Type};
        if !arguments.is_empty() {
            return None;
        }
        let converted = match self.inference.resolve(ty) {
            Ty::Bool => Type::Builtin(Builtin::Bool),
            Ty::Char => Type::Builtin(Builtin::Char),
            Ty::Float(Float::F32) => Type::Builtin(Builtin::F32),
            Ty::Float(_) => Type::Builtin(Builtin::F64),
            Ty::Int(kind) => Type::Builtin(match kind {
                Int::I8 => Builtin::I8,
                Int::I16 => Builtin::I16,
                Int::I32 | Int::Var(_) => Builtin::I32,
                Int::I64 => Builtin::I64,
                Int::Isize => Builtin::Isize,
                Int::U8 => Builtin::U8,
                Int::U16 => Builtin::U16,
                Int::U32 => Builtin::U32,
                Int::U64 => Builtin::U64,
                Int::Usize => Builtin::Usize,
                Int::I128 | Int::U128 => return None,
            }),
            Ty::Host { proof, arguments } if arguments.is_empty() => Type::Nominal {
                definition: super::surface::host_surface_definition(&proof.definition)?,
                arguments: Box::default(),
            },
            Ty::Arguments => Type::Builtin(Builtin::Arguments),
            _ => return None,
        };
        let tag = rawr_import::classify::representability::object::tag(&converted)?;
        let path = crate::host::path::Path::parse(trait_name).ok()?;
        let identity = self.models.host.trait_header(&path)?.identity;
        self.models
            .host
            .impl_headers(identity)
            .any(|header| {
                header.parameters.is_empty()
                    && header.arguments.is_empty()
                    && implementor_tag(&header.self_type).as_ref() == Some(&tag)
            })
            .then_some(tag)
    }

    /// The callable a function or closure fills the crate's trait
    /// `trait_name` at `arguments` as, when the crate implements the trait
    /// for every callable of one signature, as iced's `impl<T: Fn(Input,
    /// &Theme) -> Style> Highlighter<Input, Theme> for T` (#1981): the
    /// impl's `Fn` bound, each of the trait's parameters an argument or
    /// its default. `None` for a trait with no such impl.
    #[cfg(feature = "rust-import")]
    pub(super) fn native_trait_call(&mut self, trait_name: &str, arguments: &[Ty]) -> Option<Ty> {
        let path = crate::host::path::Path::parse(trait_name).ok()?;
        let header = self.models.host.trait_header(&path)?.clone();
        let call = header.call.as_ref()?;
        let owner = self.models.host.type_owner(trait_name).cloned();
        let mut parameters = BTreeMap::new();
        for (index, name) in header.parameters.iter().enumerate() {
            let ty = match arguments.get(index) {
                Some(ty) => ty.clone(),
                None => {
                    let default = header.defaults.get(index)?.as_ref()?;
                    self.instantiate_host_type(
                        default,
                        &mut parameters,
                        trait_name,
                        Span::NONE,
                        owner.as_ref(),
                        None,
                    )
                    .ok()?
                }
            };
            parameters.insert(name.clone(), ty);
        }
        self.instantiate_host_type(
            call,
            &mut parameters,
            trait_name,
            Span::NONE,
            owner.as_ref(),
            None,
        )
        .ok()
    }

    /// Fixes the open type parameters of `ty` by the one impl of the trait
    /// `identity` at `arguments` that fits it, then by that impl's
    /// where-clause bounds (D604). An impl for any type (`impl<T> Trait for
    /// T`) fits only when no impl for a named type does.
    #[cfg(feature = "rust-import")]
    pub(super) fn select_impl(
        &mut self,
        ty: &Ty,
        identity: [u8; 32],
        arguments: &[Ty],
        owner: Option<&crate::host::facade::Dependency>,
        depth: usize,
    ) -> Result<(), Selection> {
        let ty = self.inference.resolve(ty);
        // A script type's own impl of the trait, which fixes what it can
        // and is compiled for the use (D629). Otherwise a crate's blanket
        // impl may lead to the script's own impl of another trait.
        if matches!(ty, Ty::Script(..))
            && self.select_script_impl(&ty, identity, arguments).is_some()
        {
            return Ok(());
        }
        let open = contains_inference(&ty)
            || arguments
                .iter()
                .any(|argument| contains_inference(&self.inference.resolve(argument)));
        // A bare variable has nothing to select by; the caller's check
        // reports it if it stays open. A closed type that holds a script
        // type is followed for the script impls its bounds use.
        let script = mentions_script(&ty);
        if (!open && !script) || matches!(ty, Ty::Infer(_)) || depth > 32 {
            return Ok(());
        }
        // One impl reached through several of a crate's paths is one impl,
        // as iced's blanket `impl<T: core::Widget> Widget for T` is. Each
        // names its types as the facade that declares it has them, as a
        // game's impl of iced's `Widget` for its own opaque type (#1804).
        let host = self.models.host.clone();
        let mut headers = Vec::new();
        for (header, declaring) in host.owned_impl_headers(identity) {
            if !headers.iter().any(|(kept, _)| *kept == header) {
                headers.push((header, declaring.or(owner)));
            }
        }
        let mut named = Vec::new();
        let mut blanket = Vec::new();
        for (index, &(header, declaring)) in headers.iter().enumerate() {
            let checkpoint = self.inference.checkpoint();
            let fits = self.apply_impl(header, &ty, arguments, declaring).is_some();
            self.inference.rollback(checkpoint);
            if fits {
                if matches!(
                    header.self_type.shape(),
                    crate::host::type_ref::Shape::Parameter(_)
                ) {
                    blanket.push(index);
                } else {
                    named.push(index);
                }
            }
        }
        let chosen = match (named.as_slice(), blanket.as_slice()) {
            ([index], _) | ([], [index]) => *index,
            // Nothing open is rustc's to prove.
            _ if !open => return Ok(()),
            ([], []) => return Err(Selection::None(ty.name(&self.models).to_string())),
            _ => return Err(Selection::Many(ty.name(&self.models).to_string())),
        };
        let checkpoint = self.inference.checkpoint();
        let (header, declaring) = headers[chosen];
        let Some(mut bounds) = self.apply_impl(header, &ty, arguments, declaring) else {
            self.inference.rollback(checkpoint);
            return Err(Selection::None(ty.name(&self.models).to_string()));
        };
        self.inference.commit(checkpoint);
        // A bound on a bare variable waits for one that fixes it. The impl
        // is chosen by its self type and the trait's arguments alone, as
        // rustc selects one, and its where-clause bounds are obligations
        // it proves afterwards: a bound no header fits, as iced's `W:
        // Widget<..>` for a `&str` content whose impl the facade can't
        // write, fixes nothing more and leaves the impl chosen.
        for _ in 0..2 {
            for (bounded, trait_identity, bound_arguments) in &bounds.traits {
                let checkpoint = self.inference.checkpoint();
                match self.select_impl(bounded, *trait_identity, bound_arguments, owner, depth + 1)
                {
                    Ok(()) => self.inference.commit(checkpoint),
                    Err(_) => self.inference.rollback(checkpoint),
                }
            }
            // A bound the others fixed the type of fixes its associated
            // type in turn.
            bounds.projections = self
                .project_impl(std::mem::take(&mut bounds.projections))
                .unwrap_or_default();
        }
        // One whose type a later use fixes waits for it.
        self.inference.projections.extend(bounds.projections);
        Ok(())
    }

    /// The one script impl of the crate trait `identity` that fits the
    /// script type `ty` at `arguments`, applied: its parameters and what it
    /// fixes of `arguments` are bound, as rustc selects a program's own impl
    /// of a crate's trait. The use is recorded, so an impl whose parameters
    /// touch a crate's types is compiled for it (D629), as at a type that
    /// holds a crate's opaque type, which the runner names by a name of its
    /// own. `None` when none fits, or more than one.
    #[cfg(feature = "rust-import")]
    pub(super) fn select_script_impl(
        &mut self,
        ty: &Ty,
        identity: [u8; 32],
        arguments: &[Ty],
    ) -> Option<()> {
        let Ty::Script(definition, _) = self.inference.resolve(ty) else {
            return None;
        };
        let candidates = self
            .models
            .script_impls
            .iter()
            .filter(|candidate| {
                matches!(&candidate.owner, type_ref::Type::Path(type_ref::Path {
                    target: type_ref::Target::Script { definition: owner, .. },
                    ..
                }) if *owner == definition)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut fits = Vec::new();
        for candidate in candidates {
            let checkpoint = self.inference.checkpoint();
            let fit = self
                .apply_script_impl(&candidate, ty, identity, arguments)
                .is_some();
            self.inference.rollback(checkpoint);
            if fit {
                fits.push(candidate);
            }
        }
        let [chosen] = fits.as_slice() else {
            return None;
        };
        let checkpoint = self.inference.checkpoint();
        let Some((types, full)) = self.apply_script_impl(chosen, ty, identity, arguments) else {
            self.inference.rollback(checkpoint);
            return None;
        };
        self.inference.commit(checkpoint);
        self.impl_uses.push((chosen.index, types, full));
        Some(())
    }

    /// `script` at fresh variables, unified with `ty` and `arguments`, the
    /// trait's defaults filling the arguments its header leaves out: its
    /// parameters' types and the trait's arguments, or `None` when it
    /// doesn't fit or isn't of the trait `identity`.
    #[cfg(feature = "rust-import")]
    pub(super) fn apply_script_impl(
        &mut self,
        script: &ScriptTraitImpl,
        ty: &Ty,
        identity: [u8; 32],
        arguments: &[Ty],
    ) -> Option<(Vec<(ImmutableString, Ty)>, UseArguments)> {
        let path = crate::host::path::Path::parse(script.path.as_str()).ok()?;
        let header = self.models.host.trait_header(&path).cloned()?;
        if header.identity != identity || header.parameters.len() != arguments.len() {
            return None;
        }
        let substitution = script
            .generics
            .iter()
            .map(|name| (name.clone(), self.inference.fresh()))
            .collect::<BTreeMap<_, _>>();
        let owner = instantiate(&Ty::from_resolved(&script.owner), &substitution);
        if !self.unify_type(&owner, ty) {
            return None;
        }
        let trait_owner = self.models.host.type_owner(&script.path).cloned();
        let mut parameters = BTreeMap::new();
        let mut full = Vec::with_capacity(header.parameters.len());
        for (index, name) in header.parameters.iter().enumerate() {
            let argument = match script.arguments.get(index) {
                Some(written) => instantiate(&Ty::from_resolved(written), &substitution),
                None => match header.defaults.get(index).and_then(Option::as_ref) {
                    Some(default) => self
                        .instantiate_host_type(
                            default,
                            &mut parameters,
                            &script.path,
                            Span::NONE,
                            trait_owner.as_ref(),
                            None,
                        )
                        .ok()?,
                    None => self.inference.fresh(),
                },
            };
            parameters.insert(name.clone(), argument.clone());
            full.push(argument);
        }
        for (written, argument) in full.iter().zip(arguments) {
            if !self.unify_type(written, argument) {
                return None;
            }
        }
        let types = script
            .generics
            .iter()
            .map(|name| (name.clone(), substitution[name].clone()))
            .collect();
        Some((types, full))
    }

    /// `arguments` from `start` on unified with the defaults of the crate
    /// trait `identity`, its parameters in them at `arguments`, as rustc
    /// reads a bound written `impl Widget<Message>`. `None` when one has
    /// no default, or its default doesn't fit.
    #[cfg(feature = "rust-import")]
    fn unify_trait_defaults(
        &mut self,
        identity: [u8; 32],
        start: usize,
        arguments: &[Ty],
    ) -> Option<()> {
        let host = self.models.host.clone();
        let name = host.trait_name(&identity)?;
        let header = host.trait_header(&crate::host::path::Path::parse(name.as_str()).ok()?)?;
        if header.parameters.len() != arguments.len() {
            return None;
        }
        let owner = host.type_owner(&name);
        let mut parameters = header
            .parameters
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        for (default, argument) in header.defaults.iter().zip(arguments).skip(start) {
            let default = self
                .instantiate_host_type(
                    default.as_ref()?,
                    &mut parameters,
                    &name,
                    Span::NONE,
                    owner,
                    None,
                )
                .ok()?;
            if !self.unify_type(&default, argument) {
                return None;
            }
        }
        Some(())
    }

    /// `header` at fresh variables, unified with `ty` and `arguments`: its
    /// where-clause bounds at those variables, or `None` when it doesn't fit.
    #[cfg(feature = "rust-import")]
    pub(super) fn apply_impl(
        &mut self,
        header: &crate::host::facade::ImplHeader,
        ty: &Ty,
        arguments: &[Ty],
        owner: Option<&crate::host::facade::Dependency>,
    ) -> Option<ImplBounds> {
        // A header may leave out the trait's defaulted arguments its
        // facade can't write; they are the trait's defaults (#1804).
        if header.arguments.len() > arguments.len() {
            return None;
        }
        let mut parameters = BTreeMap::new();
        let pattern = self
            .instantiate_host_type(
                &header.self_type,
                &mut parameters,
                "impl",
                Span::NONE,
                owner,
                None,
            )
            .ok()?;
        if !self.unify_type(&pattern, ty) {
            return None;
        }
        for (written, argument) in header.arguments.iter().zip(arguments) {
            let written = self
                .instantiate_host_type(written, &mut parameters, "impl", Span::NONE, owner, None)
                .ok()?;
            if !self.unify_type(&written, argument) {
                return None;
            }
        }
        if header.arguments.len() < arguments.len() {
            self.unify_trait_defaults(header.trait_identity, header.arguments.len(), arguments)?;
        }
        let traits = header
            .bounds
            .iter()
            .map(|bound| {
                let bounded = self
                    .instantiate_host_type(
                        &bound.ty,
                        &mut parameters,
                        "impl",
                        Span::NONE,
                        owner,
                        None,
                    )
                    .ok()?;
                let arguments = bound
                    .arguments
                    .iter()
                    .map(|argument| {
                        self.instantiate_host_type(
                            argument,
                            &mut parameters,
                            "impl",
                            Span::NONE,
                            owner,
                            None,
                        )
                        .ok()
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some((bounded, bound.trait_identity, arguments))
            })
            .collect::<Option<Vec<_>>>()?;
        // A case names no parameter of the impl's.
        let at_case = |this: &mut Self, ty| {
            this.instantiate_host_type(ty, &mut BTreeMap::new(), "impl", Span::NONE, owner, None)
                .ok()
        };
        let mut projections = Vec::with_capacity(header.projections.len());
        for projection in header.projections.iter() {
            let mut at = |ty| {
                self.instantiate_host_type(ty, &mut parameters, "impl", Span::NONE, owner, None)
                    .ok()
            };
            let (Some(ty), Some(value)) = (at(&projection.ty), at(&projection.value)) else {
                return None;
            };
            let cases = projection
                .cases
                .iter()
                .filter_map(|(this, at)| Some((at_case(self, this)?, at_case(self, at)?)))
                .collect();
            projections.push(ImplProjection { ty, value, cases });
        }
        let projections = self.project_impl(projections)?;
        Some(ImplBounds {
            traits,
            projections,
        })
    }

    /// Each of an impl's bounds `<ty as Trait>::Name == value` whose `ty`
    /// is known fixes `value` to the associated type the crate's impls give
    /// `ty`, as rustc's projection does: the bounds still open. `None` when
    /// one contradicts: `ty` is a case whose associated type `value` can't
    /// be, so the impl doesn't apply (rustc's E0271).
    #[cfg(feature = "rust-import")]
    pub(super) fn project_impl(
        &mut self,
        projections: Vec<ImplProjection>,
    ) -> Option<Vec<ImplProjection>> {
        let mut open = Vec::new();
        for projection in projections {
            let ty = self.inference.resolve(&projection.ty);
            if contains_inference(&ty) {
                open.push(projection);
                continue;
            }
            for (this, value) in projection.cases.iter() {
                let checkpoint = self.inference.checkpoint();
                if !self.unify_type(this, &ty) {
                    self.inference.rollback(checkpoint);
                    continue;
                }
                if !self.unify_type(value, &projection.value) {
                    self.inference.rollback(checkpoint);
                    return None;
                }
                self.inference.commit(checkpoint);
                break;
            }
        }
        Some(open)
    }

    /// Each pending impl bound `<ty as Trait>::Name == value` (`Inference::
    /// projections`) whose `ty` is known by now fixes its `value`, until
    /// none does more.
    #[cfg(feature = "rust-import")]
    pub(super) fn finish_projections(&mut self) {
        loop {
            let pending = std::mem::take(&mut self.inference.projections);
            let count = pending.len();
            let open = self.project_impl(pending).unwrap_or_default();
            let settled = open.len() == count;
            self.inference.projections.extend(open);
            if settled {
                break;
            }
        }
    }

    /// The method `member` of the script type `owner`: inherent first, then
    /// trait functions; in a trait's default body, that trait's own function.
    pub(super) fn receiver_method(
        &self,
        owner: type_def::Id,
        member: &str,
    ) -> Option<method::Definition> {
        if let Some((default_owner, trait_name)) = &self.default_trait
            && *default_owner == owner
            && let Some(definition) = self
                .models
                .trait_methods
                .get(&(owner, member.into()))
                .and_then(|definitions| {
                    definitions.iter().find(|definition| {
                        definition.trait_name() == Some(trait_name)
                            && instance_text(definition.trait_arguments()) == self.impl_instance
                    })
                })
        {
            return Some(definition.clone());
        }
        // A supertrait's function, as a generic `Self: Shape` sees it, before
        // an inherent function of the owner.
        if let Some((default_owner, trait_name)) = &self.default_trait
            && *default_owner == owner
            && let Some(definitions) = self.models.trait_methods.get(&(owner, member.into()))
            && let Some(definition) =
                supertraits(&self.models, trait_name)
                    .iter()
                    .find_map(|parent| {
                        // A generic supertrait's instance is the one the
                        // trait names, through this impl's arguments
                        // (`Convert<i64>` in `impl Both<i64> for C`).
                        let expected = self.supertrait_instances(trait_name, parent);
                        definitions
                            .iter()
                            .filter(|definition| definition.trait_name() == Some(parent))
                            .find(|definition| {
                                expected.is_empty()
                                    || expected.iter().any(|arguments| {
                                        definition
                                            .trait_arguments()
                                            .iter()
                                            .map(Ty::from_resolved)
                                            .eq(arguments.iter().cloned())
                                    })
                            })
                            .or_else(|| {
                                definitions
                                    .iter()
                                    .find(|definition| definition.trait_name() == Some(parent))
                            })
                    })
        {
            return Some(definition.clone());
        }
        self.models.method(owner, member).cloned()
    }

    /// `receiver_method` in Rust's probe order for a receiver of type
    /// `receiver`: a trait function found at an earlier step than the
    /// inherent one comes first (`self` on a value before `&self`).
    pub(super) fn probed_method(
        &self,
        owner: type_def::Id,
        member: &str,
        receiver: &Ty,
    ) -> Option<method::Definition> {
        let found = self.receiver_method(owner, member);
        let receiver = self.inference.resolve(receiver);
        let rank = |definition: &method::Definition| {
            definition
                .receiver()
                .map(|mode| probe_rank(&receiver, mode))
        };
        let Some(inherent) = found
            .as_ref()
            .filter(|definition| definition.trait_name().is_none())
            .and_then(rank)
        else {
            return found;
        };
        self.models
            .trait_methods
            .get(&(owner, member.into()))
            .and_then(|definitions| {
                definitions
                    .iter()
                    .filter(|definition| self.trait_in_scope(definition))
                    .filter_map(|definition| Some((rank(definition)?, definition)))
                    .min_by_key(|(rank, _)| *rank)
            })
            .filter(|(rank, _)| *rank < inherent)
            .map(|(_, definition)| definition.clone())
            .or(found)
    }

    /// The trait function `member` of the builtin owner's impl whose Self
    /// type is `self_ty`'s (through its references): `f32`'s `half` beside
    /// `f64`'s, which share one impl owner and one runtime type, so the
    /// call selects it here as rustc selects the impl. An unsuffixed
    /// literal both fit is its fallback kind. `None` where the impls have
    /// one Self type, or the type fits other than one.
    pub(super) fn self_typed_function(
        &mut self,
        owner: type_def::Id,
        member: &str,
        self_ty: &Ty,
    ) -> Option<method::Definition> {
        owner.builtin_code()?;
        let definitions = self
            .models
            .trait_methods
            .get(&(owner, member.into()))?
            .clone();
        let selves = definitions
            .iter()
            .filter_map(method::Definition::builtin_owner)
            .map(crate::resolve::full_type_name)
            .collect::<BTreeSet<_>>();
        if selves.len() < 2 {
            return None;
        }
        let mut self_ty = self.inference.resolve(self_ty);
        while let Ty::Reference { target, .. } = self_ty {
            self_ty = self.inference.resolve(&target);
        }
        let fitting = definitions
            .into_iter()
            .filter(|definition| {
                let probe = definition
                    .generics()
                    .iter()
                    .map(|generic| (generic.name.text.clone(), self.inference.fresh()))
                    .collect();
                let owner = impl_self_ty(definition, &probe);
                self.unifies(&owner, &self_ty)
            })
            .collect::<Vec<_>>();
        match &fitting[..] {
            [definition] => Some(definition.clone()),
            _ => literal_fallback(&self_ty).and_then(|fallback| {
                let mut preferred = fitting.into_iter().filter(|definition| {
                    definition
                        .builtin_owner()
                        .is_some_and(|owner| Ty::from_resolved(owner) == fallback)
                });
                let definition = preferred.next()?;
                preferred.next().is_none().then_some(definition)
            }),
        }
    }

    /// Whether the trait of `definition`, a trait impl's function, is in
    /// scope where the method is called, as Rust's probe asks of a trait
    /// method before an inherent one: declared in this module or imported
    /// into it. A std trait counts as in scope.
    pub(super) fn trait_in_scope(&self, definition: &method::Definition) -> bool {
        let Some(trait_name) = definition.trait_name() else {
            return true;
        };
        if self.models.import(trait_name).is_some()
            || self.models.std_traits.contains(trait_name.as_str())
            || self
                .models
                .traits
                .get(trait_name.as_str())
                .is_some_and(|(_, declared)| declared.std.is_some())
        {
            return true;
        }
        // A facade trait (iced's `canvas::Program`) is declared at the root
        // for the whole crate; only its import puts it in scope.
        self.models
            .traits
            .get(trait_name.as_str())
            .is_some_and(|(source, declared)| {
                declared.declaration.is_none() && *source == self.models.context
            })
    }

    /// The instances of the supertrait `parent` that `dyn sub<..>` has:
    /// `[[i64]]` of `Convert` for `dyn Both<i64>` (`trait Both<T>:
    /// Convert<T>`).
    pub(super) fn dyn_supertrait_instances(
        &self,
        sub: &str,
        bindings: &[(ImmutableString, Ty)],
        object: &Ty,
        parent: &str,
    ) -> Vec<Vec<Ty>> {
        let Some((_, declared)) = self.models.traits.get(sub) else {
            return Vec::new();
        };
        let mut substitution: BTreeMap<ImmutableString, Ty> = declared
            .generics
            .iter()
            .map(|generic| generic.name.text.clone())
            .zip(dyn_arguments(bindings))
            .collect();
        substitution.insert("Self".into(), object.clone());
        declared
            .supertrait_arguments
            .iter()
            .filter(|(name, _)| name == parent)
            .map(|(_, arguments)| {
                arguments
                    .iter()
                    .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                    .collect()
            })
            .collect()
    }

    /// The arguments the trait `sub` gives its supertrait `parent` in the
    /// impl being lowered: its parameters are the impl's arguments and
    /// `Self` its owner (`[C]` of `trait Ord2: PartialOrd2<Self>` in `impl
    /// Ord2 for C`).
    pub(super) fn supertrait_instances(&self, sub: &str, parent: &str) -> Vec<Vec<Ty>> {
        let Some((_, declared)) = self.models.traits.get(sub) else {
            return Vec::new();
        };
        let mut substitution = self.trait_parameters.clone();
        if let Some(self_ty) = &self.self_ty {
            substitution.insert("Self".into(), self_ty.clone());
        }
        declared
            .supertrait_arguments
            .iter()
            .filter(|(name, _)| name == parent)
            .map(|(_, arguments)| {
                arguments
                    .iter()
                    .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                    .collect()
            })
            .collect()
    }

    /// The function `member` of the one generic trait whose impls for
    /// `owner` give it more than once (`Convert<i64>` and
    /// `Convert<String>`), with that trait's name.
    pub(super) fn generic_trait_method(
        &self,
        owner: type_def::Id,
        member: &str,
    ) -> Option<(ImmutableString, item::Function)> {
        let definitions = self.models.trait_methods.get(&(owner, member.into()))?;
        let trait_name = definitions.first()?.trait_name()?;
        if definitions.len() < 2
            || definitions
                .iter()
                .any(|definition| definition.trait_name() != Some(trait_name))
        {
            return None;
        }
        let (_, declared) = self.models.traits.get(trait_name)?;
        if declared.generics.is_empty() {
            return None;
        }
        let function = declared
            .functions
            .iter()
            .find(|function| function.name.text == member)?;
        Some((trait_name.clone(), function.clone()))
    }

    /// The function `member` of the blanket impl (`impl<T: Bound> Trait
    /// for T`) whose where-clauses hold for `ty`, as rustc's probe finds it
    /// for a type no other impl gives `member`.
    pub(super) fn blanket_method(&mut self, ty: &Ty, member: &str) -> Option<method::Definition> {
        let owner = type_def::Id::builtin(crate::resolve::BLANKET_OWNER);
        let definitions = self
            .models
            .trait_methods
            .get(&(owner, member.into()))?
            .clone();
        let mut fitting = definitions
            .into_iter()
            .filter(|definition| self.blanket_fits(definition, ty, None));
        let definition = fitting.next()?;
        fitting.next().is_none().then_some(definition)
    }

    /// Whether a blanket impl of `trait_name` at `arguments` holds for
    /// `ty`: its where-clauses do (rustc's E0277 where none does).
    pub(super) fn blanket_implements(
        &mut self,
        ty: &Ty,
        trait_name: &str,
        arguments: &[Ty],
    ) -> bool {
        let owner = type_def::Id::builtin(crate::resolve::BLANKET_OWNER);
        if !self.models.implements(trait_name, owner) {
            return false;
        }
        let definitions = self
            .models
            .trait_methods
            .iter()
            .filter(|((id, _), _)| *id == owner)
            .flat_map(|(_, definitions)| definitions)
            .filter(|definition| {
                definition
                    .trait_name()
                    .is_some_and(|name| name == trait_name)
            })
            .cloned()
            .collect::<Vec<_>>();
        definitions
            .iter()
            .any(|definition| self.blanket_fits(definition, ty, Some(arguments)))
    }

    /// Whether the blanket impl of `definition` applies to `ty`, and to the
    /// trait's `arguments` when given: its owner, a parameter, takes `ty`
    /// and its bounds hold; an owner bounded by `IntoIterator<Item = X>` is
    /// the iterator of `X` (`resolve::erase_iterator_owner`), which `ty`
    /// is when Rust's `IntoIterator` walks it with items of `X`.
    fn blanket_fits(
        &mut self,
        definition: &method::Definition,
        ty: &Ty,
        arguments: Option<&[Ty]>,
    ) -> bool {
        let checkpoint = self.inference.checkpoint();
        let substitution = definition
            .generics()
            .iter()
            .map(|generic| (generic.name.text.clone(), self.inference.fresh()))
            .collect::<BTreeMap<_, _>>();
        let ty = self.inference.resolve(ty);
        let owner = impl_self_ty(definition, &substitution);
        let mut fits = match &owner {
            Ty::Iter(item, _) => self
                .iterable_item(&ty)
                .is_some_and(|found| self.unify_type(item, &found)),
            owner => self.unify_type(owner, &ty),
        };
        if let Some(arguments) = arguments {
            let written = definition
                .trait_arguments()
                .iter()
                .map(|argument| instantiate(&Ty::from_resolved(argument), &substitution))
                .collect::<Vec<_>>();
            fits = fits
                && written.len() == arguments.len()
                && written
                    .iter()
                    .zip(arguments)
                    .all(|(written, argument)| self.unify_type(written, argument));
        }
        let trait_name = definition.trait_name().cloned();
        for generic in definition.generics() {
            if !fits {
                break;
            }
            let bounded = self.inference.resolve(&substitution[&generic.name.text]);
            if matches!(bounded, Ty::Infer(_)) {
                continue;
            }
            fits = bound_kinds(generic.bounds).all(|kind| self.type_provides(&bounded, kind))
                && generic
                    .traits
                    .iter()
                    .filter(|bound| trait_name.as_ref() != Some(&bound.text))
                    .all(|bound| {
                        let lists = generic
                            .trait_arguments
                            .iter()
                            .filter(|(name, _)| *name == bound.text)
                            .map(|(_, arguments)| {
                                arguments
                                    .iter()
                                    .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                                    .collect::<Vec<_>>()
                            })
                            .collect::<Vec<_>>();
                        if lists.is_empty() {
                            return self.type_implements_with(&bounded, &bound.text, &[]);
                        }
                        lists
                            .iter()
                            .all(|list| self.type_implements_with(&bounded, &bound.text, list))
                    });
        }
        self.inference.rollback(checkpoint);
        fits
    }

    /// The items Rust's `IntoIterator` gives for `ty`: an iterator's, a
    /// range's, an array's or a collection's, and a borrowed one's by
    /// reference.
    pub(super) fn iterable_item(&self, ty: &Ty) -> Option<Ty> {
        Some(match self.inference.resolve(ty) {
            Ty::Iter(item, _) => *item,
            Ty::InclusiveRange(kind) => Ty::Int(kind),
            Ty::Range(element) => *element,
            Ty::Array { element, .. } => *element,
            Ty::Keyed(kind, arguments) => keyed::keyed_item(kind, &arguments, None),
            Ty::Reference { mutable, target } => match self.inference.resolve(&target) {
                Ty::Array { element, .. } => Ty::Reference {
                    mutable,
                    target: element,
                },
                Ty::Keyed(kind, arguments) => keyed::keyed_item(kind, &arguments, Some(mutable)),
                _ => return None,
            },
            _ => return None,
        })
    }

    /// Whether `ty` implements the crate trait `trait_name`: a script type
    /// with an impl of it, or a generic parameter bounded by it.
    pub(super) fn type_implements(&self, ty: &Ty, trait_name: &str) -> bool {
        match ty {
            Ty::Dyn(name, _) => {
                name == trait_name
                    || supertraits(&self.models, name)
                        .iter()
                        .any(|parent| parent == trait_name)
            }
            Ty::Script(id, _) if self.models.implements(trait_name, *id) => true,
            // Rawr's iterator type: a `Range`, a chain, `impl Iterator`.
            Ty::Iter(..) if is_iterator_trait(trait_name) => true,
            // An integer range a crate holds by value, as itertools'
            // `Product<I, RangeInclusive<isize>>` does: std's `Iterator`.
            Ty::InclusiveRange(_) if is_iterator_trait(trait_name) => true,
            Ty::Range(element)
                if is_iterator_trait(trait_name) && matches!(**element, Ty::Int(_)) =>
            {
                true
            }
            // An imported type the crate implements `Iterator`,
            // `PartialOrd`, `Ord` or `std::error::Error` for.
            Ty::Host { proof, .. }
                if match trait_name {
                    "Iterator" => Some(capability::Kind::Iterator),
                    "Error" if self.models.std_error_trait() => Some(capability::Kind::Error),
                    "PartialOrd" | "Ord" => capability::Kind::from_derive(trait_name),
                    _ => None,
                }
                .is_some_and(|kind| proof.definition.capabilities().contains(kind)) =>
            {
                true
            }
            Ty::Param(_) | Ty::Projection(..) => self
                .bound_traits(ty)
                .iter()
                .any(|bound| bound == trait_name),
            // Rust's own `Display` impls.
            Ty::Int(_)
            | Ty::Float(_)
            | Ty::String
            | Ty::Str
            | Ty::Char
            | Ty::Bool
            | Ty::ParseError(_)
                if trait_name == "Display" && self.models.std_traits.contains(trait_name) =>
            {
                true
            }
            // std's other `Display` impls, from the std table: `Rc<T>`,
            // `Box<T>`, `&T` and `Cow<'_, B>` where what they hold is.
            ty if trait_name == "Display"
                && self.models.std_traits.contains(trait_name)
                && self.std_declared(ty) =>
            {
                std_impl::holds(
                    &mut std_impl::Probe(self),
                    "core::fmt::Display",
                    ty,
                    &[],
                    true,
                    0,
                )
                .is_ok()
            }
            // `impl<T: For + Copy> For for &T` forwarding to `T`'s impl
            // (`resolve::forward`): `&T` implements it where `T` does and
            // has its other bounds.
            Ty::Reference { mutable, target } => {
                let target = self.inference.resolve(target);
                self.models
                    .traits
                    .get(trait_name)
                    .into_iter()
                    .flat_map(|(_, declared)| declared.reference_forwards.iter())
                    .filter(|forward| forward.mutable == *mutable)
                    .any(|forward| {
                        self.type_implements(&target, trait_name)
                            && bound_kinds(forward.bounds)
                                .all(|kind| self.type_provides(&target, kind))
                            && forward
                                .traits
                                .iter()
                                .all(|bound| self.type_implements(&target, &bound.text))
                    })
            }
            // Rust's own operator impls on numbers (`i64: Add`, `f64: Neg`),
            // `bool: Not`, and `String: Add<&str>` / `AddAssign<&str>`.
            ty if self.models.std_traits.contains(trait_name)
                && builtin_operator(ty, trait_name) =>
            {
                true
            }
            // Rust's own `PartialOrd` and `Ord` impls.
            ty if matches!(trait_name, "PartialOrd" | "Ord")
                && self.models.std_traits.contains(trait_name)
                && self.std_ordered(ty, trait_name == "PartialOrd") =>
            {
                true
            }
            // A crate trait's impl for a builtin type.
            ty => owner_id(ty).is_some_and(|id| self.models.implements(trait_name, id)),
        }
    }

    /// Whether `ty` implements the generic trait `trait_name` with the
    /// type arguments `arguments` (`C: Convert<i64>`); without arguments,
    /// `type_implements`.
    pub(super) fn type_implements_with(
        &mut self,
        ty: &Ty,
        trait_name: &str,
        arguments: &[Ty],
    ) -> bool {
        if arguments.is_empty() && self.type_implements(ty, trait_name) {
            return true;
        }
        // A blanket impl whose where-clauses hold for `ty`.
        if self.blanket_implements(ty, trait_name, arguments) {
            return true;
        }
        if arguments.is_empty() {
            return false;
        }
        // `PartialEq<Self>` and `PartialOrd<Self>`, their `Rhs = Self`
        // filled in: a derive or a builtin's comparison is with its own type.
        if let [argument] = arguments
            && let Some(kind) = capability::Kind::from_derive(trait_name)
            && self.unifies(argument, ty)
            && self.type_provides(ty, kind)
        {
            return true;
        }
        // The prelude's blanket impls: `impl<T> From<T> for T`, its builtin
        // conversions, and `impl<T, U: From<T>> Into<U> for T`.
        let resolved = self.inference.resolve(ty);
        if let [argument] = arguments
            && !matches!(resolved, Ty::Param(_) | Ty::Projection(..))
            && self.prelude_trait(trait_name)
        {
            let argument = self.inference.resolve(argument);
            // `T: From<S>` for the `From` bound, `S: Into<T>` for `Into`.
            let (target, source) = match trait_name {
                "From" => (resolved, argument),
                _ => (argument, resolved),
            };
            if self.prelude_from(&target, &source) {
                return true;
            }
            // A script's own `impl From<Shade> for iced::Color`, of a script
            // type into a crate's.
            let script_from = self.models.conversions.values().flatten().any(|method| {
                method
                    .builtin_owner()
                    .zip(method.conversion())
                    .is_some_and(|(owner, from)| {
                        self.inference.resolve(&Ty::from_resolved(owner)) == target
                            && self.inference.resolve(&Ty::from_resolved(from)) == source
                    })
            });
            if script_from {
                return true;
            }
            // `impl Into<Length>` of a number: a library type's own
            // `from` of it.
            if self.host_from(&target, &source) {
                return true;
            }
            // A crate's `impl From<Its> for PathBuf`, of its type into a
            // std type Rawr's std holds: the `into` it gives `Its`, which a
            // generic so bounded runs where it is compiled at `Its`
            // (`imported::instanced`).
            if conversion::held_std(&target) && self.source_into(&target, &source) {
                return true;
            }
            if trait_name != "From" {
                return self.type_implements_with(&target, "From", &[source]);
            }
        }
        // Rust's own operator impls take their own type: `i64: Add<i64>`,
        // and `String: Add<&str>`.
        let resolved = self.inference.resolve(ty);
        if let [argument] = arguments
            && self.models.std_traits.contains(trait_name)
            && builtin_operator(&resolved, trait_name)
        {
            let argument = self.inference.resolve(argument);
            let text = argument == Ty::Str
                || matches!(argument, Ty::Reference { ref target, .. }
                    if self.inference.resolve(target).is_text());
            return if resolved == Ty::String {
                text
            } else {
                self.unifies(&resolved, &argument)
            };
        }
        // std's impls of a std trait for a type std declares: `String:
        // AsRef<str>`, `Box<u32>: AsMut<u32>` (D711 step 3). A bound's
        // bare `str` reads as a `&str`, and is std's unsized `str`.
        if self.std_trait(trait_name)
            && self.std_declared(&resolved)
            && let Some(definition) = crate::resolve::std_trait::anywhere(trait_name)
        {
            let arguments = arguments
                .iter()
                .map(|argument| match self.inference.resolve(argument) {
                    Ty::Str => Ty::UnsizedStr,
                    argument => argument,
                })
                .collect::<Vec<_>>();
            // Matched by unification, so `Vec<{integer}>: BorrowMut<Vec<i32>>`
            // holds, and checked only: nothing it binds is kept.
            let checkpoint = self.inference.checkpoint();
            let holds = std_impl::holds(self, definition, &resolved, &arguments, true, 0).is_ok();
            self.inference.rollback(checkpoint);
            if holds {
                return true;
            }
        }
        let candidates: Vec<Vec<Ty>> = match self.inference.resolve(ty) {
            Ty::Dyn(name, bindings) if name == trait_name => vec![dyn_arguments(&bindings)],
            Ty::Dyn(name, bindings) => {
                self.dyn_supertrait_instances(&name, &bindings, ty, trait_name)
            }
            ty @ (Ty::Param(_) | Ty::Projection(..)) => self
                .bound_trait_arguments(&ty, trait_name)
                .iter()
                .map(|arguments| arguments.to_vec())
                .collect(),
            ty => match owner_id(&ty) {
                Some(id) => self.impl_candidates(trait_name, id, owner_arguments(&ty)),
                None => Vec::new(),
            },
        };
        candidates.iter().any(|candidate| {
            candidate.len() == arguments.len()
                && candidate
                    .iter()
                    .zip(arguments)
                    .all(|(candidate, argument)| self.unifies(candidate, argument))
        })
    }

    /// Whether std's `impl From<source> for target` holds (D711 step 3),
    /// one rawr runs (`std_impl::runs_from`).
    pub(super) fn prelude_from(&mut self, target: &Ty, source: &Ty) -> bool {
        let (target, source) = (
            self.inference.resolve(target),
            self.inference.resolve(source),
        );
        self.std_from(&target, &source) && std_impl::runs_from(&target, &source).is_some()
    }

    /// Whether `trait_name` is std's trait, not a crate trait of that name.
    pub(super) fn std_trait(&self, trait_name: &str) -> bool {
        self.models
            .traits
            .get(trait_name)
            .is_some_and(|(_, declared)| declared.name.span == Span::NONE)
    }

    /// Whether `trait_name` is the prelude's `From` or `Into`, not a crate
    /// trait of that name.
    pub(super) fn prelude_trait(&self, trait_name: &str) -> bool {
        matches!(trait_name, "From" | "Into")
            && self
                .models
                .traits
                .get(trait_name)
                .is_some_and(|(_, declared)| declared.name.span == Span::NONE)
    }

    /// The trait arguments of each impl of the generic trait `trait_name`
    /// for `owner`, each impl's own parameters fresh but those the owner's
    /// type arguments `owner_arguments` are: `impl<M> IntoContent<M> for
    /// Content<M>` gives `Content<i64>` the arguments `[i64]`.
    pub(super) fn impl_candidates(
        &mut self,
        trait_name: &str,
        owner: type_def::Id,
        owner_arguments: &[Ty],
    ) -> Vec<Vec<Ty>> {
        let impls = self
            .models
            .impl_arguments
            .get(&(trait_name.into(), owner))
            .cloned()
            .unwrap_or_default();
        impls
            .iter()
            .map(|(parameters, owner_parameters, arguments)| {
                let mut substitution: BTreeMap<ImmutableString, Ty> = parameters
                    .iter()
                    .map(|name| (name.clone(), self.inference.fresh()))
                    .collect();
                for (parameter, argument) in owner_parameters.iter().zip(owner_arguments) {
                    if let Some(parameter) = parameter {
                        substitution.insert(parameter.clone(), argument.clone());
                    }
                }
                arguments
                    .iter()
                    .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                    .collect()
            })
            .collect()
    }

    /// `value.f(arguments)` for a hand-written std trait function `f`.
    pub(super) fn std_call(
        definition: &method::Definition,
        value: origin::Fragment,
        arguments: &[origin::Fragment],
    ) -> origin::Fragment {
        let mut call = value.surrounded("(", ")");
        call.push_text(format!(".{}(", method_name(definition)));
        call.push(origin::Fragment::join(arguments.iter().cloned(), ", "));
        call.push_text(")");
        call
    }

    /// `value.f(arguments)` on a generic value through the std trait's
    /// dispatcher, which falls back to Rawr's own behaviour.
    pub(super) fn std_dispatch(
        trait_name: &str,
        function: &str,
        value: origin::Fragment,
        arguments: &[origin::Fragment],
    ) -> origin::Fragment {
        let mut call = value.surrounded("(", ")");
        call.push_text(format!(".{}(", trait_dispatcher_name(trait_name, function)));
        call.push(origin::Fragment::join(arguments.iter().cloned(), ", "));
        call.push_text(")");
        call
    }

    /// The function `member` of a user trait bounding the generic parameter
    /// `param`, with that trait's name.
    pub(super) fn generic_trait_function(
        &self,
        param: &str,
        member: &str,
        span: Span,
    ) -> Result<Option<(ImmutableString, item::Function)>, Error> {
        self.bound_trait_function(&Ty::Param(param.into()), member, span)
    }

    /// The user trait bounds of a generic parameter or of an associated
    /// type (`type Output: Shape;`).
    pub(super) fn bound_traits(&self, ty: &Ty) -> Vec<ImmutableString> {
        match ty {
            // One trait bounding a parameter twice (`U: Convert<i64> +
            // Convert<String>`) is one trait here.
            Ty::Param(name) => {
                let mut traits = self
                    .generic_traits
                    .get(name)
                    .map(|traits| traits.to_vec())
                    .unwrap_or_default();
                let mut seen = BTreeSet::new();
                traits.retain(|name| seen.insert(name.clone()));
                traits
            }
            Ty::Projection(_, trait_name, name) => self
                .associated_bounds(trait_name, name)
                .map(|(_, traits)| traits.iter().map(|name| name.text.clone()).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Each associated type of a trait impl satisfies the bounds its trait
    /// declares for it (`type Output: Clone + Shape;`).
    pub(super) fn check_associated_bounds(
        &mut self,
        implementation: &item::Impl,
    ) -> Result<(), Error> {
        let Some(item::TraitImpl::User {
            name: trait_name,
            arguments: impl_arguments,
        }) = &implementation.trait_
        else {
            return Ok(());
        };
        self.generics = implementation
            .generics
            .iter()
            .map(|generic| (generic.name.text.clone(), generic.bounds))
            .collect();
        self.generic_traits = generic_traits(&implementation.generics);
        self.generic_trait_arguments = generic_trait_arguments(&implementation.generics);
        self.inference.equalities = generic_equalities(&implementation.generics);
        // `impl Named for C` needs `C: Shape` for `trait Named: Shape`
        // (E0277).
        if let Some((_, declared)) = self.models.traits.get(&trait_name.text).cloned() {
            let (bounds, traits) = declared.supertraits.clone();
            let owner = Ty::from_resolved(&implementation.owner);
            let missing = bound_kinds(bounds)
                .find(|kind| !self.type_provides(&owner, *kind))
                .map(|kind| ImmutableString::from(kind.name()))
                .or_else(|| {
                    traits
                        .iter()
                        .find(|bound| !self.type_implements(&owner, &bound.text))
                        .map(|bound| bound.text.clone())
                });
            if let Some(capability) = missing {
                return Err(Error::MissingCapability {
                    owner: owner.name(&self.models),
                    capability,
                    operation: trait_name.span,
                    derive: matches!(owner, Ty::Script(..)),
                    required_by: None,
                });
            }
            // `impl Both<i64> for C` needs `C: Convert<i64>` for `trait
            // Both<T>: Convert<T>`, and `impl Ord2 for C` needs `C:
            // PartialOrd2<C>` for `trait Ord2: PartialOrd2<Self>`.
            let mut substitution: BTreeMap<ImmutableString, Ty> = declared
                .generics
                .iter()
                .map(|generic| generic.name.text.clone())
                .zip(impl_arguments.iter().map(Ty::from_resolved))
                .collect();
            substitution.insert("Self".into(), owner.clone());
            for (parent, arguments) in declared.supertrait_arguments.iter() {
                let arguments = arguments
                    .iter()
                    .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                    .collect::<Vec<_>>();
                if !self.type_implements_with(&owner, parent, &arguments) {
                    let names = arguments
                        .iter()
                        .map(|ty| ty.name(&self.models).to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(Error::MissingCapability {
                        owner: owner.name(&self.models),
                        capability: format!("{parent}<{names}>").into(),
                        operation: trait_name.span,
                        derive: matches!(owner, Ty::Script(..)),
                        required_by: None,
                    });
                }
            }
        }
        let owner = Ty::from_resolved(&implementation.owner);
        for (name, ty) in &implementation.associated {
            let Some((bounds, traits)) = self
                .associated_bounds(&trait_name.text, &name.text)
                .cloned()
            else {
                continue;
            };
            let ty = Ty::from_resolved(ty);
            // `type Item: Into<String>;`: the generic trait's instance, its
            // `Self` the impl's owner.
            let instances = self.associated_bound_instances(&trait_name.text, &name.text, &owner);
            let missing = bound_kinds(bounds)
                .find(|kind| !self.type_provides(&ty, *kind))
                .map(|kind| ImmutableString::from(kind.name()))
                .or_else(|| {
                    traits
                        .iter()
                        .filter(|bound| !instances.iter().any(|(name, _)| *name == bound.text))
                        .find(|bound| !self.type_implements(&ty, &bound.text))
                        .map(|bound| bound.text.clone())
                })
                .or_else(|| {
                    instances
                        .iter()
                        .find(|(bound, arguments)| {
                            !self.type_implements_with(&ty, bound, arguments)
                        })
                        .map(|(bound, arguments)| {
                            let names = arguments
                                .iter()
                                .map(|ty| ty.name(&self.models).to_string())
                                .collect::<Vec<_>>()
                                .join(", ");
                            format!("{bound}<{names}>").into()
                        })
                });
            if let Some(capability) = missing {
                return Err(Error::MissingCapability {
                    owner: ty.name(&self.models),
                    capability,
                    operation: name.span,
                    derive: matches!(ty, Ty::Script(..)),
                    required_by: Some(format!("{}::{}", trait_name.text, name.text).into()),
                });
            }
            self.check_associated_equalities(&trait_name.text, name, &ty, &owner)?;
        }
        Ok(())
    }

    /// The impl's associated type `ty` for `name` has the types its trait's
    /// bound equalities name (rustc's E0271): `type Fore = Range<usize>;`
    /// for `type Fore: Iterator<Item = Self::For>;` needs `type For =
    /// usize;`.
    pub(super) fn check_associated_equalities(
        &mut self,
        trait_name: &ImmutableString,
        name: &crate::item::Name,
        ty: &Ty,
        owner: &Ty,
    ) -> Result<(), Error> {
        let Some(equalities) = self
            .inference
            .associated
            .equalities
            .get(&(trait_name.clone(), name.text.clone()))
            .cloned()
        else {
            return Ok(());
        };
        let substitution = BTreeMap::from([(ImmutableString::from("Self"), owner.clone())]);
        for (bound, item, expected) in equalities.iter() {
            let expected = instantiate(&Ty::from_resolved(expected), &substitution);
            let actual = if is_iterator_trait(bound) && item == "Item" {
                // A program's own iterator type: its impl names its `Item`,
                // which its uses read.
                if matches!(self.inference.resolve(ty), Ty::Script(..)) {
                    continue;
                }
                // Rawr's iterator type stands for the projection
                // (`Inference::project`), so the impl's type must be one.
                let Ty::Iter(item, _) = self.inference.resolve(ty) else {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: format!(
                            "`{}` as an associated type bounded by `{bound}<Item = ..>` is not \
                             supported in Rawr yet; only Rust's own iterators (a `Range`, an \
                             iterator chain) are",
                            ty.name(&self.models)
                        )
                        .into(),
                        source: name.span,
                    });
                };
                *item
            } else {
                Ty::Projection(Box::new(ty.clone()), bound.clone(), item.clone())
            };
            if !self.unify_type(&expected, &actual) {
                return Err(Error::TypeMismatch {
                    expected: format!(
                        "{} (`<{} as {bound}>::{item}`)",
                        self.inference.resolve(&expected).name(&self.models),
                        ty.name(&self.models)
                    )
                    .into(),
                    actual: self.inference.resolve(&actual).name(&self.models),
                    expression: name.span,
                });
            }
        }
        Ok(())
    }

    /// The generic traits among the bounds of the associated type `name` of
    /// `trait_name`, with their arguments for the implementing type `base`:
    /// `(Into, [String])` of `type Item: Into<String>;`.
    pub(super) fn associated_bound_instances(
        &self,
        trait_name: &str,
        name: &str,
        base: &Ty,
    ) -> Vec<(ImmutableString, Vec<Ty>)> {
        let Some((_, declared)) = self.models.traits.get(trait_name) else {
            return Vec::new();
        };
        let Some(index) = declared
            .associated
            .iter()
            .position(|declared| declared.text == name)
        else {
            return Vec::new();
        };
        let substitution = BTreeMap::from([(ImmutableString::from("Self"), base.clone())]);
        declared
            .associated_bound_arguments
            .get(index)
            .into_iter()
            .flatten()
            .map(|(bound, arguments)| {
                let arguments = arguments
                    .iter()
                    .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                    .collect();
                (bound.clone(), arguments)
            })
            .collect()
    }

    /// The argument lists `ty` is bounded by for the generic trait
    /// `trait_name`: a parameter's bounds (`U: Convert<i64>`), or an
    /// associated type's (`type Item: Into<String>;`).
    pub(super) fn bound_trait_arguments(&self, ty: &Ty, trait_name: &str) -> Vec<Box<[Ty]>> {
        match ty {
            Ty::Param(name) => self
                .generic_trait_arguments
                .get(&(name.clone(), trait_name.into()))
                .cloned()
                .unwrap_or_default(),
            Ty::Projection(base, owner_trait, name) => self
                .associated_bound_instances(owner_trait, name, base)
                .into_iter()
                .filter(|(bound, _)| bound == trait_name)
                .map(|(_, arguments)| arguments.into_boxed_slice())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The bounds of the associated type `name` of the trait `trait_name`:
    /// its capabilities and user traits (`type Output: Clone + Shape;`).
    pub(super) fn associated_bounds(
        &self,
        trait_name: &str,
        name: &str,
    ) -> Option<&(capability::Set, Box<[crate::item::Name]>)> {
        let (_, declared) = self.models.traits.get(trait_name)?;
        let index = declared
            .associated
            .iter()
            .position(|declared| declared.text == name)?;
        declared.associated_bounds.get(index)
    }

    /// The function `member` of a user trait bounding `bounded`, a generic
    /// parameter or an associated type, with that trait's name.
    pub(super) fn bound_trait_function(
        &self,
        bounded: &Ty,
        member: &str,
        span: Span,
    ) -> Result<Option<(ImmutableString, item::Function)>, Error> {
        let param = bounded.name(&self.models);
        let mut found = None;
        // A bound's supertraits' functions too, as `T: Ord` has
        // `partial_cmp`.
        let mut traits = self.bound_traits(bounded);
        for trait_name in traits.clone() {
            for parent in supertraits(&self.models, &trait_name) {
                if !traits.contains(&parent) {
                    traits.push(parent);
                }
            }
        }
        for trait_name in &traits {
            let Some((_, declared)) = self.models.traits.get(trait_name) else {
                continue;
            };
            if let Some(function) = declared
                .functions
                .iter()
                .find(|function| function.name.text == member)
            {
                if found.is_some() {
                    return Err(Error::InvalidMethodCall {
                        owner: param,
                        method: member.into(),
                        reason: "multiple applicable items in scope: more than one bound \
                                 trait has it"
                            .into(),
                        call: span,
                    });
                }
                found = Some((trait_name.clone(), function.clone()));
            }
        }
        Ok(found)
    }

    /// `::ok` after `Result`, or `::trim` after `str`: the method a path
    /// names, called or not. A variant (`Option::Some`) or a path that goes
    /// on (`str::from_utf8::<..>`, `Result::Ok::<..>`) is not one.
    pub(super) fn std_method_member(&self) -> Option<ImmutableString> {
        let mut ahead = self.input.clone();
        if !matches!(ahead.next(), Some((Token::DoubleColon, _))) {
            return None;
        }
        let Some((Token::Identifier(member), _)) = ahead.next() else {
            return None;
        };
        if !member.starts_with(|c: char| c.is_ascii_lowercase())
            || matches!(
                ahead.peek(),
                Some((Token::DoubleColon | Token::LessThan, _))
            )
        {
            return None;
        }
        Some((*member).as_str().into())
    }

    /// How the std trait `trait_name`'s function `member` takes `self`, as
    /// std's table declares it: `Some(Some("&mut self"))` for
    /// `Iterator::next`, `Some(None)` for `FromStr::from_str`, `None` when
    /// the trait has no such function.
    pub(super) fn std_receiver(
        &self,
        trait_name: &str,
        member: &str,
    ) -> Option<Option<&'static str>> {
        Some(self.std_trait_function(trait_name, member)?.receiver())
    }

    /// Whether `member` is a method the program declares for the type
    /// `receiver` names through its references: its own, or its trait's.
    /// Whether `member` is a method the crate gives `receiver`'s host type.
    pub(super) fn host_method(&self, receiver: &Ty, member: &ImmutableString) -> bool {
        #[cfg(feature = "rust-import")]
        return self.host_has_member(receiver, member);
        #[cfg(not(feature = "rust-import"))]
        {
            let _ = (receiver, member);
            false
        }
    }

    pub(super) fn program_method(&self, receiver: &Ty, member: &ImmutableString) -> bool {
        let mut receiver = self.inference.resolve(receiver);
        while let Ty::Reference { target, .. } | Ty::Boxed(target) = receiver {
            receiver = self.inference.resolve(&target);
        }
        owner_id(&receiver).is_some_and(|id| {
            self.models.method(id, member).is_some()
                || self
                    .models
                    .trait_methods
                    .contains_key(&(id, member.clone()))
        })
    }
}

/// The tag of an impl's self type `ty`, as `object::tag` tags the type a
/// program's value is coerced from (D872).
#[cfg(feature = "rust-import")]
fn implementor_tag(ty: &crate::host::type_ref::Type) -> Option<String> {
    use crate::host::type_ref::{Builtin as Host, Shape};
    use rawr_import::classify::representability::convert::{Builtin, Type};
    let converted = match ty.shape() {
        Shape::Builtin(builtin) => Type::Builtin(match builtin {
            Host::Bool => Builtin::Bool,
            Host::Char => Builtin::Char,
            Host::F64 => Builtin::F64,
            Host::F32 => Builtin::F32,
            Host::I64 => Builtin::I64,
            Host::I8 => Builtin::I8,
            Host::I16 => Builtin::I16,
            Host::I32 => Builtin::I32,
            Host::Isize => Builtin::Isize,
            Host::U8 => Builtin::U8,
            Host::U16 => Builtin::U16,
            Host::U32 => Builtin::U32,
            Host::U64 => Builtin::U64,
            Host::Usize => Builtin::Usize,
            Host::Arguments => Builtin::Arguments,
            _ => return None,
        }),
        Shape::Imported {
            identity,
            arguments,
        } if arguments.is_empty() => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::authenticated(
                *identity.as_bytes(),
            ),
            arguments: Box::default(),
        },
        _ => return None,
    };
    rawr_import::classify::representability::object::tag(&converted)
}
