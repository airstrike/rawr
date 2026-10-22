use super::*;
use crate::input::proof::Role;
use crate::input::{Allocation, Usage};
use crate::node;

const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

fn entries() -> [ProtectedEntry; 1] {
    [ProtectedEntry::Fact {
        role: Role::DependencySource,
        package: "pkg".into(),
        name: "quote\"\\\n".into(),
        value: "Unicode λ".into(),
    }]
}

#[test]
fn canonical_bytes_and_domain_hash_match_the_existing_projection() {
    let entries = entries();
    let mut expected =
        serde_json::to_vec(&(crate::cargo::universe::BUILD_PROJECTION_SCHEMA, &entries))
            .expect("legacy canonical JSON");
    expected.push(b'\n');
    let expected = BuildProjection::from_canonical(expected.into_boxed_slice());
    let limits = Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let actual = current(
        &entries,
        &limits,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("current canonical output");
    assert_eq!(actual.bytes(), expected.bytes());
    assert_eq!(actual.digest(), expected.digest());
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 1,
            bytes: PRIOR.bytes + actual.bytes().len()
        }
    );
}

#[test]
fn canonical_output_spends_exact_storage_and_hash_floors() {
    let entries = entries();
    let bytes = serde_json::to_vec(&(crate::cargo::universe::BUILD_PROJECTION_SCHEMA, &entries))
        .expect("canonical")
        .len()
        + 1;
    let hashed = BuildProjection::hash_bytes(bytes).expect("bounded hash");
    // The canonical output is owned once by the original allocation and hashed
    // on the hash axis; it is not a decode.
    for (item_floor, byte_floor, hash_floor) in [(0, 0, 0), (1, 0, 0), (0, 1, 0), (0, 0, 1)] {
        let mut limits = Limits::standard();
        limits.items = PRIOR.items + 1 - item_floor;
        limits.input_proof_bytes = PRIOR.bytes + bytes - byte_floor;
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("all decoding already spent");
        graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize - hashed + hash_floor)
            .expect("prior hashed work");
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let result = current(&entries, &limits, &mut budget);
        // Every floor still admits; the canonical output is one item of
        // exactly `bytes` on the original account and `hashed` on the hash
        // axis, and it is never a decode.
        assert_eq!(
            result
                .expect("accounting targets are recorded, never refused")
                .bytes()
                .len(),
            bytes
        );
        assert_eq!(
            graph.census_usage().expect("graph census").0,
            node::MAX_DECODED_BYTES_PER_GRAPH
        );
        assert!(graph.exceedances().axis(node::DECODED_RESOURCE).is_none());
        let record = budget.exceedances();
        budget
            .check()
            .expect("an accounting exceedance never latches the account");
        assert_eq!(
            allocation.usage(),
            Usage {
                items: PRIOR.items + 1,
                bytes: PRIOR.bytes + bytes,
            }
        );
        if (item_floor, byte_floor, hash_floor) == (0, 0, 0) {
            assert!(!record.is_exceeded());
            assert!(!graph.exceedances().is_exceeded());
            graph.hash(1).expect("the hash target never refuses");
            assert_eq!(
                graph
                    .exceedances()
                    .axis(node::HASHED_RESOURCE)
                    .expect("one more hashed byte records the axis")
                    .high_water,
                node::MAX_HASHED_BYTES_PER_GRAPH + 1
            );
        } else {
            // One under on an axis records that axis alone with a high water
            // equal to the exact charge, and nothing latches.
            let graph_record = graph.exceedances();
            if item_floor == 1 {
                let axis = *record
                    .axis(crate::input::INPUT_ITEMS_RESOURCE)
                    .expect("the one-under item axis records");
                assert_eq!(
                    (axis.target, axis.high_water),
                    (PRIOR.items as u64, PRIOR.items as u64 + 1)
                );
            } else {
                assert!(record.axis(crate::input::INPUT_ITEMS_RESOURCE).is_none());
            }
            if byte_floor == 1 {
                let axis = *record
                    .axis(crate::input::INPUT_BYTES_RESOURCE)
                    .expect("the one-under byte axis records");
                assert_eq!(
                    (axis.target, axis.high_water),
                    (
                        (PRIOR.bytes + bytes - 1) as u64,
                        (PRIOR.bytes + bytes) as u64
                    )
                );
            } else {
                assert!(record.axis(crate::input::INPUT_BYTES_RESOURCE).is_none());
            }
            if hash_floor == 1 {
                let axis = *graph_record
                    .axis(node::HASHED_RESOURCE)
                    .expect("the one-under hash axis records");
                assert_eq!(
                    (axis.target, axis.first_requested, axis.high_water),
                    (
                        node::MAX_HASHED_BYTES_PER_GRAPH,
                        node::MAX_HASHED_BYTES_PER_GRAPH + 1,
                        node::MAX_HASHED_BYTES_PER_GRAPH + 1
                    )
                );
            } else {
                assert!(!graph_record.is_exceeded());
            }
            // A genuine failure latches the account; a latched account does
            // no further canonical work.
            allocation.refuse();
            let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
            assert!(matches!(
                current(&[], &limits, &mut budget),
                Err(Error::Refused)
            ));
        }
    }
}

