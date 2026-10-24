//! Generation-bound runtime models for nominal values.

use super::{construction, schema, GenerationId};
use crate::resolve::{item, type_ref, Program};
use crate::types::{dynamic::Union, nominal};
use crate::{capability, type_def, Dynamic, ImmutableString};
use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

pub(crate) mod carry;

type EnumConstruction = (
    type_def::Identity,
    schema::Fingerprint,
    nominal::variant::Id,
    Box<[Dynamic]>,
);

/// The immutable nominal runtime model paired with one executable generation.
#[derive(Clone)]
pub(crate) struct Model {
    generation: GenerationId,
    definitions: BTreeMap<type_def::Identity, Definition>,
}

#[derive(Clone)]
struct Definition {
    schema: schema::Fingerprint,
    capabilities: capability::Set,
    shape: Shape,
    /// An enum's `#[default]` variant, which a derived `Default` builds.
    default_variant: Option<nominal::variant::Id>,
}

#[derive(Clone)]
enum Shape {
    Struct(Box<[type_ref::Type]>),
    Enum(BTreeMap<nominal::variant::Id, Box<[type_ref::Type]>>),
}

/// Exact owned values recursively validated for one nominal construction.
///
/// The fields and constructor remain private to this module. The proof is not
/// cloneable and can only be consumed once by a nominal constructor.
pub(crate) struct Construction {
    generation: GenerationId,
    identity: type_def::Identity,
    schema: schema::Fingerprint,
    target: Target,
    members: Box<[Dynamic]>,
}

enum Target {
    Struct,
    Enum(nominal::variant::Id),
}

impl Model {
    pub(crate) fn new(generation: GenerationId, program: &Program) -> Self {
        let definitions: BTreeMap<_, _> = program
            .types()
            .iter()
            .map(|definition| (definition.id(), definition))
            .collect();
        let mut runtime = BTreeMap::new();

        for shell in program.items() {
            let (definition, shape, default_variant) = match &shell.kind {
                item::Kind::Struct(item) => {
                    let definition = definitions[&item.definition];
                    let fields = item.fields.iter().map(|field| field.ty.clone()).collect();
                    (definition, Shape::Struct(fields), None)
                }
                item::Kind::Enum(item) => {
                    let definition = definitions[&item.definition];
                    let variants = item
                        .variants
                        .iter()
                        .map(|variant| {
                            (
                                nominal::variant::Id::from_name(&variant.name),
                                variant.payload.clone(),
                            )
                        })
                        .collect();
                    let default_variant = item
                        .variants
                        .iter()
                        .find(|variant| variant.default.is_some())
                        .map(|variant| nominal::variant::Id::from_name(&variant.name));
                    (definition, Shape::Enum(variants), default_variant)
                }
                item::Kind::Use(_)
                | item::Kind::Impl(_)
                | item::Kind::Trait(_)
                | item::Kind::Function(_) => continue,
            };
            runtime.insert(
                type_def::Identity::Script(definition.key().clone()),
                Definition {
                    schema: definition.schema().clone(),
                    capabilities: definition.capabilities(),
                    shape,
                    default_variant,
                },
            );
        }

        Self {
            generation,
            definitions: runtime,
        }
    }

    pub(crate) const fn generation(&self) -> GenerationId {
        self.generation
    }

    pub(crate) fn prove_struct(
        &self,
        identity: &type_def::Identity,
        schema: &schema::Fingerprint,
        members: Box<[Dynamic]>,
    ) -> Result<Construction, construction::Error> {
        let definition = self.definition(identity, schema)?;
        let Shape::Struct(fields) = &definition.shape else {
            return Err(construction::Error::KindMismatch {
                target: identity.clone(),
                expected: construction::Kind::Struct,
                actual: construction::Kind::Enum,
            });
        };
        self.validate_members(identity, None, &members, fields)?;
        Ok(Construction {
            generation: self.generation,
            identity: identity.clone(),
            schema: schema.clone(),
            target: Target::Struct,
            members,
        })
    }

