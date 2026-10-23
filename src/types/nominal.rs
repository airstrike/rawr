//! Runtime values for resolved Rawr structs and enums.

pub(crate) mod variant;

use crate::resolve::type_ref;
use crate::types::dynamic::Union;
use crate::{program, type_def, Dynamic};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// An ordered runtime value of a resolved Rawr struct type.
#[derive(Clone)]
pub(crate) struct Struct {
    identity: type_def::Identity,
    schema: program::schema::Fingerprint,
    fields: Box<[Dynamic]>,
}

/// An ordered runtime value of a resolved Rawr enum variant.
#[derive(Clone)]
pub(crate) struct Enum {
    identity: type_def::Identity,
    schema: program::schema::Fingerprint,
    variant: variant::Id,
    payload: Box<[Dynamic]>,
}

impl fmt::Display for Struct {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "<Rawr struct {}>", self.identity_name())
    }
}

impl fmt::Debug for Struct {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl Struct {
    /// Consume exact owned fields proven by this executable's runtime model.
    pub(crate) fn construct(
        model: &program::model::Model,
        identity: &type_def::Identity,
        schema: &program::schema::Fingerprint,
        proof: program::model::Construction,
    ) -> Result<Self, program::construction::Error> {
        let (identity, schema, fields) = proof.into_struct(model, identity, schema)?;
        Ok(Self {
            identity,
            schema,
            fields,
        })
    }

    pub(crate) const fn identity(&self) -> &type_def::Identity {
        &self.identity
    }

    pub(crate) const fn schema(&self) -> &program::schema::Fingerprint {
        &self.schema
    }

    pub(crate) const fn fields(&self) -> &[Dynamic] {
        &self.fields
    }

    pub(crate) fn fields_mut(&mut self) -> &mut [Dynamic] {
        &mut self.fields
    }

    fn identity_name(&self) -> IdentityName<'_> {
        IdentityName(&self.identity)
    }
}

impl fmt::Display for Enum {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "<Rawr enum {}::{}>",
            self.identity_name(),
            self.variant
        )
    }
}

impl fmt::Debug for Enum {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl Enum {
    /// Consume an exact owned payload proven for one stable enum variant.
    pub(crate) fn construct(
        model: &program::model::Model,
        identity: &type_def::Identity,
        schema: &program::schema::Fingerprint,
        variant: &variant::Id,
        proof: program::model::Construction,
    ) -> Result<Self, program::construction::Error> {
        let (identity, schema, variant, payload) =
            proof.into_enum(model, identity, schema, variant)?;
        Ok(Self {
            identity,
            schema,
            variant,
            payload,
        })
    }

    pub(crate) const fn identity(&self) -> &type_def::Identity {
        &self.identity
    }

    pub(crate) const fn schema(&self) -> &program::schema::Fingerprint {
        &self.schema
    }

    pub(crate) const fn variant(&self) -> &variant::Id {
        &self.variant
    }

    pub(crate) const fn payload(&self) -> &[Dynamic] {
        &self.payload
    }

    pub(crate) fn payload_mut(&mut self) -> &mut [Dynamic] {
        &mut self.payload
    }

    fn identity_name(&self) -> IdentityName<'_> {
        IdentityName(&self.identity)
    }
}