#[test]
fn canonical_limit_counts_the_trailing_newline_before_ownership() {
    let mut limits = Limits::standard();
    limits.input_proof_bytes = b"[4,[]]".len();
    let original = Limits::standard();
    let mut allocation = Allocation::for_proof(PRIOR, &original);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&original, &mut allocation, &graph);
    // The projection byte target is measured against the canonical
    // length including the trailing newline (7, not 6) and recorded on the
    // original account before ownership proceeds; nothing refuses or latches.
    let output = current(&[], &limits, &mut budget).expect("the target is recorded, not refused");
    assert_eq!(output.bytes(), b"[4,[]]\n");
    let axis = *budget
        .exceedances()
        .axis("protected source/build projection bytes")
        .expect("the canonical length records its axis");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (6, 7, 7)
    );
    budget
        .check()
        .expect("an accounting exceedance never latches the account");
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 1,
            bytes: PRIOR.bytes + 7,
        }
    );
    // The canonical output is owned by the original account and hashed on the
    // hash axis: no decode happened, and filling the decode target exactly
    // records nothing.
    assert_eq!(
        graph.census_usage().expect("graph census"),
        (
            0,
            0,
            BuildProjection::hash_bytes(7).expect("bounded hash") as u64
        )
    );
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .expect("no storage work");
    assert!(!graph.exceedances().is_exceeded());
}

#[test]
fn wider_call_limits_cannot_bypass_the_borrowed_budget_ceiling() {
    let original = Limits::standard();
    let mut borrowed = original;
    borrowed.input_proof_bytes = 6;
    let mut allocation = Allocation::for_proof(PRIOR, &original);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&borrowed, &mut allocation, &graph);
    // The wider call limits do not widen the borrowed ceiling: the
    // recorded target is the borrowed 6, requested 7, on the original account.
    current(&[], &original, &mut budget).expect("the borrowed target is recorded, not refused");
    let axis = *budget
        .exceedances()
        .axis("protected source/build projection bytes")
        .expect("the borrowed ceiling records its axis");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (6, 7, 7)
    );
    // A second pass still works and keeps the truthful high water.
    current(&[], &original, &mut budget).expect("an exceedance never latches the account");
    assert_eq!(
        budget
            .exceedances()
            .axis("protected source/build projection bytes")
            .expect("the axis persists")
            .high_water,
        7
    );
    // A genuine failure latches the account; a latched account does no
    // further canonical work.
    budget.refuse();
    assert!(matches!(
        current(&[], &original, &mut budget),
        Err(Error::Refused)
    ));
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 2,
            bytes: PRIOR.bytes + 14,
        }
    );
    assert_eq!(graph.census_usage().expect("graph census").0, 0);
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .expect("no allocation");
    assert!(graph.exceedances().axis(node::DECODED_RESOURCE).is_none());
}
