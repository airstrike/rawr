use rawr_toml::document;

use super::super::super::{session, storage, value, walk, Error, Stop, Tag};
use super::{Inherited, Level, Lint, Table};

pub(in crate::input::membership::manifest) mod reference;

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
            Err("lint refusal")
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

fn inherited<'d>(
    document: &'d document::Document<'d>,
    account: &mut Account,
) -> Result<Inherited<'d>, Error<&'static str>> {
    let mut session = session::Session::new(account);
    let result = Inherited::interpret(input(document), &mut session);
    session.finish(result)
}

fn lint<'d>(
    input: value::Input<'_, 'd>,
    account: &mut Account,
) -> Result<Lint<'d>, Error<&'static str>> {
    let mut session = session::Session::new(account);
    let result = Lint::interpret(input, &mut session);
    session.finish(result)
}

#[test]
fn workspace_presence_and_ordinary_tool_interpretation_remain_distinct() {
    for (source, workspace, declared) in [
        ("value={}", false, false),
        ("value={workspace=true}", true, true),
        ("value={workspace=true,rust={unused='warn'}}", true, true),
    ] {
        let document = parse(source);
        let output = inherited(&document, &mut Account::default()).unwrap();
        assert_eq!(output.workspace(), workspace, "{source}");
        assert_eq!(output.declared().is_some(), declared, "{source}");
    }
    let document = parse("value={workspace=false}");
    assert!(
        matches!(inherited(&document, &mut Account::default()), Err(Error::Invalid(error)) if error.tag == Tag::WorkspaceFalse)
    );
    let document = parse("value={workspace={rule='allow'}}");
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = Table::interpret(input(&document), &mut session);
    let ordinary = session.finish(result).unwrap();
    assert_eq!(
        ordinary.tools().next().unwrap().0.text.as_str(),
        "workspace"
    );
    assert!(
        matches!(inherited(&document, &mut Account::default()), Err(Error::Invalid(error)) if error.tag == Tag::Type)
    );
}

