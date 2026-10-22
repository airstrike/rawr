use super::*;
use crate::input::membership::manifest::Stop;
use crate::input::membership::{observed, stored, Budget};
use crate::{input, node};

fn image(files: &[(&str, &str)]) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records: Vec<_> = files
        .iter()
        .map(|(path, text)| observed::Record {
            query: observed::Query::File(
                observed::Path::from_bytes(path.as_bytes(), &mut budget).unwrap(),
            ),
            value: observed::Value::File {
                bytes: text.as_bytes().to_vec(),
                end: observed::End::Eof,
            },
        })
        .collect();
    records.sort_by(|a, b| a.query.cmp(&b.query));
    stored::Stored::from_records(&records, &mut budget).unwrap()
}

fn run<T>(
    files: &[(&str, &str)],
    operation: impl for<'a, 'image> FnOnce(
        &mut Files<'a, 'image>,
        &mut Session<'_, host::Host<'image, '_>>,
        &cargo::Limits,
    ) -> Step<T>,
) -> Result<T, Error<input::membership::Error>> {
    let image = image(files);
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
    let mut paths = slots::Slots::new();
    let mut documents = slots::Slots::new();
    let mut files = Files::from_owners(paths.writer().unwrap(), documents.writer().unwrap());
    let mut session = Session::new(&mut host);
    let result = operation(&mut files, &mut session, &limits);
    let result = session.finish(result);
    if result.is_ok() {
        host.finish().unwrap();
    }
    result
}

#[test]
fn descendants_merge_before_siblings_then_parent_and_keep_exact_origins() {
    for why in [Why::Discovery, Why::Cli] {
        let result = run(
            &[
                ("/p/root.toml", "include=['a.toml','b.toml']\nanswer=3"),
                ("/p/a.toml", "include=['leaf.toml']\nanswer=1\nfirst=true"),
                ("/p/leaf.toml", "leaf='retained'\nanswer=0"),
                ("/p/b.toml", "answer=2\nsecond=true"),
            ],
            |files, session, limits| {
                files
                    .load(
                        Path::new("/p/root.toml"),
                        true,
                        why,
                        Path::new("/unused"),
                        limits,
                        session,
                    )
                    .map(|tree| tree.snapshot())
            },
        )
        .unwrap();
        let class = match why {
            Why::Discovery => "file",
            Why::Cli => "cli",
        };
        assert_eq!(
            result,
            serde_json::json!([
                "table",
                [class, "/p/root.toml"],
                [
                    ["answer", ["integer", [class, "/p/root.toml"], 3]],
                    ["first", ["boolean", [class, "/p/a.toml"], true]],
                    ["leaf", ["string", [class, "/p/leaf.toml"], "retained"]],
                    ["second", ["boolean", [class, "/p/b.toml"], true]],
                ]
            ])
        );
    }
}

#[test]
fn repeated_siblings_diamonds_and_component_aliases_are_cycles() {
    for (root, a, b) in [
        ("include=['a.toml','a.toml']", "", ""),
        (
            "include=['a.toml','b.toml']",
            "include=['leaf.toml']",
            "include=['leaf.toml']",
        ),
        ("include=['a.toml']", "include=['root.toml']", ""),
    ] {
        let result = run(
            &[
                ("/p/root.toml", root),
                ("/p/a.toml", a),
                ("/p/b.toml", b),
                ("/p/leaf.toml", ""),
            ],
            |files, session, limits| {
                files
                    .load(
                        Path::new("/p/root.toml"),
                        true,
                        Why::Discovery,
                        Path::new("/unused"),
                        limits,
                        session,
                    )
                    .map(|_| ())
            },
        );
        assert!(matches!(
            result,
            Err(Error::Invalid(Diagnostic {
                tag: Tag::ConfigCycle,
                ..
            }))
        ));
    }
    let result = run(&[("/p/a.toml", "")], |files, session, limits| {
        files.load(
            Path::new("/p/a.toml"),
            false,
            Why::Cli,
            Path::new("/unused"),
            limits,
            session,
        )?;
        files
            .load(
                Path::new("/p//./a.toml"),
                false,
                Why::Cli,
                Path::new("/unused"),
                limits,
                session,
            )
            .map(|_| ())
    });
    assert!(matches!(
        result,
        Err(Error::Invalid(Diagnostic {
            tag: Tag::ConfigCycle,
            ..
        }))
    ));
}

#[test]
fn complete_validation_and_child_merge_failures_precede_later_reads() {
    let result = run(
        &[("/p/root.toml", "include=['unobserved.toml',17]")],
        |files, session, limits| {
            files
                .load(
                    Path::new("/p/root.toml"),
                    true,
                    Why::Discovery,
                    Path::new("/unused"),
                    limits,
                    session,
                )
                .map(|_| ())
        },
    );
    assert!(matches!(
        result,
        Err(Error::Invalid(Diagnostic {
            tag: Tag::ConfigInclude,
            ..
        }))
    ));
    let result = run(
        &[
            (
                "/p/root.toml",
                "include=['a.toml','b.toml','unobserved.toml']",
            ),
            ("/p/a.toml", "value=1"),
            ("/p/b.toml", "value={child=1}"),
        ],
        |files, session, limits| {
            files
                .load(
                    Path::new("/p/root.toml"),
                    true,
                    Why::Discovery,
                    Path::new("/unused"),
                    limits,
                    session,
                )
                .map(|_| ())
        },
    );
    assert!(matches!(
        result,
        Err(Error::Invalid(Diagnostic {
            tag: Tag::ConfigMerge,
            ..
        }))
    ));
}

