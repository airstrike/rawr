use super::super::files::Files;
use super::*;
use crate::input::membership::manifest::{storage::slots, Stop};
use crate::input::membership::observed::discovery::{
    After, Attempt, Exists, Identity, NativeError, Transcript,
};
use crate::input::membership::{observed, stored, Budget};
use crate::{cargo, input, node};

fn with_budget<T>(run: impl FnOnce(&mut Budget<'_>) -> T) -> T {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    run(&mut Budget {
        graph: &graph,
        allocation: &mut allocation,
    })
}

fn acquired(inode: u64) -> Attempt {
    Attempt::Acquired(Identity { device: 7, inode })
}

fn transcript(filename: Filename, warn: bool, selected: Selection) -> Transcript {
    let (legacy, after) = match selected {
        Selection::Legacy => (
            acquired(1),
            if warn {
                After::Second(acquired(2))
            } else {
                After::None
            },
        ),
        Selection::Toml => (
            Attempt::OpenFailed(NativeError::Os(13)),
            After::Fallback(Exists::Present),
        ),
        Selection::None => (
            Attempt::OpenFailed(NativeError::Os(13)),
            After::Fallback(Exists::Failed(NativeError::Os(2))),
        ),
    };
    Transcript::checked(filename, warn, legacy, after).expect("source branch")
}

fn discovery(directory: &[u8], value: Transcript) -> observed::Record {
    observed::Record {
        query: observed::Query::Discovery {
            directory: with_budget(|budget| {
                observed::Path::from_bytes(directory, budget).expect("absolute path")
            }),
            filename: value.filename(),
            warn: value.warn(),
        },
        value: observed::Value::Discovery(value),
    }
}

fn file(path: &[u8], value: observed::Value) -> observed::Record {
    observed::Record {
        query: observed::Query::File(with_budget(|budget| {
            observed::Path::from_bytes(path, budget).expect("absolute path")
        })),
        value,
    }
}

fn content(text: &str) -> observed::Value {
    observed::Value::File {
        bytes: text.as_bytes().to_vec(),
        end: observed::End::Eof,
    }
}

fn image(mut records: Vec<observed::Record>) -> stored::Stored {
    records.sort_by(|a, b| a.query.cmp(&b.query));
    with_budget(|budget| {
        let image = stored::Stored::from_records(&records, budget).expect("checked image");
        let bytes = image.encode(budget).expect("codec");
        stored::Stored::decode(&bytes, budget).expect("decoded image")
    })
}

#[test]
fn selects_exact_native_joined_path_for_each_family_mode_and_branch() {
    for directory in [b"/".as_slice(), b"/p/", b"/p//./\xff"] {
        for filename in [Filename::Config, Filename::Credentials] {
            for warn in [false, true] {
                for selected in [Selection::Legacy, Selection::Toml, Selection::None] {
                    let image = image(vec![discovery(
                        directory,
                        transcript(filename, warn, selected),
                    )]);
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
                    .expect("host");
                    let mut session = Session::new(&mut host);
                    let directory = Path::new(std::ffi::OsStr::from_bytes(directory));
                    let result = select(directory, filename, warn, &mut session);
                    let actual = session.finish(result).expect("selection");
                    let expected = match selected {
                        Selection::Legacy => Some(directory.join(filename.legacy())),
                        Selection::Toml => Some(directory.join(filename.toml())),
                        Selection::None => None,
                    };
                    assert_eq!(
                        actual.as_ref().map(|path| path.as_os_str().as_bytes()),
                        expected.as_ref().map(|path| path.as_os_str().as_bytes())
                    );
                    host.finish().expect("exact discovery consumed");
                }
            }
        }
    }
}

