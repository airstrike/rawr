//! Narrow comparisons with the executed, committed pinned Cargo reference.
//!
//! observations.jsonl: SHA-256
//! 4bdb6ccb690b4d8eb75a45895d04a659994198666e7c09cfdf24e123cc171b7b.
//! These exercise actual raw Document -> Target and raw Document -> generic
//! TOML conversion. They do not interpret a complete Original, replay staged
//! values, normalize a manifest, or establish resource/source authority.

use std::convert::Infallible;
use std::path::Path;

use rawr_toml::document;
use serde_json::{json, Value};

use super::{schema, session, value, walk, Error, Tag};

const OBSERVATIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/reference/manifest/observations.jsonl"
));

// Test admission deliberately permits all requests. Budget/refusal correctness
// is covered separately; this comparison isolates the converted result.
pub(super) struct Account;

impl document::Admission for Account {
    type Error = Infallible;

    fn admit(&mut self, _: document::Request) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub(super) fn observations() -> Vec<Value> {
    let rows: Vec<Value> = OBSERVATIONS
        .lines()
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("reference line {}: {error}", index + 1))
        })
        .collect();
    assert_eq!(
        rows.len(),
        130,
        "review comparison scopes when the corpus changes"
    );
    rows
}

pub(super) fn id(row: &Value) -> &str {
    row["id"].as_str().expect("reference case ID")
}

pub(super) fn parse<'i>(row: &'i Value, account: &mut Account) -> document::Document<'i> {
    let source = row["source"].as_str().expect("reference source");
    document::parse(source, 80, account)
        .unwrap_or_else(|error| panic!("{}: document: {error:?}", id(row)))
}

pub(super) fn some<'a>(option: &'a Value, context: &str) -> Option<&'a Value> {
    match option["type"].as_str() {
        Some("Some") => Some(&option["value"]),
        Some("None") => {
            assert!(option["value"].is_null(), "{context}: malformed None");
            None
        }
        _ => panic!("{context}: missing reference Option: {option:?}"),
    }
}

pub(super) fn table<'a, 'i>(
    value: &'a document::Spanned<document::Value<'i>>,
    context: &str,
) -> &'a document::Table<'i> {
    let document::Value::Table(table) = value.value() else {
        panic!("{context}: this direct comparison requires a source table")
    };
    table
}

pub(super) fn tagged(kind: &str, value: Value) -> Value {
    json!({"type": kind, "value": value})
}

pub(super) fn children<'a, 'd>(
    projection: &'a value::Projection<'d>,
    children: value::Children,
    context: &str,
) -> Vec<&'a value::Edge<'d>> {
    let mut result = Vec::new();
    let mut next = children.first;
    while let Some(index) = next {
        assert!(
            result.len() < children.length,
            "{context}: cyclic or excess child edge"
        );
        let edge = projection
            .edges
            .as_slice()
            .get(index)
            .unwrap_or_else(|| panic!("{context}: invalid edge {index}"));
        result.push(edge);
        next = edge.next;
        if next.is_none() {
            assert_eq!(children.last, Some(index), "{context}: last child edge");
        }
    }
    assert_eq!(result.len(), children.length, "{context}: child count");
    result
}

// This is a transport mapping from converted Target field names to the exact
// reference's Rust field names; it performs no input visitation or conversion.
const TARGET_FIELDS: [(&str, &str); 15] = [
    ("name", "name"),
    ("crate-type", "crate_type"),
    ("crate_type", "crate_type2"),
    ("path", "path"),
    ("filename", "filename"),
    ("test", "test"),
    ("doctest", "doctest"),
    ("bench", "bench"),
    ("doc", "doc"),
    ("doc-scrape-examples", "doc_scrape_examples"),
    ("proc-macro", "proc_macro"),
    ("proc_macro", "proc_macro2"),
    ("harness", "harness"),
    ("required-features", "required_features"),
    ("edition", "edition"),
];

pub(super) fn target_projection(target: &schema::Target<'_>, context: &str) -> Value {
    let projection = target.values();
    let value::Kind::Map(root) = projection.nodes.as_slice()[0].kind else {
        panic!("{context}: Target result root must be a map")
    };
    let mut fields = serde_json::Map::new();
    for (_, name) in TARGET_FIELDS {
        fields.insert(name.to_owned(), tagged("None", Value::Null));
    }
    for edge in children(projection, root, context) {
        let key = edge.key.expect("Target field key");
        let (_, name) = TARGET_FIELDS
            .iter()
            .find(|(source, _)| *source == key.text.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "{context}: unexpected converted field {}",
                    key.text.as_str()
                )
            });
        assert_eq!(
            fields[*name]["type"], "None",
            "{context}: repeated converted field {name}"
        );
        let value = match projection.nodes.as_slice()[edge.child].kind {
            value::Kind::String(text) if *name == "path" => {
                let path = Path::new(text.as_str());
                tagged(
                    "PathValue",
                    json!({"utf8": path.to_str(),
                    "encoded_bytes": path.as_os_str().as_encoded_bytes()}),
                )
            }
            value::Kind::String(text) => json!(text.as_str()),
            value::Kind::Boolean(value) => json!(value),
            value::Kind::Sequence(sequence) => Value::Array(
                children(projection, sequence, context)
                    .into_iter()
                    .map(|edge| {
                        assert!(edge.key.is_none(), "{context}: array element key");
                        let value::Kind::String(text) =
                            projection.nodes.as_slice()[edge.child].kind
                        else {
                            panic!("{context}: Target string vector element")
                        };
                        json!(text.as_str())
                    })
                    .collect(),
            ),
            other => panic!("{context}: unexpected Target payload {other:?}"),
        };
        fields.insert((*name).to_owned(), tagged("Some", value));
    }
    json!({"type": "TomlTarget", "fields": fields})
}

