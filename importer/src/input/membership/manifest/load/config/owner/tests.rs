use super::*;
use crate::input::membership::manifest::load::{
    features::{self, Features},
    Warnings,
};
use crate::input::membership::manifest::{
    reference, scalar, schema::manifest::Original, storage::slots, value,
};
use crate::input::membership::observed::discovery::{
    After, Attempt, Exists, Identity, NativeError, Transcript,
};
use crate::input::membership::{observed, stored, Budget};
use crate::{input, node};
use rawr_toml::document;

fn budget<T>(run: impl FnOnce(&mut Budget<'_>) -> T) -> T {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    run(&mut Budget {
        graph: &graph,
        allocation: &mut allocation,
    })
}
fn path(value: &str) -> observed::Path {
    budget(|budget| observed::Path::from_bytes(value.as_bytes(), budget).unwrap())
}
fn file(value: &str, text: &str) -> observed::Record {
    observed::Record {
        query: observed::Query::File(path(value)),
        value: observed::Value::File {
            bytes: text.as_bytes().to_vec(),
            end: observed::End::Eof,
        },
    }
}
fn discovery(filename: Filename, present: bool) -> observed::Record {
    let (first, after) = if present {
        (
            Attempt::Acquired(Identity {
                device: 1,
                inode: 2,
            }),
            After::Second(Attempt::OpenFailed(NativeError::Os(2))),
        )
    } else {
        (
            Attempt::OpenFailed(NativeError::Os(2)),
            After::Fallback(Exists::Failed(NativeError::Os(2))),
        )
    };
    observed::Record {
        query: observed::Query::Discovery {
            directory: path("/p/.cargo"),
            filename,
            warn: true,
        },
        value: observed::Value::Discovery(
            Transcript::checked(filename, true, first, after).unwrap(),
        ),
    }
}
fn normal(text: &str) -> Vec<observed::Record> {
    vec![
        observed::Record {
            query: observed::Query::Environment(b"__CARGO_TEST_ROOT".to_vec()),
            value: observed::Value::Environment(None),
        },
        discovery(Filename::Config, true),
        file("/p/.cargo/config", text),
        observed::Record {
            query: observed::Query::Canonical(path("/p/.cargo")),
            value: observed::Value::Canonical(Ok(path("/p/.cargo"))),
        },
    ]
}
fn present(value: &str) -> observed::Record {
    observed::Record {
        query: observed::Query::Metadata {
            path: path(value),
            follow: true,
        },
        value: observed::Value::Metadata(observed::Metadata {
            kind: observed::Kind::File,
            device: 1,
            inode: 2,
            owner: 0,
            mode: 0,
            length: 0,
            modified_seconds: 0,
            modified_nanos: 0,
            changed_seconds: 0,
            changed_nanos: 0,
        }),
    }
}
fn image(mut records: Vec<observed::Record>) -> stored::Stored {
    records.sort_by(|a, b| a.query.cmp(&b.query));
    budget(|budget| {
        let image = stored::Stored::from_records(&records, budget).unwrap();
        let bytes = image.encode(budget).unwrap();
        stored::Stored::decode(&bytes, budget).unwrap()
    })
}
fn context() -> Context<'static> {
    Context {
        pwd: Path::new("/p"),
        home: Path::new("/p/.cargo"),
        search_stop: Some(Path::new("/p")),
        cwd: Path::new("/p"),
    }
}
fn key<'a>(name: &'a str, session: &mut Session<'_, impl document::Admission>) -> Key<'a> {
    let doc =
        document::parse("cargo-features=['path-bases']", 80, &mut reference::Account).unwrap();
    let original = Original::interpret(value::Input::root(&doc), session).unwrap();
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
        scalar::Name::parse(name, scalar::Kind::PathBase).unwrap(),
        &features,
        session,
    )
    .unwrap()
}

macro_rules! fixture {
    ($records:expr, |$owner:ident, $session:ident, $limits:ident| $body:block) => {{
        let image = image($records);
        let $limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&$limits);
        let mut host = host::Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .unwrap();
        let mut paths = slots::Slots::new();
        let mut documents = slots::Slots::new();
        let files = Files::from_owners(paths.writer().unwrap(), documents.writer().unwrap());
        let mut $owner = Owner::from_files(files, context());
        let mut $session = Session::new(&mut host);
        $body
    }};
}

