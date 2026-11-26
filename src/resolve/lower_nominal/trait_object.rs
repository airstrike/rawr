//! A program trait's object (D1115.3): a value unsized to `dyn Trait`
//! keeps its concrete type's key, which rawr writes into the value's tag
//! where it is coerced, where its type is known. A trait dispatcher reads
//! the key back and runs the impl rustc's vtable for that type holds: the
//! instance of a generic impl's method at the type's arguments, with the
//! witnesses its generics take, or the impl for that Self type of an
//! owner with impls for several (`Foo<i32>`'s beside `Foo<i64>`'s).

use super::generics::{instantiate, mentions_parameter, supertraits, witness_generics};
use super::imported::named_type;
use super::lvalue::Lvalue;
use super::models::Models;
use super::naming::instance_text;
use super::op::{impl_self_ty, owner_id, owner_key};
use super::ty::Ty;
use super::{invariant, method_name, origin, ParsedExpr, Parser, PreludeOp};
use crate::func::FnCallArgs;
use crate::resolve::Error;
use crate::types::dynamic::Tag;
use crate::{method, Dynamic, ImmutableString, RawrResult, Span, INT};
use std::collections::BTreeMap;

/// What a trait object's key is written with until the body's types are
/// known: as wide as the widest key.
const PLACEHOLDER: &str = "__rawr_nominal_object_";

/// The tags a value may already carry for another reader, which no key is.
const RESERVED: [Tag; 4] = [
    0,
    crate::types::dynamic::UNSIGNED_TAG,
    crate::types::dynamic::TUPLE_TAG,
    crate::types::dynamic::SINGLE_TAG,
];

/// The key of a concrete type, as a trait object's value carries it: a
/// hash of rawr's name of the type (`Parser::downcast_key`). A dispatcher
/// compares it only among the types of one owner, which it has already
/// told by the value's runtime type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Key(Tag);

impl Key {
    fn of(name: &str) -> Self {
        // FNV-1a: the same name keys the same in every compile, so a value
        // made before a reload keeps its key.
        let mut hash: u32 = 0x811c_9dc5;
        for byte in name.bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "a key is the hash's low bits, as wide as a tag"
        )]
        let mut tag = hash as Tag;
        while RESERVED.contains(&tag) {
            tag = tag.wrapping_add(1);
        }
        Self(tag)
    }
}

/// One arm of a trait dispatcher by a trait object's key: the function
/// `function` of the trait `trait_name`, at the generic trait's arguments
/// `instance` (`instance_text`), for the value of the owner keyed `owner`
/// (`owner_key`) whose type is keyed `key`, is the method named `name`,
/// called with the witnesses `witnesses` after its parameters.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Arm {
    trait_name: ImmutableString,
    function: ImmutableString,
    instance: Option<String>,
    owner: String,
    key: Key,
    name: String,
    witnesses: Box<[String]>,
}

impl Arm {
    /// Whether this is an arm of the dispatcher of `trait_name::function`
    /// at the generic trait's arguments `instance`, for the owner keyed
    /// `owner`.
    pub(super) fn of(
        &self,
        trait_name: &str,
        function: &str,
        instance: Option<&str>,
        owner: &str,
    ) -> bool {
        self.trait_name == trait_name
            && self.function == function
            && self.instance.as_deref() == instance
            && self.owner == owner
    }

    /// The arm's line in a dispatcher whose parameters are `parameters`
    /// and whose value's key is in `__rawr_key`.
    pub(super) fn text(&self, parameters: &str) -> String {
        let arguments = std::iter::once(parameters)
            .filter(|parameters| !parameters.is_empty())
            .chain(self.witnesses.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "if __rawr_key == {} {{ return self.{}({arguments}); }}\n",
            self.key.0, self.name
        )
    }
}

/// A coercion to a trait object whose key waits for the body's types: of
/// a value of `ty` to `dyn trait_name<arguments>`.
pub(super) struct Pending {
    placeholder: String,
    ty: Ty,
    trait_name: ImmutableString,
    arguments: Box<[Ty]>,
    span: Span,
}

/// A trait function with a receiver a trait object reaches: of its trait,
/// or of a supertrait, at the trait's arguments.
struct Reached {
    trait_name: ImmutableString,
    function: ImmutableString,
    arguments: Box<[Ty]>,
}