fn compare_target<'d>(
    row: &Value,
    scope: &str,
    input: &'d document::Spanned<document::Value<'d>>,
    expected: &Value,
    account: &mut Account,
) {
    let context = format!("{}: {scope}", id(row));
    let mut session = session::Session::new(account);
    let result = schema::Target::interpret(value::Input::raw(input), &mut session);
    let target = session
        .finish(result)
        .unwrap_or_else(|error| panic!("{context}: Target: {error:?}"));
    assert_eq!(
        target_projection(&target, &context),
        *expected,
        "{context}: complete Target projection"
    );
    // The selected five target instances have no ignored fields upstream.
    // Require the reference to confirm this, then check the actual visitor.
    let prefix = format!("{scope}.");
    assert!(
        row["observed_unused"]
            .as_array()
            .expect("reference unused set")
            .iter()
            .all(|path| !path.as_str().expect("unused path").starts_with(&prefix)),
        "{context}: expand unused-path comparison for new reference cases"
    );
    assert!(
        target.unused().is_empty(),
        "{context}: unexpected ignored Target fields"
    );
}

#[test]
fn targets_match_complete_pinned_typed_fields() {
    let mut compared = Vec::new();
    for row in observations() {
        if row["accepted"] != true {
            continue;
        }
        let expected = &row["typed"]["fields"];
        let targets: Vec<_> = ["lib", "bin", "example", "test", "bench"]
            .into_iter()
            .filter_map(|field| some(&expected[field], id(&row)).map(|expected| (field, expected)))
            .collect();
        if targets.is_empty() {
            continue;
        }
        let mut account = Account;
        let document = parse(&row, &mut account);
        for (field, expected) in targets {
            let input = document
                .table()
                .get(field)
                .unwrap_or_else(|| panic!("{}: missing source {field}", id(&row)));
            if field == "lib" {
                compare_target(&row, field, input, expected, &mut account);
                compared.push((id(&row).to_owned(), field.to_owned()));
            } else {
                let document::Value::Array(array) = input.value() else {
                    panic!("{}: source {field} must be an array", id(&row))
                };
                let expected = expected.as_array().expect("reference target array");
                assert_eq!(
                    array.values().len(),
                    expected.len(),
                    "{}: {field} length",
                    id(&row)
                );
                for (index, (input, expected)) in array.values().iter().zip(expected).enumerate() {
                    let scope = format!("{field}.{index}");
                    compare_target(&row, &scope, input, expected, &mut account);
                    compared.push((id(&row).to_owned(), scope));
                }
            }
        }
    }
    assert_eq!(
        compared,
        ["lib", "bin.0", "example.0", "test.0", "bench.0"]
            .map(|scope| ("complete-families".to_owned(), scope.to_owned())),
        "the reference must exercise all five target positions; review additions explicitly"
    );
}

pub(super) fn generic_projection(
    projection: &value::Projection<'_>,
    index: usize,
    context: &str,
) -> Value {
    let node = &projection.nodes.as_slice()[index];
    match node.kind {
        value::Kind::String(text) => tagged("TomlValue::String", json!(text.as_str())),
        value::Kind::I64(value) => tagged("TomlValue::Integer", json!(value)),
        value::Kind::Float(value) => tagged(
            "TomlValue::Float",
            json!({
                "bits": format!("{:016x}", value.to_bits()),
                "class": if value.is_nan() { "nan" } else if value.is_infinite() { "infinite" } else { "finite" },
                "negative": value.is_sign_negative()
            }),
        ),
        value::Kind::Boolean(value) => tagged("TomlValue::Boolean", json!(value)),
        value::Kind::DateTime(value) => tagged(
            "TomlValue::Datetime",
            json!({
                "text": value.to_string(),
                "date": value.date.map(|v| json!({"year":v.year,"month":v.month,"day":v.day})),
                "time": value.time.map(|v| json!({"hour":v.hour,"minute":v.minute,
                    "second":v.second,"nanosecond":v.nanosecond})),
                // toml_edit only supplies the existing public type name here; no
                // second parser or conversion is invoked by this transport.
                "offset": value.offset.map(|v| match v {
                    toml_edit::Offset::Z => tagged("Offset::Z", Value::Null),
                    toml_edit::Offset::Custom { minutes } => tagged("Offset::Custom", json!(minutes)),
                })
            }),
        ),
        value::Kind::Sequence(sequence) => tagged(
            "TomlValue::Array",
            Value::Array(
                children(projection, sequence, context)
                    .into_iter()
                    .map(|edge| {
                        assert!(edge.key.is_none(), "{context}: generic array key");
                        generic_projection(projection, edge.child, context)
                    })
                    .collect(),
            ),
        ),
        value::Kind::Map(map) => tagged(
            "TomlValue::Table",
            tagged(
                "TomlTable",
                Value::Array(
                    children(projection, map, context)
                        .into_iter()
                        .map(|edge| {
                            let key = edge
                                .key
                                .unwrap_or_else(|| panic!("{context}: generic table key"));
                            json!([
                                key.text.as_str(),
                                generic_projection(projection, edge.child, context)
                            ])
                        })
                        .collect(),
                ),
            ),
        ),
        other => panic!("{context}: generic TOML retained an unsupported scalar {other:?}"),
    }
}

