use super::{archive, host, observed, rejection, stored, Budget, Error};
use crate::{cargo, input, node};

fn path(bytes: &[u8], budget: &mut Budget<'_>) -> observed::Path {
    observed::Path::from_bytes(bytes, budget).expect("valid absolute fixture path")
}

fn metadata() -> observed::Metadata {
    observed::Metadata {
        kind: observed::Kind::File,
        device: 3,
        inode: 7,
        owner: 42,
        mode: 0o100644,
        length: 1,
        modified_seconds: -1,
        modified_nanos: 999_999_999,
        changed_seconds: 8,
        changed_nanos: 1,
    }
}

fn image(budget: &mut Budget<'_>) -> stored::Stored {
    let records = vec![
        observed::Record {
            query: observed::Query::Metadata {
                path: path(b"/pkg/absent", budget),
                follow: true,
            },
            value: observed::Value::Absent,
        },
        observed::Record {
            query: observed::Query::Metadata {
                path: path(b"/pkg/error", budget),
                follow: true,
            },
            value: observed::Value::Error(13),
        },
        observed::Record {
            query: observed::Query::Metadata {
                path: path(b"/pkg/file", budget),
                follow: true,
            },
            value: observed::Value::Metadata(metadata()),
        },
        observed::Record {
            query: observed::Query::Read {
                path: path(b"/pkg/file", budget),
                offset: 0,
                length: 1,
            },
            value: observed::Value::Bytes(vec![b'x']),
        },
        observed::Record {
            query: observed::Query::Link(path(b"/pkg/link", budget)),
            value: observed::Value::Link(b"../external".to_vec()),
        },
        observed::Record {
            query: observed::Query::Directory(path(b"/pkg", budget)),
            value: observed::Value::Directory {
                entries: vec![observed::Entry {
                    name: vec![0xff],
                    kind: observed::Kind::Unknown,
                    inode: 9,
                }],
                end: observed::End::Eof,
            },
        },
        observed::Record {
            query: observed::Query::Environment(b"EMPTY".to_vec()),
            value: observed::Value::Environment(Some(Vec::new())),
        },
        observed::Record {
            query: observed::Query::Environment(b"UNSET".to_vec()),
            value: observed::Value::Environment(None),
        },
        observed::Record {
            query: observed::Query::Identity,
            value: observed::Value::Identity { effective_uid: 42 },
        },
    ];
    stored::Stored::from_records(&records, budget).expect("canonical fixture image")
}

#[test]
fn codec_round_trip_preserves_native_paths_errors_and_complete_frontiers() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = image(&mut budget);
    let bytes = image.encode(&mut budget).expect("image encodes");
    let decoded = stored::Stored::decode(&bytes, &mut budget).expect("image decodes");
    assert_eq!(image, decoded);
    assert_eq!(
        bytes,
        decoded.encode(&mut budget).expect("canonical reencode")
    );
    let mut host = host::Host::from_image(&decoded, budget).expect("image host");
    for record in image.records() {
        assert_eq!(
            host.observe(&record.query).expect("exact observation"),
            &record.value
        );
    }
    assert_eq!(
        host.finish().expect("all observations consumed").image(),
        &decoded
    );
}

#[test]
fn host_distinguishes_absence_error_unknown_query_and_unconsumed_record() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = image(&mut budget);
    let mut host = host::Host::from_image(&image, budget).expect("image host");
    assert_eq!(
        host.metadata(b"/pkg/absent", true)
            .expect("recorded absence"),
        None
    );
    assert!(matches!(
        host.metadata(b"/pkg/error", true),
        Err(Error::Io(13))
    ));
    assert!(matches!(
        host.metadata(b"/pkg/new", true),
        Err(Error::Unobserved)
    ));
    assert!(matches!(
        host.metadata(b"/pkg/file", false),
        Err(Error::Unobserved)
    ));
    assert!(matches!(host.finish(), Err(Error::Unconsumed)));
}

