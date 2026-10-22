use rawr_toml::document;

use super::super::super::{reference, session, value, walk, Error, Tag};
use super::super::{dependency, lints, package, primitive};
use super::{Field, Ignored, Value, Workspace};

pub(in crate::input::membership::manifest) mod pinned;
mod resources;

fn parse(source: &str) -> document::Document<'_> {
    document::parse(source, 80, &mut reference::Account).unwrap()
}

fn input<'d>(document: &'d document::Document<'d>) -> value::Input<'d, 'd> {
    value::Input::raw(document.table().get("value").unwrap())
}

fn interpret<'d>(
    input: value::Input<'_, 'd>,
) -> Result<Workspace<'d>, Error<std::convert::Infallible>> {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Workspace::interpret(input, &mut session);
    session.finish(result)
}

#[test]
fn map_absence_and_all_workspace_payloads_remain_distinct() {
    let empty = parse("value={}");
    let empty = interpret(input(&empty)).unwrap();
    assert!(empty.entries().is_empty());
    assert!(empty.get(Field::Members).is_none());

    let source = "value={resolver='future-resolver',members=['./a/../b','café/*'],exclude=[],default-members=['.'],metadata={z=true,a=[-0.0]},package={version='1.2.3+build'},dependencies={a={workspace=true}},lints={workspace={rule='deny'}}}";
    let document = parse(source);
    let output = interpret(input(&document)).unwrap();
    assert_eq!(
        output
            .entries()
            .iter()
            .map(|entry| entry.field())
            .collect::<Vec<_>>(),
        [
            Field::Resolver,
            Field::Members,
            Field::Exclude,
            Field::DefaultMembers,
            Field::Metadata,
            Field::Package,
            Field::Dependencies,
            Field::Lints,
        ]
    );
    let Some(Value::Local(primitive::Value::Strings(members))) = output.get(Field::Members) else {
        panic!("members")
    };
    assert_eq!(
        members
            .as_slice()
            .iter()
            .map(|text| text.as_str())
            .collect::<Vec<_>>(),
        ["./a/../b", "café/*"]
    );
    let Some(Value::Dependencies(dependencies)) = output.get(Field::Dependencies) else {
        panic!("direct dependencies")
    };
    let (_, _, dependency) = dependencies.entries().next().unwrap();
    assert!(matches!(
        dependency.kind(),
        dependency::direct::Kind::Detailed(_)
    ));
    let Some(Value::Lints(table)) = output.get(Field::Lints) else {
        panic!("ordinary lints")
    };
    let (key, tool) = table.tools().next().unwrap();
    assert_eq!(key.text.as_str(), "workspace");
    assert!(matches!(
        tool.rules().next().unwrap().1,
        lints::Lint::Level(lints::Level::Deny)
    ));
    assert_eq!(output.span(), input(&document).span);
    assert!(output.entries().iter().all(|entry| entry.span().is_some()));
}

#[test]
fn known_and_ignored_failures_follow_source_encounter_order() {
    for (source, tag) in [
        ("value={unknown=1e9999,resolver=false}", Tag::FloatOverflow),
        ("value={resolver=false,unknown=1e9999}", Tag::Type),
        ("value={members=['a',false]}", Tag::Type),
    ] {
        let document = parse(source);
        let Err(Error::Invalid(error)) = interpret(input(&document)) else {
            panic!("{source}")
        };
        assert_eq!(error.tag, tag, "{source}");
        assert!(error.span.is_some());
    }
}

#[test]
fn ignored_records_preserve_nested_source_paths_positions_and_spans() {
    let document = parse("value={before=1,package={'strange.key'={nested=[true]},version='1.2.3',other=false},after=[]}");
    let output = interpret(input(&document)).unwrap();
    assert_eq!(output.ignored().len(), 4);
    let Ignored::Field(before) = output.ignored()[0] else {
        panic!("before")
    };
    assert_eq!(before.text.as_str(), "before");
    for (index, expected) in [(1, "strange.key"), (2, "other")] {
        let Ignored::Package {
            position: package::Position::Map(parent),
            path,
        } = output.ignored()[index]
        else {
            panic!("nested package")
        };
        assert_eq!(parent.text.as_str(), "package");
        assert!(path.parent().is_none());
        assert_eq!(path.key().text.as_str(), expected);
        assert!(parent.span.is_some());
        assert!(path.key().span.is_some());
    }
    let Ignored::Field(after) = output.ignored()[3] else {
        panic!("after")
    };
    assert_eq!(after.text.as_str(), "after");

    let document = parse("value=[[],[],[],'3',{}, {unknown=true}, {}, {}]");
    let output = interpret(input(&document)).unwrap();
    assert!(matches!(
        output.ignored()[0],
        Ignored::Package {
            position: package::Position::Sequence(5),
            ..
        }
    ));
}

