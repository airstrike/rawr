use rawr_toml::document;

use crate::input::membership::manifest::schema::{dependency, platform};
use crate::input::membership::manifest::{
    reference, scalar, session, value, walk, Error, Span, Tag,
};

use super::{input, parse, Family, Features, Patch, Platforms, Replace, Targets};

#[test]
fn complete_features_validate_keys_and_preserve_all_string_payloads_and_origins() {
    let document = parse("value={z=['dep:a','a/feature','unchecked syntax!'],\"caf\\u00e9\"=[], '9+x.y'=['same','same']}");
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let output = Features::interpret(input(&document), &mut session).unwrap();
    session.finish(Ok(())).unwrap();
    assert_eq!(output.span(), input(&document).span);
    assert_eq!(
        output
            .entries()
            .map(|entry| entry.key().as_str())
            .collect::<Vec<_>>(),
        ["9+x.y", "café", "z"]
    );
    for entry in output.entries() {
        assert_eq!(entry.key().kind(), scalar::Kind::Feature);
        assert_eq!(entry.key().as_str(), entry.origin().text.as_str());
        assert!(entry.origin().span.is_some());
        assert!(entry.span().is_some());
    }
    let z = output.entries().last().unwrap();
    assert_eq!(
        z.value()
            .as_slice()
            .iter()
            .map(value::Text::as_str)
            .collect::<Vec<_>>(),
        ["dep:a", "a/feature", "unchecked syntax!"]
    );
}

#[test]
fn feature_key_failure_precedes_its_value_but_never_an_earlier_value() {
    let document = parse("value={'dep:a'=1e9999}");
    let Err(Error::Invalid(error)) = Family::Features.interpret(input(&document)) else {
        panic!("invalid feature key");
    };
    assert!(matches!(
        error.tag,
        Tag::Scalar(scalar::Error::Name {
            kind: scalar::Kind::Feature,
            ..
        })
    ));
    let document::Value::Table(table) = document.table().get("value").unwrap().value() else {
        panic!("table");
    };
    assert_eq!(
        error.span,
        Some(table.iter().next().unwrap().0.span().into())
    );

    let document = parse("value={z=[1e9999], 'dep:a'=[]}");
    let Err(Error::Invalid(error)) = Family::Features.interpret(input(&document)) else {
        panic!("first value");
    };
    assert_eq!(error.tag, Tag::FloatOverflow);
    let document = parse("value={z=['first',false], 'a/b'=[]}");
    assert!(
        matches!(Family::Features.interpret(input(&document)), Err(Error::Invalid(error)) if error.tag == Tag::Type)
    );
}

#[test]
fn unvalidated_outer_strings_remain_distinct_from_nested_package_names() {
    let document =
        parse("value={'z invalid cfg!'={dependencies={ok='not a parsed requirement'}},''={}}");
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let output = Platforms::interpret(input(&document), &mut session).unwrap();
    assert_eq!(
        output
            .entries()
            .map(|entry| entry.origin().text.as_str())
            .collect::<Vec<_>>(),
        ["", "z invalid cfg!"]
    );
    assert_eq!(output.span(), input(&document).span);
    session.finish(Ok(())).unwrap();

    let document = parse("value={'not a URL %'={'café::outil'={version='not a parsed requirement',workspace=false}},''={}}");
    let mut session = session::Session::new(&mut account);
    let output = Patch::interpret(input(&document), &mut session).unwrap();
    assert_eq!(
        output
            .entries()
            .map(|entry| entry.origin().text.as_str())
            .collect::<Vec<_>>(),
        ["", "not a URL %"]
    );
    assert_eq!(output.span(), input(&document).span);
    let (_, _, dependency) = output
        .entries()
        .last()
        .unwrap()
        .value()
        .entries()
        .next()
        .unwrap();
    let dependency::direct::Kind::Detailed(dependency) = dependency.kind() else {
        panic!("direct detailed dependency");
    };
    assert_eq!(
        dependency.extras().next().unwrap().0.text.as_str(),
        "workspace"
    );
    session.finish(Ok(())).unwrap();

    let document = parse("value={'not a package spec /☃'='unparsed', ''={path='../x'}}");
    let mut session = session::Session::new(&mut account);
    let output = Replace::interpret(input(&document), &mut session).unwrap();
    assert_eq!(
        output
            .entries()
            .map(|entry| entry.origin().text.as_str())
            .collect::<Vec<_>>(),
        ["", "not a package spec /☃"]
    );
    assert_eq!(output.span(), input(&document).span);
    session.finish(Ok(())).unwrap();

    let document = parse("value={source={'9wrong'=1e9999}}");
    assert!(
        matches!(Family::Patch.interpret(input(&document)), Err(Error::Invalid(error)) if matches!(error.tag, Tag::Scalar(scalar::Error::Name { kind: scalar::Kind::Package, .. })))
    );
}

