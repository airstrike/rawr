use super::super::trust;
use super::*;
use crate::input::membership::{
    manifest::{self, load},
    observed::{
        self,
        git_read::{Event, Role, Transcript},
    },
    stored, Budget,
};
use crate::{cargo, input, node};

#[test]
fn complete_discovery_calls_opening_cwd_and_replays_commondir_before_stage_one() {
    for refuse_opening in [false, true] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut records = Vec::new();
        for (name, follow, kind) in [
            ("/r/.git", true, Some(observed::Kind::Directory)),
            ("/r/.git", false, Some(observed::Kind::Directory)),
            ("/r/.git/HEAD", true, Some(observed::Kind::File)),
            ("/r/.git/objects", true, Some(observed::Kind::Directory)),
            ("/r/.git/refs", true, Some(observed::Kind::Directory)),
            ("/r/.git/index", true, None),
        ] {
            let value = kind.map_or(observed::Value::Absent, |kind| {
                observed::Value::Metadata(observed::Metadata {
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
                })
            });
            records.push(observed::Record {
                query: observed::Query::Metadata {
                    path: observed::Path::from_bytes(name.as_bytes(), &mut budget).unwrap(),
                    follow,
                },
                value,
            });
        }
        let head = b"ref: refs/heads/main\n";
        for (name, role, events, bytes) in [
            (
                "/r/.git/HEAD",
                Role::LooseReference,
                vec![
                    Event::Open(Ok(())),
                    Event::Length(Ok(head.len() as u64)),
                    Event::Position(Ok(0)),
                    Event::Read {
                        requested: 128,
                        result: Ok(head.len()),
                    },
                    Event::Read {
                        requested: 128 - head.len(),
                        result: Ok(0),
                    },
                ],
                head.to_vec(),
            ),
            (
                "/r/.git/commondir",
                Role::DiscoveryPath,
                vec![Event::Open(Err(libc::ENOENT))],
                vec![],
            ),
            (
                "/r/.git/config",
                Role::Configuration { capacity: 512 },
                vec![Event::Open(Err(libc::ENOENT))],
                vec![],
            ),
        ] {
            records.push(observed::Record {
                query: observed::Query::GitRead {
                    path: observed::Path::from_bytes(name.as_bytes(), &mut budget).unwrap(),
                    role,
                },
                value: observed::Value::GitRead(Transcript::checked(role, events, bytes).unwrap()),
            });
        }
        records.push(observed::Record {
            query: observed::Query::Identity,
            value: observed::Value::Identity { effective_uid: 3 },
        });
        records.sort_by(|a, b| a.query.cmp(&b.query));
        let image = stored::Stored::from_records(&records, &mut budget).unwrap();
        let wire = image.encode(&mut budget).unwrap();
        let image = stored::Stored::decode(&wire, &mut budget).unwrap();
        let mut host = host::Host::from_image(&image, budget).unwrap();
        let mut session = Session::new(&mut host);
        let mut whitespace = whitespace::Owner::admit(&mut session).unwrap();
        let root = upwards::Root::absolute(Path::new("/r/.git"), &mut session).unwrap();
        let mut calls = vec![];
        let result = super::discover(
            root,
            &mut whitespace,
            &mut |request, session| {
                let path = match request {
                    upwards::Request::Initial => {
                        calls.push(0);
                        "/initial"
                    }
                    upwards::Request::Opening => {
                        calls.push(1);
                        if refuse_opening {
                            session.items(usize::MAX)?;
                        }
                        "/opening"
                    }
                    upwards::Request::Candidate(_) => panic!("cached first candidate"),
                };
                session.items(path.len())?;
                load::path::copy(Path::new(path), session).map(Ok)
            },
            &mut session,
        );
        let result = session.finish(result);
        assert_eq!(calls, [0, 1]);
        if refuse_opening {
            assert!(matches!(result, Err(manifest::Error::Admission(_))));
        } else {
            let local = result.unwrap().unwrap();
            assert_eq!(local.git_dir, Path::new("/r/.git"));
            assert_eq!(local.worktree.as_deref(), Some(Path::new("/r")));
            assert_eq!(local.current_dir, Path::new("/opening"));
            assert!(local.common_dir.is_none());
            host.finish().unwrap();
        }
    }
}

#[test]
fn local_configuration_controls_all_four_transforms_after_original_spelling_read() {
    for enabled in [false, true] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let git = Path::new("/cafe\u{301}/.git");
        let common = Path::new("/commo\u{308}n");
        let worktree = Path::new("/worke\u{301}");
        let cwd = Path::new("/cwda\u{30a}");
        let bytes = format!("[core]\nprecomposeUnicode={enabled}\n").into_bytes();
        let role = Role::Configuration { capacity: 512 };
        let transcript = Transcript::checked(
            role,
            vec![
                Event::Open(Ok(())),
                Event::Length(Ok(bytes.len() as u64)),
                Event::Position(Ok(0)),
                Event::Read {
                    requested: 512,
                    result: Ok(bytes.len()),
                },
                Event::Read {
                    requested: 512 - bytes.len(),
                    result: Ok(0),
                },
            ],
            bytes,
        )
        .unwrap();
        let records = [observed::Record {
            query: observed::Query::GitRead {
                path: observed::Path::from_bytes("/commo\u{308}n/config".as_bytes(), &mut budget)
                    .unwrap(),
                role,
            },
            value: observed::Value::GitRead(transcript),
        }];
        let image = stored::Stored::from_records(&records, &mut budget).unwrap();
        let wire = image.encode(&mut budget).unwrap();
        let image = stored::Stored::decode(&wire, &mut budget).unwrap();
        let mut host = host::Host::from_image(&image, budget).unwrap();
        let mut session = Session::new(&mut host);
        let stage = config::Stage::local(common, git, trust::Level::Full, &mut session)
            .unwrap()
            .unwrap();
        let local = Local {
            git_dir: load::path::copy(git, &mut session).unwrap(),
            worktree: Some(load::path::copy(worktree, &mut session).unwrap()),
            common_dir: Some(load::path::copy(common, &mut session).unwrap()),
            current_dir: load::path::copy(cwd, &mut session).unwrap(),
            stage,
        };
        let result = local.precomposed(&mut session);
        let local = session.finish(result).unwrap();
        if enabled {
            assert_eq!(local.git_dir, Path::new("/café/.git"));
            assert_eq!(local.common_dir.as_deref(), Some(Path::new("/commön")));
            assert_eq!(local.worktree.as_deref(), Some(Path::new("/worké")));
            assert_eq!(local.current_dir, Path::new("/cwdå"));
        } else {
            assert_eq!(local.git_dir, git);
            assert_eq!(local.common_dir.as_deref(), Some(common));
            assert_eq!(local.worktree.as_deref(), Some(worktree));
            assert_eq!(local.current_dir, cwd);
        }
        host.finish().unwrap();
    }
}
