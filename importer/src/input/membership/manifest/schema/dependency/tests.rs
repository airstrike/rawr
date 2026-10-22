use rawr_toml::document;

use super::super::super::{reference, scalar, session, value, walk, Error, Tag};
use super::super::primitive;
use super::{direct, inheritable, Detailed, Direct, Field, Inheritable, Inherited};

mod pinned;
mod resources;

fn parse(source: &str) -> document::Document<'_> {
    document::parse(source, 80, &mut reference::Account).unwrap()
}
fn input<'d>(document: &'d document::Document<'d>) -> value::Input<'d, 'd> {
    value::Input::raw(document.table().get("value").unwrap())
}
fn direct<'d>(
    document: &'d document::Document<'d>,
) -> Result<Direct<'d>, Error<std::convert::Infallible>> {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Direct::interpret(input(document), &mut session);
    session.finish(result)
}
fn inherited<'d>(
    document: &'d document::Document<'d>,
) -> Result<Inheritable<'d>, Error<std::convert::Infallible>> {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Inheritable::interpret(input(document), &mut session);
    session.finish(result)
}

#[test]
fn simple_detailed_inherited_and_absent_fields_remain_distinct() {
    let document = parse("value='not interpreted as a version requirement here'");
    let value = direct(&document).unwrap();
    assert!(
        matches!(value.kind(),direct::Kind::Simple(text) if text.as_str()=="not interpreted as a version requirement here")
    );
    assert_eq!(value.span(), input(&document).span);
    let document = parse("value={}");
    let value = direct(&document).unwrap();
    let direct::Kind::Detailed(value) = value.kind() else {
        panic!("detailed")
    };
    assert!(value.fields().is_empty());
    assert!(value.extras().next().is_none());
    let value = inherited(&document).unwrap();
    assert!(matches!(value.kind(), inheritable::Kind::Value(_)));
    let document = parse("value={workspace=true}");
    let value = inherited(&document).unwrap();
    let inheritable::Kind::Inherit(record) = value.kind() else {
        panic!("inherited")
    };
    assert!(record.workspace());
    assert_eq!(record.fields().len(), 1);
    assert!(record.get(Field::Features).is_none());
    assert_eq!(value.span(), input(&document).span);
    assert!(
        record.span().is_none(),
        "replay must not invent a nested source span"
    );
}

#[test]
fn all_six_inherited_fields_and_aliases_finish_before_false_is_rejected() {
    let document = parse("value={workspace=true,features=['f'],default-features=false,default_features=true,optional=false,public=true,extra=2}");
    let value = inherited(&document).unwrap();
    let inheritable::Kind::Inherit(record) = value.kind() else {
        panic!("inherited")
    };
    assert_eq!(record.fields().len(), 6);
    assert!(matches!(
        record.get(Field::DefaultFeatures),
        Some(primitive::Value::Boolean(false))
    ));
    assert!(matches!(
        record.get(Field::DefaultFeatures2),
        Some(primitive::Value::Boolean(true))
    ));
    assert_eq!(record.extras().next().unwrap().0.text.as_str(), "extra");
    for (source, expected) in [
        ("value={workspace=false}", Tag::WorkspaceFalse),
        (
            "value={workspace=false,registry='1invalid'}",
            Tag::WorkspaceFalse,
        ),
        ("value={workspace=false,features=1}", Tag::Type),
        (
            "value={workspace=false,unused=9223372036854775808}",
            Tag::Integer64,
        ),
        (
            "value={workspace=false,registry=9223372036854775808}",
            Tag::Type,
        ),
    ] {
        let document = parse(source);
        assert!(
            matches!(inherited(&document),Err(Error::Invalid(error)) if error.tag==expected),
            "{source}"
        );
    }
}

#[test]
fn failed_workspace_trial_can_fall_back_with_workspace_retained_as_extra() {
    let document = parse(
        "value={workspace='not-bool',version='1',default-features=false,default_features=true}",
    );
    let value = inherited(&document).unwrap();
    let inheritable::Kind::Value(direct) = value.kind() else {
        panic!("local")
    };
    let direct::Kind::Detailed(record) = direct.kind() else {
        panic!("detailed")
    };
    assert_eq!(record.fields().len(), 3);
    let (key, value) = record.extras().next().unwrap();
    assert_eq!(key.text.as_str(), "workspace");
    assert!(
        matches!(value.nodes.as_slice()[0].kind,value::Kind::String(text) if text.as_str()=="not-bool")
    );
}