    pub(crate) fn prove_enum(
        &self,
        identity: &type_def::Identity,
        schema: &schema::Fingerprint,
        variant: &nominal::variant::Id,
        members: Box<[Dynamic]>,
    ) -> Result<Construction, construction::Error> {
        let definition = self.definition(identity, schema)?;
        let Shape::Enum(variants) = &definition.shape else {
            return Err(construction::Error::KindMismatch {
                target: identity.clone(),
                expected: construction::Kind::Enum,
                actual: construction::Kind::Struct,
            });
        };
        let Some(payload) = variants.get(variant) else {
            return Err(construction::Error::UnknownVariant {
                target: identity.clone(),
                variant: variant.name().into(),
            });
        };
        self.validate_members(identity, Some(variant.name()), &members, payload)?;
        Ok(Construction {
            generation: self.generation,
            identity: identity.clone(),
            schema: schema.clone(),
            target: Target::Enum(variant.clone()),
            members,
        })
    }

    pub(crate) fn validate_value(
        &self,
        value: &Dynamic,
        identity: &type_def::Identity,
        schema: &schema::Fingerprint,
    ) -> Result<(), construction::Error> {
        let definition = self.definition(identity, schema)?;
        self.validate_nominal(value, identity, definition, &mut BTreeSet::new())
    }

    pub(crate) fn validate_value_exact(
        &self,
        value: &Dynamic,
        identity: &type_def::Identity,
        schema: &schema::Fingerprint,
        registry: &crate::host::runtime::Registry,
    ) -> Result<(), crate::host::runtime::type_ref::Error> {
        let definition = self.definition(identity, schema)?;
        self.validate_nominal_exact(value, identity, definition, registry, &mut BTreeSet::new())
    }

    pub(crate) fn schema(&self, identity: &type_def::Identity) -> Option<&schema::Fingerprint> {
        self.definitions
            .get(identity)
            .map(|definition| &definition.schema)
    }

    pub(crate) fn runtime(&self, identity: &type_def::Identity) -> Option<TypeId> {
        Some(match &self.definitions.get(identity)?.shape {
            Shape::Struct(_) => TypeId::of::<nominal::Struct>(),
            Shape::Enum(_) => TypeId::of::<nominal::Enum>(),
        })
    }

    pub(crate) fn kind(&self, identity: &type_def::Identity) -> Option<type_def::Kind> {
        match &self.definitions.get(identity)?.shape {
            Shape::Struct(_) => Some(type_def::Kind::Struct),
            Shape::Enum(_) => Some(type_def::Kind::Enum),
        }
    }

    pub(crate) fn materialize_default(
        &self,
        identity: &type_def::Identity,
        schema: &schema::Fingerprint,
    ) -> Result<Dynamic, construction::Error> {
        self.default_nominal(identity, schema, &mut BTreeSet::new())
    }