#[test]
fn tools_and_rules_sort_after_conversion_while_config_extras_keep_order() {
    let document = parse("value={z={z='deny',a={level='warn',z=1,a=2}},a={b='forbid'}}");
    let output = inherited(&document, &mut Account::default()).unwrap();
    assert_eq!(
        output
            .table()
            .tools()
            .map(|(key, _)| key.text.as_str())
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    let tool = output.table().tools().nth(1).unwrap().1;
    assert_eq!(
        tool.rules()
            .map(|(key, _)| key.text.as_str())
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    let Lint::Config(config) = tool.rules().next().unwrap().1 else {
        panic!("config")
    };
    assert_eq!(config.level(), Level::Warn);
    assert_eq!(config.priority(), 0);
    assert!(config.priority_key().is_none());
    assert_eq!(config.level_key().text.as_str(), "level");
    assert_eq!(
        config
            .extras()
            .map(|(key, _)| key.text.as_str())
            .collect::<Vec<_>>(),
        ["z", "a"]
    );
}

#[test]
fn flattened_capture_precedes_missing_fields_and_later_generic_conversion() {
    for (source, tag) in [
        ("value={extra=18446744073709551616}", Tag::Integer128),
        ("value={extra=9223372036854775808}", Tag::Missing),
        ("value={extra=9223372036854775808,priority=128}", Tag::Type),
        (
            "value={extra=9223372036854775808,level='warn'}",
            Tag::Integer64,
        ),
        (
            "value={level='warn',extra={'$__toml_private_datetime'='2026-09-05',trailing=1}}",
            Tag::Length,
        ),
    ] {
        let document = parse(source);
        let value = input(&document);
        let Err(Error::Invalid(error)) = lint(value, &mut Account::default()) else {
            panic!("{source}")
        };
        assert_eq!(error.tag, tag, "{source}");
        assert_eq!(
            error.span, value.span,
            "erased untagged error uses outer span: {source}"
        );
    }
}

#[test]
fn flattened_values_outlive_local_capture_and_preserve_toml_table_semantics() {
    let document = parse("value={level='warn',text=\"decoded\\ntext\",date=2026-09-05T01:02:03Z,'$__toml_private_datetime'='ordinary extra key'}");
    let output = lint(input(&document), &mut Account::default()).unwrap();
    let Lint::Config(config) = output else {
        panic!("config")
    };
    let mut extras = config.extras();
    let (_, text) = extras.next().unwrap();
    let value::Kind::String(value::Text::Borrowed(text)) = text.nodes.as_slice()[0].kind else {
        panic!("borrowed text")
    };
    let document::Value::Table(raw) = document.table().get("value").unwrap().value() else {
        panic!("source table")
    };
    let document::Value::String(original) = raw.get("text").unwrap().value() else {
        panic!("source text")
    };
    assert_eq!(text.as_ptr(), original.as_str().as_ptr());
    assert_eq!(text, "decoded\ntext");
    assert!(matches!(
        extras.next().unwrap().1.nodes.as_slice()[0].kind,
        value::Kind::DateTime(_)
    ));
    let (key, extra) = extras.next().unwrap();
    assert_eq!(key.text.as_str(), "$__toml_private_datetime");
    assert!(matches!(
        extra.nodes.as_slice()[0].kind,
        value::Kind::String(_)
    ));
}

#[test]
fn enum_shape_and_cardinality_reject_before_unselected_scalar_conversion() {
    for (source, tag) in [
        (
            "value={level=340282366920938463463374607431768211456}",
            Tag::Type,
        ),
        ("value={level=2026-09-05}", Tag::Type),
        (
            "value={level={bad=340282366920938463463374607431768211456,warn=[]}}",
            Tag::Length,
        ),
        (
            "value={level={bad=340282366920938463463374607431768211456}}",
            Tag::Variant,
        ),
        (
            "value={level={warn=340282366920938463463374607431768211456}}",
            Tag::Type,
        ),
        (
            "value={level={warn=[340282366920938463463374607431768211456]}}",
            Tag::Length,
        ),
    ] {
        let document = parse(source);
        assert!(
            matches!(lint(input(&document), &mut Account::default()), Err(Error::Invalid(error)) if error.tag == tag),
            "{source}"
        );
    }
}

#[test]
fn raw_and_toml_unit_payloads_differ_from_content_and_serde_value() {
    for source in ["value={level={warn=[]}}", "value={level={warn={}}}"] {
        let document = parse(source);
        assert!(lint(input(&document), &mut Account::default()).is_ok());
        for mode in [
            value::Mode::Content,
            value::Mode::SerdeValue,
            value::Mode::Toml,
        ] {
            let captured = walk::project(input(&document), mode, &mut Account::default()).unwrap();
            let result = lint(value::Input::replay(&captured, 0), &mut Account::default());
            if mode == value::Mode::Toml {
                assert!(result.is_ok(), "{source}: {mode:?}");
            } else {
                assert!(
                    matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::Type),
                    "{source}: {mode:?}"
                );
            }
        }
    }
}

#[test]
fn priority_uses_only_the_pinned_i8_visitor_callbacks() {
    for priority in [-128, 0, 127] {
        let source = format!("value={{level='warn',priority={priority}}}");
        let document = parse(&source);
        let Lint::Config(config) = lint(input(&document), &mut Account::default()).unwrap() else {
            panic!("config")
        };
        assert_eq!(config.priority(), priority);
        assert!(config.priority_key().is_some());
    }
    for kind in [value::Kind::I128(1), value::Kind::U128(1)] {
        let document = parse("value={level='warn',priority=1}");
        let mut captured = walk::project(
            input(&document),
            value::Mode::Direct,
            &mut Account::default(),
        )
        .unwrap();
        let number = captured
            .nodes
            .as_mut_slice()
            .iter_mut()
            .find(|node| matches!(node.kind, value::Kind::I64(1)))
            .unwrap();
        number.kind = kind;
        assert!(
            matches!(lint(value::Input::replay(&captured, 0), &mut Account::default()), Err(Error::Invalid(error)) if error.tag == Tag::Type)
        );
    }
}

#[test]
fn replayed_known_duplicates_reject_before_the_duplicate_value() {
    let document = parse("value={level='warn',other=1}");
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
    duplicate.key.as_mut().unwrap().text = value::Text::Borrowed("level");
    assert!(
        matches!(lint(value::Input::replay(&captured, 0), &mut Account::default()), Err(Error::Invalid(error)) if error.tag == Tag::Duplicate)
    );
}

#[test]
fn map_replacement_preserves_first_key_and_last_converted_value() {
    let document = parse("value={b={z='warn'},a={x='deny'},c={q='allow'}}");
    let mut captured = walk::project(
        input(&document),
        value::Mode::Direct,
        &mut Account::default(),
    )
    .unwrap();
    let replacement = captured
        .edges
        .as_mut_slice()
        .iter_mut()
        .find(|edge| edge.key.is_some_and(|key| key.text.as_str() == "c"))
        .unwrap();
    replacement.key.as_mut().unwrap().text = value::Text::Borrowed("b");
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = Inherited::interpret(value::Input::replay(&captured, 0), &mut session);
    let output = session.finish(result).unwrap();
    assert_eq!(
        output
            .table()
            .tools()
            .map(|(key, _)| key.text.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    let Lint::Level(Level::Allow) = output
        .table()
        .tools()
        .nth(1)
        .unwrap()
        .1
        .rules()
        .next()
        .unwrap()
        .1
    else {
        panic!("last converted tool replaces earlier value")
    };
}

#[test]
fn every_lint_refusal_latches_before_further_ownership_or_fallback() {
    let document = parse("value={workspace=true,z={a={level='warn',priority=-128,z=['a','b','c','d','e'],a={date=2026-09-05}}},a={b='deny'}}");
    let prior = vec![document::Request::Items(7), document::Request::String(11)];
    let mut full = Account {
        requests: prior.clone(),
        refusal: None,
    };
    inherited(&document, &mut full).unwrap();
    for refusal in prior.len()..full.requests.len() {
        let mut account = Account {
            requests: prior.clone(),
            refusal: Some(refusal),
        };
        storage::ALLOCATIONS.with(|count| count.set(0));
        let mut session = session::Session::new(&mut account);
        let result = Inherited::interpret(input(&document), &mut session);
        assert!(matches!(result, Err(Stop::Refused)), "request {refusal}");
        assert!(matches!(
            Inherited::interpret(input(&document), &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            session.finish(result),
            Err(Error::Admission("lint refusal"))
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
