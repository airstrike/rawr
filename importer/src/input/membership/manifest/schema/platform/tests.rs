use rawr_toml::document;

use super::super::super::{reference, session, value, walk, Error, Tag};
use super::super::{dependency, package};
use super::{Field, Platform};

mod pinned;
mod resources;

fn parse(source: &str) -> document::Document<'_> {
    document::parse(source, 80, &mut reference::Account).unwrap()
}

fn input<'d>(document: &'d document::Document<'d>) -> value::Input<'d, 'd> {
    value::Input::raw(document.table().get("value").unwrap())
}

fn interpret<'d>(
    input: value::Input<'_, 'd>,
) -> Result<Platform<'d>, Error<std::convert::Infallible>> {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Platform::interpret(input, &mut session);
    session.finish(result)
}

#[test]
fn all_five_alias_slots_keep_their_converted_values_and_source_order() {
    let document = parse("value={dev_dependencies={a='underscore-dev'},build_dependencies={a='underscore-build'},dependencies={z={workspace=true},a='normal'},dev-dependencies={a='kebab-dev'},build-dependencies={a='kebab-build'}}");
    let output = interpret(input(&document)).unwrap();
    assert_eq!(
        output
            .entries()
            .iter()
            .map(|entry| entry.field())
            .collect::<Vec<_>>(),
        [
            Field::DevDependencies2,
            Field::BuildDependencies2,
            Field::Dependencies,
            Field::DevDependencies,
            Field::BuildDependencies,
        ]
    );
    for (field, expected) in [
        (Field::BuildDependencies, "kebab-build"),
        (Field::BuildDependencies2, "underscore-build"),
        (Field::DevDependencies, "kebab-dev"),
        (Field::DevDependencies2, "underscore-dev"),
    ] {
        let (_, key, value) = output.get(field).unwrap().entries().next().unwrap();
        assert_eq!(key.text.as_str(), "a");
        let dependency::inheritable::Kind::Value(value) = value.kind() else {
            panic!("local dependency")
        };
        let dependency::direct::Kind::Simple(text) = value.kind() else {
            panic!("simple dependency")
        };
        assert_eq!(text.as_str(), expected);
    }
    let mut dependencies = output.get(Field::Dependencies).unwrap().entries();
    assert_eq!(dependencies.next().unwrap().0.as_str(), "a");
    let (name, _, value) = dependencies.next().unwrap();
    assert_eq!(name.as_str(), "z");
    assert!(matches!(
        value.kind(),
        dependency::inheritable::Kind::Inherit(_)
    ));
    assert_eq!(output.span(), input(&document).span);
    for entry in output.entries() {
        assert!(entry.span().is_some());
        assert!(matches!(entry.position(), package::Position::Map(_)));
        assert!(entry.dependencies().entries().len() >= 1);
    }
}

#[test]
fn absent_map_fields_default_but_all_five_sequence_slots_are_required() {
    let document = parse("value={}");
    let output = interpret(input(&document)).unwrap();
    assert!(output.entries().is_empty());
    assert!(output.get(Field::Dependencies).is_none());
    for (source, expected) in [
        ("value=[]", "dependencies"),
        ("value=[{},{},{},{}]", "dev_dependencies"),
    ] {
        let document = parse(source);
        let Err(Error::Invalid(error)) = interpret(input(&document)) else {
            panic!("{source}")
        };
        assert_eq!(error.tag, Tag::Missing);
        assert_eq!(error.expected, Some(expected));
    }
    let document = parse("value=[{},{},{},{},{},340282366920938463463374607431768211456]");
    let output = interpret(input(&document)).unwrap();
    assert_eq!(output.entries().len(), 5);
    for (index, entry) in output.entries().iter().enumerate() {
        assert!(matches!(entry.position(), package::Position::Sequence(found) if found == index));
        assert_eq!(entry.dependencies().entries().len(), 0);
    }
}

