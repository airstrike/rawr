//! Call-specific selection and ABI proof; ordinary facade selectors stay separate.

use std::collections::BTreeSet;

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::{inventory, resolve, scope, Catalog};
use crate::api::{identity, model};
use crate::classify::representability::convert;
use crate::surface;

/// Checked source types and the original lookup operation supplied by lowering.
/// Expected output is deliberately absent from candidate selection.
#[derive(Clone, Debug, Serialize)]
pub struct Request {
    pub receiver: convert::Type,
    pub mutable: bool,
    pub lookup: Lookup,
    pub arguments: Box<[convert::Type]>,
    pub type_arguments: Box<[convert::Type]>,
}

#[derive(Clone, Debug, Serialize)]
pub enum Lookup {
    Method {
        name: Box<str>,
    },
    Ufcs {
        trait_id: surface::identity::Definition,
        path: Box<[Box<str>]>,
        name: Box<str>,
    },
}

/// An applicable source method plus its concrete ABI and all local determinants.
#[derive(Clone, Debug)]
pub struct Proof {
    request: Request,
    scope: scope::Bound,
    declaration: surface::identity::Definition,
    trait_id: surface::identity::Definition,
    trait_arguments: Box<[convert::Type]>,
    output: convert::Type,
    implementations: Box<[surface::identity::Definition]>,
    adjustment: usize,
    fingerprint: [u8; 32],
}

#[derive(Debug)]
pub enum Outcome {
    Applicable(Box<Proof>),
    NotApplicable(Exclusion),
    Ambiguous(Ambiguity),
    Unknown { reason: &'static str },
}

/// A complete negative conclusion, bound to the whole competing domain.
#[derive(Debug)]
pub struct Exclusion {
    fingerprint: [u8; 32],
}

/// Distinct applicable declarations at the same source receiver rank.
#[derive(Debug)]
pub struct Ambiguity {
    declarations: Box<[surface::identity::Definition]>,
    fingerprint: [u8; 32],
}

impl Exclusion {
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

impl Ambiguity {
    pub fn declarations(&self) -> &[surface::identity::Definition] {
        &self.declarations
    }
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

struct Candidate {
    declaration: identity::Definition,
    trait_id: identity::Definition,
    arguments: Vec<model::Argument>,
    state: resolve::State,
    adjustment: usize,
}

impl Catalog<'_> {
    pub fn prove(&self, request: Request, scope: &scope::Bound) -> Outcome {
        match self.select(&request, scope) {
            Ok(candidates) if candidates.len() > 1 => {
                let declarations = candidates
                    .iter()
                    .map(|candidate| surface::identity::Definition::of(&candidate.declaration))
                    .collect::<Box<[_]>>();
                match self.conclusion("ambiguous", &request, scope, &declarations) {
                    Some(fingerprint) => Outcome::Ambiguous(Ambiguity {
                        declarations,
                        fingerprint,
                    }),
                    None => Outcome::Unknown {
                        reason: "ambiguity evidence encoding failed",
                    },
                }
            }
            Ok(mut candidates) => match candidates.pop() {
                Some(candidate) => match self.finish(&request, scope, candidate) {
                    Ok(proof) => Outcome::Applicable(Box::new(proof)),
                    Err(resolve::Failure::Absent) => self.exclusion(&request, scope),
                    Err(resolve::Failure::Ambiguous) => Outcome::Unknown {
                        reason: "associated normalization is ambiguous",
                    },
                    Err(resolve::Failure::Unknown(reason)) => Outcome::Unknown { reason },
                },
                None => self.exclusion(&request, scope),
            },
            Err(resolve::Failure::Absent) => self.exclusion(&request, scope),
            Err(resolve::Failure::Ambiguous) => Outcome::Unknown {
                reason: "trait substitutions are ambiguous",
            },
            Err(resolve::Failure::Unknown(reason)) => Outcome::Unknown { reason },
        }
    }

