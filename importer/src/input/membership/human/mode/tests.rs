use super::{Inputs, Mode, Term};
use crate::input::membership::{host, observed, stored, Budget, Error};
use crate::{cargo, input, node};

fn mode(input: Inputs<'_>, environment: &[(&[u8], Option<&[u8]>)]) -> Mode {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let records: Vec<_> = environment
        .iter()
        .map(|(key, value)| observed::Record {
            query: observed::Query::Environment(key.to_vec()),
            value: observed::Value::Environment(value.map(<[u8]>::to_vec)),
        })
        .collect();
    let image =
        stored::Stored::from_records(&records, &mut budget).expect("canonical command environment");
    let mut host = host::Host::from_image(&image, budget).expect("command environment host");
    let mode = Mode::from_inputs(input, &mut host).expect("pinned color choice");
    host.finish()
        .expect("exact conditional observation closure");
    mode
}

#[test]
fn explicit_cli_color_precedes_loaded_term_and_skips_auto_environment() {
    for (cli, term, strip) in [
        (Some(b"always".as_slice()), Some(b"never".as_slice()), false),
        (Some(b"never".as_slice()), Some(b"always".as_slice()), true),
        (None, Some(b"always".as_slice()), false),
        (None, Some(b"never".as_slice()), true),
        (
            Some(b"always".as_slice()),
            Some(b"invalid".as_slice()),
            false,
        ),
    ] {
        assert_eq!(
            mode(
                Inputs {
                    cli,
                    term: Term::Loaded { color: term }
                },
                &[]
            )
            .strips(),
            strip
        );
    }
}

#[test]
fn auto_uses_recorded_command_environment_and_native_nonempty_rules() {
    for no_color in [
        None,
        Some(b"".as_slice()),
        Some(b"0".as_slice()),
        Some(b"\xff".as_slice()),
    ] {
        for force in [
            None,
            Some(b"".as_slice()),
            Some(b"0".as_slice()),
            Some(b"\xff".as_slice()),
        ] {
            for clicolor in [None, Some(b"0".as_slice()), Some(b"1".as_slice())] {
                let mut environment = vec![(b"CLICOLOR".as_slice(), clicolor)];
                let no = no_color.is_some_and(|value| !value.is_empty());
                if !no {
                    environment.push((b"CLICOLOR_FORCE".as_slice(), force));
                }
                environment.push((b"NO_COLOR".as_slice(), no_color));
                let selected = mode(
                    Inputs {
                        cli: Some(b"auto"),
                        term: Term::Loaded {
                            color: Some(b"always"),
                        },
                    },
                    &environment,
                );
                assert_eq!(
                    selected.strips(),
                    no || !force.is_some_and(|value| !value.is_empty())
                );
            }
        }
    }
}

#[test]
fn absent_color_and_failed_whole_term_load_use_auto() {
    let environment = [
        (b"CLICOLOR".as_slice(), None),
        (b"CLICOLOR_FORCE".as_slice(), Some(b"1".as_slice())),
        (b"NO_COLOR".as_slice(), None),
    ];
    for term in [Term::Loaded { color: None }, Term::Defaulted] {
        assert!(!mode(Inputs { cli: None, term }, &environment).strips());
    }
}

#[test]
fn invalid_effective_color_and_missing_environment_are_distinct_rejections() {
    for color in [b"".as_slice(), b"ALWAYS", b"always ", b"\xff"] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let image = stored::Stored::from_records(&[], &mut budget).expect("empty image");
        let mut host = host::Host::from_image(&image, budget).expect("host");
        assert!(matches!(
            Mode::from_inputs(
                Inputs {
                    cli: None,
                    term: Term::Loaded { color: Some(color) }
                },
                &mut host
            ),
            Err(Error::Encoding("invalid Cargo color choice"))
        ));
        assert!(matches!(
            Mode::from_inputs(
                Inputs {
                    cli: Some(b"auto"),
                    term: Term::Loaded { color: None }
                },
                &mut host
            ),
            Err(Error::Unobserved)
        ));
    }
}