#[test]
fn empty_configuration_is_lazy_and_values_are_retained_once() {
    fixture!(Vec::new(), |owner, session, limits| {
        owner.configure_files(&[], &limits, &mut session).unwrap();
        assert!(owner.ordinary.is_none());
        assert!(owner.credentials.is_none());
    });
    fixture!(normal("answer=1"), |owner, session, limits| {
        let first = owner.values(&limits, &mut session).unwrap() as *const _;
        let second = owner.values(&limits, &mut session).unwrap() as *const _;
        assert_eq!(first, second);
        owner.configure_files(&[], &limits, &mut session).unwrap();
        assert_eq!(
            first,
            owner.values(&limits, &mut session).unwrap() as *const _
        );
    });
}

#[test]
fn cli_failure_precedes_normal_and_failed_owner_cannot_continue() {
    fixture!(
        vec![present("/cli.toml"), file("/cli.toml", "a=1.0")],
        |owner, session, limits| {
            let first = owner
                .configure_files(&["/cli.toml"], &limits, &mut session)
                .unwrap_err();
            assert!(matches!(first, Stop::Rejected(_)));
            let second = owner
                .configure_files(&["/missing.toml"], &limits, &mut session)
                .unwrap_err();
            assert_eq!(format!("{first:?}"), format!("{second:?}"));
            assert!(owner.values(&limits, &mut session).is_err());
            assert!(owner.load_credentials(&limits, &mut session).is_err());
            session.check().unwrap(); // No missing observation was reached.
        }
    );
}

#[test]
fn selected_credentials_include_failure_precedes_normal_initialization() {
    fixture!(
        vec![
            discovery(Filename::Credentials, true),
            file("/p/.cargo/credentials", "include=['child.toml']"),
            file("/p/.cargo/child.toml", "a=1.0")
        ],
        |owner, session, limits| {
            assert!(matches!(
                owner.load_credentials(&limits, &mut session),
                Err(Stop::Rejected(_))
            ));
            assert!(owner.ordinary.is_none());
            assert!(owner.credentials.is_none());
            session.check().unwrap();
        }
    );
}

#[test]
fn absent_credentials_are_not_memoized_but_successful_empty_file_is() {
    fixture!(
        vec![discovery(Filename::Credentials, false)],
        |owner, session, limits| {
            owner.load_credentials(&limits, &mut session).unwrap();
            assert!(owner.credentials.is_none());
            assert!(owner.ordinary.is_none());
            owner.load_credentials(&limits, &mut session).unwrap();
            assert!(owner.credentials.is_none());
        }
    );
    let mut records = normal("normal=true");
    records.extend([
        discovery(Filename::Credentials, true),
        file("/p/.cargo/credentials", ""),
    ]);
    fixture!(records, |owner, session, limits| {
        owner.load_credentials(&limits, &mut session).unwrap();
        assert!(owner.ordinary.is_some());
        let first = owner.credentials.as_ref().unwrap() as *const _;
        owner.load_credentials(&limits, &mut session).unwrap();
        assert_eq!(first, owner.credentials.as_ref().unwrap() as *const _);
    });
}

#[test]
fn credential_success_then_normal_failure_does_not_publish_overlay() {
    fixture!(
        vec![
            discovery(Filename::Credentials, true),
            file("/p/.cargo/credentials", "token='fixture'")
        ],
        |owner, session, limits| {
            assert!(matches!(
                owner.load_credentials(&limits, &mut session),
                Err(Stop::Refused)
            ));
            assert!(owner.ordinary.is_none());
            assert!(owner.credentials.is_none());
            assert!(matches!(
                owner.configure_files(&[], &limits, &mut session),
                Err(Stop::Refused)
            ));
        }
    );
}

