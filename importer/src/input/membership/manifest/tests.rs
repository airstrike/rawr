use rawr_toml::document;

use super::{schema, session, value, walk, Error, Stop, Tag};
use crate::input;

mod resources;
mod source;

/// One short of an exact accounting target still admits the whole
/// charge. The account records only that axis, and its high water is the
/// exact total, so the figure a refusal used to prove is still proven.
pub(super) fn one_short(record: &crate::exceedance::Record, resource: &str, exact: usize) -> bool {
    let mut axes = record.axes();
    matches!(
        (axes.next(), axes.next()),
        (Some(axis), None) if axis.name == resource && axis.high_water == exact as u64
    )
}

/// Assert that one accounting axis one short of `exact` admits the whole
/// charge, spends exactly `exact`, and records only that axis at `total`.
pub(super) fn admits_one_short(
    (okay, usage, exceeded): (bool, input::Usage, crate::exceedance::Record),
    exact: input::Usage,
    resource: &str,
    total: usize,
) {
    assert!(
        okay,
        "{resource}: an accounting target is not an admission gate"
    );
    assert_eq!(usage, exact, "{resource}: the exact charge is still spent");
    assert!(
        one_short(&exceeded, resource, total),
        "{resource}: {exceeded:?}"
    );
}

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
            Err("original refusal")
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

fn keys<'a>(projection: &'a value::Projection<'_>, index: usize) -> Vec<&'a str> {
    let value::Kind::Map(children) = projection.nodes.as_slice()[index].kind else {
        panic!("map")
    };
    let mut next = children.first;
    let mut keys = Vec::new();
    while let Some(index) = next {
        let edge = &projection.edges.as_slice()[index];
        keys.push(edge.key.as_ref().unwrap().text.as_str());
        next = edge.next;
    }
    keys
}

#[test]
fn adapter_integer_domains_remain_distinct() {
    for (spelling, content, toml) in [
        ("9223372036854775807", true, true),
        ("9223372036854775808", true, false),
        ("18446744073709551615", true, false),
        ("18446744073709551616", false, false),
        ("-9223372036854775809", false, false),
        ("340282366920938463463374607431768211455", false, false),
    ] {
        let source = format!("value={spelling}");
        let document = parse(&source);
        for mode in [
            value::Mode::Direct,
            value::Mode::Ignored,
            value::Mode::Content,
            value::Mode::SerdeValue,
            value::Mode::Toml,
        ] {
            let result = walk::project(input(&document), mode, &mut Account::default());
            let accepted = match mode {
                value::Mode::Direct | value::Mode::Ignored => true,
                value::Mode::Content | value::Mode::SerdeValue => content,
                value::Mode::Toml => toml,
            };
            assert_eq!(result.is_ok(), accepted, "{spelling} {mode:?}");
        }
    }
}

#[test]
fn ignored_values_still_reject_dispatcher_overflow() {
    for (source, expected) in [
        (
            "value=[340282366920938463463374607431768211456]",
            Tag::IntegerOverflow,
        ),
        ("value={x=1e9999}", Tag::FloatOverflow),
    ] {
        let document = parse(source);
        assert!(
            matches!(walk::project(input(&document), value::Mode::Ignored, &mut Account::default()), Err(Error::Invalid(error)) if error.tag == expected)
        );
    }
}

#[test]
fn serde_value_capture_sorts_only_after_source_order_conversion() {
    let document = parse("value={z={b=1,a=2},a=3}");
    let content = walk::project(
        input(&document),
        value::Mode::Content,
        &mut Account::default(),
    )
    .unwrap();
    assert_eq!(keys(&content, 0), ["z", "a"]);
    let sorted = walk::project(
        input(&document),
        value::Mode::SerdeValue,
        &mut Account::default(),
    )
    .unwrap();
    assert_eq!(keys(&sorted, 0), ["a", "z"]);
    assert_eq!(keys(&sorted, 1), ["a", "b"]);

    let document = parse("value={z=18446744073709551616,a=1e9999}");
    assert!(
        matches!(walk::project(input(&document), value::Mode::SerdeValue, &mut Account::default()), Err(Error::Invalid(error)) if error.tag == Tag::Integer128)
    );
}

