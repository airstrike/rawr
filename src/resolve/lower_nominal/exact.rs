//! The exact native instances the runner compiles, bound up front
//! (#2698). A call of a crate trait's method over its `Self`, as `column!`'s
//! `Widget::<Message, Theme, Renderer>::_boxed` of iced's `Text`, fixes the
//! trait's arguments to types the runner holds: the crates' own and its
//! stand-ins for the program's, as `crate::Message`. Every native impl of
//! the trait at those arguments whose own parameters they fix, and whose
//! where-clause bounds hold, is then an instance the runner can compile
//! whatever the program writes next, so each member of it is bound: the
//! impl's own methods, and every member of the trait over `Self` at the
//! impl's self type, which a generic body's call through its bound
//! selects. An impl parameter neither the trait's arguments nor the self
//! type fixes, which the impl bounds by the trait itself, as `W` of
//! `impl<W, ..> Widget<..> for Row<W>`, is each settled type the trait's
//! own members over `Self` give there that implements it, as `_boxed`
//! gives `Element`: `row!` boxes each child so, and the runner then holds
//! `Row<Element>`. Each member is bound once, by the item it defines and its
//! types, whichever of the trait's paths spells it (D1118): a trait iced
//! re-exports under a dozen chains is one set of adapters, and a use
//! spelled through any of them is that adapter's. A later save whose
//! source runs a generic body over such a value,
//! as iced's `Container<Text>::draw` calls `self.content.draw(..)`, then
//! finds the member in the runner. This binds native items only and makes
//! no instance of the program's.

use super::surface::{fixed_parameter, fixed_surface_type, imported_surface_type, ImportedUse};
use super::ty::{contains_inference, reaches_param, Ty};
use super::Parser;
use crate::host::defined::crate_item;
use crate::host::facade::{Dependency, ImplHeader};
use crate::host::type_ref::Shape;
use crate::ImmutableString;
use crate::Span;
use std::collections::BTreeMap;

/// How deep a bound's own bounds are followed before one is taken to hold.
const BOUND_DEPTH: usize = 4;

