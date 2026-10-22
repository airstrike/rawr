use super::*;
use crate::input::membership::manifest::{load::features, storage::slots::Slots, Error, Source};
use crate::input::membership::{observed, stored, Budget, Error as InputError};
use crate::{input, node};

fn image(files: &[(&[u8], &[u8], observed::End)]) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records: Vec<_> = files
        .iter()
        .map(|(path, bytes, end)| observed::Record {
            query: observed::Query::File(observed::Path::from_bytes(path, &mut budget).unwrap()),
            value: observed::Value::File {
                bytes: bytes.to_vec(),
                end: *end,
            },
        })
        .collect();
    records.sort_by(|a, b| a.query.cmp(&b.query));
    stored::Stored::from_records(&records, &mut budget).unwrap()
}
fn context() -> Context<'static> {
    Context {
        features: features::Context {
            allow_features: None,
            nightly_features_allowed: true,
            channel: "nightly",
            is_local: true,
        },
        cargo_lints: false,
        profiles: crate::input::membership::manifest::load::ProfileContext::fixture(),
    }
}

#[test]
fn repeated_reads_retain_decoded_text_and_publish_each_checked_root() {
    use crate::input::membership::manifest::schema::{manifest, package, primitive, workspace};
    let image = image(&[
        (
            b"/one/script.rs",
            b"---cargo\n[workspace.package]\ndescription=\"first\\ntext\"\n---\nfn main() {}",
            observed::End::Eof,
        ),
        (b"/two/Cargo.toml", b"[workspace]", observed::End::Eof),
    ]);
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
    let mut bodies = Slots::new();
    let mut documents = Slots::new();
    let mut originals = Slots::new();
    let mut reader = Reader {
        bodies: bodies.writer().unwrap(),
        documents: documents.writer().unwrap(),
        originals: originals.writer().unwrap(),
    };
    let mut roots = Roots::new();
    let first = reader
        .prepare(
            Path::new("/one/script.rs"),
            true,
            context(),
            &mut roots,
            &limits,
            &mut session,
        )
        .unwrap();
    let second = reader
        .prepare(
            Path::new("/two/Cargo.toml"),
            false,
            context(),
            &mut roots,
            &limits,
            &mut session,
        )
        .unwrap();
    assert!(first.embedded);
    assert!(!second.embedded);
    assert!(roots
        .get(Path::new("/one"), &mut session)
        .unwrap()
        .is_some());
    assert!(roots
        .get(Path::new("/two"), &mut session)
        .unwrap()
        .is_some());
    let Some(manifest::Value::Workspace(ws)) = first.original.get(manifest::Field::Workspace)
    else {
        panic!("workspace");
    };
    let Some(workspace::Value::Package(package)) = ws.get(workspace::Field::Package) else {
        panic!("defaults");
    };
    let Some(package::Value::Local(primitive::Value::String(text))) =
        package.get(package::Field::Description)
    else {
        panic!("description");
    };
    assert_eq!(text.as_str(), "first\ntext");
    session.finish(Ok(())).unwrap();
    host.finish().unwrap();
}

fn run(
    path: &Path,
    bytes: &[u8],
    end: observed::End,
    script: bool,
    items: usize,
    allocated: usize,
    paths: usize,
) -> (
    Result<(), Error<InputError>>,
    input::Usage,
    crate::exceedance::Record,
    crate::exceedance::Record,
) {
    let image = image(&[(path.as_os_str().as_bytes(), bytes, end)]);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH - paths)
        .unwrap();
    let mut allocation = input::Allocation::with_limits(items, allocated, usize::MAX);
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
        Err(error) => {
            return (
                Err(Error::Host(error)),
                allocation.usage(),
                allocation.exceedances(),
                graph.exceedances(),
            )
        }
    };
    let mut session = Session::new(&mut host);
    let mut bodies = Slots::new();
    let mut documents = Slots::new();
    let mut originals = Slots::new();
    let mut reader = Reader {
        bodies: bodies.writer().unwrap(),
        documents: documents.writer().unwrap(),
        originals: originals.writer().unwrap(),
    };
    let mut roots = Roots::new();
    let result = reader
        .prepare(path, script, context(), &mut roots, &limits, &mut session)
        .map(|_| ());
    let result = session.finish(result);
    drop(host);
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (
        result,
        allocation.usage(),
        allocation.exceedances(),
        graph.exceedances(),
    )
}

#[test]
fn read_and_encoding_errors_precede_script_gate_then_expansion_and_toml() {
    let run = |bytes: &[u8], end, script| {
        run(
            Path::new("/script.rs"),
            bytes,
            end,
            script,
            usize::MAX,
            usize::MAX,
            1,
        )
        .0
    };
    assert!(matches!(
        run(b"\xff", observed::End::Error(5), false),
        Err(Error::Host(InputError::Io(5)))
    ));
    assert!(matches!(
        run(b"\xff", observed::End::Eof, false),
        Err(Error::Host(InputError::ManifestEncoding(_)))
    ));
    assert!(matches!(
        run(b"---\n-", observed::End::Eof, false),
        Err(Error::Invalid(Diagnostic {
            tag: Tag::ScriptRequired,
            ..
        }))
    ));
    assert!(matches!(
        run(b"---\n-", observed::End::Eof, true),
        Err(Error::Source {
            cause: Source::FrontmatterPartialAdvance { .. },
            ..
        })
    ));
    assert!(matches!(
        run(b"---\nnot valid TOML\n---", observed::End::Eof, true),
        Err(Error::Host(InputError::Document(_)))
    ));
}

