//! Complete package/default projections from the committed130-case Cargo run.

use serde_json::{json, Value};

use super::super::super::super::{reference, session, value, Error, Tag};
use super::super::super::primitive;
use super::super::{Definition, Entry, Shape, DEFAULTS, PACKAGE};
use super::{Defaults, Package};

pub(in crate::input::membership::manifest) fn primitive(value: &primitive::Value<'_>) -> Value {
    match value {
        primitive::Value::String(text) => json!(text.as_str()),
        primitive::Value::Boolean(value) => json!(value),
        primitive::Value::Strings(values) => json!(values
            .as_slice()
            .iter()
            .map(|text| text.as_str())
            .collect::<Vec<_>>()),
        primitive::Value::Version(value) => {
            json!({"type":"Version", "major":value.major(),"minor":value.minor(),"patch":value.patch(),"pre":value.pre(),"build":value.build()})
        }
        primitive::Value::Rust(value) => reference::tagged(
            "RustVersion",
            json!({
                "type":"PartialVersion", "major":value.major(),
                "minor":option(value.minor().map(|value| json!(value))),
                "patch":option(value.patch().map(|value| json!(value))),
                "pre":option(None), "build":option(None),
            }),
        ),
        primitive::Value::Name(name) => reference::tagged("PackageName", json!(name.as_str())),
        primitive::Value::StringOrBool(primitive::StringOrBool::String(text)) => {
            reference::tagged("StringOrBool::String", json!(text.as_str()))
        }
        primitive::Value::StringOrBool(primitive::StringOrBool::Boolean(value)) => {
            reference::tagged("StringOrBool::Bool", json!(value))
        }
        primitive::Value::StringsOrBool(primitive::StringsOrBool::Boolean(value)) => {
            reference::tagged("VecStringOrBool::Bool", json!(value))
        }
        primitive::Value::StringsOrBool(primitive::StringsOrBool::Strings(values)) => {
            reference::tagged(
                "VecStringOrBool::VecString",
                json!(values
                    .as_slice()
                    .iter()
                    .map(|text| text.as_str())
                    .collect::<Vec<_>>()),
            )
        }
        primitive::Value::Build(primitive::Build::Auto(value)) => {
            reference::tagged("TomlPackageBuild::Auto", json!(value))
        }
        primitive::Value::Build(primitive::Build::Single(text)) => {
            reference::tagged("TomlPackageBuild::SingleScript", json!(text.as_str()))
        }
        primitive::Value::Build(primitive::Build::Multiple(values)) => reference::tagged(
            "TomlPackageBuild::MultipleScript",
            json!(values
                .as_slice()
                .iter()
                .map(|text| text.as_str())
                .collect::<Vec<_>>()),
        ),
        primitive::Value::Badges(badges) => reference::tagged(
            "BTreeMap",
            Value::Array(
                badges
                    .entries()
                    .iter()
                    .map(|badge| {
                        json!([
                            badge.key().text.as_str(),
                            reference::tagged(
                                "BTreeMap",
                                Value::Array(
                                    badge
                                        .values()
                                        .iter()
                                        .map(|pair| json!([
                                            pair.key().text.as_str(),
                                            pair.text().as_str()
                                        ]))
                                        .collect()
                                )
                            )
                        ])
                    })
                    .collect(),
            ),
        ),
    }
}

fn option(value: Option<Value>) -> Value {
    match value {
        Some(value) => reference::tagged("Some", value),
        None => reference::tagged("None", Value::Null),
    }
}