#[test]
fn positional_fields_are_required_and_raw_extras_are_not_converted() {
    for (source, expected) in [
        ("value=[]", "members"),
        ("value=[[],[],[],'3',{}, {}, {}]", "lints"),
    ] {
        let document = parse(source);
        let Err(Error::Invalid(error)) = interpret(input(&document)) else {
            panic!("{source}")
        };
        assert_eq!(error.tag, Tag::Missing);
        assert_eq!(error.expected, Some(expected));
    }
    let document = parse(
        "value=[['member'],[],[],'3',{}, {}, {}, {},340282366920938463463374607431768211456]",
    );
    let output = interpret(input(&document)).unwrap();
    assert_eq!(output.entries().len(), 8);
    for (index, entry) in output.entries().iter().enumerate() {
        assert!(matches!(entry.position(), package::Position::Sequence(found) if found == index));
    }
}

#[test]
fn replay_exhaustion_follows_the_actual_adapter_origin() {
    let document = parse("value=[[],[],[],'3',{}, {}, {}, {},false]");
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
            assert_eq!(result.unwrap().entries().len(), 8, "{mode:?}");
        }
    }
}

#[test]
fn metadata_keeps_order_float_bits_datetime_and_original_document_borrow() {
    let document = parse("value={metadata={text=\"decoded\\ntext\",zero=-0.0,nan=-nan,date=2026-09-05T01:02:03.123456789+05:30}} ");
    let output = interpret(input(&document)).unwrap();
    let Some(Value::Metadata(projection)) = output.get(Field::Metadata) else {
        panic!("metadata")
    };
    let value::Kind::Map(children) = projection.nodes.as_slice()[0].kind else {
        panic!("table")
    };
    let edges = reference::children(projection, children, "workspace.metadata");
    assert_eq!(
        edges
            .iter()
            .map(|edge| edge.key.unwrap().text.as_str().to_owned())
            .collect::<Vec<_>>(),
        ["text", "zero", "nan", "date"]
    );
    let value::Kind::String(value::Text::Borrowed(text)) =
        projection.nodes.as_slice()[edges[0].child].kind
    else {
        panic!("borrowed text")
    };
    let raw = reference::table(document.table().get("value").unwrap(), "workspace");
    let raw = reference::table(raw.get("metadata").unwrap(), "metadata");
    let document::Value::String(original) = raw.get("text").unwrap().value() else {
        panic!("source text")
    };
    assert_eq!(text.as_ptr(), original.as_str().as_ptr());
    let value::Kind::Float(zero) = projection.nodes.as_slice()[edges[1].child].kind else {
        panic!("zero")
    };
    assert_eq!(zero.to_bits(), (-0.0_f64).to_bits());
    let value::Kind::Float(nan) = projection.nodes.as_slice()[edges[2].child].kind else {
        panic!("nan")
    };
    assert!(nan.is_nan() && nan.is_sign_negative());
    let value::Kind::DateTime(date) = projection.nodes.as_slice()[edges[3].child].kind else {
        panic!("datetime")
    };
    let document::Value::DateTime(original) = raw.get("date").unwrap().value() else {
        panic!("source datetime")
    };
    assert_eq!(date, *original);
}

#[test]
fn duplicate_replay_field_rejects_before_converting_the_second_payload() {
    let document = parse("value={members=[],exclude=false}");
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
        .find(|edge| edge.key.is_some_and(|key| key.text.as_str() == "exclude"))
        .unwrap();
    edge.key.as_mut().unwrap().text = value::Text::Borrowed("members");
    let Err(Error::Invalid(error)) = interpret(value::Input::replay(&captured, 0)) else {
        panic!("duplicate")
    };
    assert_eq!(error.tag, Tag::Duplicate);
    assert_eq!(error.expected, Some("members"));
}

#[test]
fn synthetic_datetime_map_is_distinct_from_generic_toml_datetime_replay() {
    let document = parse("value=2026-09-05");
    let output = interpret(input(&document)).unwrap();
    assert!(output.entries().is_empty());
    let Ignored::Field(key) = output.ignored()[0] else {
        panic!("synthetic field")
    };
    assert_eq!(key.text.as_str(), "$__toml_private_datetime");
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
