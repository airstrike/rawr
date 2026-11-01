use super::*;
use crate::input::membership::manifest::Error;
use crate::input::membership::{observed, stored, Budget, Error as InputError};
use crate::{cargo, input, node};

#[derive(Default)]
struct Account;
impl document::Admission for Account {
    type Error = ();
    fn admit(&mut self, _: document::Request) -> Result<(), ()> {
        Ok(())
    }
}

fn image(env: Option<Option<&[u8]>>, paths: &[(&[u8], bool)]) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records = Vec::new();
    if let Some(env) = env {
        records.push(observed::Record {
            query: observed::Query::Environment(b"__CARGO_TEST_ROOT".to_vec()),
            value: observed::Value::Environment(env.map(<[u8]>::to_vec)),
        });
    }
    for (path, present) in paths {
        let path = observed::Path::from_bytes(path, &mut budget).unwrap();
        records.push(observed::Record {
            query: observed::Query::Metadata { path, follow: true },
            value: if *present {
                observed::Value::Metadata(observed::Metadata {
                    kind: observed::Kind::Directory,
                    device: 1,
                    inode: 2,
                    owner: 3,
                    mode: 0o755,
                    length: 0,
                    modified_seconds: 0,
                    modified_nanos: 0,
                    changed_seconds: 0,
                    changed_nanos: 0,
                })
            } else {
                observed::Value::Absent
            },
        });
    }
    records.sort_by(|a, b| a.query.cmp(&b.query));
    stored::Stored::from_records(&records, &mut budget).unwrap()
}

#[test]
fn cache_hit_does_not_read_environment_or_probe_candidates() {
    let image = image(None, &[]);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
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
    let mut roots = roots::Roots::new();
    roots
        .insert(
            Path::new("/root"),
            roots::Root {
                directory: Path::new("/root"),
                declaration: None,
            },
            &mut session,
        )
        .unwrap();
    let result = begin(
        Path::new("/root/member/Cargo.toml"),
        Path::new("/cargo"),
        &roots,
        &mut session,
    )
    .unwrap();
    let Discovery::Cached(path) = result else {
        panic!("cache miss");
    };
    assert_eq!(path, Path::new("/root/Cargo.toml"));
    session.finish(Ok(())).unwrap();
    host.finish().unwrap();
}

#[test]
fn present_candidate_yields_before_any_later_probe() {
    // A directory is present for Path::exists. It must be passed to the full
    // loader, whose subsequent read will decide its failure; do not skip it.
    let image = image(
        Some(None),
        &[(b"/root/a/Cargo.toml", false), (b"/root/Cargo.toml", true)],
    );
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
    let Discovery::Candidates(mut candidates) = begin(
        Path::new("/root/a/member/Cargo.toml"),
        Path::new("/cargo"),
        &roots::Roots::new(),
        &mut session,
    )
    .unwrap() else {
        panic!("cache hit");
    };
    assert_eq!(
        candidates.next(&mut session).unwrap().unwrap(),
        Path::new("/root/Cargo.toml")
    );
    session.finish(Ok(())).unwrap();
    host.finish().unwrap(); // No /Cargo.toml observation is needed yet.
}

#[test]
fn candidate_boundaries_match_skip_stop_and_lookbehind_order() {
    for (manifest, home, stop, expected) in [
        (
            "/a/b/Cargo.toml",
            "/cargo",
            None,
            vec!["/a/Cargo.toml", "/Cargo.toml"],
        ),
        ("/a/b/Cargo.toml", "/a", None, vec!["/a/Cargo.toml"]),
        (
            "/a/b/Cargo.toml",
            "/a/b",
            None,
            vec!["/a/Cargo.toml", "/Cargo.toml"],
        ),
        ("/a/target/package/b/Cargo.toml", "/cargo", None, vec![]),
        (
            "/a/b/Cargo.toml",
            "/cargo",
            Some("/a"),
            vec!["/a/Cargo.toml"],
        ),
        ("/a/b/Cargo.toml", "/cargo", Some("/a/b"), vec![]),
        ("/a/b/Cargo.toml", "/cargo", Some("/a/b/Cargo.toml"), vec![]),
        (
            "a/b/Cargo.toml",
            "/cargo",
            None,
            vec!["a/Cargo.toml", "Cargo.toml"],
        ),
    ] {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut candidates = Candidates::new(
            Path::new(manifest),
            Path::new(home),
            stop.map(Path::new),
            &mut session,
        )
        .unwrap();
        let mut seen = Vec::new();
        assert!(candidates
            .next_with(&mut session, |path, _| {
                seen.push(path.to_owned());
                Ok(false)
            })
            .unwrap()
            .is_none());
        assert_eq!(
            seen,
            expected.iter().map(PathBuf::from).collect::<Vec<_>>(),
            "{manifest}, {home}, {stop:?}"
        );
        assert!(candidates
            .next_with(&mut session, |_, _| panic!("probe after exhaustion"))
            .unwrap()
            .is_none());
    }
}