    fn default_nominal(
        &self,
        identity: &type_def::Identity,
        schema: &schema::Fingerprint,
        active: &mut BTreeSet<type_def::Identity>,
    ) -> Result<Dynamic, construction::Error> {
        let definition = self.definition(identity, schema)?;
        if !definition.capabilities.contains(capability::Kind::Default) {
            return Err(construction::Error::MissingDefault {
                target: identity.clone(),
            });
        }
        let Shape::Struct(fields) = &definition.shape else {
            // A derived `Default` builds the `#[default]` unit variant.
            let Some(variant) = &definition.default_variant else {
                return Err(construction::Error::KindMismatch {
                    target: identity.clone(),
                    expected: construction::Kind::Struct,
                    actual: construction::Kind::Enum,
                });
            };
            let proof = self.prove_enum(identity, schema, variant, Box::default())?;
            return nominal::Enum::construct(self, identity, schema, variant, proof)
                .map(Into::into);
        };
        if !active.insert(identity.clone()) {
            return Err(construction::Error::RecursiveDefault {
                target: identity.clone(),
            });
        }
        let members = fields
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                self.default_for_type(ty, active)
                    .map_err(|source| construction::Error::Member {
                        target: identity.clone(),
                        index,
                        source: Box::new(source),
                    })
            })
            .collect::<Result<Box<[_]>, _>>();
        active.remove(identity);
        let members = members?;
        let proof = self.prove_struct(identity, schema, members)?;
        nominal::Struct::construct(self, identity, schema, proof).map(Into::into)
    }

    fn default_for_type(
        &self,
        ty: &type_ref::Type,
        active: &mut BTreeSet<type_def::Identity>,
    ) -> Result<Dynamic, construction::Error> {
        match ty {
            type_ref::Type::Unit(_) => Ok(Dynamic::UNIT),
            type_ref::Type::Tuple { members, .. } => members
                .iter()
                .map(|member| self.default_for_type(member, active))
                .collect::<Result<crate::Array, _>>()
                .map(Dynamic::from_array),
            type_ref::Type::Path(_) if let Some(inner) = ty.box_element() => {
                self.default_for_type(inner, active)
            }
            type_ref::Type::Path(_) if let Some((element, length)) = ty.array_element() => (0
                ..length)
                .map(|_| self.default_for_type(element, active))
                .collect::<Result<crate::Array, _>>()
                .map(Dynamic::from_array),
            type_ref::Type::Reference { .. } => {
                Err(construction::Error::ValueType {
                    expected: type_name(ty),
                    actual: "no executable Default implementation".into(),
                })
            }
            type_ref::Type::Path(_) if let Some(inner) = ty.reverse_element() => self
                .default_for_type(inner, active)
                .map(|value| Dynamic::from(std::cmp::Reverse(value))),
            type_ref::Type::Path(path) => match &path.target {
                type_ref::Target::Builtin => match crate::host::runtime_builtin(&path.name) {
                    "Option" => Ok(crate::option::none()),
                    "Vec" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet" => {
                        Ok(Dynamic::from_array(crate::Array::new()))
                    }
                    "bool" => Ok(false.into()),
                    "char" => Ok('\0'.into()),
                    "i64" => Ok(0_i64.into()),
                    #[cfg(not(feature = "no_float"))]
                    "f64" => Ok(0.0_f64.into()),
                    "String" => Ok(ImmutableString::new().into()),
                    "Duration" => Ok(Dynamic::from(std::time::Duration::ZERO)),
                    _ => Err(construction::Error::ValueType {
                        expected: path.name.clone(),
                        actual: "no executable Default implementation".into(),
                    }),
                },
                type_ref::Target::Script { identity, .. } => {
                    let nested = self.definitions.get(identity).ok_or_else(|| {
                        construction::Error::UnknownTarget {
                            target: identity.clone(),
                        }
                    })?;
                    self.default_nominal(identity, &nested.schema.clone(), active)
                }
                type_ref::Target::Parameter | type_ref::Target::Projection(..) | type_ref::Target::Alias(_) => {
                    Err(construction::Error::ValueType {
                        expected: path.name.clone(),
                        actual: "no executable Default implementation of a generic parameter"
                            .into(),
                    })
                }
                type_ref::Target::Host { definition, .. } => {
                    #[cfg(not(feature = "no_std"))]
                    if let Some(value) = definition
                        .native_identity()
                        .and_then(crate::host::value::host_default)
                    {
                        return Ok(value);
                    }
                    let _ = definition;
                    Err(construction::Error::ValueType {
                        expected: path.name.clone(),
                        actual: "no executable host Default implementation".into(),
                    })
                }
            },
        }
    }

    fn definition(
        &self,
        identity: &type_def::Identity,
        schema: &schema::Fingerprint,
    ) -> Result<&Definition, construction::Error> {
        let Some(definition) = self.definitions.get(identity) else {
            return Err(construction::Error::UnknownTarget {
                target: identity.clone(),
            });
        };
        if &definition.schema != schema {
            return Err(construction::Error::SchemaMismatch {
                target: identity.clone(),
            });
        }
        Ok(definition)
    }

    fn validate_members(
        &self,
        identity: &type_def::Identity,
        variant: Option<&str>,
        members: &[Dynamic],
        expected: &[type_ref::Type],
    ) -> Result<(), construction::Error> {
        self.validate_members_with_shared(
            identity,
            variant,
            members,
            expected,
            &mut BTreeSet::new(),
        )
    }

    fn validate_members_with_shared(
        &self,
        identity: &type_def::Identity,
        variant: Option<&str>,
        members: &[Dynamic],
        expected: &[type_ref::Type],
        shared: &mut BTreeSet<usize>,
    ) -> Result<(), construction::Error> {
        if members.len() != expected.len() {
            return Err(construction::Error::InvalidArity {
                target: identity.clone(),
                variant: variant.map(Into::into),
                expected: expected.len(),
                actual: members.len(),
            });
        }
        for (index, (value, ty)) in members.iter().zip(expected).enumerate() {
            self.validate_type(value, ty, shared).map_err(|source| {
                construction::Error::Member {
                    target: identity.clone(),
                    index,
                    source: Box::new(source),
                }
            })?;
        }
        Ok(())
    }

    fn validate_type(
        &self,
        value: &Dynamic,
        expected: &type_ref::Type,
        shared: &mut BTreeSet<usize>,
    ) -> Result<(), construction::Error> {
        if let Some(element) = expected.option_element() {
            return match crate::option::read(value) {
                Some(Some(inner)) => self.validate_type(&inner, element, shared),
                Some(None) => Ok(()),
                None => Err(construction::Error::ValueType {
                    expected: type_name(expected),
                    actual: value.type_name().into(),
                }),
            };
        }
        if let Some(element) = expected.vec_element() {
            let Ok(array) = value.as_array_ref() else {
                return Err(construction::Error::ValueType {
                    expected: type_name(expected),
                    actual: value.type_name().into(),
                });
            };
            return array
                .iter()
                .try_for_each(|value| self.validate_type(value, element, shared));
        }
        if let Some((element, length)) = expected.array_element() {
            let array = value
                .as_array_ref()
                .ok()
                .filter(|array| array.len() == length)
                .ok_or_else(|| construction::Error::ValueType {
                    expected: type_name(expected),
                    actual: value.type_name().into(),
                })?;
            return array
                .iter()
                .try_for_each(|value| self.validate_type(value, element, shared));
        }
        if let Some(element) = expected.box_element() {
            return self.validate_type(value, element, shared);
        }
        // A `RefCell`'s value is checked where it is stored.
        if expected.cell_element().is_some() {
            return if crate::resolve::lower_nominal::is_cell(value) {
                Ok(())
            } else {
                Err(construction::Error::ValueType {
                    expected: type_name(expected),
                    actual: value.type_name().into(),
                })
            };
        }
        // A shared reference, as a `&'a State` field, is its referent's
        // value at runtime.
        if let type_ref::Type::Reference {
            mutable: false,
            target,
            ..
        } = expected
        {
            return self.validate_type(value, target, shared);
        }
        if let Some((ok, err)) = expected.result_elements() {
            return match crate::result::read(value) {
                Some(Ok(inner)) => self.validate_type(&inner, ok, shared),
                Some(Err(inner)) => self.validate_type(&inner, err, shared),
                None => Err(construction::Error::ValueType {
                    expected: type_name(expected),
                    actual: value.type_name().into(),
                }),
            };
        }
        let type_ref::Type::Path(path) = expected else {
            return matches_simple_type(value, expected);
        };
        let type_ref::Target::Script { identity, .. } = &path.target else {
            return matches_simple_type(value, expected);
        };
        let definition =
            self.definitions
                .get(identity)
                .ok_or_else(|| construction::Error::UnknownTarget {
                    target: identity.clone(),
                })?;
        self.validate_nominal(value, identity, definition, shared)
    }

    fn validate_nominal(
        &self,
        value: &Dynamic,
        identity: &type_def::Identity,
        definition: &Definition,
        shared: &mut BTreeSet<usize>,
    ) -> Result<(), construction::Error> {
        #[cfg(not(feature = "no_closure"))]
        if let Union::Shared(cell, ..) = &value.0 {
            let address = crate::Shared::as_ptr(cell).cast::<()>() as usize;
            if !shared.insert(address) {
                return Err(construction::Error::Cycle {
                    target: identity.clone(),
                });
            }
            let guard =
                crate::func::locked_read(cell).ok_or_else(|| construction::Error::ValueType {
                    expected: identity_name(identity),
                    actual: value.type_name().into(),
                })?;
            let result = self.validate_nominal(&guard, identity, definition, shared);
            shared.remove(&address);
            return result;
        }

        match (&value.0, &definition.shape) {
            (Union::Struct(value, ..), Shape::Struct(fields)) => {
                validate_identity_schema(value.identity(), value.schema(), identity, definition)?;
                self.validate_members_with_shared(identity, None, value.fields(), fields, shared)
            }
            (Union::Enum(value, ..), Shape::Enum(variants)) => {
                validate_identity_schema(value.identity(), value.schema(), identity, definition)?;
                let Some(payload) = variants.get(value.variant()) else {
                    return Err(construction::Error::UnknownVariant {
                        target: identity.clone(),
                        variant: value.variant().name().into(),
                    });
                };
                self.validate_members_with_shared(
                    identity,
                    Some(value.variant().name()),
                    value.payload(),
                    payload,
                    shared,
                )
            }
            (Union::Struct(..), Shape::Enum(_)) => Err(construction::Error::KindMismatch {
                target: identity.clone(),
                expected: construction::Kind::Enum,
                actual: construction::Kind::Struct,
            }),
            (Union::Enum(..), Shape::Struct(_)) => Err(construction::Error::KindMismatch {
                target: identity.clone(),
                expected: construction::Kind::Struct,
                actual: construction::Kind::Enum,
            }),
            _ => Err(construction::Error::ValueType {
                expected: identity_name(identity),
                actual: value.type_name().into(),
            }),
        }
    }

    fn validate_nominal_exact(
        &self,
        value: &Dynamic,
        identity: &type_def::Identity,
        definition: &Definition,
        registry: &crate::host::runtime::Registry,
        shared: &mut BTreeSet<usize>,
    ) -> Result<(), crate::host::runtime::type_ref::Error> {
        #[cfg(not(feature = "no_closure"))]
        if let Union::Shared(cell, ..) = &value.0 {
            let address = crate::Shared::as_ptr(cell).cast::<()>() as usize;
            if !shared.insert(address) {
                return Err(crate::program::construction::Error::Cycle {
                    target: identity.clone(),
                }
                .into());
            }
            let guard = crate::func::locked_read(cell).ok_or_else(|| {
                crate::program::construction::Error::ValueType {
                    expected: identity_name(identity),
                    actual: value.type_name().into(),
                }
            })?;
            let result =
                self.validate_nominal_exact(&guard, identity, definition, registry, shared);
            shared.remove(&address);
            return result;
        }

        let (members, expected, variant) = match (&value.0, &definition.shape) {
            (Union::Struct(value, ..), Shape::Struct(fields)) => {
                validate_identity_schema(value.identity(), value.schema(), identity, definition)?;
                (value.fields(), fields.as_ref(), None)
            }
            (Union::Enum(value, ..), Shape::Enum(variants)) => {
                validate_identity_schema(value.identity(), value.schema(), identity, definition)?;
                let expected = variants.get(value.variant()).ok_or_else(|| {
                    crate::program::construction::Error::UnknownVariant {
                        target: identity.clone(),
                        variant: value.variant().name().into(),
                    }
                })?;
                (
                    value.payload(),
                    expected.as_ref(),
                    Some(value.variant().name().into()),
                )
            }
            (Union::Struct(..), Shape::Enum(_)) => {
                return Err(crate::program::construction::Error::KindMismatch {
                    target: identity.clone(),
                    expected: construction::Kind::Enum,
                    actual: construction::Kind::Struct,
                }
                .into());
            }
            (Union::Enum(..), Shape::Struct(_)) => {
                return Err(crate::program::construction::Error::KindMismatch {
                    target: identity.clone(),
                    expected: construction::Kind::Struct,
                    actual: construction::Kind::Enum,
                }
                .into());
            }
            _ => {
                return Err(crate::program::construction::Error::ValueType {
                    expected: identity_name(identity),
                    actual: value.type_name().into(),
                }
                .into());
            }
        };

        if members.len() != expected.len() {
            return Err(crate::program::construction::Error::InvalidArity {
                target: identity.clone(),
                variant,
                expected: expected.len(),
                actual: members.len(),
            }
            .into());
        }
        for (index, (member, expected)) in members.iter().zip(expected).enumerate() {
            // A generic type's value does not carry its type arguments, so a
            // member of a parameter's type (`content: W`) holds any value.
            if names_parameter(expected) {
                continue;
            }
            let ty = crate::host::runtime::type_ref::Type::from_resolved(expected, self)?;
            ty.validate(member, self, registry).map_err(|source| {
                crate::host::runtime::type_ref::Error::Member {
                    target: identity.clone(),
                    index,
                    source: Box::new(source),
                }
            })?;
        }
        Ok(())
    }
}