fn projection(entries: &[Entry<'_>], definitions: &[Definition], kind: &str) -> Value {
    let mut fields = serde_json::Map::new();
    for definition in definitions {
        let name = if matches!(definition.shape, Shape::Forbidden) {
            "_invalid_cargo_features".to_owned()
        } else {
            definition.name.replace('-', "_")
        };
        let value = entries
            .iter()
            .find(|entry| entry.field() == definition.field)
            .map(|entry| match entry.value() {
                super::super::Value::Inherited => {
                    reference::tagged("InheritableField::Inherit", json!({"workspace":true}))
                }
                super::super::Value::Metadata(value) => {
                    reference::generic_projection(value, 0, kind)
                }
                super::super::Value::Local(value) => {
                    let value = primitive(value);
                    match definition.shape {
                        Shape::Inheritable(_) => {
                            reference::tagged("InheritableField::Value", value)
                        }
                        Shape::Value(primitive::Shape::StringOrStrings) => {
                            reference::tagged("StringOrVec", value)
                        }
                        _ => value,
                    }
                }
            });
        fields.insert(name, option(value));
    }
    json!({"type":kind,"fields":fields})
}

pub(in crate::input::membership::manifest) fn package(value: &Package<'_>) -> Value {
    projection(value.entries(), &PACKAGE, "TomlPackage")
}

#[test]
fn all_package_fields_and_workspace_defaults_match_the_actual_cargo_projection() {
    let mut packages = 0;
    let mut defaults = 0;
    for row in reference::observations() {
        if row["accepted"] != true {
            continue;
        }
        let mut account = reference::Account;
        let document = reference::parse(&row, &mut account);
        for name in ["package", "project"] {
            let Some(expected) =
                reference::some(&row["typed"]["fields"][name], reference::id(&row))
            else {
                continue;
            };
            let input = document.table().get(name).unwrap();
            let mut session = session::Session::new(&mut account);
            let result = Package::interpret(value::Input::raw(input), &mut session);
            let package = session
                .finish(result)
                .unwrap_or_else(|error| panic!("{}: {error:?}", reference::id(&row)));
            assert_eq!(
                projection(package.entries(), &PACKAGE, "TomlPackage"),
                *expected,
                "{}: {name}",
                reference::id(&row)
            );
            let mut ignored: Vec<_> = package
                .ignored()
                .iter()
                .map(|ignored| {
                    let parent = match ignored.parent() {
                        None => name.to_owned(),
                        Some(super::super::Position::Map(key)) => {
                            format!("{name}.{}", key.text.as_str())
                        }
                        Some(super::super::Position::Sequence(index)) => format!("{name}.{index}"),
                    };
                    format!("{parent}.{}", ignored.key().text.as_str())
                })
                .collect();
            ignored.sort();
            ignored.dedup();
            let expected: Vec<_> = row["observed_unused"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|item| item.as_str())
                .filter(|item| item.starts_with(&format!("{name}.")))
                .collect();
            assert_eq!(ignored, expected, "{}: ignored paths", reference::id(&row));
            packages += 1;
        }
        if let Some(workspace) =
            reference::some(&row["typed"]["fields"]["workspace"], reference::id(&row))
            && let Some(expected) =
                reference::some(&workspace["fields"]["package"], reference::id(&row))
        {
            let input = reference::table(document.table().get("workspace").unwrap(), "workspace")
                .get("package")
                .unwrap();
            let mut session = session::Session::new(&mut account);
            let result = Defaults::interpret(value::Input::raw(input), &mut session);
            let value = session.finish(result).unwrap();
            assert_eq!(
                projection(value.entries(), &DEFAULTS, "InheritablePackage"),
                *expected,
                "{}: workspace.package",
                reference::id(&row)
            );
            defaults += 1;
        }
    }
    assert_eq!(
        packages, 23,
        "review package/project corpus coverage changes explicitly"
    );
    assert_eq!(defaults, 1);
}

#[test]
fn scoped_package_failures_match_reference_tag_and_span() {
    for (id, expected) in [
        ("inherit-field-false", Tag::WorkspaceFalse),
        ("invalid-cargo-features", Tag::CargoFeatures),
    ] {
        let rows = reference::observations();
        let row = rows.iter().find(|row| reference::id(row) == id).unwrap();
        let mut account = reference::Account;
        let document = reference::parse(row, &mut account);
        let mut session = session::Session::new(&mut account);
        let result = Package::interpret(
            value::Input::raw(document.table().get("package").unwrap()),
            &mut session,
        );
        let Err(Error::Invalid(error)) = session.finish(result) else {
            panic!("{id}: expected rejection")
        };
        assert_eq!(error.tag, expected);
        let span = error.span.unwrap();
        assert_eq!(json!([span.start, span.end]), row["error"]["span"], "{id}");
    }
}

/// Transport the complete converted defaults for containing-schema comparisons.
pub(in crate::input::membership::manifest) fn defaults(value: &Defaults<'_>) -> Value {
    projection(value.entries(), &DEFAULTS, "InheritablePackage")
}