#[test]
fn malformed_truncated_old_and_noncanonical_images_reject() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = image(&mut budget);
    let bytes = image.encode(&mut budget).expect("image encodes");
    for length in 0..bytes.len() {
        let mut allocation = input::Allocation::new(&limits);
        let graph = node::GraphBudget::new();
        assert!(stored::Stored::decode(
            &bytes[..length],
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation
            }
        )
        .is_err());
    }
    for offset in [
        0,
        b"rawr-package-observations\0".len(),
        b"rawr-package-observations\0\x01".len(),
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 0xff;
        assert!(stored::Stored::decode(&changed, &mut budget).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(stored::Stored::decode(&trailing, &mut budget).is_err());
    let duplicate = vec![
        observed::Record {
            query: observed::Query::Identity,
            value: observed::Value::Identity { effective_uid: 0 },
        },
        observed::Record {
            query: observed::Query::Identity,
            value: observed::Value::Identity { effective_uid: 0 },
        },
    ];
    assert!(stored::Stored::from_records(&duplicate, &mut budget).is_err());
}

#[test]
fn directory_omission_and_reordering_do_not_become_complete_frontiers() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    for names in [
        vec![b"b".as_slice(), b"a".as_slice()],
        vec![b"a".as_slice(), b"a".as_slice()],
        vec![b"..".as_slice()],
    ] {
        let entries = names
            .into_iter()
            .map(|name| observed::Entry {
                name: name.to_vec(),
                kind: observed::Kind::File,
                inode: 1,
            })
            .collect();
        let record = observed::Record {
            query: observed::Query::Directory(path(b"/pkg", &mut budget)),
            value: observed::Value::Directory {
                entries,
                end: observed::End::Eof,
            },
        };
        assert!(stored::Stored::from_records(&[record], &mut budget).is_err());
    }
    let record = observed::Record {
        query: observed::Query::Directory(path(b"/pkg", &mut budget)),
        value: observed::Value::Directory {
            entries: Vec::new(),
            end: observed::End::Eof,
        },
    };
    let image =
        stored::Stored::from_records(&[record], &mut budget).expect("empty complete directory");
    let mut bytes = image.encode(&mut budget).expect("encode directory");
    bytes.pop();
    assert!(stored::Stored::decode(&bytes, &mut budget).is_err());
}

