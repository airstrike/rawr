use super::*;
use crate::input::membership::{observed, stored, Budget};
use crate::{cargo, input, node};
use observed::stream::{Row, Transcript};

fn present() -> observed::Value {
    observed::Value::Metadata(observed::Metadata {
        kind: observed::Kind::File,
        device: 1,
        inode: 2,
        owner: 3,
        mode: 0o644,
        length: 1,
        modified_seconds: 0,
        modified_nanos: 0,
        changed_seconds: 0,
        changed_nanos: 0,
    })
}
fn image(rows: Option<Vec<Row>>, metadata: Vec<(&[u8], observed::Value)>) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records = Vec::new();
    if let Some(rows) = rows {
        records.push(observed::Record {
            query: observed::Query::DirectoryStream(
                observed::Path::from_bytes(b"/p/src/bin", &mut budget).unwrap(),
            ),
            value: observed::Value::DirectoryStream(Transcript::Started {
                mount: rawr_directory::Mount::Known(false),
                rows,
                end: observed::End::Eof,
            }),
        });
    }
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
fn run(
    image: &stored::Stored,
    infer: bool,
    name: &str,
    explicit: Option<&Path>,
    kind: Kind,
    edition: u16,
    legacy: Legacy,
) -> Result<Resolved, String> {
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
    let inferred = if infer {
        super::infer::directory(Path::new("/p"), Path::new("src/bin"), &mut session).unwrap()
    } else {
        storage::Buffer::new()
    };
    let result = resolve(
        name,
        explicit,
        inferred.as_slice(),
        kind,
        Path::new("/p"),
        edition,
        legacy,
        &mut session,
    );
    let result = session.finish(result).unwrap();
    host.finish().unwrap();
    result
}
fn file(name: &[u8]) -> Row {
    Row::checked(name.to_vec(), 8, Some(Ok(observed::Kind::File))).unwrap()
}
fn directory(name: &[u8]) -> Row {
    Row::checked(name.to_vec(), 4, Some(Ok(observed::Kind::Directory))).unwrap()
}
fn error(result: Result<Resolved, String>) -> String {
    match result {
        Ok(_) => panic!("expected diagnostic"),
        Err(error) => error,
    }
}

#[test]
fn explicit_and_single_inferred_paths_do_not_probe_filesystem_or_normalize() {
    let image = image(None, Vec::new());
    let result = run(
        &image,
        false,
        "x",
        Some(Path::new("src/../missing.rs")),
        Kind::Bin,
        2015,
        Legacy::Bin { has_lib: false },
    )
    .ok()
    .unwrap();
    assert_eq!(result.path(), Path::new("src/../missing.rs"));
    assert!(result.warning().is_none());
    let image = self::image(Some(vec![file(b"x.rs")]), Vec::new());
    let result = run(
        &image,
        true,
        "x",
        None,
        Kind::Bin,
        2015,
        Legacy::Bin { has_lib: false },
    )
    .ok()
    .unwrap();
    assert_eq!(result.path(), Path::new("src/bin/x.rs"));
    assert!(result.warning().is_none());
}

#[test]
fn ambiguity_uses_first_two_paths_without_missing_path_probes() {
    let image = image(
        Some(vec![directory(b"x"), file(b"x.rs"), file(b"x.rs")]),
        vec![(b"/p/src/bin/x/main.rs", present())],
    );
    assert_eq!(error(run(&image, true, "x", None, Kind::Bin, 2024, Legacy::Bin { has_lib: false })),
        "cannot infer path for `x` bin\nCargo doesn't know which to use because multiple target files found at `src/bin/x/main.rs` and `src/bin/x.rs`.");
}