#[test]
fn path_extension_controls_only_the_embedded_gate_and_expansion() {
    for path in ["/script.rs", "/script", "/.hidden"] {
        assert!(matches!(
            run(
                Path::new(path),
                b"fn main() {}",
                observed::End::Eof,
                false,
                usize::MAX,
                usize::MAX,
                1
            )
            .0,
            Err(Error::Invalid(Diagnostic {
                tag: Tag::ScriptRequired,
                ..
            }))
        ));
        assert!(run(
            Path::new(path),
            b"fn main() {}",
            observed::End::Eof,
            true,
            usize::MAX,
            usize::MAX,
            1
        )
        .0
        .is_ok());
    }
    for path in ["/Cargo.toml", "/script.RS", "/script."] {
        assert!(run(
            Path::new(path),
            b"[workspace]",
            observed::End::Eof,
            false,
            usize::MAX,
            usize::MAX,
            1
        )
        .0
        .is_ok());
    }
}

#[test]
fn complete_entry_uses_original_item_byte_and_graph_limits() {
    use crate::input::membership::manifest::tests::one_short;
    let path = Path::new("/root/script.rs");
    let bytes = b"---cargo\n[package]\nname='example'\n---\nfn main() {}";
    let (result, usage, _, _) = run(
        path,
        bytes,
        observed::End::Eof,
        true,
        usize::MAX,
        usize::MAX,
        1,
    );
    result.unwrap();
    let (result, exact, exceeded, graph) = run(
        path,
        bytes,
        observed::End::Eof,
        true,
        usage.items,
        usage.bytes,
        1,
    );
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(!exceeded.is_exceeded() && !graph.is_exceeded());
    // One short of an aggregate target admits and records that axis.
    let (result, exact, exceeded, _) = run(
        path,
        bytes,
        observed::End::Eof,
        true,
        usage.items - 1,
        usage.bytes,
        1,
    );
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_ITEMS_RESOURCE,
        usage.items
    ));
    let (result, exact, exceeded, _) = run(
        path,
        bytes,
        observed::End::Eof,
        true,
        usage.items,
        usage.bytes - 1,
        1,
    );
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes
    ));
    // The one file read past the exhausted path target is recorded, not refused.
    let (result, exact, exceeded, graph) = run(
        path,
        bytes,
        observed::End::Eof,
        true,
        usage.items,
        usage.bytes,
        0,
    );
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(!exceeded.is_exceeded());
    assert!(one_short(
        &graph,
        node::PATHS_RESOURCE,
        node::MAX_PATH_CHECKS_PER_GRAPH + 1
    ));
}

#[test]
fn source_failure_prevents_later_read_and_root_publication() {
    let image = image(&[
        (b"/bad.rs", b"---\n-", observed::End::Eof),
        (b"/later/Cargo.toml", b"[workspace]", observed::End::Eof),
    ]);
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
    let mut bodies = Slots::new();
    let mut documents = Slots::new();
    let mut originals = Slots::new();
    let mut reader = Reader {
        bodies: bodies.writer().unwrap(),
        documents: documents.writer().unwrap(),
        originals: originals.writer().unwrap(),
    };
    let mut roots = Roots::new();
    assert!(reader
        .prepare(
            Path::new("/bad.rs"),
            true,
            context(),
            &mut roots,
            &limits,
            &mut session
        )
        .is_err());
    assert!(reader
        .prepare(
            Path::new("/later/Cargo.toml"),
            false,
            context(),
            &mut roots,
            &limits,
            &mut session
        )
        .is_err());
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Source {
            cause: Source::FrontmatterPartialAdvance { .. },
            ..
        })
    ));
    // The second file was neither consumed nor used to publish a root.
    assert!(matches!(host.finish(), Err(InputError::Unconsumed)));
}

#[test]
fn sourced_reads_keep_each_manifest_identity_and_retained_original() {
    let image = image(&[
        (b"/one/Cargo.toml", b"[workspace]", observed::End::Eof),
        (b"/two/Cargo.toml", b"[workspace]", observed::End::Eof),
    ]);
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
    let mut history = source::History::new();
    let mut sources = history.open(&mut session).unwrap();
    let mut bodies = Slots::new();
    let mut documents = Slots::new();
    let mut originals = Slots::new();
    let mut reader = Reader {
        bodies: bodies.writer().unwrap(),
        documents: documents.writer().unwrap(),
        originals: originals.writer().unwrap(),
    };
    let mut roots = Roots::new();
    assert!(reader
        .begin(
            Path::new("relative/Cargo.toml"),
            false,
            context(),
            &mut sources,
            &mut roots,
            &limits,
            &mut session
        )
        .unwrap()
        .is_err());
    let first = reader
        .begin(
            Path::new("/one/Cargo.toml"),
            false,
            context(),
            &mut sources,
            &mut roots,
            &limits,
            &mut session,
        )
        .unwrap()
        .unwrap();
    let second = reader
        .begin(
            Path::new("/two/Cargo.toml"),
            false,
            context(),
            &mut sources,
            &mut roots,
            &limits,
            &mut session,
        )
        .unwrap()
        .unwrap();
    assert!(first.source != second.source);
    for (value, path, url) in [
        (first, "/one/Cargo.toml", "file:///one"),
        (second, "/two/Cargo.toml", "file:///two"),
    ] {
        assert_eq!(value.preparing.path, Path::new(path));
        assert!(value
            .preparing
            .original
            .get(crate::input::membership::manifest::schema::manifest::Field::Workspace)
            .is_some());
        assert_eq!(
            sources
                .get(value.source, &mut session)
                .unwrap()
                .url(&mut session)
                .unwrap()
                .as_str(&mut session)
                .unwrap(),
            url
        );
    }
    session.finish(Ok(())).unwrap();
    host.finish().unwrap();
}
