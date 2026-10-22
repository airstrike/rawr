use super::*;
use crate::{input, node};

enum Reply {
    Bytes(Vec<u8>),
    Error(rawr_canonical::Error),
}

struct Native {
    reply: Option<Reply>,
    queries: Vec<Vec<u8>>,
}

impl Native {
    fn new(reply: Reply) -> Self {
        Self {
            reply: Some(reply),
            queries: Vec::new(),
        }
    }
}

impl System for Native {
    fn canonicalize<'buffer>(
        &mut self,
        path: &CStr,
        buffer: &'buffer mut rawr_canonical::Buffer,
    ) -> std::result::Result<&'buffer CStr, rawr_canonical::Error> {
        assert!(buffer.iter().all(|byte| *byte == 0));
        self.queries.push(path.to_bytes_with_nul().to_vec());
        match self.reply.take().expect("only one native call") {
            Reply::Error(error) => Err(error),
            Reply::Bytes(bytes) => {
                assert!(bytes.len() < buffer.len());
                buffer[..bytes.len()].copy_from_slice(&bytes);
                CStr::from_bytes_until_nul(buffer)
                    .map_err(|_| rawr_canonical::Framing::Termination.into())
            }
        }
    }
}

fn run(
    path: &[u8],
    reply: Reply,
    items: usize,
    bytes: usize,
    available_paths: usize,
) -> (Result<observed::Record>, Native, input::Usage) {
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH - available_paths)
        .unwrap();
    let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
    allocation.items(7).unwrap();
    allocation.bytes(31).unwrap();
    let mut native = Native::new(reply);
    let result = acquire(
        path,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
        &mut native,
    );
    (result, native, allocation.usage())
}

fn bytes(value: &[u8]) -> Reply {
    Reply::Bytes(value.to_vec())
}

#[test]
fn exact_native_query_and_actual_success_bytes_are_owned_under_original_account() {
    let path = b"/p//./\xff";
    let output = b"/canonical/\xff";
    let (result, native, usage) = run(path, bytes(output), usize::MAX, usize::MAX, 1);
    let record = result.unwrap();
    let observed::Query::Canonical(query) = record.query else {
        panic!("canonical query")
    };
    let observed::Value::Canonical(Ok(value)) = record.value else {
        panic!("canonical success")
    };
    assert_eq!(query.bytes(), path);
    assert_eq!(value.bytes(), output);
    assert_eq!(native.queries, [b"/p//./\xff\0".to_vec()]);
    assert_eq!(usage.items, 7 + 3);
    assert_eq!(usage.bytes, 31 + path.len() + path.len() + 1 + output.len());
}

#[test]
fn positive_native_errors_remain_exact_data_without_retry_or_output_ownership() {
    for code in [2, 4, 13, 107] {
        let (result, native, usage) = run(
            b"/p",
            Reply::Error(rawr_canonical::Error::Native(
                std::io::Error::from_raw_os_error(code),
            )),
            usize::MAX,
            usize::MAX,
            1,
        );
        assert!(
            matches!(result.unwrap().value, observed::Value::Canonical(Err(actual)) if actual == code)
        );
        assert_eq!(native.queries.len(), 1);
        assert_eq!(usage.items, 9);
        assert_eq!(usage.bytes, 31 + 2 + 3);
    }
}

#[test]
fn malformed_native_results_and_unsupported_are_outer_terminal_errors() {
    for error in [
        rawr_canonical::Error::Framing(rawr_canonical::Framing::Pointer),
        rawr_canonical::Error::Framing(rawr_canonical::Framing::Termination),
        rawr_canonical::Error::Unsupported,
        rawr_canonical::Error::Native(std::io::Error::from_raw_os_error(0)),
        rawr_canonical::Error::Native(std::io::Error::from_raw_os_error(-1)),
        rawr_canonical::Error::Native(std::io::Error::from(std::io::ErrorKind::Other)),
    ] {
        let (result, native, _) = run(b"/p", Reply::Error(error), usize::MAX, usize::MAX, 1);
        assert!(matches!(result, Err(Error::Encoding(_))));
        assert_eq!(native.queries.len(), 1);
    }
    for output in [b"".as_slice(), b"relative"] {
        let (result, native, _) = run(b"/p", bytes(output), usize::MAX, usize::MAX, 1);
        assert!(matches!(result, Err(Error::Encoding(_))));
        assert_eq!(native.queries.len(), 1);
    }
}

