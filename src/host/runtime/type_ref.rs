//! Exact runtime type proofs retained by compiler-private operations.

use crate::{host, program, type_def, Dynamic, ImmutableString};
use std::any::TypeId;
use std::collections::BTreeSet;
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

const WITNESS_PREFIX: &str = "\0rawr.witness.";

/// The runtime representation of a generic parameter, which no value has.
struct WitnessParameter;

/// One sealed, fully instantiated runtime type proof.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum Type {
    Unit,
    /// `[T; N]`: an array of exactly `length` elements; a slice `&[T]`
    /// without one.
    Array {
        element: Box<Type>,
        length: Option<usize>,
    },
    /// Rust's `Option<T>`: at runtime an `Option<Dynamic>`.
    Option(Box<Type>),
    /// Rust's `Result<T, E>`: at runtime a `Result<Dynamic, Dynamic>`.
    Result(Box<Type>, Box<Type>),
    /// A tuple `(A, B, ..)`: at runtime an array of exactly its members.
    Tuple(Box<[Type]>),
    /// A symbolic item taking `inputs` in order.
    Callable {
        inputs: Box<[Type]>,
        output: Box<Type>,
        item: crate::item::Value,
    },
    Builtin {
        name: ImmutableString,
        runtime: Representation,
    },
    /// `dyn Trait`: a value of any crate type, which the checker proved
    /// implements the trait where it was coerced.
    Object(ImmutableString),
    Script {
        identity: type_def::Identity,
        schema: program::schema::Fingerprint,
        kind: type_def::Kind,
        runtime: Representation,
    },
    Host {
        path: ImmutableString,
        identity: TypeId,
        arguments: Box<[Type]>,
        owner: Option<host::facade::Dependency>,
    },
    #[cfg(feature = "rust-import")]
    Imported {
        path: ImmutableString,
        identity: host::type_def::import::Identity,
        arguments: Box<[Type]>,
        owner: host::facade::Dependency,
    },
    Support {
        path: ImmutableString,
        identity: TypeId,
        owner: host::facade::Dependency,
        contract: host::support::SignatureContract,
    },
}

/// One exact local runtime witness with an optional private semantic marker.
///
/// The marker changes hashing only on the cloned, non-authoritative application
/// semantic proof. The exact witness remains available to the original runtime
/// proof and cannot be reconstructed from the marker.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Representation(RepresentationValue);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum RepresentationValue {
    Exact(TypeId),
    #[cfg(feature = "rust-import")]
    Semantic(SemanticRepresentation),
}

#[cfg(feature = "rust-import")]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum SemanticRepresentation {
    Builtin(BuiltinRepresentation),
    Script(ScriptRepresentation),
}

#[cfg(feature = "rust-import")]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum BuiltinRepresentation {
    Bool,
    Char,
    I64,
    F64,
    String,
}

#[cfg(feature = "rust-import")]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum ScriptRepresentation {
    Struct,
    Enum,
}

impl Representation {
    pub(crate) const fn exact(exact: TypeId) -> Self {
        Self(RepresentationValue::Exact(exact))
    }

