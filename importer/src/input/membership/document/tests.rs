use super::{parse, Budget, Error};
use crate::input::membership::{host, observed, stored};
use crate::{cargo, input, node};

const PATH: &[u8] = b"/raw";

fn encoded(bytes: &[u8]) -> Vec<u8> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let record = observed::Record {
        query: observed::Query::File(observed::Path::from_bytes(PATH, &mut budget).unwrap()),
        value: observed::Value::File {
            bytes: bytes.to_vec(),
            end: observed::End::Eof,
        },
    };
    stored::Stored::from_records(&[record], &mut budget)
        .unwrap()
        .encode(&mut budget)
        .unwrap()
}

fn image(bytes: &[u8]) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    stored::Stored::decode(
        &encoded(bytes),
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap()
}

fn from_image<'image>(
    image: &'image stored::Stored,
    limits: &cargo::Limits,
    budget: Budget<'_>,
) -> super::Result<rawr_toml::document::Document<'image>> {
    let mut host = host::Host::from_image(image, budget)?;
    let file = host.file(PATH)?.expect("complete-file fixture");
    let result = parse(file, limits, host.budget());
    host.finish().unwrap();
    result
}

#[test]
fn complete_document_borrows_the_decoded_image_and_retains_unknown_tables() {
    let bytes = b"[package]\nname = 'demo'\nversion = '1.2.3'\n[unknown]\nx = [1, 2]\n";
    let image = image(bytes);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let document = from_image(
        &image,
        &limits,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let observed::Value::File { bytes, .. } = &image.records()[0].value else {
        panic!("complete-file fixture");
    };
    assert!(std::ptr::eq(document.source().as_bytes(), bytes.as_slice()));
    assert_eq!(document.table().len(), 2);
    let rawr_toml::document::Value::Table(package) =
        document.table().get("package").unwrap().value()
    else {
        panic!("package table");
    };
    let rawr_toml::document::Value::String(name) = package.get("name").unwrap().value() else {
        panic!("package name");
    };
    assert_eq!(name.as_str(), "demo");
    assert!(document.table().get("unknown").is_some());
}

#[test]
fn document_rejects_invalid_utf8_and_trailing_syntax_after_successful_eof() {
    for (bytes, utf8) in [
        (b"a = 1\n\xff".as_slice(), true),
        (b"a = 1\n[broken", false),
        (b"a = 1\n[unknown]\nx = [", false),
    ] {
        let image = image(bytes);
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let result = from_image(
            &image,
            &limits,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        );
        if utf8 {
            assert!(matches!(result, Err(Error::ManifestEncoding(_))));
        } else {
            assert!(matches!(result, Err(Error::Document(_))));
        }
    }
}

/// The manifest byte target is recorded before UTF-8 or parser work and
/// never refuses. At the exact size nothing is recorded; one byte under, the
/// same document is parsed (or rejected for its own encoding fault), the axis
/// records the truthful size, the record charges nothing and latches nothing.
#[test]
fn manifest_byte_target_is_recorded_before_utf8_or_parser_work() {
    for bytes in [b"a = 1\n".as_slice(), b"a = 1\xff"] {
        let image = image(bytes);
        for floor in [false, true] {
            let mut limits = cargo::Limits::standard();
            limits.manifest_bytes = bytes.len() - usize::from(floor);
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
            let file = host.file(PATH).unwrap().unwrap();
            let before = host.budget().allocation.usage();
            let result = parse(file, &limits, host.budget());
            if bytes.last() == Some(&0xff) {
                assert!(matches!(result, Err(Error::ManifestEncoding(_))));
                assert_eq!(host.budget().allocation.usage(), before);
            } else {
                result.unwrap();
            }
            let record = host.budget().allocation.exceedances();
            if floor {
                let axis = record
                    .axis("membership manifest bytes")
                    .expect("one byte under the manifest size records its axis");
                assert_eq!(
                    (axis.target, axis.first_requested, axis.high_water),
                    (
                        (bytes.len() - 1) as u64,
                        bytes.len() as u64,
                        bytes.len() as u64
                    )
                );
                assert!(
                    host.budget().allocation.items(0).is_ok(),
                    "an exceedance never latches"
                );
            } else {
                assert!(!record.is_exceeded());
            }
            host.finish().unwrap();
        }
    }
}

#[test]
fn decoding_query_and_document_share_prior_ownership_without_fake_graph_work() {
    let bytes = encoded(b"name = \"ab\\u0063d\"\na = [1, { b = true }]\n");
    let limits = cargo::Limits::standard();
    let run = |allocation: &mut input::Allocation| {
        let graph = node::GraphBudget::new();
        graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH - 1).unwrap();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize - bytes.len())
            .unwrap();
        let mut budget = Budget {
            graph: &graph,
            allocation,
        };
        let image = stored::Stored::decode(&bytes, &mut budget)?;
        let document = from_image(&image, &limits, budget)?;
        assert!(document.table().get("name").is_some());
        // Exactly one path check and exactly the image's bytes were
        // charged (no exceedance); one more of either crosses the target and is
        // recorded rather than refused.
        assert!(!graph.exceedances().is_exceeded());
        graph.paths(1).unwrap();
        graph.decode(1).unwrap();
        let exceeded = graph.exceedances();
        assert_eq!(
            exceeded.axis(node::PATHS_RESOURCE).unwrap().high_water,
            node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
        );
        assert_eq!(
            exceeded.axis(node::DECODED_RESOURCE).unwrap().high_water,
            node::MAX_DECODED_BYTES_PER_GRAPH + 1
        );
        Ok::<_, Error>(())
    };
    let mut baseline = input::Allocation::new(&limits);
    baseline.items(7).unwrap();
    baseline.bytes(19).unwrap();
    run(&mut baseline).unwrap();
    let total = baseline.usage();
    assert!(!baseline.exceedances().is_exceeded());
    for floor in 0..3 {
        let mut allocation = input::Allocation::with_limits(
            total.items - usize::from(floor == 1),
            total.bytes - usize::from(floor == 2),
            limits.string_bytes,
        );
        allocation.items(7).unwrap();
        allocation.bytes(19).unwrap();
        run(&mut allocation).unwrap();
        assert_eq!(allocation.usage(), total);
        let exceeded = allocation.exceedances();
        match floor {
            0 => assert!(!exceeded.is_exceeded()),
            1 => {
                let axis = exceeded.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
                assert_eq!(axis.target, total.items as u64 - 1);
                assert_eq!(axis.high_water, total.items as u64);
                assert!(exceeded.axis(input::INPUT_BYTES_RESOURCE).is_none());
            }
            _ => {
                let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
                assert_eq!(axis.target, total.bytes as u64 - 1);
                assert_eq!(axis.high_water, total.bytes as u64);
                assert!(exceeded.axis(input::INPUT_ITEMS_RESOURCE).is_none());
            }
        }
    }
}

