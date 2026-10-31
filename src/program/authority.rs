//! Generation-scoped capability for invoking symbolic items.

use super::{Aspect, Error, Executable, GenerationId};
use crate::eval::Caches;
use crate::{item, Dynamic, Engine, Shared};
use crate::{types::nominal, CallFnOptions, Scope};
use std::fmt;

/// The host-only capability that authorizes one immutable executable.
///
/// ```compile_fail
/// use rawr::program::Authority;
/// let _ = Authority {};
/// ```
#[derive(Clone)]
pub struct Authority {
    engine: Shared<Engine>,
    executable: Shared<Executable>,
}

impl fmt::Debug for Authority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Authority")
            .field("generation", &self.executable.generation())
            .field("executable", &self.executable)
            .finish_non_exhaustive()
    }
}

impl Authority {
    pub(crate) fn new(engine: Shared<Engine>, executable: Executable) -> Result<Self, Error> {
        if engine.origin() != executable.origin() {
            return Err(Error::EngineOriginMismatch);
        }

        Ok(Self {
            engine,
            executable: Shared::new(executable),
        })
    }

    /// Return this authority's opaque generation identifier.
    #[must_use]
    pub fn generation(&self) -> GenerationId {
        self.executable.generation()
    }

    pub(crate) fn package(&self) -> &str {
        self.executable.package()
    }

    /// The runtime model of this generation's script types.
    #[cfg(feature = "rust-import")]
    pub(crate) fn model(&self) -> &super::model::Model {
        self.executable.model()
    }

    /// Return stable symbolic values declared by this generation.
    pub fn items(&self) -> impl ExactSizeIterator<Item = &item::Value> {
        self.executable.items()
    }

    /// Rebind a retained symbolic item to this exact generation.
    ///
    /// Every stable proof component is checked before the current canonical
    /// value is returned.
    pub fn rebind(&self, retained: &item::Value) -> Result<item::Value, Error> {
        self.verify(retained)
            .map(|callable| retained.rebound(callable.value()))
    }

    /// Invoke a symbolic item only after rebinding all stable proof metadata.
    ///
    /// Receiver methods take their receiver as the first value in `args`.
    /// Variant construction consumes arguments only after this generation's
    /// runtime model recursively proves their exact ordered shape.
    pub fn invoke(&self, retained: &item::Value, args: &mut [Dynamic]) -> Result<Dynamic, Error> {
        self.invoke_inner(retained, args, None, None)
    }

    pub(crate) fn invoke_bound(
        &self,
        retained: &item::Value,
        args: &mut [Dynamic],
        runtime: Shared<crate::host::runtime::Registry>,
    ) -> Result<Dynamic, Error> {
        self.invoke_inner(retained, args, Some(runtime), None)
    }

    /// Validate `value` against the exact type `witness` proves.
    #[cfg(feature = "rust-import")]
    pub(crate) fn validate_witness(
        &self,
        witness: &crate::host::runtime::Witness,
        value: &Dynamic,
        runtime: &crate::host::runtime::Registry,
    ) -> Result<(), crate::host::runtime::ValueError> {
        witness
            .ty()
            .validate(value, self.executable.model(), runtime)
    }

    pub(crate) fn runtime_requirements(&self) -> &[crate::host::runtime::Requirement] {
        self.executable.runtime_requirements()
    }