pub(crate) fn matches_type(value: &Dynamic, ty: &type_ref::Type) -> bool {
    if let Some(element) = ty.option_element() {
        return match crate::option::read(value) {
            Some(Some(inner)) => matches_type(&inner, element),
            Some(None) => true,
            None => false,
        };
    }
    if let Some(element) = ty.vec_element() {
        return value
            .as_array_ref()
            .is_ok_and(|array| array.iter().all(|value| matches_type(value, element)));
    }
    if let Some((element, length)) = ty.array_element() {
        return value.as_array_ref().is_ok_and(|array| {
            array.len() == length && array.iter().all(|value| matches_type(value, element))
        });
    }
    // An iterator crossing a function boundary is the array of its items.
    if let Some((item, _)) = ty.iterator_item() {
        return value
            .as_array_ref()
            .is_ok_and(|array| array.iter().all(|value| matches_type(value, item)));
    }
    // A map is its `[key, value]` pairs at runtime, a set its keys.
    if let Some((_, arguments)) = ty.keyed_arguments() {
        return value.as_array_ref().is_ok_and(|array| {
            array.iter().all(|entry| match &arguments[..] {
                [key, value] => entry.as_array_ref().is_ok_and(|pair| {
                    pair.len() == 2 && matches_type(&pair[0], key) && matches_type(&pair[1], value)
                }),
                [key] => matches_type(entry, key),
                _ => false,
            })
        });
    }
    if let Some(inner) = ty.reverse_element() {
        return value
            .read_lock::<std::cmp::Reverse<Dynamic>>()
            .is_some_and(|reverse| matches_type(&reverse.0, inner));
    }
    // A box is its value at runtime.
    if let Some(element) = ty.box_element() {
        return matches_type(value, element);
    }
    // A trait object is any value of a type implementing the trait, which
    // was checked where it was made.
    if ty.dyn_trait().is_some() {
        return true;
    }
    // A function value: its signature was checked where it was made.
    if ty.fn_signature().is_some() {
        return value.is_fnptr();
    }
    if let Some((ok, err)) = ty.result_elements() {
        return match crate::result::read(value) {
            Some(Ok(inner)) => matches_type(&inner, ok),
            Some(Err(inner)) => matches_type(&inner, err),
            None => false,
        };
    }
    match ty {
        type_ref::Type::Unit(_) => value.is_unit(),
        // A tuple is an array at runtime.
        type_ref::Type::Tuple { members, .. } => value.as_array_ref().is_ok_and(|array| {
            array.len() == members.len()
                && array
                    .iter()
                    .zip(members.iter())
                    .all(|(value, member)| matches_type(value, member))
        }),
        type_ref::Type::Reference { .. } => false,
        type_ref::Type::Path(path) => match &path.target {
            type_ref::Target::Builtin => match crate::host::runtime_builtin(&path.name) {
                "bool" => value.is_bool(),
                "char" => value.is_char(),
                // `std::cmp::Ordering` is its discriminant at runtime.
                "i64" | "Ordering" => value.is_int(),
                // `std::time`'s types are Rust's at runtime.
                "Duration" => value.is::<std::time::Duration>(),
                "Instant" => value.is::<std::time::Instant>(),
                "String" => value.is_string(),
                #[cfg(not(feature = "no_float"))]
                "f64" => value.is_float(),
                #[cfg(feature = "no_float")]
                "f64" => false,
                "RangeInclusive" => {
                    value.is::<std::ops::RangeInclusive<crate::INT>>() || {
                        #[cfg(not(feature = "no_float"))]
                        {
                            value.is::<std::ops::RangeInclusive<crate::FLOAT>>()
                        }
                        #[cfg(feature = "no_float")]
                        false
                    }
                }
                // A `Range<T>` is an iterator in Rawr.
                name if name == crate::item::type_ref::ITER_TYPE => true,
                _ => false,
            },
            // An imported Rust value is a handle the generated runner
            // names, whose type its bridge checks where the value is used.
            type_ref::Target::Host { definition, .. } => definition
                .native_identity()
                .map_or(definition.identity().imported().is_some(), |native| {
                    native == value.type_id()
                }),
            // A generic parameter and its associated types are erased:
            // any value.
            type_ref::Target::Parameter | type_ref::Target::Projection(..) | type_ref::Target::Alias(_) => true,
            type_ref::Target::Script { identity, .. } => match &value.0 {
                Union::Struct(value, ..) => value.identity() == identity,
                Union::Enum(value, ..) => value.identity() == identity,
                _ => false,
            },
        },
    }
}

struct IdentityName<'a>(&'a type_def::Identity);

