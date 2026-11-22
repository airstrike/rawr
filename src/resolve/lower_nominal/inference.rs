//! Type variables and their solutions, with checkpoints to roll a
//! tentative unification back.

#[cfg(feature = "rust-import")]
use super::ImplProjection;
use super::{
    builtin_operator, instantiate, is_iterator_trait, owner_id, sequence, Float, FnCandidate, Int,
    Parameter, ParseError, Ty,
};
use crate::resolve::item;
use crate::resolve::type_ref;
use crate::{type_def, ImmutableString, Shared};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub(super) struct Inference {
    pub(super) next: usize,
    pub(super) bindings: BTreeMap<usize, Ty>,
    pub(super) journal: Vec<Undo>,
    pub(super) depth: usize,
    /// The impls' associated types, which `T::Output` becomes once `T` is
    /// known.
    pub(super) associated: Shared<AssociatedTypes>,
    /// `T::Output = i64` of the bounds in scope (`T: Shape<Output = i64>`),
    /// by parameter, trait and name.
    pub(super) equalities: BTreeMap<(ImmutableString, ImmutableString, ImmutableString), Ty>,
    /// Every function's opaque output (D1109), whose written bindings
    /// (`impl Named<Out = i64>`) its projections are.
    pub(super) opaque_outputs: Shared<BTreeMap<type_ref::Opaque, item::OpaqueOutput>>,
    /// The variables a diverging expression was coerced into, as rustc's
    /// diverging type variables: one nothing else settles falls back once
    /// the body is read (`fall_back_diverging`). A rollback drops those it
    /// marked.
    pub(super) diverging: Vec<usize>,
    /// The crate impls' bounds `<ty as Trait>::Name == value` whose `ty`
    /// wasn't known where their impl was chosen: each fixes its `value`
    /// once a later use fixes `ty`. A rollback drops those it made.
    #[cfg(feature = "rust-import")]
    pub(super) projections: Vec<ImplProjection>,
}

/// The associated types of `impl Trait for Owner` blocks.
#[derive(Debug, Default)]
pub(super) struct AssociatedTypes {
    /// `type Name = T;` by owner, trait and name, one per impl for the
    /// owner's instances (`impl Shape for Foo<i32>` beside `impl Shape for
    /// Foo<String>`), each with the impl parameter each of the owner's type
    /// arguments names (`impl<T> Shape for Wrapper<T>`).
    pub(super) impls:
        BTreeMap<(type_def::Id, ImmutableString, ImmutableString), Box<[AssociatedType]>>,
    /// The equalities of each trait's associated types' bounds, by trait
    /// and name, in the trait's `Self`: `(Iterator, Item, Self::For)` of
    /// `type Fore: Iterator<Item = Self::For>;`.
    pub(super) equalities: BTreeMap<(ImmutableString, ImmutableString), Box<[AssociatedEquality]>>,
    /// The traits whose impl for `&T` (or, `true`, `&mut T`) forwards to
    /// `T`'s (`resolve::forward`): `<&T as For>::Fore` is `T`'s.
    pub(super) reference_forwards: BTreeSet<(ImmutableString, bool)>,
    /// Each trait's associated types bounded by `Iterator` with no `Item =`,
    /// by trait and name: `type Fore: Iterator;`, whose item is the opaque
    /// `<T::Fore as Iterator>::Item`.
    pub(super) iterators: BTreeSet<(ImmutableString, ImmutableString)>,
}

/// An equality of an associated type's bound, as `(trait, name, type)`.
pub(super) type AssociatedEquality = (ImmutableString, ImmutableString, type_ref::Type);

/// An associated type's `T`, with the impl parameter each of the owner's
/// type arguments names, and the owner's type arguments as its impl writes
/// them: `i32` of `impl Shape for Foo<i32>`.
pub(super) type AssociatedType = (
    type_ref::Type,
    Box<[Option<ImmutableString>]>,
    Box<[type_ref::Type]>,
);

#[derive(Clone, Copy)]
pub(super) struct Checkpoint {
    pub(super) next: usize,
    pub(super) journal: usize,
    pub(super) depth: usize,
    pub(super) diverging: usize,
    #[cfg(feature = "rust-import")]
    pub(super) projections: usize,
}