    fn invoke_inner(
        &self,
        retained: &item::Value,
        args: &mut [Dynamic],
        runtime: Option<Shared<crate::host::runtime::Registry>>,
        invocation: Option<crate::host::runtime::invocation::Proof>,
    ) -> Result<Dynamic, Error> {
        let callable = self.verify(retained)?;
        // A closure's captures come before its own arguments.
        let mut captured;
        let args = if retained.captures().is_empty() {
            args
        } else {
            captured = retained.captures().to_vec();
            captured.extend(args.iter_mut().map(Dynamic::take));
            captured.as_mut_slice()
        };
        if callable.arity() != args.len() {
            return Err(Error::InvalidArity {
                key: retained.key().clone(),
                expected: callable.arity(),
                actual: args.len(),
            });
        }

        // A free function's `this` parameter moves first, as a receiver,
        // and back once the call has changed it.
        let rotated = match callable.dispatch() {
            super::executable::Dispatch::Function(_) => {
                callable.this().filter(|index| *index < args.len())
            }
            _ => None,
        };
        if let Some(index) = rotated {
            args[..=index].rotate_right(1);
        }
        let (name, this_ptr, arguments) = match callable.dispatch() {
            super::executable::Dispatch::Function(name) if rotated.is_some() => {
                let (receiver, parameters) = args
                    .split_first_mut()
                    .expect("a rotated receiver is within the arguments");
                (name.as_str(), Some(receiver), parameters)
            }
            super::executable::Dispatch::Function(name) => (name.as_str(), None, &mut *args),
            super::executable::Dispatch::Method(name) => {
                if retained.receiver().is_some() {
                    let (receiver, parameters) = args
                        .split_first_mut()
                        .expect("authorized receiver arity includes one receiver value");
                    (name.as_str(), Some(receiver), parameters)
                } else {
                    (name.as_str(), None, &mut *args)
                }
            }
            super::executable::Dispatch::Variant {
                identity,
                schema,
                variant,
            } => {
                let members = args.iter_mut().map(Dynamic::take).collect();
                let proof = self
                    .executable
                    .model()
                    .prove_enum(identity, schema, variant, members)
                    .map_err(|error| Error::Construction {
                        key: retained.key().clone(),
                        error,
                    })?;
                return nominal::Enum::construct(
                    self.executable.model(),
                    identity,
                    schema,
                    variant,
                    proof,
                )
                .map(Into::into)
                .map_err(|error| Error::Construction {
                    key: retained.key().clone(),
                    error,
                });
            }
        };

        let options = CallFnOptions::new().eval_ast(false);
        let options = match this_ptr {
            Some(this_ptr) => options.bind_this_ptr(this_ptr),
            None => options,
        };
        let mut scope = Scope::new();
        let mut global = self.engine.new_global_runtime_state();
        global.host_runtime = runtime;
        global.host_invocation = invocation;
        let result = self
            .engine
            ._call_fn(
                options,
                &mut scope,
                self.executable.ast(),
                name,
                arguments,
                &mut global,
                &mut Caches::new(),
            )
            .map_err(|error| Error::Evaluation {
                key: retained.key().clone(),
                error,
            });
        if let Some(index) = rotated {
            args[..=index].rotate_left(1);
        }
        result
    }

    fn verify(&self, retained: &item::Value) -> Result<&super::executable::Callable, Error> {
        let Some(callable) = self.executable.callable(retained.key()) else {
            if let Some(related) = self.executable.related_callable(retained.key()) {
                let current = related.value();
                let aspect = if current.kind() != retained.kind() {
                    Aspect::Kind
                } else if current.owner() != retained.owner() {
                    Aspect::Owner
                } else {
                    return Err(Error::MissingItem {
                        key: retained.key().clone(),
                    });
                };
                return Err(Error::Mismatch {
                    key: retained.key().clone(),
                    aspect,
                });
            }
            return Err(Error::MissingItem {
                key: retained.key().clone(),
            });
        };
        let current = callable.value();
        let aspect = if current.kind() != retained.kind() {
            Some(Aspect::Kind)
        } else if current.owner() != retained.owner() {
            Some(Aspect::Owner)
        } else if current.receiver() != retained.receiver() {
            Some(Aspect::Receiver)
        } else if current.signature() != retained.signature() {
            Some(Aspect::Signature)
        } else if current.capabilities() != retained.capabilities() {
            Some(Aspect::Capabilities)
        } else {
            None
        };
        if let Some(aspect) = aspect {
            Err(Error::Mismatch {
                key: retained.key().clone(),
                aspect,
            })
        } else {
            Ok(callable)
        }
    }
}