impl fmt::Display for IdentityName<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            type_def::Identity::Host(id) => write!(formatter, "host::{id:?}"),
            type_def::Identity::Script(key) => fmt::Display::fmt(key, formatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Enum, Struct};
    use crate::resolve::item;
    use crate::types::dynamic::Union;
    use crate::{host, resolve, type_def, Dynamic, Engine};

    #[test]
    fn struct_value_preserves_identity_schema_and_field_order() {
        let (model, definition, _) =
            struct_model("struct Counter { value: i64, label: String }", "Counter");
        let value = construct_struct(
            &model,
            &definition,
            vec![Dynamic::from(42_i64), Dynamic::from("answer")].into_boxed_slice(),
        );

        assert_eq!(
            value.identity(),
            &type_def::Identity::Script(definition.key().clone())
        );
        assert_eq!(value.schema(), definition.schema());
        assert_eq!(value.fields()[0].as_int().unwrap(), 42);
        assert_eq!(value.fields()[1].clone().into_string().unwrap(), "answer");
    }

    #[test]
    fn enum_value_preserves_stable_variant_and_payload_order() {
        let (model, definition, item) = enum_model("enum Message { Move(i64, bool) }", "Message");
        let value = construct_enum(
            &model,
            &definition,
            &item,
            0,
            vec![Dynamic::from(7_i64), Dynamic::from(true)].into_boxed_slice(),
        );

        assert_eq!(value.variant().name(), "Move");
        assert_eq!(value.payload()[0].as_int().unwrap(), 7);
        assert!(value.payload()[1].as_bool().unwrap());
    }

    #[test]
    fn dynamic_clone_preserves_nominal_proofs_and_owned_values() {
        let (struct_model, struct_definition, _) =
            struct_model("struct Counter { value: i64 }", "Counter");
        let (enum_model, enum_definition, enum_item) =
            enum_model("enum Message { Move(i64) }", "Message");
        let struct_value: Dynamic = construct_struct(
            &struct_model,
            &struct_definition,
            vec![Dynamic::from(3_i64)].into_boxed_slice(),
        )
        .into();
        let enum_value: Dynamic = construct_enum(
            &enum_model,
            &enum_definition,
            &enum_item,
            0,
            vec![Dynamic::from(5_i64)].into_boxed_slice(),
        )
        .into();

        let struct_clone = struct_value.clone();
        let enum_clone = enum_value.clone();

        let Union::Struct(value, ..) = struct_clone.0 else {
            panic!("expected nominal struct");
        };
        assert_eq!(
            value.identity(),
            &type_def::Identity::Script(struct_definition.key().clone())
        );
        assert_eq!(value.schema(), struct_definition.schema());
        assert_eq!(value.fields()[0].as_int().unwrap(), 3);

        let Union::Enum(value, ..) = enum_clone.0 else {
            panic!("expected nominal enum");
        };
        assert_eq!(
            value.identity(),
            &type_def::Identity::Script(enum_definition.key().clone())
        );
        assert_eq!(value.schema(), enum_definition.schema());
        assert_eq!(value.variant().name(), "Move");
        assert_eq!(value.payload()[0].as_int().unwrap(), 5);
    }

    #[test]
    fn generic_dynamic_construction_preserves_nominal_branches() {
        let (struct_model, struct_definition, _) =
            struct_model("struct Counter { value: i64 }", "Counter");
        let (enum_model, enum_definition, enum_item) =
            enum_model("enum Message { Move(i64) }", "Message");
        let struct_value = construct_struct(
            &struct_model,
            &struct_definition,
            vec![Dynamic::from(3_i64)].into_boxed_slice(),
        );
        let enum_value = construct_enum(
            &enum_model,
            &enum_definition,
            &enum_item,
            0,
            vec![Dynamic::from(5_i64)].into_boxed_slice(),
        );

        let generic_struct = Dynamic::from(struct_value);
        let generic_enum = Dynamic::from(enum_value);

        let Union::Struct(value, ..) = generic_struct.0 else {
            panic!("generic construction demoted a nominal struct to a custom Rust value");
        };
        assert_eq!(
            value.identity(),
            &type_def::Identity::Script(struct_definition.key().clone())
        );
        assert_eq!(value.schema(), struct_definition.schema());

        let Union::Enum(value, ..) = generic_enum.0 else {
            panic!("generic construction demoted a nominal enum to a custom Rust value");
        };
        assert_eq!(
            value.identity(),
            &type_def::Identity::Script(enum_definition.key().clone())
        );
        assert_eq!(value.schema(), enum_definition.schema());
        assert_eq!(value.variant().name(), "Move");
    }

    #[test]
    fn identity_and_schema_mismatches_are_independent_checks() {
        let (counter_model, counter, _) = struct_model("struct Counter { value: i64 }", "Counter");
        let (_, changed_counter, _) = struct_model("struct Counter { value: bool }", "Counter");
        let (_, other, _) = struct_model("struct Other { value: i64 }", "Other");
        let value = construct_struct(
            &counter_model,
            &counter,
            vec![Dynamic::from(0_i64)].into_boxed_slice(),
        );

        assert_eq!(counter.key(), changed_counter.key());
        assert_ne!(counter.schema(), changed_counter.schema());
        assert_ne!(counter.key(), other.key());
        assert_eq!(
            value.identity(),
            &type_def::Identity::Script(changed_counter.key().clone())
        );
        assert_ne!(value.schema(), changed_counter.schema());
        assert_ne!(
            value.identity(),
            &type_def::Identity::Script(other.key().clone())
        );
    }

    #[test]
    fn nominal_values_have_explicit_opaque_dynamic_semantics() {
        let (model, definition, _) = struct_model("struct Counter { value: i64 }", "Counter");
        let value: Dynamic = construct_struct(
            &model,
            &definition,
            vec![Dynamic::from(1_i64)].into_boxed_slice(),
        )
        .into();

        assert_eq!(value.type_name(), "Rawr struct");
        assert_eq!(value.to_string(), "<Rawr struct main::Counter>");
        assert_eq!(format!("{value:?}"), "<Rawr struct main::Counter>");
        assert!(!value.is_variant());
        assert!(!value.is_hashable());
    }

    #[test]
    fn definition_kind_is_proven_before_runtime_value_construction() {
        let (struct_model, struct_definition, _) =
            struct_model("struct Counter { value: i64 }", "Counter");
        let (enum_model, enum_definition, enum_item) =
            enum_model("enum Message { Tick }", "Message");
        let struct_identity = type_def::Identity::Script(struct_definition.key().clone());
        let enum_identity = type_def::Identity::Script(enum_definition.key().clone());
        let enum_variant = super::variant::Id::from_name(&enum_item.variants[0].name);

        assert!(matches!(
            enum_model.prove_struct(&enum_identity, enum_definition.schema(), Box::default()),
            Err(crate::program::construction::Error::KindMismatch { .. })
        ));
        assert!(matches!(
            struct_model.prove_enum(
                &struct_identity,
                struct_definition.schema(),
                &enum_variant,
                Box::default(),
            ),
            Err(crate::program::construction::Error::KindMismatch { .. })
        ));
    }

    #[test]
    #[cfg(not(feature = "no_closure"))]
    fn recursive_shared_value_cycles_are_rejected_without_recursing_forever() {
        let (model, definition, _) = struct_model("struct Loop { next: Loop }", "Loop");
        let identity = type_def::Identity::Script(definition.key().clone());
        let malformed = Struct {
            identity: identity.clone(),
            schema: definition.schema().clone(),
            fields: vec![Dynamic::UNIT].into_boxed_slice(),
        };
        let mut shared = Dynamic::from(malformed).into_shared();
        let cycle = shared.clone();
        shared.write_lock::<Struct>().unwrap().fields_mut()[0] = cycle;

        let error = match model.prove_struct(
            &identity,
            definition.schema(),
            vec![shared].into_boxed_slice(),
        ) {
            Ok(_) => panic!("recursive cycle unexpectedly produced a construction proof"),
            Err(error) => error,
        };

        assert!(contains_cycle(&error));
    }

    #[test]
    fn nominal_branches_leave_custom_rust_values_opaque() {
        #[derive(Clone, Debug, Eq, PartialEq)]
        struct HostValue(i64);

        let value = Dynamic::from(HostValue(42));
        let clone = value.clone();

        assert!(value.is::<HostValue>());
        assert!(value.is_variant());
        assert_eq!(clone.cast::<HostValue>(), HostValue(42));
    }

    fn struct_model(
        source: &str,
        name: &str,
    ) -> (
        crate::program::model::Model,
        type_def::Definition,
        item::Struct,
    ) {
        let (model, definition, kind) = model(source, name);
        let item::Kind::Struct(item) = kind else {
            panic!("expected struct model");
        };
        (model, definition, item)
    }

    fn enum_model(
        source: &str,
        name: &str,
    ) -> (
        crate::program::model::Model,
        type_def::Definition,
        item::Enum,
    ) {
        let (model, definition, kind) = model(source, name);
        let item::Kind::Enum(item) = kind else {
            panic!("expected enum model");
        };
        (model, definition, item)
    }

    fn model(
        source: &str,
        name: &str,
    ) -> (
        crate::program::model::Model,
        type_def::Definition,
        item::Kind,
    ) {
        let engine = Engine::new();
        let program =
            resolve::resolve(engine.parse_unit(source).unwrap(), &host::Registry::new()).unwrap();
        let model =
            crate::program::model::Model::new(crate::program::GenerationId::issue(), &program);
        let definition = program
            .types()
            .iter()
            .find(|definition| definition.name() == name)
            .unwrap()
            .clone();
        let kind = program
            .items()
            .iter()
            .find(|shell| shell.key.as_ref().is_some_and(|key| key.name() == name))
            .unwrap()
            .kind
            .clone();
        (model, definition, kind)
    }

    fn construct_struct(
        model: &crate::program::model::Model,
        definition: &type_def::Definition,
        fields: Box<[Dynamic]>,
    ) -> Struct {
        let identity = type_def::Identity::Script(definition.key().clone());
        let proof = model
            .prove_struct(&identity, definition.schema(), fields)
            .unwrap();
        Struct::construct(model, &identity, definition.schema(), proof).unwrap()
    }

    fn construct_enum(
        model: &crate::program::model::Model,
        definition: &type_def::Definition,
        item: &item::Enum,
        variant: usize,
        payload: Box<[Dynamic]>,
    ) -> Enum {
        let identity = type_def::Identity::Script(definition.key().clone());
        let variant = super::variant::Id::from_name(&item.variants[variant].name);
        let proof = model
            .prove_enum(&identity, definition.schema(), &variant, payload)
            .unwrap();
        Enum::construct(model, &identity, definition.schema(), &variant, proof).unwrap()
    }

    fn contains_cycle(error: &crate::program::construction::Error) -> bool {
        match error {
            crate::program::construction::Error::Cycle { .. } => true,
            crate::program::construction::Error::Member { source, .. } => contains_cycle(source),
            _ => false,
        }
    }
}