#[test]
fn joined_cli_credentials_and_optional_path_keep_complete_origins() {
    let mut records = normal("[path-bases]\nbase='ordinary'\nother='unchanged'");
    records.extend([
        present("/cli.toml"),
        file("/cli.toml", "[path-bases]\nbase='cli'"),
        discovery(Filename::Credentials, true),
        file(
            "/p/.cargo/credentials",
            "include=['child.toml']\ntoken='fixture'",
        ),
        file("/p/.cargo/child.toml", "[path-bases]\nbase='credential'"),
        observed::Record {
            query: observed::Query::Environment(b"CARGO_PATH_BASES_BASE".to_vec()),
            value: observed::Value::Environment(None),
        },
        observed::Record {
            query: observed::Query::EnvironmentPrefix(b"CARGO_PATH_BASES_BASE_".to_vec()),
            value: observed::Value::EnvironmentPrefix(false),
        },
    ]);
    fixture!(records, |owner, session, limits| {
        owner
            .configure_files(&["/cli.toml"], &limits, &mut session)
            .unwrap();
        owner.load_credentials(&limits, &mut session).unwrap();
        let key = key("base", &mut session);
        let relative = owner
            .optional_path(&key, &limits, &mut session)
            .unwrap()
            .unwrap();
        // Credential force merge wins the value; its included file definition
        // survives, so resolution uses that file's two-parent root.
        assert_eq!(
            relative
                .resolve(Path::new("/elsewhere"), &mut session)
                .unwrap(),
            Path::new("/p/credential")
        );
        assert!(owner
            .credentials
            .as_ref()
            .unwrap()
            .snapshot()
            .to_string()
            .contains("unchanged"));
    });
}

#[test]
fn registry_string_uses_loaded_config_and_retains_definition() {
    let mut records = normal("[registries.my-registry]\nindex='file:relative-index'");
    records.extend([
        observed::Record {
            query: observed::Query::Environment(b"CARGO_REGISTRIES_MY_REGISTRY_INDEX".to_vec()),
            value: observed::Value::Environment(None),
        },
        observed::Record {
            query: observed::Query::EnvironmentPrefix(
                b"CARGO_REGISTRIES_MY_REGISTRY_INDEX_".to_vec(),
            ),
            value: observed::Value::EnvironmentPrefix(false),
        },
    ]);
    fixture!(records, |owner, session, limits| {
        let key = Key::registry(
            scalar::Name::parse("my-registry", scalar::Kind::Registry).unwrap(),
            &mut session,
        )
        .unwrap();
        let value = owner
            .optional_string(&key, &limits, &mut session)
            .unwrap()
            .unwrap();
        assert_eq!(value.text(), "file:relative-index");
        assert!(
            matches!(value.definition(), super::super::definition::Definition::Path(path) if path == Path::new("/p/.cargo/config"))
        );
        assert!(owner.ordinary.is_some());
        session.finish(Ok(())).unwrap();
    });
}

#[test]
fn obsolete_registry_index_is_checked_through_original_owner() {
    for (source, rejected) in [("[registry]\nindex=''", true), ("[registry]", false)] {
        let mut records = normal(source);
        records.extend([
            observed::Record {
                query: observed::Query::Environment(b"CARGO_REGISTRY_INDEX".to_vec()),
                value: observed::Value::Environment(None),
            },
            observed::Record {
                query: observed::Query::EnvironmentPrefix(b"CARGO_REGISTRY_INDEX_".to_vec()),
                value: observed::Value::EnvironmentPrefix(false),
            },
        ]);
        fixture!(records, |owner, session, limits| {
            let result = owner
                .check_registry_index_not_set(&limits, &mut session)
                .unwrap();
            if rejected {
                assert_eq!(result.unwrap_err(), "the `registry.index` config value is no longer supported\nUse `[source]` replacement to alter the default index for crates.io.");
            } else {
                result.unwrap();
            }
            session.finish(Ok(())).unwrap();
        });
    }
}