impl Parser<'_> {
    /// Binds each member of every native impl of the trait whose method
    /// over `Self` is `item`, at the trait arguments the call gives in
    /// `requirements`, when each is settled.
    pub(super) fn bind_exact_instances(
        &mut self,
        item: &crate::host::item::Definition,
        requirements: &[(ImmutableString, Ty, crate::capability::Set)],
        span: Span,
    ) {
        let path = item.path();
        if !rawr_import::surface::member::declared(&path) {
            return;
        }
        let Some(trait_path) = path.rsplitn(3, "::").nth(2) else {
            return;
        };
        let Ok(parsed) = crate::host::path::Path::parse(trait_path) else {
            return;
        };
        let host = self.models.host.clone();
        let Some(header) = host.trait_header(&parsed) else {
            return;
        };
        let identity = header.identity;
        let written = header
            .parameters
            .iter()
            .map(|name| {
                let (_, ty, _) = requirements.iter().find(|(own, ..)| own == name)?;
                Some(self.inference.resolve(ty))
            })
            .collect::<Option<Vec<_>>>();
        // A reading for the program's instances leaves the arguments the
        // body's bound on the receiver gives to the check: they are the
        // bound's, as rustc selects a where-clause first.
        let given = written.and_then(|written| {
            if written.iter().all(settled) {
                return Some(written);
            }
            let own = rawr_import::classify::representability::signature::argument_impl_name(0);
            let (_, receiver, _) = requirements.iter().find(|(name, ..)| **name == *own)?;
            let receiver = self.inference.resolve(receiver);
            self.param_env_arguments(&receiver, identity)
                .filter(|bound| bound.len() == written.len() && bound.iter().all(settled))
        });
        let Some(given) = given else {
            return;
        };
        // A reading for the program's instances seeds the demand reads of
        // the opened impls of the trait the use fixes (decision 3); the
        // check, and a demand read itself, bind the native ones.
        if self.collecting_instances {
            self.seed_demand(identity, &given);
        }
        if self.demand.is_some() {
            // A pass of demand reads binds each instance once.
            let names = given
                .iter()
                .map(|ty| ImmutableString::from(ty.name(&self.models).to_string()))
                .collect::<Box<[_]>>();
            if self.demand_bound.insert((identity, None, names.clone())) {
                self.bind_trait_instance(identity, &given, None, span, true);
            }
            let member = Some(ImmutableString::from(path.as_str()));
            if !self
                .demand_bound
                .contains(&(identity, member.clone(), names.clone()))
            {
                let bound = self.bind_through_bound(item, requirements, identity, &given, span);
                if bound {
                    self.demand_bound.insert((identity, member, names));
                }
            }
        } else if !self.collecting_instances {
            self.bind_trait_instance(identity, &given, None, span, true);
        }
    }

    /// A demand read's call of the trait's member `item` over `Self` on an
    /// open parameter bounded by the trait at `given`, as `self.content
    /// .operate(..)` of `W: Widget<..>` in `Container`'s impl, bound at the
    /// self type of each native impl of the trait at `given` (decision 2)
    /// with the call's other types, as rustc compiles it for each `W` the
    /// runner holds. The call gives what the trait's members alone don't,
    /// as the object `dyn Operation<()>` the `&mut dyn Operation` input is.
    /// Whether the call was such a call.
    fn bind_through_bound(
        &mut self,
        item: &crate::host::item::Definition,
        requirements: &[(ImmutableString, Ty, crate::capability::Set)],
        identity: [u8; 32],
        given: &[Ty],
        span: Span,
    ) -> bool {
        use rawr_import::classify::representability::signature::argument_impl_name;
        let own = argument_impl_name(0);
        let mut receiver = None;
        let mut types = Vec::with_capacity(requirements.len());
        for (name, ty, _) in requirements {
            let ty = self.inference.resolve(ty);
            if **name == *own {
                receiver = Some(ty);
            } else if settled(&ty) {
                types.push((name.clone(), ty));
            } else {
                return false;
            }
        }
        if !matches!(receiver, Some(Ty::Param(..))) {
            return false;
        }
        let host = self.models.host.clone();
        let mut headers = Vec::<(&ImplHeader, Option<&Dependency>)>::new();
        for (header, owner) in host.owned_impl_headers(identity) {
            if !headers.iter().any(|(kept, _)| *kept == header) {
                headers.push((header, owner));
            }
        }
        let order = item
            .signature()
            .declared_type_parameters()
            .collect::<Vec<_>>();
        let filling = self.filling(identity, given);
        for &(header, owner) in &headers {
            let alike = headers
                .iter()
                .filter(|(other, _)| other.self_type == header.self_type)
                .count();
            if alike > 1 {
                continue;
            }
            for found in self.exact_instances(header, owner, given, None, &filling) {
                let mut at = types.clone();
                at.push((own.as_ref().into(), found.self_type));
                at.sort_by_key(|(name, _)| order.iter().position(|declared| declared == name));
                self.bind_member(item, Some(at), None, span);
            }
        }
        true
    }

    /// Each script impl of the crate trait `identity` from an opened
    /// module that the trait arguments `given` fit, with the impl's own
    /// parameters they fix and whose bounds hold there: a demand read's
    /// seed (decision 3). A parameter they leave open, as the content `W`
    /// of iced's `impl Widget<Message, Theme, Renderer> for Container<'_,
    /// W, Theme>`, stays the impl's, read through its bound.
    fn seed_demand(&mut self, identity: [u8; 32], given: &[Ty]) {
        let host = self.models.host.clone();
        let scripts = self
            .models
            .script_impls
            .iter()
            .filter(|script| {
                crate::host::path::Path::parse(script.path.as_str())
                    .ok()
                    .and_then(|path| host.trait_header(&path))
                    .is_some_and(|header| header.identity == identity)
            })
            .cloned()
            .collect::<Vec<_>>();
        for script in &scripts {
            let checkpoint = self.inference.checkpoint();
            let fixed = self.fixed_parameters(script, identity, given);
            self.inference.rollback(checkpoint);
            let Some(fixed) = fixed.filter(|fixed| !fixed.is_empty()) else {
                continue;
            };
            let seed = super::demand::Seed {
                index: script.index,
                fixed,
            };
            if !self.demand_seeds.contains(&seed) {
                self.demand_seeds.push(seed);
            }
        }
    }

    /// The parameters of `script` that `given` fixes to settled types, when
    /// it fits and the crate bound on each holds there.
    fn fixed_parameters(
        &mut self,
        script: &super::models::ScriptTraitImpl,
        identity: [u8; 32],
        given: &[Ty],
    ) -> Option<Vec<(ImmutableString, Ty)>> {
        let owner = self.inference.fresh();
        let (types, _) = self.apply_script_impl(script, &owner, identity, given)?;
        let substitution = types.iter().cloned().collect::<BTreeMap<_, _>>();
        let host = self.models.host.clone();
        for (name, bound) in script.crate_bounds.iter() {
            let bounded = self.inference.resolve(&substitution[name]);
            if !settled(&bounded) {
                continue;
            }
            let Some(header) = bound
                .opaque_trait()
                .and_then(|path| crate::host::path::Path::parse(path).ok())
                .and_then(|path| host.trait_header(&path))
            else {
                continue;
            };
            let arguments = bound
                .opaque_arguments()
                .into_iter()
                .map(|argument| {
                    super::generics::instantiate(&Ty::from_resolved(argument), &substitution)
                })
                .collect::<Vec<_>>();
            if !self.bound_holds(&bounded, header.identity, &arguments, None, 0) {
                return None;
            }
        }
        Some(
            types
                .into_iter()
                .filter_map(|(name, ty)| {
                    let ty = self.inference.resolve(&ty);
                    settled(&ty).then_some((name, ty))
                })
                .collect(),
        )
    }

    /// Binds each member of every native impl of the crate trait
    /// `identity` at the trait arguments `given`, for the self type `fixed`
    /// when there is one. With `bounds`, each where-clause bound of a
    /// fitting impl at settled types is such an instance too, for its
    /// bounded type, one level down: an opened impl's body runs from
    /// source and calls through them, as `Container`'s `draw` calls
    /// `Catalog::style` at iced's `Theme` and `Renderer::fill_quad` at its
    /// renderer, and rustc compiles each for the impl's instance.
    fn bind_trait_instance(
        &mut self,
        identity: [u8; 32],
        given: &[Ty],
        fixed: Option<&Ty>,
        span: Span,
        bounds: bool,
    ) {
        let host = self.models.host.clone();
        let paths = host.trait_paths(&identity);
        let Some(first) = paths.first() else {
            return;
        };
        let names = header_names(&host, first);
        // The trait's stand-in is the runner's one type for a program's
        // value of it at these arguments, as a program's widget given to
        // `column!`'s `_boxed` is: each member over `Self` that takes the
        // value itself is bound at it.
        if bounds {
            self.bind_stand_in_members(identity, given, &paths, &names, span);
        }
        let mut headers = Vec::<(&ImplHeader, Option<&Dependency>)>::new();
        for (header, owner) in host.owned_impl_headers(identity) {
            if !headers.iter().any(|(kept, _)| *kept == header) {
                headers.push((header, owner));
            }
        }
        let filling = self.filling(identity, given);
        let found = headers
            .iter()
            .flat_map(|&(header, owner)| {
                self.exact_instances(header, owner, given, fixed, &filling)
                    .into_iter()
                    .map(move |found| (header, found))
            })
            .collect::<Vec<_>>();
        for (header, found) in found {
            let Ty::Host { proof, .. } = &found.self_type else {
                continue;
            };
            let owner_path = proof.definition.path();
            // Two impls of the trait for one type, at other arguments,
            // would share the type's members: neither is bound.
            let alike = headers
                .iter()
                .filter(|(other, _)| other.self_type == header.self_type)
                .count();
            if alike > 1 {
                continue;
            }
            for member in host.members(&owner_path) {
                if member.trait_of() == Some(&identity) {
                    let types =
                        member_parameters(member, &header.parameters, &found.parameters, None);
                    self.bind_member(member, types, None, span);
                }
            }
            // The trait's members over `Self`: a generic body's call through
            // its bound selects these, whether the impl overrides the member
            // or leaves it. Each is bound once, under the first of the
            // trait's paths that has it (`bind_member` keys it by its item).
            let trait_parameters = header_arguments(&names, given);
            for written in &paths {
                for member in host.members(&format!("{written}::__rawr_self")) {
                    let types =
                        member_parameters(member, &[], &trait_parameters, Some(&found.self_type));
                    self.bind_member(member, types, None, span);
                }
            }
            if bounds {
                for (bounded, inner, arguments) in &found.bounds {
                    self.bind_trait_instance(*inner, arguments, Some(bounded), span, false);
                }
            }
        }
    }

    /// The settled types the outputs of the crate trait `identity`'s own
    /// members over `Self` are at the trait arguments `given`, whatever
    /// `Self` is, that implement the trait there: iced's `Element<'_,
    /// Message, Theme, Renderer>`, which `Widget::_boxed` gives. Each
    /// fills an impl parameter only a bound by the trait constrains
    /// (`exact_instances`), as `W` of `impl<W, ..> Widget<..> for Row<W>`,
    /// so the runner holds `Row<Element>` that `row!` makes. Computed once
    /// per trait instance.
    fn filling(&mut self, identity: [u8; 32], given: &[Ty]) -> std::rc::Rc<[Ty]> {
        let names = given
            .iter()
            .map(|ty| ImmutableString::from(ty.name(&self.models).to_string()))
            .collect::<Box<[_]>>();
        let key = (identity, names);
        if let Some(found) = self.fillings.get(&key) {
            return found.clone();
        }
        let host = self.models.host.clone();
        let paths = host.trait_paths(&identity);
        let trait_parameters = paths
            .first()
            .map(|first| header_arguments(&header_names(&host, first), given))
            .unwrap_or_default();
        let headers = host.owned_impl_headers(identity).collect::<Vec<_>>();
        let mut found = Vec::<Ty>::new();
        for written in &paths {
            for member in host.members(&format!("{written}::__rawr_self")) {
                let owner = host.item_owner(&member.path()).cloned();
                let mut parameters = trait_parameters.clone();
                let checkpoint = self.inference.checkpoint();
                let output = self
                    .instantiate_host_type(
                        member.signature().output(),
                        &mut parameters,
                        "impl",
                        Span::NONE,
                        owner.as_ref(),
                        None,
                    )
                    .ok()
                    .map(|output| self.inference.resolve(&output));
                self.inference.rollback(checkpoint);
                let Some(output) = output.filter(|output| {
                    matches!(output, Ty::Host { .. }) && settled(output) && !found.contains(output)
                }) else {
                    continue;
                };
                let implements = headers.iter().any(|&(header, owner)| {
                    self.exact_instance(header, owner, given, Some(&output), &BTreeMap::new())
                        .is_some()
                });
                if implements {
                    found.push(output);
                }
            }
        }
        let found = std::rc::Rc::<[Ty]>::from(found);
        self.fillings.insert(key, found.clone());
        found
    }

    /// `header` at the trait arguments `given`, for the self type `fixed`
    /// when there is one, as `exact_instance` gives it. A parameter neither
    /// fixes, that the impl bounds by its own trait, as `W` of `impl<W,
    /// Message, Theme, Renderer> Widget<Message, Theme, Renderer> for
    /// Row<W> where W: Widget<Message, Theme, Renderer>`, is each of
    /// `filling` in turn.
    fn exact_instances(
        &mut self,
        header: &ImplHeader,
        owner: Option<&Dependency>,
        given: &[Ty],
        fixed: Option<&Ty>,
        filling: &[Ty],
    ) -> Vec<Exact> {
        if let Some(found) = self.exact_instance(header, owner, given, fixed, &BTreeMap::new()) {
            return vec![found];
        }
        if filling.is_empty() || header.arguments.len() != given.len() {
            return Vec::new();
        }
        let checkpoint = self.inference.checkpoint();
        let open =
            self.fit(header, owner, given, fixed, &BTreeMap::new())
                .map(|(parameters, _)| {
                    parameters
                        .into_iter()
                        .filter(|(_, ty)| !settled(&self.inference.resolve(ty)))
                        .map(|(name, _)| name)
                        .collect::<Vec<_>>()
                });
        self.inference.rollback(checkpoint);
        let Some(open) = open.filter(|open| !open.is_empty()) else {
            return Vec::new();
        };
        let bounded_by_its_trait = |name: &ImmutableString| {
            header.bounds.iter().any(|bound| {
                bound.trait_identity == header.trait_identity
                    && matches!(bound.ty.shape(), Shape::Parameter(own) if *own.to_string() == **name)
            })
        };
        if !open.iter().all(bounded_by_its_trait) {
            return Vec::new();
        }
        let mut fills = vec![BTreeMap::new()];
        for name in &open {
            fills = fills
                .into_iter()
                .flat_map(|fill: BTreeMap<ImmutableString, Ty>| {
                    filling.iter().map(move |ty| {
                        let mut fill = fill.clone();
                        fill.insert(name.clone(), ty.clone());
                        fill
                    })
                })
                .collect();
        }
        fills
            .iter()
            .filter_map(|fill| self.exact_instance(header, owner, given, fixed, fill))
            .collect()
    }

    /// Binds each member over `Self` of the trait `identity`, written at
    /// `paths` with the parameters `names`, that takes `Self` by value and
    /// names it nowhere else, at `given` with the trait's stand-in as
    /// `Self`, when the trait has an object (`object_stand_in.rs`).
    fn bind_stand_in_members(
        &mut self,
        identity: [u8; 32],
        given: &[Ty],
        paths: &[crate::host::path::Path],
        names: &[ImmutableString],
        span: Span,
    ) {
        use rawr_import::classify::representability::signature::argument_impl_name;
        let host = self.models.host.clone();
        let Some(trait_name) = host.trait_name(&identity) else {
            return;
        };
        let has_object = crate::host::path::Path::parse(&trait_name)
            .ok()
            .and_then(|path| host.trait_header(&path))
            .is_some_and(|header| header.object.is_some());
        if !has_object || !self.models.native_trait(&trait_name) {
            return;
        }
        let stand_in = Ty::Dyn(
            trait_name,
            given
                .iter()
                .enumerate()
                .map(|(index, argument)| (index.to_string().into(), argument.clone()))
                .collect(),
            super::ty::Auto::Unknown,
        );
        let own = argument_impl_name(0);
        let trait_parameters = header_arguments(names, given);
        for written in paths {
            for member in host.members(&format!("{written}::__rawr_self")) {
                let signature = member.signature();
                let by_value = signature.parameters().iter().any(|parameter| {
                    matches!(parameter.shape(), Shape::Parameter(name) if *name.to_string() == *own)
                });
                let named = signature
                    .parameters()
                    .iter()
                    .chain([signature.output()])
                    .filter(|ty| ty.mentions_parameter(&own))
                    .count();
                if !by_value || named != 1 {
                    continue;
                }
                let types = member_parameters(member, &[], &trait_parameters, Some(&stand_in));
                self.bind_member(member, types, Some(&own), span);
            }
        }
    }

    /// `header` at the trait arguments `given`, for the self type `fixed`
    /// when there is one: its self type, its own parameters and its
    /// where-clause bounds at settled types, when `given` fixes each
    /// parameter and the bounds hold.
    fn exact_instance(
        &mut self,
        header: &ImplHeader,
        owner: Option<&Dependency>,
        given: &[Ty],
        fixed: Option<&Ty>,
        fill: &BTreeMap<ImmutableString, Ty>,
    ) -> Option<Exact> {
        if header.arguments.len() != given.len() {
            return None;
        }
        let checkpoint = self.inference.checkpoint();
        let found = self.exact_at(header, owner, given, fixed, fill);
        self.inference.rollback(checkpoint);
        found
    }

    fn exact_at(
        &mut self,
        header: &ImplHeader,
        owner: Option<&Dependency>,
        given: &[Ty],
        fixed: Option<&Ty>,
        fill: &BTreeMap<ImmutableString, Ty>,
    ) -> Option<Exact> {
        let (parameters, self_type) = self.fit(header, owner, given, fixed, fill)?;
        let self_type = self.inference.resolve(&self_type);
        if !settled(&self_type) {
            return None;
        }
        let parameters = parameters
            .into_iter()
            .map(|(name, ty)| {
                let ty = self.inference.resolve(&ty);
                settled(&ty).then_some((name, ty))
            })
            .collect::<Option<BTreeMap<_, _>>>()?;
        let held = self.apply_impl(header, &self_type, given, owner)?;
        let mut bounds = Vec::with_capacity(held.traits.len());
        for (bounded, identity, arguments) in &held.traits {
            if !self.bound_holds(bounded, *identity, arguments, owner, 0) {
                return None;
            }
            let bounded = self.inference.resolve(bounded);
            let arguments = arguments
                .iter()
                .map(|argument| self.inference.resolve(argument))
                .collect::<Vec<_>>();
            if settled(&bounded) && arguments.iter().all(settled) {
                bounds.push((bounded, *identity, arguments));
            }
        }
        Some(Exact {
            self_type,
            parameters,
            bounds,
        })
    }

    /// `header`'s parameters and self type, unified with the trait
    /// arguments `given` and the self type `fixed`, each parameter `fill`
    /// names at its type: what is left open stays open.
    fn fit(
        &mut self,
        header: &ImplHeader,
        owner: Option<&Dependency>,
        given: &[Ty],
        fixed: Option<&Ty>,
        fill: &BTreeMap<ImmutableString, Ty>,
    ) -> Option<(BTreeMap<ImmutableString, Ty>, Ty)> {
        let mut parameters = BTreeMap::new();
        for name in &header.parameters {
            let ty = fill
                .get(name)
                .cloned()
                .unwrap_or_else(|| self.inference.fresh());
            parameters.insert(name.clone(), ty);
        }
        for (written, argument) in header.arguments.iter().zip(given) {
            let written = self
                .instantiate_host_type(written, &mut parameters, "impl", Span::NONE, owner, None)
                .ok()?;
            if !self.unify_type(&written, argument) {
                return None;
            }
        }
        let self_type = self
            .instantiate_host_type(
                &header.self_type,
                &mut parameters,
                "impl",
                Span::NONE,
                owner,
                None,
            )
            .ok()?;
        if let Some(fixed) = fixed
            && !self.unify_type(&self_type, fixed)
        {
            return None;
        }
        Some((parameters, self_type))
    }

    /// Whether some impl of the crate trait `identity` at `arguments` fits
    /// `ty`, with its own bounds, as rustc proves an impl's where-clause.
    /// A trait no facade has an impl of, as std's, and a type still open
    /// are taken to hold: rustc proves those when the runner builds.
    fn bound_holds(
        &mut self,
        ty: &Ty,
        identity: [u8; 32],
        arguments: &[Ty],
        owner: Option<&Dependency>,
        depth: usize,
    ) -> bool {
        let ty = self.inference.resolve(ty);
        if depth > BOUND_DEPTH || !settled(&ty) {
            return true;
        }
        let host = self.models.host.clone();
        let headers = host.owned_impl_headers(identity).collect::<Vec<_>>();
        if headers.is_empty() {
            return true;
        }
        headers.into_iter().any(|(header, declaring)| {
            let checkpoint = self.inference.checkpoint();
            let holds = self
                .apply_impl(header, &ty, arguments, declaring.or(owner))
                .is_some_and(|bounds| {
                    bounds
                        .traits
                        .iter()
                        .all(|(bounded, inner, inner_arguments)| {
                            self.bound_holds(bounded, *inner, inner_arguments, owner, depth + 1)
                        })
                });
            self.inference.rollback(checkpoint);
            holds
        })
    }

    /// The use of `member` at `types`, its declared parameters' types, among
    /// the body's uses once: a use of the item it defines at the same types
    /// is that use, whichever path spells either.
    fn bind_member(
        &mut self,
        member: &crate::host::item::Definition,
        types: Option<Vec<(ImmutableString, Ty)>>,
        stand_in: Option<&str>,
        span: Span,
    ) {
        let Some(types) = types else {
            return;
        };
        if member.signature().takes_callable()
            || member
                .signature()
                .declared_length_parameters()
                .next()
                .is_some()
        {
            return;
        }
        let Some(selector) = member
            .imported_identity()
            .and_then(|selector| selector.try_clone().ok())
        else {
            return;
        };
        let Some(context) = self.imported.as_mut() else {
            return;
        };
        let mut surface = Vec::with_capacity(types.len());
        for (name, ty) in &types {
            let convert = if fixed_parameter(member, name) {
                fixed_surface_type
            } else {
                imported_surface_type
            };
            match convert(&self.inference, &self.models, ty, span, context) {
                Ok(ty) => surface.push(match stand_in {
                    Some(stand_in) => {
                        super::object_stand_in::standing_in(ty, name, &[stand_in.into()])
                    }
                    None => ty,
                }),
                Err(_) => return,
            }
        }
        let path: ImmutableString = member.path().into();
        let surface = surface.into_boxed_slice();
        if context.uses.iter().any(|used| {
            used.selector == selector
                && used.types == surface
                && (used.path == path || crate_item(&selector).is_some())
        }) {
            return;
        }
        context.uses.push(ImportedUse {
            source: context.source,
            path,
            selector,
            types: surface,
            constants: Box::default(),
            functions: Box::default(),
            mappers: Box::default(),
            callable: false,
            slots: Box::default(),
            fillers: Box::default(),
            application: member.application().copied(),
            span,
        });
    }
}

