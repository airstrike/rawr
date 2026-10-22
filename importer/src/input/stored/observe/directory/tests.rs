use std::ffi::CStr;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::super::Error;
use crate::cargo;
use crate::input::stored::{self, Budget};
use crate::input::{Allocation, Usage};
use crate::node;

struct Fixture {
    root: PathBuf,
    file: super::lookup::Metadata,
    link: super::lookup::Metadata,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-stored-kind-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("fixture directory");
        let root = fs::canonicalize(root).expect("canonical fixture root");
        fs::write(root.join("file"), b"one").expect("fixture file");
        std::os::unix::fs::symlink(root.join("file"), root.join("link")).expect("fixture symlink");
        let file = super::lookup::Metadata::from_metadata(
            &fs::symlink_metadata(root.join("file")).expect("file metadata"),
        );
        let link = super::lookup::Metadata::from_metadata(
            &fs::symlink_metadata(root.join("link")).expect("symlink metadata"),
        );
        Self { root, file, link }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn cached_symlink_rejects_even_when_replacement_metadata_would_be_regular() {
    let fixture = Fixture::new();
    let limits = cargo::Limits::standard();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let graph = node::GraphBudget::new();
    let mut calls = 0;
    let result = super::inspect(
        nix::libc::DT_LNK,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
        |_| {
            calls += 1;
            Ok(fixture.file)
        },
    );
    assert!(matches!(
        result,
        Err(stored::Error::Observation(Error::Policy("symlink-input")))
    ));
    assert_eq!(calls, 0);
}

#[test]
fn every_known_nonlink_kind_uses_one_subsequent_nofollow_observation() {
    let fixture = Fixture::new();
    let limits = cargo::Limits::standard();
    for kind in [
        nix::libc::DT_CHR,
        nix::libc::DT_FIFO,
        nix::libc::DT_REG,
        nix::libc::DT_SOCK,
        nix::libc::DT_DIR,
        nix::libc::DT_BLK,
    ] {
        let mut allocation = Allocation::for_proof(
            Usage {
                items: 7,
                bytes: 31,
            },
            &limits,
        );
        let graph = node::GraphBudget::new();
        let mut calls = 0;
        let metadata = super::inspect(
            kind,
            &mut Budget::borrow(&limits, &mut allocation, &graph),
            |_| {
                calls += 1;
                Ok(fixture.file)
            },
        )
        .expect("cached nonsymlink kind permits metadata to decide the new type");
        assert_eq!(calls, 1);
        assert!(metadata.is_file());
    }
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let native =
        super::super::path::terminated(&fixture.root, &mut budget).expect("admitted lookup path");
    let lookup = super::lookup::Lookup::open(
        CStr::from_bytes_with_nul(&native).expect("native lookup path"),
        &mut budget,
    )
    .expect("independent lookup directory");
    let metadata = super::metadata(&lookup, Path::new("link"), nix::libc::DT_REG, &mut budget)
        .expect("file record replaced by symlink is observed without following it");
    assert!(metadata.is_symlink());
    assert!(
        !metadata.is_file() && !metadata.is_dir(),
        "the directory loop must select unsupported-input-shape, never read the target"
    );
}

#[test]
fn unknown_and_unrecognized_kinds_preserve_both_metadata_stages_and_errors() {
    let fixture = Fixture::new();
    let limits = cargo::Limits::standard();
    for kind in [nix::libc::DT_UNKNOWN, 255] {
        for first_link in [false, true] {
            let mut allocation = Allocation::for_proof(
                Usage {
                    items: 7,
                    bytes: 31,
                },
                &limits,
            );
            let graph = node::GraphBudget::new();
            let mut calls = 0;
            let result = super::inspect(
                kind,
                &mut Budget::borrow(&limits, &mut allocation, &graph),
                |_| {
                    calls += 1;
                    Ok(if first_link || calls == 2 {
                        fixture.link
                    } else {
                        fixture.file
                    })
                },
            );
            if first_link {
                assert!(matches!(
                    result,
                    Err(stored::Error::Observation(Error::Policy("symlink-input")))
                ));
                assert_eq!(calls, 1);
            } else {
                assert!(result.expect("second metadata is no-follow").is_symlink());
                assert_eq!(calls, 2);
            }
        }
        for failure in [1, 2] {
            let mut allocation = Allocation::for_proof(
                Usage {
                    items: 7,
                    bytes: 31,
                },
                &limits,
            );
            let graph = node::GraphBudget::new();
            let mut calls = 0;
            let result = super::inspect(
                kind,
                &mut Budget::borrow(&limits, &mut allocation, &graph),
                |_| {
                    calls += 1;
                    if calls == failure {
                        Err(super::super::io(
                            "fixture no-follow metadata",
                            std::io::Error::from_raw_os_error(107),
                        ))
                    } else {
                        Ok(fixture.file)
                    }
                },
            );
            assert!(
                matches!(result, Err(stored::Error::Observation(Error::Io { source, .. })) if source.raw_os_error() == Some(107))
            );
            assert_eq!(calls, failure);
        }
    }
}

#[test]
fn fallback_failure_prevents_the_next_metadata_operation() {
    let fixture = Fixture::new();
    let limits = cargo::Limits::standard();
    // Path checks past the target are charged and recorded; both
    // fallback metadata operations still run on the exhausted graph.
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
        .expect("prior path work");
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let mut calls = 0;
    super::inspect(
        nix::libc::DT_UNKNOWN,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
        |budget| {
            budget.paths(1)?;
            calls += 1;
            Ok(fixture.file)
        },
    )
    .expect("exceeding path checks are recorded, not refused");
    assert_eq!(calls, 2);
    let record = graph.exceedances();
    let axis = record
        .axis(node::PATHS_RESOURCE)
        .expect("path axis recorded");
    assert_eq!(axis.target as usize, node::MAX_PATH_CHECKS_PER_GRAPH);
    assert_eq!(
        axis.first_requested as usize,
        node::MAX_PATH_CHECKS_PER_GRAPH + 1
    );
    assert_eq!(
        axis.high_water as usize,
        node::MAX_PATH_CHECKS_PER_GRAPH + 2
    );
    assert!(allocation.bytes(0).is_ok(), "an exceedance never latches");
    // A genuine failure latched during one fallback operation (test-only cause
    // injection) refuses that operation's charge and prevents the next one.
    for remaining in [0, 1] {
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH - remaining)
            .expect("prior path work");
        let mut allocation = Allocation::for_proof(
            Usage {
                items: 7,
                bytes: 31,
            },
            &limits,
        );
        let mut calls = 0;
        let result = super::inspect(
            nix::libc::DT_UNKNOWN,
            &mut Budget::borrow(&limits, &mut allocation, &graph),
            |budget| {
                if calls == remaining {
                    budget.refuse();
                }
                budget.paths(1)?;
                calls += 1;
                Ok(fixture.file)
            },
        );
        assert!(matches!(result, Err(stored::Error::Refused)));
        assert_eq!(calls, remaining);
        assert_eq!(
            graph.census_usage().expect("graph census").1,
            node::MAX_PATH_CHECKS_PER_GRAPH,
            "a latched account charges no further path check"
        );
        assert!(
            !graph.exceedances().is_exceeded(),
            "a refused charge is a failure, not an exceedance"
        );
        assert!(matches!(
            allocation.bytes(0),
            Err(crate::Error::InputAccountRefused)
        ));
    }
}