#[test]
fn disabled_includes_retain_the_complete_value_and_nul_read_is_terminal() {
    let value = run(
        &[("/p/root.toml", "include=[17]")],
        |files, session, limits| {
            files
                .load(
                    Path::new("/p/root.toml"),
                    false,
                    Why::Discovery,
                    Path::new("/unused"),
                    limits,
                    session,
                )
                .map(|tree| tree.snapshot())
        },
    )
    .unwrap();
    assert_eq!(value[2][0][0], "include");
    let result = run(&[], |files, session, limits| {
        assert!(matches!(
            files.load(
                Path::new("/p/nul\0.toml"),
                true,
                Why::Discovery,
                Path::new("/unused"),
                limits,
                session
            ),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.items(1), Err(Stop::Refused)));
        Ok(())
    });
    assert!(matches!(result, Err(Error::ConfigPathNul)));
}

#[test]
fn nested_include_frames_and_owners_drop_on_a_small_native_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let owned: Vec<_> = (0..256)
                .map(|index| {
                    (
                        format!("/p/{index}.toml"),
                        if index == 255 {
                            "answer=255".into()
                        } else {
                            format!("include=['{}.toml']\nanswer={index}", index + 1)
                        },
                    )
                })
                .collect();
            let files: Vec<_> = owned
                .iter()
                .map(|(path, text)| (path.as_str(), text.as_str()))
                .collect();
            let value = run(&files, |files, session, limits| {
                files
                    .load(
                        Path::new("/p/0.toml"),
                        true,
                        Why::Discovery,
                        Path::new("/unused"),
                        limits,
                        session,
                    )
                    .map(|tree| tree.snapshot())
            })
            .unwrap();
            assert_eq!(value[2][0][1][2], 0);
            // Failure occurs with every ancestor frame and owner still retained.
            let mut owned = owned;
            owned[255].1 = "include=[17]".into();
            let files: Vec<_> = owned
                .iter()
                .map(|(path, text)| (path.as_str(), text.as_str()))
                .collect();
            assert!(matches!(
                run(&files, |files, session, limits| files
                    .load(
                        Path::new("/p/0.toml"),
                        true,
                        Why::Discovery,
                        Path::new("/unused"),
                        limits,
                        session
                    )
                    .map(|_| ())),
                Err(Error::Invalid(Diagnostic {
                    tag: Tag::ConfigInclude,
                    ..
                }))
            ));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn original_item_and_byte_floors_cover_file_read_frames_and_merge() {
    use crate::input::membership::manifest::tests::one_short;
    let image = image(&[
        ("/p/root.toml", "include=['a.toml']\nroot=true"),
        ("/p/a.toml", "answer=1"),
    ]);
    // `latched` injects a genuine original-account failure; accounting
    // targets alone never refuse.
    let run = |items, bytes, latched| {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        let mut host = host::Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .unwrap();
        if latched {
            host.budget().allocation.refuse();
        }
        let mut paths = slots::Slots::new();
        let mut documents = slots::Slots::new();
        let mut files = Files::from_owners(paths.writer().unwrap(), documents.writer().unwrap());
        let mut session = Session::new(&mut host);
        let result = files
            .load(
                Path::new("/p/root.toml"),
                true,
                Why::Discovery,
                Path::new("/unused"),
                &limits,
                &mut session,
            )
            .map(|_| ());
        if result.is_err() {
            let allocated = storage::ALLOCATIONS.with(|count| count.get());
            assert!(matches!(
                files.load(
                    Path::new("/after-failure.toml"),
                    true,
                    Why::Discovery,
                    Path::new("/unused"),
                    &limits,
                    &mut session
                ),
                Err(Stop::Refused)
            ));
            assert_eq!(allocated, storage::ALLOCATIONS.with(|count| count.get()));
        }
        let result = session.finish(result);
        if result.is_ok() {
            host.finish().unwrap();
        } else {
            drop(host);
        }
        assert_eq!(allocation.check().is_ok(), !latched);
        (result, allocation.usage(), allocation.exceedances())
    };
    let (result, usage, _) = run(usize::MAX, usize::MAX, false);
    result.unwrap();
    let (result, exact, exceeded) = run(usage.items, usage.bytes, false);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(!exceeded.is_exceeded());
    // One short of an aggregate target admits and records that axis.
    let (result, exact, exceeded) = run(usage.items - 1, usage.bytes, false);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_ITEMS_RESOURCE,
        usage.items
    ));
    let (result, exact, exceeded) = run(usage.items, usage.bytes - 1, false);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(one_short(
        &exceeded,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes
    ));
    // A genuinely refused account stops every later load without allocating.
    assert!(run(usage.items, usage.bytes, true).0.is_err());
}

#[test]
fn inline_configuration_uses_cwd_and_preserves_inline_and_file_cli_origins() {
    let value = run(
        &[("/work/a.toml", "child=true\nanswer=1")],
        |files, session, limits| {
            let document = session.parse_text("include=['a.toml']\nanswer=9", limits)?;
            let document = files.documents.push(document, session)?;
            let values = Values::from_document(document, Definition::Cli(None), session)?;
            let tree = Tree::from_values(values, session)?;
            files
                .includes(tree, Why::Cli, Path::new("/work"), limits, session)
                .map(|tree| tree.snapshot())
        },
    )
    .unwrap();
    assert_eq!(
        value,
        serde_json::json!([
            "table",
            ["cli", null],
            [
                ["answer", ["integer", ["cli", null], 9]],
                ["child", ["boolean", ["cli", "/work/a.toml"], true]],
            ]
        ])
    );
}
