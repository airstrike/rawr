use super::*;
use crate::input::membership::{
    observed::stream::{Row, Transcript},
    stored, Budget,
};
use crate::{cargo, input, node};

fn row(name: &[u8], kind: u8, inspected: Option<Result<observed::Kind, i32>>) -> Row {
    Row::checked(name.to_vec(), kind, inspected).unwrap()
}
fn present() -> observed::Value {
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
}
fn image(
    directory: &[u8],
    transcript: Transcript,
    metadata: Vec<(&[u8], observed::Value)>,
) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records = vec![observed::Record {
        query: observed::Query::DirectoryStream(
            observed::Path::from_bytes(directory, &mut budget).unwrap(),
        ),
        value: observed::Value::DirectoryStream(transcript),
    }];
    for (path, value) in metadata {
        records.push(observed::Record {
            query: observed::Query::Metadata {
                path: observed::Path::from_bytes(path, &mut budget).unwrap(),
                follow: true,
            },
            value,
        });
    }
    records.sort_by(|a, b| a.query.cmp(&b.query));
    stored::Stored::from_records(&records, &mut budget).unwrap()
}
fn started(rows: Vec<Row>) -> Transcript {
    Transcript::Started {
        mount: rawr_directory::Mount::Known(false),
        rows,
        end: observed::End::Error(nix::libc::EIO),
    }
}
fn run(image: &stored::Stored, bins: bool, root: &Path, relative: &Path) -> Vec<(String, PathBuf)> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut host = host::Host::from_image(
        image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let mut session = Session::new(&mut host);
    let result = if bins {
        super::bins(root, "pkg-name", &mut session)
    } else {
        directory(root, relative, &mut session)
    };
    let output = session.finish(result).unwrap();
    host.finish().unwrap();
    output.into_iter().map(Item::into_parts).collect()
}

#[test]
fn bins_preserve_main_encounter_order_skips_and_reached_type_errors() {
    let image = image(
        b"/p/src/bin",
        started(vec![
            row(b"z.rs", 8, Some(Ok(observed::Kind::File))),
            row(b".hidden.rs", 0, None),
            row(b"bad\xff.rs", 0, None),
            row(b"sub", 4, Some(Ok(observed::Kind::Directory))),
            row(b"a.rs", 0, Some(Err(nix::libc::ENOENT))),
            row(b"z.rs", 10, Some(Ok(observed::Kind::Symlink))),
            row(b"skip.txt", 8, Some(Ok(observed::Kind::File))),
            row(b"empty", 4, Some(Ok(observed::Kind::Directory))),
        ]),
        vec![
            (b"/p/src/main.rs", present()),
            (b"/p/src/bin/sub/main.rs", present()),
            (b"/p/src/bin/empty/main.rs", observed::Value::Absent),
        ],
    );
    assert_eq!(
        run(&image, true, Path::new("/p"), Path::new("")),
        vec![
            ("pkg-name".into(), "src/main.rs".into()),
            ("z".into(), "src/bin/z.rs".into()),
            ("sub".into(), "src/bin/sub/main.rs".into()),
            ("a".into(), "src/bin/a.rs".into()),
            ("z".into(), "src/bin/z.rs".into()),
        ]
    );
}

#[test]
fn spelling_extension_and_absolute_strip_prefix_fallback() {
    let image = image(
        b"/elsewhere",
        started(vec![
            row(b"two.part.rs", 8, Some(Ok(observed::Kind::File))),
            row(b"upper.RS", 8, Some(Ok(observed::Kind::File))),
            row(b"rs", 8, Some(Ok(observed::Kind::File))),
            row(b"dir.rs", 4, Some(Ok(observed::Kind::Directory))),
        ]),
        vec![(b"/elsewhere/dir.rs/main.rs", present())],
    );
    assert_eq!(
        run(&image, false, Path::new("/p"), Path::new("/elsewhere")),
        vec![
            ("two.part".into(), "/elsewhere/two.part.rs".into()),
            ("dir.rs".into(), "/elsewhere/dir.rs/main.rs".into()),
        ]
    );
}

#[test]
fn native_open_error_is_empty_and_main_probe_is_still_required_first() {
    let image = image(
        b"/p/src/bin",
        Transcript::OpenFailed(nix::libc::EACCES),
        vec![(b"/p/src/main.rs", observed::Value::Absent)],
    );
    assert!(run(&image, true, Path::new("/p"), Path::new("")).is_empty());
    let image = self::image(
        b"/p/src/bin",
        Transcript::OpenFailed(nix::libc::EACCES),
        Vec::new(),
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
    let result = super::bins(Path::new("/p"), "p", &mut session);
    assert!(session.finish(result).is_err());
    assert!(host.finish().is_err());
}

#[test]
fn missing_subdirectory_probe_refuses_instead_of_becoming_absent() {
    let image = image(
        b"/p/examples",
        started(vec![row(b"sub", 4, Some(Ok(observed::Kind::Directory)))]),
        Vec::new(),
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
    let result = directory(Path::new("/p"), Path::new("examples"), &mut session);
    assert!(session.finish(result).is_err());
    assert!(host.finish().is_err());
}
