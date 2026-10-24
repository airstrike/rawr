use std::path::{Path, PathBuf};

use super::{repository, trust, whitespace, Failure, Request, Root};
use crate::input::membership::{
    self, host,
    manifest::{self, io, load, session::Session, Error},
    observed::{
        self,
        git_read::{Event, Role, Transcript},
    },
    stored, Budget,
};
use crate::{cargo, input, node};

enum Fact<'a> {
    Metadata(&'a str, bool, Result<observed::Metadata, i32>),
    Absent(&'a str),
    Git(&'a str, Role, Result<&'a [u8], i32>),
    Identity(u32),
}

fn metadata(kind: observed::Kind, device: u64) -> observed::Metadata {
    observed::Metadata {
        kind,
        device,
        inode: 2,
        owner: 3,
        mode: 0,
        length: 0,
        modified_seconds: 0,
        modified_nanos: 0,
        changed_seconds: 0,
        changed_nanos: 0,
    }
}

fn directory(path: &str, device: u64) -> Fact<'_> {
    Fact::Metadata(path, true, Ok(metadata(observed::Kind::Directory, device)))
}

fn read(role: Role, result: Result<&[u8], i32>) -> observed::Value {
    let (events, bytes) = match result {
        Err(code) => (vec![Event::Open(Err(code))], vec![]),
        Ok(bytes) => {
            let capacity = if role == Role::DiscoveryPath {
                512
            } else {
                128
            };
            assert!(bytes.len() < capacity);
            let mut events = vec![Event::Open(Ok(()))];
            if role == Role::DiscoveryPath {
                events.push(Event::Length(Ok(bytes.len() as u64)));
            }
            events.extend([
                Event::Length(Ok(bytes.len() as u64)),
                Event::Position(Ok(0)),
            ]);
            if !bytes.is_empty() {
                events.push(Event::Read {
                    requested: capacity,
                    result: Ok(bytes.len()),
                });
            }
            events.push(Event::Read {
                requested: capacity - bytes.len(),
                result: Ok(0),
            });
            (events, bytes.to_vec())
        }
    };
    observed::Value::GitRead(Transcript::checked(role, events, bytes).unwrap())
}

type Outcome =
    Result<Result<(repository::Path, trust::Level), Failure>, manifest::Error<membership::Error>>;

