use super::*;
use crate::input::membership::manifest::{reference, Stop};

#[derive(Default)]
struct Account {
    count: usize,
    refusal: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, _: document::Request) -> Result<(), usize> {
        let index = self.count;
        self.count += 1;
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn rendered(tree: &value::Projection<'_>, index: usize) -> serde_json::Value {
    use serde_json::json;
    match tree.nodes.as_slice()[index].kind {
        value::Kind::String(text) => json!(["string", text.as_str()]),
        value::Kind::I64(value) => json!(["integer", value]),
        value::Kind::Boolean(value) => json!(["boolean", value]),
        value::Kind::Sequence(children) | value::Kind::Map(children) => {
            let mut rows = Vec::new();
            let mut next = children.first;
            while let Some(index) = next {
                let edge = &tree.edges.as_slice()[index];
                next = edge.next;
                rows.push((
                    edge.key.map(|key| key.text.as_str().to_owned()),
                    rendered(tree, edge.child),
                ));
            }
            if matches!(tree.nodes.as_slice()[index].kind, value::Kind::Map(_)) {
                rows.sort_by(|left, right| left.0.cmp(&right.0));
                json!(["table", rows])
            } else {
                json!([
                    "list",
                    rows.into_iter().map(|(_, value)| value).collect::<Vec<_>>()
                ])
            }
        }
        _ => panic!("unconverted config value"),
    }
}

#[test]
fn complete_conversion_matches_pinned_reference_observations() {
    for line in include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-values/observations.jsonl"
    ))
    .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let source = row["source"].as_str().unwrap();
        let document = document::parse(source, 80, &mut reference::Account).unwrap();
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let result = Values::from_document(&document, Definition::BuiltIn, &mut session);
        let result = session.finish(result);
        if row.get("value").is_some() {
            let values = result.unwrap();
            assert_eq!(rendered(&values.tree, 0), row["value"], "{source}");
            assert_eq!(values.definition, Definition::BuiltIn);
        } else if row.get("parse_error").is_some() {
            assert!(
                matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::Integer64),
                "{source}"
            );
        } else {
            let expected = if row["config_error"].as_str().unwrap().contains("`float`") {
                Unsupported::Float
            } else {
                Unsupported::Datetime
            };
            assert!(
                matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::ConfigValue(expected)),
                "{source}"
            );
        }
    }
}

#[test]
fn conversion_keeps_original_refusal_and_no_successful_partial_value() {
    let source = "a=[1,true,{b='value'}]\n[path-bases]\nworkspace='root'";
    let document = document::parse(source, 80, &mut reference::Account).unwrap();
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    let result = Values::from_document(&document, Definition::BuiltIn, &mut session);
    session.finish(result).unwrap();
    for refusal in 0..baseline.count {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(
            Values::from_document(&document, Definition::BuiltIn, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            Values::from_document(&document, Definition::BuiltIn, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())),Err(Error::Admission(index)) if index==refusal));
        assert_eq!(account.count, refusal + 1);
    }
}

#[test]
fn initial_conversion_preserves_unresolved_root_file_definition() {
    let document = document::parse(
        "[path-bases]\nworkspace='child'",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let file = std::path::Path::new("/config.toml");
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let result = Values::from_document(&document, Definition::Path(file), &mut session);
    let values = session.finish(result).unwrap();
    assert!(matches!(values.definition, Definition::Path(retained) if std::ptr::eq(retained,file)));
    // The source cause is reached on selected path resolution, never merely file conversion.
}

#[test]
fn nested_conversion_and_teardown_remain_flat() {
    // Remain inside the pinned parser's independent nesting ceiling.
    let depth = 64;
    let source = format!("x={}1{}", "[".repeat(depth), "]".repeat(depth));
    let document = document::parse(
        &source,
        u32::try_from(depth + 10).unwrap(),
        &mut reference::Account,
    )
    .unwrap();
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let result = Values::from_document(&document, Definition::BuiltIn, &mut session);
    let values = session.finish(result).unwrap();
    assert_eq!(values.tree.nodes.len(), depth + 2);
    drop(values);
}