/// Whether `ty` names a generic parameter anywhere.
fn names_parameter(ty: &crate::resolve::type_ref::Type) -> bool {
    use crate::resolve::type_ref::{Argument, Target, Type};
    match ty {
        Type::Unit(_) => false,
        Type::Tuple { members, .. } => members.iter().any(names_parameter),
        Type::Reference { target, .. } => names_parameter(target),
        Type::Path(path) => {
            matches!(path.target, Target::Parameter | Target::Projection(..))
                || path.arguments.iter().any(|argument| match argument {
                    Argument::Type(ty) => names_parameter(ty),
                    Argument::Lifetime(_) => false,
                })
        }
    }
}

impl Construction {
    pub(crate) fn into_struct(
        self,
        model: &Model,
        expected_identity: &type_def::Identity,
        expected_schema: &schema::Fingerprint,
    ) -> Result<(type_def::Identity, schema::Fingerprint, Box<[Dynamic]>), construction::Error>
    {
        self.verify(model, expected_identity, expected_schema)?;
        if !matches!(self.target, Target::Struct) {
            return Err(construction::Error::KindMismatch {
                target: self.identity,
                expected: construction::Kind::Struct,
                actual: construction::Kind::Enum,
            });
        }
        Ok((self.identity, self.schema, self.members))
    }

