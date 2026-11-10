//! Exact runtime type proofs retained by compiler-private operations.

use crate::{host, program, type_def, Dynamic, ImmutableString};
use std::any::TypeId;
use std::collections::BTreeSet;
use std::fmt;

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
    /// Rust's `Rc<T>` or `Arc<T>`, by its name, of the one type in
    /// `element`: rawr's counted pointer, whose value is checked as a `T`
    /// (#775).
    Counted {
        name: ImmutableString,
        element: Box<[Type]>,
    },
    Script {
        identity: type_def::Identity,
        schema: program::schema::Fingerprint,
        kind: type_def::Kind,
        runtime: Representation,
        /// A generic type's type arguments, which its value doesn't carry:
        /// a member of a parameter's type is checked against them.
        arguments: Box<[Type]>,
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
}

impl Representation {
    pub(crate) const fn exact(exact: TypeId) -> Self {
        Self(RepresentationValue::Exact(exact))
    }

    pub(crate) fn runtime(self) -> TypeId {
        match self.0 {
            RepresentationValue::Exact(exact) => exact,
        }
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
            Self::Counted { name, element } => Self::Counted {
                name: name.clone(),
                element: each(element),
            },
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
            Self::Unit | Self::Object(_) | Self::Script { .. } => self.clone(),
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

    /// Rust's `Rc` or `Arc` (`name`) of `element`.
    pub(crate) fn counted(name: impl Into<ImmutableString>, element: Self) -> Self {
        Self::Counted {
            name: name.into(),
            element: Box::new([element]),
        }
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
        arguments: Box<[Self]>,
    ) -> Self {
        Self::Script {
            identity,
            schema,
            kind,
            runtime: Representation::exact(runtime),
            arguments,
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

    pub(crate) fn runtime(&self) -> TypeId {
        match self {
            Self::Unit => TypeId::of::<()>(),
            Self::Object(_) => TypeId::of::<Dynamic>(),
            Self::Array { .. } | Self::Tuple(_) => TypeId::of::<crate::Array>(),
            Self::Option(_) => TypeId::of::<crate::option::Value>(),
            Self::Counted { .. } => TypeId::of::<crate::resolve::lower_nominal::counted::Counted>(),
            Self::Result(..) => TypeId::of::<crate::result::Value>(),
            Self::Callable { .. } => TypeId::of::<crate::item::Value>(),
            Self::Builtin { runtime, .. } | Self::Script { runtime, .. } => runtime.runtime(),
            Self::Host {
                identity: runtime, ..
            } => *runtime,
            #[cfg(feature = "rust-import")]
            Self::Imported { .. } => TypeId::of::<Dynamic>(),
        }
    }

    /// Whether `self` and `other` are one type. An imported type is one type
    /// at each of its paths, as rustc has it: a crate's macros name its types
    /// through a hidden segment (`iced::__rawr_crate_iced_core_<id>::Color`)
    /// beside their public paths, and a value made at one path is the type
    /// a signature or a pattern names at the other (#893, #953).
    pub(crate) fn same(&self, other: &Self) -> bool {
        let all = |own: &[Self], other: &[Self]| {
            own.len() == other.len() && own.iter().zip(other).all(|(own, other)| own.same(other))
        };
        match (self, other) {
            (
                Self::Array { element, length },
                Self::Array {
                    element: other_element,
                    length: other_length,
                },
            ) => length == other_length && element.same(other_element),
            (Self::Option(own), Self::Option(other)) => own.same(other),
            (Self::Result(ok, error), Self::Result(other_ok, other_error)) => {
                ok.same(other_ok) && error.same(other_error)
            }
            (Self::Tuple(own), Self::Tuple(other)) => all(own, other),
            (
                Self::Callable {
                    inputs,
                    output,
                    item,
                },
                Self::Callable {
                    inputs: other_inputs,
                    output: other_output,
                    item: other_item,
                },
            ) => item == other_item && all(inputs, other_inputs) && output.same(other_output),
            (
                Self::Counted { name, element },
                Self::Counted {
                    name: other_name,
                    element: other_element,
                },
            ) => name == other_name && all(element, other_element),
            (
                Self::Script {
                    identity,
                    schema,
                    kind,
                    runtime,
                    arguments,
                },
                Self::Script {
                    identity: other_identity,
                    schema: other_schema,
                    kind: other_kind,
                    runtime: other_runtime,
                    arguments: other_arguments,
                },
            ) => {
                identity == other_identity
                    && schema == other_schema
                    && kind == other_kind
                    && runtime == other_runtime
                    && all(arguments, other_arguments)
            }
            (
                Self::Host {
                    path,
                    identity,
                    arguments,
                    owner,
                },
                Self::Host {
                    path: other_path,
                    identity: other_identity,
                    arguments: other_arguments,
                    owner: other_owner,
                },
            ) => {
                path == other_path
                    && identity == other_identity
                    && owner == other_owner
                    && all(arguments, other_arguments)
            }
            // One crate's type is one type whichever facade reaches it: inboard's
            // `space!()` makes `ui::Element` through `common::core`, which
            // re-exports it, and `text(..).boxed()` through the program's own
            // path to `ui`. The identity names the definition, not the path.
            #[cfg(feature = "rust-import")]
            (
                Self::Imported {
                    identity,
                    arguments,
                    ..
                },
                Self::Imported {
                    identity: other_identity,
                    arguments: other_arguments,
                    ..
                },
            ) => identity == other_identity && all(arguments, other_arguments),
            (own, other) => own == other,
        }
    }

    pub(crate) const fn arguments(&self) -> &[Self] {
        match self {
            Self::Host { arguments, .. } => arguments,
            #[cfg(feature = "rust-import")]
            Self::Imported { arguments, .. } => arguments,
            Self::Script { arguments, .. } => arguments,
            Self::Counted { element, .. } => element,
            Self::Unit
            | Self::Array { .. }
            | Self::Option(_)
            | Self::Result(..)
            | Self::Tuple(_)
            | Self::Callable { .. }
            | Self::Builtin { .. }
            | Self::Object(_) => &[],
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
            | Self::Counted { .. }
            | Self::Script { .. } => None,
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

    pub(crate) fn validate(
        &self,
        value: &Dynamic,
        model: &program::model::Model,
        registry: &host::runtime::Registry,
    ) -> Result<(), Error> {
        // A sibling static not written yet (D865) is only an address, which
        // a `&'static` parameter takes; the trampoline checks its exact type.
        if value.is::<crate::place::statics::Slot>() {
            return Ok(());
        }
        if let Some(place) = crate::place::get(value) {
            return if place.witness().matches_native(registry.witness, self) {
                Ok(())
            } else {
                Err(Error::Representation {
                    expected: self.to_string().into(),
                    actual: "a native place of another type".into(),
                })
            };
        }
        match self {
            // A function type, as a script struct's `Box<dyn Fn(i64) ->
            // Message>` field is: a callable the program made, of any of
            // its forms.
            Self::Object(name) if crate::item::type_ref::is_fn_type(name) => Ok(()),
            // A crate's trait object (D872): a program's value coerced to it,
            // or one native code gave, which the adapter takes at its exact
            // type.
            Self::Object(name) if crate::resolve::type_ref::is_native_trait(name) => Ok(()),
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
            // A slice native code lends the call (#1292), as a value it
            // lends (`Registry::attest`), is reached only through the
            // accessor its trampoline made, whose exact type each access
            // checks.
            Self::Array { length: None, .. } if value.is::<crate::scope::Lent>() => Ok(()),
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
                identity,
                schema,
                arguments,
                ..
            } => model.validate_value_exact(value, identity, schema, arguments, registry),
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
            // The pointer, and the value it points to as its type.
            Self::Counted { element, .. } => {
                let pointee =
                    crate::resolve::lower_nominal::counted::value(value).ok_or_else(|| {
                        Error::Representation {
                            expected: self.to_string().into(),
                            actual: value.type_name().into(),
                        }
                    })?;
                element
                    .iter()
                    .try_for_each(|element| element.validate(&pointee, model, registry))
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
            Self::Array { element, .. } | Self::Option(element) => {
                element.collect_dependencies(dependencies);
            }
            Self::Counted { element, .. } => {
                for element in element {
                    element.collect_dependencies(dependencies);
                }
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

    /// [`Self::from_resolved`] with each generic parameter named in
    /// `parameters` given its type: a generic script type's member of a
    /// parameter's type takes the type its value was given (review 1).
    pub(crate) fn from_resolved_in(
        ty: &crate::resolve::type_ref::Type,
        model: &program::model::Model,
        parameters: &[(ImmutableString, Self)],
    ) -> Result<Self, Error> {
        use crate::resolve::type_ref::{Argument, Target};

        let crate::resolve::type_ref::Type::Path(path) = ty else {
            return match ty {
                crate::resolve::type_ref::Type::Unit(_) => Ok(Self::unit()),
                crate::resolve::type_ref::Type::Tuple { members, .. } => Ok(Self::Tuple(
                    members
                        .iter()
                        .map(|member| Self::from_resolved_in(member, model, parameters))
                        .collect::<Result<_, _>>()?,
                )),
                // A shared reference is its referent's value at runtime.
                crate::resolve::type_ref::Type::Reference {
                    mutable: false,
                    target,
                    ..
                } => Self::from_resolved_in(target, model, parameters),
                crate::resolve::type_ref::Type::Reference { .. } => Err(Error::Unsupported {
                    ty: "reference".into(),
                }),
                crate::resolve::type_ref::Type::Path(_) => unreachable!(),
            };
        };

        if let Some(element) = ty.option_element() {
            return Ok(Self::option(Self::from_resolved_in(
                element, model, parameters,
            )?));
        }
        if let Some((ok, err)) = ty.result_elements() {
            return Ok(Self::result(
                Self::from_resolved_in(ok, model, parameters)?,
                Self::from_resolved_in(err, model, parameters)?,
            ));
        }
        // A `Vec` is an array, a `Box` its value, and a map or set an array
        // sorted by key: `(key, value)` pairs or keys.
        if let Some(element) = ty.sequence_element() {
            return Ok(Self::array(
                Self::from_resolved_in(element, model, parameters)?,
                None,
            ));
        }
        if let Some((element, length)) = ty.array_element() {
            return Ok(Self::array(
                Self::from_resolved_in(element, model, parameters)?,
                Some(length),
            ));
        }
        if let Some(element) = ty.box_element() {
            return Self::from_resolved_in(element, model, parameters);
        }
        if ty.dyn_trait().is_some() {
            return Ok(Self::Object(path.name.clone()));
        }
        if let Some((_, arguments)) = ty.keyed_arguments() {
            let mut arguments = arguments
                .into_iter()
                .map(|argument| Self::from_resolved_in(argument, model, parameters))
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
                "f64" => Ok(Self::builtin("f64", TypeId::of::<crate::FLOAT>())),
                // `std::time`'s types are Rust's at runtime, as a script
                // struct's `Instant` or `Duration` field holds them.
                "Duration" => Ok(Self::builtin(
                    "Duration",
                    TypeId::of::<std::time::Duration>(),
                )),
                "Instant" => Ok(Self::builtin("Instant", TypeId::of::<crate::Instant>())),
                // A function type: any callable (`validate`).
                name if crate::item::type_ref::is_fn_type(name) => Ok(Self::Object(name.into())),
                name => Err(Error::Unsupported { ty: name.into() }),
            },
            Target::Parameter
                if let Some((_, ty)) = parameters.iter().find(|(name, _)| *name == path.name) =>
            {
                Ok(ty.clone())
            }
            Target::Parameter | Target::Projection(..) | Target::Alias(_) | Target::Std(_) => {
                Err(Error::Unsupported {
                    ty: path.name.clone(),
                })
            }
            Target::Script { identity, .. } => {
                Self::script_in(identity, &path.arguments, model, parameters)
            }
            // A mirror is its marked type, as every type the lowering reads
            // names it (#1484): a native value given in an `Element<'a,
            // Message>` over the marked `Theme` is attested as the type the
            // calls that take it expect.
            #[cfg(feature = "rust-import")]
            Target::Host { definition, .. } if let Some(identity) = model.mirrored(definition) => {
                Self::script_in(identity, &path.arguments, model, parameters)
            }
            Target::Host { definition, owner } => {
                let arguments = path
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        Argument::Type(ty) => Some(Self::from_resolved_in(ty, model, parameters)),
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

    /// The marked type `identity` with `arguments`, as
    /// [`Self::from_resolved_in`] reads each.
    fn script_in(
        identity: &type_def::Identity,
        arguments: &[crate::resolve::type_ref::Argument],
        model: &program::model::Model,
        parameters: &[(ImmutableString, Self)],
    ) -> Result<Self, Error> {
        let unknown = || {
            Error::Nominal(program::construction::Error::UnknownTarget {
                target: identity.clone(),
            })
        };
        Ok(Self::script(
            identity.clone(),
            model.schema(identity).ok_or_else(unknown)?.clone(),
            model.kind(identity).ok_or_else(unknown)?,
            model.runtime(identity).ok_or_else(unknown)?,
            arguments
                .iter()
                .filter_map(|argument| match argument {
                    crate::resolve::type_ref::Argument::Type(ty) => {
                        Some(Self::from_resolved_in(ty, model, parameters))
                    }
                    crate::resolve::type_ref::Argument::Lifetime(_) => None,
                })
                .collect::<Result<Box<[_]>, _>>()?,
        ))
    }
}

impl crate::digest::Write for Type {
    fn write(&self, writer: &mut crate::digest::Writer) -> Result<(), crate::digest::Error> {
        use crate::digest::{write_option, write_script_identity, write_slice};

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
            Self::Counted { name, element } => {
                writer.tag(0x2106);
                writer.text(name);
                write_slice(writer, element)?;
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
                arguments,
            } => {
                writer.tag(0x2104);
                writer.tag(0x2121);
                write_script_identity(writer, identity, "preview runtime script identity")?;
                writer.tag(0x2122);
                schema.write(writer)?;
                // A type without arguments writes as it did before them.
                if !arguments.is_empty() {
                    writer.tag(0x2123);
                    for argument in arguments {
                        argument.write(writer)?;
                    }
                }
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
            Self::Counted { name, element } => {
                let element = element.iter().map(ToString::to_string).collect::<String>();
                write!(formatter, "{name}<{element}>")
            }
            Self::Script {
                identity: type_def::Identity::Script(key),
                arguments,
                ..
            } if arguments.is_empty() => key.fmt(formatter),
            Self::Script {
                identity: type_def::Identity::Script(key),
                arguments,
                ..
            } => write!(
                formatter,
                "{key}<{}>",
                arguments
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
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

impl Error {
    /// Whether a native value, or one a value holds, is not the type the
    /// runner's own witness of it names. Imported types don't change while
    /// a runner lives, and rustc has checked every operand's type, so no
    /// program causes it: it is a rawr bug (D664).
    pub(crate) fn is_invariant(&self) -> bool {
        match self {
            Self::AttestationMismatch { .. } => true,
            Self::Member { source, .. } => source.is_invariant(),
            Self::Representation { .. }
            | Self::SymbolicCallable { .. }
            | Self::Nominal(_)
            | Self::MissingAttestation { .. }
            | Self::Unsupported { .. } => false,
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

fn identity_name(identity: &type_def::Identity) -> String {
    match identity {
        type_def::Identity::Host(identity) => format!("host::{identity:?}"),
        type_def::Identity::Script(key) => key.to_string(),
    }
}