#[test]
fn legacy_binary_short_circuits_and_has_lib_skips_name_path() {
    for (has_lib, values, expected) in [
        (
            false,
            vec![(b"/p/src/x.rs".as_slice(), present())],
            "src/x.rs",
        ),
        (
            true,
            vec![(b"/p/src/main.rs".as_slice(), present())],
            "src/main.rs",
        ),
        (
            false,
            vec![
                (b"/p/src/x.rs".as_slice(), observed::Value::Absent),
                (
                    b"/p/src/main.rs".as_slice(),
                    observed::Value::Error(nix::libc::EACCES),
                ),
                (b"/p/src/bin/main.rs".as_slice(), present()),
            ],
            "src/bin/main.rs",
        ),
    ] {
        let image = image(None, values);
        let result = run(
            &image,
            false,
            "x",
            None,
            Kind::Bin,
            2015,
            Legacy::Bin { has_lib },
        )
        .ok()
        .unwrap();
        assert_eq!(result.path(), Path::new(expected));
        assert_eq!(result.warning(), Some(format!("path `{expected}` was erroneously implicitly accepted for binary `x`,\nplease set bin.path in Cargo.toml").as_str()));
    }
}

#[test]
fn benchmark_legacy_is_name_and_edition_gated() {
    let image = image(None, vec![(b"/p/src/bench.rs", present())]);
    let result = run(
        &image,
        false,
        "bench",
        None,
        Kind::Bench,
        2015,
        Legacy::Bench,
    )
    .ok()
    .unwrap();
    assert_eq!(result.path(), Path::new("src/bench.rs"));
    assert_eq!(result.warning(), Some("path `src/bench.rs` was erroneously implicitly accepted for benchmark `bench`,\nplease set bench.path in Cargo.toml"));
    for (name, edition) in [("other", 2015), ("bench", 2024)] {
        let file = format!("/p/bench/{name}.rs");
        let subdir = format!("/p/bench/{name}/main.rs");
        let image = self::image(
            None,
            vec![
                (file.as_bytes(), observed::Value::Absent),
                (subdir.as_bytes(), observed::Value::Absent),
            ],
        );
        assert!(run(
            &image,
            false,
            name,
            None,
            Kind::Bench,
            edition,
            Legacy::Bench
        )
        .is_err());
    }
}

#[test]
fn missing_path_wrong_file_then_subdir_and_exact_suggestion_suffix() {
    let image = image(
        Some(vec![file(b"runner.rs")]),
        vec![(b"/p/src/bins/runer.rs", present())],
    );
    assert_eq!(error(run(&image, true, "runer", None, Kind::Bin, 2024, Legacy::None)),
        "can't find `runer` bin at default paths, but found a file at `src/bins/runer.rs`.\nPerhaps rename the file to `src/bin/runer.rs` for target auto-discovery, or specify bin.path if you want to use a non-default path.\n\nhelp: a bin with a similar name exists: `runner`");
    let image = self::image(
        None,
        vec![
            (b"/p/example/demo.rs", observed::Value::Absent),
            (b"/p/example/demo/main.rs", present()),
        ],
    );
    assert_eq!(error(run(&image, false, "demo", None, Kind::Example, 2024, Legacy::None)),
        "can't find `demo` example at default paths, but found a file at `example/demo/main.rs`.\nPerhaps rename the file to `examples/demo/main.rs` for target auto-discovery, or specify example.path if you want to use a non-default path.");
}

#[test]
fn no_wrong_paths_and_native_nul_before_host_query() {
    let image = image(
        None,
        vec![
            (b"/p/test/check.rs", observed::Value::Absent),
            (b"/p/test/check/main.rs", observed::Value::Absent),
        ],
    );
    assert_eq!(error(run(&image, false, "check", None, Kind::Test, 2024, Legacy::None)),
        "can't find `check` test at `tests/check.rs` or `tests/check/main.rs`. Please specify test.path if you want to use a non-default path.");
    let image = self::image(None, Vec::new());
    assert_eq!(error(run(&image, false, "bad\0name", None, Kind::Test, 2024, Legacy::None)),
        "can't find `bad\0name` test at `tests/bad\0name.rs` or `tests/bad\0name/main.rs`. Please specify test.path if you want to use a non-default path.");
}
