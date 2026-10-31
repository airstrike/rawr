//! Proof-to-application transformation, independent of native compiler feedback.

use sha2::{Digest as _, Sha256};

use super::{call, resolve, scope, Catalog};
use crate::api::{identity, ledger, model};
use crate::classify::representability::{convert, ownership, signature};
use crate::surface::application::{Application, Expression, Invocation, Path, Type};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("local application is unsupported: {0}")]
    Unsupported(&'static str),
}

impl Catalog<'_> {
    /// Classify an established call without consulting any compiler or expected
    /// return type. Public render paths remain local to this application.
    pub fn apply(&self, proof: call::Proof) -> Result<Application, Error> {
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
        if !matches!(
            declared.receiver(),
            model::Receiver::Reference {
                mutability: model::Mutability::Shared,
                ..
            }
        ) {
            return Err(Error::Unsupported(
                "the first native application transport requires a shared receiver",
            ));
        }
        let request = proof.request();
        if request.mutable {
            return Err(Error::Unsupported(
                "mutable source receiver transport is not yet proved",
            ));
        }
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
            receiver,
            ownership: ownership::Receiver::Shared,
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

    fn native_type(&self, ty: &convert::Type) -> Result<Type, Error> {
        Ok(match ty {
            convert::Type::Unit => Type::Tuple(Box::new([])),
            convert::Type::Builtin(builtin) => Type::Primitive(
                builtin
                    .rust_name()
                    .filter(|name| matches!(*name, "i64" | "bool" | "f64" | "char"))
                    .ok_or(Error::Unsupported(
                        "native scalar spelling is outside the closed rules",
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
            convert::Type::Nominal {
                definition,
                arguments,
            } => {
                let semantic = self
                    .identities
                    .get(definition)
                    .ok_or(Error::Unsupported("native type identity is absent"))?;
                let mut exports = self
                    .graphs
                    .iter()
                    .flat_map(|graph| graph.exports())
                    .filter_map(|path| match path {
                        ledger::Path::Imported(export)
                            if export.target().namespace() == identity::Namespace::Type =>
                        {
                            Some(export)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                exports.sort_unstable_by(|a, b| a.key().cmp(b.key()));
                if let Some(export) = exports.iter().find(|export| export.target() == *semantic) {
                    Type::Export {
                        path: path(export.key()),
                        arguments: arguments
                            .iter()
                            .map(|ty| self.native_type(ty))
                            .collect::<Result<Box<[_]>, _>>()?,
                    }
                } else {
                    // A public closed alias may name a private target. Compare
                    // normalized semantic types; never guess a private path.
                    let mut machine = resolve::Machine::over(self);
                    let expected = machine
                        .normalize(
                            &self.input_type(ty).map_err(failure)?,
                            &mut resolve::State::default(),
                        )
                        .map_err(failure)?;
                    let mut alias = None;
                    for export in exports {
                        let Some(declaration) = self.definitions.get(export.target()) else {
                            continue;
                        };
                        let model::Kind::TypeAlias { generics, .. } = declaration.kind() else {
                            continue;
                        };
                        if generics.parameters().len() != 0 {
                            continue;
                        }
                        let candidate = model::Type::Definition {
                            id: export.target().clone(),
                            arguments: Box::new([]),
                        };
                        if machine
                            .normalize(&candidate, &mut resolve::State::default())
                            .map_err(failure)?
                            == expected
                        {
                            alias = Some(path(export.key()));
                            break;
                        }
                    }
                    Type::Export {
                        path: alias.ok_or(Error::Unsupported(
                            "closed native type has no authenticated public spelling",
                        ))?,
                        arguments: Box::new([]),
                    }
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

fn path(export: &identity::Export) -> Path {
    std::iter::once(export.root_alias())
        .chain(export.path())
        .map(Into::into)
        .collect()
}

fn failure(_: resolve::Failure) -> Error {
    Error::Unsupported("native public spelling could not be normalized")
}