/// `ObjectNew(value, key)` keys a value as a trait object; `ObjectKey(value)`
/// reads the key a dispatcher selects its arm by, `0` for a value no
/// coercion keyed. A place keyed where it is keeps its key: it is its
/// type's, which every coercion of the place writes alike.
pub(super) fn invoke(op: PreludeOp, args: &mut FnCallArgs) -> RawrResult {
    match op {
        PreludeOp::ObjectNew => {
            let key = args[1]
                .as_int()
                .map_err(|_| invariant("a trait object's key is an integer"))?;
            let mut value = std::mem::take(&mut *args[0]);
            #[allow(
                clippy::cast_possible_truncation,
                reason = "the key was written from a tag"
            )]
            value.set_tag(key as Tag);
            Ok(value)
        }
        PreludeOp::ObjectKey => Ok(Dynamic::from(INT::from(args[0].tag()))),
        _ => Err(invariant("not a trait object operation")),
    }
}

/// Whether a dispatcher by runtime type can't run `definitions`, the
/// impls of one trait's function for one owner, without the value's
/// concrete type: impls for two of its Self types, or one whose method is
/// lowered per instance (D1115) or takes its generics' witnesses.
fn needs_key(definitions: &[&method::Definition], models: &Models) -> bool {
    let mut selves = definitions
        .iter()
        .map(|definition| impl_self_ty(definition, &BTreeMap::new()).name(models));
    let first = selves.next();
    selves.any(|name| Some(name) != first)
        || definitions.iter().any(|definition| {
            models.templates.contains_key(definition.key())
                || witness_generics(definition.generics(), models)
                    .next()
                    .is_some()
        })
}