#[test]
fn map_families_require_maps_and_target_vectors_require_sequences() {
    for family in [
        Family::Features,
        Family::Platforms,
        Family::Patch,
        Family::Replace,
    ] {
        let document = parse("value={}");
        assert_eq!(family.interpret(input(&document)).unwrap(), 0, "{family:?}");
        for source in ["value=[]", "value=false", "value='text'"] {
            let document = parse(source);
            assert!(
                matches!(family.interpret(input(&document)), Err(Error::Invalid(error)) if error.tag == Tag::Type),
                "{family:?}: {source}"
            );
        }
    }
    let document = parse("value=[]");
    assert_eq!(Family::Targets.interpret(input(&document)).unwrap(), 0);
    for source in ["value={}", "value='text'", "value=false"] {
        let document = parse(source);
        assert!(
            matches!(Family::Targets.interpret(input(&document)), Err(Error::Invalid(error)) if error.tag == Tag::Type),
            "{source}"
        );
    }
}

#[test]
fn replacement_uses_the_source_datetime_map_and_replay_provenance() {
    let document = parse("value=2026-09-05T01:02:03.000000001+00:00");
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let output = Replace::interpret(input(&document), &mut session).unwrap();
    assert_eq!(output.span(), input(&document).span);
    let entry = output.entries().next().unwrap();
    assert_eq!(entry.origin().text.as_str(), "$__toml_private_datetime");
    let dependency::direct::Kind::Simple(text) = entry.value().kind() else {
        panic!("datetime string");
    };
    assert_eq!(text.as_str(), "2026-09-05T01:02:03.000000001+00:00");
    session.finish(Ok(())).unwrap();
    for mode in [
        value::Mode::Content,
        value::Mode::SerdeValue,
        value::Mode::Toml,
    ] {
        let projection = walk::project(input(&document), mode, &mut reference::Account).unwrap();
        let result = Family::Replace.interpret(value::Input::replay(&projection, 0));
        if mode == value::Mode::Toml {
            assert!(matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::Type));
        } else {
            assert_eq!(result.unwrap(), 1);
        }
    }
}

#[test]
fn duplicate_map_values_convert_in_order_and_callbacks_survive_replacement() {
    let document = parse("value={dup={old=[1]},other={new=[2]},z={dependencies={a='1'}}}");
    let mut projection = walk::project(
        input(&document),
        value::Mode::Direct,
        &mut reference::Account,
    )
    .unwrap();
    for edge in projection.edges.as_mut_slice() {
        if edge.key.is_some_and(|key| key.text.as_str() == "other") {
            edge.key.as_mut().unwrap().text = value::Text::Borrowed("dup");
        }
    }
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let output = Platforms::interpret(value::Input::replay(&projection, 0), &mut session).unwrap();
    session.finish(Ok(())).unwrap();
    assert_eq!(
        output
            .entries()
            .map(|entry| entry.origin().text.as_str())
            .collect::<Vec<_>>(),
        ["dup", "z"]
    );
    let first = output.entries().next().unwrap();
    assert_eq!(
        first
            .value()
            .ignored()
            .iter()
            .map(|key| key.text.as_str())
            .collect::<Vec<_>>(),
        ["new"]
    );
    assert_eq!(
        output
            .ignored()
            .iter()
            .map(|path| (path.parent().text.as_str(), path.field().text.as_str()))
            .collect::<Vec<_>>(),
        [("dup", "old"), ("dup", "new")]
    );
    assert_eq!(
        output
            .entries()
            .last()
            .unwrap()
            .value()
            .get(platform::Field::Dependencies)
            .unwrap()
            .entries()
            .len(),
        1
    );

    let document = parse("value={dup=false,other={}}");
    let mut projection = walk::project(
        input(&document),
        value::Mode::Direct,
        &mut reference::Account,
    )
    .unwrap();
    for edge in projection.edges.as_mut_slice() {
        if edge.key.is_some_and(|key| key.text.as_str() == "other") {
            edge.key.as_mut().unwrap().text = value::Text::Borrowed("dup");
        }
    }
    assert!(
        matches!(Family::Platforms.interpret(value::Input::replay(&projection, 0)), Err(Error::Invalid(error)) if error.tag == Tag::Type)
    );
}

#[test]
fn table_compaction_preserves_the_first_key_and_latest_value_span() {
    let first = String::from("duplicate");
    let last = String::from("duplicate");
    let mut table = super::super::table::Table::new();
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    for (key, text, number, offset) in [
        (1, "z", 1, 1),
        (2, first.as_str(), 2, 2),
        (3, "a", 3, 3),
        (4, last.as_str(), 4, 4),
    ] {
        table
            .push(
                key,
                value::Key {
                    text: value::Text::Borrowed(text),
                    span: Some(Span {
                        start: offset,
                        end: offset + 1,
                    }),
                },
                number,
                Some(Span {
                    start: offset + 10,
                    end: offset + 11,
                }),
                &mut session,
            )
            .unwrap();
    }
    table.finish(&mut session).unwrap();
    let entries = table.entries().collect::<Vec<_>>();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.origin().text.as_str())
            .collect::<Vec<_>>(),
        ["a", "duplicate", "z"]
    );
    assert_eq!(*entries[1].key(), 2);
    assert_eq!(*entries[1].value(), 4);
    assert_eq!(entries[1].origin().text.as_str().as_ptr(), first.as_ptr());
    assert_eq!(entries[1].origin().span, Some(Span { start: 2, end: 3 }));
    assert_eq!(entries[1].span(), Some(Span { start: 14, end: 15 }));
    session.finish(Ok(())).unwrap();
}

