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
//! selects. Each member is bound once, by the item it defines and its
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
        let Some(given) = header
            .parameters
            .iter()
            .map(|name| {
                let (_, ty, _) = requirements.iter().find(|(own, ..)| own == name)?;
                let ty = self.inference.resolve(ty);
                settled(&ty).then_some(ty)
            })
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let mut headers = Vec::<(&ImplHeader, Option<&Dependency>)>::new();
        for (header, owner) in host.owned_impl_headers(identity) {
            if !headers.iter().any(|(kept, _)| *kept == header) {
                headers.push((header, owner));
            }
        }
        for &(header, owner) in &headers {
            let Some((self_type, parameters)) = self.exact_instance(header, owner, &given) else {
                continue;
            };
            let Ty::Host { proof, .. } = &self_type else {
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
                    let types = member_parameters(member, &header.parameters, &parameters, None);
                    self.bind_member(member, types, span);
                }
            }
            // The trait's members over `Self`: a generic body's call through
            // its bound selects these, whether the impl overrides the member
            // or leaves it. Each is bound once, under the first of the
            // trait's paths that has it (`bind_member` keys it by its item).
            let trait_parameters = header_arguments(&header_names(&host, &parsed), &given);
            for written in host.trait_paths(&identity) {
                for member in host.members(&format!("{written}::__rawr_self")) {
                    let types = member_parameters(member, &[], &trait_parameters, Some(&self_type));
                    self.bind_member(member, types, span);
                }
            }
        }
    }

    /// `header` at the trait arguments `given`: its self type and its own
    /// parameters, when `given` fixes each and its where-clause bounds hold.
    fn exact_instance(
        &mut self,
        header: &ImplHeader,
        owner: Option<&Dependency>,
        given: &[Ty],
    ) -> Option<(Ty, BTreeMap<ImmutableString, Ty>)> {
        if header.arguments.len() != given.len() {
            return None;
        }
        let checkpoint = self.inference.checkpoint();
        let found = self.exact_at(header, owner, given);
        self.inference.rollback(checkpoint);
        found
    }

    fn exact_at(
        &mut self,
        header: &ImplHeader,
        owner: Option<&Dependency>,
        given: &[Ty],
    ) -> Option<(Ty, BTreeMap<ImmutableString, Ty>)> {
        let mut parameters = BTreeMap::new();
        for name in &header.parameters {
            parameters.insert(name.clone(), self.inference.fresh());
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
        let bounds = self.apply_impl(header, &self_type, given, owner)?;
        bounds
            .traits
            .iter()
            .all(|(bounded, identity, arguments)| {
                self.bound_holds(bounded, *identity, arguments, owner, 0)
            })
            .then_some((self_type, parameters))
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
                Ok(ty) => surface.push(ty),
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
