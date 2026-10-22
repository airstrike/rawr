//! Every Platform field compared with the executed complete-families row.

use serde_json::{json, Value};

use super::super::super::super::{reference, session, value};
use super::super::super::dependency;
use super::super::{Field, Platform};

fn projection(platform: &Platform<'_>) -> Value {
    let mut fields = serde_json::Map::new();
    for (field, name) in [
        (Field::Dependencies, "dependencies"),
        (Field::BuildDependencies, "build_dependencies"),
        (Field::BuildDependencies2, "build_dependencies2"),
        (Field::DevDependencies, "dev_dependencies"),
        (Field::DevDependencies2, "dev_dependencies2"),
    ] {
        let value = match platform.get(field) {
            Some(value) => reference::tagged("Some", dependency::reference::inheritable_map(value)),
            None => reference::tagged("None", Value::Null),
        };
        fields.insert(name.to_owned(), value);
    }
    json!({"type":"TomlPlatform", "fields":fields})
}

#[test]
fn every_platform_alias_matches_the_committed_complete_typed_projection() {
    let mut compared = Vec::new();
    for row in reference::observations() {
        if row["accepted"] != true {
            continue;
        }
        let id = reference::id(&row);
        let Some(expected) = reference::some(&row["typed"]["fields"]["target"], id) else {
            continue;
        };
        assert_eq!(expected["type"], "BTreeMap", "{id}");
        let mut account = reference::Account;
        let document = reference::parse(&row, &mut account);
        let targets = reference::table(document.table().get("target").unwrap(), id);
        for pair in expected["value"].as_array().unwrap() {
            let name = pair[0].as_str().unwrap();
            let context = format!("{id}: target.{name}");
            let mut session = session::Session::new(&mut account);
            let result =
                Platform::interpret(value::Input::raw(targets.get(name).unwrap()), &mut session);
            let output = session
                .finish(result)
                .unwrap_or_else(|error| panic!("{context}: {error:?}"));
            assert_eq!(projection(&output), pair[1], "{context}: complete Platform");
            let parent = format!("target.{name}.");
            let actual: Vec<_> = output
                .ignored()
                .iter()
                .map(|key| format!("{parent}{}", key.text.as_str()))
                .collect();
            let expected: Vec<_> = row["unused_callbacks"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .filter(|path| path.starts_with(&parent))
                .collect();
            assert_eq!(actual, expected, "{context}: ordered ignored callbacks");
            compared.push((id.to_owned(), name.to_owned()));
        }
    }
    assert_eq!(
        compared,
        [("complete-families".to_owned(), "cfg(unix)".to_owned())],
        "review complete Platform comparison scope"
    );
}