#[test]
fn complete_capture_precedes_trial_and_sorted_trial_differs_from_raw_order() {
    let document = parse("value={version=false,later=18446744073709551616}");
    assert!(matches!(direct(&document),Err(Error::Invalid(error)) if error.tag==Tag::Type));
    assert!(
        matches!(inherited(&document),Err(Error::Invalid(error)) if error.tag==Tag::Integer128)
    );
    let document = parse("value={version=false,registry='1invalid'}");
    assert!(matches!(direct(&document),Err(Error::Invalid(error)) if error.tag==Tag::Type));
    assert!(
        matches!(inherited(&document),Err(Error::Invalid(error)) if matches!(error.tag,Tag::Scalar(_)))
    );
    let document = parse("value={z={z=1,a=2},a=3}");
    let value = inherited(&document).unwrap();
    let inheritable::Kind::Value(value) = value.kind() else {
        panic!("local")
    };
    let direct::Kind::Detailed(value) = value.kind() else {
        panic!("detailed")
    };
    assert_eq!(
        value
            .extras()
            .map(|(key, _)| key.text.as_str())
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    let projection = value.extras().nth(1).unwrap().1;
    let value::Kind::Map(children) = projection.nodes.as_slice()[0].kind else {
        panic!("map")
    };
    assert_eq!(
        reference::children(projection, children, "sorted nested capture")
            .iter()
            .map(|edge| edge.key.as_ref().unwrap().text.as_str())
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
}

#[test]
fn missing_workspace_precedes_flattened_generic_conversion() {
    let document = parse("value={unused=9223372036854775808}");
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Inherited::interpret(input(&document), &mut session);
    assert!(matches!(session.finish(result),Err(Error::Invalid(error)) if error.tag==Tag::Missing));
    assert!(matches!(inherited(&document),Err(Error::Invalid(error)) if error.tag==Tag::Integer64));
    let document = parse("value={unused=18446744073709551616}");
    let mut session = session::Session::new(&mut account);
    let result = Inherited::interpret(input(&document), &mut session);
    assert!(
        matches!(session.finish(result),Err(Error::Invalid(error)) if error.tag==Tag::Integer128)
    );
}

#[test]
fn flattened_marker_checks_content_exhaustion_and_outer_dependency_span() {
    let document =
        parse("value={extra={'$__toml_private_datetime'='2026-09-05',later=9223372036854775808}}");
    for result in [
        direct(&document).map(|_| ()),
        inherited(&document).map(|_| ()),
    ] {
        assert!(
            matches!(result,Err(Error::Invalid(error)) if error.tag==Tag::Length&&error.span==input(&document).span)
        );
    }
}

#[test]
fn name_grammars_are_distinct_and_key_errors_precede_value_capture() {
    for (source, kind) in [
        ("value={registry='a::b'}", scalar::Kind::Registry),
        ("value={base='a::b'}", scalar::Kind::PathBase),
        ("value={package='1bad'}", scalar::Kind::Package),
    ] {
        let document = parse(source);
        assert!(
            matches!(direct(&document),Err(Error::Invalid(error)) if matches!(error.tag,Tag::Scalar(scalar::Error::Name{kind:found,..}) if found==kind))
        );
    }
    let document = parse("value={package='a::b',registry='a-b',base='a_b'}");
    let value = direct(&document).unwrap();
    let direct::Kind::Detailed(value) = value.kind() else {
        panic!("detailed")
    };
    for (field, kind) in [
        (Field::Registry, scalar::Kind::Registry),
        (Field::Base, scalar::Kind::PathBase),
        (Field::Package, scalar::Kind::Package),
    ] {
        let Some(primitive::Value::Name(name)) = value.get(field) else {
            panic!("name")
        };
        assert_eq!(name.kind(), kind);
    }
    let document = parse("value={'1bad'={extra=340282366920938463463374607431768211456}}");
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = inheritable::Map::interpret(input(&document), &mut session);
    assert!(
        matches!(session.finish(result),Err(Error::Invalid(error)) if matches!(error.tag,Tag::Scalar(_)))
    );
}

#[test]
fn flattened_structs_reject_sequences_without_visiting_them_but_capture_still_finishes() {
    let document = parse("value=[340282366920938463463374607431768211456]");
    assert!(matches!(direct(&document),Err(Error::Invalid(error)) if error.tag==Tag::Type));
    assert!(
        matches!(inherited(&document),Err(Error::Invalid(error)) if error.tag==Tag::IntegerOverflow)
    );
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Detailed::interpret(input(&document), &mut session);
    assert!(matches!(session.finish(result),Err(Error::Invalid(error)) if error.tag==Tag::Type));
}

#[test]
fn known_duplicate_replay_rejects_before_its_value() {
    let document = parse("value={version='1',other=1}");
    let mut captured = walk::project(
        input(&document),
        value::Mode::Direct,
        &mut reference::Account,
    )
    .unwrap();
    let edge = captured
        .edges
        .as_mut_slice()
        .iter_mut()
        .find(|edge| edge.key.is_some_and(|key| key.text.as_str() == "other"))
        .unwrap();
    edge.key.as_mut().unwrap().text = value::Text::Borrowed("version");
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Direct::interpret(value::Input::replay(&captured, 0), &mut session);
    assert!(
        matches!(session.finish(result),Err(Error::Invalid(error)) if error.tag==Tag::Duplicate)
    );
}

#[test]
fn dependency_maps_keep_first_key_and_last_converted_value() {
    let document = parse("value={b='old',a='a',c='new'}");
    let mut captured = walk::project(
        input(&document),
        value::Mode::Direct,
        &mut reference::Account,
    )
    .unwrap();
    let edge = captured
        .edges
        .as_mut_slice()
        .iter_mut()
        .find(|edge| edge.key.is_some_and(|key| key.text.as_str() == "c"))
        .unwrap();
    edge.key.as_mut().unwrap().text = value::Text::Borrowed("b");
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = direct::Map::interpret(value::Input::replay(&captured, 0), &mut session);
    let map = session.finish(result).unwrap();
    assert_eq!(
        map.entries()
            .map(|(name, _, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(
        matches!(map.entries().nth(1).unwrap().2.kind(),direct::Kind::Simple(text) if text.as_str()=="new")
    );
}