#[derive(Debug)]
pub(super) struct Undo {
    pub(super) id: usize,
    pub(super) previous: Option<Ty>,
}

pub(super) struct Patch {
    pub(super) next: usize,
    pub(super) bindings: Box<[(usize, Ty)]>,
    pub(super) diverging: Box<[usize]>,
}

#[cfg(test)]
std::thread_local! {
    static INFERENCE_METRICS: std::cell::Cell<(usize, usize)> = const {
        std::cell::Cell::new((0, 0))
    };
}

#[cfg(test)]
pub(super) fn reset_inference_metrics() {
    INFERENCE_METRICS.with(|metrics| metrics.set((0, 0)));
}

#[cfg(test)]
pub(super) fn inference_metrics() -> (usize, usize) {
    INFERENCE_METRICS.with(std::cell::Cell::get)
}

impl Inference {
    pub(super) fn checkpoint(&mut self) -> Checkpoint {
        let checkpoint = Checkpoint {
            next: self.next,
            journal: self.journal.len(),
            depth: self.depth,
            diverging: self.diverging.len(),
            #[cfg(feature = "rust-import")]
            projections: self.projections.len(),
        };
        self.depth += 1;
        checkpoint
    }

    pub(super) fn rollback(&mut self, checkpoint: Checkpoint) {
        debug_assert_eq!(self.depth, checkpoint.depth + 1);
        while self.journal.len() > checkpoint.journal {
            let Some(undo) = self.journal.pop() else {
                unreachable!("inference journal length was checked before rollback");
            };
            match undo.previous {
                Some(previous) => {
                    self.bindings.insert(undo.id, previous);
                }
                None => {
                    self.bindings.remove(&undo.id);
                }
            }
        }
        self.next = checkpoint.next;
        self.depth = checkpoint.depth;
        self.diverging.truncate(checkpoint.diverging);
        #[cfg(feature = "rust-import")]
        self.projections.truncate(checkpoint.projections);
    }

    pub(super) fn commit(&mut self, checkpoint: Checkpoint) {
        debug_assert_eq!(self.depth, checkpoint.depth + 1);
        self.depth = checkpoint.depth;
        if self.depth == 0 {
            self.journal.truncate(checkpoint.journal);
        }
    }

    pub(super) fn patch(&self, checkpoint: Checkpoint) -> Patch {
        debug_assert_eq!(self.depth, checkpoint.depth + 1);
        let changed = self.journal[checkpoint.journal..]
            .iter()
            .map(|undo| undo.id)
            .collect::<BTreeSet<_>>();
        let bindings = changed
            .into_iter()
            .filter_map(|id| self.bindings.get(&id).cloned().map(|ty| (id, ty)))
            .collect();
        Patch {
            next: self.next,
            bindings,
            diverging: self.diverging[checkpoint.diverging..].into(),
        }
    }

    pub(super) fn apply(&mut self, patch: Patch) {
        self.next = patch.next;
        for (id, ty) in patch.bindings {
            self.set(id, ty);
        }
        self.diverging.extend(patch.diverging);
    }

    pub(super) fn fresh(&mut self) -> Ty {
        Ty::Infer(self.fresh_id())
    }

    /// A fresh variable for a diverging expression's value, which falls
    /// back if nothing else settles it.
    pub(super) fn fresh_diverging(&mut self) -> Ty {
        let id = self.fresh_id();
        self.diverging.push(id);
        Ty::Infer(id)
    }

    /// Each diverging variable still open is the edition's fallback type,
    /// `!` from edition 2024 and `()` before, as rustc's fallback makes it
    /// at the end of a body's inference.
    pub(super) fn fall_back_diverging(&mut self) {
        let fallback = if crate::resolve::std_path::EDITION >= "2024" {
            Ty::Never
        } else {
            Ty::Unit
        };
        for id in std::mem::take(&mut self.diverging) {
            if let Ty::Infer(open) = self.resolve(&Ty::Infer(id)) {
                self.bind(open, fallback.clone());
            }
        }
    }

