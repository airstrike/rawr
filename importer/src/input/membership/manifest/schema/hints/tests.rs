use rawr_toml::document;
use serde_json::{json, Value};

use super::super::super::{reference, session, storage, value, walk, Error, Stop, Tag};
use super::Hints;

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}

impl document::Admission for Account {
    type Error = &'static str;
    fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err("hints refusal")
        } else {
            Ok(())
        }
    }
}

fn parse(source: &str) -> document::Document<'_> {
    document::parse(source, 80, &mut Account::default()).unwrap()
}

fn input<'d>(document: &'d document::Document<'d>) -> value::Input<'d, 'd> {
    value::Input::raw(document.table().get("value").unwrap())
}

fn interpret<'d>(
    input: value::Input<'_, 'd>,
    account: &mut Account,
) -> Result<Hints<'d>, Error<&'static str>> {
    let mut session = session::Session::new(account);
    let result = Hints::interpret(input, &mut session);
    session.finish(result)
}

#[test]
fn map_absence_differs_from_a_missing_positional_option() {
    let document = parse("value={}");
    let output = interpret(input(&document), &mut Account::default()).unwrap();
    assert!(output.mostly_unused().is_none());
    assert!(output.declared().is_none());
    let document = parse("value=[]");
    assert!(
        matches!(interpret(input(&document), &mut Account::default()), Err(Error::Invalid(error)) if error.tag == Tag::Missing)
    );
    let document = parse("value=[false]");
    let output = interpret(input(&document), &mut Account::default()).unwrap();
    assert!(output.declared().is_some());
    assert!(matches!(
        output.mostly_unused().unwrap().nodes.as_slice()[0].kind,
        value::Kind::Boolean(false)
    ));
}

#[test]
fn raw_and_serde_value_sequences_leave_trailing_values_to_their_adapter() {
    let document = parse("value=[true,340282366920938463463374607431768211456]");
    assert!(interpret(input(&document), &mut Account::default()).is_ok());
    let document = parse("value=[true,false]");
    for mode in [
        value::Mode::SerdeValue,
        value::Mode::Content,
        value::Mode::Toml,
    ] {
        let captured = walk::project(input(&document), mode, &mut Account::default()).unwrap();
        let result = interpret(value::Input::replay(&captured, 0), &mut Account::default());
        if mode == value::Mode::SerdeValue {
            assert!(result.is_ok());
        } else {
            assert!(
                matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::Length),
                "{mode:?}"
            );
        }
    }
}

#[test]
fn unknown_callbacks_preserve_order_and_still_convert_raw_ignored_values() {
    let document = parse("value={z=1,a={x=true},mostly-unused='kept'}");
    let output = interpret(input(&document), &mut Account::default()).unwrap();
    assert_eq!(
        output
            .unused()
            .iter()
            .map(|key| key.text.as_str())
            .collect::<Vec<_>>(),
        ["z", "a"]
    );
    assert_eq!(output.declared().unwrap().text.as_str(), "mostly-unused");
    let document = parse("value={unknown=340282366920938463463374607431768211456}");
    assert!(
        matches!(interpret(input(&document), &mut Account::default()), Err(Error::Invalid(error)) if error.tag == Tag::IntegerOverflow)
    );
    let document = parse("value=2026-09-05");
    let output = interpret(input(&document), &mut Account::default()).unwrap();
    assert!(output.mostly_unused().is_none());
    assert_eq!(output.unused()[0].text.as_str(), "$__toml_private_datetime");
}

#[test]
fn hints_retain_the_converted_generic_value_and_borrow_decoded_text() {
    let document =
        parse("value={mostly-unused={text=\"escaped\\ntext\",date=2026-09-05T01:02:03Z}}");
    let output = interpret(input(&document), &mut Account::default()).unwrap();
    let converted = output.mostly_unused().unwrap();
    let value::Kind::Map(children) = converted.nodes.as_slice()[0].kind else {
        panic!("map")
    };
    let first = &converted.edges.as_slice()[children.first.unwrap()];
    let value::Kind::String(value::Text::Borrowed(text)) =
        converted.nodes.as_slice()[first.child].kind
    else {
        panic!("borrowed text")
    };
    let document::Value::Table(hints) = document.table().get("value").unwrap().value() else {
        panic!("source hints")
    };
    let document::Value::Table(metadata) = hints.get("mostly-unused").unwrap().value() else {
        panic!("source metadata")
    };
    let document::Value::String(raw) = metadata.get("text").unwrap().value() else {
        panic!("source text")
    };
    assert_eq!(text.as_ptr(), raw.as_str().as_ptr());
    let last = &converted.edges.as_slice()[children.last.unwrap()];
    assert!(matches!(
        converted.nodes.as_slice()[last.child].kind,
        value::Kind::DateTime(_)
    ));
}

