use serde_json::{json, Value};

use super::super::super::super::{reference, session, value, Error, Tag};
use super::super::{Config, Inherited, Level, Lint, Table};

fn level(level: Level) -> Value {
    reference::tagged(
        match level {
            Level::Forbid => "TomlLintLevel::Forbid",
            Level::Deny => "TomlLintLevel::Deny",
            Level::Warn => "TomlLintLevel::Warn",
            Level::Allow => "TomlLintLevel::Allow",
        },
        Value::Null,
    )
}

fn config(config: &Config<'_>, context: &str) -> Value {
    let extras: Vec<_> = config
        .extras()
        .map(|(key, value)| {
            json!([
                key.text.as_str(),
                reference::generic_projection(value, 0, context)
            ])
        })
        .collect();
    json!({"type":"TomlLintConfig","fields":{
        "level":level(config.level()), "priority":config.priority(),
        "config":reference::tagged("TomlTable", json!(extras))
    }})
}

pub(in crate::input::membership::manifest) fn table(table: &Table<'_>, context: &str) -> Value {
    let tools: Vec<_> = table
        .tools()
        .map(|(key, tool)| {
            let rules: Vec<_> = tool
                .rules()
                .map(|(key, lint)| {
                    let lint = match lint {
                        Lint::Level(value) => reference::tagged("TomlLint::Level", level(*value)),
                        Lint::Config(value) => {
                            reference::tagged("TomlLint::Config", config(value, context))
                        }
                    };
                    json!([key.text.as_str(), lint])
                })
                .collect();
            json!([
                key.text.as_str(),
                reference::tagged("BTreeMap", json!(rules))
            ])
        })
        .collect();
    reference::tagged("BTreeMap", json!(tools))
}

#[test]
fn complete_lint_families_match_the_committed_pinned_projection() {
    let mut compared = Vec::new();
    for row in reference::observations() {
        if row["accepted"] != true {
            continue;
        }
        let context = reference::id(&row);
        let mut account = reference::Account;
        if let Some(expected) = reference::some(&row["typed"]["fields"]["lints"], context) {
            let document = reference::parse(&row, &mut account);
            let input = value::Input::raw(document.table().get("lints").unwrap());
            let mut session = session::Session::new(&mut account);
            let result = Inherited::interpret(input, &mut session);
            let output = session
                .finish(result)
                .unwrap_or_else(|error| panic!("{context}: {error:?}"));
            let actual = json!({"type":"InheritableLints","fields":{
                "workspace":output.workspace(), "lints":table(output.table(), context)
            }});
            assert_eq!(actual, *expected, "{context}: complete inherited lints");
            compared.push((context.to_owned(), "lints"));
        }
        if let Some(workspace) = reference::some(&row["typed"]["fields"]["workspace"], context)
            && let Some(expected) = reference::some(&workspace["fields"]["lints"], context)
        {
            let document = reference::parse(&row, &mut account);
            let workspace = reference::table(document.table().get("workspace").unwrap(), context);
            let input = value::Input::raw(workspace.get("lints").unwrap());
            let mut session = session::Session::new(&mut account);
            let result = Table::interpret(input, &mut session);
            let output = session
                .finish(result)
                .unwrap_or_else(|error| panic!("{context}: {error:?}"));
            assert_eq!(
                table(&output, context),
                *expected,
                "{context}: complete ordinary lints"
            );
            compared.push((context.to_owned(), "workspace.lints"));
        }
    }
    assert_eq!(
        compared,
        [
            ("complete-families", "lints"),
            ("workspace-and-inheritance", "lints"),
            ("workspace-and-inheritance", "workspace.lints"),
            ("inherit-lints-empty", "lints"),
            ("lint-unit-table", "lints"),
            ("lint-unit-array", "lints"),
        ]
        .map(|(id, scope)| (id.to_owned(), scope)),
        "review exact lint comparison scope"
    );
}

#[test]
fn isolated_lint_failures_match_pinned_precedence_and_erased_spans() {
    let rows = reference::observations();
    let mut compared = 0;
    for (id, tag, message) in [
        (
            "missing-before-flatten",
            Tag::Missing,
            "missing field `level`",
        ),
        (
            "ordinary-before-missing",
            Tag::Type,
            "invalid value: integer `128`, expected i8",
        ),
        (
            "flatten-after-known",
            Tag::Type,
            "invalid value: integer `128`, expected i8",
        ),
        (
            "inherit-lints-false",
            Tag::WorkspaceFalse,
            "`workspace` cannot be false",
        ),
        ("lint-unit-nonempty", Tag::Length, "expected empty array"),
        (
            "datetime-flattened",
            Tag::Length,
            "invalid length 2, expected 1 element in map",
        ),
    ] {
        let row = rows.iter().find(|row| reference::id(row) == id).unwrap();
        assert_eq!(row["accepted"], false, "{id}");
        assert_eq!(row["error"]["phase"], "typed", "{id}");
        assert!(
            row["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with(message),
            "{id}: review bounded category"
        );
        let mut account = reference::Account;
        let document = reference::parse(row, &mut account);
        assert_eq!(document.table().len(), 1, "{id}: isolated lints source");
        let input = value::Input::raw(document.table().get("lints").unwrap());
        let mut session = session::Session::new(&mut account);
        let result = Inherited::interpret(input, &mut session);
        let Err(Error::Invalid(error)) = session.finish(result) else {
            panic!("{id}: semantic rejection")
        };
        assert_eq!(error.tag, tag, "{id}: error precedence");
        assert_eq!(
            json!(error.span.map(|span| [span.start, span.end])),
            row["error"]["span"],
            "{id}: applicable pinned span"
        );
        compared += 1;
    }
    assert_eq!(compared, 6);
}
