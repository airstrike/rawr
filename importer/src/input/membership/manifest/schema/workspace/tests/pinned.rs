//! Complete selected Workspace comparisons with the executed Cargo reference.

use serde_json::{json, Value};

use super::super::super::super::{reference, session, value, Error, Tag};
use super::super::super::{dependency, lints, package, primitive};
use super::super::{Field, Ignored, Workspace};

pub(in crate::input::membership::manifest) fn projection(
    workspace: &Workspace<'_>,
    context: &str,
) -> Value {
    let mut fields = serde_json::Map::new();
    for (field, name) in [
        (Field::Members, "members"),
        (Field::Exclude, "exclude"),
        (Field::DefaultMembers, "default_members"),
        (Field::Resolver, "resolver"),
        (Field::Metadata, "metadata"),
        (Field::Package, "package"),
        (Field::Dependencies, "dependencies"),
        (Field::Lints, "lints"),
    ] {
        let converted = workspace.get(field).map(|value| match value {
            super::super::Value::Local(primitive::Value::String(text)) => json!(text.as_str()),
            super::super::Value::Local(primitive::Value::Strings(values)) => json!(values
                .as_slice()
                .iter()
                .map(|text| text.as_str())
                .collect::<Vec<_>>()),
            super::super::Value::Local(_) => panic!("{context}: unexpected workspace primitive"),
            super::super::Value::Metadata(value) => {
                reference::generic_projection(value, 0, context)
            }
            super::super::Value::Package(value) => package::tests::pinned::defaults(value),
            super::super::Value::Dependencies(value) => dependency::reference::direct_map(value),
            super::super::Value::Lints(value) => lints::tests::reference::table(value, context),
        });
        fields.insert(
            name.to_owned(),
            match converted {
                Some(value) => reference::tagged("Some", value),
                None => reference::tagged("None", Value::Null),
            },
        );
    }
    json!({"type":"TomlWorkspace", "fields":fields})
}

fn position(parent: &str, position: package::Position<'_>) -> String {
    match position {
        package::Position::Map(key) => format!("{parent}.{}", key.text.as_str()),
        package::Position::Sequence(index) => format!("{parent}.{index}"),
    }
}

#[test]
fn complete_workspace_payload_matches_the_committed_pinned_projection() {
    let mut compared = Vec::new();
    for row in reference::observations() {
        if row["accepted"] != true {
            continue;
        }
        let context = reference::id(&row);
        let Some(expected) = reference::some(&row["typed"]["fields"]["workspace"], context) else {
            continue;
        };
        let mut account = reference::Account;
        let document = reference::parse(&row, &mut account);
        let mut session = session::Session::new(&mut account);
        let result = Workspace::interpret(
            value::Input::raw(document.table().get("workspace").unwrap()),
            &mut session,
        );
        let output = session
            .finish(result)
            .unwrap_or_else(|error| panic!("{context}: {error:?}"));
        assert_eq!(
            projection(&output, context),
            *expected,
            "{context}: complete Workspace"
        );
        let actual: Vec<_> = output
            .ignored()
            .iter()
            .map(|ignored| match ignored {
                Ignored::Field(key) => format!("workspace.{}", key.text.as_str()),
                Ignored::Package {
                    position: outer,
                    path,
                } => {
                    let parent = position("workspace", *outer);
                    let parent = path
                        .parent()
                        .map_or(parent.clone(), |part| position(&parent, part));
                    format!("{parent}.{}", path.key().text.as_str())
                }
            })
            .collect();
        let expected: Vec<_> = row["unused_callbacks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .filter(|path| path.starts_with("workspace."))
            .collect();
        assert_eq!(actual, expected, "{context}: ordered ignored callbacks");
        compared.push(context.to_owned());
    }
    assert_eq!(
        compared,
        ["workspace-and-inheritance"],
        "review exact complete Workspace comparison scope"
    );
}

#[test]
fn isolated_workspace_version_rejection_preserves_the_pinned_span() {
    let rows = reference::observations();
    let row = rows
        .iter()
        .find(|row| reference::id(row) == "workspace-keeps-version-space")
        .unwrap();
    assert_eq!(row["accepted"], false);
    assert_eq!(row["error"]["phase"], "typed");
    assert!(row["error"]["message"]
        .as_str()
        .unwrap()
        .starts_with("unexpected character ' ' while parsing major version number"));
    let mut account = reference::Account;
    let document = reference::parse(row, &mut account);
    assert_eq!(document.table().len(), 1, "isolated Workspace source");
    let mut session = session::Session::new(&mut account);
    let result = Workspace::interpret(
        value::Input::raw(document.table().get("workspace").unwrap()),
        &mut session,
    );
    let Err(Error::Invalid(error)) = session.finish(result) else {
        panic!("workspace version rejection")
    };
    assert!(matches!(error.tag, Tag::Scalar(_)));
    assert_eq!(
        json!(error.span.map(|span| [span.start, span.end])),
        row["error"]["span"]
    );
}