fn run(
    root: &str,
    facts: Vec<Fact<'_>>,
    cwd: &[Result<&str, i32>],
    refuse: Option<usize>,
) -> (Outcome, Vec<Option<PathBuf>>) {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records = Vec::new();
    for fact in facts {
        let (query, value) = match fact {
            Fact::Metadata(path, follow, result) => (
                observed::Query::Metadata {
                    path: observed::Path::from_bytes(path.as_bytes(), &mut budget).unwrap(),
                    follow,
                },
                result
                    .map(observed::Value::Metadata)
                    .unwrap_or_else(observed::Value::Error),
            ),
            Fact::Absent(path) => (
                observed::Query::Metadata {
                    path: observed::Path::from_bytes(path.as_bytes(), &mut budget).unwrap(),
                    follow: true,
                },
                observed::Value::Absent,
            ),
            Fact::Git(path, role, result) => (
                observed::Query::GitRead {
                    path: observed::Path::from_bytes(path.as_bytes(), &mut budget).unwrap(),
                    role,
                },
                read(role, result),
            ),
            Fact::Identity(effective_uid) => (
                observed::Query::Identity,
                observed::Value::Identity { effective_uid },
            ),
        };
        records.push(observed::Record { query, value });
    }
    records.sort_by(|left, right| left.query.cmp(&right.query));
    let image = stored::Stored::from_records(&records, &mut budget).unwrap();
    let wire = image.encode(&mut budget).unwrap();
    let image = stored::Stored::decode(&wire, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut calls = Vec::new();
    let mut session = Session::new(&mut host);
    let mut whitespace = whitespace::Owner::admit(&mut session).unwrap();
    let root = Root::absolute(Path::new(root), &mut session);
    let result = root.and_then(|root| {
        super::discover(
            root,
            &mut whitespace,
            &mut |request, session| {
                let index = calls.len();
                calls.push(match request {
                    Request::Initial => None,
                    Request::Candidate(path) => Some(path.to_owned()),
                    Request::Opening => panic!("upwards cannot request opening cwd"),
                });
                if refuse == Some(index) {
                    session.items(usize::MAX)?;
                }
                let Some(value) = cwd.get(index) else {
                    return session.fail(Error::State);
                };
                match value {
                    Ok(path) => {
                        session.items(path.len())?;
                        load::path::copy(Path::new(path), session).map(Ok)
                    }
                    Err(code) => Ok(Err(io::Failure::Native(*code))),
                }
            },
            &mut session,
        )
    });
    let result = session.finish(result);
    if result.is_ok() {
        host.finish().unwrap();
    }
    (result, calls)
}

fn worktree() -> Vec<Fact<'static>> {
    vec![
        directory("/r/.git", 1),
        Fact::Metadata("/r/.git/HEAD", true, Ok(metadata(observed::Kind::File, 1))),
        Fact::Git(
            "/r/.git/HEAD",
            Role::LooseReference,
            Ok(b"ref: refs/heads/main\n"),
        ),
        Fact::Git(
            "/r/.git/commondir",
            Role::DiscoveryPath,
            Err(libc::ENOENT),
        ),
        directory("/r/.git/objects", 1),
        directory("/r/.git/refs", 1),
        Fact::Absent("/r/.git/index"),
        Fact::Metadata("/r/.git", false, Ok(metadata(observed::Kind::Directory, 1))),
        Fact::Identity(3),
    ]
}

#[test]
fn ascends_with_cached_metadata_and_each_reached_candidate_cwd_occurrence() {
    for candidate_cwd_error in [false, true] {
        let mut facts = worktree();
        facts.extend([
            directory("/r/sub", 1),
            Fact::Absent("/r/sub/HEAD"),
            directory("/r", 1),
        ]);
        facts.push(if candidate_cwd_error {
            directory("/r/sub/.git", 1)
        } else {
            Fact::Metadata("/r/sub/.git", true, Err(libc::ENOENT))
        });
        let cwd = if candidate_cwd_error {
            vec![Ok("/initial"), Err(libc::EACCES), Ok("/later")]
        } else {
            vec![Ok("/initial"), Ok("/later")]
        };
        let (result, calls) = run("/r/sub", facts, &cwd, None);
        let (repository::Path::WorkTree(path), trust::Level::Full) = result.unwrap().unwrap()
        else {
            panic!("worktree")
        };
        assert_eq!(path, Path::new("/r"));
        assert_eq!(path.capacity(), 12); // initial len6 clone, then one separator growth
        let expected = if candidate_cwd_error {
            vec![None, Some("/r/sub/.git".into()), Some("/r/.git".into())]
        } else {
            vec![None, Some("/r/.git".into())]
        };
        assert_eq!(calls, expected);
    }
}

#[test]
fn bare_attempt_reuses_initial_metadata_and_cwd_after_dot_git_miss() {
    let facts = vec![
        directory("/r", 1),
        Fact::Metadata("/r/.git", true, Err(libc::ENOENT)),
        Fact::Metadata("/r/HEAD", true, Ok(metadata(observed::Kind::File, 1))),
        Fact::Git(
            "/r/HEAD",
            Role::LooseReference,
            Ok(b"ref: refs/heads/main\n"),
        ),
        Fact::Git("/r/commondir", Role::DiscoveryPath, Err(libc::ENOENT)),
        directory("/r/objects", 1),
        directory("/r/refs", 1),
        Fact::Absent("/r/index"),
        Fact::Metadata("/r", false, Ok(metadata(observed::Kind::Directory, 1))),
        Fact::Identity(3),
    ];
    let (result, calls) = run("/r", facts, &[Ok("/initial")], None);
    let (repository::Path::Repository(path), trust::Level::Full) = result.unwrap().unwrap() else {
        panic!("bare")
    };
    assert_eq!(path, Path::new("/r"));
    assert_eq!(path.capacity(), 8);
    assert_eq!(calls, vec![None]);
}