#[test]
fn ownership_and_graph_refusal_precede_targeted_host_and_decoder_work() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let image = image(&mut Budget {
        graph: &graph,
        allocation: &mut allocation,
    });
    let bytes = image
        .encode(&mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        })
        .expect("encode fixture");
    let decode = |items, bytes_limit| {
        let graph = node::GraphBudget::new();
        let mut allocation =
            input::Allocation::with_limits(items, bytes_limit, limits.string_bytes);
        let decoded = stored::Stored::decode(
            &bytes,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )?;
        Ok::<_, Error>((decoded, allocation.usage(), allocation.exceedances()))
    };
    let (_, exact, _) = decode(usize::MAX, usize::MAX).expect("measure exact decode ownership");
    // The exact account records nothing; one less on either axis still
    // decodes and records an exceedance whose high water is the exact total.
    let (_, _, exceeded) = decode(exact.items, exact.bytes).expect("exact ownership");
    assert!(!exceeded.is_exceeded());
    let (_, usage, exceeded) = decode(exact.items - 1, exact.bytes).expect("items target");
    assert_eq!(usage, exact);
    let axis = exceeded
        .axis(input::INPUT_ITEMS_RESOURCE)
        .expect("items axis");
    assert_eq!(
        (axis.target, axis.high_water),
        (exact.items as u64 - 1, exact.items as u64)
    );
    assert!(exceeded.axis(input::INPUT_BYTES_RESOURCE).is_none());
    let (_, usage, exceeded) = decode(exact.items, exact.bytes - 1).expect("bytes target");
    assert_eq!(usage, exact);
    let axis = exceeded
        .axis(input::INPUT_BYTES_RESOURCE)
        .expect("bytes axis");
    assert_eq!(
        (axis.target, axis.high_water),
        (exact.bytes as u64 - 1, exact.bytes as u64)
    );
    assert!(exceeded.axis(input::INPUT_ITEMS_RESOURCE).is_none());

    // The host bitmap crossing its items target is recorded, not refused.
    super::OWNED_VECTORS.with(|count| count.set(0));
    let mut exceeding =
        input::Allocation::with_limits(image.records().len() - 1, usize::MAX, limits.string_bytes);
    host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut exceeding,
        },
    )
    .expect("host bitmap past its items target");
    assert_eq!(super::OWNED_VECTORS.with(std::cell::Cell::get), 1);
    assert_eq!(
        exceeding
            .exceedances()
            .axis(input::INPUT_ITEMS_RESOURCE)
            .expect("items axis")
            .high_water,
        image.records().len() as u64
    );
    // A genuinely latched account does no host work at all.
    super::OWNED_VECTORS.with(|count| count.set(0));
    let mut refused = input::Allocation::new(&limits);
    refused.refuse();
    assert!(matches!(
        host::Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut refused
            }
        ),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert_eq!(super::OWNED_VECTORS.with(std::cell::Cell::get), 0);

    // Graph decode past its target still decodes and records the axis.
    let graph = node::GraphBudget::new();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .expect("consume graph decode target");
    let mut allocation = input::Allocation::new(&limits);
    stored::Stored::decode(
        &bytes,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("decode past the graph target");
    let axis = graph
        .exceedances()
        .axis(node::DECODED_RESOURCE)
        .copied()
        .expect("decoded axis");
    assert_eq!(axis.target, node::MAX_DECODED_BYTES_PER_GRAPH);
    assert_eq!(
        axis.high_water,
        node::MAX_DECODED_BYTES_PER_GRAPH + bytes.len() as u64
    );
    assert_eq!(allocation.usage(), exact);
    // Checked-arithmetic overflow of the graph account is a genuine failure
    // that precedes every owned decoder vector.
    let graph = node::GraphBudget::new();
    graph
        .decode(usize::MAX)
        .expect("saturate the graph decode account");
    let mut allocation = input::Allocation::new(&limits);
    super::OWNED_VECTORS.with(|count| count.set(0));
    assert!(matches!(
        stored::Stored::decode(
            &bytes,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation
            }
        ),
        Err(Error::Budget(node::Error::Allocation))
    ));
    assert_eq!(super::OWNED_VECTORS.with(std::cell::Cell::get), 0);
    assert_eq!(allocation.usage().bytes, 0);
}

#[test]
fn borrowed_retention_matches_decode_ownership_with_prior_use_and_exact_limits() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = image(&mut budget);
    let bytes = image.encode(&mut budget).expect("encode fixture");
    let retain = |items, bytes_limit, decode| {
        let graph = node::GraphBudget::new();
        let mut allocation =
            input::Allocation::with_limits(items, bytes_limit, limits.string_bytes);
        allocation.items(7)?;
        allocation.bytes(19)?;
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let retained = if decode {
            stored::Stored::decode(&bytes, &mut budget)?
        } else {
            stored::Stored::from_records(image.records(), &mut budget)?
        };
        Ok::<_, Error>((retained, allocation.usage(), allocation.exceedances()))
    };
    let (retained, exact, _) =
        retain(usize::MAX, usize::MAX, false).expect("measure nested retention");
    assert_eq!(retained, image);
    let (decoded, decoded_usage, _) =
        retain(usize::MAX, usize::MAX, true).expect("measure decoder ownership");
    assert_eq!(decoded, image);
    assert_eq!(exact, decoded_usage);
    for decode in [false, true] {
        // Exact ownership records nothing; one less on an axis still
        // retains the image and records that axis with the exact high water.
        let (_, _, exceeded) = retain(exact.items, exact.bytes, decode).expect("exact");
        assert!(!exceeded.is_exceeded());
        let (_, usage, exceeded) =
            retain(exact.items - 1, exact.bytes, decode).expect("items target");
        assert_eq!(usage, exact);
        let axis = exceeded
            .axis(input::INPUT_ITEMS_RESOURCE)
            .expect("items axis");
        assert_eq!(axis.high_water, exact.items as u64);
        assert!(exceeded.axis(input::INPUT_BYTES_RESOURCE).is_none());
        let (_, usage, exceeded) =
            retain(exact.items, exact.bytes - 1, decode).expect("bytes target");
        assert_eq!(usage, exact);
        let axis = exceeded
            .axis(input::INPUT_BYTES_RESOURCE)
            .expect("bytes axis");
        assert_eq!(axis.high_water, exact.bytes as u64);
        assert!(exceeded.axis(input::INPUT_ITEMS_RESOURCE).is_none());
    }

    // A genuinely latched account owns nothing at all.
    super::OWNED_VECTORS.with(|count| count.set(0));
    super::OWNED_BYTES.with(|count| count.set(0));
    let mut allocation = input::Allocation::new(&limits);
    allocation.refuse();
    assert!(matches!(
        stored::Stored::from_records(
            image.records(),
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            }
        ),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert_eq!(super::OWNED_VECTORS.with(std::cell::Cell::get), 0);
    assert_eq!(super::OWNED_BYTES.with(std::cell::Cell::get), 0);
    assert_eq!(allocation.usage().bytes, 0);
}

