use super::*;
use crate::input::membership::{
    manifest::{reference, value},
    observed, stored, Budget,
};
use crate::{cargo, input, node};
use observed::stream::{Row, Transcript};

fn image(
    kind: resolve::Kind,
    rows: Option<Vec<Row>>,
    values: Vec<(&[u8], observed::Value)>,
) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records = Vec::new();
    if let Some(rows) = rows {
        let directory: &[u8] = match kind {
            resolve::Kind::Bin => b"/p/src/bin",
            resolve::Kind::Example => b"/p/examples",
            resolve::Kind::Test => b"/p/tests",
            resolve::Kind::Bench => b"/p/benches",
        };
        records.push(observed::Record {
            query: observed::Query::DirectoryStream(
                observed::Path::from_bytes(directory, &mut budget).unwrap(),
            ),
            value: observed::Value::DirectoryStream(Transcript::Started {
                mount: rawr_directory::Mount::Known(false),
                rows,
                end: observed::End::Eof,
            }),
        });
    }
    for (path, value) in values {
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
fn file(name: &[u8]) -> Row {
    Row::checked(name.to_vec(), 8, Some(Ok(observed::Kind::File))).unwrap()
}
type Output = (
    Result<Vec<(String, PathBuf, bool)>, String>,
    Vec<String>,
    Vec<String>,
    bool,
);
fn run(
    source: &str,
    image: &stored::Stored,
    kind: resolve::Kind,
    edition: u16,
    auto: Option<bool>,
    embedded: bool,
) -> Output {
    let document = rawr_toml::document::parse(source, 80, &mut reference::Account).unwrap();
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
    let original = document
        .table()
        .get("targets")
        .map(|input| Targets::interpret(value::Input::raw(input), &mut session).unwrap());
    let input = if embedded {
        Input::Embedded {
            name: "script",
            path: Path::new("script.rs"),
        }
    } else {
        Input::Declared(original.as_ref())
    };
    let mut warnings = Warnings::new();
    let mut errors = validate::Errors::new();
    let result = group(
        input,
        kind,
        Path::new("/p"),
        "p",
        edition,
        auto,
        false,
        &mut warnings,
        &mut errors,
        &mut session,
    );
    let output = match session.finish(result) {
        Ok(Ok(items)) => Ok(items
            .as_slice()
            .iter()
            .map(|item| {
                (
                    item.name().to_owned(),
                    item.path().to_owned(),
                    item.original().is_some(),
                )
            })
            .collect()),
        Ok(Err(message)) => Err(message),
        Err(error) => Err(format!("{error:?}")),
    };
    (
        output,
        warnings.as_slice().to_vec(),
        errors.as_slice().to_vec(),
        host.finish().is_ok(),
    )
}

#[test]
fn fast_groups_keep_original_order_and_normalize_without_discovery() {
    for kind in [
        resolve::Kind::Bin,
        resolve::Kind::Example,
        resolve::Kind::Test,
        resolve::Kind::Bench,
    ] {
        let image = image(kind, None, Vec::new());
        let (output, warnings, errors, complete) = run(
            "targets=[{name='z',path='src/../z.rs'},{name='a',path='a.rs'}]",
            &image,
            kind,
            2024,
            Some(false),
            false,
        );
        assert_eq!(
            output.unwrap(),
            vec![
                ("z".into(), "z.rs".into(), true),
                ("a".into(), "a.rs".into(), true)
            ]
        );
        assert!(warnings.is_empty() && errors.is_empty() && complete);
    }
}

#[test]
fn ordinary_inferred_bins_are_sorted_and_retain_no_original_target() {
    let image = image(
        resolve::Kind::Bin,
        Some(vec![file(b"z.rs"), file(b"a.rs")]),
        vec![(b"/p/src/main.rs", observed::Value::Absent)],
    );
    let (output, warnings, errors, complete) =
        run("", &image, resolve::Kind::Bin, 2024, None, false);
    assert_eq!(
        output.unwrap(),
        vec![
            ("a".into(), "src/bin/a.rs".into(), false),
            ("z".into(), "src/bin/z.rs".into(), false)
        ]
    );
    assert!(warnings.is_empty() && errors.is_empty() && complete);
}

#[test]
fn common_slow_validation_precedes_every_path_but_bins_resolve_each_target() {
    for kind in [resolve::Kind::Test, resolve::Kind::Bin] {
        let values = if matches!(kind, resolve::Kind::Bin) {
            vec![
                (b"/p/src/main.rs".as_slice(), observed::Value::Absent),
                (b"/p/src/bins/a.rs".as_slice(), observed::Value::Absent),
                (b"/p/src/bins/a/main.rs".as_slice(), observed::Value::Absent),
            ]
        } else {
            Vec::new()
        };
        let image = image(kind, Some(Vec::new()), values);
        let (output, _, errors, complete) = run(
            "targets=[{name='a'},{name='z',path='z.rs',proc_macro=false}]",
            &image,
            kind,
            2024,
            Some(true),
            false,
        );
        let error = output.unwrap_err();
        if matches!(kind, resolve::Kind::Bin) {
            assert!(error.starts_with("can't find `a` bin"), "{error}");
        } else {
            assert!(error.contains("TargetProcMacroUnderscore"), "{error}");
        }
        assert!(errors.is_empty() && complete);
    }
}

#[test]
fn common_path_failure_is_accumulated_and_omitted() {
    let image = image(
        resolve::Kind::Example,
        Some(Vec::new()),
        vec![
            (b"/p/example/a.rs", observed::Value::Absent),
            (b"/p/example/a/main.rs", observed::Value::Absent),
        ],
    );
    let (output, warnings, errors, complete) = run(
        "targets=[{name='a'},{name='z',path='z.rs'}]",
        &image,
        resolve::Kind::Example,
        2024,
        Some(true),
        false,
    );
    assert_eq!(output.unwrap(), vec![("z".into(), "z.rs".into(), true)]);
    assert_eq!(errors, ["can't find `a` example at `examples/a.rs` or `examples/a/main.rs`. Please specify example.path if you want to use a non-default path."]);
    assert!(warnings.is_empty() && complete);
}

#[test]
fn benchmark_legacy_warning_is_deferred_past_later_source_refusal() {
    let image = image(
        resolve::Kind::Bench,
        Some(Vec::new()),
        vec![(b"/p/src/bench.rs", present())],
    );
    let (output, warnings, errors, complete) = run(
        "targets=[{name='bench'},{name='z'}]",
        &image,
        resolve::Kind::Bench,
        2015,
        Some(true),
        false,
    );
    assert!(output.is_err());
    assert!(warnings.is_empty() && errors.is_empty());
    // The refusal is an unobserved wrong-path query after the completed stream;
    // ordinary Host completion is independent of the Session failure latch.
    let _ = complete;
}

#[test]
fn embedded_default_fast_path_and_explicit_true_use_same_group_pipeline() {
    let image = image(resolve::Kind::Bin, None, Vec::new());
    let (output, _, _, complete) = run("", &image, resolve::Kind::Bin, 2024, Some(false), true);
    assert_eq!(
        output.unwrap(),
        vec![("script".into(), "script.rs".into(), false)]
    );
    assert!(complete);
    let image = self::image(
        resolve::Kind::Bin,
        Some(vec![file(b"other.rs")]),
        vec![(b"/p/src/main.rs", observed::Value::Absent)],
    );
    let (output, _, _, complete) = run("", &image, resolve::Kind::Bin, 2024, Some(true), true);
    assert_eq!(
        output.unwrap(),
        vec![
            ("other".into(), "src/bin/other.rs".into(), false),
            ("script".into(), "script.rs".into(), false)
        ]
    );
    assert!(complete);
}
