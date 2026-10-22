use rawr_toml::document;
use serde_json::json;

use super::{Field, PackageSpec, Profiles, Value};
use crate::input::membership::manifest::{
    reference,
    schema::{package, primitive},
    session, value, walk, Error, Source, Step, Stop, Tag,
};

mod resources;

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}

impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn document(source: &str) -> document::Document<'_> {
    document::parse(source, 4096, &mut reference::Account).unwrap()
}

fn input<'d>(document: &'d document::Document<'d>) -> value::Input<'d, 'd> {
    value::Input::raw(document.table().get("profile").unwrap())
}

fn interpret<'d>(
    input: value::Input<'_, 'd>,
) -> Result<Profiles<'d>, Error<std::convert::Infallible>> {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let result = Profiles::interpret(input, &mut session);
    session.finish(result)
}

#[test]
fn complete_profile_results_and_errors_match_all_committed_profile_observations() {
    let mut accepted = 0;
    let mut rejected = 0;
    for row in reference::observations() {
        if row["error"]["phase"] == "document" {
            continue;
        }
        let context = reference::id(&row);
        let parsed = reference::parse(&row, &mut reference::Account);
        let Some(source) = parsed.table().get("profile") else {
            continue;
        };
        let actual = interpret(value::Input::raw(source));
        if row["accepted"] == true {
            let actual = actual.unwrap_or_else(|error| panic!("{context}: {error:?}"));
            let expected = reference::some(&row["typed"]["fields"]["profile"], context).unwrap();
            assert_eq!(&super::reference::profiles(&actual), expected, "{context}");
            let callbacks: Vec<_> = row["unused_callbacks"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|value| {
                    value
                        .as_str()
                        .filter(|path| path.starts_with("profile."))
                        .map(str::to_owned)
                })
                .collect();
            assert_eq!(
                super::reference::ignored(&actual, "profile"),
                callbacks,
                "{context}"
            );
            accepted += 1;
        } else {
            // Every rejected selected row has only this field: the reference's
            // error cannot come from another top-level visitor.
            assert_eq!(parsed.table().len(), 1, "review rejected scope: {context}");
            let Err(Error::Invalid(error)) = actual else {
                panic!("{context}: expected semantic profile rejection");
            };
            assert_eq!(row["error"]["phase"], "typed", "{context}");
            let span = error.span.expect(context);
            assert_eq!(
                json!([span.start, span.end]),
                row["error"]["span"],
                "{context}"
            );
            rejected += 1;
        }
    }
    assert_eq!((accepted, rejected), (44, 23));
}

#[test]
fn container_defaults_apply_to_short_sequences_and_extra_values_follow_adapter_policy() {
    const FIELDS: [&str; 19] = [
        "0", "false", "'llvm'", "1", "false", "'off'", "true", "false", "'unwind'", "true",
        "false", "'dir'", "'dev'", "false", "[]", "{}", "[]", "false", "true",
    ];
    for count in 0..=19 {
        let source = format!("profile.dev=[{}]", FIELDS[..count].join(","));
        let document = document(&source);
        let output = interpret(input(&document)).unwrap();
        let (_, _, profile) = output.entries().next().unwrap();
        assert_eq!(profile.entries().len(), count);
        for (index, entry) in profile.entries().iter().enumerate() {
            assert!(
                matches!(entry.position(), package::Position::Sequence(found) if found == index)
            );
        }
    }
    let source = format!("profile.dev=[{},false]", FIELDS.join(","));
    let document = document(&source);
    for mode in [
        value::Mode::Direct,
        value::Mode::SerdeValue,
        value::Mode::Content,
        value::Mode::Toml,
    ] {
        let captured = walk::project(input(&document), mode, &mut reference::Account).unwrap();
        let result = interpret(value::Input::replay(&captured, 0));
        if matches!(mode, value::Mode::Content | value::Mode::Toml) {
            assert!(
                matches!(result, Err(Error::Invalid(error)) if error.tag == Tag::Length),
                "{mode:?}"
            );
        } else {
            assert_eq!(
                result.unwrap().entries().next().unwrap().2.entries().len(),
                19,
                "{mode:?}"
            );
        }
    }
}

