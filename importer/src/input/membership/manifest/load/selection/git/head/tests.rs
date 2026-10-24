use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

use super::{check, Failure};
use crate::input::membership::manifest::{
    load::selection::git::loose::{read, store},
    session::Session,
};
use crate::input::membership::{
    host,
    observed::{
        self,
        git_read::{Event, Role, Transcript},
    },
    stored, Budget,
};
use crate::{cargo, input, node};

enum File<'a> {
    Absent,
    Error(i32),
    Bytes(&'a [u8]),
}

fn run(exists: observed::Value, files: &[(&[u8], File<'_>)]) -> Result<(), Failure> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records = vec![observed::Record {
        query: observed::Query::Metadata {
            path: observed::Path::from_bytes(b"/fixture/HEAD", &mut budget).unwrap(),
            follow: true,
        },
        value: exists,
    }];
    for (path, file) in files {
        let (events, bytes) = match file {
            File::Absent => (vec![Event::Open(Err(libc::ENOENT))], vec![]),
            File::Error(code) => (vec![Event::Open(Err(*code))], vec![]),
            File::Bytes(bytes) => {
                assert!(bytes.len() < 128);
                let mut events = vec![
                    Event::Open(Ok(())),
                    Event::Length(Ok(bytes.len() as u64)),
                    Event::Position(Ok(0)),
                ];
                if !bytes.is_empty() {
                    events.push(Event::Read {
                        requested: 128,
                        result: Ok(bytes.len()),
                    });
                }
                events.push(Event::Read {
                    requested: 128 - bytes.len(),
                    result: Ok(0),
                });
                (events, bytes.to_vec())
            }
        };
        records.push(observed::Record {
            query: observed::Query::GitRead {
                path: observed::Path::from_bytes(path, &mut budget).unwrap(),
                role: Role::LooseReference,
            },
            value: observed::Value::GitRead(
                Transcript::checked(Role::LooseReference, events, bytes).unwrap(),
            ),
        });
    }
    records.sort_by(|left, right| left.query.cmp(&right.query));
    let image = stored::Stored::from_records(&records, &mut budget).unwrap();
    let encoded = image.encode(&mut budget).unwrap();
    let image = stored::Stored::decode(&encoded, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut session = Session::new(&mut host);
    let result = check(Path::new("/fixture"), &mut session);
    let result = session.finish(result).unwrap();
    if let Ok(checked) = &result {
        assert_eq!(checked.directory(), Path::new("/fixture"));
    }
    host.finish().unwrap();
    result.map(|_| ())
}

fn present() -> observed::Value {
    observed::Value::Metadata(observed::Metadata {
        kind: observed::Kind::File,
        device: 1,
        inode: 2,
        owner: 3,
        mode: 0,
        length: 0,
        modified_seconds: 0,
        modified_nanos: 0,
        changed_seconds: 0,
        changed_nanos: 0,
    })
}

#[test]
fn head_fast_guard_precedes_store_and_direct_decode_failure_is_tolerated() {
    assert_eq!(run(observed::Value::Absent, &[]), Err(Failure::MissingHead));
    assert_eq!(
        run(observed::Value::Error(libc::EACCES), &[]),
        Err(Failure::MissingHead)
    );
    for bytes in [
        b"ref: refs/heads/main\n".as_slice(),
        b"invalid",
        &[b'0'; 64],
        b"ref: bad?",
    ] {
        assert_eq!(
            run(present(), &[(b"/fixture/HEAD", File::Bytes(bytes))]),
            Ok(())
        );
    }
}

#[test]
fn head_alternates_preserve_missing_misplaced_and_original_errors() {
    let prefix = b"/fixture/HEAD".as_slice();
    assert_eq!(
        run(
            present(),
            &[
                (prefix, File::Absent),
                (b"/fixture/refs/HEAD", File::Bytes(b"ref: refs/heads/main"))
            ]
        ),
        Err(Failure::MisplacedHead {
            name: b"refs/HEAD".to_vec()
        })
    );
    assert!(matches!(
        run(
            present(),
            &[
                (prefix, File::Absent),
                (b"/fixture/refs/HEAD", File::Bytes(b"broken"))
            ]
        ),
        Err(Failure::Find(store::Failure::Find(
            read::Failure::ReferenceCreation { .. }
        )))
    ));
    assert!(matches!(
        run(present(), &[(prefix, File::Error(libc::EACCES))]),
        Err(Failure::Find(store::Failure::Find(
            read::Failure::ReadFileContents {
                code: libc::EACCES,
                ..
            }
        )))
    ));
    let all = [
        b"/fixture/HEAD".as_slice(),
        b"/fixture/refs/HEAD",
        b"/fixture/refs/tags/HEAD",
        b"/fixture/refs/heads/HEAD",
        b"/fixture/refs/remotes/HEAD",
    ];
    let missing = all.map(|path| (path, File::Absent));
    assert_eq!(
        run(present(), &missing),
        Err(Failure::Find(store::Failure::NotFound {
            name: "HEAD".into()
        }))
    );
    let last = [
        (all[0], File::Absent),
        (all[1], File::Absent),
        (all[2], File::Absent),
        (all[3], File::Absent),
        (all[4], File::Bytes(b"ref: refs/heads/main")),
    ];
    assert_eq!(
        run(present(), &last),
        Err(Failure::MisplacedHead {
            name: b"refs/remotes/HEAD".to_vec()
        })
    );
}

#[test]
fn head_nul_is_source_local_missing_without_observation() {
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
    let result = check(
        Path::new(OsStr::from_bytes(b"/fixture/\0git")),
        &mut session,
    );
    assert!(matches!(
        session.finish(result).unwrap(),
        Err(Failure::MissingHead)
    ));
    host.finish().unwrap();
}

#[test]
fn head_nul_at_cstring_boundary_admits_conversion_before_local_rejection() {
    use crate::input::membership::manifest::tests::one_short;
    for joined in [383usize, 384] {
        let conversion = if joined < 384 { 0 } else { joined + 1 };
        let parent = joined - 5;
        // Source join clones parent, then the separator grows its byte Vec.
        let joined_storage = parent + parent * 2;
        let required = joined_storage + conversion;
        let mut directory = vec![b'x'; joined - 5];
        directory[0] = b'/';
        directory[2] = 0;
        for allowed in [joined_storage, required - 1, required] {
            let graph = node::GraphBudget::new();
            let mut allocation = input::Allocation::with_limits(10_000, allowed, 1024);
            let mut budget = Budget {
                graph: &graph,
                allocation: &mut allocation,
            };
            let image = stored::Stored::from_records(&[], &mut budget).unwrap();
            let mut host = host::Host::from_image(&image, budget).unwrap();
            let mut session = Session::new(&mut host);
            let result = check(Path::new(OsStr::from_bytes(&directory)), &mut session);
            let result = session.finish(result);
            assert!(
                matches!(result.unwrap(), Err(Failure::MissingHead)),
                "joined {joined}, allowed {allowed}"
            );
            assert_eq!(host.budget().allocation.usage().bytes, required);
            let exceeded = host.budget().allocation.exceedances();
            if allowed < required {
                // The conversion past a short byte target is admitted and
                // recorded with the exact joined-plus-conversion extent.
                assert!(
                    one_short(&exceeded, input::INPUT_BYTES_RESOURCE, required),
                    "joined {joined}, allowed {allowed}: {exceeded:?}"
                );
            } else {
                assert!(!exceeded.is_exceeded());
            }
            host.finish().unwrap();
        }
    }
}
