//! Complete enclosing collections compared with committed executed Cargo data.

use serde_json::{json, Value};

use crate::input::membership::manifest::schema::{dependency, platform};
use crate::input::membership::manifest::{reference, session, value};

use super::{Features, Patch, Platforms, Replace, Targets};

fn platform(value: &platform::Platform<'_>) -> Value {
    let fields: serde_json::Map<_, _> = [
        (platform::Field::Dependencies, "dependencies"),
        (platform::Field::BuildDependencies, "build_dependencies"),
        (platform::Field::BuildDependencies2, "build_dependencies2"),
        (platform::Field::DevDependencies, "dev_dependencies"),
        (platform::Field::DevDependencies2, "dev_dependencies2"),
    ]
    .into_iter()
    .map(|(field, name)| {
        let value = match value.get(field) {
            Some(value) => reference::tagged("Some", dependency::reference::inheritable_map(value)),
            None => reference::tagged("None", Value::Null),
        };
        (name.to_owned(), value)
    })
    .collect();
    json!({"type":"TomlPlatform","fields":fields})
}

#[test]
fn every_available_complete_enclosing_collection_matches_the_pinned_reference() {
    let mut compared = Vec::new();
    for row in reference::observations() {
        if row["accepted"] != true {
            continue;
        }
        let id = reference::id(&row);
        let mut account = reference::Account;
        let document = reference::parse(&row, &mut account);
        for field in [
            "features", "target", "patch", "replace", "bin", "example", "test", "bench",
        ] {
            let Some(expected) = reference::some(&row["typed"]["fields"][field], id) else {
                continue;
            };
            let input = value::Input::raw(document.table().get(field).unwrap());
            let mut session = session::Session::new(&mut account);
            let mut unused = Vec::new();
            let actual = match field {
                "features" => {
                    let output = Features::interpret(input, &mut session).unwrap();
                    reference::tagged(
                        "BTreeMap",
                        Value::Array(
                            output
                                .entries()
                                .map(|entry| {
                                    json!([
                                        reference::tagged(
                                            "FeatureName",
                                            json!(entry.key().as_str())
                                        ),
                                        entry
                                            .value()
                                            .as_slice()
                                            .iter()
                                            .map(value::Text::as_str)
                                            .collect::<Vec<_>>()
                                    ])
                                })
                                .collect(),
                        ),
                    )
                }
                "target" => {
                    let output = Platforms::interpret(input, &mut session).unwrap();
                    unused.extend(output.ignored().iter().map(|path| {
                        format!(
                            "target.{}.{}",
                            path.parent().text.as_str(),
                            path.field().text.as_str()
                        )
                    }));
                    reference::tagged(
                        "BTreeMap",
                        Value::Array(
                            output
                                .entries()
                                .map(|entry| {
                                    json!([entry.origin().text.as_str(), platform(entry.value())])
                                })
                                .collect(),
                        ),
                    )
                }
                "patch" => {
                    let output = Patch::interpret(input, &mut session).unwrap();
                    reference::tagged(
                        "BTreeMap",
                        Value::Array(
                            output
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
                "replace" => {
                    let output = Replace::interpret(input, &mut session).unwrap();
                    reference::tagged(
                        "BTreeMap",
                        Value::Array(
                            output
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
                _ => {
                    let output = Targets::interpret(input, &mut session).unwrap();
                    unused.extend(output.ignored().iter().map(|path| {
                        format!("{field}.{}.{}", path.index(), path.field().text.as_str())
                    }));
                    Value::Array(
                        output
                            .entries()
                            .iter()
                            .map(|entry| reference::target_projection(entry.target(), id))
                            .collect(),
                    )
                }
            };
            session.finish(Ok(())).unwrap();
            assert_eq!(actual, *expected, "{id}: complete {field}");
            let prefix = format!("{field}.");
            let expected: Vec<_> = row["unused_callbacks"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .filter(|path| path.starts_with(&prefix))
                .collect();
            assert_eq!(unused, expected, "{id}: ordered {field} callbacks");
            compared.push((id.to_owned(), field.to_owned()));
        }
    }
    let mut expected: Vec<_> = [
        "features", "target", "patch", "replace", "bin", "example", "test", "bench",
    ]
    .into_iter()
    .map(|field| ("complete-families".to_owned(), field.to_owned()))
    .collect();
    expected.push(("names-unicode-namespace".to_owned(), "features".to_owned()));
    assert_eq!(
        compared, expected,
        "review exact complete comparison scopes when the corpus changes"
    );
}