    fn conclusion(
        &self,
        kind: &str,
        request: &Request,
        scope: &scope::Bound,
        declarations: &[surface::identity::Definition],
    ) -> Option<[u8; 32]> {
        let mut hash = Sha256::new();
        hash.update(b"rawr.import.applicability.conclusion/v1");
        hash.update(self.fingerprint);
        hash.update(scope.fingerprint());
        hash.update(serde_json::to_vec(&(kind, request, declarations)).ok()?);
        Some(hash.finalize().into())
    }

    fn exclusion(&self, request: &Request, scope: &scope::Bound) -> Outcome {
        match self.conclusion("excluded", request, scope, &[]) {
            Some(fingerprint) => Outcome::NotApplicable(Exclusion { fingerprint }),
            None => Outcome::Unknown {
                reason: "exclusion evidence encoding failed",
            },
        }
    }

    fn select(
        &self,
        request: &Request,
        scope: &scope::Bound,
    ) -> Result<Vec<Candidate>, resolve::Failure> {
        if scope.catalog != self.fingerprint {
            return Err(resolve::Failure::Unknown(
                "scope belongs to another inventory union",
            ));
        }
        let name = match &request.lookup {
            Lookup::Method { name } | Lookup::Ufcs { name, .. } => name.as_ref(),
        };
        let mut traits = BTreeSet::new();
        if let Lookup::Ufcs { trait_id, path, .. } = &request.lookup {
            let Some((root, path)) = path.split_first() else {
                return Err(resolve::Failure::Unknown("explicit trait path is absent"));
            };
            let path = path.iter().map(AsRef::as_ref).collect::<Vec<_>>();
            if self.export(root, &path) != Some(*trait_id) {
                return Err(resolve::Failure::Unknown(
                    "explicit trait path differs from its chosen identity",
                ));
            }
            traits.insert(
                *self
                    .identities
                    .get(trait_id)
                    .ok_or(resolve::Failure::Unknown(
                        "explicit trait identity is absent",
                    ))?,
            );
        } else {
            for imported in scope.traits() {
                traits.insert(
                    *self
                        .identities
                        .get(&imported.definition)
                        .ok_or(resolve::Failure::Unknown("scope trait identity is absent"))?,
                );
            }
            if let scope::Prelude::Active { module } = scope.prelude() {
                let prelude = self
                    .partitions
                    .values()
                    .flat_map(|partition| partition.preludes.iter())
                    .find(|prelude| {
                        surface::identity::Definition::of(&prelude.module) == *module
                            && prelude.edition.as_ref() == scope.edition()
                    })
                    .ok_or(resolve::Failure::Unknown(
                        "effective prelude domain is absent",
                    ))?;
                for trait_ in &prelude.traits {
                    if trait_.methods.iter().any(|method| method.as_ref() == name) {
                        traits.insert(&trait_.id);
                    }
                }
            }
        }
        let mut machine = resolve::Machine::over(self);
        let receiver = self.input_type(&request.receiver)?;
        let receiver = machine.normalize(&receiver, &mut resolve::State::default())?;
        let mut steps = Vec::new();
        let mut current = receiver;
        loop {
            steps.push(current.clone());
            if matches!(request.lookup, Lookup::Method { .. }) {
                steps.push(model::Type::Reference {
                    lifetime: model::Lifetime::Erased,
                    mutability: model::Mutability::Shared,
                    ty: Box::new(current.clone()),
                });
                steps.push(model::Type::Reference {
                    lifetime: model::Lifetime::Erased,
                    mutability: model::Mutability::Mutable,
                    ty: Box::new(current.clone()),
                });
            }
            match current {
                model::Type::Reference { ty, .. }
                    if matches!(request.lookup, Lookup::Method { .. }) =>
                {
                    current = *ty
                }
                _ => break,
            }
        }
        for (adjustment, receiver) in steps.iter().enumerate() {
            if matches!(request.lookup, Lookup::Method { .. }) {
                self.inherent_absence(receiver, name)?;
            }
            let mut candidates = Vec::new();
            let mut unknown = None;
            for trait_id in &traits {
                let definition = self
                    .definitions
                    .get(trait_id)
                    .copied()
                    .ok_or(resolve::Failure::Unknown("candidate trait shape is absent"))?;
                let model::Kind::Trait {
                    associated,
                    generics,
                    ..
                } = definition.kind()
                else {
                    return Err(resolve::Failure::Unknown(
                        "candidate trait semantics are unsupported",
                    ));
                };
                for member in associated {
                    let member =
                        self.definitions
                            .get(member)
                            .copied()
                            .ok_or(resolve::Failure::Unknown(
                                "trait member domain is incomplete",
                            ))?;
                    if member.display_path().last() != Some(name) {
                        continue;
                    }
                    let model::Kind::Function { signature, .. } = member.kind() else {
                        continue;
                    };
                    let self_type = match signature.receiver() {
                        model::Receiver::Value { .. } => receiver,
                        model::Receiver::Reference { mutability, .. } => match receiver {
                            model::Type::Reference {
                                mutability: actual,
                                ty,
                                ..
                            } if actual == mutability => ty,
                            _ => continue,
                        },
                        model::Receiver::None => continue,
                        _ => {
                            return Err(resolve::Failure::Unknown(
                                "arbitrary receiver adjustment is unsupported",
                            ))
                        }
                    };
                    let arguments = generics
                        .parameters()
                        .map(|parameter| {
                            if parameter.index() == 0 {
                                model::Argument::Type(self_type.clone())
                            } else {
                                model::Argument::Type(model::Type::Parameter {
                                    owner: parameter.owner().clone(),
                                    index: parameter.index(),
                                })
                            }
                        })
                        .collect::<Vec<_>>();
                    match machine.goal(trait_id, &arguments, &resolve::State::default()) {
                        Ok(state) => {
                            let arguments = state
                                .arguments(trait_id)
                                .ok_or(resolve::Failure::Unknown(
                                    "candidate relationship is not retained",
                                ))?
                                .to_vec();
                            candidates.push(Candidate {
                                declaration: member.id().clone(),
                                trait_id: (*trait_id).clone(),
                                arguments,
                                state,
                                adjustment,
                            });
                        }
                        Err(resolve::Failure::Absent) => {}
                        Err(error) => unknown = Some(error),
                    }
                }
            }
            if let Some(error) = unknown {
                return Err(error);
            }
            if !candidates.is_empty() {
                return Ok(candidates);
            }
        }
        if matches!(request.lookup, Lookup::Ufcs { .. }) {
            return Err(resolve::Failure::Absent);
        }
        let terminal = steps
            .last()
            .ok_or(resolve::Failure::Unknown("receiver chain is empty"))?;
        self.terminal(terminal)?;
        Err(resolve::Failure::Absent)
    }