#[test]
fn replayed_known_duplicates_reject_before_the_duplicate_value() {
    let document = parse("value={mostly-unused=true,other=1}");
    let mut captured = walk::project(
        input(&document),
        value::Mode::Direct,
        &mut Account::default(),
    )
    .unwrap();
    let duplicate = captured
        .edges
        .as_mut_slice()
        .iter_mut()
        .find(|edge| edge.key.is_some_and(|key| key.text.as_str() == "other"))
        .unwrap();
    duplicate.key.as_mut().unwrap().text = value::Text::Borrowed("mostly-unused");
    assert!(
        matches!(interpret(value::Input::replay(&captured, 0), &mut Account::default()), Err(Error::Invalid(error)) if error.tag == Tag::Duplicate)
    );
}

#[test]
fn every_hints_refusal_latches_before_further_ownership() {
    let document = parse(
        "value={unknown={x=[1,2]},mostly-unused={z=['a','b','c','d','e'],a=2026-09-05T01:02:03Z}}",
    );
    let prior = vec![document::Request::Items(7), document::Request::String(11)];
    let mut full = Account {
        requests: prior.clone(),
        refusal: None,
    };
    interpret(input(&document), &mut full).unwrap();
    for refusal in prior.len()..full.requests.len() {
        let mut account = Account {
            requests: prior.clone(),
            refusal: Some(refusal),
        };
        storage::ALLOCATIONS.with(|count| count.set(0));
        let mut session = session::Session::new(&mut account);
        let result = Hints::interpret(input(&document), &mut session);
        assert!(matches!(result, Err(Stop::Refused)), "request {refusal}");
        assert!(matches!(
            Hints::interpret(input(&document), &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            session.finish(result),
            Err(Error::Admission("hints refusal"))
        ));
        assert_eq!(
            account.requests,
            full.requests[..=refusal],
            "request {refusal}"
        );
        let allocations = full.requests[prior.len()..refusal]
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count();
        assert_eq!(
            storage::ALLOCATIONS.with(|count| count.get()),
            allocations,
            "request {refusal}"
        );
    }
}

#[test]
fn complete_hints_match_the_committed_pinned_projection() {
    let mut compared = Vec::new();
    for row in reference::observations() {
        if row["accepted"] != true {
            continue;
        }
        let context = reference::id(&row);
        let Some(expected) = reference::some(&row["typed"]["fields"]["hints"], context) else {
            continue;
        };
        let mut account = reference::Account;
        let document = reference::parse(&row, &mut account);
        let mut session = session::Session::new(&mut account);
        let result = Hints::interpret(
            value::Input::raw(document.table().get("hints").unwrap()),
            &mut session,
        );
        let output = session
            .finish(result)
            .unwrap_or_else(|error| panic!("{context}: {error:?}"));
        let mostly_unused = match output.mostly_unused() {
            Some(value) => {
                reference::tagged("Some", reference::generic_projection(value, 0, context))
            }
            None => reference::tagged("None", Value::Null),
        };
        assert_eq!(
            json!({"type":"Hints","fields":{"mostly_unused":mostly_unused}}),
            *expected,
            "{context}: complete Hints"
        );
        let unused: Vec<_> = output
            .unused()
            .iter()
            .map(|key| format!("hints.{}", key.text.as_str()))
            .collect();
        let expected_unused: Vec<_> = row["unused_callbacks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap())
            .filter(|path| path.starts_with("hints."))
            .collect();
        assert_eq!(unused, expected_unused, "{context}: ignored callbacks");
        compared.push(context.to_owned());
    }
    assert_eq!(
        compared,
        [
            "complete-families",
            "hints-positional",
            "hints-positional-extra"
        ],
        "review exact Hints comparison scope"
    );
}

#[test]
fn isolated_hints_failure_matches_the_pinned_span() {
    let rows = reference::observations();
    let row = rows
        .iter()
        .find(|row| reference::id(row) == "hints-positional-missing")
        .unwrap();
    assert_eq!(row["accepted"], false);
    assert_eq!(row["error"]["phase"], "typed");
    assert_eq!(
        row["error"]["message"],
        "invalid length 0, expected struct Hints with 1 element"
    );
    let mut account = reference::Account;
    let document = reference::parse(row, &mut account);
    assert_eq!(document.table().len(), 1);
    let mut session = session::Session::new(&mut account);
    let result = Hints::interpret(
        value::Input::raw(document.table().get("hints").unwrap()),
        &mut session,
    );
    let Err(Error::Invalid(error)) = session.finish(result) else {
        panic!("hints-positional-missing: semantic error")
    };
    assert_eq!(error.tag, Tag::Missing);
    assert_eq!(
        json!(error.span.map(|span| [span.start, span.end])),
        row["error"]["span"],
        "hints-positional-missing"
    );
}