#[test]
fn initial_dot_git_candidate_keeps_source_pop_order_and_root_termination() {
    let facts = vec![
        directory("/r/.git", 1),
        Fact::Absent("/r/.git/HEAD"),
        directory("/r", 1),
        Fact::Absent("/r/HEAD"),
    ];
    let (result, calls) = run("/r/.git", facts, &[Ok("/initial"), Ok("/second")], None);
    assert_eq!(
        result.unwrap(),
        Err(Failure::NoGitRepository {
            path: "/r/.git".into()
        })
    );
    assert_eq!(calls, vec![None, Some("/r".into())]);
}

#[test]
fn original_errors_and_filesystem_boundary_keep_their_owned_paths() {
    let (result, calls) = run("/../../r", vec![], &[Ok("/initial")], None);
    assert_eq!(
        result.unwrap(),
        Err(Failure::InvalidInput {
            directory: "/../../r".into()
        })
    );
    assert_eq!(calls, vec![None]);
    for fact in [
        Fact::Metadata("/r", true, Err(libc::EACCES)),
        Fact::Metadata("/r", true, Ok(metadata(observed::Kind::File, 1))),
    ] {
        assert_eq!(
            run("/r", vec![fact], &[Ok("/initial")], None).0.unwrap(),
            Err(Failure::InaccessibleDirectory { path: "/r".into() })
        );
    }
    let facts = vec![
        directory("/r/sub", 1),
        Fact::Metadata("/r/sub/.git", true, Err(libc::ENOENT)),
        Fact::Absent("/r/sub/HEAD"),
        directory("/r", 2),
    ];
    assert_eq!(
        run("/r/sub", facts, &[Ok("/initial")], None).0.unwrap(),
        Err(Failure::NoGitRepositoryWithinFs {
            path: "/r/sub".into(),
            limit: "/r".into()
        })
    );
    let mut facts = worktree();
    facts.retain(|fact| !matches!(fact, Fact::Identity(_) | Fact::Metadata(_, false, _)));
    facts.extend([
        directory("/r", 1),
        Fact::Metadata("/r/.git", false, Err(libc::EACCES)),
    ]);
    assert_eq!(
        run("/r", facts, &[Ok("/initial"), Ok("/later")], None)
            .0
            .unwrap(),
        Err(Failure::CheckTrust {
            path: "/r/.git".into(),
            err: io::Failure::Native(libc::EACCES)
        })
    );
}

#[test]
fn initial_cwd_error_occurrence_refusal_and_missing_facts_do_not_become_absence() {
    assert_eq!(
        run("/r", vec![], &[Err(libc::EACCES)], None)
            .0
            .unwrap(),
        Err(Failure::CurrentDir(io::Failure::Native(libc::EACCES)))
    );
    assert!(matches!(
        run("/r", vec![], &[Ok("/initial")], None).0,
        Err(manifest::Error::Host(membership::Error::Unobserved))
    ));
    let (result, calls) = run(
        "/r",
        vec![directory("/r", 1), directory("/r/.git", 1)],
        &[Ok("/initial"), Ok("/later")],
        Some(1),
    );
    assert!(matches!(result, Err(manifest::Error::Admission(_))));
    assert_eq!(calls, vec![None, Some("/r/.git".into())]);
    let (result, calls) = run("..", vec![], &[], None);
    assert!(matches!(result, Err(manifest::Error::State)));
    assert!(calls.is_empty()); // rejected input domain, not a source semantic miss
}