#[test]
fn registry_index_resolves_definition_relative_urls_and_rejects_passwords() {
    for (source, expected) in [
        ("[registries.my-registry]\nindex='file:relative-index'", Ok("file:///p/relative-index")),
        ("[registries.my-registry]\nindex='https://example.com/index'", Ok("https://example.com/index")),
        ("[registries.my-registry]\nindex='https://u:secret@example.com/index'", Err("invalid index URL for registry `my-registry` defined in /p/.cargo/config: registry URLs may not contain passwords")),
        ("[registries]", Err("registry index was not found in any configuration: `my-registry`")),
    ] {
        let mut records = normal(source);
        records.extend([
            observed::Record {
                query: observed::Query::Environment(b"CARGO_REGISTRIES_MY_REGISTRY_INDEX".to_vec()),
                value: observed::Value::Environment(None),
            },
            observed::Record {
                query: observed::Query::EnvironmentPrefix(b"CARGO_REGISTRIES_MY_REGISTRY_INDEX_".to_vec()),
                value: observed::Value::EnvironmentPrefix(false),
            },
        ]);
        fixture!(records, |owner, session, limits| {
            let name = scalar::Name::parse("my-registry", scalar::Kind::Registry).unwrap();
            let result = owner.registry_index(name, &limits, &mut session).unwrap();
            match (result, expected) {
                (Ok(url), Ok(expected)) => assert_eq!(url.as_str(&mut session).unwrap(), expected),
                (Err(error), Err(expected)) => assert_eq!(error, expected),
                _ => panic!("unexpected registry result"),
            }
            session.finish(Ok(())).unwrap();
        });
    }
}

#[test]
fn environment_presence_does_not_skip_lazy_configuration_failure() {
    fixture!(
        vec![observed::Record {
            query: observed::Query::Environment(b"CARGO_PATH_BASES_BASE".to_vec()),
            value: observed::Value::Environment(Some(b"env".to_vec()))
        }],
        |owner, session, limits| {
            let key = key("base", &mut session);
            assert!(matches!(
                owner.optional_path(&key, &limits, &mut session),
                Err(Stop::Refused)
            ));
            assert!(owner.ordinary.is_none());
        }
    );
}

