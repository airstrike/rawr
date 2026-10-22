//! Test-only complete typed projection into the executed pinned harness format.

use serde_json::{json, Value};

use super::{Location, PackageSpec, Profile, Profiles, FIELDS};
use crate::input::membership::manifest::{
    reference,
    schema::{package, primitive, profile, trim},
    spec,
};

pub(in crate::input::membership::manifest) fn profiles(input: &Profiles<'_>) -> Value {
    let mut nodes: Vec<Option<Value>> = (0..input.nodes().len()).map(|_| None).collect();
    for (index, node) in input.nodes().iter().enumerate().rev() {
        nodes[index] = Some(project(node, &mut nodes));
    }
    let pairs: Vec<_> = input
        .roots
        .entries()
        .map(|(name, _, index)| {
            json!([
                reference::tagged("ProfileName", json!(name.as_str())),
                nodes[index].take().unwrap()
            ])
        })
        .collect();
    reference::tagged("TomlProfiles", reference::tagged("BTreeMap", json!(pairs)))
}

fn project(node: &Profile<'_>, nodes: &mut [Option<Value>]) -> Value {
    let mut fields = serde_json::Map::new();
    for (field, name) in FIELDS {
        let result = node.get(field).map(|value| match value {
            super::Value::Opt(value) => reference::tagged("TomlOptLevel", json!(value.as_str())),
            super::Value::Debug(value) => reference::tagged(
                match value {
                    profile::Debug::None => "TomlDebugInfo::None",
                    profile::Debug::LineDirectivesOnly => "TomlDebugInfo::LineDirectivesOnly",
                    profile::Debug::LineTablesOnly => "TomlDebugInfo::LineTablesOnly",
                    profile::Debug::Limited => "TomlDebugInfo::Limited",
                    profile::Debug::Full => "TomlDebugInfo::Full",
                },
                Value::Null,
            ),
            super::Value::Units(value) => json!(value),
            super::Value::Primitive(value) => match value {
                primitive::Value::String(value) => json!(value.as_str()),
                primitive::Value::Boolean(value) => json!(value),
                primitive::Value::Strings(values) => json!(values
                    .as_slice()
                    .iter()
                    .map(|value| value.as_str())
                    .collect::<Vec<_>>()),
                primitive::Value::StringOrBool(primitive::StringOrBool::String(value)) => {
                    reference::tagged("StringOrBool::String", json!(value.as_str()))
                }
                primitive::Value::StringOrBool(primitive::StringOrBool::Boolean(value)) => {
                    reference::tagged("StringOrBool::Bool", json!(value))
                }
                _ => panic!("profile primitive shape for {field:?}"),
            },
            super::Value::Trim(trim::Paths::All) => {
                reference::tagged("TomlTrimPaths::All", Value::Null)
            }
            super::Value::Trim(trim::Paths::Values(values)) => reference::tagged(
                "TomlTrimPaths::Values",
                json!(values
                    .as_slice()
                    .iter()
                    .map(|value| reference::tagged(
                        match value {
                            trim::Trim::Diagnostics => "TomlTrimPathsValue::Diagnostics",
                            trim::Trim::Macro => "TomlTrimPathsValue::Macro",
                            trim::Trim::Object => "TomlTrimPathsValue::Object",
                        },
                        Value::Null
                    ))
                    .collect::<Vec<_>>()),
            ),
            super::Value::BuildOverride(index) => nodes[*index].take().unwrap(),
            super::Value::Package(packages) => reference::tagged(
                "BTreeMap",
                json!(packages
                    .entries()
                    .map(|(key, _, index)| {
                        json!([package_spec(key), nodes[index].take().unwrap()])
                    })
                    .collect::<Vec<_>>()),
            ),
        });
        fields.insert(name.replace('-', "_"), optional(result));
    }
    json!({"type":"TomlProfile", "fields":fields})
}

fn optional(value: Option<Value>) -> Value {
    match value {
        Some(value) => reference::tagged("Some", value),
        None => reference::tagged("None", Value::Null),
    }
}

fn raw_option(value: &Value) -> Value {
    optional((!value.is_null()).then(|| value.clone()))
}

fn package_spec(value: &PackageSpec) -> Value {
    let PackageSpec::Spec(value) = value else {
        return reference::tagged("ProfilePackageSpec::All", Value::Null);
    };
    let raw = spec::tests::project(value);
    let version = (!raw["version"].is_null()).then(|| {
        let version = &raw["version"];
        json!({"type":"PartialVersion", "major":version["major"], "minor":raw_option(&version["minor"]),
            "patch":raw_option(&version["patch"]), "pre":raw_option(&version["pre"]), "build":raw_option(&version["build"])})
    });
    let url = (!raw["url"].is_null()).then(|| reference::tagged("Url", raw["url"].clone()));
    let kind = (!raw["kind"].is_null()).then(|| {
        let kind = &raw["kind"];
        match kind["type"].as_str().unwrap() {
            "path" => reference::tagged("SourceKind::Path", Value::Null),
            "registry" => reference::tagged("SourceKind::Registry", Value::Null),
            "sparse" => reference::tagged("SourceKind::SparseRegistry", Value::Null),
            "git" => {
                let reference = &kind["reference"];
                let reference = match reference["kind"].as_str().unwrap() {
                    "tag" => reference::tagged("GitReference::Tag", reference["value"].clone()),
                    "branch" => {
                        reference::tagged("GitReference::Branch", reference["value"].clone())
                    }
                    "rev" => reference::tagged("GitReference::Rev", reference["value"].clone()),
                    "default" => reference::tagged("GitReference::DefaultBranch", Value::Null),
                    _ => panic!("reference kind"),
                };
                reference::tagged("SourceKind::Git", reference)
            }
            _ => panic!("source kind"),
        }
    });
    reference::tagged(
        "ProfilePackageSpec::Spec",
        json!({"type":"PackageIdSpec", "name":raw["name"],
        "version":optional(version), "url":optional(url), "kind":optional(kind)}),
    )
}

pub(in crate::input::membership::manifest) fn ignored(
    input: &Profiles<'_>,
    prefix: &str,
) -> Vec<String> {
    input
        .ignored()
        .map(|(mut index, key)| {
            let mut parts = vec![key.text.as_str().to_owned()];
            loop {
                match input.node(index).location() {
                    Location::Root(key) => {
                        parts.push(key.text.as_str().to_owned());
                        break;
                    }
                    Location::Package {
                        parent,
                        position,
                        key,
                    } => {
                        parts.push(key.text.as_str().to_owned());
                        parts.push(position_text(position));
                        index = parent;
                    }
                    Location::BuildOverride { parent, position } => {
                        parts.push(position_text(position));
                        index = parent;
                    }
                }
            }
            if !prefix.is_empty() {
                parts.push(prefix.to_owned());
            }
            parts.reverse();
            parts.join(".")
        })
        .collect()
}

fn position_text(position: package::Position<'_>) -> String {
    match position {
        package::Position::Map(key) => key.text.as_str().to_owned(),
        package::Position::Sequence(index) => index.to_string(),
    }
}