    pub(crate) fn into_enum(
        self,
        model: &Model,
        expected_identity: &type_def::Identity,
        expected_schema: &schema::Fingerprint,
        expected_variant: &nominal::variant::Id,
    ) -> Result<EnumConstruction, construction::Error> {
        self.verify(model, expected_identity, expected_schema)?;
        let Target::Enum(actual_variant) = self.target else {
            return Err(construction::Error::KindMismatch {
                target: self.identity,
                expected: construction::Kind::Enum,
                actual: construction::Kind::Struct,
            });
        };
        if &actual_variant != expected_variant {
            return Err(construction::Error::VariantMismatch {
                target: self.identity,
                expected: expected_variant.name().into(),
                actual: actual_variant.name().into(),
            });
        }
        Ok((self.identity, self.schema, actual_variant, self.members))
    }

    fn verify(
        &self,
        model: &Model,
        expected_identity: &type_def::Identity,
        expected_schema: &schema::Fingerprint,
    ) -> Result<(), construction::Error> {
        if self.generation != model.generation {
            return Err(construction::Error::CrossGeneration {
                issued: self.generation,
                current: model.generation,
            });
        }
        if &self.identity != expected_identity {
            return Err(construction::Error::TargetMismatch {
                expected: expected_identity.clone(),
                actual: self.identity.clone(),
            });
        }
        if &self.schema != expected_schema {
            return Err(construction::Error::SchemaMismatch {
                target: self.identity.clone(),
            });
        }
        model.definition(expected_identity, expected_schema)?;
        Ok(())
    }
}

