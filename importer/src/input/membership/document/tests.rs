use super::{parse, Budget, Error};
use crate::input::membership::{host, observed, stored};
use crate::{cargo, input};

const PATH: &[u8] = b"/raw";

fn encoded(bytes: &[u8]) -> Vec<u8> {
    let limits = cargo::Limits::standard();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
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
    let mut allocation = input::Allocation::new(&limits);
    stored::Stored::decode(
        &encoded(bytes),
        &mut Budget {
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
    let mut allocation = input::Allocation::new(&limits);
    let document = from_image(
        &image,
        &limits,
        Budget {
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
        let mut allocation = input::Allocation::new(&limits);
        let result = from_image(
            &image,
            &limits,
            Budget {
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
            let mut allocation = input::Allocation::new(&limits);
            let mut host = host::Host::from_image(
                &image,
                Budget {
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
            host.finish().unwrap();
        }
    }
}

#[test]
fn decoding_query_and_document_share_prior_ownership_without_fake_graph_work() {
    let bytes = encoded(b"name = \"ab\\u0063d\"\na = [1, { b = true }]\n");
    let limits = cargo::Limits::standard();
    let run = |allocation: &mut input::Allocation| {
        let mut budget = Budget { allocation };
        let image = stored::Stored::decode(&bytes, &mut budget)?;
        let document = from_image(&image, &limits, budget)?;
        assert!(document.table().get("name").is_some());
        Ok::<_, Error>(())
    };
    let mut baseline = input::Allocation::new(&limits);
    baseline.items(7).unwrap();
    baseline.bytes(19).unwrap();
    run(&mut baseline).unwrap();
}

/// The caller's `depth` bounds parser recursion and is a physical refusal.
#[test]
fn caller_depth_limit_refuses() {
    for (depth, accepted) in [(0, false), (80, true)] {
        let image = image(b"key = [[1]]\n");
        let mut limits = cargo::Limits::standard();
        limits.depth = depth;
        let mut allocation = input::Allocation::new(&limits);
        let result = from_image(
            &image,
            &limits,
            Budget {
                allocation: &mut allocation,
            },
        );
        assert_eq!(result.is_ok(), accepted, "{result:?}");
    }
}