#[test]
fn borrowed_retention_records_each_oversized_string_and_still_owns_it() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    // (query, value, buffers owned before the oversized string, native path)
    // An oversized native path or owned string crosses the input string
    // target, which is recorded on its axis with the truthful length; the
    // record is still retained, every buffer is owned, nothing latches.
    let cases = [
        (
            observed::Query::Metadata {
                path: path(b"/long", &mut budget),
                follow: true,
            },
            observed::Value::Absent,
            0,
            true,
        ),
        (
            observed::Query::Read {
                path: path(b"/long", &mut budget),
                offset: 0,
                length: 0,
            },
            observed::Value::Bytes(Vec::new()),
            0,
            true,
        ),
        (
            observed::Query::Link(path(b"/long", &mut budget)),
            observed::Value::Link(b"a".to_vec()),
            0,
            true,
        ),
        (
            observed::Query::Directory(path(b"/long", &mut budget)),
            observed::Value::Directory {
                entries: Vec::new(),
                end: observed::End::Eof,
            },
            0,
            true,
        ),
        (
            observed::Query::Environment(b"ABCDE".to_vec()),
            observed::Value::Environment(None),
            0,
            false,
        ),
        (
            observed::Query::Environment(b"E".to_vec()),
            observed::Value::Environment(Some(b"12345".to_vec())),
            1,
            false,
        ),
        (
            observed::Query::Link(path(b"/ln", &mut budget)),
            observed::Value::Link(b"12345".to_vec()),
            1,
            false,
        ),
        (
            observed::Query::Directory(path(b"/dir", &mut budget)),
            observed::Value::Directory {
                entries: vec![observed::Entry {
                    name: b"12345".to_vec(),
                    kind: observed::Kind::File,
                    inode: 1,
                }],
                end: observed::End::Eof,
            },
            1,
            false,
        ),
    ];
    for (query, value, preceding_buffers, native_path) in cases {
        let records = [observed::Record { query, value }];
        let image = stored::Stored::from_records(&records, &mut budget)
            .expect("fixture admits five-byte strings");
        let bytes = image.encode(&mut budget).expect("encode string fixture");
        for decode in [false, true] {
            let mut exact = input::Allocation::with_limits(usize::MAX, usize::MAX, 5);
            let mut exact_budget = Budget {
                graph: &graph,
                allocation: &mut exact,
            };
            super::OWNED_BYTES.with(|count| count.set(0));
            let admitted = if decode {
                stored::Stored::decode(&bytes, &mut exact_budget)
            } else {
                stored::Stored::from_records(&records, &mut exact_budget)
            };
            assert_eq!(
                admitted.expect("exact string bound admits the record"),
                image
            );
            assert!(!exact.exceedances().is_exceeded());
            let exact_buffers = super::OWNED_BYTES.with(std::cell::Cell::get);
            let graph = node::GraphBudget::new();
            let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, 4);
            let mut budget = Budget {
                graph: &graph,
                allocation: &mut allocation,
            };
            super::OWNED_BYTES.with(|count| count.set(0));
            let result = if decode {
                stored::Stored::decode(&bytes, &mut budget)
            } else {
                stored::Stored::from_records(&records, &mut budget)
            };
            if native_path {
                assert_eq!(result.expect("oversized native path is recorded"), image);
                assert_eq!(super::OWNED_BYTES.with(std::cell::Cell::get), exact_buffers);
                let axis = allocation
                    .exceedances()
                    .axis("membership native path bytes")
                    .copied()
                    .expect("string axis");
                assert_eq!(
                    (axis.target, axis.first_requested, axis.high_water),
                    (4, 5, 5)
                );
                assert!(allocation.items(0).is_ok(), "an exceedance never latches");
            } else {
                assert_eq!(result.expect("oversized owned string is recorded"), image);
                let owned = super::OWNED_BYTES.with(std::cell::Cell::get);
                assert_eq!(owned, exact_buffers);
                assert!(
                    owned > preceding_buffers,
                    "the oversized string is owned after the preceding buffers"
                );
                let axis = allocation
                    .exceedances()
                    .axis("membership owned string bytes")
                    .copied()
                    .expect("owned string axis");
                assert_eq!(
                    (axis.target, axis.first_requested, axis.high_water),
                    (4, 5, 5)
                );
                assert!(allocation.items(0).is_ok(), "an exceedance never latches");
            }
        }
    }
}

