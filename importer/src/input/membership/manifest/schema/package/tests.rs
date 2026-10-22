use rawr_toml::document;

use super::super::super::{reference, session, value, walk, Error, Tag};
use super::super::primitive;
use super::{Defaults, Field, Package, Position, Value, PACKAGE};

pub(in crate::input::membership::manifest) mod pinned;
mod resources;

fn parse(source: &str) -> document::Document<'_> {
    document::parse(source, 80, &mut reference::Account).unwrap()
}

fn input<'d>(document: &'d document::Document<'d>) -> value::Input<'d, 'd> {
    value::Input::raw(document.table().get("value").unwrap())
}

fn package<'d>(document: &'d document::Document<'d>) -> Package<'d> {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Package::interpret(input(document), &mut session);
    session.finish(result).unwrap()
}

#[test]
fn absence_and_every_inheritance_form_are_retained() {
    let document = parse("value={}");
    let package = package(&document);
    assert!(package.entries().is_empty());
    for definition in PACKAGE {
        assert!(package.get(definition.field).is_none());
    }
    for definition in PACKAGE {
        if !matches!(definition.shape, super::Shape::Inheritable(_)) {
            continue;
        }
        let source = format!("value={{{}={{workspace=true}}}}", definition.name);
        let document = parse(&source);
        let package = self::package(&document);
        assert!(
            matches!(package.get(definition.field), Some(Value::Inherited)),
            "{}",
            definition.name
        );
        assert_eq!(package.entries().len(), 1);
    }
}

