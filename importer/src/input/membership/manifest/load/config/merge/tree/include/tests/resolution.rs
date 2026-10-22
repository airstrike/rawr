use super::*;
use crate::input::membership::{observed, stored, Budget};
use crate::{cargo, input, node};
use std::ffi::OsStr;

#[allow(
    dead_code,
    clippy::redundant_guards,
    reason = "pinned upstream sources are included verbatim; this site uses only part of them"
)]
mod pinned {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-definition/definition.rs"
    ));
    mod native {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/input/membership/manifest/load/path/reference.rs"
        ));
    }
    use native::cargo as paths;
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-include/resolve.rs"
    ));
    pub(super) fn run(def: &Definition, text: &str, cwd: &Path) -> std::path::PathBuf {
        resolved(def, Path::new(text), cwd)
    }
}

#[test]
fn lexical_resolution_matches_pinned_one_parent_join_and_normalize() {
    let cwd = Path::new(OsStr::from_bytes(b"/work/\xff/./place"));
    for (definition, expected) in [
        (Definition::BuiltIn, pinned::Definition::BuiltIn),
        (
            Definition::Environment("CARGO_INCLUDE"),
            pinned::Definition::Environment("CARGO_INCLUDE".into()),
        ),
        (Definition::Cli(None), pinned::Definition::Cli(None)),
        (
            Definition::Path(Path::new("/config.toml")),
            pinned::Definition::Path("/config.toml".into()),
        ),
        (
            Definition::Path(Path::new("/project/.cargo/config.toml")),
            pinned::Definition::Path("/project/.cargo/config.toml".into()),
        ),
        (
            Definition::Cli(Some(Path::new("/project/options.toml"))),
            pinned::Definition::Cli(Some("/project/options.toml".into())),
        ),
        (
            Definition::Path(Path::new(OsStr::from_bytes(b"/native/\xff/config.toml"))),
            pinned::Definition::Path(
                Path::new(OsStr::from_bytes(b"/native/\xff/config.toml")).into(),
            ),
        ),
    ] {
        for raw in [
            "child.toml",
            "../child.toml",
            "/absolute.toml",
            "./a//b.toml",
            "../../../../a.toml",
            "nul\0.toml",
            "nul\0/../a.toml",
            "🦀/../a.toml",
        ] {
            let include = Include {
                path: raw,
                definition,
                optional: false,
                span: None,
            };
            let mut account = Account::default();
            let mut session = Session::new(&mut account);
            assert_eq!(
                include.normalized(cwd, &mut session).unwrap(),
                pinned::run(&expected, raw, cwd)
            );
        }
    }
}

fn image(records: Vec<(&[u8], observed::Value)>) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records: Vec<_> = records
        .into_iter()
        .map(|(path, value)| observed::Record {
            query: observed::Query::Metadata {
                path: observed::Path::from_bytes(path, &mut budget).unwrap(),
                follow: true,
            },
            value,
        })
        .collect();
    records.sort_by(|a, b| a.query.cmp(&b.query));
    stored::Stored::from_records(&records, &mut budget).unwrap()
}

fn metadata(kind: observed::Kind) -> observed::Value {
    observed::Value::Metadata(observed::Metadata {
        kind,
        device: 1,
        inode: 2,
        owner: 3,
        mode: 0o644,
        length: 4,
        modified_seconds: 0,
        modified_nanos: 0,
        changed_seconds: 0,
        changed_nanos: 0,
    })
}

