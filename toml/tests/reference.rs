//! Compare actual contained documents with separately executed pinned DeTable.
//! Allocating traversal and datetime Display here are test-only observation.

use rawr_toml::document;
use serde_json::json;

#[derive(Default)]
struct Account {
    items: usize,
    bytes: usize,
}

impl document::Admission for Account {
    type Error = &'static str;

    fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
        match request {
            document::Request::Items(count) => {
                self.items = self.items.checked_add(count).ok_or("items overflow")?;
                if self.items > 1_000_000 {
                    return Err("fixture items");
                }
            }
            document::Request::String(length) => {
                if length > 1_048_576 {
                    return Err("fixture string");
                }
            }
            document::Request::Allocation { bytes, .. } => {
                self.bytes = self.bytes.checked_add(bytes).ok_or("bytes overflow")?;
                if self.bytes > 64 * 1024 * 1024 {
                    return Err("fixture bytes");
                }
            }
        }
        Ok(())
    }
}

fn entries(table: &document::Table<'_>, parent: usize, output: &mut Vec<serde_json::Value>) {
    for (index, (key, item)) in table.iter().enumerate() {
        let span = key.span();
        append(
            item,
            json!({
                "parent": parent, "index": index,
                "key": key.as_str(), "key_span": [span.start, span.end],
            }),
            output,
        );
    }
}

fn append(
    item: &document::Spanned<document::Value<'_>>,
    mut node: serde_json::Value,
    output: &mut Vec<serde_json::Value>,
) {
    let id = output.len();
    let span = item.span();
    node["id"] = json!(id);
    node["span"] = json!([span.start, span.end]);
    node["kind"] = json!(match item.value() {
        document::Value::Table(_) => "table",
        document::Value::Array(_) => "array",
        document::Value::String(_) => "string",
        document::Value::Integer(_) => "integer",
        document::Value::Float(_) => "float",
        document::Value::Boolean(_) => "boolean",
        document::Value::DateTime(_) => "datetime",
    });
    match item.value() {
        document::Value::String(value) => node["text"] = json!(value.as_str()),
        document::Value::Integer(value) => {
            node["text"] = json!(value.as_str());
            node["radix"] = json!(value.radix());
        }
        document::Value::Float(value) => node["text"] = json!(value.as_str()),
        document::Value::Boolean(value) => node["value"] = json!(value),
        document::Value::DateTime(value) => node["text"] = json!(value.to_string()),
        document::Value::Table(_) | document::Value::Array(_) => {}
    }
    output.push(node);
    match item.value() {
        document::Value::Table(table) => entries(table, id, output),
        document::Value::Array(array) => {
            for (index, value) in array.values().iter().enumerate() {
                append(
                    value,
                    json!({"parent": id, "index": index, "key": null, "key_span": null}),
                    output,
                );
            }
        }
        _ => {}
    }
}

fn observed(document: &document::Document<'_>) -> serde_json::Value {
    let mut nodes = vec![json!({
        "id": 0, "parent": null, "index": 0, "key": null,
        "key_span": null, "kind": "table", "span": [0, 0],
    })];
    entries(document.table(), 0, &mut nodes);
    nodes.into()
}

fn category(message: &str) -> document::Tag {
    match message {
        "duplicate key" => document::Tag::DuplicateKey,
        "recursion limit" | "cannot recurse further; max recursion depth met" => {
            document::Tag::Depth
        }
        "cannot extend value of type inline table with a dotted key" => {
            document::Tag::ExtendInlineTable
        }
        "cannot extend value of type array with a dotted key" => document::Tag::ExtendWrongType,
        "invalid date, expected day between 01 and 28" => document::Tag::DateTime,
        "unclosed array, expected `]`"
        | "string values must be quoted, expected literal string"
        | "missing escaped value, expected `b`, `e`, `f`, `n`, `r`, `\\`, `\"`, `x`, `u`, `U`"
        | "missing key for inline table element, expected key" => document::Tag::Syntax,
        other => panic!("reference diagnostic requires explicit classification: {other}"),
    }
}

#[test]
fn bounded_document_matches_executed_pinned_reference() {
    let mut count = 0;
    let mut accepted = 0;
    let mut rejected = 0;
    for line in include_str!("../reference/output.jsonl").lines() {
        let reference: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(reference["schema"], "rawr-toml-reference-v1");
        let id = reference["id"].as_str().unwrap();
        let source = reference["source"].as_str().unwrap();
        let expected = reference["accepted"].as_bool().unwrap();
        assert_eq!(
            reference["expected_acceptance"], expected,
            "{id}: reference fixture intent"
        );
        let result = document::parse(source, 80, &mut Account::default());
        assert_eq!(result.is_ok(), expected, "{id}: acceptance: {result:?}");
        if expected {
            accepted += 1;
            assert_eq!(
                observed(&result.unwrap()),
                reference["document"],
                "{id}: semantic data"
            );
        } else {
            rejected += 1;
            let document::Error::Invalid(error) = result.unwrap_err() else {
                panic!("{id}: reference syntax rejection became resource failure");
            };
            assert_eq!(
                error.tag,
                category(reference["errors"][0]["message"].as_str().unwrap()),
                "{id}: first error category"
            );
            let span = error.span.map(|span| json!([span.start, span.end]));
            assert_eq!(
                span.unwrap_or(serde_json::Value::Null),
                reference["errors"][0]["span"],
                "{id}: first error span"
            );
            if let Some(first) = reference["raw"]["errors"].as_array().unwrap().first() {
                let context = error.context.map(|span| json!([span.start, span.end]));
                assert_eq!(
                    context.unwrap_or(serde_json::Value::Null),
                    first["context"],
                    "{id}: raw error context"
                );
            }
        }
        count += 1;
    }
    assert_eq!(
        (count, accepted, rejected),
        (35, 14, 21),
        "reference corpus cardinality"
    );
}