#[test]
fn typed_package_replacement_keeps_first_key_last_value_and_every_callback_in_source_order() {
    let source = "profile.dev={package={'foo:1.2'={early=1,build-override={inner=1},late=1},'foo@1.2'={opt-level=2,last=1},'*'={opt-level=3}},last=1}";
    let document = document(source);
    let output = interpret(input(&document)).unwrap();
    let (_, _, profile) = output.entries().next().unwrap();
    let Some(Value::Package(packages)) = profile.get(Field::Package) else {
        panic!("package");
    };
    let entries: Vec<_> = packages.entries().collect();
    assert_eq!(entries.len(), 2);
    assert!(matches!(entries[0].0, PackageSpec::Spec(_)));
    assert_eq!(entries[0].1.text.as_str(), "foo:1.2");
    assert!(matches!(entries[1].0, PackageSpec::All));
    assert!(
        matches!(output.node(entries[0].2).get(Field::OptLevel), Some(Value::Opt(value)) if value.as_str() == "2")
    );
    assert_eq!(
        super::reference::ignored(&output, "profile"),
        [
            "profile.dev.package.foo:1.2.early",
            "profile.dev.package.foo:1.2.build-override.inner",
            "profile.dev.package.foo:1.2.late",
            "profile.dev.package.foo@1.2.last",
            "profile.dev.last",
        ]
    );
    assert_eq!(output.nodes().len(), 5);
}

