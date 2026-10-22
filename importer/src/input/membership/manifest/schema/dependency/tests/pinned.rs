use rawr_toml::document;
use serde_json::{json, Value};

use super::super::super::super::{reference, session, value, Error, Tag};
use super::super::{direct, inheritable, Direct};

const MAPS: [(&str, &str); 5] = [
    ("dependencies", "dependencies"),
    ("dev-dependencies", "dev_dependencies"),
    ("dev_dependencies", "dev_dependencies2"),
    ("build-dependencies", "build_dependencies"),
    ("build_dependencies", "build_dependencies2"),
];

fn inherited_maps<'d>(source: &'d document::Table<'d>, expected: &Value, context: &str) -> usize {
    let mut count = 0;
    for (source_key, field) in MAPS {
        let Some(expected) = reference::some(&expected[field], context) else {
            continue;
        };
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let result = inheritable::Map::interpret(
            value::Input::raw(source.get(source_key).unwrap()),
            &mut session,
        );
        let map = session
            .finish(result)
            .unwrap_or_else(|error| panic!("{context}.{source_key}: {error:?}"));
        assert_eq!(
            super::super::reference::inheritable_map(&map),
            *expected,
            "{context}.{source_key}"
        );
        count += 1;
    }
    count
}

#[test]
fn all_committed_dependency_maps_and_replace_values_match_complete_typed_outputs() {
    let mut inherited = 0;
    let mut direct = 0;
    let mut replaced = 0;
    for row in reference::observations() {
        if row["accepted"] != true {
            continue;
        }
        let context = reference::id(&row);
        let expected = &row["typed"]["fields"];
        let mut account = reference::Account;
        let document = reference::parse(&row, &mut account);
        inherited += inherited_maps(document.table(), expected, context);
        if let Some(workspace) = reference::some(&expected["workspace"], context)
            && let Some(expected) = reference::some(&workspace["fields"]["dependencies"], context)
        {
            let source = reference::table(document.table().get("workspace").unwrap(), context)
                .get("dependencies")
                .unwrap();
            let mut session = session::Session::new(&mut account);
            let result = direct::Map::interpret(value::Input::raw(source), &mut session);
            let map = session.finish(result).unwrap();
            assert_eq!(
                super::super::reference::direct_map(&map),
                *expected,
                "{context}.workspace.dependencies"
            );
            direct += 1;
        }
        if let Some(targets) = reference::some(&expected["target"], context) {
            let source = reference::table(document.table().get("target").unwrap(), context);
            for pair in targets["value"].as_array().unwrap() {
                let key = pair[0].as_str().unwrap();
                let source = reference::table(source.get(key).unwrap(), context);
                inherited += inherited_maps(
                    source,
                    &pair[1]["fields"],
                    &format!("{context}.target.{key}"),
                );
            }
        }
        if let Some(patches) = reference::some(&expected["patch"], context) {
            let source = reference::table(document.table().get("patch").unwrap(), context);
            for pair in patches["value"].as_array().unwrap() {
                let key = pair[0].as_str().unwrap();
                let mut session = session::Session::new(&mut account);
                let result = direct::Map::interpret(
                    value::Input::raw(source.get(key).unwrap()),
                    &mut session,
                );
                let map = session.finish(result).unwrap();
                assert_eq!(
                    super::super::reference::direct_map(&map),
                    pair[1],
                    "{context}.patch.{key}"
                );
                direct += 1;
            }
        }
        if let Some(replace) = reference::some(&expected["replace"], context) {
            let source = reference::table(document.table().get("replace").unwrap(), context);
            for pair in replace["value"].as_array().unwrap() {
                let key = pair[0].as_str().unwrap();
                let mut session = session::Session::new(&mut account);
                let result =
                    Direct::interpret(value::Input::raw(source.get(key).unwrap()), &mut session);
                let value = session.finish(result).unwrap();
                assert_eq!(
                    super::super::reference::direct_value(&value),
                    pair[1],
                    "{context}.replace.{key}"
                );
                replaced += 1;
            }
        }
    }
    assert_eq!(
        (inherited, direct, replaced),
        (19, 2, 1),
        "review complete reference dependency coverage changes"
    );
}

#[test]
fn isolated_dependency_errors_match_the_committed_tags_and_spans() {
    for (id, tag) in [
        ("flatten-positional-rejected", Tag::Type),
        ("dependency-stage-u128", Tag::Integer128),
        ("dependency-extra-u64", Tag::Integer64),
        ("inherit-dependency-false", Tag::WorkspaceFalse),
        ("inherit-false-trial-fallback", Tag::Type),
        ("datetime-staged-extra", Tag::Length),
    ] {
        let rows = reference::observations();
        let row = rows.iter().find(|row| reference::id(row) == id).unwrap();
        let mut account = reference::Account;
        let document = reference::parse(row, &mut account);
        let mut session = session::Session::new(&mut account);
        let result = inheritable::Map::interpret(
            value::Input::raw(document.table().get("dependencies").unwrap()),
            &mut session,
        );
        let Err(Error::Invalid(error)) = session.finish(result) else {
            panic!("{id}: rejection")
        };
        assert_eq!(error.tag, tag, "{id}");
        assert_eq!(
            json!(error.span.map(|span| [span.start, span.end])),
            row["error"]["span"],
            "{id}"
        );
    }
}