impl Parser<'_> {
    /// `value`, of the concrete type `concrete`, unsized to `dyn
    /// trait_name<arguments>`: keyed by its type where a dispatcher of the
    /// trait's, or of one of its supertraits', needs the key to run its
    /// impl.
    pub(super) fn key_object(
        &mut self,
        concrete: &Ty,
        (trait_name, arguments): (&ImmutableString, &[Ty]),
        mutable: bool,
        value: &mut ParsedExpr,
    ) {
        let concrete = self.unreferenced(concrete);
        let Some(owner) = owner_id(&concrete) else {
            return;
        };
        let keyed = self
            .object_traits(trait_name, arguments)
            .iter()
            .any(|reached| {
                let definitions = self.object_impls(owner, reached);
                needs_key(&definitions.iter().collect::<Vec<_>>(), &self.models)
            });
        if !keyed {
            return;
        }
        let placeholder = format!("{PLACEHOLDER}{:011}", self.pending_objects.len());
        let helper = self.push_op(super::Op::Prelude(PreludeOp::ObjectNew));
        // A `&mut` borrow of a place is the place itself (`places.rs`):
        // the place is keyed where it is, before the borrow is used.
        let place = match &value.lvalue {
            Some(Lvalue::Binding { binding, .. }) if mutable => {
                Some(origin::Fragment::plain(binding.as_str()))
            }
            Some(Lvalue::Place { place, .. }) if mutable => Some(place.clone()),
            Some(Lvalue::Field(field)) if mutable => Some(field.place()),
            _ => None,
        };
        if let Some(place) = place {
            let mut keyed = place.clone();
            keyed.push_text(" = ");
            keyed.push(origin::Fragment::call(
                &helper,
                [place, placeholder.as_str().into()],
            ));
            keyed.push_text(";\n");
            value.prelude.push(keyed);
        } else if !mutable {
            let source = value.rendered_source();
            value.prelude = origin::Fragment::default();
            value.source = origin::Fragment::call(&helper, [source, placeholder.as_str().into()]);
            value.source.cover(value.span);
        } else {
            return;
        }
        self.pending_objects.push(Pending {
            placeholder,
            ty: concrete,
            trait_name: trait_name.clone(),
            arguments: arguments.into(),
            span: value.span,
        });
    }

    /// Before an unsuffixed literal falls back to its default type, unify
    /// each generic trait object's type with the one impl of its trait's
    /// arguments for it: `Holder { v: 5 }` as `dyn Convert<i64>` holds an
    /// `i64`, as rustc's obligation of the coercion fixes it.
    pub(super) fn settle_objects(&mut self) {
        let pending = self
            .pending_objects
            .iter()
            .filter(|pending| !pending.arguments.is_empty())
            .map(|pending| {
                (
                    pending.ty.clone(),
                    pending.trait_name.clone(),
                    pending.arguments.clone(),
                )
            })
            .collect::<Vec<_>>();
        for (ty, trait_name, arguments) in pending {
            let ty = self.inference.resolve(&ty);
            let Some(owner) = owner_id(&ty) else {
                continue;
            };
            let reached = self.object_traits(&trait_name, &arguments);
            let Some(reached) = reached.first() else {
                continue;
            };
            let definitions = self.object_impls(owner, reached);
            if let Some(definition) = self.fitting_impl(definitions, &ty) {
                self.impl_at(&definition, &ty, &arguments);
            }
        }
    }

    /// The impl `definition`'s parameters at the Self type `ty` and the
    /// trait arguments `arguments`, which the types unify with its
    /// header's.
    fn impl_at(
        &mut self,
        definition: &method::Definition,
        ty: &Ty,
        arguments: &[Ty],
    ) -> BTreeMap<ImmutableString, Ty> {
        let substitution = definition
            .generics()
            .iter()
            .map(|generic| (generic.name.text.clone(), self.inference.fresh()))
            .collect::<BTreeMap<_, _>>();
        self.unify_type(&impl_self_ty(definition, &substitution), ty);
        for (written, argument) in definition.trait_arguments().iter().zip(arguments) {
            let written = instantiate(&Ty::from_resolved(written), &substitution);
            self.unify_type(&written, argument);
        }
        substitution
    }

    /// Write each trait object's key over its placeholder, now that the
    /// body's types are known, with the arms of the dispatchers it reaches
    /// (`Uses::objects`). A type that holds a parameter of the body's has
    /// no key: its value runs what a dispatcher runs by its runtime type.
    pub(super) fn finish_objects(&mut self) -> Result<(), Error> {
        for pending in std::mem::take(&mut self.pending_objects) {
            let ty = self.inference.resolve(&pending.ty);
            let key = if super::instance::concrete(&ty) {
                let key = Key::of(&self.downcast_key(&ty));
                for reached in self.object_traits(&pending.trait_name, &pending.arguments) {
                    self.object_arm(&ty, reached, key, pending.span)?;
                }
                key.0
            } else {
                0
            };
            let text = format!("{key:<width$}", width = pending.placeholder.len());
            self.output.replace_same_length(&pending.placeholder, &text);
            for closure in &mut self.lifted {
                closure
                    .body
                    .replace_same_length(&pending.placeholder, &text);
            }
        }
        Ok(())
    }

    /// The functions with a receiver a `dyn trait_name<arguments>` reaches:
    /// its trait's, and each supertrait's without parameters.
    fn object_traits(&self, trait_name: &ImmutableString, arguments: &[Ty]) -> Vec<Reached> {
        let mut reached = Vec::new();
        let traits = std::iter::once((trait_name.clone(), arguments)).chain(
            supertraits(&self.models, trait_name)
                .into_iter()
                .map(|name| (name, &[][..])),
        );
        for (name, arguments) in traits {
            let Some((_, declared)) = self.models.traits.get(&name) else {
                continue;
            };
            if declared.generics.len() != arguments.len() {
                continue;
            }
            let functions = declared
                .functions
                .iter()
                .filter(|function| function.receiver.is_some());
            for function in functions {
                reached.push(Reached {
                    trait_name: name.clone(),
                    function: function.name.text.clone(),
                    arguments: arguments.into(),
                });
            }
        }
        reached
    }

    /// `owner`'s impls of the function `reached`, for its trait's
    /// arguments.
    fn object_impls(
        &mut self,
        owner: crate::type_def::Id,
        reached: &Reached,
    ) -> Vec<method::Definition> {
        let definitions = self
            .models
            .trait_methods
            .get(&(owner, reached.function.clone()))
            .into_iter()
            .flatten()
            .filter(|definition| definition.trait_name() == Some(&reached.trait_name))
            .cloned()
            .collect::<Vec<_>>();
        definitions
            .into_iter()
            .filter(|definition| {
                let written = definition.trait_arguments();
                written.len() == reached.arguments.len()
                    && written
                        .iter()
                        .zip(&reached.arguments)
                        .all(|(written, argument)| {
                            mentions_parameter(written)
                                || self.unifies(&Ty::from_resolved(written), argument)
                        })
            })
            .collect()
    }

    /// Record the arm of the dispatcher of `reached` a value of `ty`,
    /// keyed `key`, reaches: the impl rustc's vtable for `ty` holds, at
    /// `ty`'s arguments.
    fn object_arm(&mut self, ty: &Ty, reached: Reached, key: Key, span: Span) -> Result<(), Error> {
        let owner = owner_id(ty).expect("a keyed object's type has an owner");
        let definitions = self.object_impls(owner, &reached);
        if !needs_key(&definitions.iter().collect::<Vec<_>>(), &self.models) {
            return Ok(());
        }
        let Some(definition) = self.fitting_impl(definitions, ty) else {
            return Ok(());
        };
        let instance = if reached.arguments.is_empty() {
            None
        } else {
            let Some(written) = reached
                .arguments
                .iter()
                .map(|argument| named_type(&self.inference.resolve(argument), &self.models, span))
                .collect::<Option<Vec<_>>>()
            else {
                return Ok(());
            };
            Some(instance_text(&written))
        };
        let substitution = self.impl_at(&definition, ty, &reached.arguments);
        let witnessed = witness_generics(definition.generics(), &self.models)
            .map(|generic| {
                let ty = self.inference.resolve(&substitution[&generic.name.text]);
                (generic.name.text.clone(), ty)
            })
            .collect::<Vec<_>>();
        // A type the header leaves open is the erased body's to run.
        if !witnessed
            .iter()
            .all(|(_, ty)| super::instance::concrete(ty))
        {
            return Ok(());
        }
        let witnesses = witnessed
            .iter()
            .map(|(name, ty)| self.witness(ty, name, span))
            .collect::<Result<Box<[_]>, _>>()?;
        let name = self
            .instance_name(definition.key(), &substitution, span)
            .unwrap_or_else(|| method_name(&definition));
        self.instance_uses.objects.push(Arm {
            trait_name: reached.trait_name,
            function: reached.function,
            instance,
            owner: owner_key(owner),
            key,
            name,
            witnesses,
        });
        Ok(())
    }
}