#[test]
fn datetime_marker_early_return_differs_from_intermediate_capture() {
    let document = parse("value={'$__toml_private_datetime'='2024-01-02T03:04:05.1200Z', later=340282366920938463463374607431768211456}");
    let generic =
        walk::project(input(&document), value::Mode::Toml, &mut Account::default()).unwrap();
    assert!(matches!(
        generic.nodes.as_slice()[0].kind,
        value::Kind::DateTime(_)
    ));
    assert_eq!(generic.nodes.len(), 1);
    assert!(
        matches!(walk::project(input(&document), value::Mode::Content, &mut Account::default()), Err(Error::Invalid(error)) if error.tag == Tag::IntegerOverflow)
    );
}

#[test]
fn datetime_capture_uses_the_normalized_synthetic_string() {
    let document = parse("value=2024-01-02t03:04:05.1200z");
    let captured = walk::project(
        input(&document),
        value::Mode::SerdeValue,
        &mut Account::default(),
    )
    .unwrap();
    assert_eq!(keys(&captured, 0), ["$__toml_private_datetime"]);
    let value::Kind::String(text) = &captured.nodes.as_slice()[1].kind else {
        panic!("date string")
    };
    assert_eq!(text.as_str(), "2024-01-02T03:04:05.12Z");
    let replayed = walk::project(
        value::Input::replay(&captured, 0),
        value::Mode::Toml,
        &mut Account::default(),
    )
    .unwrap();
    assert!(matches!(
        replayed.nodes.as_slice()[0].kind,
        value::Kind::DateTime(_)
    ));
}

#[test]
fn generic_toml_datetime_replays_as_string_while_raw_datetime_is_a_map() {
    let document = parse("value=2024-01-02t03:04:05.1200z");
    let toml = walk::project(input(&document), value::Mode::Toml, &mut Account::default()).unwrap();
    for mode in [
        value::Mode::Direct,
        value::Mode::Content,
        value::Mode::SerdeValue,
        value::Mode::Toml,
    ] {
        let replayed = walk::project(
            value::Input::replay(&toml, 0),
            mode,
            &mut Account::default(),
        )
        .unwrap();
        assert_eq!(replayed.nodes.len(), 1);
        let value::Kind::String(text) = &replayed.nodes.as_slice()[0].kind else {
            panic!("normalized string")
        };
        assert_eq!(text.as_str(), "2024-01-02T03:04:05.12Z");
    }
    let raw = walk::project(
        input(&document),
        value::Mode::Content,
        &mut Account::default(),
    )
    .unwrap();
    assert_eq!(keys(&raw, 0), ["$__toml_private_datetime"]);
}

#[test]
fn content_ignored_replay_is_unit_while_other_adapters_consume_descendants() {
    let document = parse("value={a=[{b=1}],d=2024-01-02}");
    for mode in [
        value::Mode::Direct,
        value::Mode::Content,
        value::Mode::SerdeValue,
        value::Mode::Toml,
    ] {
        let captured = walk::project(input(&document), mode, &mut Account::default()).unwrap();
        let mut account = Account::default();
        let ignored = walk::project(
            value::Input::replay(&captured, 0),
            value::Mode::Ignored,
            &mut account,
        )
        .unwrap();
        assert!(ignored.nodes.as_slice().is_empty());
        assert_eq!(account.requests.is_empty(), mode == value::Mode::Content);
        if mode == value::Mode::Content {
            let mut session = session::Session::new(&mut account);
            assert!(matches!(
                session.fail::<()>(Error::Overflow),
                Err(Stop::Refused)
            ));
            let result = walk::run(
                value::Input::replay(&captured, 0),
                value::Mode::Ignored,
                &mut session,
            );
            assert!(matches!(session.finish(result), Err(Error::Overflow)));
            assert!(account.requests.is_empty());
        }
    }
}

#[test]
fn remaining_map_entries_are_rejected_only_by_exhausting_replay_adapters() {
    let document = parse("value={a=1,b=2}");
    for mode in [
        value::Mode::Direct,
        value::Mode::Content,
        value::Mode::SerdeValue,
        value::Mode::Toml,
    ] {
        let captured = walk::project(input(&document), mode, &mut Account::default()).unwrap();
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let value::View::Map(mut cursor) = value::Input::replay(&captured, 0)
            .view(&mut session)
            .unwrap()
        else {
            panic!("map")
        };
        cursor.next(&mut session).unwrap();
        let result = cursor.finish(None, &mut session);
        assert_eq!(
            session.finish(result).is_err(),
            matches!(mode, value::Mode::Content | value::Mode::Toml)
        );
    }
}

