//! Test transport of actual converted dependencies into the committed pinned
//! Cargo reference's complete field/variant format. This is not another parser.

use serde_json::{json, Value};

use super::super::super::reference;
use super::super::primitive;
use super::{direct, inheritable, Direct, Entry, Field, Inheritable};

const DETAILED: [(Field, &str); 18] = [
    (Field::Version, "version"),
    (Field::Registry, "registry"),
    (Field::RegistryIndex, "registry_index"),
    (Field::Path, "path"),
    (Field::Base, "base"),
    (Field::Git, "git"),
    (Field::Branch, "branch"),
    (Field::Tag, "tag"),
    (Field::Rev, "rev"),
    (Field::Features, "features"),
    (Field::Optional, "optional"),
    (Field::DefaultFeatures, "default_features"),
    (Field::DefaultFeatures2, "default_features2"),
    (Field::Package, "package"),
    (Field::Public, "public"),
    (Field::Artifact, "artifact"),
    (Field::Lib, "lib"),
    (Field::Target, "target"),
];
const INHERITED: [(Field, &str); 6] = [
    (Field::Workspace, "workspace"),
    (Field::Features, "features"),
    (Field::DefaultFeatures, "default_features"),
    (Field::DefaultFeatures2, "default_features2"),
    (Field::Optional, "optional"),
    (Field::Public, "public"),
];

fn primitive(value: &primitive::Value<'_>, field: Field) -> Value {
    match value {
        primitive::Value::String(text) => json!(text.as_str()),
        primitive::Value::Boolean(value) => json!(value),
        primitive::Value::Strings(values) => {
            let values = json!(values
                .as_slice()
                .iter()
                .map(|text| text.as_str())
                .collect::<Vec<_>>());
            if field == Field::Artifact {
                reference::tagged("StringOrVec", values)
            } else {
                values
            }
        }
        primitive::Value::Name(name) => reference::tagged(
            match field {
                Field::Registry => "RegistryName",
                Field::Base => "PathBaseName",
                Field::Package => "PackageName",
                _ => panic!("unexpected dependency name field"),
            },
            json!(name.as_str()),
        ),
        _ => panic!("unexpected converted dependency primitive"),
    }
}

fn fields(entries: &[Entry<'_>], definitions: &[(Field, &str)]) -> serde_json::Map<String, Value> {
    definitions
        .iter()
        .map(|(field, name)| {
            let entry = entries.iter().find(|entry| entry.field() == *field);
            let value = match entry {
                Some(entry) if *field == Field::Workspace => primitive(entry.value(), *field),
                Some(entry) => reference::tagged("Some", primitive(entry.value(), *field)),
                None => reference::tagged("None", Value::Null),
            };
            ((*name).to_owned(), value)
        })
        .collect()
}

pub(in super::super::super) fn direct_value(value: &Direct<'_>) -> Value {
    match value.kind() {
        direct::Kind::Simple(text) => {
            reference::tagged("TomlDependency::Simple", json!(text.as_str()))
        }
        direct::Kind::Detailed(value) => {
            let mut fields = fields(value.fields(), &DETAILED);
            let extras: Vec<_> = value
                .extras()
                .map(|(key, value)| {
                    json!([
                        key.text.as_str(),
                        reference::generic_projection(value, 0, "dependency extra")
                    ])
                })
                .collect();
            fields.insert(
                "_unused_keys".to_owned(),
                reference::tagged("BTreeMap", json!(extras)),
            );
            reference::tagged(
                "TomlDependency::Detailed",
                json!({"type":"TomlDetailedDependency","fields":fields}),
            )
        }
    }
}

pub(in super::super::super) fn inheritable_value(value: &Inheritable<'_>) -> Value {
    match value.kind() {
        inheritable::Kind::Value(value) => {
            reference::tagged("InheritableDependency::Value", direct_value(value))
        }
        inheritable::Kind::Inherit(value) => {
            let mut fields = fields(value.fields(), &INHERITED);
            let extras: Vec<_> = value
                .extras()
                .map(|(key, value)| {
                    json!([
                        key.text.as_str(),
                        reference::generic_projection(value, 0, "inherited dependency extra")
                    ])
                })
                .collect();
            fields.insert(
                "_unused_keys".to_owned(),
                reference::tagged("BTreeMap", json!(extras)),
            );
            reference::tagged(
                "InheritableDependency::Inherit",
                json!({"type":"TomlInheritedDependency","fields":fields}),
            )
        }
    }
}

pub(in super::super::super) fn direct_map(value: &direct::Map<'_>) -> Value {
    reference::tagged(
        "BTreeMap",
        Value::Array(
            value
                .entries()
                .map(|(name, _, value)| {
                    json!([
                        reference::tagged("PackageName", json!(name.as_str())),
                        direct_value(value)
                    ])
                })
                .collect(),
        ),
    )
}

pub(in super::super::super) fn inheritable_map(value: &inheritable::Map<'_>) -> Value {
    reference::tagged(
        "BTreeMap",
        Value::Array(
            value
                .entries()
                .map(|(name, _, value)| {
                    json!([
                        reference::tagged("PackageName", json!(name.as_str())),
                        inheritable_value(value)
                    ])
                })
                .collect(),
        ),
    )
}
