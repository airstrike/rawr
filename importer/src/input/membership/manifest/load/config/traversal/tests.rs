use super::*;
use crate::input::membership::manifest::storage::slots;
use crate::input::membership::observed::discovery::{
    After, Attempt, Exists, Identity, NativeError, Transcript,
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
    budget(|budget| observed::Path::from_bytes(value.as_bytes(), budget).unwrap())
}

fn env(value: Option<&[u8]>) -> observed::Record {
    observed::Record {
        query: observed::Query::Environment(b"__CARGO_TEST_ROOT".to_vec()),
        value: observed::Value::Environment(value.map(<[u8]>::to_vec)),
    }
}

fn discovery(root: &str, selected: bool) -> observed::Record {
    let failed = Attempt::OpenFailed(NativeError::Os(2));
    let (legacy, after) = if selected {
        (
            Attempt::Acquired(Identity {
                device: 1,
                inode: 1,
            }),
            After::Second(failed),
        )
    } else {
        (failed, After::Fallback(Exists::Failed(NativeError::Os(2))))
    };
    observed::Record {
        query: observed::Query::Discovery {
            directory: path(root),
            filename: Filename::Config,
            warn: true,
        },
        value: observed::Value::Discovery(
            Transcript::checked(Filename::Config, true, legacy, after).unwrap(),
        ),
    }
}

fn canonical(root: &str, value: Result<&str, i32>) -> observed::Record {
    observed::Record {
        query: observed::Query::Canonical(path(root)),
        value: observed::Value::Canonical(value.map(path)),
    }
}

fn file(name: &str, text: &str) -> observed::Record {
    observed::Record {
        query: observed::Query::File(path(name)),
        value: observed::Value::File {
            bytes: text.as_bytes().to_vec(),
            end: observed::End::Eof,
        },
    }
}

fn run(
    mut records: Vec<observed::Record>,
    pwd: &str,
    home: &str,
    stop: Option<&str>,
) -> Result<serde_json::Value, Error<input::membership::Error>> {
    records.sort_by(|a, b| a.query.cmp(&b.query));
    let image = budget(|budget| {
        let image = stored::Stored::from_records(&records, budget).unwrap();
        stored::Stored::decode(&image.encode(budget).unwrap(), budget).unwrap()
    });
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
    let mut paths = slots::Slots::new();
    let mut documents = slots::Slots::new();
    let files = Files::from_owners(paths.writer().unwrap(), documents.writer().unwrap());
    let mut session = Session::new(&mut host);
    let result = load(
        files,
        Path::new(pwd),
        Path::new(home),
        stop.map(Path::new),
        Path::new(pwd),
        &limits,
        &mut session,
    )
    .map(|tree| tree.snapshot());
    let result = session.finish(result);
    if result.is_ok() {
        host.finish().unwrap();
    }
    result
}

#[test]
fn ancestor_includes_finish_then_merge_with_fresh_seen_and_nearest_precedence() {
    let result = run(
        vec![
            env(Some(b"/p")),
            discovery("/p/child/.cargo", true),
            file(
                "/p/child/.cargo/config",
                "include=['/shared.toml']\nanswer=2",
            ),
            file("/shared.toml", "shared=true\nanswer=0"),
            canonical("/p/child/.cargo", Err(2)),
            discovery("/p/.cargo", true),
            file("/p/.cargo/config", "include=['/shared.toml']\nanswer=1"),
            canonical("/p/.cargo", Ok("/canonical-parent")),
            canonical("/home", Ok("/canonical-parent")),
        ],
        "/p/child",
        "/home",
        Some("/p/child"),
    )
    .unwrap();
    assert_eq!(
        result[2][0],
        serde_json::json!(["answer", ["integer", ["file", "/p/child/.cargo/config"], 2]])
    );
    assert_eq!(
        result[2][1],
        serde_json::json!(["shared", ["boolean", ["file", "/shared.toml"], true]])
    );
}

#[test]
fn failed_file_read_wins_before_missing_canonical_and_later_ancestor_queries() {
    let result = run(
        vec![
            env(None),
            discovery("/p/child/.cargo", true),
            observed::Record {
                query: observed::Query::File(path("/p/child/.cargo/config")),
                value: observed::Value::Error(13),
            },
        ],
        "/p/child",
        "/home",
        None,
    );
    assert!(matches!(result, Err(Error::Host(InputError::Io(13)))));
}

#[test]
fn native_canonical_failure_keeps_raw_home_and_component_equality_suppresses_repeat() {
    run(
        vec![
            env(None),
            discovery("/p/.cargo", false),
            canonical("/p/.cargo", Err(13)),
            canonical("/p//./.cargo", Ok("/elsewhere")),
        ],
        "/p",
        "/p//./.cargo",
        Some("/p"),
    )
    .unwrap();
}

#[test]
fn nonunicode_override_uses_inclusive_explicit_stop_and_distinct_home_loads_last() {
    let result = run(
        vec![
            env(Some(b"\xff")),
            discovery("/p/.cargo", false),
            canonical("/p/.cargo", Err(2)),
            canonical("/home", Err(13)),
            discovery("/home", true),
            file("/home/config", "answer=7"),
        ],
        "/p",
        "/home",
        Some("/p"),
    )
    .unwrap();
    assert_eq!(result[2][0][1][2], 7);
}

#[test]
fn missing_canonical_is_terminal_not_native_fallback() {
    let result = run(
        vec![env(None), discovery("/p/.cargo", false)],
        "/p",
        "/home",
        Some("/p"),
    );
    assert!(matches!(result, Err(Error::Host(InputError::Unobserved))));
}

#[test]
fn successful_home_canonicalization_still_admits_eager_fallback_copy() {
    use crate::input::membership::manifest::tests::one_short;
    let mut records = vec![
        env(None),
        discovery("/p/.cargo", false),
        canonical("/p/.cargo", Err(2)),
        canonical("/home", Ok("/p/.cargo")),
    ];
    records.sort_by(|a, b| a.query.cmp(&b.query));
    let image = budget(|budget| stored::Stored::from_records(&records, budget).unwrap());
    let run = |byte_limit| {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(usize::MAX, byte_limit, usize::MAX);
        allocation.bytes(31).unwrap();
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
        let mut session = Session::new(&mut host);
        let result = load(
            files,
            Path::new("/p"),
            Path::new("/home"),
            Some(Path::new("/p")),
            Path::new("/p"),
            &limits,
            &mut session,
        )
        .map(|_| ());
        let result = session.finish(result);
        // Equality suppresses home discovery, so no later allocations follow
        // the eagerly evaluated raw-home fallback copy in this fixture.
        host.finish().unwrap();
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        (result, allocation.usage().bytes, allocation.exceedances())
    };
    let (result, complete, _) = run(usize::MAX);
    assert!(result.is_ok());
    let (result, exact, exceeded) = run(complete);
    assert!(result.is_ok());
    assert_eq!(exact, complete);
    assert!(!exceeded.is_exceeded());
    // One byte short admits the eager raw-home fallback copy, the last
    // charge, and records it as the first and only exceeding charge.
    let (result, spent, exceeded) = run(complete - 1);
    assert!(result.is_ok());
    assert_eq!(spent, complete);
    assert!(one_short(&exceeded, input::INPUT_BYTES_RESOURCE, complete));
    let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
    assert_eq!(axis.first_requested, complete as u64);
}