fn check(
    image: &stored::Stored,
    paths: usize,
    run: impl FnOnce(&mut Session<'_, host::Host<'_, '_>>) -> Step<()>,
) {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH - paths)
        .unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    let mut allocation = input::Allocation::new(&limits);
    let mut host = host::Host::from_image(
        image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let mut session = Session::new(&mut host);
    let result = run(&mut session);
    session.finish(result).unwrap();
    host.finish().unwrap();
}

#[test]
fn optional_only_probes_normalized_followed_path_and_preserves_os_false() {
    for (value, present) in [
        (metadata(observed::Kind::File), true),
        (metadata(observed::Kind::Directory), true),
        (metadata(observed::Kind::Symlink), true),
        (metadata(observed::Kind::Fifo), true),
        (metadata(observed::Kind::Socket), true),
        (metadata(observed::Kind::Character), true),
        (metadata(observed::Kind::Block), true),
        (metadata(observed::Kind::Unknown), true),
        (observed::Value::Absent, false),
        (observed::Value::Error(13), false),
    ] {
        let image = image(vec![(b"/native/\xff/a.toml", value)]);
        check(&image, 1, |session| {
            let include = Include {
                path: "nul\0/../a.toml",
                definition: Definition::Path(Path::new(OsStr::from_bytes(
                    b"/native/\xff/config.toml",
                ))),
                optional: true,
                span: None,
            };
            let result = include.resolve(Path::new("/unused"), session)?;
            assert_eq!(result.is_some(), present);
            if let Some(path) = result {
                assert_eq!(path.as_os_str().as_bytes(), b"/native/\xff/a.toml");
            }
            Ok(())
        });
    }
    let image = image(vec![]);
    check(&image, 0, |session| {
        for optional in [false, true] {
            let include = Include {
                path: "nul\0.toml",
                definition: Definition::Cli(None),
                optional,
                span: None,
            };
            let result = include.resolve(Path::new("/work"), session)?;
            assert_eq!(result.is_none(), optional);
        }
        let include = Include {
            path: "a.toml",
            definition: Definition::Path(Path::new("/config.toml")),
            optional: false,
            span: None,
        };
        assert_eq!(
            include.resolve(Path::new("/unused"), session)?.unwrap(),
            Path::new("/a.toml")
        );
        Ok(())
    });
}

#[test]
fn every_lexical_refusal_retains_original_cause() {
    let include = Include {
        path: "../a/./b.toml",
        definition: Definition::Path(Path::new("/work/.cargo/config.toml")),
        optional: false,
        span: None,
    };
    let run = |refusal| {
        let mut account = Account {
            refusal,
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let result = include.normalized(Path::new("/unused"), &mut session);
        if let Some(index) = refusal {
            assert!(matches!(result, Err(Stop::Refused)));
            assert!(matches!(session.items(1), Err(Stop::Refused)));
            assert!(matches!(session.finish(Ok(())), Err(Error::Admission(found)) if found==index));
        } else {
            assert_eq!(result.unwrap(), Path::new("/work/a/b.toml"));
        }
        account.requests
    };
    for index in 0..run(None) {
        assert_eq!(run(Some(index)), index + 1);
    }
}

#[test]
fn unobserved_optional_path_is_terminal_and_cannot_become_a_skip() {
    let image = image(vec![]);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut host = host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let mut session = Session::new(&mut host);
    let include = Include {
        path: "a.toml",
        definition: Definition::Cli(None),
        optional: true,
        span: None,
    };
    assert!(matches!(
        include.resolve(Path::new("/work"), &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        include.resolve(Path::new("/other"), &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Host(input::membership::Error::Unobserved))
    ));
}

#[test]
fn recorded_file_to_extraction_and_resolution_share_the_original_account() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let config_path = b"/project/.cargo/config.toml";
    let records = vec![
        observed::Record {
            query: observed::Query::Metadata {
                path: observed::Path::from_bytes(b"/project/extra.toml", &mut budget).unwrap(),
                follow: true,
            },
            value: metadata(observed::Kind::File),
        },
        observed::Record {
            query: observed::Query::File(
                observed::Path::from_bytes(config_path, &mut budget).unwrap(),
            ),
            value: observed::Value::File {
                bytes: b"include=[{path='../extra.toml',optional=true}, 'mandatory.toml']".to_vec(),
                end: observed::End::Eof,
            },
        },
    ];
    let mut records = records;
    records.sort_by(|a, b| a.query.cmp(&b.query));
    let image = stored::Stored::from_records(&records, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut session = Session::new(&mut host);
    let doc = session.document(config_path, &limits).unwrap().unwrap();
    let values = Values::from_document(
        &doc,
        Definition::Path(Path::new(OsStr::from_bytes(config_path))),
        &mut session,
    )
    .unwrap();
    let mut tree = Tree::from_values(values, &mut session).unwrap();
    let includes = tree.includes(true, &mut session).unwrap();
    assert_eq!(includes.len(), 2);
    for (include, expected) in includes
        .as_slice()
        .iter()
        .zip(["/project/extra.toml", "/project/.cargo/mandatory.toml"])
    {
        assert_eq!(
            include
                .resolve(Path::new("/unused"), &mut session)
                .unwrap()
                .unwrap(),
            Path::new(expected)
        );
    }
    session.finish(Ok(())).unwrap();
    host.finish().unwrap();
}
