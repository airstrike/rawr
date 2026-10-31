//! Proof-to-application transformation, independent of native compiler feedback.

use sha2::{Digest as _, Sha256};

use super::{call, resolve, scope, Catalog};
use crate::api::{identity, model};
use crate::classify::representability::{convert, ownership, signature};
use crate::surface::application::{Application, Call, Expression, Invocation, Path, Type};

/// Why a lowering call has no application: Rust rejects it, Rust finds it
/// ambiguous, or the closed rules cannot decide.
#[derive(Debug, thiserror::Error)]
pub enum Rejection {
    #[error("no method applies to the receiver in this scope")]
    NotApplicable,
    #[error("{0} candidate methods apply in this scope (E0034)")]
    Ambiguous(usize),
    #[error("applicability is undecided: {0}")]
    Unknown(&'static str),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("local application is unsupported: {0}")]
    Unsupported(&'static str),
}

impl Catalog<'_> {
    /// Classify an established call without consulting any compiler or expected
    /// return type. Public render paths remain local to this application.
    pub fn apply(&self, proof: &call::Proof, call: Call) -> Result<Application, Error> {
        if proof.scope().catalog != self.fingerprint {
            return Err(Error::Unsupported(
                "call proof belongs to another inventory union",
            ));
        }
        let definition = self
            .identities
            .get(&proof.declaration())
            .and_then(|id| self.definitions.get(id))
            .ok_or(Error::Unsupported("method declaration is absent"))?;
        let model::Kind::Function {
            signature: declared,
            ..
        } = definition.kind()
        else {
            return Err(Error::Unsupported("method declaration is not a function"));
        };
        // The declared receiver decides how the adapter lends the value:
        // `&self` and `&mut self` borrow it in its slot, `self` takes it.
        let ownership = match declared.receiver() {
            model::Receiver::Reference {
                mutability: model::Mutability::Shared,
                ..
            } => ownership::Receiver::Shared,
            model::Receiver::Reference {
                mutability: model::Mutability::Mutable,
                ..
            } => ownership::Receiver::Mutable,
            model::Receiver::Value { .. } => ownership::Receiver::Owned,
            _ => {
                return Err(Error::Unsupported(
                    "the declared receiver has no native application transport",
                ))
            }
        };
        let request = proof.request();
        let (receiver, expression) = match (&request.lookup, &request.receiver) {
            (call::Lookup::Method { name }, receiver @ convert::Type::Nominal { .. }) => {
                (receiver.clone(), Expression::Method { name: name.clone() })
            }
            (
                call::Lookup::Ufcs { path, name, .. },
                convert::Type::Borrowed { mutable: false, ty },
            ) if matches!(ty.as_ref(), convert::Type::Nominal { .. }) => (
                ty.as_ref().clone(),
                Expression::Ufcs {
                    path: path.clone(),
                    name: name.clone(),
                },
            ),
            _ => {
                return Err(Error::Unsupported(
                    "original receiver form has no proved native transport",
                ))
            }
        };
        let signature = signature::Signature::new(
            Box::new([]),
            request.arguments.clone(),
            proof.output().clone(),
        )
        .with_instance(Some(receiver.clone()));
        let native_receiver = self.native_type(&receiver)?;
        let export = self.export_of(&receiver)?;
        let native_inputs = request
            .arguments
            .iter()
            .map(|ty| self.native_type(ty))
            .collect::<Result<Box<[_]>, _>>()?;
        let native_output = self.native_type(proof.output())?;
        let prelude = match proof.scope().prelude() {
            scope::Prelude::Disabled => None,
            scope::Prelude::Active { module } => {
                let prelude = self
                    .partitions
                    .values()
                    .flat_map(|partition| &partition.preludes)
                    .find(|prelude| {
                        crate::surface::identity::Definition::of(&prelude.module) == *module
                            && prelude.edition.as_ref() == proof.scope().edition()
                    })
                    .ok_or(Error::Unsupported("effective prelude domain is absent"))?;
                Some(
                    vec![
                        prelude.module.unit().crate_name().into(),
                        "prelude".into(),
                        format!("rust_{}", prelude.edition).into_boxed_str(),
                    ]
                    .into_boxed_slice(),
                )
            }
        };
        let mut invocation = Invocation {
            identity: [0; 32],
            proof: *proof.fingerprint(),
            catalog: self.fingerprint,
            source: proof.scope().request.source,
            scope: *proof.scope().fingerprint(),
            call,
            declaration: proof.declaration(),
            export,
            origin: crate::classify::representability::origin::Set::new(
                definition.span().clone(),
                definition.span().clone(),
            ),
            receiver,
            ownership,
            signature,
            native_receiver,
            native_inputs,
            native_output,
            expression,
            prelude,
            traits: proof
                .scope()
                .traits()
                .map(|trait_| trait_.path.clone())
                .collect(),
        };
        let mut identity = Sha256::new();
        identity.update(b"rawr.import.applicability.application/v1");
        identity.update(
            serde_json::to_vec(&invocation)
                .map_err(|_| Error::Unsupported("application encoding failed"))?,
        );
        invocation.identity = identity.finalize().into();
        Ok(Application { invocation })
    }

    /// Prove one lowering call in its source scope and classify it. Only
    /// import paths that name traits in this catalog can change method
    /// lookup; other paths in scope are types, values or modules.
    pub fn prove_call(&self, call: &Call, edition: &str) -> Result<Application, Rejection> {
        let traits = self.scope_traits(call)?;
        let prelude = match self.prelude("std", edition) {
            Some(module) => scope::Prelude::Active { module },
            None => {
                return Err(Rejection::Unknown(
                    "the source edition's std prelude has no authenticated facts",
                ))
            }
        };
        let key = call.key();
        let bound = self
            .scope(scope::Request {
                source: key,
                lexical: key,
                edition: edition.into(),
                prelude,
                traits,
            })
            .map_err(|_| Rejection::Unknown("the source scope does not bind to these facts"))?;
        let receiver = convert::Type::Nominal {
            definition: call.receiver,
            arguments: call.receiver_arguments.clone(),
        };
        let request = call::Request {
            receiver,
            mutable: call.mutable,
            lookup: call::Lookup::Method {
                name: call.name.clone(),
            },
            arguments: call.arguments.clone(),
            type_arguments: Box::new([]),
        };
        match self.prove(request, &bound) {
            call::Outcome::Applicable(proof) => self
                .apply(&proof, call.clone())
                .map_err(|Error::Unsupported(reason)| Rejection::Unknown(reason)),
            call::Outcome::NotApplicable(_) => Err(Rejection::NotApplicable),
            call::Outcome::Ambiguous(ambiguity) => {
                Err(Rejection::Ambiguous(ambiguity.declarations().len()))
            }
            call::Outcome::Unknown { reason } => Err(Rejection::Unknown(reason)),
        }
    }

    /// The traits a call's lexical import paths bring into scope. A path
    /// into a crate this catalog holds names a trait, or a type, value or
    /// module that cannot change method lookup; its complete exports decide
    /// which. A glob brings every trait its module exports. A path into a
    /// crate with no intrinsic facts here, such as `std`, may name a trait
    /// the proof cannot see, so the call stays undecided.
    fn scope_traits(&self, call: &Call) -> Result<Vec<scope::Import>, Rejection> {
        let roots = self
            .table()
            .map(|(key, _)| key.root_alias())
            .collect::<std::collections::BTreeSet<_>>();
        let mut traits = Vec::new();
        let mut push = |definition: crate::surface::identity::Definition, path: Path| {
            let is_trait = self
                .identities
                .get(&definition)
                .and_then(|id| self.definitions.get(id))
                .is_some_and(|definition| matches!(definition.kind(), model::Kind::Trait { .. }));
            if is_trait {
                traits.push(scope::Import { definition, path });
            }
        };
        for path in call.scope.iter() {
            let Some((root, rest)) = path.split_first() else {
                continue;
            };
            // A path the source resolves inside its own crate names script
            // items, which no Rust impl selection here depends on.
            if matches!(root.as_ref(), "crate" | "self" | "super") {
                continue;
            }
            if !roots.contains(root.as_ref()) {
                return Err(Rejection::Unknown(
                    "an import in scope names a crate with no intrinsic facts in this proof",
                ));
            }
            if let Some((last, module)) = rest.split_last()
                && last.as_ref() == "*"
            {
                let module = module.iter().map(AsRef::as_ref).collect::<Vec<_>>();
                for (key, target) in self.table() {
                    let segments = key.path().collect::<Vec<_>>();
                    if key.root_alias() == root.as_ref()
                        && target.namespace() == identity::Namespace::Type
                        && segments.len() == module.len() + 1
                        && segments[..module.len()] == module[..]
                    {
                        push(
                            crate::surface::identity::Definition::of(target),
                            std::iter::once(root.clone())
                                .chain(segments.iter().map(|segment| (*segment).into()))
                                .collect(),
                        );
                    }
                }
                continue;
            }
            let rest = rest.iter().map(AsRef::as_ref).collect::<Vec<_>>();
            if let Some(definition) = self.export(root, &rest) {
                push(definition, path.clone());
            }
        }
        Ok(traits)
    }

    fn native_type(&self, ty: &convert::Type) -> Result<Type, Error> {
        Ok(match ty {
            convert::Type::Unit => Type::Tuple(Box::new([])),
            convert::Type::Builtin(builtin) => Type::Primitive(
                builtin
                    .rust_name()
                    .ok_or(Error::Unsupported(
                        "a text or range value has no closed native spelling here",
                    ))?
                    .into(),
            ),
            convert::Type::Tuple(members) => Type::Tuple(
                members
                    .iter()
                    .map(|ty| self.native_type(ty))
                    .collect::<Result<Box<[_]>, _>>()?,
            ),
            convert::Type::Borrowed { mutable, ty } => Type::Reference {
                mutable: *mutable,
                ty: Box::new(self.native_type(ty)?),
            },
            convert::Type::Nominal { arguments, .. } => {
                let export = self.export_of(ty)?;
                Type::Export {
                    path: path(&export),
                    arguments: if self.exported_target(ty)? {
                        arguments
                            .iter()
                            .map(|ty| self.native_type(ty))
                            .collect::<Result<Box<[_]>, _>>()?
                    } else {
                        Box::new([])
                    },
                }
            }
            _ => {
                return Err(Error::Unsupported(
                    "native type is outside the closed application rules",
                ))
            }
        })
    }
}

impl Catalog<'_> {
    /// Public type-namespace exports and their targets, sorted by export.
    fn exports(&self) -> Vec<(&identity::Export, &identity::Definition)> {
        let mut exports = self
            .table()
            .filter(|(_, target)| target.namespace() == identity::Namespace::Type)
            .collect::<Vec<_>>();
        exports.sort_unstable();
        exports.dedup();
        exports
    }