fn validate_identity_schema(
    actual_identity: &type_def::Identity,
    actual_schema: &schema::Fingerprint,
    expected_identity: &type_def::Identity,
    expected: &Definition,
) -> Result<(), construction::Error> {
    if actual_identity != expected_identity {
        return Err(construction::Error::TargetMismatch {
            expected: expected_identity.clone(),
            actual: actual_identity.clone(),
        });
    }
    if actual_schema != &expected.schema {
        return Err(construction::Error::SchemaMismatch {
            target: expected_identity.clone(),
        });
    }
    Ok(())
}

fn matches_simple_type(
    value: &Dynamic,
    expected: &type_ref::Type,
) -> Result<(), construction::Error> {
    if nominal::matches_type(value, expected) {
        Ok(())
    } else {
        Err(construction::Error::ValueType {
            expected: type_name(expected),
            actual: value.type_name().into(),
        })
    }
}

fn type_name(ty: &type_ref::Type) -> ImmutableString {
    match ty {
        type_ref::Type::Unit(_) => "()".into(),
        type_ref::Type::Tuple { members, .. } => format!(
            "({})",
            members.iter().map(type_name).collect::<Vec<_>>().join(", ")
        )
        .into(),
        type_ref::Type::Reference { .. } => "reference".into(),
        type_ref::Type::Path(_) if let Some((element, length)) = ty.array_element() => {
            format!("[{}; {length}]", type_name(element)).into()
        }
        type_ref::Type::Path(path) => path.name.clone(),
    }
}

fn identity_name(identity: &type_def::Identity) -> ImmutableString {
    match identity {
        type_def::Identity::Host(identity) => format!("host::{identity:?}").into(),
        type_def::Identity::Script(key) => key.to_string().into(),
    }
}

#[cfg(all(test, not(feature = "no_function")))]
mod tests {
    use super::Model;
    use crate::program::{construction, GenerationId};
    use crate::resolve::{self, item};
    use crate::types::nominal::{self, variant};
    use crate::{host, type_def, Dynamic, Engine};

