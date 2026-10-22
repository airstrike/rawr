use std::cell::Cell;
use std::fmt;

use super::{boxed, format, replace};
use crate::cargo::Limits;
use crate::input::stored::{Budget, Error};
use crate::input::{Allocation, Usage};
use crate::node;

const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

#[test]
fn replacement_matches_std_for_substrings_unicode_and_empty_patterns() {
    for value in [
        "",
        "a",
        "aaaaa",
        "ababa",
        "é🙂é",
        "e\u{301}",
        "a\0a",
        "/root/root",
    ] {
        for from in [
            "", "a", "aa", "aba", "é", "🙂", "\u{301}", "\0", "/root", "absent",
        ] {
            for to in ["", "-", "é🙂", "aa", "$PACKAGE/token"] {
                let limits = Limits::standard();
                let mut allocation = Allocation::for_proof(PRIOR, &limits);
                let graph = node::GraphBudget::new();
                let actual = replace(
                    value,
                    from,
                    to,
                    &mut Budget::borrow(&limits, &mut allocation, &graph),
                )
                .expect("admitted substring replacement");
                assert_eq!(
                    actual,
                    value.replace(from, to),
                    "{value:?}, {from:?}, {to:?}"
                );
                assert_eq!(
                    allocation.usage(),
                    Usage {
                        items: PRIOR.items + 1,
                        bytes: PRIOR.bytes + actual.len(),
                    }
                );
            }
        }
    }
}

#[test]
fn formatting_matches_std_without_imposing_a_scalar_limit() {
    let mut limits = Limits::standard();
    limits.string_bytes = 1;
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    let value = format(
        format_args!("$PACKAGE/{:08x}/{:>7}/{}", 0x1234_u32, "é", "\0🙂"),
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("formatted paths are not bounded scalar strings");
    assert_eq!(
        value,
        format!("$PACKAGE/{:08x}/{:>7}/{}", 0x1234_u32, "é", "\0🙂")
    );
    let replaced = replace(
        &value,
        "$PACKAGE",
        "$OUT",
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("replacement does not impose a scalar string cap");
    assert_eq!(replaced, value.replace("$PACKAGE", "$OUT"));
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 2,
            bytes: PRIOR.bytes + value.len() + replaced.len(),
        }
    );
}