#[test]
fn every_original_ownership_floor_records_exceedance_with_one_complete_native_call() {
    let path = b"/p";
    let output = b"/resolved";
    let (_, _, complete) = run(path, bytes(output), usize::MAX, usize::MAX, 1);
    // Ownership targets are measured, never gates. Every floor still
    // makes exactly one native call and owns the complete record; the
    // crossed axis records a truthful high water and nothing latches.
    for item_limit in 7..=complete.items {
        let graph = node::GraphBudget::new();
        graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH - 1).unwrap();
        let mut allocation = input::Allocation::with_limits(item_limit, complete.bytes, usize::MAX);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        let mut native = Native::new(bytes(output));
        acquire(
            path,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
            &mut native,
        )
        .expect("items past the target are owned");
        assert_eq!(native.queries.len(), 1);
        assert_eq!(allocation.usage(), complete);
        let exceeded = allocation.exceedances();
        if item_limit == complete.items {
            assert!(!exceeded.is_exceeded());
        } else {
            let axis = exceeded.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
            // Items are charged one at a time: the first crossing request is
            // exactly one past the target.
            assert_eq!(
                (axis.target, axis.first_requested, axis.high_water),
                (
                    item_limit as u64,
                    item_limit as u64 + 1,
                    complete.items as u64
                )
            );
            assert!(exceeded.axis(input::INPUT_BYTES_RESOURCE).is_none());
        }
        assert!(allocation.items(0).is_ok(), "nothing latched");
    }
    // Byte charges in ownership order: query path, terminated native path,
    // then the native output.
    let charges = [
        31 + path.len(),
        31 + path.len() + path.len() + 1,
        31 + path.len() + path.len() + 1 + output.len(),
    ];
    assert_eq!(charges[2], complete.bytes);
    for byte_limit in 31..=complete.bytes {
        let (result, native, usage) = run(path, bytes(output), complete.items, byte_limit, 1);
        result.expect("bytes past the target are owned");
        assert_eq!(native.queries.len(), 1);
        assert_eq!(usage.bytes, complete.bytes);
    }
    for byte_limit in 31..complete.bytes {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(complete.items, byte_limit, usize::MAX);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        let mut native = Native::new(bytes(output));
        acquire(
            path,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
            &mut native,
        )
        .unwrap();
        let axis = allocation
            .exceedances()
            .axis(input::INPUT_BYTES_RESOURCE)
            .copied()
            .unwrap();
        let crossing = charges
            .iter()
            .copied()
            .find(|charge| *charge > byte_limit)
            .unwrap();
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (byte_limit as u64, crossing as u64, complete.bytes as u64)
        );
        assert!(allocation
            .exceedances()
            .axis(input::INPUT_ITEMS_RESOURCE)
            .is_none());
    }
    // The graph's path-check target is likewise recorded, not refused.
    let (result, native, _) = run(path, bytes(output), complete.items, complete.bytes, 0);
    result.expect("the lookup past the target is still made");
    assert_eq!(native.queries.len(), 1);
    // A genuinely latched account makes no native call and owns nothing.
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    allocation.refuse();
    let mut native = Native::new(bytes(output));
    assert!(matches!(
        acquire(
            path,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
            &mut native,
        ),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert!(native.queries.is_empty());
    assert_eq!(allocation.usage().bytes, 0);
}

#[test]
fn output_refusal_latches_original_account_and_prevents_another_native_call() {
    let graph = node::GraphBudget::new();
    // The first input spends five bytes; the nine-byte output overflows the
    // account's checked arithmetic, a genuine failure. Nothing else
    // prevents another query/input unless that refusal is latched.
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    allocation.bytes(usize::MAX - 5).unwrap();
    let mut native = Native::new(bytes(b"/resolved"));
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    assert!(matches!(
        acquire(b"/p", &mut budget, &mut native),
        Err(Error::Input(crate::Error::Node {
            source,
            ..
        })) if matches!(*source, node::Error::Allocation)
    ));
    assert_eq!(native.queries.len(), 1);
    assert!(matches!(
        acquire(b"/p", &mut budget, &mut native),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert_eq!(native.queries.len(), 1);
    assert_eq!(allocation.usage().bytes, usize::MAX);
    assert!(!allocation.exceedances().is_exceeded());

    // Crossing the byte target instead is recorded and never latches: the
    // output is owned and another native call is still made.
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, 10, usize::MAX);
    let mut native = Native::new(bytes(b"/resolved"));
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    acquire(b"/p", &mut budget, &mut native).expect("output past the target is owned");
    assert_eq!(native.queries.len(), 1);
    assert_eq!(budget.allocation.usage().bytes, 14);
    native.reply = Some(bytes(b"/resolved"));
    acquire(b"/p", &mut budget, &mut native).expect("nothing latched");
    assert_eq!(native.queries.len(), 2);
    assert_eq!(allocation.usage().bytes, 28);
    let axis = allocation
        .exceedances()
        .axis(input::INPUT_BYTES_RESOURCE)
        .copied()
        .unwrap();
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (10, 14, 28)
    );
}

#[test]
fn input_longer_than_transfer_capacity_reaches_native_unchanged() {
    let mut path = vec![b'x'; rawr_canonical::BUFFER_BYTES * 2];
    path[0] = b'/';
    let (result, native, _) = run(&path, bytes(b"/short"), usize::MAX, usize::MAX, 1);
    assert!(result.is_ok());
    assert_eq!(native.queries[0].len(), path.len() + 1);
    assert_eq!(&native.queries[0][..path.len()], path);
}

#[test]
fn invalid_observation_queries_do_not_call_native() {
    for path in [b"".as_slice(), b"relative", b"/nul\0tail"] {
        let (result, native, _) = run(path, bytes(b"/unused"), usize::MAX, usize::MAX, 1);
        assert!(matches!(result, Err(Error::Encoding(_))));
        assert!(native.queries.is_empty());
    }
}

#[test]
fn input_and_output_string_exceedances_are_recorded_on_the_shared_path_account() {
    for oversized_input in [false, true] {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, 2);
        let mut native = Native::new(bytes(b"/resolved"));
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let query = if oversized_input {
            b"/oversized".as_slice()
        } else {
            b"/p"
        };
        // Input and output paths share one string target; crossing it
        // is recorded with the first crossing length and never latches.
        acquire(query, &mut budget, &mut native).expect("oversized paths are owned");
        assert_eq!(native.queries.len(), 1);
        let axis = budget
            .allocation
            .exceedances()
            .axis("membership native path bytes")
            .copied()
            .unwrap();
        let first = if oversized_input { 10 } else { 9 };
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (2, first, first)
        );
        // This query fits the string target and the aggregate accounts have
        // ample space; nothing prevents another call.
        native.reply = Some(bytes(b"/resolved"));
        acquire(b"/p", &mut budget, &mut native).expect("nothing latched");
        assert_eq!(native.queries.len(), 2);
        let first_call = if oversized_input {
            10 + 11 + 9
        } else {
            2 + 3 + 9
        };
        assert_eq!(allocation.usage().bytes, first_call + 2 + 3 + 9);
        assert_eq!(
            allocation
                .exceedances()
                .axis("membership native path bytes")
                .unwrap()
                .high_water,
            first
        );
    }
}

