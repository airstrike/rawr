use super::*;
use crate::input::membership::manifest::load::config::values::Values;
use crate::input::membership::manifest::load::{
    features::{self, Features},
    Warnings,
};
use crate::input::membership::manifest::{
    reference, scalar, schema::manifest::Original, value, Error, Source, Stop,
};
use crate::input::membership::{observed, stored, Budget};
use crate::{cargo, input, node};
use std::{
    ffi::OsString,
    os::unix::ffi::{OsStrExt, OsStringExt},
};

fn key<'a, A: document::Admission>(name: &'a str, session: &mut Session<'_, A>) -> Key<'a> {
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
        scalar::Name::parse(name, scalar::Kind::PathBase).unwrap(),
        &features,
        session,
    )
    .unwrap()
}

fn path(value: &serde_json::Value) -> PathBuf {
    if value[0] == "file-bytes" {
        OsString::from_vec(serde_json::from_value(value[1].clone()).unwrap()).into()
    } else {
        value[1].as_str().unwrap_or("").into()
    }
}
fn definition<'a>(value: &'a serde_json::Value, path: &'a Path) -> Definition<'a> {
    match value[0].as_str().unwrap() {
        "builtin" => Definition::BuiltIn,
        "file" | "file-bytes" => Definition::Path(path),
        "env" => Definition::Environment(value[1].as_str().unwrap()),
        "cli" => Definition::Cli(value[1].as_str().map(|_| path)),
        _ => panic!("fixture origin"),
    }
}
fn rendered_origin(origin: Definition<'_>) -> serde_json::Value {
    match origin {
        Definition::BuiltIn => serde_json::json!(["builtin", null]),
        Definition::Path(path) => serde_json::json!(["file", path]),
        Definition::Environment(key) => serde_json::json!(["env", key]),
        Definition::Cli(path) => serde_json::json!(["cli", path]),
    }
}

#[test]
fn optional_path_matches_pinned_value_protocol_and_selected_origin_resolution() {
    for line in include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-lookup/observations.jsonl"
    ))
    .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let ordinary_document = document::parse(
            row["ordinary"]["source"].as_str().unwrap(),
            80,
            &mut reference::Account,
        )
        .unwrap();
        let ordinary_path = path(&row["ordinary"]["definition"]);
        let credential_document = row["credentials"]["source"]
            .as_str()
            .map(|text| document::parse(text, 80, &mut reference::Account).unwrap());
        let credential_path = path(&row["credentials"]["definition"]);
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let environment_key = format!(
            "CARGO_PATH_BASES_{}",
            row["base"]
                .as_str()
                .unwrap()
                .replace('-', "_")
                .to_uppercase()
        );
        let bytes: Option<Vec<u8>> = serde_json::from_value(row["environment"].clone()).unwrap();
        let mut records = vec![observed::Record {
            query: observed::Query::Environment(environment_key.as_bytes().to_vec()),
            value: observed::Value::Environment(bytes.clone()),
        }];
        if bytes.is_none() {
            records.push(observed::Record {
                query: observed::Query::EnvironmentPrefix(
                    format!("{environment_key}_").into_bytes(),
                ),
                value: observed::Value::EnvironmentPrefix(row["prefix"].as_bool().unwrap()),
            });
        }
        let image = stored::Stored::from_records(&records, &mut budget).unwrap();
        let mut host = host::Host::from_image(&image, budget).unwrap();
        let mut session = Session::new(&mut host);
        let key = key(row["base"].as_str().unwrap(), &mut session);
        let values = Values::from_document(
            &ordinary_document,
            definition(&row["ordinary"]["definition"], &ordinary_path),
            &mut session,
        )
        .unwrap();
        let ordinary = Tree::from_values(values, &mut session).unwrap();
        let credentials = credential_document.as_ref().map(|document| {
            let values = Values::from_document(
                document,
                definition(&row["credentials"]["definition"], &credential_path),
                &mut session,
            )
            .unwrap();
            Tree::from_values(values, &mut session).unwrap()
        });
        let tables = Tables::from_trees(&ordinary, credentials.as_ref());
        let result = tables.optional_path(&key, &mut session);
        if let Some(error) = row["error"].as_str() {
            let Some(Stop::Rejected(diagnostic)) = result.err() else {
                panic!("expected rejection: {row}");
            };
            let tag = if error == "missing" {
                Tag::ConfigMissing
            } else if error.contains("tuple") {
                Tag::ConfigDefinition
            } else {
                Tag::ConfigLookup
            };
            assert_eq!(diagnostic.tag, tag, "{row}");
            if error.starts_with("expected table") {
                assert_eq!(diagnostic.expected, Some("table"), "{row}");
            }
            if error == "expected a string" {
                assert_eq!(diagnostic.expected, Some("a string"), "{row}");
            }
            session.finish(Ok(())).unwrap();
        } else if row["result"].is_null() {
            assert!(result.unwrap().is_none(), "{row}");
            session.finish(Ok(())).unwrap();
        } else {
            let relative = result.unwrap().unwrap();
            assert_eq!(
                relative.text,
                row["result"]["text"].as_str().unwrap(),
                "{row}"
            );
            assert_eq!(
                rendered_origin(relative.origin.definition()),
                row["result"]["origin"],
                "{row}"
            );
            let resolved = relative.resolve(Path::new("/work"), &mut session);
            if row["result"]["path"].is_null() {
                assert!(matches!(resolved, Err(Stop::Refused)), "{row}");
                assert!(
                    matches!(
                        session.finish(resolved),
                        Err(Error::Source {
                            cause: Source::ConfigDefinitionSecondParent,
                            ..
                        })
                    ),
                    "{row}"
                );
            } else {
                let expected: Vec<u8> =
                    serde_json::from_value(row["result"]["path"].clone()).unwrap();
                assert_eq!(resolved.unwrap().as_os_str().as_bytes(), expected, "{row}");
                session.finish(Ok(())).unwrap();
            }
        }
        host.finish().unwrap();
    }
}