    #[test]
    fn proof_owns_the_exact_values_that_were_validated() {
        let program = resolve("enum Message { Add(i64) }");
        let model = Model::new(GenerationId::issue(), &program);
        let (definition, item) = enum_definition(&program, "Message");
        let identity = identity(definition);
        let variant = variant::Id::from_name(&item.variants[0].name);
        let mut loose = Dynamic::from(42_i64);
        let proof = model
            .prove_enum(
                &identity,
                definition.schema(),
                &variant,
                vec![loose.clone()].into_boxed_slice(),
            )
            .unwrap();

        loose = Dynamic::from(7_i64);
        let value =
            nominal::Enum::construct(&model, &identity, definition.schema(), &variant, proof)
                .unwrap();

        assert_eq!(loose.as_int().unwrap(), 7);
        assert_eq!(value.payload()[0].as_int().unwrap(), 42);
    }

    #[test]
    fn proof_rejects_an_equivalent_but_distinct_generation() {
        let program = resolve("struct Counter { value: i64 }");
        let issued = Model::new(GenerationId::issue(), &program);
        let current = Model::new(GenerationId::issue(), &program);
        let definition = struct_definition(&program, "Counter").0;
        let identity = identity(definition);
        let proof = issued
            .prove_struct(
                &identity,
                definition.schema(),
                vec![Dynamic::from(42_i64)].into_boxed_slice(),
            )
            .unwrap();

        assert!(matches!(
            nominal::Struct::construct(&current, &identity, definition.schema(), proof,),
            Err(construction::Error::CrossGeneration { .. })
        ));
    }

    #[test]
    fn proof_rejects_target_kind_schema_and_variant_substitution() {
        let source =
            "struct A { value: i64 } struct B { value: i64 } enum Choice { A(i64), B(i64) }";
        let program = resolve(source);
        let generation = GenerationId::issue();
        let model = Model::new(generation, &program);
        let a = struct_definition(&program, "A").0;
        let b = struct_definition(&program, "B").0;
        let a_identity = identity(a);
        let b_identity = identity(b);
        let target_proof = model
            .prove_struct(
                &a_identity,
                a.schema(),
                vec![Dynamic::from(1_i64)].into_boxed_slice(),
            )
            .unwrap();
        assert!(matches!(
            nominal::Struct::construct(&model, &b_identity, b.schema(), target_proof,),
            Err(construction::Error::TargetMismatch { .. })
        ));

        let (choice, choice_item) = enum_definition(&program, "Choice");
        let choice_identity = identity(choice);
        let first = variant::Id::from_name(&choice_item.variants[0].name);
        let second = variant::Id::from_name(&choice_item.variants[1].name);
        let kind_proof = model
            .prove_enum(
                &choice_identity,
                choice.schema(),
                &first,
                vec![Dynamic::from(1_i64)].into_boxed_slice(),
            )
            .unwrap();
        assert!(matches!(
            nominal::Struct::construct(&model, &choice_identity, choice.schema(), kind_proof,),
            Err(construction::Error::KindMismatch { .. })
        ));

        let variant_proof = model
            .prove_enum(
                &choice_identity,
                choice.schema(),
                &first,
                vec![Dynamic::from(1_i64)].into_boxed_slice(),
            )
            .unwrap();
        assert!(matches!(
            nominal::Enum::construct(
                &model,
                &choice_identity,
                choice.schema(),
                &second,
                variant_proof,
            ),
            Err(construction::Error::VariantMismatch { .. })
        ));

        let changed = resolve("struct A { value: bool }");
        let changed_model = Model::new(generation, &changed);
        let changed_a = struct_definition(&changed, "A").0;
        let schema_proof = model
            .prove_struct(
                &a_identity,
                a.schema(),
                vec![Dynamic::from(1_i64)].into_boxed_slice(),
            )
            .unwrap();
        assert!(matches!(
            nominal::Struct::construct(
                &changed_model,
                &a_identity,
                changed_a.schema(),
                schema_proof,
            ),
            Err(construction::Error::SchemaMismatch { .. })
        ));
    }

