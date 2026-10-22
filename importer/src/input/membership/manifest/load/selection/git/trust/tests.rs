use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

use super::Level;
use crate::input::membership::{
    self, host,
    manifest::{self, io, session::Session},
    observed, stored, Budget,
};
use crate::{cargo, input, node};

fn run(
    path: &[u8],
    metadata: Option<observed::Value>,
    uid: Option<u32>,
    environment: Option<Option<&[u8]>>,
    overflowed: bool,
) -> Result<Result<Level, io::Failure>, manifest::Error<membership::Error>> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records = Vec::new();
    if let Some(value) = metadata {
        records.push(observed::Record {
            query: observed::Query::Metadata {
                path: observed::Path::from_bytes(path, &mut budget).unwrap(),
                follow: false,
            },
            value,
        });
    }
    if let Some(effective_uid) = uid {
        records.push(observed::Record {
            query: observed::Query::Identity,
            value: observed::Value::Identity { effective_uid },
        });
    }
    if let Some(value) = environment {
        records.push(observed::Record {
            query: observed::Query::Environment(b"SUDO_UID".to_vec()),
            value: observed::Value::Environment(value.map(<[u8]>::to_vec)),
        });
    }
    records.sort_by(|left, right| left.query.cmp(&right.query));
    let image = stored::Stored::from_records(&records, &mut budget).unwrap();
    let wire = image.encode(&mut budget).unwrap();
    let image = stored::Stored::decode(&wire, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    if overflowed {
        // An exhausted path target is recorded, not refused; only a
        // genuine failure (here, overflowing the account so the first path
        // check fails) refuses.
        graph.paths(usize::MAX).unwrap();
    }
    let mut session = Session::new(&mut host);
    let result = super::from_path(Path::new(OsStr::from_bytes(path)), &mut session);
    let result = session.finish(result);
    if result.is_ok() {
        host.finish().unwrap();
    }
    result
}

fn owned(owner: u32) -> observed::Value {
    observed::Value::Metadata(observed::Metadata {
        kind: observed::Kind::Directory,
        device: 1,
        inode: 2,
        owner,
        mode: 0,
        length: 0,
        modified_seconds: 0,
        modified_nanos: 0,
        changed_seconds: 0,
        changed_nanos: 0,
    })
}

#[test]
fn identity_match_skips_environment_and_mismatch_uses_owned_exact_decimal_bytes() {
    assert_eq!(
        run(b"/git", Some(owned(3)), Some(3), None, false).unwrap(),
        Ok(Level::Full)
    );
    for (owner, text, expected) in [
        (3, b"3".as_slice(), Level::Full),
        (3, b"+3", Level::Full),
        (3, b"00000000000000000000000000003", Level::Full),
        (u32::MAX, b"4294967295", Level::Full),
        (3, b"4", Level::Reduced),
        (3, b"3 ", Level::Reduced),
        (3, b" 3", Level::Reduced),
        (3, b"", Level::Reduced),
        (3, b"+", Level::Reduced),
        (3, b"-0", Level::Reduced),
        (3, b"3_0", Level::Reduced),
        (3, b"4294967296", Level::Reduced),
        (3, b"\xff3", Level::Reduced),
    ] {
        // The effective user need not be root for SUDO_UID to be consulted.
        assert_eq!(
            run(
                b"/git",
                Some(owned(owner)),
                Some(22),
                Some(Some(text)),
                false
            )
            .unwrap(),
            Ok(expected)
        );
    }
    assert_eq!(
        run(b"/git", Some(owned(3)), Some(22), Some(None), false).unwrap(),
        Ok(Level::Reduced)
    );
}

#[test]
fn native_metadata_failure_precedes_identity_and_missing_or_refused_facts_stay_terminal() {
    assert_eq!(
        run(
            b"/git",
            Some(observed::Value::Error(nix::libc::EACCES)),
            None,
            None,
            false
        )
        .unwrap(),
        Err(io::Failure::Native(nix::libc::EACCES))
    );
    assert_eq!(
        run(b"/git\0suffix", None, None, None, false).unwrap(),
        Err(io::Failure::Nul)
    );
    assert!(matches!(
        run(b"/git", Some(observed::Value::Absent), None, None, false),
        Err(manifest::Error::Host(membership::Error::Unobserved))
    ));
    assert!(matches!(
        run(b"/git", Some(owned(3)), None, None, false),
        Err(manifest::Error::Host(membership::Error::Unobserved))
    ));
    assert!(matches!(
        run(b"/git", Some(owned(3)), Some(3), None, true),
        Err(manifest::Error::Host(membership::Error::Budget(_)))
    ));
}