#[test]
fn patch_registry_preserves_lookup_fallback_and_terminal_refusal() {
    use crate::input::membership::manifest::load::convert::patch;
    for (spelling, source, expected) in [
        (
            "crates-io",
            None,
            Some("https://github.com/rust-lang/crates.io-index"),
        ),
        (
            "https://example.com/index",
            None,
            Some("https://example.com/index"),
        ),
        (
            "mirror",
            Some("[registries.mirror]\nindex='file:relative'"),
            Some("file:///p/relative"),
        ),
        ("mirror", Some("[registries.mirror]\nindex=42"), None),
        ("mirror", Some("[registries.mirror]\nindex='bad'"), None),
        ("crates", Some(""), None),
    ] {
        let records = match source {
            None => Vec::new(),
            Some(source) => {
                let mut records = normal(source);
                let key = format!("CARGO_REGISTRIES_{}_INDEX", spelling.to_uppercase());
                records.push(observed::Record {
                    query: observed::Query::Environment(key.as_bytes().to_vec()),
                    value: observed::Value::Environment(None),
                });
                records.push(observed::Record {
                    query: observed::Query::EnvironmentPrefix(format!("{key}_").into_bytes()),
                    value: observed::Value::EnvironmentPrefix(false),
                });
                records
            }
        };
        fixture!(records, |owner, session, limits| {
            let result = patch::registry(spelling, &mut owner, &limits, &mut session).unwrap();
            match expected {
                Some(expected) => {
                    assert_eq!(result.ok().unwrap().as_str(&mut session).unwrap(), expected)
                }
                None => {
                    let error = result.err().unwrap();
                    assert!(
                        error.starts_with(&format!(
                            "[patch] entry `{spelling}` should be a URL or registry name"
                        )),
                        "{error}"
                    );
                    assert!(
                        error.contains(&format!("invalid url `{spelling}`")),
                        "{error}"
                    );
                    assert_eq!(error.contains("with a dash"), spelling == "crates");
                }
            }
            session.finish(Ok(())).unwrap();
        });
    }
    fixture!(Vec::new(), |owner, session, limits| {
        assert!(matches!(
            patch::registry("mirror", &mut owner, &limits, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            patch::registry(
                "https://example.com/index",
                &mut owner,
                &limits,
                &mut session
            ),
            Err(Stop::Refused)
        ));
    });
}

#[test]
fn unrelated_credential_keys_fall_through_to_ordinary_without_copying_it() {
    let mut records = normal("[path-bases]\nbase='ordinary'");
    records.extend([
        discovery(Filename::Credentials, true),
        file("/p/.cargo/credentials", "unrelated=true"),
    ]);
    fixture!(records, |owner, session, limits| {
        owner.load_credentials(&limits, &mut session).unwrap();
        // An unrelated credential key leaves get_cv's ordinary fallback intact.
        let key = key("base", &mut session);
        assert_eq!(
            owner.cv(&key, &limits, &mut session).unwrap().unwrap().text,
            Some("ordinary")
        );
        assert!(!owner
            .credentials
            .as_ref()
            .unwrap()
            .snapshot()
            .to_string()
            .contains("ordinary"));
    });
}

#[test]
fn repeated_owner_operations_and_original_budget_floors() {
    use crate::input::membership::manifest::tests::admits_one_short;
    // 0: absent credentials, 1: loaded credentials, 2: failed consuming CLI
    // merge, 3: complete CLI then credentials. Compare whole original accounts.
    let run = |mode, repeats, items, bytes| {
        let mut records = if mode == 0 {
            Vec::new()
        } else {
            normal("a=[]")
        };
        if mode == 2 || mode == 3 {
            records.extend([
                present("/cli.toml"),
                file("/cli.toml", if mode == 2 { "a={}" } else { "b=true" }),
            ]);
        }
        if mode != 2 {
            records.push(discovery(Filename::Credentials, mode != 0));
            if mode != 0 {
                records.push(file("/p/.cargo/credentials", "token='fixture'"));
            }
        }
        let image = image(records);
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        let mut host = match host::Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        ) {
            Ok(host) => host,
            Err(_) => return (false, allocation.usage(), allocation.exceedances()),
        };
        let mut paths = slots::Slots::new();
        let mut documents = slots::Slots::new();
        let files = Files::from_owners(paths.writer().unwrap(), documents.writer().unwrap());
        let mut owner = Owner::from_files(files, context());
        let mut session = Session::new(&mut host);
        let result = (|| {
            if mode == 2 || mode == 3 {
                let configured = owner.configure_files(&["/cli.toml"], &limits, &mut session);
                if mode == 2 {
                    let first = configured.unwrap_err();
                    assert!(matches!(first, Stop::Rejected(_)));
                    for _ in 0..repeats {
                        let next = owner
                            .configure_files(&["/unobserved.toml"], &limits, &mut session)
                            .unwrap_err();
                        assert_eq!(format!("{first:?}"), format!("{next:?}"));
                    }
                    return Err(first);
                }
                configured?;
            }
            owner.load_credentials(&limits, &mut session)?;
            for _ in 0..repeats {
                owner.load_credentials(&limits, &mut session)?;
            }
            Ok(())
        })();
        let success = result.is_ok();
        if !success && mode != 2 {
            assert!(matches!(
                owner.configure_files(&[], &limits, &mut session),
                Err(Stop::Refused)
            ));
        }
        let _ = session.finish(result);
        drop(owner);
        drop(host);
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        (success, allocation.usage(), allocation.exceedances())
    };
    let once = run(0, 0, usize::MAX, usize::MAX).1;
    let repeated = run(0, 1, usize::MAX, usize::MAX).1;
    assert!(repeated.items > once.items && repeated.bytes > once.bytes);
    assert_eq!(
        run(1, 0, usize::MAX, usize::MAX),
        run(1, 2, usize::MAX, usize::MAX)
    );
    assert_eq!(
        run(2, 0, usize::MAX, usize::MAX),
        run(2, 3, usize::MAX, usize::MAX)
    );
    let (success, usage, _) = run(3, 1, usize::MAX, usize::MAX);
    assert!(success);
    assert_eq!(
        run(3, 1, usage.items, usage.bytes),
        (true, usage, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        run(3, 1, usage.items - 1, usage.bytes),
        usage,
        input::INPUT_ITEMS_RESOURCE,
        usage.items,
    );
    admits_one_short(
        run(3, 1, usage.items, usage.bytes - 1),
        usage,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes,
    );
}

#[test]
fn credential_intermediate_type_error_precedes_environment_string_selection() {
    // Key is the governed two-component path-base request. Ordinary has no
    // matching top-level key, so loading this scalar credential entry succeeds.
    let mut records = normal("unrelated=true");
    records.extend([
        discovery(Filename::Credentials, true),
        file("/p/.cargo/credentials", "path-bases='scalar'"),
        observed::Record {
            query: observed::Query::Environment(b"CARGO_PATH_BASES_BASE".to_vec()),
            value: observed::Value::Environment(Some(b"text".to_vec())),
        },
    ]);
    fixture!(records, |owner, session, limits| {
        owner.load_credentials(&limits, &mut session).unwrap();
        let key = key("base", &mut session);
        let result = owner.optional_path(&key, &limits, &mut session);
        assert!(matches!(
            result,
            Err(Stop::Rejected(diagnostic))
                if diagnostic.tag == crate::input::membership::manifest::Tag::ConfigLookup
                    && diagnostic.expected == Some("table")
        ));
        session.check().unwrap();
    });
}

#[path = "../../normalize/dependencies/integration.rs"]
mod dependency_normalization;
mod normalization;
#[path = "../../normalize/package/tests.rs"]
mod package_normalization;
#[path = "../../normalize/tail/tests.rs"]
mod tail_normalization;

#[test]
fn crates_io_memo_retains_incumbent_and_is_bound_to_source_history() {
    use crate::input::membership::manifest::{load::convert::source, spec::url::Url};
    let empty_image = image(Vec::new());
    let mut history = source::History::new();
    let mut foreign_history = source::History::new();
    let mut records = normal("");
    records.push(observed::Record {
        query: observed::Query::Environment(b"CARGO_REGISTRY_INDEX".to_vec()),
        value: observed::Value::Environment(None),
    });
    records.push(observed::Record {
        query: observed::Query::EnvironmentPrefix(b"CARGO_REGISTRY_INDEX_".to_vec()),
        value: observed::Value::EnvironmentPrefix(false),
    });
    fixture!(records, |owner, session, limits| {
        owner.configure_files(&[], &limits, &mut session).unwrap();
        let mut sources = history.open(&mut session).unwrap();
        let url = Url::parse("https://github.com/rust-lang/crates.io-index", &mut session)
            .unwrap()
            .unwrap();
        let prior = sources
            .registry(&url, None, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        let first = owner
            .crates_io(&mut sources, &limits, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        let second = owner
            .crates_io(&mut sources, &limits, &mut session)
            .unwrap()
            .ok()
            .unwrap();
        assert!(prior == first && first == second);
        assert_eq!(sources.get(first, &mut session).unwrap().label(), None);
        // A successful memo hit needs neither any observation nor any positive
        // admission. An empty image and zero-capacity account make that exact.
        let graph = node::GraphBudget::new();
        let mut zero = input::Allocation::with_limits(0, 0, 0);
        let mut empty_host = host::Host::from_image(
            &empty_image,
            Budget {
                graph: &graph,
                allocation: &mut zero,
            },
        )
        .unwrap();
        let mut memo_session = Session::new(&mut empty_host);
        let memo = owner
            .crates_io(&mut sources, &limits, &mut memo_session)
            .unwrap()
            .ok()
            .unwrap();
        assert!(memo == first);
        memo_session.finish(Ok(())).unwrap();
        empty_host.finish().unwrap();
        let mut foreign = foreign_history.open(&mut session).unwrap();
        assert!(owner
            .crates_io(&mut foreign, &limits, &mut session)
            .is_err());
        assert!(owner.failure.is_some());
    });
}

#[test]
fn crates_io_semantic_initialization_failure_is_not_memoized() {
    use crate::input::membership::manifest::load::convert::source;
    let mut history = source::History::new();
    let mut records = normal("[registry]\nindex='obsolete'");
    records.extend([
        observed::Record {
            query: observed::Query::Environment(b"CARGO_REGISTRY_INDEX".to_vec()),
            value: observed::Value::Environment(None),
        },
        observed::Record {
            query: observed::Query::EnvironmentPrefix(b"CARGO_REGISTRY_INDEX_".to_vec()),
            value: observed::Value::EnvironmentPrefix(false),
        },
    ]);
    fixture!(records, |owner, session, limits| {
        owner.configure_files(&[], &limits, &mut session).unwrap();
        let mut sources = history.open(&mut session).unwrap();
        for _ in 0..2 {
            assert!(owner
                .crates_io(&mut sources, &limits, &mut session)
                .unwrap()
                .is_err());
            assert!(owner.crates_io.is_none());
        }
    });
}