#[test]
fn borrowed_read_payload_uses_aggregate_bytes_without_a_string_limit() {
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, 4);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let records = [observed::Record {
        query: observed::Query::Read {
            path: path(b"/raw", &mut budget),
            offset: 9,
            length: 8,
        },
        value: observed::Value::Bytes(vec![0xff, 0, 1, 2, 3, 4, 5, 6]),
    }];
    let image = stored::Stored::from_records(&records, &mut budget)
        .expect("read bytes may exceed the string limit");
    assert_eq!(image.records(), records.as_slice());
    let bytes = image.encode(&mut budget).expect("encode raw read");
    assert_eq!(
        stored::Stored::decode(&bytes, &mut budget).expect("decode uses the same raw byte policy"),
        image
    );
}

#[test]
fn borrowed_directory_container_exceedance_precedes_entry_name_retention() {
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, 4);
    let records = [observed::Record {
        query: observed::Query::Directory(path(
            b"/dir",
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )),
        value: observed::Value::Directory {
            entries: vec![
                observed::Entry {
                    name: b"a".to_vec(),
                    kind: observed::Kind::File,
                    inode: 1,
                },
                observed::Entry {
                    name: b"b".to_vec(),
                    kind: observed::Kind::File,
                    inode: 2,
                },
            ],
            end: observed::End::Eof,
        },
    }];
    // The entry container's reservation is the charge that crosses the
    // three-item target (record vector, path, then two entries at once), before
    // either entry name is owned; the names then raise the high water to six.
    let mut allocation = input::Allocation::with_limits(3, usize::MAX, 4);
    super::OWNED_VECTORS.with(|count| count.set(0));
    super::OWNED_BYTES.with(|count| count.set(0));
    stored::Stored::from_records(
        &records,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("directory retained past its items target");
    assert_eq!(super::OWNED_VECTORS.with(std::cell::Cell::get), 2);
    assert_eq!(super::OWNED_BYTES.with(std::cell::Cell::get), 3);
    let axis = allocation
        .exceedances()
        .axis(input::INPUT_ITEMS_RESOURCE)
        .copied()
        .expect("items axis");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (3, 4, 6)
    );
    assert_eq!(allocation.usage().items, 6);
    // A genuinely latched account owns neither container nor name.
    let mut refused = input::Allocation::with_limits(3, usize::MAX, 4);
    refused.refuse();
    super::OWNED_VECTORS.with(|count| count.set(0));
    super::OWNED_BYTES.with(|count| count.set(0));
    assert!(matches!(
        stored::Stored::from_records(
            &records,
            &mut Budget {
                graph: &graph,
                allocation: &mut refused,
            }
        ),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert_eq!(super::OWNED_VECTORS.with(std::cell::Cell::get), 0);
    assert_eq!(super::OWNED_BYTES.with(std::cell::Cell::get), 0);
}

fn archive_image(paths: &[(&[u8], observed::Value)], budget: &mut Budget<'_>) -> stored::Stored {
    let mut records = Vec::new();
    for (name, value) in paths {
        let value = match value {
            observed::Value::Metadata(value) => observed::Value::Metadata(*value),
            observed::Value::Absent => observed::Value::Absent,
            _ => panic!("fixture expects metadata or absence"),
        };
        records.push(observed::Record {
            query: observed::Query::Metadata {
                path: path(name, budget),
                follow: true,
            },
            value,
        });
    }
    stored::Stored::from_records(&records, budget).expect("archive metadata image")
}

#[test]
fn archive_preserves_manifest_origin_and_external_collision_precedence() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = archive_image(
        &[
            (b"/outside/LICENSE", observed::Value::Metadata(metadata())),
            (b"/outside/README", observed::Value::Metadata(metadata())),
        ],
        &mut budget,
    );
    let mut host = host::Host::from_image(&image, budget).expect("archive host");
    let projection = archive::Projection::from_inputs(
        archive::Inputs {
            root: b"/pkg",
            manifest: b"/pkg/Cargo.toml",
            selected: &[
                b"/pkg/cARGo.ToML",
                b"/pkg/LICENSE",
                b"/pkg/Cargo.lock",
                b"/pkg/src/lib.rs",
            ],
            license: Some(b"../outside/LICENSE"),
            readme: Some(b"../outside/README"),
            build_scripts: &[],
            include_lockfile: true,
            vcs: true,
        },
        &mut host,
    )
    .expect("physical projection");
    let rows: Vec<_> = projection.physical().collect();
    assert_eq!(
        rows,
        vec![
            (b"Cargo.toml.orig".as_slice(), b"/pkg/Cargo.toml".as_slice()),
            (b"LICENSE".as_slice(), b"/pkg/LICENSE".as_slice()),
            (b"README".as_slice(), b"/outside/README".as_slice()),
            (b"src/lib.rs".as_slice(), b"/pkg/src/lib.rs".as_slice()),
        ]
    );
    host.finish()
        .expect("both outside inputs were probed despite collision");
}