/// The decoded string target reaches the parser as a recorded accounting axis:
/// the exact decoded length records nothing, one byte over is parsed
/// and recorded with the truthful decoded length. The caller's `depth` bounds
/// parser recursion and stays a physical refusal.
#[test]
fn decoded_string_target_records_and_caller_depth_limit_refuses() {
    for (bytes, string_limit, depth, accepted, recorded) in [
        (b"key = \"abc\\u0064\"\n".as_slice(), 4, 80, true, None),
        (b"key = \"abc\\u0064e\"\n", 4, 80, true, Some((4, 5))),
        (b"key = [[1]]\n", 4, 0, false, None),
        (b"key = [[1]]\n", 4, 80, true, None),
    ] {
        let image = image(bytes);
        let mut limits = cargo::Limits::standard();
        limits.string_bytes = string_limit;
        limits.depth = depth;
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let result = from_image(
            &image,
            &limits,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        );
        assert_eq!(result.is_ok(), accepted, "{result:?}");
        let axis = allocation
            .exceedances()
            .axis("membership decoded TOML string bytes")
            .map(|axis| (axis.target, axis.high_water));
        assert_eq!(axis, recorded, "{bytes:?}");
        assert!(allocation.items(0).is_ok(), "an exceedance never latches");
    }
}