    pub(super) fn fresh_id(&mut self) -> usize {
        let id = self.next;
        self.next += 1;
        id
    }

    #[cfg(feature = "rust-import")]
    pub(super) fn resolved_ref<'a>(&'a self, ty: &'a Ty) -> &'a Ty {
        match ty {
            Ty::Infer(id) | Ty::Int(Int::Var(id)) | Ty::Float(Float::Var(id)) => self
                .bindings
                .get(id)
                .map_or(ty, |bound| self.resolved_ref(bound)),
            _ => ty,
        }
    }

    /// A choice's candidates with their own types resolved: a crate
    /// function's types are what the choice's pointer met.
    fn resolve_candidates(&self, keys: &Shared<Vec<FnCandidate>>) -> Shared<Vec<FnCandidate>> {
        #[cfg(feature = "rust-import")]
        if keys
            .iter()
            .any(|candidate| matches!(candidate, FnCandidate::Imported { .. }))
        {
            let resolved = keys.iter().map(|candidate| match candidate {
                FnCandidate::Imported {
                    input,
                    output,
                    proof,
                } => FnCandidate::Imported {
                    input: Box::new(self.resolve(input)),
                    output: Box::new(self.resolve(output)),
                    proof: proof.clone(),
                },
                FnCandidate::Script(_) => candidate.clone(),
            });
            return Shared::new(resolved.collect());
        }
        keys.clone()
    }