#[test]
fn expansion_uses_original_exact_floors_without_graph_decode_and_preserves_refusal() {
    let value = "ééé";
    let to = "$PACKAGE/longer-token";
    let bytes = value.replace("", to).len();
    // The expanded layout is owned once by the original allocation.
    for (item_floor, byte_floor) in [(0, 0), (1, 0), (0, 1)] {
        let mut limits = Limits::standard();
        limits.items = PRIOR.items + 1 - item_floor;
        limits.input_proof_bytes = PRIOR.bytes + bytes - byte_floor;
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("all decoding already spent");
        graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .expect("all hashing already spent");
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("all filesystem work already spent");
        graph.exhaust_comparisons();
        let mut budget = Budget::borrow(&Limits::standard(), &mut allocation, &graph);
        let result = replace(value, "", to, &mut budget);
        // Every floor still expands; the layout is one item of exactly
        // `bytes` on the original account and every graph axis, filled exactly
        // to its target beforehand, records nothing.
        assert_eq!(
            result.expect("accounting targets are recorded, never refused"),
            value.replace("", to)
        );
        assert_eq!(
            graph.census_usage().expect("graph census").0,
            node::MAX_DECODED_BYTES_PER_GRAPH,
            "the output layout is owned by the original account alone"
        );
        assert!(!graph.exceedances().is_exceeded());
        let record = budget.exceedances();
        if item_floor == 0 && byte_floor == 0 {
            assert!(!record.is_exceeded());
            assert_eq!(
                allocation.usage(),
                Usage {
                    items: PRIOR.items + 1,
                    bytes: PRIOR.bytes + bytes,
                }
            );
        } else {
            // One under records that axis alone with a high water equal to
            // the exact layout charge, and nothing latches.
            let (exceeded, untouched, total) = if item_floor == 1 {
                (
                    crate::input::INPUT_ITEMS_RESOURCE,
                    crate::input::INPUT_BYTES_RESOURCE,
                    PRIOR.items + 1,
                )
            } else {
                (
                    crate::input::INPUT_BYTES_RESOURCE,
                    crate::input::INPUT_ITEMS_RESOURCE,
                    PRIOR.bytes + bytes,
                )
            };
            let axis = *record
                .axis(exceeded)
                .expect("the one-under axis records the exact layout charge");
            assert_eq!(
                (axis.target, axis.high_water),
                ((total - 1) as u64, total as u64)
            );
            assert!(record.axis(untouched).is_none());
            budget
                .check()
                .expect("an accounting exceedance never latches the account");
            // A genuine failure latches the original account: no further
            // replacement or formatting work runs, no formatting callback is
            // visited, and the latch survives a fresh graph argument.
            budget.refuse();
            assert!(matches!(
                replace("", "", "", &mut budget),
                Err(Error::Refused)
            ));
            let visits = Cell::new(0);
            assert!(matches!(
                format(format_args!("{}", Counted(&visits)), &mut budget),
                Err(Error::Refused)
            ));
            assert_eq!(visits.get(), 0, "refusal precedes formatting callbacks");
            assert_eq!(
                allocation.usage(),
                Usage {
                    items: PRIOR.items + 1,
                    bytes: PRIOR.bytes + bytes,
                }
            );
            let fresh_graph = node::GraphBudget::new();
            assert!(matches!(
                replace(
                    "x",
                    "x",
                    "",
                    &mut Budget::borrow(&Limits::standard(), &mut allocation, &fresh_graph,)
                ),
                Err(Error::Refused)
            ));
        }
    }
}

struct Counted<'a>(&'a Cell<usize>);

impl fmt::Display for Counted<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.set(self.0.get() + 1);
        formatter.write_str("é-value")
    }
}

#[test]
fn formatting_refusal_precedes_the_render_pass() {
    let bytes = "é-value".len();
    for (item_floor, byte_floor) in [(0, 0), (1, 0), (0, 1)] {
        let mut limits = Limits::standard();
        limits.items = PRIOR.items + 1 - item_floor;
        limits.input_proof_bytes = PRIOR.bytes + bytes - byte_floor;
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("all decoding already spent");
        let visits = Cell::new(0);
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let result = format(format_args!("{}", Counted(&visits)), &mut budget);
        // Every floor still formats through both the count and render
        // passes; one under records that axis with the exact charge.
        assert_eq!(
            result.expect("accounting targets are recorded, never refused"),
            "é-value"
        );
        assert_eq!(visits.get(), 2);
        let record = budget.exceedances();
        if item_floor == 0 && byte_floor == 0 {
            assert!(!record.is_exceeded());
        } else {
            let (exceeded, total) = if item_floor == 1 {
                (crate::input::INPUT_ITEMS_RESOURCE, PRIOR.items + 1)
            } else {
                (crate::input::INPUT_BYTES_RESOURCE, PRIOR.bytes + bytes)
            };
            let axis = *record
                .axis(exceeded)
                .expect("the one-under axis records the exact format charge");
            assert_eq!(
                (axis.target, axis.high_water),
                ((total - 1) as u64, total as u64)
            );
            budget
                .check()
                .expect("an accounting exceedance never latches the account");
            // A genuine failure latches the account: the refusal precedes both
            // the count and the render pass, so no callback is visited.
            budget.refuse();
            assert!(matches!(
                format(format_args!("{}", Counted(&visits)), &mut budget),
                Err(Error::Refused)
            ));
            assert_eq!(visits.get(), 2, "no count or render after refusal");
            assert!(matches!(budget.check(), Err(Error::Refused)));
        }
    }
}

