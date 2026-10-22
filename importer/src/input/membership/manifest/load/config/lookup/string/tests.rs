use super::*;
use crate::input::membership::manifest::load::{
    features::{self, Features},
    Warnings,
};
use crate::input::membership::manifest::{
    reference, scalar, schema::manifest::Original, value, Error, Stop,
};
use crate::input::membership::{observed, stored, Budget};
use crate::{cargo, input, node};
use std::path::Path;

fn key<'a, A: document::Admission>(session: &mut Session<'_, A>) -> Key<'a> {
    let document =
        document::parse("cargo-features=['path-bases']", 80, &mut reference::Account).unwrap();
    let original = Original::interpret(value::Input::root(&document), session).unwrap();
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
        session,
    )
    .unwrap();
    Key::path_base(
        scalar::Name::parse("workspace", scalar::Kind::PathBase).unwrap(),
        &features,
        session,
    )
    .unwrap()
}

#[test]
fn selected_text_and_decoded_definition_share_the_transparent_path_protocol() {
    for (environment, definition, expected, env_origin) in [
        (
            Some(b"env".as_slice()),
            Definition::Path(Path::new("/p/.cargo/config")),
            "env",
            true,
        ),
        (
            Some(b"\xff".as_slice()),
            Definition::Path(Path::new("/p/.cargo/config")),
            "configured",
            true,
        ),
        (
            None,
            Definition::Path(Path::new("/p/.cargo/config")),
            "configured",
            false,
        ),
        (
            Some(b"env".as_slice()),
            Definition::Cli(None),
            "configured",
            false,
        ),
    ] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let records = [
            observed::Record {
                query: observed::Query::Environment(b"CARGO_PATH_BASES_WORKSPACE".to_vec()),
                value: observed::Value::Environment(environment.map(<[u8]>::to_vec)),
            },
            observed::Record {
                query: observed::Query::EnvironmentPrefix(b"CARGO_PATH_BASES_WORKSPACE_".to_vec()),
                value: observed::Value::EnvironmentPrefix(false),
            },
        ];
        let image = stored::Stored::from_records(&records, &mut budget).unwrap();
        let mut host = host::Host::from_image(&image, budget).unwrap();
        let mut session = Session::new(&mut host);
        let key = key(&mut session);
        let mut string_calls = 0;
        let result = get(&key, &mut session, |_, _| {
            string_calls += 1;
            Ok(Some(Found {
                text: Some("configured"),
                definition,
                span: None,
            }))
        })
        .unwrap()
        .unwrap();
        assert_eq!(result.text(), expected);
        match result.definition() {
            Definition::Environment(name) => {
                assert!(env_origin);
                assert_eq!(name, "CARGO_PATH_BASES_WORKSPACE");
            }
            Definition::Path(path) => {
                assert!(!env_origin);
                assert_eq!(path, Path::new("/p/.cargo/config"));
            }
            Definition::Cli(None) => assert!(!env_origin),
            _ => panic!("unexpected definition"),
        }
        let mut path_calls = 0;
        let relative = super::super::optional_path(&key, &mut session, |_, _| {
            path_calls += 1;
            Ok(Some(Found {
                text: Some("configured"),
                definition,
                span: None,
            }))
        })
        .unwrap()
        .unwrap();
        assert_eq!(relative.text, result.text());
        assert_eq!(
            format!("{:?}", relative.origin.definition()),
            format!("{:?}", result.definition())
        );
        assert_eq!(string_calls, path_calls);
        assert_eq!(string_calls, if environment.is_some() { 2 } else { 3 });
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn wrong_final_type_wins_over_valid_environment_and_builtin_codec_runs_last() {
    for (text, definition, environment, expected) in [
        (
            None,
            Definition::Path(Path::new("/p/.cargo/config")),
            Some(b"env".as_slice()),
            Tag::ConfigLookup,
        ),
        (None, Definition::BuiltIn, None, Tag::ConfigLookup),
        (
            Some("configured"),
            Definition::BuiltIn,
            None,
            Tag::ConfigDefinition,
        ),
    ] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        // A valid environment cannot conceal wrong configuration type. Without
        // environment presence, BuiltIn is selected and its codec runs last.
        let records = [
            observed::Record {
                query: observed::Query::Environment(b"CARGO_PATH_BASES_WORKSPACE".to_vec()),
                value: observed::Value::Environment(environment.map(<[u8]>::to_vec)),
            },
            observed::Record {
                query: observed::Query::EnvironmentPrefix(b"CARGO_PATH_BASES_WORKSPACE_".to_vec()),
                value: observed::Value::EnvironmentPrefix(false),
            },
        ];
        let image = stored::Stored::from_records(&records, &mut budget).unwrap();
        let mut host = host::Host::from_image(&image, budget).unwrap();
        let mut session = Session::new(&mut host);
        let key = key(&mut session);
        let result = get(&key, &mut session, |_, _| {
            Ok(Some(Found {
                text,
                definition,
                span: None,
            }))
        });
        assert!(matches!(result,Err(Stop::Rejected(ref diagnostic)) if diagnostic.tag==expected));
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn missing_record_refuses_before_config_callback_and_stays_terminal() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = stored::Stored::from_records(&[], &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut session = Session::new(&mut host);
    let key = key(&mut session);
    let mut calls = 0;
    for _ in 0..2 {
        assert!(matches!(
            get(&key, &mut session, |_, _| {
                calls += 1;
                Ok(None)
            }),
            Err(Stop::Refused)
        ));
    }
    assert_eq!(calls, 0);
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Host(input::membership::Error::Unobserved))
    ));
}