#[test]
fn scalar_unions_keep_their_actual_variants() {
    for (source, field, expected) in [
        ("build=true", Field::Build, "auto-true"),
        ("build=false", Field::Build, "auto-false"),
        ("build='build.rs'", Field::Build, "single"),
        ("build=['a.rs','b.rs']", Field::Build, "multiple"),
        ("readme=true", Field::Readme, "readme-bool"),
        ("readme='README'", Field::Readme, "readme-string"),
        ("publish=false", Field::Publish, "publish-bool"),
        ("publish=['private']", Field::Publish, "publish-strings"),
        ("metabuild='builder'", Field::Metabuild, "strings"),
        ("metabuild=['builder']", Field::Metabuild, "strings"),
    ] {
        let source = format!("value={{{source}}}");
        let document = parse(&source);
        let package = package(&document);
        let Some(Value::Local(value)) = package.get(field) else {
            panic!("local union")
        };
        let actual = match value {
            primitive::Value::Build(primitive::Build::Auto(true)) => "auto-true",
            primitive::Value::Build(primitive::Build::Auto(false)) => "auto-false",
            primitive::Value::Build(primitive::Build::Single(text)) => {
                assert_eq!(text.as_str(), "build.rs");
                "single"
            }
            primitive::Value::Build(primitive::Build::Multiple(values)) => {
                assert_eq!(values.len(), 2);
                "multiple"
            }
            primitive::Value::StringOrBool(primitive::StringOrBool::Boolean(true)) => "readme-bool",
            primitive::Value::StringOrBool(primitive::StringOrBool::String(text)) => {
                assert_eq!(text.as_str(), "README");
                "readme-string"
            }
            primitive::Value::StringsOrBool(primitive::StringsOrBool::Boolean(false)) => {
                "publish-bool"
            }
            primitive::Value::StringsOrBool(primitive::StringsOrBool::Strings(values)) => {
                assert_eq!(values.len(), 1);
                "publish-strings"
            }
            primitive::Value::Strings(values) => {
                assert_eq!(values.as_slice()[0].as_str(), "builder");
                "strings"
            }
            _ => panic!("wrong union variant"),
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn package_and_default_versions_preserve_their_distinct_conversion_rules() {
    let document = parse("value={version=' \t1.2.3-alpha.1+build.7\t ',name='café::outil'}");
    let package = package(&document);
    let Some(Value::Local(primitive::Value::Version(version))) = package.get(Field::Version) else {
        panic!("version")
    };
    assert_eq!(
        (version.major(), version.minor(), version.patch()),
        (1, 2, 3)
    );
    assert_eq!((version.pre(), version.build()), ("alpha.1", "build.7"));
    assert_eq!(version.to_string(), "1.2.3-alpha.1+build.7");
    let Some(Value::Local(primitive::Value::Name(name))) = package.get(Field::Name) else {
        panic!("name")
    };
    assert_eq!(name.as_str(), "café::outil");
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Defaults::interpret(input(&document), &mut session);
    assert!(
        matches!(session.finish(result), Err(Error::Invalid(error)) if matches!(error.tag, Tag::Scalar(_)))
    );
    // PartialVersion's requirement fallback accepts ASCII spaces itself;
    // unlike package SemVer's explicit trim, it does not trim Unicode spaces.
    let spaced = parse("value={rust-version=' 1.96 '}");
    let spaced = self::package(&spaced);
    let Some(Value::Local(primitive::Value::Rust(version))) = spaced.get(Field::RustVersion) else {
        panic!("Rust version")
    };
    assert_eq!(
        (version.major(), version.minor(), version.patch()),
        (1, Some(96), None)
    );
    for source in [
        "value={rust-version='\u{a0}1.96\u{a0}'}",
        "value={rust-version='1.96-beta'}",
        "value={name='a/b'}",
    ] {
        let document = parse(source);
        let mut session = session::Session::new(&mut account);
        let result = Package::interpret(input(&document), &mut session);
        assert!(
            matches!(session.finish(result), Err(Error::Invalid(error)) if matches!(error.tag, Tag::Scalar(_)))
        );
    }
}

#[test]
fn inheritance_and_ignored_failures_follow_encounter_order() {
    for (source, expected) in [
        (
            "value={edition={workspace=false,later=1e9999}}",
            Tag::WorkspaceFalse,
        ),
        (
            "value={edition={first=1e9999,workspace=false}}",
            Tag::FloatOverflow,
        ),
        (
            "value={edition={workspace=true,later=1e9999}}",
            Tag::FloatOverflow,
        ),
        ("value={edition={other=true}}", Tag::Missing),
        ("value={edition={workspace='yes'}}", Tag::Type),
        ("value={edition=[true]}", Tag::Type),
        (
            "value={name='okay',ignored={nested=[340282366920938463463374607431768211456]}}",
            Tag::IntegerOverflow,
        ),
    ] {
        let document = parse(source);
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let result = Package::interpret(input(&document), &mut session);
        assert!(
            matches!(session.finish(result), Err(Error::Invalid(error)) if error.tag == expected),
            "{source}"
        );
    }
}

#[test]
fn ignored_callback_records_preserve_relative_paths_and_source_spans() {
    let document = parse(
        "value={unknown={nested=[1]},edition={workspace=true,'strange.key'=[false]},after=true}",
    );
    let package = package(&document);
    let ignored = package.ignored();
    assert_eq!(ignored.len(), 3);
    assert!(ignored[0].parent().is_none());
    assert_eq!(ignored[0].key().text.as_str(), "unknown");
    let Some(Position::Map(parent)) = ignored[1].parent() else {
        panic!("inherited parent")
    };
    assert_eq!(parent.text.as_str(), "edition");
    assert_eq!(ignored[1].key().text.as_str(), "strange.key");
    assert!(ignored[1].key().span.is_some());
    assert_eq!(ignored[2].key().text.as_str(), "after");
    assert_eq!(package.span(), input(&document).span);
}

fn package_sequence() -> String {
    let mut values = Vec::new();
    for definition in PACKAGE {
        let spelling = match definition.shape {
            super::Shape::Inheritable(_) => "{workspace=true}",
            super::Shape::Value(primitive::Shape::Name) => "'name'",
            super::Shape::Value(primitive::Shape::String) => "'text'",
            super::Shape::Value(primitive::Shape::Boolean | primitive::Shape::Build) => "false",
            super::Shape::Value(primitive::Shape::StringOrStrings) => "[]",
            super::Shape::Metadata => "{}",
            super::Shape::Forbidden => "340282366920938463463374607431768211456",
            _ => panic!("review package field fixture"),
        };
        values.push(spelling);
    }
    format!("value=[{}]", values.join(","))
}

#[test]
fn positional_package_reaches_the_forbidden_slot_without_converting_it() {
    for (source, expected) in [
        ("value=[]".to_owned(), Tag::Missing),
        (package_sequence(), Tag::CargoFeatures),
        (
            "value={cargo-features=340282366920938463463374607431768211456,later=1e9999}"
                .to_owned(),
            Tag::CargoFeatures,
        ),
    ] {
        let document = parse(&source);
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let result = Package::interpret(input(&document), &mut session);
        assert!(
            matches!(session.finish(result), Err(Error::Invalid(error)) if error.tag == expected)
        );
    }
}

#[test]
fn defaults_sequence_requires_seventeen_values_and_obeys_adapter_exhaustion() {
    let source = "value=['1.2.3',[],'d','h','doc',false,[],[],'MIT','LICENSE','repo',false,'2024',{},[],[],'1.96',true]";
    let document = parse(source);
    for mode in [
        value::Mode::Direct,
        value::Mode::SerdeValue,
        value::Mode::Content,
        value::Mode::Toml,
    ] {
        let captured = walk::project(input(&document), mode, &mut reference::Account).unwrap();
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let result = Defaults::interpret(value::Input::replay(&captured, 0), &mut session);
        let result = session.finish(result);
        if matches!(mode, value::Mode::Content | value::Mode::Toml) {
            assert!(matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::Length));
        } else {
            let defaults = result.unwrap();
            assert_eq!(defaults.entries().len(), 17);
            assert!(matches!(
                defaults.entries()[16].position(),
                Position::Sequence(16)
            ));
        }
    }
    let document = parse("value=[]");
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Defaults::interpret(input(&document), &mut session);
    assert!(
        matches!(session.finish(result), Err(Error::Invalid(error)) if error.tag == Tag::Missing && error.expected == Some("version"))
    );
}

#[test]
fn untagged_sequence_errors_restore_outer_span_but_direct_vectors_keep_child_span() {
    for (field, erased) in [
        ("build", true),
        ("metabuild", true),
        ("publish", true),
        ("authors", false),
    ] {
        let source = format!("value={{{field}=['okay',false]}}");
        let document = parse(&source);
        let raw = reference::table(document.table().get("value").unwrap(), "value")
            .get(field)
            .unwrap();
        let document::Value::Array(array) = raw.value() else {
            panic!("array")
        };
        let expected = if erased {
            raw.span()
        } else {
            array.values()[1].span()
        };
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let result = Package::interpret(input(&document), &mut session);
        assert!(
            matches!(session.finish(result), Err(Error::Invalid(error)) if error.tag == Tag::Type && error.span == Some(expected.into())),
            "{field}"
        );
    }
}

#[test]
fn typed_package_survives_temporary_projection_and_inline_string_storage() {
    let document =
        parse("value={name='owner',description=2024-01-02,version='1.2.3',build='build.rs'}");
    let package = {
        let captured =
            walk::project(input(&document), value::Mode::Toml, &mut reference::Account).unwrap();
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let result = Package::interpret(value::Input::replay(&captured, 0), &mut session);
        session.finish(result).unwrap()
    };
    let Some(Value::Local(primitive::Value::Name(name))) = package.get(Field::Name) else {
        panic!("name")
    };
    assert_eq!(name.as_str(), "owner");
    let Some(Value::Local(primitive::Value::String(description))) = package.get(Field::Description)
    else {
        panic!("description")
    };
    assert!(matches!(description, value::Text::Date(_)));
    assert_eq!(description.as_str(), "2024-01-02");
    let Some(Value::Local(primitive::Value::Version(version))) = package.get(Field::Version) else {
        panic!("version")
    };
    assert_eq!(version.to_string(), "1.2.3");
}

fn append<'d>(
    projection: &mut value::Projection<'d>,
    parent: Option<usize>,
    key: Option<&'d str>,
    kind: value::Kind<'d>,
    session: &mut session::Session<'_, reference::Account>,
) -> usize {
    projection
        .append(
            value::Node { kind, span: None },
            parent.map(|parent| {
                (
                    parent,
                    key.map(|text| value::Key {
                        text: value::Text::Borrowed(text),
                        span: None,
                    }),
                )
            }),
            session,
        )
        .unwrap()
}

#[test]
fn replay_duplicate_known_fields_reject_before_converting_the_second_value() {
    for inheritance in [false, true] {
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let mut captured = value::Projection::new(value::Mode::Content);
        let root = append(
            &mut captured,
            None,
            None,
            value::Kind::Map(value::Children::default()),
            &mut session,
        );
        let (parent, key, first) = if inheritance {
            let inherited = append(
                &mut captured,
                Some(root),
                Some("edition"),
                value::Kind::Map(value::Children::default()),
                &mut session,
            );
            (inherited, "workspace", value::Kind::Boolean(true))
        } else {
            (
                root,
                "name",
                value::Kind::String(value::Text::Borrowed("first")),
            )
        };
        append(&mut captured, Some(parent), Some(key), first, &mut session);
        append(
            &mut captured,
            Some(parent),
            Some(key),
            value::Kind::I128(i128::MAX),
            &mut session,
        );
        session.finish(Ok(())).unwrap();
        let mut session = session::Session::new(&mut account);
        let result = Package::interpret(value::Input::replay(&captured, root), &mut session);
        assert!(
            matches!(session.finish(result), Err(Error::Invalid(error)) if error.tag == Tag::Duplicate)
        );
    }
}

#[test]
fn badge_maps_sort_and_replace_values_after_source_order_conversion() {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let mut captured = value::Projection::new(value::Mode::Content);
    let root = append(
        &mut captured,
        None,
        None,
        value::Kind::Map(value::Children::default()),
        &mut session,
    );
    let badges = append(
        &mut captured,
        Some(root),
        Some("badges"),
        value::Kind::Map(value::Children::default()),
        &mut session,
    );
    for (name, entries) in [
        ("z", &[("status", "old")][..]),
        ("a", &[("status", "a")][..]),
        (
            "z",
            &[("z", "last"), ("status", "new"), ("status", "newest")][..],
        ),
    ] {
        let badge = append(
            &mut captured,
            Some(badges),
            Some(name),
            value::Kind::Map(value::Children::default()),
            &mut session,
        );
        for (key, text) in entries {
            append(
                &mut captured,
                Some(badge),
                Some(key),
                value::Kind::String(value::Text::Borrowed(text)),
                &mut session,
            );
        }
    }
    session.finish(Ok(())).unwrap();
    let mut session = session::Session::new(&mut account);
    let result = Defaults::interpret(value::Input::replay(&captured, 0), &mut session);
    let defaults = session.finish(result).unwrap();
    let Some(Value::Local(primitive::Value::Badges(badges))) = defaults.get(Field::Badges) else {
        panic!("badges")
    };
    assert_eq!(
        badges
            .entries()
            .iter()
            .map(|badge| badge.key().text.as_str())
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    assert_eq!(
        badges.entries()[1]
            .values()
            .iter()
            .map(|pair| (pair.key().text.as_str(), pair.text().as_str()))
            .collect::<Vec<_>>(),
        [("status", "newest"), ("z", "last")]
    );
}
