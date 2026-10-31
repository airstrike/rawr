//! Closed structural proof rules over complete intrinsic facts.
//!
//! This module never invokes the compiler. Unsupported syntax, missing domains,
//! cycles and exhausted work remain unresolved; none establishes an exclusion.

use std::collections::BTreeMap;

use super::Catalog;
use crate::api::{identity, model};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Failure {
    Absent,
    Ambiguous,
    Unknown(&'static str),
}

type Result<T> = std::result::Result<T, Failure>;
type Key = (identity::Definition, u32);
type Projection = (
    identity::Definition,
    identity::Definition,
    Vec<model::Argument>,
);

#[derive(Clone, Debug, Default)]
pub(super) struct State {
    substitutions: BTreeMap<Key, model::Type>,
    pub(super) implementations: Vec<identity::Definition>,
    established: Vec<Established>,
    normalized: BTreeMap<Projection, model::Type>,
}

#[derive(Clone, Debug)]
struct Established {
    trait_id: identity::Definition,
    arguments: Vec<model::Argument>,
    implementation: identity::Definition,
    substitutions: BTreeMap<Key, model::Type>,
}

impl State {
    pub(super) fn arguments(&self, trait_id: &identity::Definition) -> Option<&[model::Argument]> {
        self.established
            .iter()
            .rev()
            .find(|proof| &proof.trait_id == trait_id)
            .map(|proof| proof.arguments.as_slice())
    }
}

pub(super) struct Machine<'catalog, 'facts> {
    catalog: &'catalog Catalog<'facts>,
    remaining: usize,
    active: Vec<(identity::Definition, Vec<model::Argument>)>,
    active_implementations: Vec<identity::Definition>,
    active_projections: Vec<Projection>,
}

impl<'catalog, 'facts> Machine<'catalog, 'facts> {
    pub(super) fn over(catalog: &'catalog Catalog<'facts>) -> Self {
        Self {
            catalog,
            remaining: 4096,
            active: Vec::new(),
            active_implementations: Vec::new(),
            active_projections: Vec::new(),
        }
    }

    fn tick(&mut self) -> Result<()> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(Failure::Unknown("local proof work exhausted"))?;
        Ok(())
    }

    pub(super) fn normalize(&mut self, ty: &model::Type, state: &mut State) -> Result<model::Type> {
        self.tick()?;
        Ok(match ty {
            model::Type::Parameter { owner, index } => {
                if let Some(value) = state.substitutions.get(&(owner.clone(), *index)).cloned() {
                    self.normalize(&value, state)?
                } else {
                    ty.clone()
                }
            }
            model::Type::SelfType(owner) => self.normalize(
                &model::Type::Parameter {
                    owner: owner.clone(),
                    index: 0,
                },
                state,
            )?,
            model::Type::Definition { id, arguments } => {
                let arguments = self.arguments(arguments, state)?;
                let definition = self
                    .catalog
                    .definitions
                    .get(id)
                    .ok_or(Failure::Unknown("nominal definition is absent"))?;
                if let model::Kind::TypeAlias { ty, generics } = definition.kind() {
                    let mut local = state.clone();
                    self.bind(generics, &arguments, &mut local)?;
                    self.generics(generics, &mut local, None)?;
                    let normalized = self.normalize(ty, &mut local)?;
                    *state = local;
                    normalized
                } else {
                    model::Type::Definition {
                        id: id.clone(),
                        arguments: arguments.into_boxed_slice(),
                    }
                }
            }
            model::Type::Projection {
                trait_id,
                associated,
                arguments,
            } => self.projection(trait_id, associated, arguments, state)?,
            model::Type::Tuple(items) => model::Type::Tuple(
                items
                    .iter()
                    .map(|item| self.normalize(item, state))
                    .collect::<Result<Vec<_>>>()?
                    .into_boxed_slice(),
            ),
            model::Type::Reference {
                lifetime,
                mutability,
                ty,
            } => model::Type::Reference {
                lifetime: lifetime.clone(),
                mutability: *mutability,
                ty: Box::new(self.normalize(ty, state)?),
            },
            model::Type::Primitive(_) | model::Type::Never => ty.clone(),
            _ => {
                return Err(Failure::Unknown(
                    "type form is outside closed structural rules",
                ))
            }
        })
    }

    fn projection(
        &mut self,
        trait_id: &identity::Definition,
        associated: &identity::Definition,
        arguments: &[model::Argument],
        state: &mut State,
    ) -> Result<model::Type> {
        let arguments = self.arguments(arguments, state)?;
        let key = (trait_id.clone(), associated.clone(), arguments.clone());
        if let Some(value) = state.normalized.get(&key) {
            return Ok(value.clone());
        }
        if self.active_projections.contains(&key) {
            return Err(Failure::Unknown("recursive associated value"));
        }
        self.active_projections.push(key.clone());
        let result = (|| {
            let established = self.goal(trait_id, &arguments, state)?;
            let application = established
                .established
                .last()
                .cloned()
                .ok_or(Failure::Unknown(
                    "projection relationship has no implementation",
                ))?;
            let implementation = self
                .catalog
                .implementations
                .get(&application.implementation)
                .copied()
                .ok_or(Failure::Unknown("projection implementation is absent"))?;
            let member = self
                .catalog
                .partitions
                .values()
                .flat_map(|partition| partition.associated.iter())
                .find(|link| {
                    link.implementation == *implementation.id()
                        && link.declaration.as_ref() == Some(associated)
                })
                .map_or(associated, |link| &link.item);
            let definition = self
                .catalog
                .definitions
                .get(member)
                .ok_or(Failure::Unknown("associated definition is absent"))?;
            let model::Kind::AssociatedType {
                default: model::Default::Present(value),
                bounds,
                generics,
            } = definition.kind()
            else {
                return Err(Failure::Unknown(
                    "associated definition has no concrete value",
                ));
            };
            let mut local = established;
            let previous = local.substitutions.clone();
            local.substitutions.extend(application.substitutions);
            self.generics(generics, &mut local, None)?;
            let normalized = self.normalize(value, &mut local)?;
            // The concrete RHS belongs to an already established relation.
            // Its own bounds may name that projection, so retain the value
            // before checking them; a failed check never publishes this state.
            local.normalized.insert(key, normalized.clone());
            self.predicates(bounds.iter(), &mut local, None)?;
            local.substitutions = previous;
            *state = local;
            Ok(normalized)
        })();
        self.active_projections.pop();
        result
    }

    fn arguments(
        &mut self,
        arguments: &[model::Argument],
        state: &mut State,
    ) -> Result<Vec<model::Argument>> {
        arguments
            .iter()
            .map(|argument| match argument {
                model::Argument::Type(ty) => Ok(model::Argument::Type(self.normalize(ty, state)?)),
                _ => Err(Failure::Unknown(
                    "lifetime or const substitution is unsupported",
                )),
            })
            .collect()
    }

    pub(super) fn unify(
        &mut self,
        left: &model::Type,
        right: &model::Type,
        state: &mut State,
    ) -> Result<()> {
        let left = self.normalize(left, state)?;
        let right = self.normalize(right, state)?;
        if left == right {
            return Ok(());
        }
        match (&left, &right) {
            (model::Type::Parameter { owner, index }, value)
            | (value, model::Type::Parameter { owner, index }) => {
                let key = (owner.clone(), *index);
                if occurs(&key, value) {
                    return Err(Failure::Unknown("recursive type substitution"));
                }
                state.substitutions.insert(key, value.clone());
                Ok(())
            }
            (
                model::Type::Definition {
                    id: a,
                    arguments: aa,
                },
                model::Type::Definition {
                    id: b,
                    arguments: ba,
                },
            ) if a == b => self.unify_arguments(aa, ba, state),
            (model::Type::Tuple(a), model::Type::Tuple(b)) if a.len() == b.len() => {
                for (a, b) in a.iter().zip(b) {
                    self.unify(a, b, state)?;
                }
                Ok(())
            }
            (
                model::Type::Reference {
                    mutability: a,
                    ty: at,
                    ..
                },
                model::Type::Reference {
                    mutability: b,
                    ty: bt,
                    ..
                },
            ) if a == b => self.unify(at, bt, state),
            _ => Err(Failure::Absent),
        }
    }

    fn unify_arguments(
        &mut self,
        left: &[model::Argument],
        right: &[model::Argument],
        state: &mut State,
    ) -> Result<()> {
        if left.len() != right.len() {
            return Err(Failure::Absent);
        }
        for (left, right) in left.iter().zip(right) {
            match (left, right) {
                (model::Argument::Type(left), model::Argument::Type(right)) => {
                    self.unify(left, right, state)?
                }
                _ => {
                    return Err(Failure::Unknown(
                        "lifetime or const equality is unsupported",
                    ))
                }
            }
        }
        Ok(())
    }

    pub(super) fn bind(
        &mut self,
        generics: &model::Generics,
        arguments: &[model::Argument],
        state: &mut State,
    ) -> Result<()> {
        if generics.parameters().len() != arguments.len() {
            return Err(Failure::Unknown("generic arity differs"));
        }
        for (parameter, argument) in generics.parameters().zip(arguments) {
            match (parameter.kind(), argument) {
                (model::ParameterKind::Type { .. }, model::Argument::Type(ty)) => self.unify(
                    &model::Type::Parameter {
                        owner: parameter.owner().clone(),
                        index: parameter.index(),
                    },
                    ty,
                    state,
                )?,
                _ => return Err(Failure::Unknown("generic parameter kind is unsupported")),
            }
        }
        Ok(())
    }

    pub(super) fn goal(
        &mut self,
        trait_id: &identity::Definition,
        arguments: &[model::Argument],
        state: &State,
    ) -> Result<State> {
        self.tick()?;
        let mut current = state.clone();
        let arguments = self.arguments(arguments, &mut current)?;
        if let Some(proof) = current
            .established
            .iter()
            .find(|proof| proof.trait_id == *trait_id && proof.arguments == arguments)
            .cloned()
        {
            current.established.push(proof);
            return Ok(current);
        }
        let key = (trait_id.clone(), arguments.clone());
        if self.active.contains(&key) {
            return Err(Failure::Unknown("recursive trait obligation"));
        }
        if let Some(result) = self.builtin(trait_id, &arguments, &mut current)? {
            return result.map(|()| current);
        }
        self.active.push(key);
        let result = self.implementations(trait_id, &arguments, &current);
        self.active.pop();
        result
    }

    fn implementations(
        &mut self,
        trait_id: &identity::Definition,
        arguments: &[model::Argument],
        state: &State,
    ) -> Result<State> {
        let declaration = self
            .catalog
            .definitions
            .get(trait_id)
            .copied()
            .ok_or(Failure::Unknown("trait declaration is absent"))?;
        let model::Kind::Trait {
            auto_trait: false,
            generics,
            supertraits,
            ..
        } = declaration.kind()
        else {
            return Err(Failure::Unknown("auto or unavailable trait semantics"));
        };
        let domain_complete = self.domain(trait_id, arguments);
        let mut successes = Vec::new();
        let mut unresolved = !domain_complete;
        // Only compiler-owned intrinsic implementations are in this index.
        for implementation in self.catalog.implementations.values() {
            let model::Trait::Trait {
                id,
                arguments: pattern,
            } = implementation.trait_ref()
            else {
                continue;
            };
            if id != trait_id {
                continue;
            }
            if self.active_implementations.contains(implementation.id()) {
                unresolved = true;
                continue;
            }
            let mut candidate = state.clone();
            let local_keys = implementation
                .generics()
                .parameters()
                .chain(generics.parameters())
                .map(|parameter| (parameter.owner().clone(), parameter.index()))
                .collect::<Vec<_>>();
            for key in &local_keys {
                candidate.substitutions.remove(key);
            }
            match self.unify_arguments(pattern, arguments, &mut candidate) {
                Ok(()) => {}
                Err(Failure::Absent) => continue,
                Err(_) => {
                    unresolved = true;
                    continue;
                }
            }
            if implementation.polarity() != model::Polarity::Positive
                || implementation.const_impl()
                || implementation.default_impl()
            {
                unresolved = true;
                continue;
            }
            self.active_implementations
                .push(implementation.id().clone());
            let attempt = (|| {
                self.generics(implementation.generics(), &mut candidate, None)?;
                self.bind(generics, arguments, &mut candidate)?;
                let assumption = Some((trait_id, arguments));
                self.generics(generics, &mut candidate, assumption)?;
                self.predicates(supertraits.iter(), &mut candidate, assumption)?;
                let resolved = self.arguments(arguments, &mut candidate)?;
                if resolved
                    .iter()
                    .any(|argument| matches!(argument, model::Argument::Type(ty) if !closed(ty)))
                {
                    return Err(Failure::Unknown("trait substitution is not determined"));
                }
                let values = candidate.substitutions.clone();
                for (key, value) in values {
                    let value = self.normalize(&value, &mut candidate)?;
                    candidate.substitutions.insert(key, value);
                }
                let bindings = local_keys
                    .iter()
                    .filter_map(|key| {
                        candidate
                            .substitutions
                            .get(key)
                            .map(|value| (key.clone(), value.clone()))
                    })
                    .collect();
                for key in local_keys {
                    candidate.substitutions.remove(&key);
                    if let Some(previous) = state.substitutions.get(&key) {
                        candidate.substitutions.insert(key, previous.clone());
                    }
                }
                candidate.established.push(Established {
                    trait_id: trait_id.clone(),
                    arguments: resolved,
                    implementation: implementation.id().clone(),
                    substitutions: bindings,
                });
                candidate.implementations.push(implementation.id().clone());
                Ok(candidate)
            })();
            self.active_implementations.pop();
            match attempt {
                Ok(candidate) => successes.push(candidate),
                Err(Failure::Absent) => {}
                Err(_) => unresolved = true,
            }
        }
        match successes.len() {
            0 if !unresolved => Err(Failure::Absent),
            1 if !unresolved => successes
                .pop()
                .ok_or(Failure::Unknown("proof result disappeared")),
            2.. if !unresolved => Err(Failure::Ambiguous),
            _ => Err(Failure::Unknown(
                "applicability domain or predicates are incomplete",
            )),
        }
    }

    /// A foreign pre-existing crate cannot add an implementation mentioning a
    /// newly defined trait. Every active Cargo dependency must nevertheless be
    /// accounted for; missing partitions are never interpreted as absence.
    fn domain(&self, trait_id: &identity::Definition, arguments: &[model::Argument]) -> bool {
        self.catalog.partitions.contains_key(trait_id.unit())
            && self.catalog.partitions.values().all(|partition| {
                partition.unsupported.is_empty()
                    && partition.missing_dependencies.is_empty()
                    && partition
                        .dependencies
                        .iter()
                        .all(|dependency| self.catalog.partitions.contains_key(dependency))
            })
            && arguments
                .iter()
                .all(|argument| matches!(argument, model::Argument::Type(ty) if self.covered(ty)))
    }

    fn covered(&self, ty: &model::Type) -> bool {
        match ty {
            model::Type::Primitive(_) | model::Type::Parameter { .. } => true,
            model::Type::Definition { id, arguments } => {
                self.catalog.partitions.contains_key(id.unit())
                    && arguments.iter().all(
                        |argument| matches!(argument,model::Argument::Type(ty) if self.covered(ty)),
                    )
            }
            model::Type::Tuple(items) => items.iter().all(|item| self.covered(item)),
            model::Type::Reference { ty, .. } => self.covered(ty),
            _ => false,
        }
    }

    pub(super) fn generics(
        &mut self,
        generics: &model::Generics,
        state: &mut State,
        assumption: Option<(&identity::Definition, &[model::Argument])>,
    ) -> Result<()> {
        let mut predicates = Vec::new();
        for parameter in generics.parameters() {
            let model::ParameterKind::Type { bounds, .. } = parameter.kind() else {
                return Err(Failure::Unknown(
                    "non-type generic predicate is unsupported",
                ));
            };
            let ty = model::Type::Parameter {
                owner: parameter.owner().clone(),
                index: parameter.index(),
            };
            predicates.extend(bounds.iter().map(|bound| model::Predicate::Bound {
                ty: ty.clone(),
                bound: bound.clone(),
            }));
        }
        predicates.extend(generics.predicates().cloned());
        predicates.sort();
        predicates.dedup();
        self.predicates(predicates.iter(), state, assumption)
    }

    fn predicates<'predicate>(
        &mut self,
        predicates: impl Iterator<Item = &'predicate model::Predicate>,
        state: &mut State,
        assumption: Option<(&identity::Definition, &[model::Argument])>,
    ) -> Result<()> {
        let mut pending = predicates.collect::<Vec<_>>();
        loop {
            let before = state.substitutions.clone();
            let count = pending.len();
            let mut next = Vec::new();
            for predicate in pending {
                let result = match predicate {
                    model::Predicate::Bound { ty, bound } => {
                        self.bound(ty, bound, state, assumption)
                    }
                    model::Predicate::ProjectionEquality { projection, ty } => {
                        self.unify(projection, ty, state)
                    }
                    _ => Err(Failure::Unknown(
                        "lifetime or const predicate is unsupported",
                    )),
                };
                match result {
                    Ok(()) => {}
                    Err(Failure::Absent) => return Err(Failure::Absent),
                    Err(_) => next.push(predicate),
                }
            }
            if next.is_empty() {
                return Ok(());
            }
            if next.len() == count && before == state.substitutions {
                return Err(Failure::Unknown(
                    "predicates require unsupported or unresolved reasoning",
                ));
            }
            pending = next;
            self.tick()?;
        }
    }

    fn bound(
        &mut self,
        ty: &model::Type,
        bound: &model::Bound,
        state: &mut State,
        assumption: Option<(&identity::Definition, &[model::Argument])>,
    ) -> Result<()> {
        match bound {
            model::Bound::MaybeSized => Ok(()),
            model::Bound::Trait {
                trait_id,
                arguments,
                polarity: model::Polarity::Positive,
                binders,
            } if binders.is_empty() => {
                let mut complete = vec![model::Argument::Type(self.normalize(ty, state)?)];
                complete.extend(self.arguments(arguments, state)?);
                if let Some((id, arguments)) = assumption
                    && id == trait_id
                    && self.arguments(arguments, state)? == complete
                {
                    return Ok(());
                }
                *state = self.goal(trait_id, &complete, state)?;
                Ok(())
            }
            _ => Err(Failure::Unknown(
                "negative or higher-ranked predicate is unsupported",
            )),
        }
    }

    fn builtin(
        &mut self,
        trait_id: &identity::Definition,
        arguments: &[model::Argument],
        state: &mut State,
    ) -> Result<Option<Result<()>>> {
        let Some(definition) = self.catalog.definitions.get(trait_id).copied() else {
            return Ok(None);
        };
        if !matches!(trait_id.unit().authority(), identity::Authority::Sysroot(_))
            || trait_id.unit().crate_name() != "core"
        {
            return Ok(None);
        }
        let path = definition.display_path().collect::<Vec<_>>();
        let [model::Argument::Type(ty)] = arguments else {
            return Ok(None);
        };
        if path.ends_with(&["marker", "Sized"]) {
            return Ok(Some(self.sized(ty, state)));
        }
        if path.ends_with(&["marker", "MetaSized"]) {
            // Sized implies MetaSized. The reverse implication is not valid;
            // this closed slice has no exclusion rule for other types.
            return Ok(Some(
                self.sized(ty, state)
                    .map_err(|_| Failure::Unknown("MetaSized needs another rule")),
            ));
        }
        if (path.ends_with(&["marker", "Copy"]) || path.ends_with(&["clone", "Clone"]))
            && (matches!(ty, model::Type::Primitive(name) if matches!(name.as_ref(), "bool"|"char"|"i8"|"i16"|"i32"|"i64"|"i128"|"isize"|"u8"|"u16"|"u32"|"u64"|"u128"|"usize"|"f32"|"f64"))
                || matches!(ty, model::Type::Tuple(items) if items.is_empty()))
        {
            return Ok(Some(Ok(())));
        }
        Ok(None)
    }

    fn sized(&mut self, ty: &model::Type, state: &mut State) -> Result<()> {
        let ty = self.normalize(ty, state)?;
        match ty {
            model::Type::Primitive(name) if name.as_ref() != "str" => Ok(()),
            model::Type::Primitive(_) => Err(Failure::Absent),
            model::Type::Reference { .. } | model::Type::Never => Ok(()),
            model::Type::Tuple(items) => {
                for item in &items {
                    self.sized(item, state)?;
                }
                Ok(())
            }
            model::Type::Definition { id, arguments } => {
                let definition = self
                    .catalog
                    .definitions
                    .get(&id)
                    .copied()
                    .ok_or(Failure::Unknown("Sized owner is absent"))?;
                let model::Kind::Struct { fields, generics } = definition.kind() else {
                    return Err(Failure::Unknown("nominal Sized rule is unsupported"));
                };
                if !fields.public_complete() {
                    return Err(Failure::Unknown("nominal fields do not establish Sized"));
                }
                let mut local = state.clone();
                self.bind(generics, &arguments, &mut local)?;
                for field in fields.public_fields() {
                    self.sized(field.ty(), &mut local)?;
                }
                Ok(())
            }
            _ => Err(Failure::Unknown("Sized depends on an unresolved type")),
        }
    }
}

fn occurs(key: &Key, ty: &model::Type) -> bool {
    match ty {
        model::Type::Parameter { owner, index } => owner == &key.0 && *index == key.1,
        model::Type::SelfType(owner) => owner == &key.0 && key.1 == 0,
        model::Type::Definition { arguments, .. } | model::Type::Projection { arguments, .. } => {
            arguments
                .iter()
                .any(|argument| matches!(argument,model::Argument::Type(ty) if occurs(key,ty)))
        }
        model::Type::Tuple(items) => items.iter().any(|item| occurs(key, item)),
        model::Type::Reference { ty, .. } => occurs(key, ty),
        _ => false,
    }
}

fn closed(ty: &model::Type) -> bool {
    match ty {
        model::Type::Primitive(_) | model::Type::Never => true,
        model::Type::Definition { arguments, .. } => arguments
            .iter()
            .all(|argument| matches!(argument,model::Argument::Type(ty) if closed(ty))),
        model::Type::Tuple(items) => items.iter().all(closed),
        model::Type::Reference { ty, .. } => closed(ty),
        _ => false,
    }
}