#[test]
fn changing_formatter_cannot_spend_allocator_bonus_or_grow_after_admission() {
    struct Changing(Cell<usize>);
    impl fmt::Display for Changing {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            let prior = self.0.replace(self.0.get() + 1);
            formatter.write_str(if prior == 0 { "x" } else { "xy" })
        }
    }
    let limits = Limits::standard();
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    assert!(matches!(
        format(format_args!("{}", Changing(Cell::new(0))), &mut budget),
        Err(Error::Overflow)
    ));
    assert!(matches!(
        boxed(String::new(), &mut budget),
        Err(Error::Refused)
    ));
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 1,
            bytes: PRIOR.bytes + 1,
        }
    );
}

#[test]
fn boxing_moves_exact_storage_without_another_charge() {
    let value: Box<str> = "é-owned".into();
    let value = value.into_string();
    assert_eq!(value.capacity(), value.len());
    let pointer = value.as_ptr();
    let bytes = value.len();
    let mut limits = Limits::standard();
    limits.items = PRIOR.items + 1;
    limits.input_proof_bytes = PRIOR.bytes + bytes;
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize - bytes)
        .expect("prior graph storage");
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    budget
        .storage(1, bytes)
        .expect("original string already admitted");
    let value = boxed(value, &mut budget).expect("ownership moves at the exact cap");
    assert_eq!(value.as_ptr(), pointer);
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 1,
            bytes: PRIOR.bytes + bytes,
        }
    );
}

#[test]
fn boxing_shrink_layout_has_exact_byte_floors_without_another_item() {
    for byte_floor in [0, 1] {
        let mut value = String::with_capacity(64);
        value.push('é');
        let capacity = value.capacity();
        let bytes = value.len();
        let mut limits = Limits::standard();
        limits.items = PRIOR.items + 1;
        limits.input_proof_bytes = PRIOR.bytes + capacity + bytes - byte_floor;
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("all decoding already spent");
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        budget.storage(1, capacity).expect("original string layout");
        let result = boxed(value, &mut budget);
        // The shrink layout is exactly `bytes` more on the byte axis and
        // never another item; one byte under records the byte axis alone.
        assert_eq!(
            result
                .expect("accounting targets are recorded, never refused")
                .as_ref(),
            "é"
        );
        let record = budget.exceedances();
        assert!(record.axis(crate::input::INPUT_ITEMS_RESOURCE).is_none());
        if byte_floor == 0 {
            assert!(!record.is_exceeded());
        } else {
            let axis = *record
                .axis(crate::input::INPUT_BYTES_RESOURCE)
                .expect("the one-under byte axis records the exact shrink layout");
            assert_eq!(
                (axis.target, axis.high_water),
                (
                    (PRIOR.bytes + capacity + bytes - 1) as u64,
                    (PRIOR.bytes + capacity + bytes) as u64
                )
            );
            budget
                .check()
                .expect("an accounting exceedance never latches the account");
            // A genuine failure latches the account; a latched account moves
            // no further ownership.
            budget.refuse();
            assert!(matches!(
                boxed(String::new(), &mut budget),
                Err(Error::Refused)
            ));
        }
        assert_eq!(
            allocation.usage(),
            Usage {
                items: PRIOR.items + 1,
                bytes: PRIOR.bytes + capacity + bytes,
            }
        );
    }
}

#[test]
fn checked_counter_overflow_and_format_errors_are_distinct() {
    let mut counter = super::Counter {
        bytes: usize::MAX,
        overflow: false,
    };
    assert!(fmt::Write::write_str(&mut counter, "x").is_err());
    assert!(counter.overflow);

    struct Failing;
    impl fmt::Display for Failing {
        fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            Err(fmt::Error)
        }
    }
    let limits = Limits::standard();
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    assert!(matches!(
        format(format_args!("{Failing}"), &mut budget),
        Err(Error::Encoding("protected text cannot be formatted"))
    ));
    budget
        .check()
        .expect("semantic formatter failure is not resource refusal");
    assert_eq!(allocation.usage(), PRIOR);
}
