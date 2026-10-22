//! Full typed transport into the pinned reference's field and variant format.

use serde_json::{json, Value};

use crate::input::membership::manifest::reference;
use crate::input::membership::manifest::schema::{
    dependency, hints, lints, package, platform, profile, workspace,
};

use super::super::{maps, Field, Original};

const FIELDS: [Field; 22] = [
    Field::CargoFeatures,
    Field::Package,
    Field::Project,
    Field::Badges,
    Field::Features,
    Field::Lib,
    Field::Bin,
    Field::Example,
    Field::Test,
    Field::Bench,
    Field::Dependencies,
    Field::DevDependencies,
    Field::DevDependencies2,
    Field::BuildDependencies,
    Field::BuildDependencies2,
    Field::Target,
    Field::Lints,
    Field::Hints,
    Field::Workspace,
    Field::Profile,
    Field::Patch,
    Field::Replace,
];

/// Cargo's caller fills `_unused_keys` from callbacks after typed success.
pub(super) trait Project {
    fn project(&self, context: &str) -> Value;
}

impl Project for Original<'_> {
    fn project(&self, context: &str) -> Value {
        let mut fields: serde_json::Map<_, _> = FIELDS
            .into_iter()
            .map(|field| (name(field).to_owned(), option(None)))
            .collect();
        assert_eq!(
            fields.len(),
            FIELDS.len(),
            "{context}: distinct field identities"
        );
        for entry in self.entries() {
            let field = name(entry.field());
            assert_eq!(fields[field], option(None), "{context}: duplicate {field}");
            let value = match entry.value() {
                super::super::Value::Primitive(value) => package::tests::pinned::primitive(value),
                super::super::Value::Package(value) => package::tests::pinned::package(value),
                super::super::Value::Features(value) => features(value),
                super::super::Value::Target(value) => reference::target_projection(value, context),
                super::super::Value::Targets(value) => targets(value, context),
                super::super::Value::Dependencies(value) => {
                    dependency::reference::inheritable_map(value)
                }
                super::super::Value::Platforms(value) => platforms(value),
                super::super::Value::Lints(value) => inherited_lints(value, context),
                super::super::Value::Hints(value) => hints(value, context),
                super::super::Value::Workspace(value) => {
                    workspace::tests::pinned::projection(value, context)
                }
                super::super::Value::Profiles(value) => profile::tree::reference::profiles(value),
                super::super::Value::Patch(value) => patch(value),
                super::super::Value::Replace(value) => replace(value),
            };
            fields.insert(field.to_owned(), option(Some(value)));
        }
        fields.insert(
            "_unused_keys".to_owned(),
            json!(self.unused().unique().collect::<Vec<_>>()),
        );
        json!({"type":"TomlManifest", "fields":fields})
    }
}

fn name(field: Field) -> &'static str {
    match field {
        Field::CargoFeatures => "cargo_features",
        Field::Package => "package",
        Field::Project => "project",
        Field::Badges => "badges",
        Field::Features => "features",
        Field::Lib => "lib",
        Field::Bin => "bin",
        Field::Example => "example",
        Field::Test => "test",
        Field::Bench => "bench",
        Field::Dependencies => "dependencies",
        Field::DevDependencies => "dev_dependencies",
        Field::DevDependencies2 => "dev_dependencies2",
        Field::BuildDependencies => "build_dependencies",
        Field::BuildDependencies2 => "build_dependencies2",
        Field::Target => "target",
        Field::Lints => "lints",
        Field::Hints => "hints",
        Field::Workspace => "workspace",
        Field::Profile => "profile",
        Field::Patch => "patch",
        Field::Replace => "replace",
    }
}

pub(super) fn option(value: Option<Value>) -> Value {
    match value {
        Some(value) => reference::tagged("Some", value),
        None => reference::tagged("None", Value::Null),
    }
}

fn features(value: &maps::Features<'_>) -> Value {
    reference::tagged(
        "BTreeMap",
        Value::Array(
            value
                .entries()
                .map(|entry| {
                    json!([
                        reference::tagged("FeatureName", json!(entry.key().as_str())),
                        entry
                            .value()
                            .as_slice()
                            .iter()
                            .map(|text| text.as_str())
                            .collect::<Vec<_>>()
                    ])
                })
                .collect(),
        ),
    )
}

fn targets(value: &maps::Targets<'_>, context: &str) -> Value {
    Value::Array(
        value
            .entries()
            .iter()
            .map(|entry| reference::target_projection(entry.target(), context))
            .collect(),
    )
}

fn platforms(value: &maps::Platforms<'_>) -> Value {
    reference::tagged(
        "BTreeMap",
        Value::Array(
            value
                .entries()
                .map(|entry| {
                    let fields: serde_json::Map<_, _> = [
                        (platform::Field::Dependencies, "dependencies"),
                        (platform::Field::BuildDependencies, "build_dependencies"),
                        (platform::Field::BuildDependencies2, "build_dependencies2"),
                        (platform::Field::DevDependencies, "dev_dependencies"),
                        (platform::Field::DevDependencies2, "dev_dependencies2"),
                    ]
                    .into_iter()
                    .map(|(field, name)| {
                        (
                            name.to_owned(),
                            option(
                                entry
                                    .value()
                                    .get(field)
                                    .map(dependency::reference::inheritable_map),
                            ),
                        )
                    })
                    .collect();
                    json!([entry.origin().text.as_str(), {"type":"TomlPlatform", "fields":fields}])
                })
                .collect(),
        ),
    )
}

fn inherited_lints(value: &lints::Inherited<'_>, context: &str) -> Value {
    json!({"type":"InheritableLints", "fields":{
        "workspace":value.workspace(),
        "lints":lints::tests::reference::table(value.table(), context),
    }})
}

fn hints(value: &hints::Hints<'_>, context: &str) -> Value {
    json!({"type":"Hints", "fields":{
        "mostly_unused":option(value.mostly_unused().map(|value| {
            reference::generic_projection(value, 0, context)
        })),
    }})
}

fn patch(value: &maps::Patch<'_>) -> Value {
    reference::tagged(
        "BTreeMap",
        Value::Array(
            value
                .entries()
                .map(|entry| {
                    json!([
                        entry.origin().text.as_str(),
                        dependency::reference::direct_map(entry.value())
                    ])
                })
                .collect(),
        ),
    )
}

fn replace(value: &maps::Replace<'_>) -> Value {
    reference::tagged(
        "BTreeMap",
        Value::Array(
            value
                .entries()
                .map(|entry| {
                    json!([
                        entry.origin().text.as_str(),
                        dependency::reference::direct_value(entry.value())
                    ])
                })
                .collect(),
        ),
    )
}
