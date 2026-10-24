use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;

use super::{Record, MAGIC, SECTIONS};
use crate::cargo::{self, toolchain};
use crate::input::stored::{self, codec, Budget, Error, Stored};
use crate::input::{build, Allocation, Usage};
use crate::node;

const METADATA: &[u8] = b"{\"packages\":[]}\n";
const UNITS: &[u8] = b"{\"units\":[]}\n";
const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

fn fixture() -> (build::Retained, Stored) {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let (input, _) = Stored::capture(stored::tests::baseline(), &limits, &graph).into_parts();
    let build = build::Retained {
        build: build::Build {
            evidence: build::Evidence {
                usage: Usage {
                    items: 19,
                    bytes: 71,
                },
                packages: Default::default(),
                scripts: Box::default(),
                artifacts: Box::default(),
                environment: Box::from([(
                    OsString::from_vec(b"NAME\xff".to_vec()),
                    OsString::from_vec(b"value\xfe".to_vec()),
                )]),
            },
            package_files: BTreeMap::new(),
        },
    };
    (build, input.expect("historical input captures"))
}

fn encoded(build: &build::Retained, input: &Stored) -> Vec<u8> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut account = Allocation::for_proof(PRIOR, &limits);
    Record::encode(
        METADATA,
        UNITS,
        build,
        input,
        &mut Budget::borrow(&limits, &mut account, &graph),
    )
    .expect("complete record encodes")
}

#[test]
fn one_output_allocation_preserves_nested_codecs_and_decode_only_borrows() {
    let (build, input) = fixture();
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut account = Allocation::for_proof(PRIOR, &limits);
    let mut budget = Budget::borrow(&limits, &mut account, &graph);
    let bytes = Record::encode(METADATA, UNITS, &build, &input, &mut budget).expect("aggregate");
    assert_eq!(budget.allocations(), 1);
    assert_eq!(
        budget.usage(),
        Usage {
            items: PRIOR.items + 1,
            bytes: PRIOR.bytes + bytes.len()
        }
    );
    let before = budget.usage();
    let record = Record::decode(&bytes, &mut budget).expect("borrowed framing");
    assert_eq!(budget.usage(), before);
    assert_eq!(budget.allocations(), 1);
    assert_eq!(record.metadata(), METADATA);
    assert_eq!(record.units(), UNITS);
    for section in [
        record.metadata(),
        record.units(),
        record.build(),
        record.input(),
    ] {
        let offset = section.as_ptr() as usize - bytes.as_ptr() as usize;
        assert!(offset + section.len() <= bytes.len());
        assert_eq!(&bytes[offset..offset + section.len()], section);
    }
    assert_eq!(
        record.build(),
        build.encode(&mut budget).expect("existing Build codec")
    );
    assert_eq!(
        record.input(),
        input.encode(&mut budget).expect("existing input codec")
    );
    let decoded = Stored::decode(record.input(), &mut budget).expect("typed input still decodes");
    assert_eq!(decoded.fingerprint(), input.fingerprint());
    assert_eq!(
        decoded.encode(&mut budget).expect("canonical input"),
        record.input()
    );
}

#[test]
fn framing_rejects_pin_version_count_lengths_truncation_and_trailing_bytes() {
    let (build, input) = fixture();
    let bytes = encoded(&build, &input);
    let reject = |bytes: &[u8]| {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut account = Allocation::for_proof(PRIOR, &limits);
        let mut budget = Budget::borrow(&limits, &mut account, &graph);
        assert!(Record::decode(bytes, &mut budget).is_err());
        assert_eq!(budget.allocations(), 0);
        assert_eq!(budget.usage(), PRIOR);
    };
    for end in 0..bytes.len() {
        reject(&bytes[..end]);
    }
    for offset in [0, MAGIC.len() - 1, MAGIC.len() + 8] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        reject(&changed);
    }
    let count = MAGIC.len() + 8 + toolchain::supported_cargo_commit().len();
    for value in [0, SECTIONS - 1, SECTIONS + 1] {
        let mut changed = bytes.clone();
        changed[count..count + 4].copy_from_slice(&value.to_le_bytes());
        reject(&changed);
    }
    let mut changed = bytes.clone();
    changed[count + 4..count + 12].copy_from_slice(&u64::MAX.to_le_bytes());
    reject(&changed);
    let mut changed = bytes;
    changed.push(0);
    reject(&changed);
}