#[test]
fn archive_collision_uses_path_equality_and_preserves_selected_spelling_and_origin() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = archive_image(
        &[(b"/pkg/a/README", observed::Value::Metadata(metadata()))],
        &mut budget,
    );
    let mut host = host::Host::from_image(&image, budget).expect("archive host");
    let archive = archive::Projection::from_inputs(
        archive::Inputs {
            root: b"/pkg",
            manifest: b"/pkg/Cargo.toml",
            selected: &[b"/pkg/a/./README"],
            license: None,
            readme: Some(b"a/README"),
            build_scripts: &[],
            include_lockfile: false,
            vcs: false,
        },
        &mut host,
    )
    .expect("native Path equality suppresses the declared duplicate");
    assert_eq!(archive.rows().len(), 1);
    assert_eq!(
        archive.physical().collect::<Vec<_>>(),
        vec![(b"a/./README".as_slice(), b"/pkg/a/./README".as_slice())]
    );
    host.finish().expect("declared readme was still probed");
}

#[test]
fn archive_reserved_physical_and_nonunicode_sources_reject() {
    let limits = cargo::Limits::standard();
    for selected in [
        b"/pkg/Cargo.toml.orig".as_slice(),
        b"/pkg/.cargo_vcs_info.json",
        b"/pkg/bad:name",
        b"/pkg/\xff",
        b"/outside/file",
    ] {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let image = stored::Stored::from_records(&[], &mut budget).expect("empty image");
        let mut host = host::Host::from_image(&image, budget).expect("archive host");
        assert!(archive::Projection::from_inputs(
            archive::Inputs {
                root: b"/pkg",
                manifest: b"/pkg/Cargo.toml",
                selected: &[selected],
                license: None,
                readme: None,
                build_scripts: &[],
                include_lockfile: true,
                vcs: true,
            },
            &mut host
        )
        .is_err());
    }
}

