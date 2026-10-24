use super::*;
use crate::input::membership::{live, observed, stored, Budget};
use crate::{input, node};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-selection-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn metadata(path: &Path, follow: bool, budget: &mut Budget<'_>) -> observed::Record {
    let value = match if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    } {
        Err(error) => observed::Value::Error(error.raw_os_error().unwrap()),
        Ok(value) => observed::Value::Metadata(observed::Metadata {
            kind: if value.is_dir() {
                observed::Kind::Directory
            } else {
                observed::Kind::File
            },
            device: value.dev(),
            inode: value.ino(),
            owner: value.uid(),
            mode: value.mode(),
            length: value.len(),
            modified_seconds: value.mtime(),
            modified_nanos: value.mtime_nsec() as u32,
            changed_seconds: value.ctime(),
            changed_nanos: value.ctime_nsec() as u32,
        }),
    };
    observed::Record {
        query: observed::Query::Metadata {
            path: observed::Path::from_bytes(path.as_os_str().as_bytes(), budget).unwrap(),
            follow,
        },
        value,
    }
}

#[test]
fn explicit_include_walk_roundtrips_native_pruning_and_archive_projection() {
    let fixture = Fixture::new();
    for name in ["Cargo.toml", "lib.rs", ".hidden.rs", "README.md"] {
        fs::write(fixture.0.join(name), b"fixture").unwrap();
    }
    for name in ["target", "nested"] {
        fs::create_dir(fixture.0.join(name)).unwrap();
    }
    fs::write(fixture.0.join("nested/Cargo.toml"), b"nested").unwrap();
    fs::write(fixture.0.join("target/unread.rs"), b"unused").unwrap();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };

    // Capture the real source open/read/type/close calls for this fixture. The
    // two child directories are opened but never read, even with target/** in
    // includes; pruning must be recomputed by the actual Cargo continuation.
    let record = {
        let mut owner = live::traversal::Owner::new(&mut budget).unwrap();
        let mut capture = owner.begin(fixture.0.as_os_str().as_bytes(), true).unwrap();
        let mut root = capture.open(fixture.0.as_os_str().as_bytes()).unwrap();
        loop {
            match capture.next(&mut root).unwrap() {
                live::traversal::Read::Entry { name, kind } => {
                    if kind == Ok(observed::Kind::Directory) {
                        let child = fixture.0.join(std::ffi::OsStr::from_bytes(name));
                        let directory = capture.open(child.as_os_str().as_bytes()).unwrap();
                        capture
                            .close(directory, observed::traversal::Close::Pruned)
                            .unwrap();
                    }
                }
                live::traversal::Read::Eof => break,
                live::traversal::Read::Error { .. } => panic!("fixture read"),
            }
        }
        capture
            .close(root, observed::traversal::Close::Exhausted)
            .unwrap();
        capture.finish().unwrap()
    };
    let mut records = vec![
        metadata(&fixture.0, false, &mut budget),
        metadata(&fixture.0.join("target/Cargo.toml"), true, &mut budget),
        metadata(&fixture.0.join("nested/Cargo.toml"), true, &mut budget),
        record,
    ];
    records.sort_by(|a, b| a.query.cmp(&b.query));
    let image = stored::Stored::from_records(&records, &mut budget).unwrap();
    let encoded = image.encode(&mut budget).unwrap();
    let image = stored::Stored::decode(&encoded, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let includes = vec!["*.rs".to_owned(), "target/**".to_owned()];
    let excludes = vec!["*.rs".to_owned()];
    let manifest = fixture.0.join("Cargo.toml");
    let package = Package {
        manifest: &manifest,
        root: &fixture.0,
        include: &includes,
        exclude: &excludes,
        license_file: None,
        readme: None,
        builds: &[],
    };
    let mut session = Session::new(&mut host);
    let result = package.included(&mut session);
    let Ok(Some(files)) = result.unwrap_or_else(|_| panic!("session refused")) else {
        panic!("physical include selection failed");
    };
    let mut names = files
        .entries()
        .iter()
        .map(|entry| {
            assert!(!entry.under_symlink_dir);
            assert!(matches!(
                entry.kind,
                super::super::super::selection::Kind::File
            ));
            entry.path.file_name().unwrap().to_str().unwrap()
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, [".hidden.rs", "Cargo.toml", "lib.rs"]);
    assert!(files.warnings().is_empty());
    let projection = files.project(false, false, &mut session).unwrap();
    assert_eq!(projection.physical().count(), 3);
    session.finish(Ok(())).unwrap();
    host.finish().unwrap();
}

#[test]
fn repeated_open_occurrences_preserve_discarded_and_delivered_native_errors() {
    use observed::traversal::Close;
    let fixture = Fixture::new();
    let missing = fixture.0.join("missing");
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let record = {
        let mut owner = live::traversal::Owner::new(&mut budget).unwrap();
        let mut capture = owner.begin(fixture.0.as_os_str().as_bytes(), true).unwrap();
        let first = capture.open(missing.as_os_str().as_bytes()).unwrap();
        assert_eq!(first.opening_error(), Some(libc::ENOENT));
        capture.close(first, Close::Pruned).unwrap();
        let mut second = capture.open(missing.as_os_str().as_bytes()).unwrap();
        assert!(matches!(
            capture.next(&mut second).unwrap(),
            live::traversal::Read::Error {
                opening: true,
                code: libc::ENOENT
            }
        ));
        assert!(matches!(
            capture.next(&mut second).unwrap(),
            live::traversal::Read::Eof
        ));
        capture.close(second, Close::Exhausted).unwrap();
        let a = capture
            .handle(fixture.0.as_os_str().as_bytes())
            .unwrap()
            .unwrap();
        let b = capture
            .handle(fixture.0.as_os_str().as_bytes())
            .unwrap()
            .unwrap();
        assert!(capture.same(&a, &b).unwrap());
        capture.release(b).unwrap();
        capture.release(a).unwrap();
        capture.finish().unwrap()
    };
    let image = stored::Stored::from_records(&[record], &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut cursor = host
        .traversal(fixture.0.as_os_str().as_bytes(), true)
        .unwrap();
    let first = cursor
        .open(&mut host, missing.as_os_str().as_bytes())
        .unwrap();
    cursor.close(&mut host, first, Close::Pruned).unwrap();
    let mut second = cursor
        .open(&mut host, missing.as_os_str().as_bytes())
        .unwrap();
    assert!(matches!(
        cursor.next(&mut host, &mut second).unwrap(),
        host::traversal::Read::Error {
            opening: true,
            code: libc::ENOENT
        }
    ));
    assert!(matches!(
        cursor.next(&mut host, &mut second).unwrap(),
        host::traversal::Read::Eof
    ));
    cursor.close(&mut host, second, Close::Exhausted).unwrap();
    let a = cursor
        .handle(&mut host, fixture.0.as_os_str().as_bytes())
        .unwrap()
        .unwrap();
    let b = cursor
        .handle(&mut host, fixture.0.as_os_str().as_bytes())
        .unwrap()
        .unwrap();
    assert!(cursor.same(&mut host, &a, &b).unwrap());
    cursor.release(&mut host, b).unwrap();
    cursor.release(&mut host, a).unwrap();
    cursor.finish(&mut host).unwrap();
    host.finish().unwrap();
}

#[test]
fn dropped_or_refused_live_traversal_cannot_restart_the_same_owner() {
    let fixture = Fixture::new();
    for refuse in [false, true] {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut owner = live::traversal::Owner::new(&mut budget).unwrap();
        let mut capture = owner.begin(fixture.0.as_os_str().as_bytes(), true).unwrap();
        let directory = capture.open(fixture.0.as_os_str().as_bytes()).unwrap();
        if refuse {
            assert!(capture
                .probe(|budget| Ok(budget.graph.paths(usize::MAX)?))
                .is_err());
            assert!(capture
                .close(directory, observed::traversal::Close::Pruned)
                .is_err());
            assert!(capture.finish().is_err());
        } else {
            drop(capture);
        }
        assert!(owner.begin(fixture.0.as_os_str().as_bytes(), true).is_err());
    }
}

#[test]
fn eleventh_open_drains_oldest_and_delivers_buffered_entries_in_source_order() {
    use observed::traversal::{Close, Event, Next, Open, Transcript};
    let fixture = Fixture::new();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut paths = vec![fixture.0.clone()];
    for _ in 1..12 {
        let child = paths.last().unwrap().join("nest");
        fs::create_dir(&child).unwrap();
        paths.push(child);
    }
    let row = |name: &[u8], kind, raw_kind| {
        Next::Entry(
            observed::stream::Row::checked(name.to_vec(), raw_kind, Some(Ok(kind))).unwrap(),
        )
    };
    let mut events = Vec::new();
    for (depth, directory) in paths.iter().enumerate() {
        if depth != 0 {
            events.push(Event::Next {
                id: (depth - 1) as u64,
                result: row(b"nest", observed::Kind::Directory, 4),
            });
        }
        if depth >= 10 {
            let oldest = (depth - 10) as u64;
            events.push(Event::Next {
                id: oldest,
                result: row(b"tail.rs", observed::Kind::File, 8),
            });
            events.push(Event::Next {
                id: oldest,
                result: Next::Eof,
            });
            events.push(Event::Close {
                id: oldest,
                reason: Close::Drained,
            });
        }
        events.push(Event::Open {
            path: observed::Path::from_bytes(directory.as_os_str().as_bytes(), &mut budget)
                .unwrap(),
            result: Open::Started(rawr_directory::Mount::Known(false)),
        });
    }
    // The two oldest frames now hold owned entries. Their final delivery must
    // issue no further native event, after every deeper frame has completed.
    for id in (2..12).rev() {
        events.push(Event::Next {
            id,
            result: row(b"tail.rs", observed::Kind::File, 8),
        });
        events.push(Event::Next {
            id,
            result: Next::Eof,
        });
        events.push(Event::Close {
            id,
            reason: Close::Exhausted,
        });
    }
    let mut records = vec![metadata(&fixture.0, false, &mut budget)];
    for directory in paths.iter().skip(1) {
        records.push(metadata(&directory.join("Cargo.toml"), true, &mut budget));
    }
    records.push(observed::Record {
        query: observed::Query::Traversal {
            root: observed::Path::from_bytes(fixture.0.as_os_str().as_bytes(), &mut budget)
                .unwrap(),
            is_root: true,
            invocation: 0,
        },
        value: observed::Value::Traversal(Transcript { events }),
    });
    records.sort_by(|a, b| a.query.cmp(&b.query));
    let image = stored::Stored::from_records(&records, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let manifest = fixture.0.join("Cargo.toml");
    let include = ["**/*.rs".to_owned()];
    let package = Package {
        manifest: &manifest,
        root: &fixture.0,
        include: &include,
        exclude: &[],
        license_file: None,
        readme: None,
        builds: &[],
    };
    let mut session = Session::new(&mut host);
    let Ok(Some(files)) = package
        .included(&mut session)
        .unwrap_or_else(|_| panic!("refused"))
    else {
        panic!("selection failed");
    };
    let expected = paths
        .iter()
        .rev()
        .map(|path| path.join("tail.rs"))
        .collect::<Vec<_>>();
    assert_eq!(
        files
            .entries()
            .iter()
            .map(|entry| &entry.path)
            .collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>()
    );
    session.finish(Ok(())).unwrap();
    host.finish().unwrap();
}