impl Parser<'_> {
    /// The member of the program's impl each method of the native trait
    /// `trait_name` calls, by the method's name, for a value of the
    /// program's own type `concrete` unsized to `dyn trait_name<arguments>`
    /// (#2698): the instance of a generic impl's method at `concrete`'s
    /// arguments, or the impl's own method, by its path in the running
    /// program, which the runner's one stand-in for the trait calls. `None`
    /// when `concrete` is no type of the program's, or the program writes
    /// no impl of the trait for it.
    #[cfg(feature = "rust-import")]
    pub(super) fn native_object_members(
        &mut self,
        concrete: &Ty,
        (trait_name, arguments): (&ImmutableString, &[Ty]),
        span: Span,
    ) -> Result<Option<Vec<(ImmutableString, ImmutableString)>>, Error> {
        let concrete = self.unreferenced(concrete);
        let Ty::Script(owner, _) = self.inference.resolve(&concrete) else {
            return Ok(None);
        };
        let names = |path: &str| {
            path == trait_name.as_str()
                || self
                    .trait_known_as(path)
                    .is_some_and(|known| known == *trait_name)
        };
        // The trait's path, as the impls of the program's that resolve to
        // it write it in their modules: `Shown` in the crate's own source.
        let implemented = self.models.script_impls.iter().any(|candidate| {
            matches!(&candidate.owner, crate::resolve::type_ref::Type::Path(crate::resolve::type_ref::Path {
                target: crate::resolve::type_ref::Target::Script { definition, .. },
                ..
            }) if *definition == owner)
                && names(candidate.path.as_str())
        });
        let last = trait_name.rsplit("::").next().unwrap_or_default();
        let by_name = self.impl_functions(owner, |written| {
            names(written) || (implemented && written.rsplit("::").next() == Some(last))
        });
        // An impl that writes no function, as `impl Meta for Mood {}`, has
        // the trait's provided ones, which the stand-in runs itself.
        let written = !by_name.is_empty();
        let Some(mut members) = self
            .impl_members(by_name, &concrete, (trait_name, arguments), span)?
            .or_else(|| (implemented && !written).then(Vec::new))
        else {
            return Ok(None);
        };
        // Each supertrait's, as the impl of it writes its last segment, by
        // the key the stand-in's impl of it calls (#2698).
        let supertraits = crate::host::path::Path::parse(trait_name)
            .ok()
            .and_then(|path| self.models.host.trait_header(&path))
            .and_then(|header| header.object.clone())
            .unwrap_or_default();
        for (position, (supertrait, _)) in supertraits.iter().enumerate() {
            let last = supertrait.rsplit("::").next().unwrap_or_default();
            let by_name =
                self.impl_functions(owner, |written| written.rsplit("::").next() == Some(last));
            let found = self
                .impl_members(by_name, &concrete, (supertrait, &[]), span)?
                .unwrap_or_default();
            members.extend(found.into_iter().map(|(function, path)| {
                (
                    rawr_import::classify::representability::object::Supertrait::key(
                        position, &function,
                    )
                    .into(),
                    path,
                )
            }));
        }
        Ok(Some(members))
    }

    /// Whether the runner has its one stand-in for the native trait
    /// `trait_name` (`object::Pointer::StandIn`), which holds a program's
    /// value of any type: none where a method names a type the runner
    /// can't give or take, as `&mut dyn Visitor`.
    #[cfg(feature = "rust-import")]
    pub(super) fn native_stand_in(&self, trait_name: &str) -> bool {
        crate::host::path::Path::parse(trait_name)
            .ok()
            .and_then(|path| self.models.host.trait_header(&path))
            .is_some_and(|header| header.object.is_some())
    }

    /// The functions of the program's impls for the type `owner` of each
    /// trait `names` holds of as the impl writes it, by name.
    #[cfg(feature = "rust-import")]
    fn impl_functions(
        &self,
        owner: crate::type_def::Id,
        names: impl Fn(&str) -> bool,
    ) -> BTreeMap<ImmutableString, Vec<method::Definition>> {
        let mut by_name = BTreeMap::<ImmutableString, Vec<method::Definition>>::new();
        for ((of, function), definitions) in &self.models.trait_methods {
            if *of != owner {
                continue;
            }
            for definition in definitions {
                if definition
                    .trait_name()
                    .is_some_and(|written| names(written))
                {
                    by_name
                        .entry(function.clone())
                        .or_default()
                        .push(definition.clone());
                }
            }
        }
        by_name
    }

    /// The member each of `by_name`, the functions of the impls of the
    /// trait `trait_name` at `arguments` for `concrete`, is in the running
    /// program, by the function's name. `None` when it has none, or one
    /// isn't a member of the program's.
    #[cfg(feature = "rust-import")]
    fn impl_members(
        &mut self,
        mut by_name: BTreeMap<ImmutableString, Vec<method::Definition>>,
        concrete: &Ty,
        (trait_name, arguments): (&ImmutableString, &[Ty]),
        span: Span,
    ) -> Result<Option<Vec<(ImmutableString, ImmutableString)>>, Error> {
        // An instance of a generic impl's method (D629) is its template's
        // to bind at `concrete`'s arguments, as a call binds it.
        for definitions in by_name.values_mut() {
            definitions.retain(|definition| {
                !self
                    .models
                    .instances
                    .templates
                    .values()
                    .any(|instances| instances.iter().any(|(_, key)| key == definition.key()))
            });
        }
        by_name.retain(|_, definitions| !definitions.is_empty());
        if by_name.is_empty() {
            return Ok(None);
        }
        let mut members = Vec::with_capacity(by_name.len());
        for (function, definitions) in by_name {
            let Some(definition) = self.fitting_impl(definitions, concrete) else {
                return Ok(None);
            };
            let substitution = self.impl_at(&definition, concrete, arguments);
            // An instance lowered at the types (D1115) takes no witnesses;
            // the method itself, or a crate-bounded one's instance (D629),
            // takes those its generics still name.
            let (key, bound) = match self.instance_key(definition.key(), &substitution, span) {
                Some(key) => (key, None),
                None => {
                    let bound = self.bind_method_instance(definition, &substitution, span)?;
                    (bound.key().clone(), Some(bound))
                }
            };
            if bound.is_some_and(|bound| {
                witness_generics(bound.generics(), &self.models)
                    .next()
                    .is_some()
            }) {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: crate::resolve::Unsupported::said(format!(
                        "coercing `{}` to the native trait object `dyn {trait_name}`, whose impl's \
                         `{function}` takes its generics' types, is not supported yet",
                        concrete.name(&self.models)
                    )),
                    source: span,
                });
            }
            let Some(crate::type_def::Identity::Script(script)) = key.owner() else {
                return Ok(None);
            };
            let path = script
                .modules()
                .iter()
                .map(ImmutableString::as_str)
                .chain([script.name()])
                .collect::<Vec<_>>()
                .join("::");
            let path = format!(
                "{}::{}",
                self.models.runtime.crossing_path(&path),
                key.name()
            );
            members.push((function, path.into()));
        }
        Ok(Some(members))
    }
}