    fn terminal(&self, receiver: &model::Type) -> Result<(), resolve::Failure> {
        let receiver = match receiver {
            model::Type::Reference { ty, .. } => ty.as_ref(),
            other => other,
        };
        let model::Type::Definition { id, .. } = receiver else {
            return Err(resolve::Failure::Unknown(
                "receiver termination requires an owned nominal",
            ));
        };
        let mut trait_id = None;
        let mut domains = std::collections::BTreeMap::new();
        for partition in self.partitions.values() {
            let lookup = partition
                .deref
                .as_ref()
                .ok_or(resolve::Failure::Unknown("Deref upstream domain is absent"))?;
            if trait_id.is_some_and(|previous| previous != &lookup.trait_id) {
                return Err(resolve::Failure::Unknown(
                    "Deref compiler identities differ",
                ));
            }
            trait_id = Some(&lookup.trait_id);
            for domain in &lookup.domains {
                if let Some(previous) = domains.insert(&domain.root, domain)
                    && previous != domain
                {
                    return Err(resolve::Failure::Unknown("Deref upstream headers disagree"));
                }
            }
        }
        for domain in domains.values() {
            for header in &domain.implementations {
                match &header.receiver {
                    inventory::Receiver::Nominal(owner) if owner != id => {}
                    inventory::Receiver::Closed => {}
                    _ => {
                        return Err(resolve::Failure::Unknown(
                            "upstream Deref may apply to the receiver",
                        ))
                    }
                }
            }
        }
        let trait_id =
            trait_id.ok_or(resolve::Failure::Unknown("Deref trait identity is absent"))?;
        let mut machine = resolve::Machine::over(self);
        for implementation in self.implementations.values() {
            if matches!(implementation.trait_ref(),model::Trait::Trait { id, .. } if id == trait_id)
            {
                match machine.unify(
                    implementation.self_type(),
                    receiver,
                    &mut resolve::State::default(),
                ) {
                    Err(resolve::Failure::Absent) => {}
                    _ => {
                        return Err(resolve::Failure::Unknown(
                            "owned Deref needs a further receiver proof",
                        ))
                    }
                }
            }
        }
        Ok(())
    }