#[test]
fn direct_metadata_matches_complete_pinned_generic_values() {
    let mut compared = Vec::new();
    for row in observations() {
        if row["accepted"] != true {
            continue;
        }
        let metadata: Vec<_> = ["package", "workspace"]
            .into_iter()
            .filter_map(|owner| {
                let expected = some(&row["typed"]["fields"][owner], id(&row))?;
                some(&expected["fields"]["metadata"], id(&row)).map(|expected| (owner, expected))
            })
            .collect();
        if metadata.is_empty() {
            continue;
        }
        let mut account = Account;
        let document = parse(&row, &mut account);
        for (owner, expected) in metadata {
            let context = format!("{}: {owner}.metadata", id(&row));
            let owner_input = document.table().get(owner).expect("source metadata owner");
            let input = table(owner_input, &context)
                .get("metadata")
                .expect("source metadata value");
            let projection =
                walk::project(value::Input::raw(input), value::Mode::Toml, &mut account)
                    .unwrap_or_else(|error| panic!("{context}: generic conversion: {error:?}"));
            assert_eq!(
                generic_projection(&projection, 0, &context),
                *expected,
                "{context}: complete generic projection"
            );
            compared.push((id(&row).to_owned(), owner.to_owned()));
        }
    }
    assert_eq!(
        compared,
        [
            ("complete-families", "package"),
            ("workspace-and-inheritance", "workspace"),
            ("metadata-i64-min", "package"),
            ("metadata-float-special", "package"),
            ("datetime-native", "package"),
            ("datetime-marker-first", "package"),
            ("datetime-marker-later", "package"),
            ("datetime-marker-sequence", "package"),
        ]
        .map(|(id, owner)| (id.to_owned(), owner.to_owned())),
        "review generic comparison scope explicitly when the corpus changes"
    );
}

#[test]
fn isolated_subvisitor_rejections_match_pinned_spans_and_categories() {
    let rows = observations();
    let mut compared = 0;
    // Each exact source contains only the selected field. Its pinned typed
    // error therefore unambiguously belongs to the invoked subvisitor. No
    // whole-manifest error precedence is inferred from these narrow calls.
    for (case, expected_tag, message) in [
        (
            "target-positional-empty",
            Tag::Missing,
            "invalid length 0, expected struct TomlTarget with 15 elements",
        ),
        (
            "metadata-u64-over-i64",
            Tag::Integer64,
            "u64 value was too large",
        ),
        (
            "metadata-float-overflow",
            Tag::FloatOverflow,
            "floating-point number overflowed",
        ),
        (
            "datetime-marker-invalid",
            Tag::DateTime,
            "invalid datetime, expected year or hour",
        ),
    ] {
        let row = rows
            .iter()
            .find(|row| id(row) == case)
            .unwrap_or_else(|| panic!("missing scoped rejected reference {case}"));
        assert_eq!(row["accepted"], false, "{case}: reference rejection");
        assert_eq!(row["error"]["phase"], "typed", "{case}: reference phase");
        assert_eq!(
            row["error"]["message"], message,
            "{case}: review category mapping if upstream error changes"
        );
        let mut account = Account;
        let document = parse(row, &mut account);
        assert_eq!(
            document.table().len(),
            1,
            "{case}: isolated selected root field"
        );
        let result = if case == "target-positional-empty" {
            let input = document.table().get("lib").expect("source lib");
            let mut session = session::Session::new(&mut account);
            let result = schema::Target::interpret(value::Input::raw(input), &mut session);
            session.finish(result).map(|_| ())
        } else {
            let owner = table(
                document.table().get("package").expect("source package"),
                case,
            );
            assert_eq!(owner.len(), 1, "{case}: isolated metadata field");
            let input = owner.get("metadata").expect("source metadata");
            walk::project(value::Input::raw(input), value::Mode::Toml, &mut account).map(|_| ())
        };
        let Err(Error::Invalid(error)) = result else {
            panic!("{case}: expected semantic rejection, got {result:?}")
        };
        assert_eq!(error.tag, expected_tag, "{case}: bounded category");
        assert_eq!(
            json!(error.span.map(|span| [span.start, span.end])),
            row["error"]["span"],
            "{case}: applicable error span"
        );
        compared += 1;
    }
    assert_eq!(compared, 4, "all four scoped rejecting visitors must run");
}
