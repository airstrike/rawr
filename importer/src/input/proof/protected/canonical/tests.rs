use super::*;
use crate::cargo::Limits;
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
fn canonical_output_spends_exact_storage_and_hash_work() {
    let entries = entries();
    let bytes = serde_json::to_vec(&(crate::cargo::universe::BUILD_PROJECTION_SCHEMA, &entries))
        .expect("canonical")
        .len()
        + 1;
    let limits = Limits::standard();
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let result = current(&entries, &mut budget);
    // The canonical output is one item of exactly `bytes` on the original
    // account and `hashed` on the hash axis, and it is never a decode.
    assert_eq!(result.expect("canonical output").bytes().len(), bytes);
    budget.check().expect("the account is not latched");
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 1,
            bytes: PRIOR.bytes + bytes,
        }
    );
    // A genuine failure latches the account; a latched account does no
    // further canonical work.
    allocation.refuse();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    assert!(matches!(current(&[], &mut budget), Err(Error::Refused)));
}

#[test]
fn canonical_output_counts_the_trailing_newline() {
    let limits = Limits::standard();
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let output = current(&[], &mut budget).expect("canonical output");
    assert_eq!(output.bytes(), b"[4,[]]\n");
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 1,
            bytes: PRIOR.bytes + 7,
        }
    );
}
