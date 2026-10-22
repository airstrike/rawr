use std::cell::Cell;

use super::Index;
use crate::cargo::Limits;
use crate::input::stored::{Budget, Error};
use crate::input::{Allocation, Usage};
use crate::node;

fn verify(index: &Index, values: &[usize]) {
    let nodes = index.nodes.as_slice();
    assert_eq!(nodes.len(), values.len());
    for (position, node) in nodes.iter().enumerate() {
        let left = index.height(node.left);
        let right = index.height(node.right);
        assert!(i16::from(left).abs_diff(i16::from(right)) <= 1);
        assert_eq!(node.height, left.max(right) + 1);
        for child in [node.left, node.right].into_iter().flatten() {
            assert_eq!(nodes[child].parent, Some(position));
        }
        if let Some(left) = node.left {
            assert!(values[left] < values[position]);
        }
        if let Some(right) = node.right {
            assert!(values[right] > values[position]);
        }
        if let Some(parent) = node.parent {
            assert!(nodes[parent].left == Some(position) || nodes[parent].right == Some(position));
        } else {
            assert_eq!(index.root, Some(position));
        }
        assert_eq!(
            index
                .locate(values, |value| value.cmp(&values[position]))
                .err(),
            Some(position)
        );
    }
}

#[test]
fn both_single_and_double_rotations_preserve_parent_and_payload_association() {
    for sequence in [[3, 2, 1], [1, 2, 3], [3, 1, 2], [1, 3, 2]] {
        let limits = Limits::standard();
        let mut allocation = Allocation::for_proof(
            Usage {
                items: 7,
                bytes: 31,
            },
            &limits,
        );
        let graph = node::GraphBudget::new();
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let mut index = Index::new();
        let mut values = Vec::new();
        for key in sequence {
            let vacant = index
                .locate(&values, |value: &usize| value.cmp(&key))
                .expect("unique key");
            values.push(key);
            index
                .insert(vacant, &mut budget)
                .expect("admitted index node");
            verify(&index, &values);
        }
        assert_eq!(values[index.root.expect("root exists")], 2);
    }
}

#[test]
fn reverse_maximum_line_inventory_has_logarithmic_height_and_comparisons() {
    let limits = Limits::standard();
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let mut index = Index::new();
    let mut values = Vec::new();
    let comparisons = Cell::new(0_usize);
    let count = 65_535;
    for key in (0..count).rev() {
        let before = comparisons.get();
        let height = usize::from(index.height(index.root));
        let vacant = index
            .locate(&values, |value: &usize| {
                comparisons.set(comparisons.get() + 1);
                value.cmp(&key)
            })
            .expect("reverse key remains unique");
        assert!(comparisons.get() - before <= height);
        values.push(key);
        index
            .insert(vacant, &mut budget)
            .expect("complete line inventory fits existing accounts");
        assert!(index.height(index.root) <= 32);
    }
    assert!(comparisons.get() <= count * 32);
    verify(&index, &values);
    let before = allocation.usage();
    assert!(index.locate(&values, |value| value.cmp(&1)).is_err());
    assert_eq!(allocation.usage(), before);
}

#[test]
fn index_growth_exceedance_is_prospective_cumulative_and_refusal_terminal() {
    let prior = Usage {
        items: 7,
        bytes: 31,
    };
    let mut limits = Limits::standard();
    limits.input_proof_bytes = prior.bytes + 3 * super::node_bytes();
    let mut allocation = Allocation::for_proof(prior, &limits);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let mut index = Index::new();
    let mut values = Vec::new();
    for key in [3, 2] {
        let vacant = index
            .locate(&values, |value: &usize| value.cmp(&key))
            .expect("unique");
        values.push(key);
        index
            .insert(vacant, &mut budget)
            .expect("one then two full node layouts");
    }
    assert!(!budget.exceedances().is_exceeded());
    // The third insertion admits the whole four-node replacement layout
    // prospectively, so the cumulative charge jumps from three to seven node
    // layouts in one step; the target is crossed, recorded with that cumulative
    // figure, and the insertion still completes.
    let vacant = index
        .locate(&values, |value| value.cmp(&1))
        .expect("unique third key");
    values.push(1);
    index
        .insert(vacant, &mut budget)
        .expect("accounting targets never refuse index growth");
    assert_eq!(index.nodes.as_slice().len(), 3);
    let record = budget.exceedances();
    let bytes = record
        .axis(crate::input::INPUT_BYTES_RESOURCE)
        .expect("the replacement layout crossed the byte target");
    assert_eq!(bytes.target, (prior.bytes + 3 * super::node_bytes()) as u64);
    assert_eq!(
        bytes.first_requested,
        (prior.bytes + 7 * super::node_bytes()) as u64
    );
    assert_eq!(bytes.high_water, bytes.first_requested);
    // Only a genuine failure latches, and the latch is terminal: a further
    // insertion does no work and the tree is not mutated.
    budget.refuse();
    let vacant = index
        .locate(&values, |value| value.cmp(&0))
        .expect("unique fourth key");
    assert!(matches!(
        index.insert(vacant, &mut budget),
        Err(Error::Refused)
    ));
    assert_eq!(index.nodes.as_slice().len(), 3);
    assert_eq!(
        allocation.usage().bytes,
        prior.bytes + 7 * super::node_bytes()
    );
}