#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
#[test]
fn native_adapter_preserves_exact_queries_and_matches_std_in_quiescent_fixture() {
    use std::{
        os::unix::{ffi::OsStrExt as _, fs::symlink},
        path::{Path, PathBuf},
    };

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn compare(path: &Path) {
        let limits = crate::cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let record = super::super::canonical(
            path.as_os_str().as_bytes(),
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .expect("native adapter returns complete observation");
        let observed::Query::Canonical(query) = record.query else {
            panic!("canonical query")
        };
        assert_eq!(query.bytes(), path.as_os_str().as_bytes());
        match (record.value, std::fs::canonicalize(path)) {
            (observed::Value::Canonical(Ok(actual)), Ok(expected)) => {
                assert_eq!(actual.bytes(), expected.as_os_str().as_bytes());
            }
            (observed::Value::Canonical(Err(actual)), Err(expected)) => {
                assert_eq!(Some(actual), expected.raw_os_error());
            }
            (actual, expected) => panic!("native/std mismatch: {actual:?} vs {expected:?}"),
        }
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rawr-live-canonical-{}-{nonce}",
        std::process::id()
    ));
    assert!(root.is_absolute());
    std::fs::create_dir(&root).expect("exclusive fixture directory");
    let fixture = Fixture(root);
    std::fs::create_dir(fixture.0.join("nested")).expect("fixture target");
    symlink(fixture.0.join("nested"), fixture.0.join("absolute-link")).expect("absolute symlink");
    compare(Path::new("/"));
    compare(&fixture.0.join("nested//./"));
    compare(&fixture.0.join("absolute-link"));
    compare(&fixture.0.join("missing"));
}