#[test]
fn every_map_family_replaces_only_after_complete_value_conversion() {
    for (family, source) in [
        (Family::Features, "value={dup=['first'],other=['last']}"),
        (Family::Patch, "value={dup={a='first'},other={b='last'}}"),
        (Family::Replace, "value={dup='first',other='last'}"),
    ] {
        let document = parse(source);
        let mut projection = walk::project(
            input(&document),
            value::Mode::Direct,
            &mut reference::Account,
        )
        .unwrap();
        for edge in projection.edges.as_mut_slice() {
            if edge.key.is_some_and(|key| key.text.as_str() == "other") {
                edge.key.as_mut().unwrap().text = value::Text::Borrowed("dup");
            }
        }
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let input = value::Input::replay(&projection, 0);
        match family {
            Family::Features => {
                let output = Features::interpret(input, &mut session).unwrap();
                assert_eq!(output.entries().len(), 1);
                let entry = output.entries().next().unwrap();
                assert_eq!(entry.key().as_str(), "dup");
                assert_eq!(entry.value().as_slice()[0].as_str(), "last");
            }
            Family::Patch => {
                let output = Patch::interpret(input, &mut session).unwrap();
                assert_eq!(output.entries().len(), 1);
                assert_eq!(
                    output
                        .entries()
                        .next()
                        .unwrap()
                        .value()
                        .entries()
                        .next()
                        .unwrap()
                        .0
                        .as_str(),
                    "b"
                );
            }
            Family::Replace => {
                let output = Replace::interpret(input, &mut session).unwrap();
                assert_eq!(output.entries().len(), 1);
                let dependency::direct::Kind::Simple(text) =
                    output.entries().next().unwrap().value().kind()
                else {
                    panic!("simple");
                };
                assert_eq!(text.as_str(), "last");
            }
            _ => unreachable!(),
        }
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn target_vectors_keep_every_target_and_callback_position() {
    let document = parse("value=[{name='first',unknown={deep=[1]}},{name='second','strange.key'=false},['positional',[],[],'src/p.rs','output',true,false,true,false,true,false,true,false,[],'2024',340282366920938463463374607431768211456]]");
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let output = Targets::interpret(input(&document), &mut session).unwrap();
    assert_eq!(output.span(), input(&document).span);
    assert_eq!(output.entries().len(), 3);
    assert!(output.entries().iter().all(|entry| entry.span().is_some()));
    assert_eq!(
        output
            .ignored()
            .iter()
            .map(|path| (path.index(), path.field().text.as_str()))
            .collect::<Vec<_>>(),
        [(0, "unknown"), (1, "strange.key")]
    );
    assert_eq!(
        output.entries()[2].target().values().nodes.as_slice().len(),
        16
    );
    session.finish(Ok(())).unwrap();
    let document = parse("value=[{name=7},false]");
    let document::Value::Array(array) = document.table().get("value").unwrap().value() else {
        panic!("array");
    };
    let document::Value::Table(first) = array.values()[0].value() else {
        panic!("table");
    };
    let Err(Error::Invalid(error)) = Family::Targets.interpret(input(&document)) else {
        panic!("invalid target");
    };
    assert_eq!(error.span, Some(first.get("name").unwrap().span().into()));
}

#[test]
fn converted_feature_payload_survives_temporary_capture_teardown() {
    let document = parse("value={\"caf\\u00e9\"=[\"a\\u00e9\"]}");
    let document::Value::Table(table) = document.table().get("value").unwrap().value() else {
        panic!("table");
    };
    let document::Value::Array(array) = table.get("café").unwrap().value() else {
        panic!("array");
    };
    let document::Value::String(text) = array.values()[0].value() else {
        panic!("string");
    };
    let pointer = text.as_str().as_ptr();
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let output = {
        let projection = walk::run(input(&document), value::Mode::Content, &mut session).unwrap();
        Features::interpret(value::Input::replay(&projection, 0), &mut session).unwrap()
    };
    let entry = output.entries().next().unwrap();
    assert_eq!(entry.key().as_str(), "café");
    assert_eq!(entry.value().as_slice()[0].as_str(), "aé");
    assert_eq!(entry.value().as_slice()[0].as_str().as_ptr(), pointer);
    session.finish(Ok(())).unwrap();
}