#[test]
fn local_nul_produces_none_after_both_joins_without_any_host_query() {
    let image = image(Vec::new());
    for directory in [b"/p\0".as_slice(), b"relative\0"] {
        for filename in [Filename::Config, Filename::Credentials] {
            for warn in [false, true] {
                let graph = node::GraphBudget::new();
                graph
                    .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
                    .expect("exhaust native queries");
                let limits = cargo::Limits::standard();
                let mut allocation = input::Allocation::new(&limits);
                let mut host = host::Host::from_image(
                    &image,
                    Budget {
                        graph: &graph,
                        allocation: &mut allocation,
                    },
                )
                .expect("empty host");
                let mut session = Session::new(&mut host);
                let result = select(
                    Path::new(std::ffi::OsStr::from_bytes(directory)),
                    filename,
                    warn,
                    &mut session,
                );
                assert!(session
                    .finish(result)
                    .expect("local InvalidInput branch")
                    .is_none());
                host.finish().expect("no observation required");
                assert_eq!(allocation.usage().items, 2);
                assert_eq!(
                    allocation.usage().bytes,
                    directory.len() * 2 + 2 + filename.legacy().len() + filename.toml().len()
                );
            }
        }
    }
    // The second source join crossing the string target is recorded
    // before NUL selects None; both spellings stay owned and nothing latches.
    let directory = Path::new(std::ffi::OsStr::from_bytes(b"/p\0"));
    let graph = node::GraphBudget::new();
    let legacy_bytes = b"/p\0/config".len();
    let toml_bytes = b"/p\0/config.toml".len();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, legacy_bytes);
    let mut host = host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("host");
    let mut session = Session::new(&mut host);
    let result = select(directory, Filename::Config, false, &mut session);
    assert!(session
        .finish(result)
        .expect("local InvalidInput branch")
        .is_none());
    host.finish().expect("no host query");
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    assert_eq!(allocation.usage().items, 2);
    assert_eq!(allocation.usage().bytes, legacy_bytes + toml_bytes);
    let record = allocation.exceedances();
    let mut axes = record.axes();
    let axis = axes
        .next()
        .copied()
        .expect("the second join's string target crossing is recorded");
    assert!(axes.next().is_none(), "{record:?}");
    assert_eq!(axis.name, "membership decoded TOML string bytes");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (legacy_bytes as u64, toml_bytes as u64, toml_bytes as u64)
    );
    // The second source join can still refuse before NUL would select None:
    // its byte charge overflows the account's checked arithmetic, a genuine
    // failure that latches the account.
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    let mut host = host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("host");
    host.budget()
        .allocation
        .bytes(usize::MAX - legacy_bytes)
        .expect("the first join fills the account exactly");
    let mut session = Session::new(&mut host);
    let result = select(directory, Filename::Config, false, &mut session);
    assert!(matches!(session.finish(result), Err(Error::Admission(_))));
    host.finish().expect("no host query");
    assert!(allocation.check().is_err(), "overflow latches the account");
    assert_eq!(allocation.usage().items, 2);
    assert_eq!(allocation.usage().bytes, usize::MAX);
    assert!(!allocation.exceedances().is_exceeded());
}

