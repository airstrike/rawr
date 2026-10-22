use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use super::{repository, whitespace, Failure};
use crate::input::membership::{
    self, host,
    manifest::{self, io, session::Session},
    observed::{
        self,
        git_read::{Event, Role, Transcript},
    },
    stored, Budget,
};
use crate::{cargo, input, node};

#[derive(Clone, Copy)]
enum Fact<'a> {
    File,
    Directory,
    Missing,
    Head,
    Path(&'a [u8]),
    ReadError(i32),
}

fn metadata(kind: observed::Kind) -> observed::Metadata {
    observed::Metadata {
        kind,
        device: 1,
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

fn read(role: Role, bytes: &[u8]) -> observed::Value {
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
    observed::Value::GitRead(Transcript::checked(role, events, bytes.to_vec()).unwrap())
}

fn evaluate(
    candidate: &str,
    kind: observed::Kind,
    facts: &[(&str, Fact<'_>)],
    overflowed: bool,
) -> Result<Result<repository::Kind, Failure>, manifest::Error<membership::Error>> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records = Vec::new();
    for &(name, fact) in facts {
        let path = observed::Path::from_bytes(name.as_bytes(), &mut budget).unwrap();
        let (query, value) = match fact {
            Fact::File | Fact::Directory | Fact::Missing => {
                let value = match fact {
                    Fact::File => observed::Value::Metadata(metadata(observed::Kind::File)),
                    Fact::Directory => {
                        observed::Value::Metadata(metadata(observed::Kind::Directory))
                    }
                    _ => observed::Value::Absent,
                };
                (observed::Query::Metadata { path, follow: true }, value)
            }
            Fact::Head => (
                observed::Query::GitRead {
                    path,
                    role: Role::LooseReference,
                },
                read(Role::LooseReference, b"ref: refs/heads/main\n"),
            ),
            Fact::Path(bytes) => (
                observed::Query::GitRead {
                    path,
                    role: Role::DiscoveryPath,
                },
                read(Role::DiscoveryPath, bytes),
            ),
            Fact::ReadError(code) => (
                observed::Query::GitRead {
                    path,
                    role: Role::DiscoveryPath,
                },
                observed::Value::GitRead(
                    Transcript::checked(Role::DiscoveryPath, vec![Event::Open(Err(code))], vec![])
                        .unwrap(),
                ),
            ),
        };
        records.push(observed::Record { query, value });
    }
    records.sort_by(|left, right| left.query.cmp(&right.query));
    let image = stored::Stored::from_records(&records, &mut budget).unwrap();
    let encoded = image.encode(&mut budget).unwrap();
    let image = stored::Stored::decode(&encoded, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    if overflowed {
        // An exhausted path target is recorded, not refused; only a
        // genuine failure (here, overflowing the account so the first path
        // check fails) refuses. The per-thread call-site census would itself
        // overflow across repeated injections, so it is cleared first.
        node::PATH_SITES.with(|sites| sites.borrow_mut().clear());
        graph.paths(usize::MAX).unwrap();
    }
    let mut session = Session::new(&mut host);
    let mut whitespace = whitespace::Owner::admit(&mut session).unwrap();
    let result = super::with_metadata(
        Path::new(candidate),
        metadata(kind),
        Path::new("/cwd"),
        &mut whitespace,
        &mut session,
    );
    let result = session.finish(result);
    if result.is_ok() {
        host.finish().unwrap();
    }
    result
}

fn run(
    candidate: &str,
    kind: observed::Kind,
    facts: &[(&str, Fact<'_>)],
) -> Result<repository::Kind, Failure> {
    evaluate(candidate, kind, facts, false).unwrap()
}

#[test]
fn original_file_metadata_selects_linked_or_submodule_and_moves_private_directory() {
    let prefix = [
        ("/checkout/.git", Fact::Path(b"gitdir: /privategit\n")),
        ("/privategit/HEAD", Fact::File),
        ("/privategit/HEAD", Fact::Head),
    ];
    let mut linked = prefix.to_vec();
    linked.extend([
        ("/privategit/commondir", Fact::Path(b"../common\n")),
        ("/privategit/../common/objects", Fact::Directory),
        ("/privategit/../common/refs", Fact::Directory),
    ]);
    let repository::Kind::WorkTree {
        linked_git_dir: Some(directory),
    } = run("/checkout/.git", observed::Kind::File, &linked).unwrap()
    else {
        panic!("linked worktree")
    };
    assert_eq!(directory, Path::new("/privategit"));
    assert_eq!(directory.capacity(), 18); // move the original parent-clone/push result

    let mut submodule = prefix.to_vec();
    submodule.extend([
        ("/privategit/commondir", Fact::ReadError(nix::libc::ENOENT)),
        ("/privategit/objects", Fact::Directory),
        ("/privategit/refs", Fact::Directory),
    ]);
    assert_eq!(
        run("/checkout/.git", observed::Kind::File, &submodule),
        Ok(repository::Kind::Submodule {
            git_dir: PathBuf::from("/privategit")
        })
    );

    let mut denied = prefix.to_vec();
    denied.push(("/privategit/commondir", Fact::ReadError(nix::libc::EACCES)));
    assert_eq!(
        run("/checkout/.git", observed::Kind::File, &denied),
        Err(Failure::MissingCommonDir {
            missing: "/privategit/commondir".into(),
            source: io::Failure::Native(nix::libc::EACCES)
        })
    );
}

#[test]
fn directory_worktree_pair_uses_common_storage_and_retains_plain_buffer_capacity() {
    let result = run(
        "/privategit",
        observed::Kind::Directory,
        &[
            ("/privategit/HEAD", Fact::File),
            ("/privategit/HEAD", Fact::Head),
            ("/privategit/commondir", Fact::Path(b"/common\n")),
            ("/privategit/gitdir", Fact::Path(b"/checkout/.git\n")),
            ("/common/objects", Fact::Directory),
            ("/common/refs", Fact::Directory),
        ],
    )
    .unwrap();
    let repository::Kind::WorkTreeGitDir { work_dir } = result else {
        panic!("private worktree directory")
    };
    assert_eq!(work_dir, Path::new("/checkout"));
    assert_eq!(work_dir.capacity(), 512);
}

#[test]
fn directory_common_or_gitdir_semantic_errors_fall_back_but_original_refusal_does_not() {
    for tail in [
        vec![("/r/.git/commondir", Fact::ReadError(nix::libc::EACCES))],
        vec![("/r/.git/commondir", Fact::Path(b" \t\n"))],
        vec![
            ("/r/.git/commondir", Fact::Path(b"/common")),
            ("/r/.git/gitdir", Fact::ReadError(nix::libc::EACCES)),
        ],
    ] {
        let mut facts = vec![("/r/.git/HEAD", Fact::File), ("/r/.git/HEAD", Fact::Head)];
        facts.extend(tail);
        facts.extend([
            ("/r/.git/objects", Fact::Directory),
            ("/r/.git/refs", Fact::Directory),
            ("/r/.git/index", Fact::Missing),
        ]);
        assert_eq!(
            run("/r/.git", observed::Kind::Directory, &facts),
            Ok(repository::Kind::WorkTree {
                linked_git_dir: None
            })
        );
        assert!(matches!(
            evaluate("/r/.git", observed::Kind::Directory, &facts, true),
            Err(manifest::Error::Host(membership::Error::Budget(_)))
        ));
    }
    let missing_evidence = [("/r/.git/HEAD", Fact::File), ("/r/.git/HEAD", Fact::Head)];
    assert!(matches!(
        evaluate(
            "/r/.git",
            observed::Kind::Directory,
            &missing_evidence,
            false
        ),
        Err(manifest::Error::Host(membership::Error::Unobserved))
    ));
}

#[test]
fn ordered_head_objects_refs_and_index_checks_produce_the_reached_owned_error_or_kind() {
    assert_eq!(
        run(
            "/r/.git",
            observed::Kind::Directory,
            &[("/r/.git/HEAD", Fact::Missing)]
        ),
        Err(Failure::MissingHead)
    );
    let prefix = [
        ("/r/.git/HEAD", Fact::File),
        ("/r/.git/HEAD", Fact::Head),
        ("/r/.git/commondir", Fact::ReadError(nix::libc::ENOENT)),
    ];
    let mut facts = prefix.to_vec();
    facts.push(("/r/.git/objects", Fact::Missing));
    assert_eq!(
        run("/r/.git", observed::Kind::Directory, &facts),
        Err(Failure::MissingObjectsDirectory {
            missing: "/r/.git/objects".into()
        })
    );
    let mut facts = prefix.to_vec();
    facts.extend([
        ("/r/.git/objects", Fact::Directory),
        ("/r/.git/refs", Fact::Missing),
    ]);
    assert_eq!(
        run("/r/.git", observed::Kind::Directory, &facts),
        Err(Failure::MissingRefsDirectory {
            missing: "/r/.git/refs".into()
        })
    );

    for (candidate, index, expected) in [
        (
            "/r/.git",
            Fact::Missing,
            repository::Kind::WorkTree {
                linked_git_dir: None,
            },
        ),
        (
            "/r/.git/modules/sub",
            Fact::File,
            repository::Kind::SubmoduleGitDir,
        ),
        ("/r/project.git", Fact::File, repository::Kind::PossiblyBare),
        ("/r/plain", Fact::Missing, repository::Kind::PossiblyBare),
    ] {
        let head = format!("{candidate}/HEAD");
        let common = format!("{candidate}/commondir");
        let objects = format!("{candidate}/objects");
        let refs = format!("{candidate}/refs");
        let index_path = format!("{candidate}/index");
        assert_eq!(
            run(
                candidate,
                observed::Kind::Directory,
                &[
                    (&head, Fact::File),
                    (&head, Fact::Head),
                    (&common, Fact::ReadError(nix::libc::ENOENT)),
                    (&objects, Fact::Directory),
                    (&refs, Fact::Directory),
                    (&index_path, index),
                ]
            ),
            Ok(expected)
        );
    }
}

#[test]
fn dot_component_class_uses_only_supplied_cwd_push_and_cow_clone_preserves_ownership() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    for spelling in [".", "./", ".//././"] {
        let mut session = Session::new(&mut budget);
        let result = super::conform(Path::new(spelling), Path::new("/c"), &mut session);
        let Cow::Owned(path) = session.finish(result).unwrap() else {
            panic!("dot owns cwd push")
        };
        assert_eq!(path, Path::new("/c"));
        assert_eq!(path.capacity(), 8);
    }
    let mut session = Session::new(&mut budget);
    let result = super::conform(Path::new("."), Path::new("/a/../verbatim"), &mut session);
    assert_eq!(
        session.finish(result).unwrap().as_os_str(),
        "/a/../verbatim"
    );

    let borrowed = Cow::Borrowed(Path::new("/borrowed"));
    let before = budget.allocation.usage().bytes;
    let mut session = Session::new(&mut budget);
    let result = super::clone(&borrowed, &mut session);
    assert!(matches!(session.finish(result).unwrap(), Cow::Borrowed(_)));
    assert_eq!(budget.allocation.usage().bytes, before);
    let mut owned = PathBuf::with_capacity(64);
    owned.push("/owned");
    let owned = Cow::Owned(owned);
    let mut session = Session::new(&mut budget);
    let result = super::clone(&owned, &mut session);
    let Cow::Owned(copy) = session.finish(result).unwrap() else {
        panic!("owned Cow clone")
    };
    assert_eq!(copy.capacity(), 6);
    assert_eq!(budget.allocation.usage().bytes - before, 6);
}