#[test]
fn original_nonzero_account_admits_exact_output_and_records_one_under() {
    let (build, input) = fixture();
    let length = encoded(&build, &input).len();
    for (item_less, byte_less) in [(0, 0), (1, 0), (0, 1)] {
        let mut limits = cargo::Limits::standard();
        limits.items = PRIOR.items + 1 - item_less;
        limits.input_proof_bytes = PRIOR.bytes + length - byte_less;
        let mut account = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        let mut budget = Budget::borrow(&limits, &mut account, &graph);
        let result = Record::encode(METADATA, UNITS, &build, &input, &mut budget);
        // The exact output is owned either way; a ceiling one under
        // records the axis with the exact total as its high water.
        assert_eq!(
            result.expect("original output is owned, not gated").len(),
            length
        );
        assert_eq!(budget.allocations(), 1);
        assert_eq!(
            budget.usage(),
            Usage {
                items: PRIOR.items + 1,
                bytes: PRIOR.bytes + length
            }
        );
        let record = budget.exceedances();
        if item_less == 0 && byte_less == 0 {
            assert!(!record.is_exceeded(), "exact original allowance");
        } else {
            let (name, total) = if item_less == 1 {
                (crate::input::INPUT_ITEMS_RESOURCE, PRIOR.items + 1)
            } else {
                (crate::input::INPUT_BYTES_RESOURCE, PRIOR.bytes + length)
            };
            let axis = record.axis(name).expect("one-under axis recorded");
            assert_eq!(axis.target as usize, total - 1);
            assert_eq!(axis.high_water as usize, total);
            assert!(budget.check().is_ok(), "an exceedance never latches");
            // A genuine failure latches; a latched account encodes nothing.
            budget.refuse();
            assert!(matches!(
                Record::encode(METADATA, UNITS, &build, &input, &mut budget),
                Err(Error::Refused)
            ));
            assert_eq!(budget.allocations(), 1);
        }
    }
}

#[test]
fn nested_sections_do_not_renew_the_outer_cap_or_decoder_graph_account() {
    let mut limits = cargo::Limits::standard();
    limits.input_proof_bytes = 40;
    let mut account = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut account, &graph);
    // The encoded length is measured against the `input_proof_bytes`
    // target once, including both nested sections, and only recorded; the
    // physical encoder bound is the node content ceiling.
    let nested = codec::encode(&mut budget, |writer| {
        writer.section(|writer| writer.raw(&[1; 16]))?;
        writer.section(|writer| writer.raw(&[2; 16]))
    })
    .expect("the encoded-bytes target records and admits");
    assert!(nested.len() > 40);
    assert_eq!(budget.allocations(), 1);
    assert!(budget.check().is_ok(), "an exceedance never latches");
    let axis = *account
        .exceedances()
        .axis("encoded stored baseline bytes")
        .expect("the nested sections cross the outer target once");
    assert_eq!(axis.target, 40);
    assert_eq!(axis.high_water as usize, nested.len());

    let (build, input) = fixture();
    let bytes = encoded(&build, &input);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    graph
        .decode(
            usize::try_from(node::MAX_DECODED_BYTES_PER_GRAPH).expect("addressable cap")
                - bytes.len(),
        )
        .expect("prior graph work");
    let mut account = Allocation::for_proof(PRIOR, &limits);
    let mut budget = Budget::borrow(&limits, &mut account, &graph);
    Record::decode(&bytes, &mut budget).expect("exact remaining graph allowance");
    assert!(!graph.exceedances().is_exceeded(), "exact decode floor");
    // A repeated decode is charged to the graph again and recorded; the
    // nested sections inside the record add no decode of their own.
    Record::decode(&bytes, &mut budget).expect("decode past the target is recorded, not gated");
    let decoded = graph.census_usage().expect("graph census").0;
    assert_eq!(
        decoded,
        node::MAX_DECODED_BYTES_PER_GRAPH + bytes.len() as u64
    );
    let record = graph.exceedances();
    let axis = record
        .axis(node::DECODED_RESOURCE)
        .expect("decoded axis recorded");
    assert_eq!(axis.target, node::MAX_DECODED_BYTES_PER_GRAPH);
    assert_eq!(axis.high_water, decoded);
    assert!(budget.check().is_ok(), "an exceedance never latches");
    // A genuine failure latches; a latched account decodes nothing.
    budget.refuse();
    assert!(matches!(
        Record::decode(&bytes, &mut budget),
        Err(Error::Refused)
    ));
    assert_eq!(budget.allocations(), 0);
    assert_eq!(budget.usage(), PRIOR);
}
