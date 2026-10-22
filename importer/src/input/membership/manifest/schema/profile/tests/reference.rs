//! Compare the actual leaf visitors with the executed pinned Cargo corpus.

use rawr_toml::document;
use serde_json::{json, Value};

use super::super::{units, Debug, Opt};
use crate::input::membership::manifest::{reference, session, value, Error, Step, Tag};

const CASES: [&str; 20] = [
    "complete-families",
    "debug-false",
    "debug-true",
    "debug-zero",
    "debug-one",
    "debug-two",
    "debug-none",
    "debug-limited",
    "debug-full",
    "debug-lines",
    "debug-tables",
    "opt-i64-min",
    "opt-i64-max",
    "opt-u64",
    "opt-string-number",
    "debug-three",
    "debug-float",
    "u32-max",
    "u32-over",
    "u32-negative",
];

fn converted<A: document::Admission>(
    field: &str,
    input: value::Input<'_, '_>,
    session: &mut session::Session<'_, A>,
) -> Step<Value> {
    match field {
        "opt-level" => Opt::interpret(input, session)
            .map(|value| reference::tagged("TomlOptLevel", json!(value.as_str()))),
        "debug" => Debug::interpret(input, session).map(|value| {
            let kind = match value {
                Debug::None => "TomlDebugInfo::None",
                Debug::LineDirectivesOnly => "TomlDebugInfo::LineDirectivesOnly",
                Debug::LineTablesOnly => "TomlDebugInfo::LineTablesOnly",
                Debug::Limited => "TomlDebugInfo::Limited",
                Debug::Full => "TomlDebugInfo::Full",
            };
            reference::tagged(kind, Value::Null)
        }),
        "codegen-units" => units(input, session).map(|value| json!(value)),
        _ => panic!("unexpected scoped field {field}"),
    }
}

fn expected<'a>(row: &'a Value, field: &str) -> &'a Value {
    let context = reference::id(row);
    let profiles = reference::some(&row["typed"]["fields"]["profile"], context).unwrap();
    assert_eq!(profiles["type"], "TomlProfiles");
    assert_eq!(profiles["value"]["type"], "BTreeMap");
    let dev = profiles["value"]["value"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair[0]["value"] == "dev")
        .unwrap();
    assert_eq!(dev[1]["type"], "TomlProfile");
    let field = match field {
        "opt-level" => "opt_level",
        "debug" => "debug",
        "codegen-units" => "codegen_units",
        _ => panic!("unexpected scoped field {field}"),
    };
    reference::some(&dev[1]["fields"][field], context).unwrap()
}

#[test]
fn complete_profile_leaf_results_and_errors_match_committed_pinned_observations() {
    let mut seen = Vec::new();
    let mut fields = 0;
    for row in reference::observations() {
        let context = reference::id(&row);
        if !CASES.contains(&context) {
            continue;
        }
        seen.push(context.to_owned());
        let mut account = reference::Account;
        let parsed = reference::parse(&row, &mut account);
        let profiles = reference::table(parsed.table().get("profile").unwrap(), context);
        let dev = reference::table(profiles.get("dev").unwrap(), context);
        for field in ["opt-level", "debug", "codegen-units"] {
            let Some(input) = dev.get(field) else {
                continue;
            };
            fields += 1;
            let mut session = session::Session::new(&mut account);
            let result = converted(field, value::Input::raw(input), &mut session);
            let result = session.finish(result);
            assert_eq!(
                result.is_ok(),
                row["accepted"].as_bool().unwrap(),
                "{context} {field}"
            );
            match result {
                Ok(actual) => assert_eq!(&actual, expected(&row, field), "{context} {field}"),
                Err(Error::Invalid(error)) => {
                    assert_eq!(row["error"]["phase"], "typed", "{context}");
                    let span = error.span.expect("direct leaf rejection has a TOML span");
                    assert_eq!(
                        json!([span.start, span.end]),
                        row["error"]["span"],
                        "{context}"
                    );
                    let message = row["error"]["message"].as_str().unwrap();
                    let expected = if message.starts_with("invalid type:") {
                        Tag::Type
                    } else {
                        assert!(
                            message.starts_with("invalid value:") || context == "opt-string-number"
                        );
                        Tag::Variant
                    };
                    assert_eq!(error.tag, expected, "{context}");
                }
                Err(error) => panic!("{context}: unexpected resource failure {error:?}"),
            }
        }
    }
    assert_eq!(seen.len(), CASES.len());
    for case in CASES {
        assert!(seen.iter().any(|seen| seen == case), "missing {case}");
    }
    assert_eq!(
        fields, 22,
        "review the scoped leaf comparison when the corpus changes"
    );
}