    pub(super) fn resolve(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Infer(id) | Ty::Int(Int::Var(id)) | Ty::Float(Float::Var(id)) => self
                .bindings
                .get(id)
                .map_or_else(|| ty.clone(), |bound| self.resolve(bound)),
            Ty::Host { proof, arguments } => Ty::Host {
                proof: crate::Shared::clone(proof),
                arguments: arguments
                    .iter()
                    .map(|argument| self.resolve(argument))
                    .collect(),
            },
            Ty::Callable {
                inputs,
                output,
                item,
            } => Ty::Callable {
                inputs: inputs.iter().map(|input| self.resolve(input)).collect(),
                output: Box::new(self.resolve(output)),
                item: item.clone(),
            },
            #[cfg(feature = "rust-import")]
            Ty::ImportedFunctionItem {
                input,
                output,
                item,
            } => Ty::ImportedFunctionItem {
                input: Box::new(self.resolve(input)),
                output: Box::new(self.resolve(output)),
                item: item.clone(),
            },
            #[cfg(feature = "rust-import")]
            Ty::ProfileMapper {
                input,
                output,
                item,
                seal,
            } => Ty::ProfileMapper {
                input: Box::new(self.resolve(input)),
                output: Box::new(self.resolve(output)),
                item: item.clone(),
                seal: *seal,
            },
            #[cfg(feature = "rust-import")]
            Ty::ProfileMessage {
                payload,
                script,
                seal,
            } => Ty::ProfileMessage {
                payload: Box::new(self.resolve(payload)),
                script: Box::new(self.resolve(script)),
                seal: *seal,
            },
            Ty::Reference { mutable, target } => Ty::Reference {
                mutable: *mutable,
                target: Box::new(self.resolve(target)),
            },
            Ty::Fn { inputs, output } => Ty::Fn {
                inputs: inputs.iter().map(|input| self.resolve(input)).collect(),
                output: Box::new(self.resolve(output)),
            },
            Ty::Array { element, kind } => Ty::Array {
                element: Box::new(self.resolve(element)),
                kind: match kind {
                    sequence::Kind::Array(length) => {
                        sequence::Kind::Array(length.mapped(|length| self.resolve(length)))
                    }
                    kind => kind.clone(),
                },
            },
            Ty::Tuple(members) => {
                Ty::Tuple(members.iter().map(|member| self.resolve(member)).collect())
            }
            Ty::Option(element) => Ty::Option(Box::new(self.resolve(element))),
            Ty::Boxed(target) => Ty::Boxed(Box::new(self.resolve(target))),
            Ty::Counted(counter, target) => Ty::Counted(*counter, Box::new(self.resolve(target))),
            Ty::Weak(counter, target) => Ty::Weak(*counter, Box::new(self.resolve(target))),
            Ty::Iter(item, std, traits) => Ty::Iter(Box::new(self.resolve(item)), *std, *traits),
            Ty::Reverse(inner) => Ty::Reverse(Box::new(self.resolve(inner))),
            Ty::Cow(inner) => Ty::Cow(Box::new(self.resolve(inner))),
            Ty::Range(element) => Ty::Range(Box::new(self.resolve(element))),
            Ty::InclusiveOf(element) => Ty::InclusiveOf(Box::new(self.resolve(element))),
            Ty::OpenRange(open, element) => Ty::OpenRange(*open, Box::new(self.resolve(element))),
            Ty::InclusiveRange(kind) => match self.resolve(&Ty::Int(*kind)) {
                Ty::Int(kind) => Ty::InclusiveRange(kind),
                _ => ty.clone(),
            },
            Ty::Cell(kind, inner) => Ty::Cell(*kind, Box::new(self.resolve(inner))),
            Ty::Thread(kind, inner) => Ty::Thread(*kind, Box::new(self.resolve(inner))),
            Ty::Keyed(kind, arguments) => Ty::Keyed(
                *kind,
                arguments
                    .iter()
                    .map(|argument| self.resolve(argument))
                    .collect(),
            ),
            Ty::Result(ok, err) => {
                Ty::Result(Box::new(self.resolve(ok)), Box::new(self.resolve(err)))
            }
            Ty::FnItem {
                key,
                inputs,
                output,
            } => Ty::FnItem {
                key: key.clone(),
                inputs: inputs.iter().map(|input| self.resolve(input)).collect(),
                output: Box::new(self.resolve(output)),
            },
            Ty::FnChoice {
                keys,
                inputs,
                output,
            } => Ty::FnChoice {
                keys: self.resolve_candidates(keys),
                inputs: inputs.iter().map(|input| self.resolve(input)).collect(),
                output: Box::new(self.resolve(output)),
            },
            Ty::Projection(base, trait_name, name) => {
                self.project(self.resolve(base), trait_name, name)
            }
            // A generic script type's arguments, as a custom component's
            // `NumericInput<Message>` holds its message's type.
            Ty::Script(definition, arguments) if !arguments.is_empty() => Ty::Script(
                *definition,
                arguments
                    .iter()
                    .map(|argument| self.resolve(argument))
                    .collect(),
            ),
            Ty::Dyn(trait_name, bindings) => Ty::Dyn(
                trait_name.clone(),
                bindings
                    .iter()
                    .map(|(name, ty)| (name.clone(), self.resolve(ty)))
                    .collect(),
            ),
            Ty::Param(name, Parameter::Opaque(captured)) if !captured.arguments.is_empty() => {
                Ty::Param(
                    name.clone(),
                    Parameter::opaque(
                        captured.opaque,
                        captured
                            .arguments
                            .iter()
                            .map(|argument| self.resolve(argument))
                            .collect(),
                    ),
                )
            }
            _ => ty.clone(),
        }
    }

    /// Of `impls`, each an impl's associated type, the one of the impl for
    /// the owner's instance at `arguments`: the only one, or the one whose
    /// written arguments are `arguments` where it writes a type and not an
    /// impl parameter. Of impls for one Self type, which differ by the
    /// trait's arguments (`TryFrom<[i16; 3]>` beside `TryFrom<&[i16]>`), the
    /// last. `None` where `arguments` leave two Self types.
    fn instance_impl<'a>(
        &self,
        impls: &'a [AssociatedType],
        arguments: &[Ty],
    ) -> Option<&'a AssociatedType> {
        if let [only] = impls {
            return Some(only);
        }
        let arguments = arguments
            .iter()
            .map(|argument| self.resolve(argument))
            .collect::<Vec<_>>();
        let fitting = impls
            .iter()
            .filter(|(_, _, written)| super::generics::instance_fits(written, &arguments))
            .collect::<Vec<_>>();
        let last = fitting.last()?;
        let named = |written: &[type_ref::Type]| {
            written
                .iter()
                .map(crate::resolve::full_type_name)
                .collect::<Vec<_>>()
        };
        fitting
            .iter()
            .all(|(_, _, written)| named(written) == named(&last.2))
            .then_some(*last)
    }

    /// `base::name`: the impl's associated type once `base` is a known
    /// type, the bound's for a parameter with `T: Trait<name = U>`, else
    /// the opaque projection, as Rust normalises it.
    pub(super) fn project(
        &self,
        base: Ty,
        trait_name: &ImmutableString,
        name: &ImmutableString,
    ) -> Ty {
        match &base {
            Ty::Script(id, arguments) => {
                // `Self::Item` names no trait: the one trait of the type's
                // that has the type, as rustc finds it.
                let mut named = self
                    .associated
                    .impls
                    .iter()
                    .filter(|((owner, bound, item), _)| {
                        owner == id
                            && item == name
                            && (trait_name.is_empty() || bound == trait_name)
                    })
                    .map(|(_, impls)| impls);
                let impls = match (named.next(), named.next()) {
                    (Some(impls), None) => Some(impls),
                    _ => None,
                };
                if let Some((ty, parameters, _)) =
                    impls.and_then(|impls| self.instance_impl(impls, arguments))
                {
                    let substitution = parameters
                        .iter()
                        .zip(arguments.iter())
                        .filter_map(|(parameter, argument)| {
                            Some((parameter.clone()?, self.resolve(argument)))
                        })
                        .collect();
                    return self.resolve(&instantiate(&Ty::from_resolved(ty), &substitution));
                }
            }
            // Rust's own operator impls: `<i64 as Add>::Output` is `i64`.
            Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::String
                if name.as_str() == "Output" && builtin_operator(&base, trait_name) =>
            {
                return base;
            }
            // std's `FromStr` impls, unless the program gives the type its
            // own trait of that name: `<u8 as FromStr>::Err` is
            // `ParseIntError`.
            Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::String
                if trait_name.as_str() == "FromStr" && name.as_str() == "Err" =>
            {
                let own = owner_id(&base).is_some_and(|id| {
                    self.associated
                        .impls
                        .contains_key(&(id, trait_name.clone(), name.clone()))
                });
                if !own && let Some(error) = ParseError::of(&base) {
                    return Ty::ParseError(error);
                }
            }
            // An iterator's `Item`, by std's `Iterator` and `IntoIterator`
            // for every iterator.
            // An associated iterator's opaque item (`<T::Fore as
            // Iterator>::Item`) is itself: resolving it reads the iterator
            // again.
            Ty::Iter(item, _, _) if name.as_str() == "Item" && is_iterator_trait(trait_name) => {
                return match &**item {
                    Ty::Projection(_, trait_name, name)
                        if name.as_str() == "Item" && is_iterator_trait(trait_name) =>
                    {
                        (**item).clone()
                    }
                    item => self.resolve(item),
                };
            }
            // std's ranges of integers and chars are iterators of them.
            range
                if name.as_str() == "Item"
                    && is_iterator_trait(trait_name)
                    && let Some((
                        super::range::Kind::Range
                        | super::range::Kind::Inclusive
                        | super::range::Kind::From,
                        element,
                    )) = super::range::parts(range)
                    && let element @ (Ty::Int(_) | Ty::Char) = self.resolve(&element) =>
            {
                return element;
            }
            // `dyn Shape<Output = i64>`: its binding.
            Ty::Dyn(object, bindings) if object == trait_name => {
                if let Some((_, ty)) = bindings.iter().find(|(bound, _)| bound == name) {
                    return self.resolve(ty);
                }
            }
            Ty::Param(parameter, Parameter::Declared) => {
                if let Some(ty) =
                    self.equalities
                        .get(&(parameter.clone(), trait_name.clone(), name.clone()))
                {
                    return self.resolve(ty);
                }
            }
            // `impl Named<Out = i64>`: its written binding (D1109).
            // At its call's type arguments (`impl Iterator<Item = T>`).
            Ty::Param(_, Parameter::Opaque(captured)) => {
                if let Some(output) = self.opaque_outputs.get(&captured.opaque)
                    && let Some((_, _, ty)) =
                        output.bounds.projections.iter().find(|(bound, item, _)| {
                            bound.text == *trait_name && item.text == *name
                        })
                {
                    let substitution =
                        super::generics::opaque_substitution(output, &captured.arguments);
                    return self.resolve(&super::generics::instantiate(
                        &Ty::from_resolved(ty),
                        &substitution,
                    ));
                }
            }
            // `<&T as For>::Fore` of an impl for `&T` forwarding to `T`'s.
            Ty::Reference { mutable, target }
                if self
                    .associated
                    .reference_forwards
                    .contains(&(trait_name.clone(), *mutable)) =>
            {
                return self.project(self.resolve(target), trait_name, name);
            }
            // `<T::Fore as Shape>::Output` of `type Fore: Shape<Output =
            // i64>;`: the equality.
            Ty::Projection(owner, owner_trait, owner_name) => {
                if let Some(ty) = self.associated_equality(
                    owner,
                    owner_trait,
                    owner_name,
                    |bound| {
                        bound == trait_name
                            || is_iterator_trait(bound) && is_iterator_trait(trait_name)
                    },
                    name,
                ) {
                    return ty;
                }
            }
            _ => {}
        }
        // `T::Fore` of `type Fore: Iterator<Item = T::For>;` is Rawr's
        // iterator of `T::For`, as a parameter `I: Iterator<Item = U>` is
        // (`erase_fn_bounds`).
        if matches!(base, Ty::Param(..) | Ty::Projection(..))
            && let Some(item) =
                self.associated_equality(&base, trait_name, name, is_iterator_trait, "Item")
        {
            return Ty::iter(item);
        }
        // `T::Fore` of `type Fore: Iterator;` is an iterator of the opaque
        // `<T::Fore as Iterator>::Item`, as rustc types its items (T379).
        if matches!(base, Ty::Param(..) | Ty::Projection(..))
            && self
                .associated
                .iterators
                .contains(&(trait_name.clone(), name.clone()))
        {
            let fore = Ty::Projection(Box::new(base), trait_name.clone(), name.clone());
            return Ty::iter(Ty::Projection(
                Box::new(fore),
                "Iterator".into(),
                "Item".into(),
            ));
        }
        Ty::Projection(Box::new(base), trait_name.clone(), name.clone())
    }

    /// The equality `item = U` that the trait `trait_name` declares on its
    /// associated type `name`'s bound by a trait `bound` accepts, with
    /// `Self` as `base`: `T::For` for `T`, `For`, `Fore` and `Item` of `type
    /// Fore: Iterator<Item = Self::For>;`.
    pub(super) fn associated_equality(
        &self,
        base: &Ty,
        trait_name: &str,
        name: &str,
        bound: impl Fn(&str) -> bool,
        item: &str,
    ) -> Option<Ty> {
        let equalities = self
            .associated
            .equalities
            .get(&(trait_name.into(), name.into()))?;
        let (_, _, ty) = equalities
            .iter()
            .find(|(written, equal, _)| bound(written) && equal == item)?;
        let substitution = BTreeMap::from([(ImmutableString::from("Self"), base.clone())]);
        Some(self.resolve(&instantiate(&Ty::from_resolved(ty), &substitution)))
    }

    pub(super) fn bind(&mut self, id: usize, ty: Ty) {
        let ty = self.resolve(&ty);
        if !matches!(ty, Ty::Infer(own) | Ty::Int(Int::Var(own)) | Ty::Float(Float::Var(own)) if own == id)
        {
            self.set(id, ty);
        }
    }

    pub(super) fn set(&mut self, id: usize, ty: Ty) {
        if self.bindings.get(&id) == Some(&ty) {
            return;
        }
        if self.depth > 0 {
            self.journal.push(Undo {
                id,
                previous: self.bindings.get(&id).cloned(),
            });
            #[cfg(test)]
            {
                INFERENCE_METRICS.with(|metrics| {
                    let (writes, peak) = metrics.get();
                    metrics.set((writes + 1, peak.max(self.journal.len())));
                });
            }
        }
        self.bindings.insert(id, ty);
    }
}