#[test]
fn missing_or_relative_domain_gap_latches_without_cargo_rejection_or_absence() {
    let image = image(vec![discovery(
        b"/known",
        transcript(Filename::Config, true, Selection::Legacy),
    )]);
    for (directory, filename, warn) in [
        (Path::new("relative"), Filename::Config, true),
        (Path::new("/unknown"), Filename::Config, true),
        (Path::new("/known"), Filename::Credentials, true),
        (Path::new("/known"), Filename::Config, false),
        (Path::new("/known/./"), Filename::Config, true),
    ] {
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
        .expect("host");
        let mut session = Session::new(&mut host);
        assert!(matches!(
            select(directory, filename, warn, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            session.discovery(b"/known", Filename::Config, true),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            session.finish(Ok(())),
            Err(Error::Host(InputError::Unobserved))
        ));
        assert!(matches!(host.finish(), Err(InputError::Unconsumed)));
    }
}

#[test]
fn selection_preserves_original_allocation_and_graph_floors() {
    use crate::input::membership::manifest::tests::admits_one_short;
    let image = image(vec![discovery(
        b"/known",
        transcript(Filename::Config, false, Selection::Legacy),
    )]);
    let run = |items, bytes, available| {
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH - available)
            .expect("prior work");
        let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
        allocation.items(7).expect("prior items");
        allocation.bytes(31).expect("prior bytes");
        let mut host = host::Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .expect("host fits fixture limits");
        let mut session = Session::new(&mut host);
        let result = select(Path::new("/known"), Filename::Config, false, &mut session);
        let result = session.finish(result);
        let success = result.is_ok();
        if available == 0 {
            // The one selection path check past the target is recorded.
            assert!(result.is_ok());
            let exceeded = graph.exceedances();
            let axis = exceeded
                .axis(node::PATHS_RESOURCE)
                .expect("exhausted path target recorded");
            assert_eq!(
                axis.first_requested,
                node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
            );
            assert_eq!(axis.high_water, node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1);
        } else {
            assert!(!graph.exceedances().is_exceeded());
        }
        if success {
            host.finish().expect("consumed");
        } else {
            assert!(matches!(host.finish(), Err(InputError::Unconsumed)));
        }
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        (success, allocation.usage(), allocation.exceedances())
    };
    let (success, usage, _) = run(usize::MAX, usize::MAX, 1);
    assert!(success);
    assert_eq!(usage.items, 7 + 1 + 2 + 1);
    assert_eq!(
        usage.bytes,
        31 + 1 + b"/known/config".len() + b"/known/config.toml".len() + b"/known".len()
    );
    assert_eq!(
        run(usage.items, usage.bytes, 1),
        (true, usage, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        run(usage.items - 1, usage.bytes, 1),
        usage,
        input::INPUT_ITEMS_RESOURCE,
        usage.items,
    );
    admits_one_short(
        run(usage.items, usage.bytes - 1, 1),
        usage,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes,
    );
    assert_eq!(
        run(usage.items, usage.bytes, 0),
        (true, usage, crate::exceedance::Record::new())
    );
}

#[test]
fn selected_file_and_includes_load_without_unselected_read_evidence() {
    for (selected, root_path) in [
        (Selection::Legacy, "/p/config"),
        (Selection::Toml, "/p/config.toml"),
    ] {
        let image = image(vec![
            discovery(b"/p", transcript(Filename::Config, true, selected)),
            file(
                root_path.as_bytes(),
                content("include=['child.toml']\nanswer=2"),
            ),
            file(b"/p/child.toml", content("answer=1\nchild=true")),
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
        .expect("host");
        let mut paths = slots::Slots::new();
        let mut documents = slots::Slots::new();
        let mut files = Files::from_owners(
            paths.writer().expect("path owner"),
            documents.writer().expect("document owner"),
        );
        let mut session = Session::new(&mut host);
        let result = files
            .load_discovered(
                Path::new("/p"),
                Filename::Config,
                true,
                true,
                Path::new("/unused"),
                &limits,
                &mut session,
            )
            .map(|tree| tree.expect("selected file").snapshot());
        let result = session.finish(result).expect("selected load");
        assert_eq!(
            result,
            serde_json::json!([
                "table",
                ["file", root_path],
                [
                    ["answer", ["integer", ["file", root_path], 2]],
                    ["child", ["boolean", ["file", "/p/child.toml"], true]],
                ]
            ])
        );
        host.finish()
            .expect("only selected body and child evidence required");
    }
}

#[test]
fn selection_absence_never_loads_and_selected_read_failure_stays_first() {
    for selected in [Selection::None, Selection::Toml] {
        let mut records = vec![discovery(
            b"/p",
            transcript(Filename::Config, false, selected),
        )];
        if selected == Selection::Toml {
            records.push(file(b"/p/config.toml", observed::Value::Error(5)));
        }
        let image = image(records);
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
        .expect("host");
        let mut paths = slots::Slots::new();
        let mut documents = slots::Slots::new();
        let mut files = Files::from_owners(
            paths.writer().expect("paths"),
            documents.writer().expect("documents"),
        );
        let mut session = Session::new(&mut host);
        let result = files.load_discovered(
            Path::new("/p"),
            Filename::Config,
            false,
            true,
            Path::new("/unused"),
            &limits,
            &mut session,
        );
        if selected == Selection::None {
            assert!(session.finish(result).expect("absent discovery").is_none());
        } else {
            assert!(matches!(result, Err(Stop::Refused)));
            assert!(matches!(
                session.discovery(b"/different", Filename::Config, false),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                session.finish(Ok(())),
                Err(Error::Host(InputError::Io(5)))
            ));
        }
        host.finish().expect("all reached records consumed");
    }
}