#[test]
fn names_validate_before_children_and_preserve_exact_case_unicode_and_empty_spelling() {
    let parsed = document("profile={Z={},z={},'café'={},''={}} ");
    let output = interpret(input(&parsed)).unwrap();
    assert_eq!(
        output
            .entries()
            .map(|(name, _, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["", "Z", "café", "z"]
    );
    for name in ["DEBUG", "build-override", "cargo-dev", "check", "invalid!"] {
        let source = format!("profile={{'{name}'=340282366920938463463374607431768211456}}");
        let parsed = document(&source);
        assert!(
            matches!(interpret(input(&parsed)), Err(Error::Invalid(error)) if matches!(error.tag, Tag::Scalar(_))),
            "{name}"
        );
    }
    let parsed = document("profile.dev.package={'bad@^1'=340282366920938463463374607431768211456}");
    assert!(
        matches!(interpret(input(&parsed)), Err(Error::Invalid(error)) if matches!(error.tag, Tag::Scalar(_)))
    );
}

#[test]
fn known_duplicates_stop_before_the_second_payload_and_unknown_values_follow_replay_mode() {
    let parsed = document("profile.dev={debug=false,unknown=1}");
    let mut captured =
        walk::project(input(&parsed), value::Mode::Direct, &mut reference::Account).unwrap();
    let edge = captured
        .edges
        .as_mut_slice()
        .iter_mut()
        .find(|edge| edge.key.is_some_and(|key| key.text.as_str() == "unknown"))
        .unwrap();
    edge.key.as_mut().unwrap().text = value::Text::Borrowed("debug");
    let child = edge.child;
    captured.nodes.as_mut_slice()[child].kind = value::Kind::I128(i128::MAX);
    assert!(
        matches!(interpret(value::Input::replay(&captured, 0)), Err(Error::Invalid(error)) if error.tag == Tag::Duplicate && error.expected == Some("debug"))
    );

    let parsed = document("profile.dev={unknown={nested=1}} ");
    for mode in [
        value::Mode::Direct,
        value::Mode::SerdeValue,
        value::Mode::Content,
        value::Mode::Toml,
    ] {
        let captured = walk::project(input(&parsed), mode, &mut reference::Account).unwrap();
        let output = interpret(value::Input::replay(&captured, 0)).unwrap();
        assert_eq!(
            super::reference::ignored(&output, "profile"),
            ["profile.dev.unknown"],
            "{mode:?}"
        );
    }
}

#[test]
fn recursive_success_failure_and_teardown_use_flat_nodes_at_large_depth() {
    let depth = 1024;
    for invalid in [false, true] {
        // Exercise the actual replay visitor and its admitted staging data;
        // raw TOML's pinned parser depth remains unchanged.
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let mut captured = value::Projection::new(value::Mode::Content);
        let root = captured
            .append(
                value::Node {
                    kind: value::Kind::Map(value::Children::default()),
                    span: None,
                },
                None,
                &mut session,
            )
            .unwrap();
        let mut parent = root;
        for index in 0..=depth {
            let key = if index == 0 { "dev" } else { "build-override" };
            parent = captured
                .append(
                    value::Node {
                        kind: value::Kind::Map(value::Children::default()),
                        span: None,
                    },
                    Some((
                        parent,
                        Some(value::Key {
                            text: value::Text::Borrowed(key),
                            span: None,
                        }),
                    )),
                    &mut session,
                )
                .unwrap();
        }
        let key = if invalid { "debug" } else { "opt-level" };
        captured
            .append(
                value::Node {
                    kind: value::Kind::I64(if invalid { 7 } else { 1 }),
                    span: None,
                },
                Some((
                    parent,
                    Some(value::Key {
                        text: value::Text::Borrowed(key),
                        span: None,
                    }),
                )),
                &mut session,
            )
            .unwrap();
        session.finish(Ok(())).unwrap();
        let output = interpret(value::Input::replay(&captured, root));
        if invalid {
            assert!(matches!(output, Err(Error::Invalid(error)) if error.tag == Tag::Variant));
        } else {
            let output = output.unwrap();
            assert_eq!(output.nodes().len(), depth + 1);
            let mut node = output.entries().next().unwrap().2;
            for _ in 0..depth {
                let Some(Value::BuildOverride(index)) = node.get(Field::BuildOverride) else {
                    panic!("child");
                };
                node = output.node(*index);
            }
            assert!(
                matches!(node.get(Field::OptLevel), Some(Value::Opt(value)) if value.as_str() == "1")
            );
            drop(output);
        }
    }
}

#[test]
fn compound_spec_source_failure_is_terminal_before_profile_value_conversion() {
    let parsed = document(
        "profile.dev.package={'git+http://a%00/foo'=340282366920938463463374607431768211456}",
    );
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    assert!(matches!(
        Profiles::interpret(input(&parsed), &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        Profiles::interpret(input(&parsed), &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Source {
            cause: Source::SpecReparse,
            ..
        })
    ));
}

#[test]
fn converted_profile_strings_borrow_the_original_document_after_capture_teardown() {
    let parsed = document("profile.dev={codegen-backend='llvm',rustflags=['flag']} ");
    let output = {
        let captured = walk::project(
            input(&parsed),
            value::Mode::Content,
            &mut reference::Account,
        )
        .unwrap();
        interpret(value::Input::replay(&captured, 0)).unwrap()
    };
    let (_, _, node) = output.entries().next().unwrap();
    let Some(Value::Primitive(primitive::Value::String(value::Text::Borrowed(text)))) =
        node.get(Field::CodegenBackend)
    else {
        panic!("borrowed source");
    };
    let profiles = reference::table(parsed.table().get("profile").unwrap(), "profile");
    let dev = reference::table(profiles.get("dev").unwrap(), "dev");
    let document::Value::String(original) = dev.get("codegen-backend").unwrap().value() else {
        panic!("string");
    };
    assert_eq!(text.as_ptr(), original.as_str().as_ptr());
}

fn exercise<'d, A: document::Admission>(
    input: value::Input<'_, 'd>,
    session: &mut session::Session<'_, A>,
) -> Step<Profiles<'d>> {
    Profiles::interpret(input, session)
}