    pub(crate) fn runtime(self) -> TypeId {
        match self.0 {
            RepresentationValue::Exact(exact) => exact,
            #[cfg(feature = "rust-import")]
            RepresentationValue::Semantic(_) => {
                unreachable!("semantic runtime representation has no execution authority")
            }
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn project_builtin(&mut self, name: &str) -> bool {
        let RepresentationValue::Exact(exact) = self.0 else {
            return false;
        };
        let expected = match name {
            "bool" => (TypeId::of::<bool>(), BuiltinRepresentation::Bool),
            "char" => (TypeId::of::<char>(), BuiltinRepresentation::Char),
            "i64" => (TypeId::of::<crate::INT>(), BuiltinRepresentation::I64),
            #[cfg(not(feature = "no_float"))]
            "f64" => (TypeId::of::<crate::FLOAT>(), BuiltinRepresentation::F64),
            #[cfg(feature = "no_float")]
            "f64" => (TypeId::of::<crate::Dynamic>(), BuiltinRepresentation::F64),
            "String" => (
                TypeId::of::<crate::ImmutableString>(),
                BuiltinRepresentation::String,
            ),
            _ => return false,
        };
        if exact != expected.0 {
            return false;
        }
        self.0 = RepresentationValue::Semantic(SemanticRepresentation::Builtin(expected.1));
        true
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn project_script(&mut self, kind: type_def::Kind) -> bool {
        let RepresentationValue::Exact(exact) = self.0 else {
            return false;
        };
        let (expected, semantic) = match kind {
            type_def::Kind::Struct => (
                TypeId::of::<crate::types::nominal::Struct>(),
                ScriptRepresentation::Struct,
            ),
            type_def::Kind::Enum => (
                TypeId::of::<crate::types::nominal::Enum>(),
                ScriptRepresentation::Enum,
            ),
        };
        if exact != expected {
            return false;
        }
        self.0 = RepresentationValue::Semantic(SemanticRepresentation::Script(semantic));
        true
    }
}

/// An exact runtime value did not satisfy its sealed type proof.
#[derive(Debug)]
pub enum Error {
    /// The outer Rust representation differs from the exact runtime proof.
    Representation {
        /// Required logical runtime type.
        expected: ImmutableString,
        /// Actual Rust runtime type.
        actual: ImmutableString,
    },
    /// A symbolic callable does not retain the exact proven script item.
    SymbolicCallable {
        /// Required stable callable identity.
        expected: ImmutableString,
    },
    /// Recursive validation of a script nominal value failed.
    Nominal(program::construction::Error),
    /// A nested nominal member failed exact validation.
    Member {
        /// Nominal value containing the member.
        target: type_def::Identity,
        /// Zero-based member position.
        index: usize,
        /// Exact nested failure.
        source: Box<Error>,
    },
    /// No facade-owned attestation is available for a generic host value.
    MissingAttestation {
        /// Required generic host type.
        ty: ImmutableString,
    },
    /// A facade-owned attestor rejected the exact generic host value.
    AttestationMismatch {
        /// Rejected generic host type.
        ty: ImmutableString,
    },
    /// The structural type has no exact runtime representation.
    Unsupported {
        /// Unsupported structural type.
        ty: ImmutableString,
    },
}

impl Type {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        fn types(values: &mut [Type], profile: &host::facade::ProfileFinalization<'_>) -> usize {
            values.iter_mut().fold(0_usize, |completed, value| {
                completed.saturating_add(value.normalize_profile(profile))
            })
        }

        match self {
            Self::Unit | Self::Object(_) => 0,
            Self::Array { element, .. } | Self::Option(element) => {
                element.normalize_profile(profile)
            }
            Self::Result(ok, err) => ok
                .normalize_profile(profile)
                .saturating_add(err.normalize_profile(profile)),
            Self::Tuple(members) => types(members, profile),
            Self::Builtin { name, runtime } => {
                if runtime.project_builtin(name) {
                    1
                } else {
                    profile.reject_runtime_representation();
                    0
                }
            }
            Self::Script {
                schema,
                kind,
                runtime,
                ..
            } => {
                let completed = schema.normalize_profile(profile);
                if runtime.project_script(*kind) {
                    completed.saturating_add(1)
                } else {
                    profile.reject_runtime_representation();
                    completed
                }
            }
            Self::Callable { inputs, output, .. } => {
                types(inputs, profile).saturating_add(output.normalize_profile(profile))
            }
            Self::Host {
                path,
                identity,
                arguments,
                owner,
            } => {
                let mut completed = types(arguments, profile);
                let direct = owner
                    .as_mut()
                    .map(|owner| {
                        let (direct, nested) = profile.normalize_dependency(owner);
                        completed = completed.saturating_add(nested);
                        direct
                    })
                    .unwrap_or(false);
                let logical = direct
                    .then(|| profile.normalize_runtime_identity(path, *identity))
                    .flatten();
                if let (Some(logical), Some(owner)) = (logical, owner.take()) {
                    *self = Self::Imported {
                        path: std::mem::take(path),
                        identity: logical,
                        arguments: std::mem::take(arguments),
                        owner,
                    };
                    completed = completed.saturating_add(1);
                }
                completed
            }
            #[cfg(feature = "rust-import")]
            Self::Imported {
                arguments, owner, ..
            } => types(arguments, profile).saturating_add(profile.normalize_dependency(owner).1),
            Self::Support {
                path,
                identity,
                owner,
                ..
            } => {
                let (direct, nested) = profile.normalize_dependency(owner);
                let logical = direct
                    .then(|| profile.normalize_runtime_identity(path, *identity))
                    .flatten();
                if let Some(logical) = logical {
                    let path = std::mem::take(path);
                    let owner = owner.clone();
                    *self = Self::Imported {
                        path,
                        identity: logical,
                        arguments: Box::default(),
                        owner,
                    };
                    nested.saturating_add(1)
                } else {
                    nested
                }
            }
        }
    }

    pub(crate) const fn unit() -> Self {
        Self::Unit
    }

    /// The generic parameter at `index` of a proof that a witnessed body
    /// instantiates at run time.
    pub(crate) fn witness_parameter(index: usize) -> Self {
        Self::builtin(
            format!("{WITNESS_PREFIX}{index}"),
            TypeId::of::<WitnessParameter>(),
        )
    }

    /// The index of a generic parameter `witness_parameter` made.
    pub(crate) fn witness_index(&self) -> Option<usize> {
        match self {
            Self::Builtin { name, .. } => name.strip_prefix(WITNESS_PREFIX)?.parse().ok(),
            _ => None,
        }
    }

    /// This type with each generic parameter replaced by its argument.
    pub(crate) fn instantiate(&self, arguments: &[Self]) -> Self {
        let each = |types: &[Self]| -> Box<[Self]> {
            types.iter().map(|ty| ty.instantiate(arguments)).collect()
        };
        match self {
            Self::Builtin { .. } => match self.witness_index() {
                Some(index) => arguments.get(index).cloned().unwrap_or(Self::Unit),
                None => self.clone(),
            },
            Self::Array { element, length } => Self::Array {
                element: Box::new(element.instantiate(arguments)),
                length: *length,
            },
            Self::Option(element) => Self::Option(Box::new(element.instantiate(arguments))),
            Self::Result(ok, err) => Self::Result(
                Box::new(ok.instantiate(arguments)),
                Box::new(err.instantiate(arguments)),
            ),
            Self::Tuple(members) => Self::Tuple(each(members)),
            Self::Callable {
                inputs,
                output,
                item,
            } => Self::Callable {
                inputs: each(inputs),
                output: Box::new(output.instantiate(arguments)),
                item: item.clone(),
            },
            Self::Host {
                path,
                identity,
                arguments: own,
                owner,
            } => Self::Host {
                path: path.clone(),
                identity: *identity,
                arguments: each(own),
                owner: owner.clone(),
            },
            #[cfg(feature = "rust-import")]
            Self::Imported {
                path,
                identity,
                arguments: own,
                owner,
            } => Self::Imported {
                path: path.clone(),
                identity: *identity,
                arguments: each(own),
                owner: owner.clone(),
            },
            Self::Unit | Self::Object(_) | Self::Script { .. } | Self::Support { .. } => {
                self.clone()
            }
        }
    }

    pub(crate) fn array(element: Self, length: Option<usize>) -> Self {
        Self::Array {
            element: Box::new(element),
            length,
        }
    }

    pub(crate) fn option(element: Self) -> Self {
        Self::Option(Box::new(element))
    }

    pub(crate) fn tuple(members: Box<[Self]>) -> Self {
        Self::Tuple(members)
    }

    pub(crate) fn result(ok: Self, err: Self) -> Self {
        Self::Result(Box::new(ok), Box::new(err))
    }

    pub(crate) fn callable(inputs: Box<[Self]>, output: Self, item: crate::item::Value) -> Self {
        Self::Callable {
            inputs,
            output: Box::new(output),
            item,
        }
    }

    pub(crate) fn builtin(name: impl Into<ImmutableString>, runtime: TypeId) -> Self {
        Self::Builtin {
            name: name.into(),
            runtime: Representation::exact(runtime),
        }
    }

    #[cfg(not(feature = "no_float"))]
    pub(crate) fn f64_range_inclusive() -> Self {
        Self::builtin(
            "RangeInclusive<f64>",
            TypeId::of::<std::ops::RangeInclusive<crate::FLOAT>>(),
        )
    }

    pub(crate) fn i64_range_inclusive() -> Self {
        Self::builtin(
            "RangeInclusive<i64>",
            TypeId::of::<std::ops::RangeInclusive<crate::INT>>(),
        )
    }

    pub(crate) fn script(
        identity: type_def::Identity,
        schema: program::schema::Fingerprint,
        kind: type_def::Kind,
        runtime: TypeId,
    ) -> Self {
        Self::Script {
            identity,
            schema,
            kind,
            runtime: Representation::exact(runtime),
        }
    }

    pub(crate) fn host(
        path: ImmutableString,
        identity: TypeId,
        arguments: Box<[Self]>,
        owner: Option<host::facade::Dependency>,
    ) -> Self {
        Self::Host {
            path,
            identity,
            arguments,
            owner,
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported(
        path: ImmutableString,
        identity: host::type_def::import::Identity,
        arguments: Box<[Self]>,
        owner: host::facade::Dependency,
    ) -> Self {
        Self::Imported {
            path,
            identity,
            arguments,
            owner,
        }
    }

    pub(crate) fn support(
        path: ImmutableString,
        identity: TypeId,
        owner: host::facade::Dependency,
        contract: host::support::SignatureContract,
    ) -> Self {
        Self::Support {
            path,
            identity,
            owner,
            contract,
        }
    }

    pub(crate) fn runtime(&self) -> TypeId {
        match self {
            Self::Unit => TypeId::of::<()>(),
            Self::Object(_) => TypeId::of::<Dynamic>(),
            Self::Array { .. } | Self::Tuple(_) => TypeId::of::<crate::Array>(),
            Self::Option(_) => TypeId::of::<crate::option::Value>(),
            Self::Result(..) => TypeId::of::<crate::result::Value>(),
            Self::Callable { .. } => TypeId::of::<crate::item::Value>(),
            Self::Builtin { runtime, .. } | Self::Script { runtime, .. } => runtime.runtime(),
            Self::Host {
                identity: runtime, ..
            }
            | Self::Support {
                identity: runtime, ..
            } => *runtime,
            #[cfg(feature = "rust-import")]
            Self::Imported { .. } => TypeId::of::<Dynamic>(),
        }
    }

    pub(crate) const fn arguments(&self) -> &[Self] {
        match self {
            Self::Host { arguments, .. } => arguments,
            #[cfg(feature = "rust-import")]
            Self::Imported { arguments, .. } => arguments,
            Self::Unit
            | Self::Array { .. }
            | Self::Option(_)
            | Self::Result(..)
            | Self::Tuple(_)
            | Self::Callable { .. }
            | Self::Builtin { .. }
            | Self::Object(_)
            | Self::Script { .. }
            | Self::Support { .. } => &[],
        }
    }

    pub(crate) fn host_attestation(
        &self,
    ) -> Option<(&str, &[Self], Option<&host::facade::Dependency>, bool)> {
        match self {
            Self::Host {
                path,
                arguments,
                owner,
                ..
            } => Some((path, arguments, owner.as_ref(), !arguments.is_empty())),
            #[cfg(feature = "rust-import")]
            Self::Imported {
                path,
                arguments,
                owner,
                ..
            } => Some((path, arguments, Some(owner), true)),
            Self::Unit
            | Self::Array { .. }
            | Self::Option(_)
            | Self::Result(..)
            | Self::Tuple(_)
            | Self::Callable { .. }
            | Self::Builtin { .. }
            | Self::Object(_)
            | Self::Script { .. }
            | Self::Support { .. } => None,
        }
    }

    pub(super) fn matches_export(&self, export: &host::type_def::Export) -> bool {
        let Self::Host {
            path,
            identity,
            arguments,
            owner: Some(owner),
        } = self
        else {
            return false;
        };

        path.as_str() == export.path()
            && Some(*identity) == export.contract().runtime_identity()
            && arguments.is_empty()
            && owner == &export.dependency()
    }

    pub(super) fn instantiates_export(&self, export: &host::type_def::Export) -> bool {
        let Self::Host {
            path,
            identity,
            arguments,
            owner: Some(owner),
        } = self
        else {
            return false;
        };

        path.as_str() == export.path()
            && Some(*identity) == export.contract().runtime_identity()
            && arguments.len() == export.parameters().len()
            && owner == &export.dependency()
    }

    pub(crate) fn validate(
        &self,
        value: &Dynamic,
        model: &program::model::Model,
        registry: &host::runtime::Registry,
    ) -> Result<(), Error> {
        match self {
            Self::Object(_) => {
                if value.read_lock::<crate::types::nominal::Struct>().is_some()
                    || value.read_lock::<crate::types::nominal::Enum>().is_some()
                {
                    Ok(())
                } else {
                    Err(Error::Representation {
                        expected: self.to_string().into(),
                        actual: value.type_name().into(),
                    })
                }
            }
            Self::Array { element, length } => {
                let elements =
                    value
                        .read_lock::<crate::Array>()
                        .ok_or_else(|| Error::Representation {
                            expected: self.to_string().into(),
                            actual: value.type_name().into(),
                        })?;
                if length.is_some_and(|length| elements.len() != length) {
                    return Err(Error::Representation {
                        expected: self.to_string().into(),
                        actual: format!("[_; {}]", elements.len()).into(),
                    });
                }
                elements
                    .iter()
                    .try_for_each(|value| element.validate(value, model, registry))
            }
            Self::Tuple(members) => {
                let elements =
                    value
                        .read_lock::<crate::Array>()
                        .ok_or_else(|| Error::Representation {
                            expected: self.to_string().into(),
                            actual: value.type_name().into(),
                        })?;
                if elements.len() != members.len() {
                    return Err(Error::Representation {
                        expected: self.to_string().into(),
                        actual: format!("a tuple of {}", elements.len()).into(),
                    });
                }
                elements
                    .iter()
                    .zip(members.iter())
                    .try_for_each(|(value, member)| member.validate(value, model, registry))
            }
            Self::Option(element) => match crate::option::read(value) {
                Some(Some(inner)) => element.validate(&inner, model, registry),
                Some(None) => Ok(()),
                None => Err(Error::Representation {
                    expected: self.to_string().into(),
                    actual: value.type_name().into(),
                }),
            },
            Self::Result(ok, err) => match crate::result::read(value) {
                Some(Ok(inner)) => ok.validate(&inner, model, registry),
                Some(Err(inner)) => err.validate(&inner, model, registry),
                None => Err(Error::Representation {
                    expected: self.to_string().into(),
                    actual: value.type_name().into(),
                }),
            },
            Self::Callable { item, .. } => {
                if value.type_id() != TypeId::of::<crate::item::Value>() {
                    return Err(Error::Representation {
                        expected: self.to_string().into(),
                        actual: value.type_name().into(),
                    });
                }
                let retained = value.read_lock::<crate::item::Value>().ok_or_else(|| {
                    Error::Representation {
                        expected: self.to_string().into(),
                        actual: value.type_name().into(),
                    }
                })?;
                if retained.same_item(item) {
                    Ok(())
                } else {
                    Err(Error::SymbolicCallable {
                        expected: item.key().to_string().into(),
                    })
                }
            }
            Self::Script {
                identity, schema, ..
            } => model.validate_value_exact(value, identity, schema, registry),
            Self::Host {
                path,
                identity,
                arguments,
                owner,
            } => {
                if value.type_id() != *identity {
                    return Err(Error::Representation {
                        expected: self.to_string().into(),
                        actual: value.type_name().into(),
                    });
                }
                if arguments.is_empty() {
                    return Ok(());
                }
                let owner = owner
                    .as_ref()
                    .ok_or_else(|| Error::MissingAttestation { ty: path.clone() })?;
                registry.attest(owner, path, value, self, model)
            }
            #[cfg(feature = "rust-import")]
            Self::Imported { path, owner, .. } => registry.attest(owner, path, value, self, model),
            Self::Support { path, identity, .. } => {
                if value.type_id() == *identity {
                    Ok(())
                } else {
                    Err(Error::Representation {
                        expected: path.clone(),
                        actual: value.type_name().into(),
                    })
                }
            }
            Self::Unit | Self::Builtin { .. } => {
                if value.type_id() == self.runtime() {
                    Ok(())
                } else {
                    Err(Error::Representation {
                        expected: self.to_string().into(),
                        actual: value.type_name().into(),
                    })
                }
            }
        }
    }

    pub(crate) fn collect_dependencies(
        &self,
        dependencies: &mut BTreeSet<host::facade::Dependency>,
    ) {
        match self {
            Self::Callable { inputs, output, .. } => {
                for input in inputs {
                    input.collect_dependencies(dependencies);
                }
                output.collect_dependencies(dependencies);
            }
            Self::Host {
                arguments, owner, ..
            } => {
                dependencies.extend(owner.iter().cloned());
                for argument in arguments {
                    argument.collect_dependencies(dependencies);
                }
            }
            #[cfg(feature = "rust-import")]
            Self::Imported {
                arguments, owner, ..
            } => {
                dependencies.insert(owner.clone());
                for argument in arguments {
                    argument.collect_dependencies(dependencies);
                }
            }
            Self::Support { owner, .. } => {
                dependencies.insert(owner.clone());
            }
            Self::Array { element, .. } | Self::Option(element) => {
                element.collect_dependencies(dependencies);
            }
            Self::Result(ok, err) => {
                ok.collect_dependencies(dependencies);
                err.collect_dependencies(dependencies);
            }
            Self::Tuple(members) => {
                for member in members {
                    member.collect_dependencies(dependencies);
                }
            }
            Self::Unit | Self::Builtin { .. } | Self::Object(_) | Self::Script { .. } => {}
        }
    }

    pub(crate) fn from_resolved(
        ty: &crate::resolve::type_ref::Type,
        model: &program::model::Model,
    ) -> Result<Self, Error> {
        use crate::resolve::type_ref::{Argument, Target};

        let crate::resolve::type_ref::Type::Path(path) = ty else {
            return match ty {
                crate::resolve::type_ref::Type::Unit(_) => Ok(Self::unit()),
                crate::resolve::type_ref::Type::Tuple { members, .. } => Ok(Self::Tuple(
                    members
                        .iter()
                        .map(|member| Self::from_resolved(member, model))
                        .collect::<Result<_, _>>()?,
                )),
                // A shared reference is its referent's value at runtime.
                crate::resolve::type_ref::Type::Reference {
                    mutable: false,
                    target,
                    ..
                } => Self::from_resolved(target, model),
                crate::resolve::type_ref::Type::Reference { .. } => Err(Error::Unsupported {
                    ty: "reference".into(),
                }),
                crate::resolve::type_ref::Type::Path(_) => unreachable!(),
            };
        };

        if let Some(element) = ty.option_element() {
            return Ok(Self::option(Self::from_resolved(element, model)?));
        }
        if let Some((ok, err)) = ty.result_elements() {
            return Ok(Self::result(
                Self::from_resolved(ok, model)?,
                Self::from_resolved(err, model)?,
            ));
        }
        // A `Vec` is an array, a `Box` its value, and a map or set an array
        // sorted by key: `(key, value)` pairs or keys.
        if let Some(element) = ty.vec_element() {
            return Ok(Self::array(Self::from_resolved(element, model)?, None));
        }
        if let Some((element, length)) = ty.array_element() {
            return Ok(Self::array(Self::from_resolved(element, model)?, Some(length)));
        }
        if let Some(element) = ty.box_element() {
            return Self::from_resolved(element, model);
        }
        if ty.dyn_trait().is_some() {
            return Ok(Self::Object(path.name.clone()));
        }
        if let Some((_, arguments)) = ty.keyed_arguments() {
            let mut arguments = arguments
                .into_iter()
                .map(|argument| Self::from_resolved(argument, model))
                .collect::<Result<Vec<_>, _>>()?;
            let element = if arguments.len() == 1 {
                arguments.remove(0)
            } else {
                Self::Tuple(arguments.into_boxed_slice())
            };
            return Ok(Self::array(element, None));
        }
        match &path.target {
            Target::Builtin => match crate::host::runtime_builtin(&path.name) {
                "bool" => Ok(Self::builtin("bool", TypeId::of::<bool>())),
                "char" => Ok(Self::builtin("char", TypeId::of::<char>())),
                "i64" => Ok(Self::builtin("i64", TypeId::of::<crate::INT>())),
                "String" => Ok(Self::builtin(
                    "String",
                    TypeId::of::<crate::ImmutableString>(),
                )),
                #[cfg(not(feature = "no_float"))]
                "f64" => Ok(Self::builtin("f64", TypeId::of::<crate::FLOAT>())),
                name => Err(Error::Unsupported { ty: name.into() }),
            },
            Target::Parameter | Target::Projection(..) | Target::Alias(_) => Err(Error::Unsupported {
                ty: path.name.clone(),
            }),
            Target::Script { identity, .. } => {
                let schema = model.schema(identity).ok_or_else(|| {
                    Error::Nominal(program::construction::Error::UnknownTarget {
                        target: identity.clone(),
                    })
                })?;
                Ok(Self::script(
                    identity.clone(),
                    schema.clone(),
                    model.kind(identity).ok_or_else(|| {
                        Error::Nominal(program::construction::Error::UnknownTarget {
                            target: identity.clone(),
                        })
                    })?,
                    model.runtime(identity).ok_or_else(|| {
                        Error::Nominal(program::construction::Error::UnknownTarget {
                            target: identity.clone(),
                        })
                    })?,
                ))
            }
            Target::Host { definition, owner } => {
                let arguments = path
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        Argument::Type(ty) => Some(Self::from_resolved(ty, model)),
                        Argument::Lifetime(_) => None,
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice();
                if let Some(identity) = definition.native_identity() {
                    Ok(Self::host(
                        definition.path().into(),
                        identity,
                        arguments,
                        owner.clone(),
                    ))
                } else {
                    #[cfg(feature = "rust-import")]
                    {
                        let identity =
                            definition
                                .identity()
                                .imported()
                                .ok_or_else(|| Error::Unsupported {
                                    ty: definition.path().into(),
                                })?;
                        let owner = owner.clone().ok_or_else(|| Error::Unsupported {
                            ty: definition.path().into(),
                        })?;
                        Ok(Self::imported(
                            definition.path().into(),
                            *identity,
                            arguments,
                            owner,
                        ))
                    }
                    #[cfg(not(feature = "rust-import"))]
                    {
                        Err(Error::Unsupported {
                            ty: definition.path().into(),
                        })
                    }
                }
            }
        }
    }
}

impl crate::preview::digest::Write for Type {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        use crate::preview::digest::{write_option, write_script_identity, write_slice};

        match self {
            Self::Unit => writer.tag(0x2101),
            Self::Object(name) => {
                writer.tag(0x210e);
                writer.text(name);
            }
            Self::Array { element, length } => {
                writer.tag(0x210a);
                element.write(writer)?;
                writer.text(&length.map_or_else(|| "_".into(), |length| length.to_string()));
            }
            Self::Option(element) => {
                writer.tag(0x210b);
                element.write(writer)?;
            }
            Self::Result(ok, err) => {
                writer.tag(0x210c);
                ok.write(writer)?;
                err.write(writer)?;
            }
            Self::Tuple(members) => {
                writer.tag(0x210d);
                write_slice(writer, members)?;
            }
            Self::Callable {
                inputs,
                output,
                item,
            } => {
                writer.tag(0x2102);
                writer.tag(0x2111);
                for input in inputs {
                    input.write(writer)?;
                }
                writer.tag(0x2112);
                output.write(writer)?;
                writer.tag(0x2113);
                item.write(writer)?;
            }
            Self::Builtin { name, runtime: _ } => {
                writer.tag(0x2103);
                writer.text(name);
            }
            Self::Script {
                identity,
                schema,
                kind: _,
                runtime: _,
            } => {
                writer.tag(0x2104);
                writer.tag(0x2121);
                write_script_identity(writer, identity, "preview runtime script identity")?;
                writer.tag(0x2122);
                schema.write(writer)?;
            }
            Self::Host {
                path,
                identity: _,
                arguments,
                owner,
            } => {
                writer.tag(0x2105);
                writer.tag(0x2131);
                writer.text(path);
                writer.tag(0x2132);
                write_slice(writer, arguments)?;
                writer.tag(0x2133);
                write_option(writer, owner.as_ref())?;
            }
            #[cfg(feature = "rust-import")]
            Self::Imported {
                path,
                identity,
                arguments,
                owner,
            } => {
                writer.tag(0x2107);
                writer.tag(0x2151);
                writer.text(path);
                writer.tag(0x2152);
                writer.bytes(identity.as_bytes());
                writer.tag(0x2153);
                write_slice(writer, arguments)?;
                writer.tag(0x2154);
                owner.write(writer)?;
            }
            Self::Support {
                path,
                owner,
                contract,
                identity: _,
            } => {
                writer.tag(0x2106);
                writer.tag(0x2141);
                writer.text(path);
                writer.tag(0x2142);
                owner.write(writer)?;
                writer.tag(0x2143);
                contract.write(writer)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unit => formatter.write_str("()"),
            Self::Object(name) => formatter.write_str(name),
            Self::Array {
                element,
                length: Some(length),
            } => write!(formatter, "[{element}; {length}]"),
            Self::Array {
                element,
                length: None,
            } => write!(formatter, "&[{element}]"),
            Self::Option(element) => write!(formatter, "Option<{element}>"),
            Self::Result(ok, err) => write!(formatter, "Result<{ok}, {err}>"),
            Self::Tuple(members) => {
                let members = members
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(formatter, "({members})")
            }
            Self::Callable { inputs, output, .. } => {
                let inputs = inputs
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(formatter, "fn({inputs}) -> {output}")
            }
            Self::Builtin { name, .. } => formatter.write_str(name),
            Self::Script {
                identity: type_def::Identity::Script(key),
                ..
            } => key.fmt(formatter),
            Self::Script {
                identity: type_def::Identity::Host(identity),
                ..
            } => write!(formatter, "host::{identity:?}"),
            Self::Host {
                path, arguments, ..
            } if arguments.is_empty() => formatter.write_str(path),
            #[cfg(feature = "rust-import")]
            Self::Imported {
                path, arguments, ..
            } if arguments.is_empty() => formatter.write_str(path),
            Self::Support { path, .. } => formatter.write_str(path),
            Self::Host {
                path, arguments, ..
            } => write!(
                formatter,
                "{path}<{}>",
                arguments
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            #[cfg(feature = "rust-import")]
            Self::Imported {
                path, arguments, ..
            } => write!(
                formatter,
                "{path}<{}>",
                arguments
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Representation { expected, actual } => {
                write!(
                    formatter,
                    "value has runtime type `{actual}`, expected `{expected}`"
                )
            }
            Self::SymbolicCallable { expected } => {
                write!(
                    formatter,
                    "symbolic callable is not exact item `{expected}`"
                )
            }
            Self::Nominal(error) => error.fmt(formatter),
            Self::Member {
                target,
                index,
                source,
            } => write!(
                formatter,
                "member {index} of `{}` failed exact validation: {source}",
                identity_name(target)
            ),
            Self::MissingAttestation { ty } => {
                write!(
                    formatter,
                    "missing exact runtime type attestation for `{ty}`"
                )
            }
            Self::AttestationMismatch { ty } => {
                write!(
                    formatter,
                    "runtime value failed exact type attestation for `{ty}`"
                )
            }
            Self::Unsupported { ty } => {
                write!(formatter, "`{ty}` has no exact runtime representation")
            }
        }
    }
}

impl From<program::construction::Error> for Error {
    fn from(error: program::construction::Error) -> Self {
        Self::Nominal(error)
    }
}

#[cfg(not(feature = "no_std"))]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Nominal(error) => Some(error),
            Self::Member { source, .. } => Some(source.as_ref()),
            Self::Representation { .. }
            | Self::SymbolicCallable { .. }
            | Self::MissingAttestation { .. }
            | Self::AttestationMismatch { .. }
            | Self::Unsupported { .. } => None,
        }
    }
}

#[cfg(feature = "no_std")]
impl core_error::Error for Error {
    fn source(&self) -> Option<&(dyn core_error::Error + 'static)> {
        match self {
            Self::Nominal(error) => Some(error),
            Self::Member { source, .. } => Some(source.as_ref()),
            Self::Representation { .. }
            | Self::SymbolicCallable { .. }
            | Self::MissingAttestation { .. }
            | Self::AttestationMismatch { .. }
            | Self::Unsupported { .. } => None,
        }
    }
}

fn identity_name(identity: &type_def::Identity) -> String {
    match identity {
        type_def::Identity::Host(identity) => format!("host::{identity:?}"),
        type_def::Identity::Script(key) => key.to_string(),
    }
}

#[cfg(all(test, feature = "rust-import"))]
mod semantic_representation_tests {
    use super::{Representation, RepresentationValue, SemanticRepresentation};
    use std::any::TypeId;
    use std::hash::{DefaultHasher, Hash, Hasher};

    fn digest(value: &impl Hash) -> u64 {
        let mut digest = DefaultHasher::new();
        value.hash(&mut digest);
        digest.finish()
    }

    #[test]
    fn builtin_projection_requires_the_closed_exact_pair() {
        let mut string = Representation::exact(TypeId::of::<crate::ImmutableString>());
        let raw = string;
        assert!(string.project_builtin("String"));
        assert_eq!(raw.runtime(), TypeId::of::<crate::ImmutableString>());
        assert_ne!(digest(&string), digest(&raw));
        assert!(matches!(
            string,
            Representation(RepresentationValue::Semantic(
                SemanticRepresentation::Builtin(_)
            ))
        ));

        let mut wrong = Representation::exact(TypeId::of::<crate::INT>());
        assert!(!wrong.project_builtin("String"));
        let mut unknown = Representation::exact(TypeId::of::<std::ops::RangeInclusive<i64>>());
        assert!(!unknown.project_builtin("RangeInclusive<i64>"));
        assert!(!string.project_builtin("String"));
    }

    #[test]
    fn script_projection_is_closed_over_exact_nominal_classes() {
        let mut structure = Representation::exact(TypeId::of::<crate::types::nominal::Struct>());
        let mut enumeration = Representation::exact(TypeId::of::<crate::types::nominal::Enum>());
        assert!(structure.project_script(crate::type_def::Kind::Struct));
        assert!(enumeration.project_script(crate::type_def::Kind::Enum));
        assert_ne!(digest(&structure), digest(&enumeration));

        let mut unsupported = Representation::exact(TypeId::of::<crate::Dynamic>());
        assert!(!unsupported.project_script(crate::type_def::Kind::Struct));
        let mut wrong_kind = Representation::exact(TypeId::of::<crate::types::nominal::Struct>());
        assert!(!wrong_kind.project_script(crate::type_def::Kind::Enum));
        assert!(!structure.project_script(crate::type_def::Kind::Struct));
    }

    #[test]
    fn semantic_marker_hashes_without_granting_runtime_identity() {
        let mut admitted = Representation::exact(TypeId::of::<crate::ImmutableString>());
        let exact = admitted;
        assert!(admitted.project_builtin("String"));
        let independent_artifact = admitted;
        assert_eq!(digest(&independent_artifact), digest(&admitted));
        assert_eq!(exact.runtime(), TypeId::of::<crate::ImmutableString>());
        assert!(matches!(
            admitted,
            Representation(RepresentationValue::Semantic(
                SemanticRepresentation::Builtin(_)
            ))
        ));
    }
}
