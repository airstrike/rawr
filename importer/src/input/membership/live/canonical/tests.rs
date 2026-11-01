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
) -> (Result<observed::Record>, Native, input::Usage) {
    let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
    allocation.items(7).unwrap();
    allocation.bytes(31).unwrap();
    let mut native = Native::new(reply);
    let result = acquire(
        path,
        &mut Budget {
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
    let (result, native, usage) = run(path, bytes(output), usize::MAX, usize::MAX);
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
        let (result, native, _) = run(b"/p", Reply::Error(error), usize::MAX, usize::MAX);
        assert!(matches!(result, Err(Error::Encoding(_))));
        assert_eq!(native.queries.len(), 1);
    }
    for output in [b"".as_slice(), b"relative"] {
        let (result, native, _) = run(b"/p", bytes(output), usize::MAX, usize::MAX);
        assert!(matches!(result, Err(Error::Encoding(_))));
        assert_eq!(native.queries.len(), 1);
    }
}

#[test]
fn output_refusal_latches_original_account_and_prevents_another_native_call() {
    // The first input spends five bytes; the nine-byte output overflows the
    // account's checked arithmetic, a genuine failure. Nothing else
    // prevents another query/input unless that refusal is latched.
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    allocation.bytes(usize::MAX - 5).unwrap();
    let mut native = Native::new(bytes(b"/resolved"));
    let mut budget = Budget {
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

    // Crossing the byte target instead is recorded and never latches: the
    // output is owned and another native call is still made.
    let mut allocation = input::Allocation::with_limits(usize::MAX, 10, usize::MAX);
    let mut native = Native::new(bytes(b"/resolved"));
    let mut budget = Budget {
        allocation: &mut allocation,
    };
    acquire(b"/p", &mut budget, &mut native).expect("output past the target is owned");
    assert_eq!(native.queries.len(), 1);
    assert_eq!(budget.allocation.usage().bytes, 14);
    native.reply = Some(bytes(b"/resolved"));
    acquire(b"/p", &mut budget, &mut native).expect("nothing latched");
    assert_eq!(native.queries.len(), 2);
    assert_eq!(allocation.usage().bytes, 28);
}

#[test]
fn input_longer_than_transfer_capacity_reaches_native_unchanged() {
    let mut path = vec![b'x'; rawr_canonical::BUFFER_BYTES * 2];
    path[0] = b'/';
    let (result, native, _) = run(&path, bytes(b"/short"), usize::MAX, usize::MAX);
    assert!(result.is_ok());
    assert_eq!(native.queries[0].len(), path.len() + 1);
    assert_eq!(&native.queries[0][..path.len()], path);
}

#[test]
fn invalid_observation_queries_do_not_call_native() {
    for path in [b"".as_slice(), b"relative", b"/nul\0tail"] {
        let (result, native, _) = run(path, bytes(b"/unused"), usize::MAX, usize::MAX);
        assert!(matches!(result, Err(Error::Encoding(_))));
        assert!(native.queries.is_empty());
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
        let mut allocation = input::Allocation::new(&limits);
        let record = super::super::canonical(
            path.as_os_str().as_bytes(),
            &mut Budget {
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
