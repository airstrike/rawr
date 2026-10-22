use rawr_toml::document;
use serde_json::{json, Value};

use super::super::{Error, Source, Span, Stop, Tag};
use super::{query, session, Invalid, Kind, Spec};

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refuse: Option<usize>,
}

impl document::Admission for Account {
    type Error = &'static str;
    fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refuse == Some(index) {
            Err("spec refusal")
        } else {
            Ok(())
        }
    }
}

fn parse(text: &str, account: &mut Account) -> Result<Spec, Error<&'static str>> {
    let mut session = session::Session::new(account);
    let result = Spec::parse(text, Some(Span { start: 7, end: 9 }), &mut session);
    session.finish(result)
}

#[test]
fn replacement_default_url_and_display_preserve_cargo_spelling() {
    for byte in 0..=255_u8 {
        let text = format!("git+https://example.com/repo?rev=%{byte:02X}#1.2.3");
        let mut account = Account::default();
        let spec = parse(&text, &mut account).unwrap();
        let Some(Kind::Git(reference)) = spec.kind() else {
            panic!("git")
        };
        let encoded: String =
            form_urlencoded::byte_serialize(reference.value().as_bytes()).collect();
        let expected = format!("git+https://example.com/repo?rev={encoded}#1.2.3");
        let mut session = session::Session::new(&mut account);
        assert_eq!(spec.display(&mut session).unwrap().to_string(), expected);
    }
    for (text, expected) in [
        (
            "pkg@1.2.3-alpha+build",
            "https://github.com/rust-lang/crates.io-index#pkg@1.2.3-alpha+build",
        ),
        (
            "https://example.com/pkg#1.2.3",
            "https://example.com/pkg#1.2.3",
        ),
        (
            "git+https://example.com/repo?branch=a%20b%2B%2F%7E#pkg@1.2",
            "git+https://example.com/repo?branch=a+b%2B%2F%7E#pkg@1.2",
        ),
        ("path+file:///p#p@1", "path+file:///p#1"),
    ] {
        let mut account = Account::default();
        let mut spec = parse(text, &mut account).unwrap();
        let mut session = session::Session::new(&mut account);
        spec.default_registry(&mut session).unwrap();
        let display = spec.display(&mut session).unwrap();
        assert_eq!(display.to_string(), expected);
        let result = super::super::load::formatted(format_args!("{display}"), &mut session);
        assert_eq!(session.finish(result).unwrap(), expected);
        let before = account.requests.len();
        let mut session = session::Session::new(&mut account);
        spec.default_registry(&mut session).unwrap();
        assert_eq!(account.requests.len(), before);
    }
    let mut baseline = Account::default();
    let mut spec = parse("pkg@1.2.3", &mut baseline).unwrap();
    let before = baseline.requests.len();
    let mut session = session::Session::new(&mut baseline);
    spec.default_registry(&mut session).unwrap();
    spec.display(&mut session).unwrap();
    let end = baseline.requests.len();
    for failure in before..end {
        let mut account = Account::default();
        let mut spec = parse("pkg@1.2.3", &mut account).unwrap();
        account.refuse = Some(failure);
        let mut session = session::Session::new(&mut account);
        let result = spec
            .default_registry(&mut session)
            .and_then(|()| spec.display(&mut session).map(|_| ()));
        assert!(matches!(result, Err(Stop::Refused)));
        assert!(matches!(
            spec.default_registry(&mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(spec.display(&mut session), Err(Stop::Refused)));
        assert_eq!(account.requests.len(), failure + 1);
    }
}

#[test]
fn full_version_requires_minor_and_patch_before_identifier_ownership() {
    for spelling in ["pkg", "pkg@1", "pkg@1.2"] {
        let mut account = Account::default();
        let spec = parse(spelling, &mut account).unwrap();
        let before = account.requests.len();
        let mut session = session::Session::new(&mut account);
        let result = spec.full_version(&mut session);
        assert!(session.finish(result).unwrap().is_none());
        assert_eq!(account.requests.len(), before);
    }
    let mut account = Account::default();
    let spec = parse("pkg@1.2.3-alpha.1+build.7", &mut account).unwrap();
    let before = account.requests.len();
    let mut session = session::Session::new(&mut account);
    let result = spec.full_version(&mut session);
    let full = session.finish(result).unwrap().unwrap();
    let after = account.requests.len();
    assert_eq!((full.major(), full.minor(), full.patch()), (1, 2, 3));
    assert_eq!((full.pre(), full.build()), ("alpha.1", "build.7"));
    assert_ne!(
        full.pre().as_ptr(),
        spec.version().unwrap().pre().unwrap().as_ptr()
    );
    drop(spec);
    assert_eq!(full.build(), "build.7");
    for failure in before..after {
        let mut account = Account::default();
        let spec = parse("pkg@1.2.3-alpha.1+build.7", &mut account).unwrap();
        account.refuse = Some(failure);
        let mut session = session::Session::new(&mut account);
        assert!(matches!(
            spec.full_version(&mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            spec.full_version(&mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(_))));
        assert_eq!(account.requests.len(), failure + 1);
    }
}

#[test]
fn complete_spec_preserves_owned_fragment_query_and_typed_order() {
    let value = parse(
        "git+https://EXAMPLE.com/pkg?branch=old&rev=a%20b#other@1.2.3-alpha.2+04",
        &mut Account::default(),
    )
    .unwrap();
    assert_eq!(value.name(), "other");
    let version = value.version().unwrap();
    assert_eq!(
        (version.major(), version.minor(), version.patch()),
        (1, Some(2), Some(3))
    );
    assert_eq!(
        (version.pre(), version.build()),
        (Some("alpha.2"), Some("04"))
    );
    let Some(Kind::Git(reference)) = value.kind() else {
        panic!("git reference")
    };
    assert_eq!(reference.kind(), query::Kind::Rev);
    assert_eq!(reference.value(), "a b");
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    assert_eq!(
        value.url.as_ref().unwrap().as_str(&mut session).unwrap(),
        "https://example.com/pkg"
    );
    let a = parse("foo@1.2.3+2", &mut Account::default()).unwrap();
    let b = parse("foo@1.2.3+10", &mut Account::default()).unwrap();
    assert!(a < b);
    let a = parse(
        "https://EXAMPLE.com:443/pkg#foo@1.2",
        &mut Account::default(),
    )
    .unwrap();
    let b = parse("https://example.com/pkg#foo:1.2", &mut Account::default()).unwrap();
    assert!(a == b);
}

#[test]
fn complete_spec_rejects_in_pinned_semantic_order() {
    for (input, expected) in [
        ("path+https://host/pkg?x", Invalid::Query),
        ("path+https://host/pkg", Invalid::PathScheme),
        ("other+https://host/pkg?x", Invalid::Protocol),
        ("https://host/pkg#", Invalid::EmptyFragment),
    ] {
        assert!(
            matches!(parse(input, &mut Account::default()), Err(Error::Invalid(error)) if error.tag == Tag::Spec(expected)),
            "{input}"
        );
    }
    assert!(
        matches!(parse("9bad@bad", &mut Account::default()), Err(Error::Invalid(error)) if matches!(error.tag, Tag::Scalar(super::super::scalar::Error::Partial(_))))
    );
}

#[test]
fn complete_spec_second_url_rejection_is_terminal_source_failure() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let span = Some(Span { start: 7, end: 9 });
    assert!(matches!(
        Spec::parse("git+http://a%00/foo", span, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        Spec::parse("valid", None, &mut session),
        Err(Stop::Refused)
    ));
    assert!(
        matches!(session.finish(Ok(())), Err(Error::Source { cause: Source::SpecReparse, span: actual }) if actual == span)
    );
}

#[test]
fn complete_spec_refusal_never_falls_back_or_restarts_after_prior_spend() {
    for input in [
        "foo@1.2.3-pre+build",
        "git+https://例え.jp/pkg?rev=a%20b#other@1.2",
        "registry+https://host/pkg#1.2",
    ] {
        let prior = vec![
            document::Request::Items(7),
            document::Request::Allocation {
                bytes: 31,
                alignment: 1,
            },
        ];
        let mut account = Account {
            requests: prior.clone(),
            refuse: None,
        };
        assert!(parse(input, &mut account).is_ok(), "{input}");
        let requests = account.requests;
        for index in prior.len()..requests.len() {
            let mut account = Account {
                requests: prior.clone(),
                refuse: Some(index),
            };
            let mut session = session::Session::new(&mut account);
            assert!(matches!(
                Spec::parse(input, None, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                Spec::parse("valid", None, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                session.finish(Ok(())),
                Err(Error::Admission("spec refusal"))
            ));
            assert_eq!(
                account.requests.len(),
                index + 1,
                "{input}, refusal {index}"
            );
        }
    }
}

#[test]
fn complete_spec_uses_original_account_at_exact_item_and_byte_floors() {
    use crate::input::membership::manifest::tests::one_short;
    use crate::{cargo, input, node};
    let text = "git+https://例え.jp/pkg?rev=a%20b#other@1.2.3-pre+build";
    let mut observed = Account::default();
    assert!(parse(text, &mut observed).is_ok());
    let mut items = 0;
    let mut bytes = 0;
    for request in observed.requests {
        match request {
            document::Request::Items(value) => items += value,
            document::Request::Allocation { bytes: value, .. } => bytes += value,
            _ => {}
        }
    }
    for (item_floor, byte_floor) in [(0, 0), (1, 0), (0, 1)] {
        let limits = cargo::Limits::standard();
        let mut allocation = input::Allocation::new(&limits);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        allocation.item_limit = 7 + items - item_floor;
        allocation.byte_limit = 31 + bytes - byte_floor;
        let graph = node::GraphBudget::new();
        graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
        graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .unwrap();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .unwrap();
        let mut budget = input::membership::Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = session::Session::new(&mut budget);
        let result = Spec::parse(text, Some(Span { start: 7, end: 9 }), &mut session);
        let result = session.finish(result);
        // A one-short item or byte target still admits the complete spec
        // and records only that axis at the exact total; nothing latches.
        assert_eq!(result.unwrap().name(), "other");
        let exact = input::Usage {
            items: 7 + items,
            bytes: 31 + bytes,
        };
        assert_eq!(allocation.usage(), exact);
        assert!(!allocation.refused);
        let exceeded = allocation.exceedances();
        match (item_floor, byte_floor) {
            (0, 0) => assert!(!exceeded.is_exceeded()),
            (1, 0) => assert!(one_short(
                &exceeded,
                input::INPUT_ITEMS_RESOURCE,
                exact.items
            )),
            _ => assert!(one_short(
                &exceeded,
                input::INPUT_BYTES_RESOURCE,
                exact.bytes
            )),
        }
    }
}

#[test]
fn complete_spec_admits_numeric_version_slot_before_interpretation() {
    for (text, expected) in [("foo", 2), ("foo@1.2.3", 3)] {
        let mut account = Account::default();
        assert!(parse(text, &mut account).is_ok());
        let items: usize = account
            .requests
            .iter()
            .map(|request| match request {
                document::Request::Items(items) => *items,
                _ => 0,
            })
            .sum();
        assert_eq!(items, expected, "{text}");
    }
    for text in ["foo@1.2.3", "https://host/foo#1.2.3"] {
        let mut observed = Account::default();
        assert!(parse(text, &mut observed).is_ok());
        let index = observed
            .requests
            .windows(2)
            .rposition(|pair| {
                matches!(pair[0], document::Request::Items(1))
                    && matches!(pair[1], document::Request::String(5))
            })
            .expect("version slot precedes its input interpretation");
        let mut refused = Account {
            requests: Vec::new(),
            refuse: Some(index),
        };
        assert!(matches!(
            parse(text, &mut refused),
            Err(Error::Admission(_))
        ));
        assert_eq!(refused.requests.len(), index + 1);
    }
}

pub(in crate::input::membership::manifest) fn project(value: &Spec) -> Value {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let version = value.version().map(|version| {
        json!({
            "major":version.major(), "minor":version.minor(), "patch":version.patch(),
            "pre":version.pre(), "build":version.build(),
        })
    });
    let kind = value.kind().map(|kind| match kind {
        Kind::Path => json!({"type":"path", "reference":null}),
        Kind::Registry => json!({"type":"registry", "reference":null}),
        Kind::SparseRegistry => json!({"type":"sparse", "reference":null}),
        Kind::Git(reference) => json!({"type":"git", "reference":{
            "kind":match reference.kind() {query::Kind::Tag=>"tag",query::Kind::Branch=>"branch",query::Kind::Rev=>"rev",query::Kind::DefaultBranch=>"default"},
            "value":reference.value(),
        }}),
    });
    let url = value
        .url
        .as_ref()
        .map(|url| url.as_str(&mut session).unwrap());
    json!({"name":value.name(),"version":version,"url":url,"kind":kind})
}

#[test]
fn complete_spec_matches_pinned_cargo_observations_and_key_order() {
    let source = include_str!("../../../../../reference/manifest/spec-observations.jsonl");
    let mut parsed = std::collections::BTreeMap::new();
    let mut specs = 0;
    let mut orders = 0;
    for line in source.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        match row["row"].as_str().unwrap() {
            "spec" => {
                specs += 1;
                let input = row["input"].as_str().unwrap();
                let result = parse(input, &mut Account::default());
                match row["status"].as_str().unwrap() {
                    "ok" => {
                        let value = result.unwrap_or_else(|error| panic!("{input:?}: {error}"));
                        let actual = project(&value);
                        for key in ["name", "version", "url", "kind"] {
                            assert_eq!(actual[key], row[key], "{input:?}: {key}");
                        }
                        parsed.insert(row["id"].as_str().unwrap().to_owned(), value);
                    }
                    "error" => assert!(matches!(result, Err(Error::Invalid(_))), "{input:?}"),
                    "source" => assert!(
                        matches!(
                            result,
                            Err(Error::Source {
                                cause: Source::SpecReparse,
                                ..
                            })
                        ),
                        "{input:?}"
                    ),
                    other => panic!("unknown source status {other}"),
                }
            }
            "orders" => {
                for pair in row["pairs"].as_array().unwrap() {
                    orders += 1;
                    let left = &parsed[pair["left"].as_str().unwrap()];
                    let right = &parsed[pair["right"].as_str().unwrap()];
                    let order = match left.cmp(right) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    };
                    assert_eq!(order, pair["ordering"].as_i64().unwrap(), "{pair}");
                    assert_eq!(left == right, pair["equal"].as_bool().unwrap(), "{pair}");
                }
            }
            other => panic!("unknown reference row {other}"),
        }
    }
    assert!(specs > 20 && orders > 20, "complete reference corpus");
}