#[test]
fn content_map_requires_exhaustion_but_raw_and_serde_value_do_not() {
    let document =
        parse("value={'$__toml_private_datetime'='2026-09-05',trailing=9223372036854775808}");
    walk::project(input(&document), value::Mode::Toml, &mut Account::default()).unwrap();
    let staged = walk::project(
        input(&document),
        value::Mode::SerdeValue,
        &mut Account::default(),
    )
    .unwrap();
    walk::project(
        value::Input::replay(&staged, 0),
        value::Mode::Toml,
        &mut Account::default(),
    )
    .unwrap();
    for from_staged in [false, true] {
        let source = if from_staged {
            value::Input::replay(&staged, 0)
        } else {
            input(&document)
        };
        let content = walk::project(source, value::Mode::Content, &mut Account::default()).unwrap();
        let result = walk::project(
            value::Input::replay(&content, 0),
            value::Mode::Toml,
            &mut Account::default(),
        );
        assert!(
            matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::Length && error.span.is_none())
        );
    }
}

#[test]
fn refusal_is_terminal_even_if_a_caller_discards_a_trial_result() {
    let document = parse("value={a=[1,2,3]}");
    let mut account = Account {
        refusal: Some(2),
        ..Account::default()
    };
    let mut session = session::Session::new(&mut account);
    assert!(matches!(
        walk::run(input(&document), value::Mode::SerdeValue, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        walk::run(input(&document), value::Mode::Ignored, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Admission("original refusal"))
    ));
    assert_eq!(account.requests.len(), 3);
}

#[test]
fn target_visits_both_aliases_and_ignores_only_after_conversion() {
    let document = parse("value={crate-type=['lib'],crate_type=['rlib'],proc-macro=false,proc_macro=true,unknown={n=1}}");
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = schema::Target::interpret(input(&document), &mut session);
    let target = session.finish(result).unwrap();
    assert_eq!(
        keys(target.values(), 0),
        ["crate-type", "crate_type", "proc-macro", "proc_macro"]
    );
    assert_eq!(target.unused()[0].text.as_str(), "unknown");

    let document = parse("value={name='okay',crate_type=[false],unknown=1e9999}");
    let mut session = session::Session::new(&mut account);
    let result = schema::Target::interpret(input(&document), &mut session);
    assert!(matches!(session.finish(result), Err(Error::Invalid(error)) if error.tag == Tag::Type));
}

#[test]
fn target_positional_fields_do_not_consume_trailing_values() {
    let document =
        parse("value=['n',[],[],'p','f',true,true,true,true,true,true,true,true,[],'2021',1e9999]");
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = schema::Target::interpret(input(&document), &mut session);
    assert!(session.finish(result).is_ok());
    let document = parse("value=[]");
    let mut session = session::Session::new(&mut account);
    let result = schema::Target::interpret(input(&document), &mut session);
    assert!(
        matches!(session.finish(result), Err(Error::Invalid(error)) if error.tag == Tag::Missing)
    );
}

#[test]
fn target_positional_content_and_toml_require_exhaustion_after_the_fields() {
    let document =
        parse("value=['n',[],[],'p','f',true,true,true,true,true,true,true,true,[],'2021',true]");
    for mode in [
        value::Mode::Direct,
        value::Mode::SerdeValue,
        value::Mode::Content,
        value::Mode::Toml,
    ] {
        let captured = walk::project(input(&document), mode, &mut Account::default()).unwrap();
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = schema::Target::interpret(value::Input::replay(&captured, 0), &mut session);
        let result = session.finish(result);
        if matches!(mode, value::Mode::Content | value::Mode::Toml) {
            assert!(
                matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::Length && error.span.is_none())
            );
        } else {
            assert!(result.is_ok());
        }
    }
}

#[test]
fn trim_paths_completes_the_string_pass_before_enum_conversion() {
    for (source, expected) in [
        ("value=['bad',false]", Tag::Type),
        ("value=['bad','macro']", Tag::Variant),
    ] {
        let document = parse(source);
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = schema::Paths::interpret(input(&document), &mut session);
        assert!(
            matches!(session.finish(result), Err(Error::Invalid(error)) if error.tag == expected && error.span == input(&document).span)
        );
    }
    let document = parse("value=['diagnostics','macro','object']");
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = schema::Paths::interpret(input(&document), &mut session);
    let schema::Paths::Values(values) = session.finish(result).unwrap() else {
        panic!("trim values")
    };
    assert_eq!(
        values.as_slice(),
        [
            schema::Trim::Diagnostics,
            schema::Trim::Macro,
            schema::Trim::Object
        ]
    );
}