#[test]
fn archive_probes_both_missing_fields_and_does_not_add_excluded_build_script() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = archive_image(
        &[
            (b"/pkg/LICENSE", observed::Value::Absent),
            (b"/pkg/README", observed::Value::Absent),
        ],
        &mut budget,
    );
    let mut host = host::Host::from_image(&image, budget).expect("archive host");
    let rejected = archive::Projection::from_inputs(
        archive::Inputs {
            root: b"/pkg",
            manifest: b"/pkg/Cargo.toml",
            selected: &[b"/pkg/Cargo.toml"],
            license: Some(b"LICENSE"),
            readme: Some(b"README"),
            build_scripts: &[],
            include_lockfile: true,
            vcs: true,
        },
        &mut host,
    );
    assert!(matches!(
        rejected,
        Err(Error::Source(archive::Rejection::Missing {
            license: true,
            readme: true
        }))
    ));
    host.finish()
        .expect("both missing-file probes are retained");

    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = archive_image(
        &[(b"/pkg/build.rs", observed::Value::Metadata(metadata()))],
        &mut budget,
    );
    let mut host = host::Host::from_image(&image, budget).expect("archive host");
    let projected = archive::Projection::from_inputs(
        archive::Inputs {
            root: b"/pkg",
            manifest: b"/pkg/Cargo.toml",
            selected: &[b"/pkg/Cargo.toml"],
            license: None,
            readme: None,
            build_scripts: &[b"build.rs"],
            include_lockfile: true,
            vcs: true,
        },
        &mut host,
    )
    .expect("existing excluded build script is valid");
    assert_eq!(
        projected.physical().collect::<Vec<_>>(),
        vec![(b"Cargo.toml.orig".as_slice(), b"/pkg/Cargo.toml".as_slice())]
    );
    host.finish().expect("build-script existence was observed");
}

#[test]
fn status_classification_preserves_shared_sources_and_names_residual_trust() {
    for operation in [
        rejection::Operation::SourceDiscovery,
        rejection::Operation::SourceIndexAndIgnore,
        rejection::Operation::SourceTraversal,
        rejection::Operation::ArchiveAssembly,
    ] {
        let classification = rejection::classify(operation);
        assert_eq!(classification.treatment, rejection::Treatment::Retained);
        assert!(!classification.source.is_empty());
        assert!(!classification.argument.is_empty());
    }
    assert_eq!(
        rejection::classify(rejection::Operation::ArbitraryFilter).treatment,
        rejection::Treatment::ResidualProcessTrust
    );
    for operation in [
        rejection::Operation::VcsSymlinkWarning,
        rejection::Operation::VcsStatusConfiguration,
        rejection::Operation::VcsStatusCollection,
        rejection::Operation::VcsSubmoduleStatus,
        rejection::Operation::VcsOutsideRootStatus,
        rejection::Operation::VcsDirtyIntersection,
        rejection::Operation::VcsHeadValue,
        rejection::Operation::VcsPathFormatting,
        rejection::Operation::ArchiveDiagnostics,
        rejection::Operation::ArchiveLockfileValue,
    ] {
        assert_eq!(
            rejection::classify(operation).treatment,
            rejection::Treatment::VirtualComputation
        );
    }
    for operation in [
        rejection::Operation::VcsDiscoveryAndManifestGate,
        rejection::Operation::VcsHeadPeeling,
    ] {
        assert_eq!(
            rejection::classify(operation).treatment,
            rejection::Treatment::RetainedForArchiveOccupancy
        );
    }
}