    #[test]
    fn validator_rejects_wrong_arity_and_stale_nested_schema() {
        let program = resolve("struct Inner { value: i64 } struct Outer { inner: Inner } enum Event { Move(i64, bool) }");
        let model = Model::new(GenerationId::issue(), &program);
        let (event, event_item) = enum_definition(&program, "Event");
        let event_identity = identity(event);
        let move_variant = variant::Id::from_name(&event_item.variants[0].name);
        assert!(matches!(
            model.prove_enum(
                &event_identity,
                event.schema(),
                &move_variant,
                vec![Dynamic::from(1_i64)].into_boxed_slice(),
            ),
            Err(construction::Error::InvalidArity {
                expected: 2,
                actual: 1,
                ..
            })
        ));

        let old_inner = struct_definition(&program, "Inner").0;
        let old_identity = identity(old_inner);
        let inner_proof = model
            .prove_struct(
                &old_identity,
                old_inner.schema(),
                vec![Dynamic::from(1_i64)].into_boxed_slice(),
            )
            .unwrap();
        let inner: Dynamic =
            nominal::Struct::construct(&model, &old_identity, old_inner.schema(), inner_proof)
                .unwrap()
                .into();

        let changed = resolve("struct Inner { value: bool } struct Outer { inner: Inner }");
        let changed_model = Model::new(GenerationId::issue(), &changed);
        let outer = struct_definition(&changed, "Outer").0;
        let outer_identity = identity(outer);
        assert!(matches!(
            changed_model.prove_struct(
                &outer_identity,
                outer.schema(),
                vec![inner].into_boxed_slice(),
            ),
            Err(construction::Error::Member { source, .. })
                if matches!(*source, construction::Error::SchemaMismatch { .. })
        ));
    }

    #[test]
    fn default_materialization_uses_the_current_nominal_schema_recursively() {
        let program = resolve(
            "#[derive(Default)] struct Inner { enabled: bool } #[derive(Default)] struct Outer { count: i64, inner: Inner }",
        );
        let model = Model::new(GenerationId::issue(), &program);
        let outer = struct_definition(&program, "Outer").0;
        let value = model
            .materialize_default(&identity(outer), outer.schema())
            .unwrap();
        let outer_value = value.read_lock::<nominal::Struct>().unwrap();

        assert_eq!(outer_value.identity(), &identity(outer));
        assert_eq!(outer_value.schema(), outer.schema());
        assert_eq!(outer_value.fields()[0].as_int().unwrap(), 0);
        let inner_value = outer_value.fields()[1]
            .read_lock::<nominal::Struct>()
            .unwrap();
        assert!(!inner_value.fields()[0].as_bool().unwrap());
    }

    #[test]
    fn default_materialization_rejects_types_without_the_capability() {
        let program = resolve("struct State { value: i64 }");
        let model = Model::new(GenerationId::issue(), &program);
        let state = struct_definition(&program, "State").0;

        assert!(matches!(
            model.materialize_default(&identity(state), state.schema()),
            Err(construction::Error::MissingDefault { .. })
        ));
    }

    fn resolve(source: &str) -> resolve::Program {
        let engine = Engine::new();
        resolve::resolve(engine.parse_unit(source).unwrap(), &host::Registry::new()).unwrap()
    }

    fn struct_definition<'a>(
        program: &'a resolve::Program,
        name: &str,
    ) -> (&'a type_def::Definition, &'a item::Struct) {
        let definition = definition(program, name);
        let shell = program
            .items()
            .iter()
            .find(|shell| shell.key.as_ref().is_some_and(|key| key.name() == name))
            .unwrap();
        let item::Kind::Struct(item) = &shell.kind else {
            panic!("expected struct");
        };
        (definition, item)
    }

    fn enum_definition<'a>(
        program: &'a resolve::Program,
        name: &str,
    ) -> (&'a type_def::Definition, &'a item::Enum) {
        let definition = definition(program, name);
        let shell = program
            .items()
            .iter()
            .find(|shell| shell.key.as_ref().is_some_and(|key| key.name() == name))
            .unwrap();
        let item::Kind::Enum(item) = &shell.kind else {
            panic!("expected enum");
        };
        (definition, item)
    }

    fn definition<'a>(program: &'a resolve::Program, name: &str) -> &'a type_def::Definition {
        program
            .types()
            .iter()
            .find(|definition| definition.name() == name)
            .unwrap()
    }

    fn identity(definition: &type_def::Definition) -> type_def::Identity {
        type_def::Identity::Script(definition.key().clone())
    }
}