#[test]
fn actual_directory_route_rejects_parent_swap_to_a_cold_matching_shorter_file() {
    let fixture = Fixture::new();
    let directory = fixture.root.join("D");
    let replacement = fixture.root.join("E");
    let retained = fixture.root.join("D-retained");
    fs::create_dir(&directory).expect("D");
    fs::create_dir(&replacement).expect("E");
    fs::write(directory.join("f"), b"x").expect("cold D/f");
    fs::write(replacement.join("f"), b"x").expect("cold-matching E/f");
    let limits = cargo::Limits::standard();
    let prior = Usage {
        items: 7,
        bytes: 31,
    };
    let mut allocation = Allocation::for_proof(prior, &limits);
    let graph = node::GraphBudget::new();
    let cold = super::entry(
        crate::input::proof::Role::DeclaredInput,
        None,
        &directory,
        &fixture.root,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("cold expected directory data");
    let crate::input::proof::Entry::Directory { files, .. } = cold else {
        panic!("cold directory")
    };
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].bytes, 1);
    fs::write(directory.join("f"), b"yy").expect("D/f is now different length2");
    let mut allocation = Allocation::for_proof(prior, &limits);
    let graph = node::GraphBudget::new();
    let mut enumerations = 0;
    let result = super::walk(
        crate::input::proof::Role::DeclaredInput,
        None,
        &directory,
        &fixture.root,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
        |path| {
            enumerations += 1;
            assert_eq!(path, directory);
            // This seam runs after names and their pathname-identity join. The
            // remaining route is the actual member metadata/read implementation.
            fs::rename(&directory, &retained).expect("retain enumerated D");
            std::os::unix::fs::symlink(&replacement, &directory)
                .expect("replace D with symlink to E");
            Ok(())
        },
    );
    assert_eq!(enumerations, 1);
    assert!(matches!(
        result,
        Err(stored::Error::Observation(Error::Drift(
            "input-changed-during-read"
        )))
    ));
    assert_eq!(
        fs::read(retained.join("f")).expect("anchored source still exists"),
        b"yy"
    );
    assert_eq!(
        fs::read(directory.join("f")).expect("path now names cold-matching replacement"),
        b"x"
    );
}

#[test]
fn lookup_identity_rejects_both_replacement_directory_and_symlink() {
    let fixture = Fixture::new();
    let limits = cargo::Limits::standard();
    for symlink in [false, true] {
        let source = fixture.root.join(if symlink {
            "link-source"
        } else {
            "directory-source"
        });
        let retained = source.with_extension("retained");
        fs::create_dir(&source).expect("lookup source");
        let mut allocation = Allocation::for_proof(
            Usage {
                items: 7,
                bytes: 31,
            },
            &limits,
        );
        let graph = node::GraphBudget::new();
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let native =
            super::super::path::terminated(&source, &mut budget).expect("lookup native path");
        let native = CStr::from_bytes_with_nul(&native).expect("lookup native CStr");
        let lookup = super::lookup::Lookup::open(native, &mut budget).expect("lookup owner");
        lookup
            .revalidate(native, &mut budget)
            .expect("original association");
        fs::rename(&source, &retained).expect("move original directory");
        if symlink {
            std::os::unix::fs::symlink(&retained, &source).expect("same-inode symlink");
        } else {
            fs::create_dir(&source).expect("different-inode directory");
        }
        assert!(matches!(
            lookup.revalidate(native, &mut budget),
            Err(stored::Error::Observation(Error::Drift(
                "input enumeration directory association changed"
            )))
        ));
    }
}
