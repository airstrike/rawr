use super::*;
use crate::input::membership::manifest::load::{features, Warnings};
use crate::input::membership::manifest::{reference, schema::manifest::Original, value, Stop, Tag};

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}

#[test]
fn registry_keys_preserve_components_and_environment_spelling() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let name = scalar::Name::parse("my-registry", scalar::Kind::Registry).unwrap();
    let key = Key::registry(name, &mut session).unwrap();
    assert_eq!(key.parts(), ["registries", "my-registry", "index"]);
    assert_eq!(key.environment(), "CARGO_REGISTRIES_MY_REGISTRY_INDEX");
    assert_eq!(key.prefix(), "CARGO_REGISTRIES_MY_REGISTRY_INDEX_");
    let key = Key::registry_index(&mut session).unwrap();
    assert_eq!(key.parts(), ["registry", "index"]);
    assert_eq!(key.environment(), "CARGO_REGISTRY_INDEX");
    assert_eq!(key.prefix(), "CARGO_REGISTRY_INDEX_");
    session.finish(Ok(())).unwrap();
}

#[test]
fn environment_arm_short_circuits_exact_presence_and_requires_negative_evidence() {
    use crate::input::membership::{observed, stored, Budget};
    use crate::{cargo, input, node};
    // Empty strings are present. Prefix evidence is reached only after None.
    for (exact, prefix, expected) in [
        (Some(""), None, Some(true)),
        (Some("root"), None, Some(true)),
        (None, Some(true), Some(true)),
        (None, Some(false), Some(false)),
        (None, None, None),
    ] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut records = vec![observed::Record {
            query: observed::Query::Environment(b"CARGO_PATH_BASES_MY_BASE".to_vec()),
            value: observed::Value::Environment(exact.map(|text| text.as_bytes().to_vec())),
        }];
        if let Some(present) = prefix {
            records.push(observed::Record {
                query: observed::Query::EnvironmentPrefix(b"CARGO_PATH_BASES_MY_BASE_".to_vec()),
                value: observed::Value::EnvironmentPrefix(present),
            });
        }
        let image = stored::Stored::from_records(&records, &mut budget).unwrap();
        let mut host = host::Host::from_image(&image, budget).unwrap();
        let mut session = Session::new(&mut host);
        let key = Key::encode("my-base", &mut session).unwrap();
        let result = key.has_environment(&mut session);
        match expected {
            Some(expected) => {
                assert_eq!(session.finish(result).unwrap(), expected);
                host.finish().unwrap();
            }
            None => {
                assert!(matches!(result, Err(Stop::Refused)));
                assert!(matches!(
                    session.finish(result),
                    Err(Error::Host(input::membership::Error::Unobserved))
                ));
            }
        }
    }
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

#[test]
fn names_keep_original_toml_spelling_and_exact_cargo_environment_mapping() {
    for base in [
        "workspace",
        "WorkSpace",
        "my-base",
        "my_base",
        "_",
        "straße",
        "ﬃ",
        "é漢字",
        "a\u{301}",
        "ıσς",
    ] {
        scalar::Name::parse(base, scalar::Kind::PathBase).unwrap();
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let result = Key::encode(base, &mut session);
        let key = session.finish(result).unwrap();
        // Exact pinned ConfigKey::push expression, including Unicode expansion.
        let expected = format!("CARGO_PATH_BASES_{}", base.replace('-', "_").to_uppercase());
        assert_eq!(key.parts(), ["path-bases", base]);
        assert_eq!(key.environment(), expected);
        assert_eq!(key.prefix(), format!("{expected}_"));
        assert_eq!(key.parts()[1].as_ptr(), base.as_ptr());
    }
}

#[test]
fn every_original_admission_refusal_stops_before_key_allocation() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let result = Key::encode("straße", &mut session);
    session.finish(result).unwrap();
    for refusal in 0..account.requests.len() {
        let mut limited = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        storage::ALLOCATIONS.with(|count| count.set(0));
        let mut session = Session::new(&mut limited);
        assert!(matches!(
            Key::encode("straße", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            Key::encode("workspace", &mut session),
            Err(Stop::Refused)
        ));
        assert!(session.finish(Ok(())).is_err());
        assert_eq!(limited.requests.len(), refusal + 1);
        storage::ALLOCATIONS.with(|count| assert_eq!(count.get(), 0));
    }
}

#[test]
fn path_base_feature_gate_precedes_key_work() {
    for enabled in [false, true] {
        let source = if enabled {
            "cargo-features=['path-bases']"
        } else {
            ""
        };
        let document = document::parse(source, 80, &mut reference::Account).unwrap();
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
        let mut warnings = Warnings::new();
        let features = Features::interpret(
            &original,
            features::Context {
                allow_features: None,
                nightly_features_allowed: true,
                channel: "nightly",
                is_local: true,
            },
            &mut warnings,
            &mut session,
        )
        .unwrap();
        storage::ALLOCATIONS.with(|count| count.set(0));
        let base = scalar::Name::parse("workspace", scalar::Kind::PathBase).unwrap();
        let result = Key::path_base(base, &features, &mut session);
        if enabled {
            assert_eq!(result.unwrap().environment(), "CARGO_PATH_BASES_WORKSPACE");
        } else {
            assert!(
                matches!(result, Err(Stop::Rejected(error)) if matches!(error.tag, Tag::Feature(features::Invalid::Required { feature: Feature::PathBases, .. })))
            );
            storage::ALLOCATIONS.with(|count| assert_eq!(count.get(), 0));
        }
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn a_different_name_grammar_cannot_construct_a_path_base_key() {
    let document =
        document::parse("cargo-features=['path-bases']", 80, &mut reference::Account).unwrap();
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
    let mut warnings = Warnings::new();
    let features = Features::interpret(
        &original,
        features::Context {
            allow_features: None,
            nightly_features_allowed: true,
            channel: "nightly",
            is_local: true,
        },
        &mut warnings,
        &mut session,
    )
    .unwrap();
    let base = scalar::Name::parse("a.b", scalar::Kind::Feature).unwrap();
    assert!(matches!(
        Key::path_base(base, &features, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(session.finish(Ok(())), Err(Error::State)));
}