    /// Whether a nominal's own definition has a public export, so its
    /// arguments are spelled; otherwise a closed alias names it whole.
    fn exported_target(&self, ty: &convert::Type) -> Result<bool, Error> {
        let convert::Type::Nominal { definition, .. } = ty else {
            return Ok(false);
        };
        let semantic = self
            .identities
            .get(definition)
            .ok_or(Error::Unsupported("native type identity is absent"))?;
        Ok(self.exports().iter().any(|(_, target)| target == semantic))
    }

    /// The public export that spells a closed nominal: its own, or a closed
    /// public alias whose normalized target is exactly this type.
    fn export_of(&self, ty: &convert::Type) -> Result<identity::Export, Error> {
        let convert::Type::Nominal { definition, .. } = ty else {
            return Err(Error::Unsupported("native receiver is not a nominal"));
        };
        let semantic = self
            .identities
            .get(definition)
            .ok_or(Error::Unsupported("native type identity is absent"))?;
        if let Some((export, _)) = self
            .exports()
            .into_iter()
            .find(|(_, target)| target == semantic)
        {
            return Ok(export.clone());
        }
        // A public closed alias may name a private target. Compare
        // normalized semantic types; never guess a private path.
        let mut machine = resolve::Machine::over(self);
        let expected = machine
            .normalize(
                &self.input_type(ty).map_err(failure)?,
                &mut resolve::State::default(),
            )
            .map_err(failure)?;
        for (export, target) in self.exports() {
            let Some(declaration) = self.definitions.get(target) else {
                continue;
            };
            let model::Kind::TypeAlias { generics, .. } = declaration.kind() else {
                continue;
            };
            if generics.parameters().len() != 0 {
                continue;
            }
            let candidate = model::Type::Definition {
                id: target.clone(),
                arguments: Box::new([]),
            };
            if machine
                .normalize(&candidate, &mut resolve::State::default())
                .map_err(failure)?
                == expected
            {
                return Ok(export.clone());
            }
        }
        Err(Error::Unsupported(
            "closed native type has no authenticated public spelling",
        ))
    }
}

fn path(export: &identity::Export) -> Path {
    std::iter::once(export.root_alias())
        .chain(export.path())
        .map(Into::into)
        .collect()
}

fn failure(_: resolve::Failure) -> Error {
    Error::Unsupported("native public spelling could not be normalized")
}
