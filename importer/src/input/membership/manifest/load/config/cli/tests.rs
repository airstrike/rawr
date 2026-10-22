use super::*;
use crate::input::membership::manifest::{storage::slots, Stop};
use crate::input::membership::observed::discovery::{
    After, Attempt, Filename, Identity, NativeError, Transcript,
};
use crate::input::membership::{observed, stored, Budget};
use crate::{input, node};

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
    budget(|budget| observed::Path::from_bytes(value.as_bytes(), budget).expect("native path"))
}

fn metadata(value: &str, result: observed::Value) -> observed::Record {
    observed::Record {
        query: observed::Query::Metadata {
            path: path(value),
            follow: true,
        },
        value: result,
    }
}

fn present(value: &str) -> observed::Record {
    metadata(
        value,
        observed::Value::Metadata(observed::Metadata {
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
    )
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

fn image(mut records: Vec<observed::Record>) -> stored::Stored {
    records.sort_by(|a, b| a.query.cmp(&b.query));
    budget(|budget| stored::Stored::from_records(&records, budget).expect("image"))
}

fn context() -> Context<'static> {
    Context {
        pwd: Path::new("/p"),
        home: Path::new("/p/.cargo"),
        search_stop: Some(Path::new("/p")),
        cwd: Path::new("/p"),
    }
}

fn run(
    records: Vec<observed::Record>,
    args: &[&str],
    initialized: bool,
) -> Result<serde_json::Value, Error<InputError>> {
    let image = image(records);
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
    .expect("host");
    let mut paths = slots::Slots::new();
    let mut documents = slots::Slots::new();
    let files = Files::from_owners(
        paths.writer().expect("paths"),
        documents.writer().expect("documents"),
    );
    let mut session = Session::new(&mut host);
    let normal = if initialized {
        Some(Tree::empty(Definition::BuiltIn, &mut session).expect("normal root"))
    } else {
        None
    };
    let result =
        load(files, args, normal, context(), &limits, &mut session).map(|tree| tree.snapshot());
    let result = session.finish(result);
    if result.is_ok() {
        host.finish().expect("all reached data consumed");
    }
    result
}

fn normal_records() -> Vec<observed::Record> {
    vec![
        observed::Record {
            query: observed::Query::Environment(b"__CARGO_TEST_ROOT".to_vec()),
            value: observed::Value::Environment(None),
        },
        observed::Record {
            query: observed::Query::Discovery {
                directory: path("/p/.cargo"),
                filename: Filename::Config,
                warn: true,
            },
            value: observed::Value::Discovery(
                Transcript::checked(
                    Filename::Config,
                    true,
                    Attempt::Acquired(Identity {
                        device: 1,
                        inode: 2,
                    }),
                    After::Second(Attempt::OpenFailed(NativeError::Os(2))),
                )
                .expect("discovery"),
            ),
        },
        observed::Record {
            query: observed::Query::Canonical(path("/p/.cargo")),
            value: observed::Value::Canonical(Ok(path("/p/.cargo"))),
        },
        file(
            "/p/.cargo/config",
            "include=['/cli/child.toml']\nanswer=0\nnormal=true",
        ),
    ]
}

#[test]
fn cli_files_and_includes_override_in_order_then_force_merge_normal_values() {
    let mut records = vec![
        present("/cli/a.toml"),
        present("/cli/b.toml"),
        file("/cli/a.toml", "include=['child.toml']\nanswer=2"),
        file("/cli/child.toml", "answer=1\nchild=true"),
        file("/cli/b.toml", "answer=3\ntail=true"),
    ];
    records.extend(normal_records());
    let result =
        run(records, &["/cli/a.toml", "/cli/b.toml"], false).expect("complete configured values");
    assert_eq!(
        result,
        serde_json::json!([
            "table",
            ["builtin", null],
            [
                ["answer", ["integer", ["cli", "/cli/b.toml"], 3]],
                ["child", ["boolean", ["cli", "/cli/child.toml"], true]],
                ["normal", ["boolean", ["file", "/p/.cargo/config"], true]],
                ["tail", ["boolean", ["cli", "/cli/b.toml"], true]],
            ]
        ])
    );
}

#[test]
fn shared_file_argument_seen_survives_each_fresh_second_include_pass() {
    for arguments in [
        ["/cli/a.toml", "/cli/b.toml"],
        ["/cli/a.toml", "/cli/./a.toml"],
    ] {
        let mut records = vec![
            present("/cli/a.toml"),
            present(arguments[1]),
            file("/cli/a.toml", "include=['child.toml']\na=1"),
            file("/cli/child.toml", "child=true"),
        ];
        if arguments[1] == "/cli/b.toml" {
            records.push(file("/cli/b.toml", "include=['child.toml']\nb=2"));
        }
        // No File observation for the duplicate spelling is needed: the shared
        // source seen set rejects before re-reading it, even after fresh pass.
        assert!(
            matches!(run(records, &arguments, true), Err(Error::Invalid(diagnostic)) if diagnostic.tag == Tag::ConfigCycle)
        );
    }
}

#[test]
fn missing_or_native_metadata_error_enters_parse_rejection_before_normal_queries() {
    for value in [
        observed::Value::Absent,
        observed::Value::Error(13),
        observed::Value::Error(107),
    ] {
        // No normal discovery/environment records: CLI must finish first.
        assert!(
            matches!(run(vec![metadata("/missing.toml", value)], &["/missing.toml"], false), Err(Error::Invalid(diagnostic)) if diagnostic.tag == Tag::ConfigCliParse && diagnostic.span.is_none())
        );
    }
    assert!(
        matches!(run(Vec::new(), &["/nul\0.toml"], false), Err(Error::Invalid(diagnostic)) if diagnostic.tag == Tag::ConfigCliParse)
    );
    assert!(matches!(
        run(Vec::new(), &["relative.toml"], false),
        Err(Error::Host(InputError::Unobserved))
    ));
    assert!(matches!(
        run(Vec::new(), &["/missing-record.toml"], false),
        Err(Error::Host(InputError::Unobserved))
    ));
}

#[test]
fn present_directory_load_error_is_not_inline_parse_and_file_credentials_are_allowed() {
    let mut directory = present("/directory");
    let observed::Value::Metadata(metadata) = &mut directory.value else {
        panic!("metadata")
    };
    metadata.kind = observed::Kind::Directory;
    let failed = observed::Record {
        query: observed::Query::File(path("/directory")),
        value: observed::Value::Error(nix::libc::EISDIR),
    };
    assert!(
        matches!(run(vec![directory, failed], &["/directory"], true), Err(Error::Host(InputError::Io(code))) if code == nix::libc::EISDIR)
    );
    // The security syntax checks belong only to successful inline parsing;
    // file configuration does not run them.
    let result = run(
        vec![
            present("/cli/auth.toml"),
            file("/cli/auth.toml", "[registry]\ntoken='fixture'"),
        ],
        &["/cli/auth.toml"],
        true,
    )
    .expect("file configuration");
    assert_eq!(
        result,
        serde_json::json!([
            "table",
            ["builtin", null],
            [[
                "registry",
                [
                    "table",
                    ["cli", "/cli/auth.toml"],
                    [["token", ["string", ["cli", "/cli/auth.toml"], "fixture"]]]
                ]
            ]]
        ])
    );
}

#[test]
fn initialized_values_and_empty_argv_avoid_normal_reinitialization() {
    assert_eq!(
        run(Vec::new(), &[], true).expect("initialized normal"),
        serde_json::json!(["table", ["builtin", null], []])
    );
    assert!(run(
        vec![present("/cli/a.toml"), file("/cli/a.toml", "a=1")],
        &["/cli/a.toml"],
        true
    )
    .is_ok());
}

#[test]
fn original_resource_floors_and_first_failure_precede_later_loading() {
    use crate::input::membership::manifest::tests::admits_one_short;
    let image = image(vec![present("/cli/a.toml"), file("/cli/a.toml", "a=1")]);
    // `graph_paths` 0 leaves the graph path account alone, 1 exhausts its
    // target (recorded, never refused), 2 overflows it so the first path check
    // is a genuine failure.
    let run = |items, bytes, graph_paths: u8| {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        match graph_paths {
            1 => graph
                .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
                .expect("exhaust graph"),
            2 => graph
                .paths(usize::MAX)
                .expect("the next path check overflows"),
            _ => {}
        }
        let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
        allocation.items(7).expect("prior items");
        allocation.bytes(31).expect("prior bytes");
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
        let files = Files::from_owners(
            paths.writer().expect("paths"),
            documents.writer().expect("documents"),
        );
        let mut session = Session::new(&mut host);
        let result = (|| {
            let normal = Tree::empty(Definition::BuiltIn, &mut session)?;
            load(
                files,
                &["/cli/a.toml"],
                Some(normal),
                context(),
                &limits,
                &mut session,
            )
        })();
        let success = result.is_ok();
        if !success {
            assert!(matches!(
                session.exists(b"/unobserved-later"),
                Err(Stop::Refused)
            ));
        }
        let result = session.finish(result);
        match graph_paths {
            1 => {
                assert!(result.is_ok());
                let exceeded = graph.exceedances();
                let axis = exceeded
                    .axis(node::PATHS_RESOURCE)
                    .expect("exhausted path target recorded");
                assert_eq!(
                    axis.first_requested,
                    node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
                );
                assert_eq!(axis.high_water, graph.census_usage().unwrap().1 as u64);
            }
            2 => assert!(matches!(result, Err(Error::Host(InputError::Budget(_))))),
            _ => {}
        }
        if success {
            host.finish().expect("consumed");
        } else {
            drop(host);
        }
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        (success, allocation.usage(), allocation.exceedances())
    };
    let (success, usage, _) = run(usize::MAX, usize::MAX, 0);
    assert!(success);
    assert_eq!(
        run(usage.items, usage.bytes, 0),
        (true, usage, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        run(usage.items - 1, usage.bytes, 0),
        usage,
        input::INPUT_ITEMS_RESOURCE,
        usage.items,
    );
    admits_one_short(
        run(usage.items, usage.bytes - 1, 0),
        usage,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes,
    );
    assert_eq!(
        run(usage.items, usage.bytes, 1),
        (true, usage, crate::exceedance::Record::new())
    );
    // Only a genuine graph failure refuses, and it precedes later loading.
    assert!(!run(usage.items, usage.bytes, 2).0);
}
