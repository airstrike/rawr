//! Generation-scoped capability for invoking symbolic items.

use super::{Aspect, Error, Executable, GenerationId};
#[cfg(not(feature = "no_function"))]
use crate::eval::Caches;
use crate::{item, Dynamic, Engine, Shared};
#[cfg(not(feature = "no_function"))]
use crate::{types::nominal, CallFnOptions, Scope};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// The host-only capability that authorizes one immutable executable.
///
/// ```compile_fail
/// use rawr::program::Authority;
/// let _ = Authority {};
/// ```
#[derive(Clone)]
#[cfg_attr(feature = "no_function", allow(dead_code))]
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

    /// Return stable symbolic values declared by this generation.
    pub fn items(&self) -> impl ExactSizeIterator<Item = &item::Value> {
        self.executable.items()
    }

    /// Return every compiler-validated local preview in stable key order.
    pub fn previews(&self) -> &[crate::preview::Definition] {
        self.executable.previews()
    }

    /// Return every compiler-validated scripted-widget protocol.
    pub fn widgets(&self) -> &[crate::widget::Definition] {
        self.executable.widgets()
    }

    /// Return every compiler-selected nominal-widget promotion site.
    pub fn widget_promotions(&self) -> &[crate::widget::Promotion] {
        self.executable.widget_promotions()
    }

    pub(crate) fn preview_source(
        &self,
        location: crate::preview::Location,
    ) -> Result<&crate::diagnostic::Source, super::source::Error> {
        self.executable.preview_source(location)
    }

    pub(crate) fn item_evidence(&self, retained: &item::Value) -> Option<super::Evidence<'_>> {
        let callable = self.verify(retained).ok()?;
        self.executable.callable_evidence(callable).ok()
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
    #[cfg(not(feature = "no_function"))]
    pub fn invoke(&self, retained: &item::Value, args: &mut [Dynamic]) -> Result<Dynamic, Error> {
        self.invoke_inner(retained, args, None, None)
    }

    #[cfg(not(feature = "no_function"))]
    pub(crate) fn invoke_bound(
        &self,
        retained: &item::Value,
        args: &mut [Dynamic],
        runtime: Shared<crate::host::runtime::Registry>,
    ) -> Result<Dynamic, Error> {
        self.invoke_inner(retained, args, Some(runtime), None)
    }

    #[cfg(not(feature = "no_function"))]
    pub(crate) fn invoke_bound_with<T: crate::func::SendSync + 'static>(
        &self,
        retained: &item::Value,
        args: &mut [Dynamic],
        runtime: Shared<crate::host::runtime::Registry>,
        invocation: &crate::host::runtime::invocation::Invocation<T>,
    ) -> Result<Dynamic, Error> {
        self.invoke_inner(retained, args, Some(runtime), Some(invocation.proof()))
    }

    #[cfg(not(feature = "no_function"))]
    pub(crate) fn project_bound(
        &self,
        retained: &item::Value,
        proof: &crate::host::projection::Site,
        value: Dynamic,
        runtime: Shared<crate::host::runtime::Registry>,
    ) -> Result<Dynamic, Error> {
        let mut value = value;
        runtime
            .invoke_projection(proof, &mut [&mut value], self.executable.model())
            .map_err(|error| Error::Evaluation {
                key: retained.key().clone(),
                error,
            })
    }

    #[cfg(not(feature = "no_function"))]
    pub(crate) fn validate_bound_type(
        &self,
        retained: &item::Value,
        ty: &crate::host::runtime::type_ref::Type,
        value: &Dynamic,
        runtime: &crate::host::runtime::Registry,
    ) -> Result<(), Error> {
        ty.validate(value, self.executable.model(), runtime)
            .map_err(|error| Error::Evaluation {
                key: retained.key().clone(),
                error: crate::EvalAltResult::ErrorRuntime(
                    format!(
                        "callback returned `{}`, expected `{ty}`: {error}",
                        value.type_name()
                    )
                    .into(),
                    crate::Position::NONE,
                )
                .into(),
            })
    }

    #[cfg(not(feature = "no_function"))]
    pub(crate) fn validate_witness_argument(
        &self,
        witness: &crate::host::runtime::Witness,
        index: usize,
        value: &Dynamic,
        runtime: &crate::host::runtime::Registry,
    ) -> Result<(), crate::host::runtime::ValueError> {
        let ty = witness.argument(index).ok_or_else(|| {
            crate::host::runtime::ValueError::Unsupported {
                ty: format!("runtime witness argument {index}").into(),
            }
        })?;
        ty.validate(value, self.executable.model(), runtime)
    }

    #[cfg(not(feature = "no_function"))]
    pub(crate) fn issue_operation(
        &self,
        request: crate::host::operation::Request,
        runtime: Shared<crate::host::runtime::Registry>,
    ) -> Result<crate::host::operation::Pending, crate::host::operation::Error> {
        let (proof, arguments, mapper) = request.into_parts();
        let selected = runtime
            .select_operation(&proof)
            .map_err(crate::host::operation::Error::Binding)?;
        if proof.inputs().len() != arguments.len() {
            return Err(crate::host::operation::Error::Arity {
                operation: proof.path().into(),
                expected: proof.inputs().len(),
                actual: arguments.len(),
            });
        }
        for (index, input) in proof.inputs().iter().enumerate() {
            let Some(argument) = arguments.get(index) else {
                return Err(crate::host::operation::Error::Arity {
                    operation: proof.path().into(),
                    expected: proof.inputs().len(),
                    actual: arguments.len(),
                });
            };
            input
                .validate(argument, self.executable.model(), &runtime)
                .map_err(|source| crate::host::operation::Error::Input {
                    operation: proof.path().into(),
                    index,
                    source,
                })?;
        }
        let mapper = self
            .rebind(&mapper)
            .map_err(crate::host::operation::Error::Mapper)?;
        let authority = self.clone();
        let operation: crate::ImmutableString = proof.path().into();
        let output = proof.output().clone();
        Ok(crate::host::operation::Pending::new(async move {
            let output_value = selected.start(arguments).await;
            output
                .validate(&output_value, authority.executable.model(), &runtime)
                .map_err(|source| crate::host::operation::Error::Output { operation, source })?;
            let mut arguments = [output_value];
            let payload = authority
                .invoke_bound(&mapper, &mut arguments, runtime.clone())
                .map_err(crate::host::operation::Error::Mapper)?;
            Ok(payload)
        }))
    }

    #[cfg(not(feature = "no_function"))]
    pub(crate) fn bind_stream(
        &self,
        request: crate::host::stream::Request,
        runtime: Shared<crate::host::runtime::Registry>,
    ) -> Result<crate::host::stream::Recipe, crate::host::stream::Error> {
        let (proof, arguments, mappers, filters, message) = request.into_parts();
        let selected = runtime
            .select_stream(&proof)
            .map_err(crate::host::stream::Error::Binding)?;
        if proof.inputs().len() != arguments.len() {
            return Err(crate::host::stream::Error::Arity {
                operation: proof.path().into(),
                expected: proof.inputs().len(),
                actual: arguments.len(),
            });
        }
        for (index, input) in proof.inputs().iter().enumerate() {
            let Some(argument) = arguments.get(index) else {
                return Err(crate::host::stream::Error::Arity {
                    operation: proof.path().into(),
                    expected: proof.inputs().len(),
                    actual: arguments.len(),
                });
            };
            input
                .validate(argument, self.executable.model(), &runtime)
                .map_err(|source| crate::host::stream::Error::Input {
                    operation: proof.path().into(),
                    index,
                    source,
                })?;
        }
        let mappers = mappers
            .iter()
            .map(|mapper| {
                self.rebind(mapper)
                    .map_err(crate::host::stream::Error::Mapper)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_boxed_slice();
        let contributions = selected.identify(&arguments).map_err(|(index, source)| {
            crate::host::stream::Error::IdentityEncoding {
                operation: proof.path().into(),
                index,
                source,
            }
        })?;
        let identity = proof
            .identity(&contributions, &mappers, &filters)
            .map_err(crate::host::stream::Error::IdentityProof)?;

        let authority = self.clone();
        let operation: crate::ImmutableString = proof.path().into();
        let output = proof.output().clone();
        let issued_identity = identity;
        Ok(crate::host::stream::Recipe::new(
            identity,
            Box::new(move || {
                let stream = selected.start(arguments);
                let runtime = runtime.clone();
                let mut mappers = mappers;
                let mapper_key = mappers.last().map(|mapper| mapper.key().clone());
                crate::host::stream::Issued::new(
                    issued_identity,
                    stream,
                    Box::new(move |value| {
                        output
                            .validate(&value, authority.executable.model(), &runtime)
                            .map_err(|source| crate::host::stream::Error::Output {
                                operation: operation.clone(),
                                source,
                            })?;
                        let mut payload = value;
                        for (mapper, filter) in mappers.iter_mut().zip(filters.iter()) {
                            // A tuple is spread over a closure of as many
                            // parameters: `listen_with(|event, status, window| ..)`.
                            let spread = authority
                                .verify(mapper)
                                .ok()
                                .and_then(|callable| {
                                    callable.arity().checked_sub(mapper.captures().len())
                                })
                                .filter(|arity| *arity > 1 && payload.is_array())
                                .and_then(|arity| {
                                    let tuple = payload.clone().into_array().ok()?;
                                    (tuple.len() == arity).then_some(tuple)
                                });
                            let mut single;
                            let mut spread = spread;
                            let arguments = match &mut spread {
                                Some(values) => values.as_mut_slice(),
                                None => {
                                    single = [payload];
                                    single.as_mut_slice()
                                }
                            };
                            payload = authority
                                .invoke_bound(mapper, arguments, runtime.clone())
                                .map_err(crate::host::stream::Error::Mapper)?;
                            // `filter_map`: `None` drops the item, `Some` carries on.
                            if *filter {
                                match crate::option::take(&mut payload) {
                                    Some(Some(inner)) => payload = inner,
                                    _ => return Ok(None),
                                }
                            }
                        }
                        message
                            .validate(&payload, authority.executable.model(), &runtime)
                            .map_err(|source| crate::host::stream::Error::Message {
                                mapper: mapper_key.clone(),
                                source,
                            })?;
                        Ok(Some(payload))
                    }),
                )
            }),
        ))
    }

    pub(crate) fn runtime_requirements(&self) -> &[crate::host::runtime::Requirement] {
        self.executable.runtime_requirements()
    }

    pub(crate) fn projection_requirements(&self) -> &[crate::host::projection::Site] {
        self.executable.projection_requirements()
    }

    pub(crate) fn widget_requirements(&self) -> &[crate::host::runtime::widget::Requirement] {
        self.executable.widget_requirements()
    }

    pub(crate) fn materialize_default(
        &self,
        ty: &crate::application::Type,
    ) -> Result<Dynamic, crate::program::construction::Error> {
        if let Some(value) = ty.builtin_default() {
            return Ok(value);
        }
        let Some(identity) = ty.identity() else {
            return Err(crate::program::construction::Error::ValueType {
                expected: ty.display_name().into(),
                actual: "non-nominal application State".into(),
            });
        };
        let Some(schema) = ty.schema() else {
            return Err(crate::program::construction::Error::ValueType {
                expected: ty.display_name().into(),
                actual: "application State without a script schema".into(),
            });
        };
        self.executable
            .model()
            .materialize_default(identity, schema)
    }

    /// A carried value rebuilt in this executable's model.
    pub(crate) fn decode_carried(&self, bytes: &[u8]) -> Option<Dynamic> {
        self.executable.model().decode_carried(bytes)
    }

    pub(crate) fn validate_application_value(
        &self,
        value: &Dynamic,
        ty: &crate::application::Type,
        runtime: &crate::host::runtime::Registry,
    ) -> Result<(), crate::host::runtime::ValueError> {
        let exact = ty.exact_runtime_type().ok_or_else(|| {
            crate::host::runtime::ValueError::Unsupported {
                ty: ty.display_name().into(),
            }
        })?;
        exact.validate(value, self.executable.model(), runtime)
    }

    pub(crate) fn promote_widget(
        &self,
        retained: &crate::widget::Promotion,
        data: Dynamic,
        message: &crate::application::Type,
        runtime: &crate::host::runtime::Registry,
    ) -> Result<crate::widget::Descriptor, crate::widget::Error> {
        let promotion = self
            .current_widget_promotion(retained)
            .ok_or(crate::widget::Error::Site)?;
        if promotion.message() != message {
            return Err(crate::widget::Error::message(promotion.message(), message));
        }
        self.executable
            .model()
            .validate_value_exact(
                &data,
                promotion.definition().owner(),
                promotion.definition().schema(),
                runtime,
            )
            .map_err(crate::widget::Error::Data)?;
        Ok(crate::widget::Descriptor::new(promotion.clone(), data))
    }

    pub(crate) fn bind_widget(
        &self,
        descriptor: &crate::widget::Descriptor,
        runtime: Shared<crate::host::runtime::Registry>,
    ) -> Result<crate::widget::Binding, crate::widget::Error> {
        let promotion = self
            .current_widget_promotion(descriptor.promotion())
            .ok_or(crate::widget::Error::Site)?;
        self.executable
            .model()
            .validate_value_exact(
                descriptor.data(),
                promotion.definition().owner(),
                promotion.definition().schema(),
                &runtime,
            )
            .map_err(crate::widget::Error::Data)?;
        Ok(crate::widget::Binding::new(
            self.clone(),
            runtime,
            promotion.clone(),
            descriptor.data().clone(),
        ))
    }

    fn current_widget_promotion(
        &self,
        retained: &crate::widget::Promotion,
    ) -> Option<&crate::widget::Promotion> {
        self.executable
            .widget_promotions()
            .iter()
            .find(|current| current.site() == retained.site() && current.compatible(retained))
    }

    #[cfg(not(feature = "no_function"))]
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

#[cfg(test)]
mod stream_tests;