#[test]
fn replay_sequence_exhaustion_preserves_source_adapter_policy() {
    let document = parse("value=[{},{},{},{},{},false]");
    for mode in [
        value::Mode::Direct,
        value::Mode::SerdeValue,
        value::Mode::Content,
        value::Mode::Toml,
    ] {
        let projection = walk::project(input(&document), mode, &mut reference::Account).unwrap();
        let result = interpret(value::Input::replay(&projection, 0));
        if matches!(mode, value::Mode::Content | value::Mode::Toml) {
            assert!(
                matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::Length),
                "{mode:?}"
            );
        } else {
            assert_eq!(result.unwrap().entries().len(), 5, "{mode:?}");
        }
    }
}

#[test]
fn unknown_callbacks_and_conversion_failures_follow_source_order() {
    let document = parse("value={before={nested=[1]},build_dependencies={},'strange.key'=false,dependencies={},after=[]}");
    let output = interpret(input(&document)).unwrap();
    assert_eq!(
        output
            .ignored()
            .iter()
            .map(|key| key.text.as_str())
            .collect::<Vec<_>>(),
        ["before", "strange.key", "after"]
    );
    assert!(output.ignored().iter().all(|key| key.span.is_some()));
    assert_eq!(
        output
            .entries()
            .iter()
            .map(|entry| entry.field())
            .collect::<Vec<_>>(),
        [Field::BuildDependencies2, Field::Dependencies]
    );
    for (source, tag) in [
        (
            "value={unknown=1e9999,dependencies=false}",
            Tag::FloatOverflow,
        ),
        ("value={dependencies=false,unknown=1e9999}", Tag::Type),
    ] {
        let document = parse(source);
        let Err(Error::Invalid(error)) = interpret(input(&document)) else {
            panic!("{source}")
        };
        assert_eq!(error.tag, tag);
    }
}

#[test]
fn duplicate_replay_field_rejects_before_the_second_dependency_map() {
    let document = parse("value={dependencies={},dev-dependencies=false}");
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
        .find(|edge| {
            edge.key
                .is_some_and(|key| key.text.as_str() == "dev-dependencies")
        })
        .unwrap();
    edge.key.as_mut().unwrap().text = value::Text::Borrowed("dependencies");
    let Err(Error::Invalid(error)) = interpret(value::Input::replay(&captured, 0)) else {
        panic!("duplicate")
    };
    assert_eq!(error.tag, Tag::Duplicate);
    assert_eq!(error.expected, Some("dependencies"));
}

#[test]
fn synthetic_datetime_map_is_not_a_generic_toml_datetime_string() {
    let document = parse("value=2026-09-05");
    let output = interpret(input(&document)).unwrap();
    assert!(output.entries().is_empty());
    assert_eq!(
        output.ignored()[0].text.as_str(),
        "$__toml_private_datetime"
    );
    for mode in [
        value::Mode::Content,
        value::Mode::SerdeValue,
        value::Mode::Toml,
    ] {
        let captured = walk::project(input(&document), mode, &mut reference::Account).unwrap();
        let result = interpret(value::Input::replay(&captured, 0));
        if mode == value::Mode::Toml {
            assert!(matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::Type));
        } else {
            assert_eq!(result.unwrap().ignored().len(), 1);
        }
    }
}

#[test]
fn nested_inheritance_errors_keep_the_dependency_span() {
    let document = parse("value={dependencies={a={workspace=false}},unknown=1e9999}");
    let platform = reference::table(document.table().get("value").unwrap(), "platform");
    let dependencies = reference::table(platform.get("dependencies").unwrap(), "dependencies");
    let dependency = dependencies.get("a").unwrap();
    let Err(Error::Invalid(error)) = interpret(input(&document)) else {
        panic!("false inheritance")
    };
    assert_eq!(error.tag, Tag::WorkspaceFalse);
    assert_eq!(error.span, Some(dependency.span().into()));
}