#[test]
fn generated_occupancy_controls_external_metadata_physical_origins() {
    let limits = cargo::Limits::standard();
    for (basename, absolute) in [
        (
            b".cargo_vcs_info.json".as_slice(),
            b"/outside/.cargo_vcs_info.json".as_slice(),
        ),
        (b"Cargo.toml".as_slice(), b"/outside/Cargo.toml".as_slice()),
        (b"Cargo.lock".as_slice(), b"/outside/Cargo.lock".as_slice()),
    ] {
        for occupied in [false, true] {
            for license_field in [false, true] {
                let graph = node::GraphBudget::new();
                let mut allocation = input::Allocation::new(&limits);
                let mut budget = Budget {
                    graph: &graph,
                    allocation: &mut allocation,
                };
                let image = archive_image(
                    &[(absolute, observed::Value::Metadata(metadata()))],
                    &mut budget,
                );
                let mut host = host::Host::from_image(&image, budget).expect("archive host");
                let manifest_selected = [b"/pkg/Cargo.toml".as_slice()];
                let selected: &[&[u8]] = if occupied && basename == b"Cargo.toml" {
                    &manifest_selected
                } else {
                    &[]
                };
                let projection = archive::Projection::from_inputs(
                    archive::Inputs {
                        root: b"/pkg",
                        manifest: b"/pkg/Cargo.toml",
                        selected,
                        license: license_field.then_some(absolute),
                        readme: (!license_field).then_some(absolute),
                        build_scripts: &[],
                        include_lockfile: occupied,
                        vcs: occupied,
                    },
                    &mut host,
                )
                .expect("archive projection includes only unshadowed physical source");
                let external = projection
                    .rows()
                    .iter()
                    .find(|row| row.archive() == basename);
                if occupied {
                    assert!(external
                        .expect("generated occupancy is retained")
                        .origin()
                        .is_none());
                } else {
                    assert_eq!(
                        external.expect("unoccupied basename adds source").origin(),
                        Some(absolute)
                    );
                }
                host.finish()
                    .expect("external metadata source was always probed");
            }
        }
    }
}

#[test]
fn generated_vcs_occupancy_also_shadows_an_excluded_root_metadata_file() {
    let limits = cargo::Limits::standard();
    for vcs in [false, true] {
        for license_field in [false, true] {
            let graph = node::GraphBudget::new();
            let mut allocation = input::Allocation::new(&limits);
            let mut budget = Budget {
                graph: &graph,
                allocation: &mut allocation,
            };
            let image = archive_image(
                &[(
                    b"/pkg/.cargo_vcs_info.json",
                    observed::Value::Metadata(metadata()),
                )],
                &mut budget,
            );
            let mut host = host::Host::from_image(&image, budget).expect("archive host");
            let projection = archive::Projection::from_inputs(
                archive::Inputs {
                    root: b"/pkg",
                    manifest: b"/pkg/Cargo.toml",
                    selected: &[b"/pkg/Cargo.toml"],
                    license: license_field.then_some(b".cargo_vcs_info.json"),
                    readme: (!license_field).then_some(b".cargo_vcs_info.json"),
                    build_scripts: &[],
                    include_lockfile: true,
                    vcs,
                },
                &mut host,
            )
            .expect("metadata addition has its own pinned collision branch");
            let row = projection
                .rows()
                .iter()
                .find(|row| row.archive() == b".cargo_vcs_info.json");
            if vcs {
                assert_eq!(
                    row.expect("generated VCS row is retained").generated(),
                    Some(archive::Generated::VcsInfo)
                );
            } else {
                assert_eq!(
                    row.expect("unoccupied archive row adds physical input")
                        .origin(),
                    Some(b"/pkg/.cargo_vcs_info.json".as_slice())
                );
            }
            host.finish()
                .expect("physical metadata existence always observed");
        }
    }
}
