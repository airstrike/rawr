use super::*;
use crate::input::membership::manifest::{reference, Stop};

mod resolution;

#[derive(Default)]
struct Account {
    requests: usize,
    refusal: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, _: document::Request) -> Result<(), usize> {
        let index = self.requests;
        self.requests += 1;
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn definition(row: &serde_json::Value) -> Definition<'_> {
    match row[0].as_str().unwrap() {
        "builtin" => Definition::BuiltIn,
        "file" => Definition::Path(Path::new(row[1].as_str().unwrap())),
        "env" => Definition::Environment(row[1].as_str().unwrap()),
        "cli" => Definition::Cli(row[1].as_str().map(Path::new)),
        _ => panic!("invalid fixture"),
    }
}

fn origin(definition: Definition<'_>) -> serde_json::Value {
    match definition {
        Definition::BuiltIn => serde_json::json!(["builtin", null]),
        Definition::Path(path) => serde_json::json!(["file", path]),
        Definition::Environment(key) => serde_json::json!(["env", key]),
        Definition::Cli(path) => serde_json::json!(["cli", path]),
    }
}

fn expected(error: &str) -> &'static str {
    if error.starts_with("missing field") {
        "field `path`"
    } else if error.starts_with("expected a list") {
        "a list of strings or a list of tables"
    } else if error.starts_with("expected a string or table") {
        "a string or table"
    } else if error.starts_with("expected a string,") {
        "a string"
    } else if error.starts_with("expected a boolean") {
        "a boolean"
    } else if error.starts_with("expected a config include path ending") {
        "a path ending with `.toml`"
    } else if error.starts_with("expected a config include path without glob") {
        "a path without glob patterns"
    } else if error.starts_with("expected a config include path without template") {
        "a path without template braces"
    } else {
        panic!("unexpected reference error: {error}")
    }
}

#[test]
fn complete_extraction_and_first_failure_match_pinned_cargo() {
    for line in include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-include/observations.jsonl"
    ))
    .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let doc =
            document::parse(row["source"].as_str().unwrap(), 80, &mut reference::Account).unwrap();
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let values =
            Values::from_document(&doc, definition(&row["definition"]), &mut session).unwrap();
        let mut tree = Tree::from_values(values, &mut session).unwrap();
        let before = tree.nodes.as_slice().len();
        match tree.includes(row["remove"].as_bool().unwrap(), &mut session) {
            Ok(includes) => {
                let actual: Vec<_> = includes
                    .as_slice()
                    .iter()
                    .map(|i| serde_json::json!([i.path, origin(i.definition), i.optional]))
                    .collect();
                assert_eq!(serde_json::json!(actual), row["includes"], "{row}");
            }
            Err(Stop::Rejected(diagnostic)) => {
                assert_eq!(diagnostic.tag, Tag::ConfigInclude, "{row}");
                assert_eq!(
                    diagnostic.expected,
                    Some(expected(row["error"].as_str().unwrap())),
                    "{row}"
                );
            }
            Err(other) => panic!("{other:?}: {row}"),
        }
        // A fresh test-only account observes mutation after terminal rejection.
        let mut probe = Account::default();
        let mut probe = Session::new(&mut probe);
        let Some(Kind::Table(table)) = tree.nodes.as_slice()[0].kind.as_ref() else {
            panic!()
        };
        let present = table.get("include", &mut probe).unwrap().is_some();
        let expected_present = row["remaining"][2]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r[0] == "include");
        assert_eq!(present, expected_present, "{row}");
        assert_eq!(tree.nodes.as_slice().len(), before); // no subtree clone
    }
}

#[test]
fn every_original_refusal_stops_extraction_and_latches() {
    let doc = document::parse(
        "include=[\"a.toml\",{path=\"b.toml\",optional=true},\"c.toml\",\"d.toml\",\"e.toml\"]",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let run = |refusal| {
        let mut account = Account {
            refusal,
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let result = Values::from_document(&doc, Definition::BuiltIn, &mut session)
            .and_then(|values| Tree::from_values(values, &mut session))
            .and_then(|mut tree| tree.includes(true, &mut session));
        if let Some(index) = refusal {
            assert!(matches!(result, Err(Stop::Refused)));
            assert!(matches!(session.items(1), Err(Stop::Refused)));
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(found)) if found == index)
            );
        } else {
            assert_eq!(result.unwrap().len(), 5);
        }
        account.requests
    };
    let count = run(None);
    for index in 0..count {
        assert_eq!(run(Some(index)), index + 1);
    }
}

#[test]
fn table_origin_and_conversion_failure_span_are_preserved() {
    let source = "include=[{path=\"one.toml\",optional=true}]";
    let doc = document::parse(source, 80, &mut reference::Account).unwrap();
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let values = Values::from_document(
        &doc,
        Definition::Path(Path::new("/table/config.toml")),
        &mut session,
    )
    .unwrap();
    let mut tree = Tree::from_values(values, &mut session).unwrap();
    for node in tree.nodes.as_mut_slice() {
        if matches!(node.kind, Some(Kind::String(_)) | Some(Kind::Boolean(_))) {
            node.definition = Definition::Cli(Some(Path::new("/child/config.toml")));
        }
    }
    let includes = tree.includes(false, &mut session).unwrap();
    assert_eq!(
        origin(includes.as_slice()[0].definition),
        serde_json::json!(["file", "/table/config.toml"])
    );

    let source = "include=[\"bad.txt\",{path=17,optional=\"bad\"}]";
    let doc = document::parse(source, 80, &mut reference::Account).unwrap();
    let values = Values::from_document(&doc, Definition::BuiltIn, &mut session).unwrap();
    let mut tree = Tree::from_values(values, &mut session).unwrap();
    let Err(Stop::Rejected(diagnostic)) = tree.includes(true, &mut session) else {
        panic!()
    };
    assert_eq!(diagnostic.expected, Some("a string"));
    let span = diagnostic.span.unwrap();
    assert_eq!(&source[span.start..span.end], "17");
}