/// The types each crate trait instance's own members over `Self` give
/// that implement it there, by the trait and its arguments' names
/// (`Parser::filling`).
pub(super) type Fillings = BTreeMap<([u8; 32], Box<[ImmutableString]>), std::rc::Rc<[Ty]>>;

/// An impl header at an instance (`exact_instance`).
struct Exact {
    self_type: Ty,
    parameters: BTreeMap<ImmutableString, Ty>,
    /// Its where-clause bounds at settled types: the bounded type, the
    /// trait's identity and its arguments.
    bounds: Vec<(Ty, [u8; 32], Vec<Ty>)>,
}

/// The trait header's parameter names at `parsed`.
fn header_names(
    host: &crate::host::Registry,
    parsed: &crate::host::path::Path,
) -> Vec<ImmutableString> {
    host.trait_header(parsed)
        .map(|header| header.parameters.to_vec())
        .unwrap_or_default()
}

/// The trait's parameters by name, at `given`.
fn header_arguments(names: &[ImmutableString], given: &[Ty]) -> BTreeMap<ImmutableString, Ty> {
    names.iter().cloned().zip(given.iter().cloned()).collect()
}

/// The types of `member`'s declared type parameters, in its order: each
/// named as one of `parameters` is that one's, and each the member names
/// by its index (`argument_impl_index`) is, in index order, the next of
/// `declared` it doesn't name, else `self_type`. `None` when one is the
/// member's own, which no instance fixes, or they don't pair one to one.
fn member_parameters(
    member: &crate::host::item::Definition,
    declared: &[ImmutableString],
    parameters: &BTreeMap<ImmutableString, Ty>,
    self_type: Option<&Ty>,
) -> Option<Vec<(ImmutableString, Ty)>> {
    let names = member
        .signature()
        .declared_type_parameters()
        .collect::<Vec<_>>();
    let mut unnamed = declared
        .iter()
        .filter(|name| !names.contains(name))
        .filter_map(|name| parameters.get(name).cloned())
        .chain(self_type.cloned());
    let mut indexed = names
        .iter()
        .filter_map(|name| {
            rawr_import::classify::representability::signature::argument_impl_index(name)
                .map(|index| (index, name.clone()))
        })
        .collect::<Vec<_>>();
    indexed.sort_by_key(|(index, _)| *index);
    let mut by_index = BTreeMap::new();
    for (_, name) in indexed {
        by_index.insert(name, unnamed.next()?);
    }
    names
        .into_iter()
        .map(|name| {
            let ty = parameters
                .get(&name)
                .or_else(|| by_index.get(&name))
                .cloned()?;
            Some((name, ty))
        })
        .collect()
}

/// Whether `ty` holds nothing left to infer and no parameter.
fn settled(ty: &Ty) -> bool {
    !contains_inference(ty) && !reaches_param(ty)
}