#[test]
fn unrecorded_environment_and_metadata_are_terminal() {
    for env in [None, Some(None)] {
        let image = image(env, &[]);
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
        let result = begin(
            Path::new("/root/member/Cargo.toml"),
            Path::new("/cargo"),
            &roots::Roots::new(),
            &mut session,
        );
        match result {
            Ok(Discovery::Candidates(mut candidates)) => {
                assert!(env.is_some());
                assert!(candidates.next(&mut session).is_err());
                assert!(candidates.next(&mut session).is_err());
            }
            Err(_) => assert!(env.is_none()),
            _ => panic!("unexpected cache hit"),
        }
        assert!(matches!(
            session.finish(Ok(())),
            Err(Error::Host(InputError::Unobserved))
        ));
    }
}

#[test]
fn non_unicode_test_root_matches_absent_but_valid_stop_is_inclusive() {
    for (env, expected) in [
        (None, "/Cargo.toml"),
        (Some(&b"\xff"[..]), "/Cargo.toml"),
        (Some(&b"/root"[..]), "none"),
    ] {
        let paths: &[(&[u8], bool)] = if expected == "none" {
            &[(b"/root/Cargo.toml", false)]
        } else {
            &[(b"/root/Cargo.toml", false), (b"/Cargo.toml", true)]
        };
        let image = image(Some(env), paths);
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
        let Discovery::Candidates(mut candidates) = begin(
            Path::new("/root/member/Cargo.toml"),
            Path::new("/cargo"),
            &roots::Roots::new(),
            &mut session,
        )
        .unwrap() else {
            panic!("cache hit");
        };
        let result = candidates.next(&mut session).unwrap();
        assert_eq!(
            result.as_deref(),
            if expected == "none" {
                None
            } else {
                Some(Path::new(expected))
            }
        );
        session.finish(Ok(())).unwrap();
        host.finish().unwrap();
    }
}

#[test]
fn discovery_spends_original_allocation_and_graph_accounts() {
    let image = image(
        Some(None),
        &[(b"/root/a/Cargo.toml", false), (b"/root/Cargo.toml", true)],
    );
    // `overflow` fills the graph path account so the first check is a
    // genuine failure.
    let run = |overflow: bool| {
        let graph = node::GraphBudget::new();
        if overflow {
            graph.paths(usize::MAX).unwrap();
        }
        let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        let mut host = match host::Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        ) {
            Ok(host) => host,
            Err(_) => return false,
        };
        let mut session = Session::new(&mut host);
        let result = (|| {
            let Discovery::Candidates(mut candidates) = begin(
                Path::new("/root/a/member/Cargo.toml"),
                Path::new("/cargo"),
                &roots::Roots::new(),
                &mut session,
            )?
            else {
                panic!("cache hit");
            };
            let result = candidates.next(&mut session);
            if result.is_err() {
                assert!(candidates.next(&mut session).is_err());
            }
            assert_eq!(result?, Some(PathBuf::from("/root/Cargo.toml")));
            Ok(())
        })();
        let result = session.finish(result);
        if overflow {
            assert!(matches!(result, Err(Error::Host(InputError::Budget(_)))));
        }
        let ok = result.is_ok();
        if ok {
            host.finish().unwrap();
        } else {
            drop(host);
        }
        assert!(allocation.check().is_ok());
        ok
    };
    assert!(run(false));
    // Only a genuine graph failure refuses, and the next candidate stays refused.
    assert!(!run(true));
}

#[test]
fn driver_search_preserves_explicit_cache_and_lazy_discovery_branches() {
    use super::super::driver::{Request, Search};
    for (explicit, cached) in [(true, false), (true, true), (false, true), (false, false)] {
        // Only a full cache miss without an explicit pointer may read the
        // environment or probe. Missing observations make accidental I/O fail.
        let image = if !explicit && !cached {
            image(Some(None), &[(b"/root/Cargo.toml", true)])
        } else {
            image(None, &[])
        };
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
        let mut roots = roots::Roots::new();
        if cached {
            roots
                .insert(
                    Path::new("/root"),
                    roots::Root {
                        directory: Path::new("/root"),
                        declaration: None,
                    },
                    &mut session,
                )
                .unwrap();
        }
        let request = Request {
            manifest: Path::new("/root/member/Cargo.toml"),
            explicit_root: explicit.then_some(".."),
        };
        match request
            .search(Path::new("/cargo"), &roots, &mut session)
            .unwrap()
        {
            Search::Selected(Lookup::Cached(root)) => {
                assert!(cached);
                assert_eq!(root.directory, Path::new("/root"));
            }
            Search::Selected(Lookup::Load(path)) => {
                assert!(explicit && !cached);
                assert_eq!(path, Path::new("/root/Cargo.toml"));
            }
            Search::Candidates(mut candidates) => {
                assert!(!explicit && !cached);
                assert_eq!(
                    candidates.next(&mut session).unwrap().unwrap(),
                    Path::new("/root/Cargo.toml")
                );
            }
        }
        session.finish(Ok(())).unwrap();
        host.finish().unwrap();
    }
}