#[test]
fn absent_record_is_terminal_even_when_a_configured_value_exists() {
    let document = document::parse(
        "path-bases.workspace='present'",
        80,
        &mut reference::Account,
    )
    .unwrap();
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
    let key = key("workspace", &mut session);
    let values = Values::from_document(
        &document,
        Definition::Path(Path::new("/project/.cargo/config")),
        &mut session,
    )
    .unwrap();
    let tree = Tree::from_values(values, &mut session).unwrap();
    let tables = Tables::from_trees(&tree, None);
    assert!(matches!(
        tables.optional_path(&key, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        tables.optional_path(&key, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Host(input::membership::Error::Unobserved))
    ));
}

#[derive(Default)]
struct Account {
    requests: usize,
    refusal: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, _: document::Request) -> Result<(), usize> {
        let index = self.requests;
        self.requests += 1;
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

#[test]
fn lossy_origin_codec_and_resolution_keep_original_refusal() {
    let file = PathBuf::from(OsString::from_vec(
        b"/a/\xff\xf0\x80/.cargo/config".to_vec(),
    ));
    fn run(file: &Path, session: &mut Session<'_, Account>) -> Step<()> {
        let origin = origin::Origin::decode(Definition::Path(file), session)?;
        origin
            .definition()
            .resolve("child", Path::new("/work"), session)?;
        Ok(())
    }
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    let result = run(&file, &mut session);
    session.finish(result).unwrap();
    for refusal in 0..baseline.requests {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(run(&file, &mut session), Err(Stop::Refused)));
        assert!(matches!(run(&file, &mut session), Err(Stop::Refused)));
        assert!(matches!(session.finish(Ok(())),Err(Error::Admission(index)) if index==refusal));
        assert_eq!(account.requests, refusal + 1);
    }
}

#[test]
fn environment_string_floor_records_before_final_type_or_cli_selection_but_not_parent_error() {
    for (source, parent_error) in [
        ("path-bases.workspace='cli'", false),
        ("path-bases.workspace=42", false),
        ("path-bases='wrong parent'", true),
    ] {
        let document = document::parse(source, 80, &mut reference::Account).unwrap();
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let image = stored::Stored::from_records(
            &[observed::Record {
                query: observed::Query::Environment(b"CARGO_PATH_BASES_WORKSPACE".to_vec()),
                value: observed::Value::Environment(Some(vec![b'x'; 100])),
            }],
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .unwrap();
        // Preserve prior image ownership; lower this original account's next
        // decoded-string allowance before interpreting the config and getter.
        allocation.string_limit = 64;
        let mut host = host::Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .unwrap();
        let mut session = Session::new(&mut host);
        let key = key("workspace", &mut session);
        let values = Values::from_document(&document, Definition::Cli(None), &mut session).unwrap();
        let tree = Tree::from_values(values, &mut session).unwrap();
        let tables = Tables::from_trees(&tree, None);
        let result = tables.optional_path(&key, &mut session);
        if parent_error {
            assert!(matches!(
                result,
                Err(Stop::Rejected(Diagnostic {
                    tag: Tag::ConfigLookup,
                    expected: Some("table"),
                    ..
                }))
            ));
            session.finish(Ok(())).unwrap();
            // The parent error precedes the environment string read.
            assert!(!host.budget().allocation.exceedances().is_exceeded());
        } else {
            // The 100-byte environment value crossing the 64-byte string
            // target is recorded; the getter still reaches the final type
            // check or CLI selection and the account never latches.
            if source == "path-bases.workspace='cli'" {
                assert!(matches!(result, Ok(Some(ref relative)) if relative.text == "cli"));
            } else {
                assert!(matches!(
                    result,
                    Err(Stop::Rejected(Diagnostic {
                        tag: Tag::ConfigLookup,
                        expected: Some("a string"),
                        ..
                    }))
                ));
            }
            session.finish(Ok(())).unwrap();
            assert!(host.budget().allocation.check().is_ok());
            let record = host.budget().allocation.exceedances();
            let mut axes = record.axes();
            let axis = axes
                .next()
                .copied()
                .expect("the environment string crossing is recorded");
            assert!(axes.next().is_none(), "{record:?}");
            assert_eq!(axis.name, "membership decoded TOML string bytes");
            assert_eq!(
                (axis.target, axis.first_requested, axis.high_water),
                (64, 100, 100)
            );
            // Only a genuinely refused original account refuses the getter,
            // and that refusal is terminal for every later query.
            host.budget().allocation.refuse();
            let mut session = Session::new(&mut host);
            assert!(matches!(
                tables.optional_path(&key, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                tables.optional_path(&key, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                session.finish(Ok(())),
                Err(Error::Host(crate::input::membership::Error::Input(_)))
            ));
        }
        host.finish().unwrap();
    }
}

#[test]
fn recorded_files_through_merge_and_optional_path_keep_one_original_account() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records = Vec::new();
    for (path, source) in [
        ("/project/Cargo.toml", "cargo-features=['path-bases']"),
        ("/base/.cargo/old.toml", "path-bases.workspace='old'"),
        (
            "/override/.cargo/new.toml",
            "path-bases.workspace='relative'",
        ),
    ] {
        records.push(observed::Record {
            query: observed::Query::File(
                observed::Path::from_bytes(path.as_bytes(), &mut budget).unwrap(),
            ),
            value: observed::Value::File {
                bytes: source.as_bytes().to_vec(),
                end: observed::End::Eof,
            },
        });
    }
    records.push(observed::Record {
        query: observed::Query::Environment(b"CARGO_PATH_BASES_WORKSPACE".to_vec()),
        value: observed::Value::Environment(Some(b"ignored-env".to_vec())),
    });
    records.sort_by(|left, right| left.query.cmp(&right.query));
    let image = stored::Stored::from_records(&records, &mut budget).unwrap();
    let encoded = image.encode(&mut budget).unwrap();
    let image = stored::Stored::decode(&encoded, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut session = Session::new(&mut host);
    let manifest = session
        .document(b"/project/Cargo.toml", &limits)
        .unwrap()
        .unwrap();
    let original = Original::interpret(value::Input::root(&manifest), &mut session).unwrap();
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
    let key = Key::path_base(
        scalar::Name::parse("workspace", scalar::Kind::PathBase).unwrap(),
        &features,
        &mut session,
    )
    .unwrap();
    let left_document = session
        .document(b"/base/.cargo/old.toml", &limits)
        .unwrap()
        .unwrap();
    let right_document = session
        .document(b"/override/.cargo/new.toml", &limits)
        .unwrap()
        .unwrap();
    let left = Values::from_document(
        &left_document,
        Definition::Path(Path::new("/base/.cargo/old.toml")),
        &mut session,
    )
    .unwrap();
    let right = Values::from_document(
        &right_document,
        Definition::Cli(Some(Path::new("/override/.cargo/new.toml"))),
        &mut session,
    )
    .unwrap();
    let left = Tree::from_values(left, &mut session).unwrap();
    let right = Tree::from_values(right, &mut session).unwrap();
    let merged = left.merge(right, true, &mut session).unwrap();
    let tables = Tables::from_trees(&merged, None);
    let selected = tables.optional_path(&key, &mut session).unwrap().unwrap();
    assert_eq!(
        selected.resolve(Path::new("/work"), &mut session).unwrap(),
        Path::new("/override/relative")
    );
    session.finish(Ok(())).unwrap();
    host.finish().unwrap();
}
