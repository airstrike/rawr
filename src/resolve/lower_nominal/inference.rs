//! Type variables and their solutions, with checkpoints to roll a
//! tentative unification back.

#[cfg(feature = "rust-import")]
use super::ImplProjection;
use super::{builtin_operator, instantiate, is_iterator_trait, Float, FnCandidate, Int, Ty};
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
    /// The crate impls' bounds `<ty as Trait>::Name == value` whose `ty`
    /// wasn't known where their impl was chosen: each fixes its `value`
    /// once a later use fixes `ty`. A rollback drops those it made.
    #[cfg(feature = "rust-import")]
    pub(super) projections: Vec<ImplProjection>,
}

/// The associated types of `impl Trait for Owner` blocks.
#[derive(Debug, Default)]
pub(super) struct AssociatedTypes {
    /// `type Name = T;` by owner, trait and name, with the impl parameter
    /// each of the owner's type arguments names (`impl<T> Shape for
    /// Wrapper<T>`).
    pub(super) impls: BTreeMap<(type_def::Id, ImmutableString, ImmutableString), AssociatedType>,
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
/// type arguments names.
pub(super) type AssociatedType = (type_ref::Type, Box<[Option<ImmutableString>]>);

#[derive(Clone, Copy)]
pub(super) struct Checkpoint {
    pub(super) next: usize,
    pub(super) journal: usize,
    pub(super) depth: usize,
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
        }
    }

    pub(super) fn apply(&mut self, patch: Patch) {
        self.next = patch.next;
        for (id, ty) in patch.bindings {
            self.set(id, ty);
        }
    }

    pub(super) fn fresh(&mut self) -> Ty {
        Ty::Infer(self.fresh_id())
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
                kind: *kind,
            },
            Ty::Tuple(members) => {
                Ty::Tuple(members.iter().map(|member| self.resolve(member)).collect())
            }
            Ty::Option(element) => Ty::Option(Box::new(self.resolve(element))),
            Ty::Boxed(target) => Ty::Boxed(Box::new(self.resolve(target))),
            Ty::Counted(counter, target) => Ty::Counted(*counter, Box::new(self.resolve(target))),
            Ty::Weak(counter, target) => Ty::Weak(*counter, Box::new(self.resolve(target))),
            Ty::Iter(item, std) => Ty::Iter(Box::new(self.resolve(item)), *std),
            Ty::Reverse(inner) => Ty::Reverse(Box::new(self.resolve(inner))),
            Ty::Cow(inner) => Ty::Cow(Box::new(self.resolve(inner))),
            Ty::Range(element) => Ty::Range(Box::new(self.resolve(element))),
            Ty::InclusiveOf(element) => Ty::InclusiveOf(Box::new(self.resolve(element))),
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
            // A generic script type's arguments, as component's
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
            _ => ty.clone(),
        }
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
                if let Some((ty, parameters)) =
                    self.associated
                        .impls
                        .get(&(*id, trait_name.clone(), name.clone()))
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
            // `dyn Shape<Output = i64>`: its binding.
            Ty::Dyn(object, bindings) if object == trait_name => {
                if let Some((_, ty)) = bindings.iter().find(|(bound, _)| bound == name) {
                    return self.resolve(ty);
                }
            }
            Ty::Param(parameter) => {
                if let Some(ty) =
                    self.equalities
                        .get(&(parameter.clone(), trait_name.clone(), name.clone()))
                {
                    return self.resolve(ty);
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
        if matches!(base, Ty::Param(_) | Ty::Projection(..))
            && let Some(item) =
                self.associated_equality(&base, trait_name, name, is_iterator_trait, "Item")
        {
            return Ty::iter(item);
        }
        // `T::Fore` of `type Fore: Iterator;` is an iterator of the opaque
        // `<T::Fore as Iterator>::Item`, as rustc types its items (T379).
        if matches!(base, Ty::Param(_) | Ty::Projection(..))
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