    fn inherent_absence(&self, receiver: &model::Type, name: &str) -> Result<(), resolve::Failure> {
        let receiver = match receiver {
            model::Type::Reference { ty, .. } => ty.as_ref(),
            other => other,
        };
        let model::Type::Definition { id, .. } = receiver else {
            return Err(resolve::Failure::Unknown(
                "inherent candidate domain is unsupported",
            ));
        };
        let domain = self
            .partitions
            .get(id.unit())
            .and_then(|partition| partition.inherent.iter().find(|domain| &domain.owner == id))
            .ok_or(resolve::Failure::Unknown(
                "inherent candidate domain is absent",
            ))?;
        for link in &domain.associated {
            let definition = self
                .definitions
                .get(&link.item)
                .copied()
                .ok_or(resolve::Failure::Unknown("inherent member is absent"))?;
            if definition.display_path().last() == Some(name) {
                return Err(resolve::Failure::Unknown(
                    "matching inherent method needs receiver ranking",
                ));
            }
        }
        Ok(())
    }

    fn finish(
        &self,
        request: &Request,
        scope: &scope::Bound,
        mut candidate: Candidate,
    ) -> Result<Proof, resolve::Failure> {
        let declaration = self
            .definitions
            .get(&candidate.declaration)
            .copied()
            .ok_or(resolve::Failure::Unknown(
                "chosen method declaration is absent",
            ))?;
        let model::Kind::Function {
            signature,
            generics,
        } = declaration.kind()
        else {
            return Err(resolve::Failure::Unknown(
                "chosen declaration is not a method",
            ));
        };
        let trait_ = self
            .definitions
            .get(&candidate.trait_id)
            .copied()
            .ok_or(resolve::Failure::Unknown("chosen trait is absent"))?;
        let model::Kind::Trait {
            generics: trait_generics,
            ..
        } = trait_.kind()
        else {
            return Err(resolve::Failure::Unknown(
                "chosen trait shape is unsupported",
            ));
        };
        if signature.safety() != model::Safety::Safe
            || signature.async_function()
            || signature.variadic()
            || signature.guard().is_some()
        {
            return Err(resolve::Failure::Unknown(
                "method ABI is outside closed rules",
            ));
        }
        if generics.parameters().len() != 0 || !request.type_arguments.is_empty() {
            return Err(resolve::Failure::Unknown(
                "method-owned generic inference is unsupported",
            ));
        }
        let mut machine = resolve::Machine::over(self);
        machine.bind(trait_generics, &candidate.arguments, &mut candidate.state)?;
        machine.generics(generics, &mut candidate.state, None)?;
        if signature.inputs().len() != request.arguments.len() {
            return Err(resolve::Failure::Absent);
        }
        for (expected, actual) in signature.inputs().zip(&request.arguments) {
            machine.unify(expected, &self.input_type(actual)?, &mut candidate.state)?;
        }
        if matches!(
            signature.receiver(),
            model::Receiver::Reference {
                mutability: model::Mutability::Mutable,
                ..
            }
        ) && !request.mutable
        {
            return Err(resolve::Failure::Absent);
        }
        let output =
            self.output_type(&machine.normalize(signature.output(), &mut candidate.state)?)?;
        let trait_arguments = candidate
            .arguments
            .iter()
            .map(|argument| match argument {
                model::Argument::Type(ty) => self.output_type(ty),
                _ => Err(resolve::Failure::Unknown(
                    "non-type trait argument is unsupported",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_boxed_slice();
        let implementations = candidate
            .state
            .implementations
            .iter()
            .map(surface::identity::Definition::of)
            .collect::<Box<[_]>>();
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"rawr.import.applicability.proof/v1");
        fingerprint.update(self.fingerprint);
        fingerprint.update(scope.fingerprint());
        fingerprint.update(
            serde_json::to_vec(&(
                &request,
                &candidate.declaration,
                &candidate.trait_id,
                &trait_arguments,
                &output,
                &implementations,
                candidate.adjustment,
            ))
            .map_err(|_| resolve::Failure::Unknown("call proof encoding failed"))?,
        );
        Ok(Proof {
            request: request.clone(),
            scope: scope.clone(),
            declaration: surface::identity::Definition::of(&candidate.declaration),
            trait_id: surface::identity::Definition::of(&candidate.trait_id),
            trait_arguments,
            output,
            implementations,
            adjustment: candidate.adjustment,
            fingerprint: fingerprint.finalize().into(),
        })
    }

    pub(super) fn input_type(&self, ty: &convert::Type) -> Result<model::Type, resolve::Failure> {
        Ok(match ty {
            convert::Type::Unit => model::Type::Tuple(Box::new([])),
            convert::Type::Builtin(builtin) => model::Type::Primitive(
                builtin
                    .rust_name()
                    .ok_or(resolve::Failure::Unknown(
                        "builtin input is not a closed primitive",
                    ))?
                    .into(),
            ),
            convert::Type::Nominal {
                definition,
                arguments,
            } => model::Type::Definition {
                id: (*self
                    .identities
                    .get(definition)
                    .ok_or(resolve::Failure::Unknown(
                        "input nominal identity is absent",
                    ))?)
                .clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| self.input_type(argument).map(model::Argument::Type))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            },
            convert::Type::Tuple(items) => model::Type::Tuple(
                items
                    .iter()
                    .map(|item| self.input_type(item))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            ),
            convert::Type::Borrowed { mutable, ty } => model::Type::Reference {
                lifetime: model::Lifetime::Erased,
                mutability: if *mutable {
                    model::Mutability::Mutable
                } else {
                    model::Mutability::Shared
                },
                ty: Box::new(self.input_type(ty)?),
            },
            _ => {
                return Err(resolve::Failure::Unknown(
                    "input is outside closed type rules",
                ))
            }
        })
    }

    fn output_type(&self, ty: &model::Type) -> Result<convert::Type, resolve::Failure> {
        Ok(match ty {
            model::Type::Primitive(name) => convert::Type::Builtin(
                convert::Builtin::primitive(name)
                    .ok_or(resolve::Failure::Unknown("output primitive has no ABI"))?,
            ),
            model::Type::Tuple(items) if items.is_empty() => convert::Type::Unit,
            model::Type::Tuple(items) => convert::Type::Tuple(
                items
                    .iter()
                    .map(|item| self.output_type(item))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            ),
            model::Type::Definition { id, arguments } if self.definitions.contains_key(id) => {
                convert::Type::Nominal {
                    definition: surface::identity::Definition::of(id),
                    arguments: arguments
                        .iter()
                        .map(|argument| match argument {
                            model::Argument::Type(ty) => self.output_type(ty),
                            _ => Err(resolve::Failure::Unknown(
                                "non-type output argument has no ABI",
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .into_boxed_slice(),
                }
            }
            _ => {
                return Err(resolve::Failure::Unknown(
                    "output is not a closed supported ABI",
                ))
            }
        })
    }
}

impl Proof {
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
    pub const fn output(&self) -> &convert::Type {
        &self.output
    }
    pub const fn declaration(&self) -> surface::identity::Definition {
        self.declaration
    }
    pub const fn trait_id(&self) -> surface::identity::Definition {
        self.trait_id
    }
    pub fn trait_arguments(&self) -> &[convert::Type] {
        &self.trait_arguments
    }
    pub const fn scope(&self) -> &scope::Bound {
        &self.scope
    }
    pub const fn request(&self) -> &Request {
        &self.request
    }
    pub fn implementations(&self) -> &[surface::identity::Definition] {
        &self.implementations
    }
    pub const fn adjustment(&self) -> usize {
        self.adjustment
    }
}
